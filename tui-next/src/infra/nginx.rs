use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::NgToolError;
use crate::infra::executor::{CommandExecutor, CommandSpec};

/// nginx 二进制相关适配。详见 architecture.md §11.6 / §6.4。
#[derive(Debug, Clone)]
pub struct NginxAdapter {
    exec: CommandExecutor,
}

impl NginxAdapter {
    pub fn new(exec: CommandExecutor) -> Self {
        Self { exec }
    }

    /// `nginx -v`：输出在 stderr，需要兜底取 stdout。
    pub async fn version(&self) -> Result<String, NgToolError> {
        let out = self
            .exec
            .run(
                CommandSpec::new("nginx")
                    .arg("-v")
                    .timeout(Duration::from_secs(3)),
            )
            .await?;
        let raw = if !out.stderr.is_empty() {
            out.stderr.clone()
        } else {
            out.stdout.clone()
        };
        Ok(raw.trim().to_string())
    }

    /// `nginx -t`：成功返回完整输出（含 stderr 中的诊断），失败返回 `NginxTestFailed`。
    pub async fn test_config(&self) -> Result<String, NgToolError> {
        let out = self
            .exec
            .run(
                CommandSpec::new("nginx")
                    .arg("-t")
                    .timeout(Duration::from_secs(5)),
            )
            .await?;
        let combined = out.combined();
        if out.ok() {
            Ok(combined)
        } else {
            Err(NgToolError::NginxTestFailed { output: combined })
        }
    }
}

/// 站点扫描结果中的原始条目。后续阶段在 domain 层进一步解析为 `Site`。
#[derive(Debug, Clone)]
pub struct RawSite {
    pub name: String,
    pub path: PathBuf,
    pub enabled: bool,
}

/// 扫描 sites-available 与 sites-enabled，构造原始条目列表。
/// 详见 architecture.md §11.3 / §6.4。
pub fn scan_sites(sites_available: &Path, sites_enabled: &Path) -> std::io::Result<Vec<RawSite>> {
    let mut sites = Vec::new();
    if !sites_available.is_dir() {
        return Ok(sites);
    }
    for entry in std::fs::read_dir(sites_available)? {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("conf") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.is_empty() {
            continue;
        }
        let enabled = is_enabled(sites_enabled, stem);
        sites.push(RawSite {
            name: stem.to_string(),
            path,
            enabled,
        });
    }
    sites.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sites)
}

fn is_enabled(sites_enabled: &Path, name: &str) -> bool {
    let p = sites_enabled.join(format!("{}.conf", name));
    // 接受真实文件、符号链接（无论是否 dangling，符号链接关系即视为已启用）
    p.symlink_metadata().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn scans_and_detects_enabled() {
        let tmp = tempfile::tempdir().unwrap();
        let avail = tmp.path().join("sites-available");
        let enabled = tmp.path().join("sites-enabled");
        fs::create_dir_all(&avail).unwrap();
        fs::create_dir_all(&enabled).unwrap();

        fs::write(avail.join("app.conf"), b"# app").unwrap();
        fs::write(avail.join("blog.conf"), b"# blog").unwrap();
        fs::write(avail.join("notes.txt"), b"ignored").unwrap();
        symlink(avail.join("app.conf"), enabled.join("app.conf")).unwrap();

        let sites = scan_sites(&avail, &enabled).unwrap();
        assert_eq!(sites.len(), 2);
        let app = sites.iter().find(|s| s.name == "app").unwrap();
        let blog = sites.iter().find(|s| s.name == "blog").unwrap();
        assert!(app.enabled);
        assert!(!blog.enabled);
    }

    #[test]
    fn missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let sites = scan_sites(
            &tmp.path().join("missing"),
            &tmp.path().join("also-missing"),
        )
        .unwrap();
        assert!(sites.is_empty());
    }
}
