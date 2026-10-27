//! minijinja 模板渲染器（include_str! 编译期内嵌），详见 architecture.md §12.1.1

use super::site_tuning::{directives, GzipMode, Http2Support, SiteTuning};
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
    pub tuning: SiteTuning,
    pub http2_support: Http2Support,
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
    /// 是否启用 SSL
    pub ssl_enabled: bool,
    /// SSL 证书路径
    pub ssl_cert_path: String,
    /// SSL 私钥路径
    pub ssl_key_path: String,
}

impl Default for RenderParams {
    fn default() -> Self {
        Self {
            tuning: SiteTuning::default(),
            http2_support: Http2Support::Unknown,
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
            ssl_enabled: false,
            ssl_cert_path: String::new(),
            ssl_key_path: String::new(),
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
    let tuning = params.tuning.normalized(kind != SiteKind::Static)?;
    let (http2_listen, http2_directive) =
        tuning.http2_directives(params.ssl_enabled, params.http2_support)?;
    let mut body_size = tuning.body_size.trim_end_matches('b').to_string();
    if body_size.is_empty() && params.feature_large_body && kind == SiteKind::Proxy {
        body_size = "50m".into();
    }
    let mut connect_timeout = tuning.connect_timeout.clone();
    let mut send_timeout = tuning.send_timeout.clone();
    let mut read_timeout = tuning.read_timeout.clone();
    if params.feature_long_timeout && kind == SiteKind::Proxy {
        if connect_timeout.is_empty() {
            connect_timeout = "60s".into();
        }
        if send_timeout.is_empty() {
            send_timeout = "300s".into();
        }
    }
    // Custom root-location timeouts take precedence over implicit streaming defaults.
    let custom_read = directives(&params.custom_inside_location)
        .iter()
        .any(|d| d.scope.is_empty() && d.name == "proxy_read_timeout");
    if read_timeout.is_empty() && !custom_read && kind == SiteKind::Proxy {
        if params.feature_websocket || params.feature_streaming {
            read_timeout = "86400s".into();
        } else if params.feature_long_timeout {
            read_timeout = "300s".into();
        }
    }
    for (slot, content) in [
        ("server", &params.custom_before_location),
        ("server", &params.custom_after_location),
        ("location", &params.custom_inside_location),
    ] {
        for d in directives(content).iter().filter(|d| d.scope.is_empty()) {
            let conflict = match d.name.as_str() {
                "http2" => {
                    slot == "server"
                        && params.ssl_enabled
                        && tuning.http2 != super::site_tuning::Http2Mode::Inherit
                }
                "listen" => {
                    slot == "server"
                        && params.ssl_enabled
                        && d.value.split_whitespace().any(|v| v == "http2")
                        && tuning.http2 != super::site_tuning::Http2Mode::Inherit
                }
                "gzip" | "gzip_vary" | "gzip_min_length" | "gzip_types" | "gzip_comp_level" => {
                    slot == "server" && tuning.gzip != GzipMode::Inherit
                }
                "client_max_body_size" => slot == "server" && !body_size.is_empty(),
                "proxy_connect_timeout" => slot == "location" && !connect_timeout.is_empty(),
                "proxy_send_timeout" => slot == "location" && !send_timeout.is_empty(),
                "proxy_read_timeout" => slot == "location" && !read_timeout.is_empty(),
                _ => false,
            };
            if conflict {
                return Err(format!(
                    "{} 已在自定义槽中设置；请移除槽内重复指令，或将表单对应设置留空/设为继承",
                    d.name
                ));
            }
        }
    }
    let tuning_json = serde_json::to_string(&tuning).map_err(|e| e.to_string())?;
    let gzip_mode = match tuning.gzip {
        GzipMode::Inherit => "inherit",
        GzipMode::On => "on",
        GzipMode::Off => "off",
    };
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
        ("tuning_json", tuning_json),
        ("http2_listen", http2_listen.to_string()),
        ("http2_directive", http2_directive.to_string()),
        ("gzip_mode", gzip_mode.to_string()),
        ("body_size", body_size),
        ("connect_timeout", connect_timeout),
        ("send_timeout", send_timeout),
        ("read_timeout", read_timeout),
        ("site_name", params.site_name.clone()),
        ("domain_name", params.domain_name.clone()),
        ("domain_aliases", params.domain_aliases.clone()),
        ("upstream_scheme", params.upstream_scheme.clone()),
        ("upstream_target", params.upstream_target.clone()),
        ("static_root", params.static_root.clone()),
        ("feature_streaming", params.feature_streaming.to_string()),
        ("feature_websocket", params.feature_websocket.to_string()),
        ("feature_large_body", params.feature_large_body.to_string()),
        ("feature_cors", params.feature_cors.to_string()),
        (
            "feature_long_timeout",
            params.feature_long_timeout.to_string(),
        ),
        ("feature_spa_mode", params.feature_spa_mode.to_string()),
        (
            "feature_static_cache",
            params.feature_static_cache.to_string(),
        ),
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
        ("ssl_enabled", params.ssl_enabled.to_string()),
        ("ssl_cert_path", params.ssl_cert_path.clone()),
        ("ssl_key_path", params.ssl_key_path.clone()),
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
    use crate::template::site_tuning::{parse_tuning, Http2Mode};

    #[test]
    fn tuning_round_trip_and_https_upgrade_for_all_templates() -> Result<(), String> {
        for kind in [SiteKind::Proxy, SiteKind::Emby, SiteKind::Static] {
            let mut params = RenderParams {
                site_name: "example".into(),
                domain_name: "example.com".into(),
                tuning: SiteTuning {
                    gzip: GzipMode::On,
                    body_size: "1024b".into(),
                    connect_timeout: "500ms".into(),
                    send_timeout: "5m".into(),
                    read_timeout: "900".into(),
                    ..Default::default()
                },
                http2_support: Http2Support::Modern,
                ..Default::default()
            };
            let http = render(kind, &params)?;
            assert!(!http.contains("http2 on;"));
            params.tuning = parse_tuning(&http);
            assert_eq!(params.tuning.http2, Http2Mode::Auto);
            params.ssl_enabled = true;
            params.ssl_cert_path = "/tmp/cert".into();
            let https = render(kind, &params)?;
            assert!(https.contains("http2 on;"));
            assert!(https.contains("gzip on;"));
            if kind == SiteKind::Static {
                assert!(!https.contains("proxy_read_timeout"));
                assert!(!https.contains("client_max_body_size"));
            } else {
                assert!(https.contains("client_max_body_size 1024;"));
                assert!(https.contains("proxy_connect_timeout 500ms;"));
                assert!(https.contains("proxy_send_timeout 5m;"));
                assert!(https.contains("proxy_read_timeout 900s;"));
            }
            let reloaded = parse_tuning(&https);
            assert_eq!(reloaded, params.tuning);
            params.tuning = reloaded;
            assert_eq!(render(kind, &params)?, https);
        }
        Ok(())
    }

    #[test]
    fn explicit_timeout_overrides_streaming_but_empty_tracks_preset() -> Result<(), String> {
        let mut params = RenderParams {
            feature_streaming: true,
            ..Default::default()
        };
        let content = render(SiteKind::Proxy, &params)?;
        assert!(content.contains("proxy_read_timeout 86400s;"));
        params.tuning = parse_tuning(&content);
        assert!(params.tuning.read_timeout.is_empty());
        params.feature_streaming = false;
        assert!(!render(SiteKind::Proxy, &params)?.contains("proxy_read_timeout"));
        params.feature_streaming = true;
        params.tuning.read_timeout = "300".into();
        let content = render(SiteKind::Proxy, &params)?;
        assert!(content.contains("proxy_read_timeout 300s;"));
        assert_eq!(content.matches("proxy_read_timeout").count(), 1);
        Ok(())
    }

    #[test]
    fn legacy_feature_values_migrate_and_can_be_cleared() -> Result<(), String> {
        let old = "server {\n# nginx-tools:features=large_body,long_timeout,\nclient_max_body_size 50m;\nlocation / { proxy_read_timeout 300s; proxy_send_timeout 300s; proxy_connect_timeout 60s; }\n}";
        let mut params = RenderParams {
            tuning: parse_tuning(old),
            ..Default::default()
        };
        assert_eq!(params.tuning.body_size, "50m");
        assert_eq!(params.tuning.read_timeout, "300s");
        assert_eq!(params.tuning.http2, Http2Mode::Inherit);
        params.tuning.body_size.clear();
        params.tuning.read_timeout.clear();
        let rendered = render(SiteKind::Proxy, &params)?;
        assert!(!rendered.contains("client_max_body_size"));
        assert!(!rendered.contains("proxy_read_timeout"));
        assert!(rendered.contains("proxy_send_timeout 300s;"));
        Ok(())
    }

    #[test]
    fn custom_slots_are_preserved_and_duplicate_directives_are_rejected() -> Result<(), String> {
        let mut params = RenderParams {
            custom_before_location: "gzip on;".into(),
            custom_inside_location: "proxy_read_timeout 7s;".into(),
            feature_streaming: true,
            ..Default::default()
        };
        let content = render(SiteKind::Proxy, &params)?;
        assert_eq!(content.matches("proxy_read_timeout").count(), 1);
        assert!(parse_tuning(&content).read_timeout.is_empty());
        params.tuning.gzip = GzipMode::On;
        assert!(render(SiteKind::Proxy, &params).is_err());
        params.tuning.gzip = GzipMode::Inherit;
        params.tuning.read_timeout = "10".into();
        assert!(render(SiteKind::Proxy, &params).is_err());
        params.custom_inside_location.clear();
        params.custom_after_location = "location /other { proxy_read_timeout 7s; }".into();
        assert!(render(SiteKind::Proxy, &params).is_ok());
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires local nginx and openssl; uses only temporary files"]
    async fn generated_configs_pass_local_nginx_validation() -> anyhow::Result<()> {
        use crate::infra::{executor::CommandExecutor, nginx::NginxAdapter};
        use std::process::Command;
        let temp = tempfile::tempdir()?;
        let cert = temp.path().join("cert.pem");
        let key = temp.path().join("key.pem");
        let certificate = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=example.com",
                "-keyout",
            ])
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .output()?;
        anyhow::ensure!(
            certificate.status.success(),
            "{}",
            String::from_utf8_lossy(&certificate.stderr)
        );
        let nginx = NginxAdapter::new(CommandExecutor::new(
            std::time::Duration::from_secs(3),
            false,
        ));
        let support = nginx.refresh_http2_support().await;
        anyhow::ensure!(
            matches!(support, Http2Support::Legacy | Http2Support::Modern),
            "HTTP/2 capability unavailable"
        );
        for kind in [SiteKind::Proxy, SiteKind::Emby, SiteKind::Static] {
            let params = RenderParams {
                site_name: "validation".into(),
                domain_name: "example.com".into(),
                static_root: temp.path().display().to_string(),
                upstream_target: "127.0.0.1:8080".into(),
                ssl_enabled: true,
                ssl_cert_path: cert.display().to_string(),
                ssl_key_path: key.display().to_string(),
                http2_support: support,
                tuning: SiteTuning {
                    gzip: GzipMode::On,
                    body_size: "1g".into(),
                    connect_timeout: "500ms".into(),
                    send_timeout: "5m".into(),
                    read_timeout: "900".into(),
                    ..Default::default()
                },
                ..Default::default()
            };
            let content = render(kind, &params).map_err(anyhow::Error::msg)?;
            // Certbot support files and site logs are external to the directives under test.
            let standalone = content
                .lines()
                .filter(|line| {
                    !line.contains("/etc/letsencrypt/") && !line.contains("/var/log/nginx/")
                })
                .collect::<Vec<_>>()
                .join("\n");
            let config = temp.path().join("nginx.conf");
            std::fs::write(&config, format!("user root;\npid {0}/nginx.pid;\nerror_log stderr;\nevents {{}}\nhttp {{ access_log off; client_body_temp_path {0}/body; proxy_temp_path {0}/proxy; fastcgi_temp_path {0}/fastcgi; uwsgi_temp_path {0}/uwsgi; scgi_temp_path {0}/scgi;\n{1}\n}}", temp.path().display(), standalone))?;
            let result = Command::new("nginx")
                .args(["-t", "-e", "stderr", "-p"])
                .arg(temp.path())
                .arg("-c")
                .arg(&config)
                .output()?;
            anyhow::ensure!(
                result.status.success(),
                "{kind:?}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        Ok(())
    }

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
            site_name: "blog".into(),
            domain_name: "blog.example.com".into(),
            static_root: "/var/www/blog".into(),
            ..Default::default()
        };
        let out = render(SiteKind::Static, &params).unwrap();
        assert!(out.contains("server_name blog.example.com;"));
        assert!(out.contains("root /var/www/blog;"));
        assert!(out.contains("try_files $uri $uri/ =404;"));
        assert!(out.contains("access_log /var/log/nginx/blog.access.log;"));
        assert!(out.contains("error_log /var/log/nginx/blog.error.log;"));
        assert!(out.contains("location ~* \\.(jpg|jpeg|png|gif|ico|css|js|svg|woff|woff2)$ {"));
        assert!(out.contains("expires 30d;"));
    }

    #[test]
    fn render_static_with_aliases() {
        let params = RenderParams {
            site_name: "example".into(),
            domain_name: "example.com".into(),
            domain_aliases: "www.example.com m.example.com".into(),
            static_root: "/var/www/example".into(),
            ..Default::default()
        };
        let out = render(SiteKind::Static, &params).unwrap();
        assert!(out.contains("server_name example.com www.example.com m.example.com;"));
    }

    #[test]
    fn render_proxy_ssl_redirect_includes_aliases() {
        let params = RenderParams {
            site_name: "app".into(),
            domain_name: "app.example.com".into(),
            domain_aliases: "www.app.example.com".into(),
            upstream_scheme: "http".into(),
            upstream_target: "127.0.0.1:8080".into(),
            ssl_enabled: true,
            ssl_cert_path: "/etc/letsencrypt/live/app/fullchain.pem".into(),
            ssl_key_path: "/etc/letsencrypt/live/app/privkey.pem".into(),
            ..Default::default()
        };
        let out = render(SiteKind::Proxy, &params).unwrap();
        assert!(out.contains("listen 443 ssl;"));
        assert!(out.contains("server_name app.example.com www.app.example.com;"));
        assert!(out.contains("return 301 https://$host$request_uri;"));
        assert!(!out.contains("return 404;"));
    }

    #[test]
    fn render_proxy_without_aliases() {
        let params = RenderParams {
            site_name: "app".into(),
            domain_name: "app.example.com".into(),
            upstream_scheme: "http".into(),
            upstream_target: "127.0.0.1:8080".into(),
            ..Default::default()
        };
        let out = render(SiteKind::Proxy, &params).unwrap();
        assert!(out.contains("server_name app.example.com;"));
        assert!(out.contains("access_log /var/log/nginx/app.access.log;"));
        assert!(out.contains("error_log /var/log/nginx/app.error.log;"));
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
