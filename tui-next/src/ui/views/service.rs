use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::state::{AppState, ServiceButton};
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        " ⚙️  服务控制 ",
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(7), // 状态摘要
        Constraint::Length(4), // 按钮行
        Constraint::Min(0),    // 输出区
    ])
    .split(inner);

    render_summary(frame, chunks[0], state);
    render_buttons(frame, chunks[1], state);
    render_output(frame, chunks[2], state);
}

fn render_summary(frame: &mut Frame, area: Rect, state: &AppState) {
    let active = state
        .dashboard
        .snapshot
        .as_ref()
        .map(|s| match &s.nginx_active {
            crate::domain::dashboard::ProbeOutcome::Ok(true) => "● 运行中",
            crate::domain::dashboard::ProbeOutcome::Ok(false) => "○ 未运行",
            _ => "（状态采集中）",
        })
        .unwrap_or("（状态未采集）");
    let version = state
        .dashboard
        .snapshot
        .as_ref()
        .and_then(|s| match &s.nginx_version {
            crate::domain::dashboard::ProbeOutcome::Ok(v) => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "（版本未知）".into());

    let update_line = state
        .service
        .update_info
        .as_ref()
        .map(|info| {
            if info.has_update {
                format!(
                    "更新:    可更新 {} -> {}",
                    info.current_version, info.latest_version
                )
            } else {
                format!("更新:    已是最新 ({})", info.current_version)
            }
        })
        .unwrap_or_else(|| "更新:    （尚未检查）".into());

    let lines = vec![
        Line::from(Span::styled(
            "═══ Nginx 服务 ═══",
            Style::default()
                .fg(theme::FG_HINT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("状态:    {}", active)),
        Line::from(format!("版本:    {}", version)),
        Line::from(update_line),
        Line::from(format!(
            "依赖:    nginx {}  systemctl {}",
            glyph(state.ctx.deps().nginx),
            glyph(state.ctx.deps().systemctl),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn render_buttons(frame: &mut Frame, area: Rect, state: &AppState) {
    let cols = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
    ])
    .split(area);

    let readonly = state.run_mode.is_readonly();
    for (i, btn) in ServiceButton::ALL.iter().enumerate() {
        let focused = state.service.focused == *btn;
        let busy = state.service.running == Some(*btn);
        let disabled = readonly
            && matches!(btn, ServiceButton::Reload | ServiceButton::Restart);
        let label = if busy {
            format!("[ {}（执行中）]", btn.label())
        } else {
            format!("[ {} ]", btn.label())
        };
        let style = if disabled {
            Style::default()
                .fg(theme::FG_DIM)
                .add_modifier(Modifier::DIM)
        } else if focused {
            Style::default()
                .bg(theme::BG_SELECTED)
                .fg(theme::FG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::FG_NORMAL)
        };
        let surrounded = if focused {
            format!("[{}]", label)
        } else {
            label
        };
        let lines = vec![Line::from(""), Line::from(Span::styled(surrounded, style))];
        frame.render_widget(Paragraph::new(lines), cols[i]);
    }
}

fn render_output(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(
            " 操作输出（[c] 清空） ",
            Style::default().fg(theme::FG_PATH),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.service.output.is_empty() {
        let p = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "（暂无输出。Tab/方向键切换按钮，Enter 执行，Esc 返回侧栏）",
                Style::default().fg(theme::FG_DIM),
            )),
        ]);
        frame.render_widget(p, inner);
        return;
    }

    let lines: Vec<Line> = state
        .service
        .output
        .iter()
        .map(|s| Line::from(s.clone()))
        .collect();
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn glyph(ok: bool) -> &'static str {
    if ok {
        "✓"
    } else {
        "✗"
    }
}
