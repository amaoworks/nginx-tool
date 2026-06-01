// 焦点状态指示，详见 design.md §二 焦点指示规范。
// 当前阶段提供基础工具函数；详细的字段焦点指示由 P5 表单视图实装。

use ratatui::style::{Modifier, Style};

use crate::ui::theme;

#[allow(dead_code)]
pub fn focused_style() -> Style {
    Style::default()
        .bg(theme::BG_SELECTED)
        .fg(theme::FG_SELECTED)
        .add_modifier(Modifier::BOLD)
}

#[allow(dead_code)]
pub fn unfocused_style() -> Style {
    Style::default().fg(theme::FG_NORMAL)
}

pub fn panel_border_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(theme::BORDER_FOCUS)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::BORDER)
    }
}

pub fn selected_text_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_HINT)
    }
}

pub fn focused_button_style() -> Style {
    Style::default()
        .fg(theme::FG_SELECTED)
        .bg(theme::BG_SELECTED)
        .add_modifier(Modifier::BOLD)
}
