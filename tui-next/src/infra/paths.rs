use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Context;

/// 应用本地数据根目录与子目录定位。详见 architecture.md §10。
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub backups: PathBuf,
    pub logs: PathBuf,
    pub cache: PathBuf,
    pub tmp: PathBuf,
    pub lock: PathBuf,
    pub audit_log: PathBuf,
}

impl AppPaths {
    pub fn detect() -> anyhow::Result<Self> {
        let home = directories::BaseDirs::new()
            .context("无法定位用户主目录（HOME 缺失）")?
            .home_dir()
            .to_path_buf();
        let root = home.join(".local/ngtool");
        Ok(Self {
            config_file: root.join("config.toml"),
            backups: root.join("backups"),
            logs: root.join("logs"),
            cache: root.join("cache"),
            tmp: root.join("tmp"),
            lock: root.join("tui.lock"),
            audit_log: root.join("logs/audit.log"),
            root,
        })
    }

    /// 创建所有子目录。已存在则跳过。
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for d in [
            &self.root,
            &self.backups,
            &self.logs,
            &self.cache,
            &self.tmp,
        ] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }

    /// 清理 tmp 中超过 7 天的残留文件。详见 architecture.md §15.0。
    /// 返回删除条目数。
    pub fn cleanup_tmp(&self) -> std::io::Result<usize> {
        cleanup_dir_older_than(&self.tmp, Duration::from_secs(60 * 60 * 24 * 7))
    }
}

fn cleanup_dir_older_than(dir: &Path, age: Duration) -> std::io::Result<usize> {
    if !dir.is_dir() {
        return Ok(0);
    }
    let cutoff = SystemTime::now()
        .checked_sub(age)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut removed = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta.modified().unwrap_or(SystemTime::now());
        if mtime < cutoff {
            let path = entry.path();
            let res = if meta.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if res.is_ok() {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::SystemTime;

    #[test]
    fn cleanup_removes_old_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let fresh = dir.join("fresh.tmp");
        fs::write(&fresh, b"x").unwrap();

        let stale = dir.join("stale.tmp");
        fs::write(&stale, b"x").unwrap();
        // 把 stale 的 mtime 拨到 8 天前
        let old = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 8);
        let _ = filetime::set_file_mtime(&stale, filetime::FileTime::from_system_time(old));

        let removed = cleanup_dir_older_than(dir, Duration::from_secs(60 * 60 * 24 * 7)).unwrap();
        assert_eq!(removed, 1);
        assert!(fresh.exists());
        assert!(!stale.exists());
    }
}
