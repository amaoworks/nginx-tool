use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::NgToolError;
use crate::infra::executor::{CommandExecutor, CommandSpec};
use crate::template::site_tuning::Http2Support;

/// nginx 二进制相关适配。详见 architecture.md §11.6 / §6.4。
#[derive(Debug, Clone)]
pub struct NginxAdapter {
    exec: CommandExecutor,
    http2_support: Arc<Mutex<Http2Support>>,
}

impl NginxAdapter {
    pub fn new(exec: CommandExecutor) -> Self {
        Self {
            exec,
            http2_support: Arc::new(Mutex::new(Http2Support::Unknown)),
        }
    }

    /// 重新运行 `nginx -V` 并缓存当前二进制的 HTTP/2 配置语法能力。
    pub async fn refresh_http2_support(&self) -> Http2Support {
        let support = match self
            .exec
            .run(
                CommandSpec::new("nginx")
                    .arg("-V")
                    .timeout(Duration::from_secs(3)),
            )
            .await
        {
            Ok(out) if out.ok() => parse_http2_support(&out.combined()),
            _ => Http2Support::Unknown,
        };

        match self.http2_support.lock() {
            Ok(mut cached) => *cached = support,
            Err(poisoned) => *poisoned.into_inner() = support,
        }
        support
    }

    /// 返回最近一次探测结果；尚未探测时为 `Unknown`。
    pub fn http2_support(&self) -> Http2Support {
        match self.http2_support.lock() {
            Ok(cached) => *cached,
            Err(poisoned) => *poisoned.into_inner(),
        }
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

fn parse_http2_support(output: &str) -> Http2Support {
    let Some(version) = parse_nginx_version(output) else {
        return Http2Support::Unknown;
    };

    let has_http2_module = output
        .split_ascii_whitespace()
        .map(|arg| arg.trim_matches(['\'', '"']))
        .any(|arg| arg == "--with-http_v2_module");
    if !has_http2_module {
        return Http2Support::Unsupported;
    }

    if version >= (1, 25, 1) {
        Http2Support::Modern
    } else {
        Http2Support::Legacy
    }
}

fn parse_nginx_version(output: &str) -> Option<(u32, u32, u32)> {
    const PREFIX: &str = "nginx version: nginx/";

    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix(PREFIX) else {
            continue;
        };
        let version_len = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
            .count();
        let raw = &rest[..version_len];
        let mut parts = raw.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if major.is_empty() || minor.is_empty() || patch.is_empty() {
            continue;
        }

        let suffix = rest.as_bytes().get(version_len).copied();
        if suffix
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'-' | b'+' | b'('))
        {
            continue;
        }

        if let (Ok(major), Ok(minor), Ok(patch)) = (major.parse(), minor.parse(), patch.parse()) {
            return Some((major, minor, patch));
        }
    }
    None
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

    #[test]
    fn parses_http2_support_from_nginx_build_info() {
        let cases = [
            (
                "nginx version: nginx/1.24.0\nconfigure arguments: --with-http_v2_module",
                Http2Support::Legacy,
            ),
            (
                "nginx version: nginx/1.26.2\nconfigure arguments: --with-http_v2_module",
                Http2Support::Modern,
            ),
            (
                "nginx version: nginx/1.25.0\nconfigure arguments: --with-http_v2_module",
                Http2Support::Legacy,
            ),
            (
                "nginx version: nginx/1.25.1\nconfigure arguments: --with-http_v2_module",
                Http2Support::Modern,
            ),
            (
                "nginx version: nginx/1.25.1-1~bookworm\nconfigure arguments: '--with-http_v2_module'",
                Http2Support::Modern,
            ),
            (
                "nginx version: nginx/1.26.2 (Ubuntu)\nconfigure arguments: --with-ssl",
                Http2Support::Unsupported,
            ),
            (
                "openresty version 1.25.1 with --with-http_v2_module",
                Http2Support::Unknown,
            ),
        ];

        for (output, expected) in cases {
            assert_eq!(parse_http2_support(output), expected, "{output}");
        }
    }

    #[test]
    fn rejects_ambiguous_nginx_versions_and_module_substrings() {
        for output in [
            "nginx version: nginx/1.25\nconfigure arguments: --with-http_v2_module",
            "nginx version: nginx/1.25.1.2\nconfigure arguments: --with-http_v2_module",
            "nginx version: fake-nginx/1.25.1\nconfigure arguments: --with-http_v2_module",
            "nginx version: nginx/1.25.1rc1\nconfigure arguments: --with-http_v2_module",
            "configure arguments: --prefix=/opt/nginx/1.25.1 --with-http_v2_module",
        ] {
            assert_eq!(
                parse_http2_support(output),
                Http2Support::Unknown,
                "{output}"
            );
        }

        assert_eq!(
            parse_http2_support(
                "nginx version: nginx/1.25.1\nconfigure arguments: --with-http_v2_module=dynamic"
            ),
            Http2Support::Unsupported
        );
    }
}
