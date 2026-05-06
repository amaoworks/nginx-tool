use serde::Deserialize;

use crate::error::NgToolError;

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
}

pub async fn check_latest_release() -> Result<UpdateInfo, NgToolError> {
    let client = reqwest::Client::builder()
        .user_agent(format!("ngtool/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(http_error)?;

    let release = client
        .get(RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(http_error)?
        .error_for_status()
        .map_err(http_error)?
        .json::<LatestRelease>()
        .await
        .map_err(http_error)?;

    let current_raw = env!("CARGO_PKG_VERSION");
    let current_norm = normalize_version(current_raw);
    let latest_norm = normalize_version(&release.tag_name);

    Ok(UpdateInfo {
        current_version: current_raw.to_string(),
        latest_version: release.tag_name,
        release_url: release.html_url,
        published_at: release.published_at,
        installed_via_release: !current_norm.is_empty(),
        has_update: !current_norm.is_empty() && current_norm != latest_norm,
    })
}

pub fn release_page() -> String {
    format!("https://github.com/{}/releases/latest", REPO_SLUG)
}

fn normalize_version(input: &str) -> String {
    input.trim().trim_start_matches('v').to_string()
}

fn http_error(err: reqwest::Error) -> NgToolError {
    NgToolError::CommandFailed {
        command: "GET /releases/latest".into(),
        code: err.status().map(|s| s.as_u16() as i32),
        stderr: err.to_string(),
    }
}
