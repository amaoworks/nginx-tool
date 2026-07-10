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

/// 操作按钮文案：焦点与否都用 `[ label ]`，靠背景/加粗区分，不加 `▶ ◀`。
pub fn button_label(label: &str) -> String {
    format!("[ {} ]", label)
}

/// 常规按钮样式：焦点蓝底白字，否则普通前景色。
pub fn button_style(focused: bool) -> Style {
    if focused {
        focused_button_style()
    } else {
        Style::default().fg(theme::FG_NORMAL)
    }
}

/// 弹窗等次要按钮：未聚焦时用暗色，聚焦仍为蓝底。
pub fn button_style_muted(focused: bool) -> Style {
    if focused {
        focused_button_style()
    } else {
        Style::default().fg(theme::FG_DIM)
    }
}
