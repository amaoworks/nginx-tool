use ratatui::layout::{Constraint, Layout, Rect};

/// 顶部 1 行 + 中部分左右两栏 + 底部 1 行的根布局。
/// 详见 design.md §二 尺寸比例 和 architecture.md §11.1。
pub struct RootAreas {
    pub header: Rect,
    pub sidebar: Rect,
    pub content: Rect,
    pub footer: Rect,
}

/// 侧边栏宽度：终端 1/4 宽，但下限 20 列（design.md 要求）。
pub fn sidebar_width(total: u16) -> u16 {
    let raw = total / 4;
    raw.max(20)
}

pub fn root_layout(area: Rect) -> RootAreas {
    let vertical = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    let sw = sidebar_width(area.width);
    let body = Layout::horizontal([Constraint::Length(sw), Constraint::Min(0)]).split(vertical[1]);

    RootAreas {
        header: vertical[0],
        sidebar: body[0],
        content: body[1],
        footer: vertical[2],
    }
}

/// 在指定区域居中开一个对话框区域。用于 modal。
pub fn centered_rect(parent: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(parent.width);
    let h = height.min(parent.height);
    let x = parent.x + (parent.width.saturating_sub(w)) / 2;
    let y = parent.y + (parent.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}
