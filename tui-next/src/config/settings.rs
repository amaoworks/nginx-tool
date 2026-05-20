use serde::{Deserialize, Serialize};

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
