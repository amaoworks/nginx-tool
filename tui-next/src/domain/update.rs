use std::cmp::Ordering;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::NgToolError;
use crate::version::APP_VERSION;

const REPO_SLUG: &str = "amaoworks/nginx-tool";
const RELEASE_API: &str = "https://api.github.com/repos/amaoworks/nginx-tool/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub published_at: Option<String>,
    pub installed_via_release: bool,
    pub has_update: bool,
}

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfUpdateOutcome {
    pub info: UpdateInfo,
    pub binary_path: PathBuf,
    pub updated: bool,
}

pub async fn check_latest_release() -> Result<UpdateInfo, NgToolError> {
    let release = fetch_latest_release().await?;
    Ok(update_info_from_release(&release))
}

pub async fn upgrade_to_latest_release() -> Result<SelfUpdateOutcome, NgToolError> {
    let release = fetch_latest_release().await?;
    let info = update_info_from_release(&release);
    let binary_path = env::current_exe().map_err(|e| NgToolError::FileOperationFailed {
        path: PathBuf::from("current_exe"),
        message: e.to_string(),
    })?;

    if !info.has_update {
        return Ok(SelfUpdateOutcome {
            info,
            binary_path,
            updated: false,
        });
    }

    let asset = release_asset_for_current_arch(&release)?;
    let client = http_client()?;
    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(http_error)?
        .error_for_status()
        .map_err(http_error)?
        .bytes()
        .await
        .map_err(http_error)?;

    if !bytes.starts_with(b"\x7fELF") {
        return Err(NgToolError::ParseFailed {
            target: asset.name.clone(),
            message: "下载结果不是有效的 Linux ELF 二进制".into(),
        });
    }

    replace_binary(&binary_path, &bytes)?;

    Ok(SelfUpdateOutcome {
        info,
        binary_path,
        updated: true,
    })
}

async fn fetch_latest_release() -> Result<LatestRelease, NgToolError> {
    let client = reqwest::Client::builder()
        .user_agent(format!("ngtool/{}", APP_VERSION))
        .build()
        .map_err(http_error)?;

    client
        .get(RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(http_error)?
        .error_for_status()
        .map_err(http_error)?
        .json::<LatestRelease>()
        .await
        .map_err(http_error)
}

fn http_client() -> Result<reqwest::Client, NgToolError> {
    reqwest::Client::builder()
        .user_agent(format!("ngtool/{}", APP_VERSION))
        .build()
        .map_err(http_error)
}

fn update_info_from_release(release: &LatestRelease) -> UpdateInfo {
    let current_raw = APP_VERSION;
    let current_norm = normalize_version(current_raw);
    let latest_norm = normalize_version(&release.tag_name);
    let has_update = matches!(
        compare_versions(&current_norm, &latest_norm),
        Some(Ordering::Less)
    );

    UpdateInfo {
        current_version: current_raw.to_string(),
        latest_version: release.tag_name.clone(),
        release_url: release.html_url.clone(),
        published_at: release.published_at.clone(),
        installed_via_release: !current_norm.is_empty(),
        has_update,
    }
}

pub fn release_page() -> String {
    format!("https://github.com/{}/releases/latest", REPO_SLUG)
}

fn normalize_version(input: &str) -> String {
    input.trim().trim_start_matches('v').to_string()
}

fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let parse = |s: &str| -> Option<Vec<u64>> {
        s.split(['.', '-'])
            .take(3)
            .map(|part| part.parse::<u64>().ok())
            .collect()
    };
    let a = parse(a)?;
    let b = parse(b)?;
    Some(a.cmp(&b))
}

fn release_asset_for_current_arch(release: &LatestRelease) -> Result<&ReleaseAsset, NgToolError> {
    let arch = match env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => {
            return Err(NgToolError::InvalidInput {
                field: "arch".into(),
                message: format!("当前架构 {} 暂无预编译 ngtool Release", other),
            })
        }
    };
    let suffix = format!("-linux-{}", arch);
    release
        .assets
        .iter()
        .find(|asset| asset.name.starts_with("ngtool-") && asset.name.ends_with(&suffix))
        .ok_or_else(|| NgToolError::ParseFailed {
            target: "GitHub Release assets".into(),
            message: format!("未找到当前架构的 ngtool 二进制 ({})", suffix),
        })
}

fn replace_binary(path: &Path, bytes: &[u8]) -> Result<(), NgToolError> {
    let dir = path
        .parent()
        .ok_or_else(|| NgToolError::FileOperationFailed {
            path: path.to_path_buf(),
            message: "无法确定二进制所在目录".into(),
        })?;
    let tmp_path = dir.join(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    fs::write(&tmp_path, bytes).map_err(|e| NgToolError::FileOperationFailed {
        path: tmp_path.clone(),
        message: e.to_string(),
    })?;
    fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755)).map_err(|e| {
        NgToolError::FileOperationFailed {
            path: tmp_path.clone(),
            message: e.to_string(),
        }
    })?;
    fs::rename(&tmp_path, path).map_err(|e| NgToolError::FileOperationFailed {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn http_error(err: reqwest::Error) -> NgToolError {
    NgToolError::CommandFailed {
        command: "GET /releases/latest".into(),
        code: err.status().map(|s| s.as_u16() as i32),
        stderr: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_leading_v() {
        assert_eq!(normalize_version("v1.0.4"), "1.0.4");
        assert_eq!(normalize_version("1.0.4"), "1.0.4");
    }

    #[test]
    fn compares_semver_core() {
        assert_eq!(compare_versions("1.0.3", "1.0.4"), Some(Ordering::Less));
        assert_eq!(compare_versions("1.0.4", "1.0.4"), Some(Ordering::Equal));
        assert_eq!(compare_versions("1.0.10", "1.0.4"), Some(Ordering::Greater));
    }

    #[test]
    fn update_info_only_reports_newer_release_as_update() {
        let release = LatestRelease {
            tag_name: "v1.0.4".into(),
            html_url: release_page(),
            published_at: None,
            assets: vec![],
        };
        let info = update_info_from_release(&release);
        assert_eq!(info.latest_version, "v1.0.4");
    }
}
