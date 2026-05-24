//! LogSource: 日志源枚举，详见 architecture.md §11.5

use std::path::{Path, PathBuf};

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
    Global {
        kind: LogKind,
        access_path: Option<PathBuf>,
        error_path: Option<PathBuf>,
    },
    /// 站点日志（/var/log/nginx/<site>.access.log 或 error.log）
    Site {
        name: String,
        kind: LogKind,
        access_path: Option<PathBuf>,
        error_path: Option<PathBuf>,
    },
}

impl Default for LogSource {
    fn default() -> Self {
        LogSource::Global {
            kind: LogKind::Access,
            access_path: None,
            error_path: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogPaths {
    pub access: Option<PathBuf>,
    pub error: Option<PathBuf>,
}

impl LogSource {
    pub fn global(kind: LogKind, paths: LogPaths) -> Self {
        Self::Global {
            kind,
            access_path: paths.access,
            error_path: paths.error,
        }
    }

    pub fn site(name: String, kind: LogKind, paths: LogPaths) -> Self {
        Self::Site {
            name,
            kind,
            access_path: paths.access,
            error_path: paths.error,
        }
    }

    /// 获取日志文件路径
    pub fn path(&self) -> PathBuf {
        match self {
            LogSource::Global {
                kind,
                access_path,
                error_path,
            } => match kind {
                LogKind::Access => access_path
                    .clone()
                    .unwrap_or_else(default_global_access_log_path),
                LogKind::Error => error_path
                    .clone()
                    .unwrap_or_else(default_global_error_log_path),
            },
            LogSource::Site {
                name,
                kind,
                access_path,
                error_path,
            } => match kind {
                LogKind::Access => access_path.clone().unwrap_or_else(|| {
                    PathBuf::from("/var/log/nginx").join(format!("{name}.access.log"))
                }),
                LogKind::Error => error_path.clone().unwrap_or_else(|| {
                    PathBuf::from("/var/log/nginx").join(format!("{name}.error.log"))
                }),
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
            LogSource::Global { kind, .. } => format!("全局 {}", kind.label()),
            LogSource::Site { name, kind, .. } => format!("{} {}", name, kind.label()),
        }
    }

    /// 获取当前日志类型
    pub fn kind(&self) -> LogKind {
        match self {
            LogSource::Global { kind, .. } => *kind,
            LogSource::Site { kind, .. } => *kind,
        }
    }
}

pub fn parse_log_paths(text: &str) -> LogPaths {
    let cleaned: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");

    let directive_re = regex::Regex::new(r"(?m)^\s*(access_log|error_log)\s+([^;]+);").unwrap();
    let mut paths = LogPaths::default();

    for cap in directive_re.captures_iter(&cleaned) {
        let name = &cap[1];
        let Some(first) = cap[2].split_whitespace().next() else {
            continue;
        };
        if first.eq_ignore_ascii_case("off") {
            continue;
        }
        let path = PathBuf::from(first);
        match name {
            "access_log" if paths.access.is_none() => paths.access = Some(path),
            "error_log" if paths.error.is_none() => paths.error = Some(path),
            _ => {}
        }
    }

    paths
}

pub fn detect_global_log_paths(nginx_conf_path: &Path) -> LogPaths {
    std::fs::read_to_string(nginx_conf_path)
        .ok()
        .map(|content| parse_log_paths(&content))
        .unwrap_or_default()
}

fn default_global_access_log_path() -> PathBuf {
    PathBuf::from("/var/log/nginx/access.log")
}

fn default_global_error_log_path() -> PathBuf {
    PathBuf::from("/var/log/nginx/error.log")
}

impl LogKind {
    pub fn toggle(&self) -> LogKind {
        match self {
            LogKind::Access => LogKind::Error,
            LogKind::Error => LogKind::Access,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_log_paths_extracts_first_access_and_error_log() {
        let text = r#"
http {
    access_log /data/logs/nginx/access-main.log main;
    error_log /data/logs/nginx/error-main.log warn;
    server {
        access_log /data/logs/nginx/site.log;
    }
}
"#;

        let paths = parse_log_paths(text);
        assert_eq!(
            paths.access,
            Some(PathBuf::from("/data/logs/nginx/access-main.log"))
        );
        assert_eq!(
            paths.error,
            Some(PathBuf::from("/data/logs/nginx/error-main.log"))
        );
    }

    #[test]
    fn parse_log_paths_ignores_off_and_comments() {
        let text = r#"
# access_log /commented/out.log;
server {
    access_log off;
    error_log /var/log/nginx/app.error.log info;
}
"#;

        let paths = parse_log_paths(text);
        assert_eq!(paths.access, None);
        assert_eq!(
            paths.error,
            Some(PathBuf::from("/var/log/nginx/app.error.log"))
        );
    }
}
