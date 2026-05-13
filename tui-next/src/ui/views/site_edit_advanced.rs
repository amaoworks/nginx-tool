//! 站点高级编辑视图

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::state::AppState;
use crate::template::config_parser::InjectionSlot;
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let edit = &state.site_edit;
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(" 📁 站点管理 ▸ 编辑: {} ▸ 高级 ", edit.site_name),
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "注入槽用于补充托管模板未覆盖的 server/location 配置。",
        Style::default().fg(theme::FG_DIM),
    )));
    if !edit.markers_intact {
        lines.push(Line::from(Span::styled(
            "注入槽标记不完整，建议切到原始模式修复后再回托管编辑。",
            Style::default().fg(theme::FG_WARN),
        )));
    }
    lines.push(Line::from(""));

    let slots = InjectionSlot::ALL;
    let slot_labels: Vec<Span> = slots
        .iter()
        .map(|s| {
            let is_current = *s == edit.current_slot;
            let style = if is_current {
                Style::default()
                    .fg(theme::FG_HINT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::FG_DIM)
            };
            let marker = if is_current { "◉ " } else { "○ " };
            Span::styled(format!("{}{}", marker, s.label()), style)
        })
        .collect();
    lines.push(Line::from(slot_labels));
    lines.push(Line::from(Span::styled(
        format!("说明: {}", edit.current_slot.description()),
        Style::default().fg(theme::FG_DIM),
    )));
    lines.push(Line::from(""));

    let slot_content = edit
        .injection_slots
        .get(&edit.current_slot)
        .cloned()
        .unwrap_or_default();
    lines.push(Line::from(Span::styled(
        format!("{}:", edit.current_slot.label()),
        Style::default().fg(theme::FG_NORMAL),
    )));
    if slot_content.is_empty() {
        lines.push(Line::from(Span::styled(
            "[空]",
            Style::default().fg(theme::FG_DIM),
        )));
    } else {
        for line in slot_content.lines().take(8) {
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(theme::FG_NORMAL),
            )));
        }
    }

    let snippets = crate::template::snippets::get_snippets_for_slot(edit.current_slot);
    if !snippets.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "相关模板:",
            Style::default().fg(theme::FG_NORMAL),
        )));
        for (i, snippet) in snippets.iter().enumerate() {
            let is_selected = i == edit.template_index;
            let style = if is_selected {
                Style::default()
                    .fg(theme::FG_HINT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::FG_NORMAL)
            };
            let prefix = if is_selected { "▸ " } else { "  " };
            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, snippet.name),
                style,
            )));
        }
    }

    if edit.dirty {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " ⚠ 有未保存的修改",
            Style::default().fg(theme::FG_WARN),
        )));
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}
