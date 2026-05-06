use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use serde::Serialize;

/// 操作审计日志，详见 architecture.md §18。
/// 文件 `~/.local/ngtool/logs/audit.log`，每行一条 JSON。
/// 单文件超过 5MB 自动滚动到 `audit.log.1`，最多保留 5 份。
pub struct AuditLogger {
    path: PathBuf,
    actor: String,
    mode: String,
    inner: Mutex<Option<File>>,
    max_size: u64,
    keep_files: usize,
}

#[derive(Serialize)]
struct AuditEntry<'a> {
    ts: String,
    actor: &'a str,
    mode: &'a str,
    action: &'a str,
    target: &'a str,
    result: &'a str,
    duration_ms: u64,
    details: serde_json::Value,
}

#[derive(Debug, Clone, Copy)]
pub enum AuditResult {
    Success,
    Failure,
    Cancelled,
}

impl AuditResult {
    fn as_str(&self) -> &'static str {
        match self {
            AuditResult::Success => "success",
            AuditResult::Failure => "failure",
            AuditResult::Cancelled => "cancelled",
        }
    }
}

impl AuditLogger {
    pub fn new(path: PathBuf, actor: String, mode: String) -> Self {
        Self {
            path,
            actor,
            mode,
            inner: Mutex::new(None),
            max_size: 5 * 1024 * 1024,
            keep_files: 5,
        }
    }

    #[allow(dead_code)]
    pub fn with_limits(mut self, max_size: u64, keep_files: usize) -> Self {
        self.max_size = max_size;
        self.keep_files = keep_files;
        self
    }

    pub fn log(
        &self,
        action: &str,
        target: &str,
        result: AuditResult,
        duration_ms: u64,
        details: serde_json::Value,
    ) {
        let entry = AuditEntry {
            ts: Utc::now().to_rfc3339(),
            actor: &self.actor,
            mode: &self.mode,
            action,
            target,
            result: result.as_str(),
            duration_ms,
            details,
        };
        let line = match serde_json::to_string(&entry) {
            Ok(s) => s,
            Err(_) => return,
        };
        if let Err(e) = self.append(&line) {
            tracing::warn!("audit log write failed: {}", e);
        }
    }

    fn append(&self, line: &str) -> std::io::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        // 大小滚动
        if let Ok(meta) = std::fs::metadata(&self.path) {
            if meta.len() > self.max_size {
                self.rotate()?;
                *guard = None;
            }
        }
        if guard.is_none() {
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            // 强制 0600 权限
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
            *guard = Some(f);
        }
        let f = guard.as_mut().unwrap();
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        f.flush()?;
        Ok(())
    }

    fn rotate(&self) -> std::io::Result<()> {
        // audit.log.<N> -> audit.log.<N+1> for N=keep_files-1..1
        for i in (1..self.keep_files).rev() {
            let from = rotated_path(&self.path, i);
            let to = rotated_path(&self.path, i + 1);
            let _ = std::fs::rename(&from, &to);
        }
        let to = rotated_path(&self.path, 1);
        let _ = std::fs::rename(&self.path, &to);
        Ok(())
    }
}

fn rotated_path(base: &Path, n: usize) -> PathBuf {
    let mut s = base.as_os_str().to_owned();
    s.push(format!(".{}", n));
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_jsonl_line() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("audit.log");
        let log = AuditLogger::new(path.clone(), "tester".into(), "read-write".into());
        log.log(
            "site.create",
            "app",
            AuditResult::Success,
            123,
            serde_json::json!({"domain": "app.example.com"}),
        );
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"action\":\"site.create\""));
        assert!(content.contains("\"target\":\"app\""));
        assert!(content.contains("\"result\":\"success\""));
        assert!(content.contains("app.example.com"));
        assert!(content.ends_with('\n'));
    }

    #[test]
    fn rotates_when_too_large() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("audit.log");
        let log =
            AuditLogger::new(path.clone(), "tester".into(), "read-write".into()).with_limits(64, 3);
        for i in 0..50 {
            log.log(
                "service.test",
                &format!("nginx-{}", i),
                AuditResult::Success,
                1,
                serde_json::json!({"i": i}),
            );
        }
        assert!(rotated_path(&path, 1).exists());
    }
}
