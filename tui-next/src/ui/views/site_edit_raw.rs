//! 站点原始配置编辑视图，对应 design.md 子模式 D

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
    let config_path = format!("/etc/nginx/sites-available/{}.conf", site_name);

    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(" 📁 站点管理 ▸ 编辑: {} ▸ 原始配置 ", site_name),
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // 文件路径提示
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("文件: {}", config_path),
        Style::default().fg(theme::FG_DIM),
    )));
    lines.push(Line::from(""));

    // 计算可见区域
    let visible_lines = inner.height.saturating_sub(3) as usize;
    let scroll_offset = edit.raw_cursor_line.saturating_sub(visible_lines / 2);

    // 渲染代码行
    for (i, line_content) in edit
        .raw_lines
        .iter()
        .skip(scroll_offset)
        .take(visible_lines)
        .enumerate()
    {
        let line_num = scroll_offset + i + 1;
        let is_cursor_line = line_num - 1 == edit.raw_cursor_line;

        let line_num_style = Style::default().fg(theme::FG_DIM);
        let content_style = Style::default().fg(theme::FG_NORMAL);

        let line_num_span = Span::styled(format!("{:4} │ ", line_num), line_num_style);

        let content_span = if is_cursor_line {
            // 光标行高亮
            let cursor_col = edit.raw_cursor_col;
            let before: String = line_content.chars().take(cursor_col).collect();
            let at_cursor: String = line_content
                .chars()
                .nth(cursor_col)
                .map(|c| c.to_string())
                .unwrap_or_else(|| " ".to_string());
            let after: String = line_content.chars().skip(cursor_col + 1).collect();

            Span::styled(
                format!("{}{}{}", before, at_cursor, after),
                Style::default().bg(theme::BG_SELECTED).fg(theme::FG_NORMAL),
            )
        } else {
            Span::styled(line_content.clone(), content_style)
        };

        lines.push(Line::from(vec![line_num_span, content_span]));
    }

    // 底部提示
    lines.push(Line::from(""));
    if edit.dirty {
        lines.push(Line::from(Span::styled(
            " ⚠ 有未保存的修改",
            Style::default().fg(theme::FG_WARN),
        )));
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}
