//! 备份还原视图，对应 design.md 视图 6 / architecture.md §11.7。

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::state::AppState;
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(" 💾 备份还原  [{} 份备份] ", state.backup.list.len()),
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(1), // 状态行
        Constraint::Min(6),    // 表格
        Constraint::Length(2), // 选中详情
        Constraint::Min(3),    // 操作输出
    ])
    .split(inner);

    render_status_line(frame, chunks[0], state);
    render_table(frame, chunks[1], state);
    render_detail(frame, chunks[2], state);
    render_output(frame, chunks[3], state);
}

fn render_status_line(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut spans: Vec<Span> = Vec::new();
    if state.backup.refreshing {
        spans.push(Span::styled("扫描中…", Style::default().fg(theme::FG_HINT)));
    } else if let Some(t) = state.backup.last_refresh {
        spans.push(Span::styled(
            format!("最近刷新 {}s 前", t.elapsed().as_secs()),
            Style::default().fg(theme::FG_DIM),
        ));
    } else {
        spans.push(Span::styled(
            "首次进入，扫描中…",
            Style::default().fg(theme::FG_DIM),
        ));
    }
    spans.push(Span::raw("  "));
    if state.run_mode.is_readonly() {
        spans.push(Span::styled(
            "[只读模式] 创建/删除/还原均不可用",
            Style::default().fg(theme::FG_WARN),
        ));
    }
    if let Some(err) = &state.backup.last_error {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("⚠ {}", err),
            Style::default().fg(theme::FG_WARN),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_table(frame: &mut Frame, area: Rect, state: &AppState) {
    if state.backup.list.is_empty() {
        let body = if state.backup.refreshing {
            "扫描中…"
        } else {
            "尚无备份。可通过操作条创建一个新备份。"
        };
        let p = Paragraph::new(vec![Line::from(""), Line::from(body)])
            .style(Style::default().fg(theme::FG_DIM));
        frame.render_widget(p, area);
        return;
    }

    let header_cells = ["", "名称", "大小", "来源", "时间"]
        .iter()
        .map(|h| {
            Cell::from(Span::styled(
                *h,
                Style::default().add_modifier(Modifier::BOLD),
            ))
        })
        .collect::<Vec<_>>();
    let header = Row::new(header_cells).height(1);

    let rows = state.backup.list.iter().enumerate().map(|(i, b)| {
        let selected = i == state.backup.selected;
        let row_style = if selected {
            Style::default()
                .bg(theme::BG_SELECTED)
                .fg(theme::FG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let mark = if b.restorable() { " " } else { "⚠" };
        let mark_style = if b.restorable() {
            Style::default().fg(theme::FG_OK)
        } else {
            Style::default().fg(theme::FG_WARN)
        };

        let size = format_size(b.size);
        let source = b.source_label().to_string();
        let created = b.created_at_label();

        Row::new(vec![
            Cell::from(Span::styled(mark.to_string(), mark_style)),
            Cell::from(b.name.clone()),
            Cell::from(size),
            Cell::from(source),
            Cell::from(short_ts(&created)),
        ])
        .style(row_style)
    });

    let widths = [
        Constraint::Length(3),
        Constraint::Min(20),
        Constraint::Length(8),
        Constraint::Length(8),
        Constraint::Length(20),
    ];
    let table = Table::new(rows, widths).header(header);
    frame.render_widget(table, area);
}

fn render_detail(frame: &mut Frame, area: Rect, state: &AppState) {
    let Some(b) = state.backup.current() else {
        return;
    };
    let manifest_summary = match &b.manifest {
        Some(m) => format!(
            "范围: nginx.conf={}  sites-available={} 个  启用={} 个  hostname={}  ngtool {}",
            if m.scope.nginx_conf { "✓" } else { "✗" },
            m.scope.sites_available.len(),
            m.scope.sites_enabled.len(),
            m.hostname,
            m.ngtool_version
        ),
        None => "（外部备份，无 manifest，仅可查看不可还原）".to_string(),
    };
    let warn = if !b.restorable() {
        " ⚠ 不可还原"
    } else {
        ""
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" 选中: {} ", b.name),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled("▏ ", Style::default().fg(theme::FG_DIM)),
        Span::raw(manifest_summary),
        Span::styled(warn, Style::default().fg(theme::FG_WARN)),
    ]);
    frame.render_widget(
        Paragraph::new(vec![Line::from(""), line]).wrap(Wrap { trim: false }),
        area,
    );
}

fn render_output(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(
            " 操作输出 ",
            Style::default().fg(theme::FG_PATH),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.backup.output.is_empty() {
        let p = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "（暂无输出）",
                Style::default().fg(theme::FG_DIM),
            )),
        ]);
        frame.render_widget(p, inner);
        return;
    }
    let visible = inner.height as usize;
    let total = state.backup.output.len();
    let skip = total.saturating_sub(visible);
    let lines: Vec<Line> = state
        .backup
        .output
        .iter()
        .skip(skip)
        .map(|s| Line::from(s.clone()))
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1}G", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}M", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}K", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

fn short_ts(ts: &str) -> String {
    // 优先取 'T' 之前的日期 + ' ' + 之后的 8 字符（HH:MM:SS）
    if let Some(idx) = ts.find('T') {
        let date = &ts[..idx];
        let time: String = ts.chars().skip(idx + 1).take(8).collect();
        return format!("{} {}", date, time);
    }
    ts.chars().take(19).collect()
}
