use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::state::AppState;
use crate::domain::dashboard::{
    CertSummary, DashboardSnapshot, DiskInfo, MemoryInfo, ProbeOutcome,
};
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        " 📊 仪表盘 ",
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    match &state.dashboard.snapshot {
        None => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                if state.dashboard.refreshing {
                    "采集中…"
                } else {
                    "首次进入，准备采集仪表盘数据"
                },
                Style::default().fg(theme::FG_DIM),
            )));
        }
        Some(snap) => render_snapshot(&mut lines, snap, state.dashboard.refreshing),
    }

    if let Some(t) = state.dashboard.last_refresh {
        lines.push(Line::from(""));
        let elapsed = t.elapsed().as_secs();
        lines.push(Line::from(Span::styled(
            format!("最近一次刷新：{}s 前　[r] 手动刷新", elapsed),
            Style::default().fg(theme::FG_DIM),
        )));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "[r] 手动刷新（30 秒自动刷新）",
            Style::default().fg(theme::FG_DIM),
        )));
    }

    let p = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn render_snapshot(lines: &mut Vec<Line>, snap: &DashboardSnapshot, refreshing: bool) {
    section(lines, "Nginx 状态");
    line_label_outcome(
        lines,
        "Nginx 服务",
        match &snap.nginx_active {
            ProbeOutcome::Ok(true) => ProbeOutcome::Ok::<String>("● 运行中".into()),
            ProbeOutcome::Ok(false) => ProbeOutcome::Ok::<String>("○ 未运行".into()),
            ProbeOutcome::Err(e) => ProbeOutcome::Err::<String>(e.clone()),
            ProbeOutcome::Timeout => ProbeOutcome::Timeout::<String>,
        },
    );
    line_label_outcome(lines, "Nginx 版本", snap.nginx_version.clone());
    line_label_outcome_count(lines, "已启用站点数", &snap.enabled_count, "个");

    blank(lines);
    section(lines, "SSL 证书");
    match &snap.certs {
        ProbeOutcome::Ok(s) => render_cert_summary(lines, s),
        other => render_outcome_warn(lines, other),
    }

    blank(lines);
    section(lines, "系统资源");
    match &snap.disk {
        ProbeOutcome::Ok(d) => render_disk(lines, d),
        other => render_outcome_warn(lines, other),
    }
    match &snap.memory {
        ProbeOutcome::Ok(m) => render_memory(lines, m),
        other => render_outcome_warn(lines, other),
    }

    blank(lines);
    section(lines, "最近错误");
    match &snap.recent_errors {
        ProbeOutcome::Ok(items) if items.is_empty() => {
            lines.push(Line::from(Span::styled(
                "（最近 error.log 为空）",
                Style::default().fg(theme::FG_DIM),
            )));
        }
        ProbeOutcome::Ok(items) => {
            for it in items {
                lines.push(Line::from(Span::styled(
                    truncate_for_display(it, 100),
                    Style::default().fg(theme::FG_NORMAL),
                )));
            }
        }
        other => render_outcome_warn(lines, other),
    }

    if refreshing {
        blank(lines);
        lines.push(Line::from(Span::styled(
            "（正在刷新…）",
            Style::default().fg(theme::FG_HINT),
        )));
    }

    blank(lines);
    lines.push(Line::from(Span::styled(
        format!("本次采集耗时 {} ms", snap.elapsed_ms),
        Style::default().fg(theme::FG_DIM),
    )));
}

fn render_cert_summary(lines: &mut Vec<Line>, s: &CertSummary) {
    if s.total == 0 {
        lines.push(Line::from(Span::styled(
            "（未发现证书）",
            Style::default().fg(theme::FG_DIM),
        )));
        return;
    }
    let mut spans = Vec::new();
    spans.push(Span::raw(format!("● 证书总数：{} ", s.total)));
    if s.ok > 0 {
        spans.push(Span::styled(
            format!("正常 {} ", s.ok),
            Style::default().fg(theme::FG_OK),
        ));
    }
    if s.warning > 0 {
        spans.push(Span::styled(
            format!("即将到期 {} ", s.warning),
            Style::default().fg(theme::FG_WARN),
        ));
    }
    if s.critical > 0 {
        spans.push(Span::styled(
            format!("紧急 {} ", s.critical),
            Style::default().fg(theme::FG_ERR),
        ));
    }
    lines.push(Line::from(spans));
}

fn render_disk(lines: &mut Vec<Line>, d: &DiskInfo) {
    let style = if d.percent >= 90 {
        Style::default().fg(theme::FG_ERR)
    } else if d.percent >= 75 {
        Style::default().fg(theme::FG_WARN)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    lines.push(Line::from(Span::styled(
        format!("● 磁盘    {} / {} ({}%)", d.used, d.total, d.percent),
        style,
    )));
}

fn render_memory(lines: &mut Vec<Line>, m: &MemoryInfo) {
    lines.push(Line::from(format!("● 内存    {} / {}", m.used, m.total)));
}

fn render_outcome_warn<T: std::fmt::Debug>(lines: &mut Vec<Line>, o: &ProbeOutcome<T>) {
    let msg = match o {
        ProbeOutcome::Err(e) => format!("⚠ 采集失败：{}", e),
        ProbeOutcome::Timeout => "⚠ 采集超时".to_string(),
        ProbeOutcome::Ok(_) => return,
    };
    lines.push(Line::from(Span::styled(
        msg,
        Style::default().fg(theme::FG_WARN),
    )));
}

fn line_label_outcome(lines: &mut Vec<Line>, label: &str, outcome: ProbeOutcome<String>) {
    let value_span: Span = match outcome {
        ProbeOutcome::Ok(v) => Span::raw(v),
        ProbeOutcome::Err(e) => {
            Span::styled(format!("⚠ {}", e), Style::default().fg(theme::FG_WARN))
        }
        ProbeOutcome::Timeout => Span::styled("⚠ 超时", Style::default().fg(theme::FG_WARN)),
    };
    lines.push(Line::from(vec![
        Span::raw(format!("● {:<14}", label)),
        value_span,
    ]));
}

fn line_label_outcome_count(
    lines: &mut Vec<Line>,
    label: &str,
    outcome: &ProbeOutcome<usize>,
    suffix: &str,
) {
    let value_span: Span = match outcome {
        ProbeOutcome::Ok(v) => Span::raw(format!("{} {}", v, suffix)),
        ProbeOutcome::Err(e) => {
            Span::styled(format!("⚠ {}", e), Style::default().fg(theme::FG_WARN))
        }
        ProbeOutcome::Timeout => Span::styled("⚠ 超时", Style::default().fg(theme::FG_WARN)),
    };
    lines.push(Line::from(vec![
        Span::raw(format!("● {:<14}", label)),
        value_span,
    ]));
}

fn section(lines: &mut Vec<Line>, title: &str) {
    lines.push(Line::from(Span::styled(
        format!("═══ {} ═══", title),
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD),
    )));
}

fn blank(lines: &mut Vec<Line>) {
    lines.push(Line::from(""));
}

fn truncate_for_display(s: &str, max_chars: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
        w += cw;
    }
    let _ = UnicodeWidthStr::width(out.as_str());
    out
}
