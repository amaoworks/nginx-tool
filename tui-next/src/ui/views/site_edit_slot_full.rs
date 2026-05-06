//! 注入槽全屏编辑视图，对应 design.md 子模式 C "全屏编辑"，由 Ctrl+E 进入。

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::state::AppState;
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let edit = &state.site_edit;
    let site_name = &edit.site_name;
    let slot = edit.slot_edit_target.unwrap_or(edit.current_slot);

    let title = format!(
        " 📁 站点管理 ▸ 编辑: {} ▸ 注入槽: {} ",
        site_name,
        slot.label()
    );
    let block = Block::default()
        .borders(Borders::NONE)
        .title(Span::styled(title, Style::default().fg(theme::FG_PATH)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // 槽位说明
    lines.push(Line::from(Span::styled(
        format!("说明：{}", slot.description()),
        Style::default().fg(theme::FG_DIM),
    )));
    lines.push(Line::from(Span::styled(
        if edit.dirty {
            " ⚠ 表单有未保存的修改（含槽位）"
        } else {
            ""
        },
        Style::default().fg(theme::FG_WARN),
    )));
    lines.push(Line::from(""));

    // 计算可见区
    let header_rows = lines.len() as u16;
    let visible_rows = inner.height.saturating_sub(header_rows + 1) as usize;
    let total = edit.slot_edit_lines.len();
    let scroll_offset = if total > visible_rows {
        edit.slot_edit_cursor_line
            .saturating_sub(visible_rows / 2)
            .min(total.saturating_sub(visible_rows))
    } else {
        0
    };

    // 渲染行号 + 内容
    for (i, content) in edit
        .slot_edit_lines
        .iter()
        .skip(scroll_offset)
        .take(visible_rows)
        .enumerate()
    {
        let line_num = scroll_offset + i;
        let is_cursor_line = line_num == edit.slot_edit_cursor_line;

        let line_num_span = Span::styled(
            format!("{:4} │ ", line_num + 1),
            Style::default().fg(theme::FG_DIM),
        );

        let content_span = if is_cursor_line {
            let col = edit.slot_edit_cursor_col;
            let before: String = content.chars().take(col).collect();
            let at: String = content
                .chars()
                .nth(col)
                .map(|c| c.to_string())
                .unwrap_or_else(|| " ".to_string());
            let after: String = content.chars().skip(col + 1).collect();
            Line::from(vec![
                line_num_span,
                Span::styled(before, Style::default().fg(theme::FG_NORMAL)),
                Span::styled(
                    at,
                    Style::default().bg(theme::BG_SELECTED).fg(theme::FG_NORMAL),
                ),
                Span::styled(after, Style::default().fg(theme::FG_NORMAL)),
            ])
        } else {
            Line::from(vec![
                line_num_span,
                Span::styled(content.clone(), Style::default().fg(theme::FG_NORMAL)),
            ])
        };
        lines.push(content_span);
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}
