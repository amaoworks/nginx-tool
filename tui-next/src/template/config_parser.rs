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

    ParsedForEdit {
        raw_content: content.to_string(),
        site_type,
        domains: parsed.server_names,
        upstream_scheme,
        upstream_target,
        static_root: parsed.static_root,
        injection_slots,
        markers_intact,
    }
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
}
