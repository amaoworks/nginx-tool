//! 配置文件解析与注入槽提取，详见 design.md 子模式 C

use std::collections::HashMap;

/// 注入槽标记前缀
const MARKER_PREFIX: &str = "# nginx-tools:custom-";
const MARKER_START_SUFFIX: &str = ":start";
const MARKER_END_SUFFIX: &str = ":end";

/// 注入槽类型
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InjectionSlot {
    /// location 块之前（server 级）
    BeforeLocation,
    /// location 块内部
    InsideLocation,
    /// server 块末尾
    AfterLocation,
}

impl InjectionSlot {
    pub const ALL: [InjectionSlot; 3] = [
        InjectionSlot::BeforeLocation,
        InjectionSlot::InsideLocation,
        InjectionSlot::AfterLocation,
    ];

    pub fn marker_name(&self) -> &'static str {
        match self {
            InjectionSlot::BeforeLocation => "before-location",
            InjectionSlot::InsideLocation => "inside-location",
            InjectionSlot::AfterLocation => "after-location",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            InjectionSlot::BeforeLocation => "Before Location",
            InjectionSlot::InsideLocation => "Inside Location",
            InjectionSlot::AfterLocation => "After Location",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            InjectionSlot::BeforeLocation => "用于放置 server 级通用指令，如 header、gzip、日志等",
            InjectionSlot::InsideLocation => {
                "用于放置 location 级指令，如 proxy_set_header、add_header 等"
            }
            InjectionSlot::AfterLocation => "用于放置附加 location 块、自定义错误页等",
        }
    }
}

/// 编辑器解析结果
#[derive(Debug, Clone)]
pub struct ParsedForEdit {
    /// 原始配置内容
    pub raw_content: String,
    /// 站点类型
    pub site_type: crate::domain::site::SiteType,
    /// 域名列表
    pub domains: Vec<String>,
    /// 上游协议（代理/Emby）
    pub upstream_scheme: Option<String>,
    /// 上游目标（代理/Emby）
    pub upstream_target: Option<String>,
    /// 静态根目录（静态站点）
    pub static_root: Option<String>,
    /// 托管类型标记
    pub managed_type: Option<crate::domain::site::SiteType>,
    /// 托管特性集合
    pub managed_features: Vec<String>,
    /// SSL 证书路径
    pub ssl_cert_path: Option<String>,
    /// SSL 私钥路径
    pub ssl_key_path: Option<String>,
    /// 注入槽内容
    pub injection_slots: HashMap<InjectionSlot, String>,
    /// 注入槽标记是否完整
    pub markers_intact: bool,
}

/// 从配置文件内容解析编辑所需信息
pub fn parse_for_edit(content: &str) -> ParsedForEdit {
    // 基础解析
    let parsed = crate::domain::site::parse_config(content);
    let site_type = crate::domain::site::infer_type(&parsed);

    // 解析 proxy_pass 获取上游信息
    let (upstream_scheme, upstream_target) = parsed
        .proxy_pass
        .as_ref()
        .map(|pp| {
            // proxy_pass 格式：http://target 或 https://target
            if let Some(pos) = pp.find("://") {
                let scheme = &pp[..pos];
                let target = &pp[pos + 3..];
                (Some(scheme.to_string()), Some(target.to_string()))
            } else {
                (None, Some(pp.clone()))
            }
        })
        .unwrap_or((None, None));

    // 提取注入槽内容
    let (injection_slots, markers_intact) = extract_injection_slots(content);
    let (managed_type, managed_features) = extract_managed_metadata(content);
    let (ssl_cert_path, ssl_key_path) = extract_ssl_certificate_paths(content);

    ParsedForEdit {
        raw_content: content.to_string(),
        site_type,
        domains: parsed.server_names,
        upstream_scheme,
        upstream_target,
        static_root: parsed.static_root,
        managed_type,
        managed_features,
        ssl_cert_path,
        ssl_key_path,
        injection_slots,
        markers_intact,
    }
}

fn extract_ssl_certificate_paths(content: &str) -> (Option<String>, Option<String>) {
    let mut cert_path = None;
    let mut key_path = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if cert_path.is_none() {
            if let Some(value) = trimmed.strip_prefix("ssl_certificate ") {
                cert_path = value
                    .split(';')
                    .next()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
            }
        }
        if key_path.is_none() {
            if let Some(value) = trimmed.strip_prefix("ssl_certificate_key ") {
                key_path = value
                    .split(';')
                    .next()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
            }
        }
    }

    (cert_path, key_path)
}

fn extract_managed_metadata(content: &str) -> (Option<crate::domain::site::SiteType>, Vec<String>) {
    let mut managed_type = None;
    let mut features = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("# nginx-tools:managed type=") {
            managed_type = match value.trim() {
                "proxy" => Some(crate::domain::site::SiteType::Proxy),
                "emby" => Some(crate::domain::site::SiteType::Emby),
                "static" => Some(crate::domain::site::SiteType::Static),
                _ => None,
            };
        }
        if trimmed == "# nginx-tools:tool-marker: type=emby" {
            managed_type = Some(crate::domain::site::SiteType::Emby);
        }
        if let Some(value) = trimmed.strip_prefix("# nginx-tools:features=") {
            features = value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect();
        }
    }

    (managed_type, features)
}

/// 从配置文件中提取注入槽内容
pub fn extract_injection_slots(content: &str) -> (HashMap<InjectionSlot, String>, bool) {
    let mut slots = HashMap::new();
    let mut all_markers_intact = true;

    for slot in InjectionSlot::ALL.iter() {
        let start_marker = format!(
            "{}{}{}",
            MARKER_PREFIX,
            slot.marker_name(),
            MARKER_START_SUFFIX
        );
        let end_marker = format!(
            "{}{}{}",
            MARKER_PREFIX,
            slot.marker_name(),
            MARKER_END_SUFFIX
        );

        let start_pos = content.find(&start_marker);
        let end_pos = content.find(&end_marker);

        match (start_pos, end_pos) {
            (Some(start), Some(end)) if start < end => {
                // 提取标记之间的内容
                let content_start = start + start_marker.len();
                let slot_content = content[content_start..end].trim().to_string();
                slots.insert(*slot, slot_content);
            }
            _ => {
                // 标记缺失或顺序错误
                slots.insert(*slot, String::new());
                all_markers_intact = false;
            }
        }
    }

    (slots, all_markers_intact)
}

/// 使用新参数重新渲染配置文件，保留注入槽内容
pub fn rebuild_config(
    kind: crate::template::renderer::SiteKind,
    params: &crate::template::renderer::RenderParams,
) -> Result<String, String> {
    crate::template::renderer::render(kind, params)
}

/// 从原始配置中提取 SSL 相关指令
pub fn extract_ssl_config(content: &str) -> Vec<String> {
    let mut ssl_lines = Vec::new();
    let mut in_server_block = false;
    let mut brace_depth = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        // 跟踪 server 块
        if trimmed.starts_with("server") && trimmed.contains('{') {
            in_server_block = true;
            brace_depth = 1;
            continue;
        }

        if in_server_block {
            // 跟踪大括号深度
            brace_depth += trimmed.matches('{').count() as i32;
            brace_depth -= trimmed.matches('}').count() as i32;

            // 只在 server 块的第一层提取 SSL 指令
            if brace_depth == 1 {
                // 提取 SSL 相关指令
                if trimmed.starts_with("listen")
                    && trimmed.contains("443")
                    && trimmed.contains("ssl")
                    || trimmed.starts_with("ssl_certificate ")
                    || trimmed.starts_with("ssl_certificate_key ")
                    || trimmed.starts_with("ssl_protocols ")
                    || trimmed.starts_with("ssl_ciphers ")
                    || trimmed.starts_with("ssl_prefer_server_ciphers ")
                    || trimmed.starts_with("ssl_session_cache ")
                    || trimmed.starts_with("ssl_session_timeout ")
                    || trimmed.starts_with("ssl_stapling ")
                    || trimmed.starts_with("ssl_stapling_verify ")
                    || trimmed.starts_with("ssl_trusted_certificate ")
                {
                    ssl_lines.push(line.to_string());
                }
            }

            // 退出 server 块
            if brace_depth == 0 {
                in_server_block = false;
            }
        }
    }

    ssl_lines
}

/// 将 SSL 配置注入到渲染后的配置中
pub fn inject_ssl_config(rendered: &str, ssl_lines: &[String]) -> String {
    if ssl_lines.is_empty() {
        return rendered.to_string();
    }

    let mut result = Vec::new();
    let mut in_server_block = false;
    let mut brace_depth = 0;
    let mut ssl_injected = false;

    for line in rendered.lines() {
        let trimmed = line.trim();

        // 检测 server 块开始
        if trimmed.starts_with("server") && trimmed.contains('{') {
            in_server_block = true;
            brace_depth = 1;
            result.push(line.to_string());
            continue;
        }

        if in_server_block {
            // 跟踪大括号深度
            brace_depth += trimmed.matches('{').count() as i32;
            brace_depth -= trimmed.matches('}').count() as i32;

            // 在第一个 listen 80 指令后注入 SSL 配置
            if !ssl_injected
                && brace_depth == 1
                && trimmed.starts_with("listen")
                && trimmed.contains("80")
            {
                result.push(line.to_string());
                // 注入 SSL 配置
                for ssl_line in ssl_lines {
                    result.push(ssl_line.clone());
                }
                ssl_injected = true;
                continue;
            }

            // 退出 server 块
            if brace_depth == 0 {
                in_server_block = false;
            }
        }

        result.push(line.to_string());
    }

    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_for_edit_extracts_upstream() {
        let content = r#"
server {
    listen 80;
    server_name app.example.com;

    # nginx-tools:custom-before-location:start
    add_header X-Frame-Options DENY;
    # nginx-tools:custom-before-location:end

    location / {
        # nginx-tools:custom-inside-location:start
        proxy_set_header X-Real-IP $remote_addr;
        # nginx-tools:custom-inside-location:end
        proxy_pass http://127.0.0.1:8080;
    }

    # nginx-tools:custom-after-location:start
    # nginx-tools:custom-after-location:end
}
"#;
        let edit = parse_for_edit(content);
        assert_eq!(edit.site_type, crate::domain::site::SiteType::Proxy);
        assert_eq!(edit.domains, vec!["app.example.com"]);
        assert_eq!(edit.upstream_scheme, Some("http".to_string()));
        assert_eq!(edit.upstream_target, Some("127.0.0.1:8080".to_string()));
        assert!(edit.markers_intact);
        assert_eq!(
            edit.injection_slots
                .get(&InjectionSlot::BeforeLocation)
                .map(|s| s.trim()),
            Some("add_header X-Frame-Options DENY;")
        );
        assert_eq!(
            edit.injection_slots
                .get(&InjectionSlot::InsideLocation)
                .map(|s| s.trim()),
            Some("proxy_set_header X-Real-IP $remote_addr;")
        );
    }

    #[test]
    fn parse_for_edit_detects_missing_markers() {
        let content = r#"
server {
    listen 80;
    server_name app.example.com;
    location / {
        proxy_pass http://127.0.0.1:8080;
    }
}
"#;
        let edit = parse_for_edit(content);
        assert!(!edit.markers_intact);
        assert!(edit
            .injection_slots
            .get(&InjectionSlot::BeforeLocation)
            .map_or(true, |s| s.is_empty()));
    }

    #[test]
    fn parse_for_edit_static_site() {
        let content = r#"
server {
    listen 80;
    server_name blog.example.com;
    root /var/www/blog;

    # nginx-tools:custom-before-location:start
    # nginx-tools:custom-before-location:end

    location / {
        try_files $uri $uri/ =404;
    }
}
"#;
        let edit = parse_for_edit(content);
        assert_eq!(edit.site_type, crate::domain::site::SiteType::Static);
        assert_eq!(edit.static_root, Some("/var/www/blog".to_string()));
        assert!(edit.upstream_scheme.is_none());
        assert!(edit.upstream_target.is_none());
    }

    #[test]
    fn parse_for_edit_extracts_managed_features() {
        let content = r#"
server {
    listen 80;
    server_name api.example.com;
    # nginx-tools:managed type=proxy
    # nginx-tools:features=streaming,websocket,long_timeout,
    location / {
        proxy_pass http://127.0.0.1:8080;
    }
}
"#;
        let edit = parse_for_edit(content);
        assert_eq!(
            edit.managed_type,
            Some(crate::domain::site::SiteType::Proxy)
        );
        assert_eq!(
            edit.managed_features,
            vec![
                "streaming".to_string(),
                "websocket".to_string(),
                "long_timeout".to_string()
            ]
        );
    }

    #[test]
    fn extract_ssl_config_finds_ssl_directives() {
        let content = r#"
server {
    listen 80;
    listen 443 ssl;
    server_name app.example.com;

    ssl_certificate /etc/letsencrypt/live/app.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/app.example.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;

    location / {
        proxy_pass http://127.0.0.1:8080;
    }
}
"#;
        let ssl_lines = extract_ssl_config(content);
        assert_eq!(ssl_lines.len(), 4);
        assert!(ssl_lines.iter().any(|l| l.contains("listen 443 ssl")));
        assert!(ssl_lines.iter().any(|l| l.contains("ssl_certificate ")));
        assert!(ssl_lines.iter().any(|l| l.contains("ssl_certificate_key")));
        assert!(ssl_lines.iter().any(|l| l.contains("ssl_protocols")));
    }

    #[test]
    fn parse_for_edit_extracts_ssl_certificate_paths() {
        let content = r#"
server {
    listen 443 ssl;
    server_name app.example.com;
    ssl_certificate /etc/letsencrypt/live/app/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/app/privkey.pem;
}
"#;
        let edit = parse_for_edit(content);
        assert_eq!(
            edit.ssl_cert_path.as_deref(),
            Some("/etc/letsencrypt/live/app/fullchain.pem")
        );
        assert_eq!(
            edit.ssl_key_path.as_deref(),
            Some("/etc/letsencrypt/live/app/privkey.pem")
        );
    }

    #[test]
    fn inject_ssl_config_adds_ssl_after_listen_80() {
        let rendered = r#"server {
    listen 80;
    server_name app.example.com;

    location / {
        proxy_pass http://127.0.0.1:8080;
    }
}"#;
        let ssl_lines = vec![
            "    listen 443 ssl;".to_string(),
            "    ssl_certificate /etc/letsencrypt/live/app.example.com/fullchain.pem;".to_string(),
            "    ssl_certificate_key /etc/letsencrypt/live/app.example.com/privkey.pem;"
                .to_string(),
        ];

        let result = inject_ssl_config(rendered, &ssl_lines);
        assert!(result.contains("listen 80"));
        assert!(result.contains("listen 443 ssl"));
        assert!(result.contains("ssl_certificate"));
        assert!(result.contains("ssl_certificate_key"));

        // 验证 SSL 配置在 listen 80 之后
        let listen_80_pos = result.find("listen 80").unwrap();
        let listen_443_pos = result.find("listen 443 ssl").unwrap();
        assert!(listen_443_pos > listen_80_pos);
    }
}
