//! LogSource: 日志源枚举，详见 architecture.md §11.5

use std::path::PathBuf;

/// 日志类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogKind {
    #[default]
    Access,
    Error,
}

impl LogKind {
    pub fn label(&self) -> &'static str {
        match self {
            LogKind::Access => "访问日志",
            LogKind::Error => "错误日志",
        }
    }

    pub fn file_suffix(&self) -> &'static str {
        match self {
            LogKind::Access => "access.log",
            LogKind::Error => "error.log",
        }
    }
}

/// 日志源：全局或特定站点
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogSource {
    /// 全局日志（/var/log/nginx/access.log 或 error.log）
    Global(LogKind),
    /// 站点日志（/var/log/nginx/<site>.access.log 或 error.log）
    Site { name: String, kind: LogKind },
}

impl Default for LogSource {
    fn default() -> Self {
        LogSource::Global(LogKind::Access)
    }
}

impl LogSource {
    /// 获取日志文件路径
    pub fn path(&self) -> PathBuf {
        match self {
            LogSource::Global(kind) => PathBuf::from("/var/log/nginx").join(kind.file_suffix()),
            LogSource::Site { name, kind } => {
                PathBuf::from("/var/log/nginx").join(format!("{}.{}", name, kind.file_suffix()))
            }
        }
    }

    /// 检查日志文件是否存在
    pub fn exists(&self) -> bool {
        self.path().exists()
    }

    /// 显示标签
    pub fn label(&self) -> String {
        match self {
            LogSource::Global(kind) => format!("全局 {}", kind.label()),
            LogSource::Site { name, kind } => format!("{} {}", name, kind.label()),
        }
    }

    /// 切换日志类型
    pub fn toggle_kind(&self) -> LogSource {
        match self {
            LogSource::Global(kind) => LogSource::Global(kind.toggle()),
            LogSource::Site { name, kind } => LogSource::Site {
                name: name.clone(),
                kind: kind.toggle(),
            },
        }
    }

    /// 切换到指定站点
    pub fn with_site(&self, name: Option<String>) -> LogSource {
        let kind = self.kind();
        match name {
            Some(n) => LogSource::Site { name: n, kind },
            None => LogSource::Global(kind),
        }
    }

    /// 获取当前日志类型
    pub fn kind(&self) -> LogKind {
        match self {
            LogSource::Global(kind) => *kind,
            LogSource::Site { kind, .. } => *kind,
        }
    }
}

impl LogKind {
    pub fn toggle(&self) -> LogKind {
        match self {
            LogKind::Access => LogKind::Error,
            LogKind::Error => LogKind::Access,
        }
    }
}
