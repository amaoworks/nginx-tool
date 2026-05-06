use ratatui::style::Color;

// 颜色方案，对应 design.md §七
// 采用深色终端主题，色彩克制，重点突出状态信息

pub const FG_NORMAL: Color = Color::Reset;
pub const FG_DIM: Color = Color::DarkGray;
pub const FG_PATH: Color = Color::Gray;

pub const BG_HEADER: Color = Color::Blue;
pub const FG_HEADER: Color = Color::White;

pub const BG_SELECTED: Color = Color::Blue;
pub const FG_SELECTED: Color = Color::White;

pub const FG_OK: Color = Color::Green;
pub const FG_WARN: Color = Color::Yellow;
pub const FG_ERR: Color = Color::Red;

pub const BG_FOOTER: Color = Color::DarkGray;
pub const FG_FOOTER: Color = Color::White;

pub const BORDER: Color = Color::DarkGray;
#[allow(dead_code)]
pub const BORDER_FOCUS: Color = Color::Cyan;

pub const FG_HINT: Color = Color::Cyan;

// 注入槽相关
#[allow(dead_code)]
pub const FG_SLOT_MARKER: Color = Color::Cyan;
