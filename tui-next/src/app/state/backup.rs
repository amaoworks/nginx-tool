//! 备份页子状态。

use std::time::Instant;

/// 备份页子状态
#[derive(Debug, Default)]
pub struct BackupState {
    pub list: Vec<crate::domain::backup::Backup>,
    pub selected: usize,
    pub last_refresh: Option<Instant>,
    pub refreshing: bool,
    pub pending_refresh: bool,
    pub last_error: Option<String>,
    /// 操作输出
    pub output: Vec<String>,
    pub running: bool,
    /// 待派发：创建备份
    pub pending_create: bool,
    /// 待派发：删除指定备份
    pub pending_delete: Option<std::path::PathBuf>,
    /// 待派发：还原指定备份
    pub pending_restore: Option<std::path::PathBuf>,
}

impl BackupState {
    pub fn move_cursor(&mut self, delta: i32) {
        if self.list.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.list.len() as i32;
        let mut idx = self.selected as i32 + delta;
        if idx < 0 {
            idx = len - 1;
        } else if idx >= len {
            idx = 0;
        }
        self.selected = idx as usize;
    }

    pub fn current(&self) -> Option<&crate::domain::backup::Backup> {
        self.list.get(self.selected)
    }

    pub fn push_output(&mut self, lines: impl IntoIterator<Item = String>) {
        let limit = 200usize;
        for line in lines {
            self.output.push(line);
        }
        if self.output.len() > limit {
            let drop = self.output.len() - limit;
            self.output.drain(0..drop);
        }
    }

    pub fn clear_output_buffer(&mut self) {
        self.output.clear();
    }
}
