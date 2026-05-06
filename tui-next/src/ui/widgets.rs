// 共用小组件占位。后续阶段将逐步抽出表格行、状态徽标、进度条等。

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::ui::theme;

#[allow(dead_code)]
pub fn dim<'a>(text: impl Into<std::borrow::Cow<'a, str>>) -> Span<'a> {
    Span::styled(text.into(), Style::default().fg(theme::FG_DIM))
}

#[allow(dead_code)]
pub fn hint<'a>(text: impl Into<std::borrow::Cow<'a, str>>) -> Span<'a> {
    Span::styled(text.into(), Style::default().fg(theme::FG_HINT))
}

#[allow(dead_code)]
pub fn bold<'a>(text: impl Into<std::borrow::Cow<'a, str>>) -> Span<'a> {
    Span::styled(text.into(), Style::default().add_modifier(Modifier::BOLD))
}
