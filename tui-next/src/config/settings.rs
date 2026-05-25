use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    #[serde(default)]
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub certbot: CertbotConfig,
}

impl AppSettings {
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default = "default_github_proxy")]
    pub github_proxy: String,
    #[serde(default = "default_auto_detect")]
    pub auto_detect: bool,
}

fn default_github_proxy() -> String {
    "https://ghfast.top".to_string()
}

fn default_auto_detect() -> bool {
    true
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            github_proxy: default_github_proxy(),
            auto_detect: default_auto_detect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertbotConfig {
    #[serde(default)]
    pub email: String,
    #[serde(default = "default_certbot_allow_unsafe")]
    pub allow_unsafe_without_email: bool,
}

fn default_certbot_allow_unsafe() -> bool {
    true
}

impl Default for CertbotConfig {
    fn default() -> Self {
        Self {
            email: String::new(),
            allow_unsafe_without_email: default_certbot_allow_unsafe(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_defaults_when_file_missing() {
        let path = std::path::Path::new("/tmp/ngtool-nonexistent-config.toml");
        let settings = AppSettings::load(path);
        assert!(settings.certbot.email.is_empty());
        assert!(settings.certbot.allow_unsafe_without_email);
    }

    #[test]
    fn parse_certbot_email_from_toml() {
        let text = r#"
[certbot]
email = "admin@example.com"
allow_unsafe_without_email = false
"#;
        let settings: AppSettings = toml::from_str(text).unwrap();
        assert_eq!(settings.certbot.email, "admin@example.com");
        assert!(!settings.certbot.allow_unsafe_without_email);
    }
}
