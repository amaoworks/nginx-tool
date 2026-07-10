//! 通用运行模式、焦点与通知。

use std::time::{Duration, Instant};

use crate::ui::theme;

#[derive(Debug, Clone)]
pub enum RunMode {
    ReadWrite,
    ReadOnly {
        #[allow(dead_code)]
        reason: String,
    },
}

impl RunMode {
    #[allow(dead_code)]
    pub fn is_readonly(&self) -> bool {
        matches!(self, RunMode::ReadOnly { .. })
    }

    pub fn label(&self) -> &str {
        match self {
            RunMode::ReadWrite => "读写",
            RunMode::ReadOnly { .. } => "只读",
        }
    }
}

/// 焦点区域：左侧菜单或右侧主视图
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    Sidebar,
    Content,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Success,
    Failure,
    Info,
}

impl NotificationKind {
    pub fn fg(&self) -> ratatui::style::Color {
        match self {
            NotificationKind::Success => theme::FG_OK,
            NotificationKind::Failure => theme::FG_ERR,
            NotificationKind::Info => theme::FG_PATH,
        }
    }

    pub fn glyph(&self) -> &'static str {
        match self {
            NotificationKind::Success => "✓",
            NotificationKind::Failure => "✗",
            NotificationKind::Info => "ℹ",
        }
    }
}

/// 操作结果提示，详见 design.md §三 操作结果提示
#[derive(Debug, Clone)]
pub struct Notification {
    pub kind: NotificationKind,
    pub message: String,
    pub expires_at: Instant,
}

#[allow(dead_code)]
impl Notification {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Success,
            message: message.into(),
            expires_at: Instant::now() + Duration::from_secs(2),
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Failure,
            message: message.into(),
            expires_at: Instant::now() + Duration::from_secs(3),
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Info,
            message: message.into(),
            expires_at: Instant::now() + Duration::from_secs(2),
        }
    }
}
