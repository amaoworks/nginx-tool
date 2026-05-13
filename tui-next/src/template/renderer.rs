//! minijinja 模板渲染器（include_str! 编译期内嵌），详见 architecture.md §12.1.1

use minijinja::{Environment, Template};

/// 编译期内嵌的三种站点模板
const PROXY_TPL: &str = include_str!("../../templates/proxy.conf.j2");
const EMBY_TPL: &str = include_str!("../../templates/emby.conf.j2");
const STATIC_TPL: &str = include_str!("../../templates/static.conf.j2");

/// 站点类型枚举，用于选择模板
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteKind {
    Proxy,
    Emby,
    Static,
}

/// 模板渲染参数
#[derive(Debug, Clone)]
pub struct RenderParams {
    /// 站点名称（仅用于日志，不进入模板）
    pub site_name: String,
    /// server_name 指令内容（域名）
    pub domain_name: String,
    /// 附加域名（空格分隔），用于 server_name 多域名
    pub domain_aliases: String,
    /// 上游协议 http / https（仅代理/Emby）
    pub upstream_scheme: String,
    /// 上游目标地址（仅代理/Emby）
    pub upstream_target: String,
    /// 静态站点根目录（仅静态）
    pub static_root: String,
    /// 托管特性：流式响应 / AI API
    pub feature_streaming: bool,
    /// 托管特性：WebSocket
    pub feature_websocket: bool,
    /// 托管特性：大请求体 / 上传
    pub feature_large_body: bool,
    /// 托管特性：浏览器跨域 CORS
    pub feature_cors: bool,
    /// 托管特性：长超时
    pub feature_long_timeout: bool,
    /// 托管特性：SPA 单页
    pub feature_spa_mode: bool,
    /// 托管特性：静态资源缓存
    pub feature_static_cache: bool,
    /// 托管特性：敏感路径保护
    pub feature_block_sensitive: bool,
    /// server 级注入槽内容
    pub custom_before_location: String,
    /// location 内注入槽内容
    pub custom_inside_location: String,
    /// server 块末尾注入槽内容
    pub custom_after_location: String,
}

impl Default for RenderParams {
    fn default() -> Self {
        Self {
            site_name: String::new(),
            domain_name: String::new(),
            domain_aliases: String::new(),
            upstream_scheme: "http".into(),
            upstream_target: String::new(),
            static_root: String::new(),
            feature_streaming: false,
            feature_websocket: false,
            feature_large_body: false,
            feature_cors: false,
            feature_long_timeout: false,
            feature_spa_mode: false,
            feature_static_cache: true,
            feature_block_sensitive: false,
            custom_before_location: String::new(),
            custom_inside_location: String::new(),
            custom_after_location: String::new(),
        }
    }
}

/// 创建带有三套模板的 minijinja 环境
fn make_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.add_template("proxy", PROXY_TPL)
        .expect("proxy template");
    env.add_template("emby", EMBY_TPL).expect("emby template");
    env.add_template("static", STATIC_TPL)
        .expect("static template");
    env
}

/// 渲染指定类型的站点配置。失败时返回可展示的错误信息。
pub fn render(kind: SiteKind, params: &RenderParams) -> Result<String, String> {
    let env = make_env();
    let tpl_name = match kind {
        SiteKind::Proxy => "proxy",
        SiteKind::Emby => "emby",
        SiteKind::Static => "static",
    };

    let template: Template<'_, '_> = env
        .get_template(tpl_name)
        .map_err(|e| format!("模板加载失败：{}", e))?;

    let ctx = minijinja::Value::from_iter([
        ("domain_name", params.domain_name.clone()),
        ("domain_aliases", params.domain_aliases.clone()),
        ("upstream_scheme", params.upstream_scheme.clone()),
        ("upstream_target", params.upstream_target.clone()),
        ("static_root", params.static_root.clone()),
        ("feature_streaming", params.feature_streaming.to_string()),
        ("feature_websocket", params.feature_websocket.to_string()),
        ("feature_large_body", params.feature_large_body.to_string()),
        ("feature_cors", params.feature_cors.to_string()),
        ("feature_long_timeout", params.feature_long_timeout.to_string()),
        ("feature_spa_mode", params.feature_spa_mode.to_string()),
        ("feature_static_cache", params.feature_static_cache.to_string()),
        (
            "feature_block_sensitive",
            params.feature_block_sensitive.to_string(),
        ),
        (
            "custom_before_location",
            if params.custom_before_location.is_empty() {
                "".into()
            } else {
                params.custom_before_location.clone()
            },
        ),
        (
            "custom_inside_location",
            if params.custom_inside_location.is_empty() {
                "".into()
            } else {
                params.custom_inside_location.clone()
            },
        ),
        (
            "custom_after_location",
            if params.custom_after_location.is_empty() {
                "".into()
            } else {
                params.custom_after_location.clone()
            },
        ),
    ]);

    template
        .render(&ctx)
        .map_err(|e| format!("模板渲染失败：{}", e))
}

/// 从 proxy_pass 解析上游协议和目标地址
/// 输入格式：纯端口 / IP:端口 / http(s)://地址
pub fn parse_upstream(input: &str) -> Result<(String, String), String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("目标地址为空".into());
    }

    // http:// 或 https:// 开头
    if input.starts_with("http://") || input.starts_with("https://") {
        let scheme = if input.starts_with("https://") {
            "https"
        } else {
            "http"
        };
        let target = input.split("://").nth(1).unwrap_or("");
        if target.is_empty() {
            return Err("目标地址无效".into());
        }
        return Ok((scheme.to_string(), target.to_string()));
    }

    // 纯端口 → 补全为 127.0.0.1:端口
    if input.chars().all(|c| c.is_ascii_digit()) {
        let port = input;
        return Ok(("http".to_string(), format!("127.0.0.1:{}", port)));
    }

    // IP:端口 格式
    if input.contains(':') && !input.contains("://") {
        return Ok(("http".to_string(), input.to_string()));
    }

    Err(format!("目标地址格式无效：{}", input))
}

/// 验证站点名称：仅允许字母、数字、连字符
pub fn validate_site_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("站点名称不能为空".into());
    }
    if name.len() > 64 {
        return Err("站点名称过长（最多64字符）".into());
    }
    for ch in name.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '-' {
            return Err(format!(
                "站点名称只能包含字母、数字、连字符，不允许 '{}'",
                ch
            ));
        }
    }
    Ok(())
}

/// 验证域名格式（允许通配符 *.example.com）
pub fn validate_domain(domain: &str) -> Result<(), String> {
    let domain = domain.trim();
    if domain.is_empty() {
        return Err("域名不能为空".into());
    }
    // 基本格式：每个段由字母/数字/连字符组成，用点分隔
    let parts: Vec<&str> = domain.split('.').collect();
    if parts.len() < 2 {
        return Err("域名格式无效，至少需要两段（如 example.com）".into());
    }
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            return Err("域名段不能为空".into());
        }
        // 首段允许 *（通配符）
        if i == 0 && *part == "*" {
            continue;
        }
        for ch in part.chars() {
            if !ch.is_ascii_alphanumeric() && ch != '-' {
                return Err(format!("域名段 '{}' 包含无效字符 '{}'", part, ch));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_proxy_basic() {
        let params = RenderParams {
            domain_name: "app.example.com".into(),
            upstream_scheme: "http".into(),
            upstream_target: "127.0.0.1:8080".into(),
            ..Default::default()
        };
        let out = render(SiteKind::Proxy, &params).unwrap();
        assert!(out.contains("server_name app.example.com;"));
        assert!(out.contains("proxy_pass http://127.0.0.1:8080;"));
        assert!(out.contains("# nginx-tools:custom-before-location:start"));
        assert!(out.contains("# nginx-tools:custom-before-location:end"));
    }

    #[test]
    fn render_emby_with_marker() {
        let params = RenderParams {
            domain_name: "emby.example.com".into(),
            upstream_scheme: "http".into(),
            upstream_target: "192.168.1.5:8096".into(),
            ..Default::default()
        };
        let out = render(SiteKind::Emby, &params).unwrap();
        assert!(out.contains("# nginx-tools:tool-marker: type=emby"));
        assert!(out.contains("proxy_http_version 1.1;"));
        assert!(out.contains("proxy_set_header Upgrade"));
    }

    #[test]
    fn render_static_basic() {
        let params = RenderParams {
            domain_name: "blog.example.com".into(),
            static_root: "/var/www/blog".into(),
            ..Default::default()
        };
        let out = render(SiteKind::Static, &params).unwrap();
        assert!(out.contains("server_name blog.example.com;"));
        assert!(out.contains("root /var/www/blog;"));
        assert!(out.contains("try_files $uri $uri/ =404;"));
        assert!(out.contains("access_log /var/log/nginx/access.log;"));
        assert!(out.contains("error_log /var/log/nginx/error.log;"));
        assert!(out.contains("location ~* \\.(jpg|jpeg|png|gif|ico|css|js|svg|woff|woff2)$ {"));
        assert!(out.contains("expires 30d;"));
    }

    #[test]
    fn render_static_with_aliases() {
        let params = RenderParams {
            domain_name: "example.com".into(),
            domain_aliases: "www.example.com m.example.com".into(),
            static_root: "/var/www/example".into(),
            ..Default::default()
        };
        let out = render(SiteKind::Static, &params).unwrap();
        assert!(out.contains("server_name example.com www.example.com m.example.com;"));
    }

    #[test]
    fn render_proxy_without_aliases() {
        let params = RenderParams {
            domain_name: "app.example.com".into(),
            upstream_scheme: "http".into(),
            upstream_target: "127.0.0.1:8080".into(),
            ..Default::default()
        };
        let out = render(SiteKind::Proxy, &params).unwrap();
        assert!(out.contains("server_name app.example.com;"));
        assert!(!out.contains("server_name app.example.com ;"));
    }

    #[test]
    fn parse_upstream_port_only() {
        let (scheme, target) = parse_upstream("8080").unwrap();
        assert_eq!(scheme, "http");
        assert_eq!(target, "127.0.0.1:8080");
    }

    #[test]
    fn parse_upstream_ip_port() {
        let (scheme, target) = parse_upstream("192.168.1.5:8096").unwrap();
        assert_eq!(scheme, "http");
        assert_eq!(target, "192.168.1.5:8096");
    }

    #[test]
    fn parse_upstream_http_url() {
        let (scheme, target) = parse_upstream("http://backend.local").unwrap();
        assert_eq!(scheme, "http");
        assert_eq!(target, "backend.local");
    }

    #[test]
    fn validate_site_name_ok() {
        assert!(validate_site_name("my-app").is_ok());
        assert!(validate_site_name("app123").is_ok());
    }

    #[test]
    fn validate_site_name_invalid() {
        assert!(validate_site_name("my app").is_err());
        assert!(validate_site_name("app_123").is_err());
        assert!(validate_site_name("").is_err());
    }

    #[test]
    fn validate_domain_ok() {
        assert!(validate_domain("example.com").is_ok());
        assert!(validate_domain("app.example.com").is_ok());
        assert!(validate_domain("*.example.com").is_ok());
    }

    #[test]
    fn validate_domain_invalid() {
        assert!(validate_domain("example").is_err());
        assert!(validate_domain("example..com").is_err());
    }
}
