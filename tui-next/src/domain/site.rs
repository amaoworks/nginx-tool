//! 站点配置解析与领域模型，对应 design.md 子模式 A、architecture.md §11.3。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;

use crate::error::NgToolError;
use crate::infra::audit::AuditResult;
use crate::infra::executor::CommandSpec;
use crate::infra::nginx::scan_sites;
use crate::infra::AppContext;

/// 站点类型：与 design.md 子模式 A 类型列对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SiteType {
    Proxy,
    Emby,
    Static,
    Unknown,
}

impl SiteType {
    pub fn label(&self) -> &'static str {
        match self {
            SiteType::Proxy => "代理",
            SiteType::Emby => "Emby",
            SiteType::Static => "静态",
            SiteType::Unknown => "未知",
        }
    }
}

/// SSL 状态。详见 design.md 子模式 A SSL 列定义。
#[derive(Debug, Clone, Serialize)]
pub enum SslStatus {
    None,
    Active { days_left: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslLevel {
    None,
    Ok,
    Warning,
    Critical,
}

impl SslStatus {
    pub fn level(&self) -> SslLevel {
        match self {
            SslStatus::None => SslLevel::None,
            SslStatus::Active { days_left } if *days_left < 7 => SslLevel::Critical,
            SslStatus::Active { days_left } if *days_left < 30 => SslLevel::Warning,
            SslStatus::Active { .. } => SslLevel::Ok,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Site {
    pub name: String,
    pub primary_domain: Option<String>,
    pub all_domains: Vec<String>,
    pub access_log_path: Option<PathBuf>,
    pub error_log_path: Option<PathBuf>,
    pub site_type: SiteType,
    pub target: Option<String>,
    pub enabled: bool,
    pub ssl: SslStatus,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedConfig {
    pub server_names: Vec<String>,
    pub proxy_pass: Option<String>,
    pub static_root: Option<String>,
    pub access_log_path: Option<PathBuf>,
    pub error_log_path: Option<PathBuf>,
    pub has_emby_marker: bool,
}

/// 解析单个站点 conf 文件，提取核心指令。
/// 注释行被剥离后再做正则匹配；emby 标记基于工具注释直接判断。
pub fn parse_config(text: &str) -> ParsedConfig {
    let cleaned: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");

    let directive_re =
        regex::Regex::new(r"(?m)^\s*(server_name|proxy_pass|root|access_log|error_log)\s+([^;]+);")
            .unwrap();

    let mut server_names: Vec<String> = Vec::new();
    let mut proxy_pass: Option<String> = None;
    let mut static_root: Option<String> = None;
    let mut access_log_path: Option<PathBuf> = None;
    let mut error_log_path: Option<PathBuf> = None;

    for cap in directive_re.captures_iter(&cleaned) {
        let name = &cap[1];
        let value = cap[2].trim();
        match name {
            "server_name" => {
                for d in value.split_whitespace() {
                    if !server_names.iter().any(|existing| existing == d) {
                        server_names.push(d.to_string());
                    }
                }
            }
            "proxy_pass" if proxy_pass.is_none() => {
                proxy_pass = Some(value.to_string());
            }
            "root" if static_root.is_none() => {
                static_root = Some(value.to_string());
            }
            "access_log" if access_log_path.is_none() => {
                if let Some(path) = value.split_whitespace().next() {
                    if !path.eq_ignore_ascii_case("off") {
                        access_log_path = Some(PathBuf::from(path));
                    }
                }
            }
            "error_log" if error_log_path.is_none() => {
                if let Some(path) = value.split_whitespace().next() {
                    if !path.eq_ignore_ascii_case("off") {
                        error_log_path = Some(PathBuf::from(path));
                    }
                }
            }
            _ => {}
        }
    }

    let has_emby_marker = text.contains("nginx-tools:tool-marker") && text.contains("type=emby");

    ParsedConfig {
        server_names,
        proxy_pass,
        static_root,
        access_log_path,
        error_log_path,
        has_emby_marker,
    }
}

pub fn infer_type(parsed: &ParsedConfig) -> SiteType {
    if parsed.has_emby_marker {
        return SiteType::Emby;
    }
    if parsed.proxy_pass.is_some() {
        return SiteType::Proxy;
    }
    if parsed.static_root.is_some() {
        return SiteType::Static;
    }
    SiteType::Unknown
}

/// 从 certbot certificates 的输出解析 域名 → 剩余天数 映射。
pub fn parse_certbot_domains(text: &str) -> HashMap<String, i64> {
    let mut map: HashMap<String, i64> = HashMap::new();
    let domains_re = regex::Regex::new(r"(?m)^\s*Domains:\s*(.+)$").unwrap();
    let valid_re = regex::Regex::new(r"VALID:\s*(\d+)\s*day").unwrap();

    let mut last_domains: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(cap) = domains_re.captures(line) {
            last_domains = cap[1].split_whitespace().map(String::from).collect();
            continue;
        }
        if let Some(cap) = valid_re.captures(line) {
            if let Ok(days) = cap[1].parse::<i64>() {
                for d in &last_domains {
                    map.insert(d.clone(), days);
                }
                last_domains.clear();
            }
        }
    }
    map
}

/// 列出全部站点。证书匹配失败不阻断列表输出（架构 §11.3 / R2 闭环）。
pub async fn list_sites(ctx: Arc<AppContext>) -> Result<Vec<Site>, NgToolError> {
    let avail = ctx.probe.sites_available.clone();
    let enabled = ctx.probe.sites_enabled.clone();

    let raws = tokio::task::spawn_blocking(move || scan_sites(&avail, &enabled))
        .await
        .map_err(|e| NgToolError::FileOperationFailed {
            path: ctx.probe.sites_available.clone(),
            message: format!("任务异常：{}", e),
        })?
        .map_err(|e| NgToolError::FileOperationFailed {
            path: ctx.probe.sites_available.clone(),
            message: e.to_string(),
        })?;

    // 证书域名 → 剩余天数
    let cert_map = if ctx.deps().certbot {
        match ctx
            .executor
            .run(
                CommandSpec::new("certbot")
                    .arg("certificates")
                    .timeout(Duration::from_secs(3)),
            )
            .await
        {
            Ok(out) => parse_certbot_domains(&out.stdout),
            Err(_) => HashMap::new(),
        }
    } else {
        HashMap::new()
    };

    let mut sites = Vec::with_capacity(raws.len());
    for raw in raws {
        let content = std::fs::read_to_string(&raw.path).unwrap_or_default();
        let parsed = parse_config(&content);
        let site_type = infer_type(&parsed);
        let primary_domain = parsed.server_names.first().cloned();
        let target = match site_type {
            SiteType::Static => parsed.static_root.clone(),
            _ => parsed.proxy_pass.clone(),
        };
        let ssl = match &primary_domain {
            Some(d) => match cert_map.get(d) {
                Some(days) => SslStatus::Active { days_left: *days },
                None => SslStatus::None,
            },
            None => SslStatus::None,
        };
        sites.push(Site {
            name: raw.name,
            primary_domain,
            all_domains: parsed.server_names,
            access_log_path: parsed.access_log_path,
            error_log_path: parsed.error_log_path,
            site_type,
            target,
            enabled: raw.enabled,
            ssl,
            config_path: raw.path,
        });
    }
    Ok(sites)
}

/// 删除站点：若已启用则先停用 → 删除 sites-available 配置文件 → nginx -t → reload。
/// 任一步失败时中止（已停用的不自动恢复，已删除的文件不可恢复）。
pub async fn delete_site(ctx: Arc<AppContext>, name: &str) -> Result<(), NgToolError> {
    let started = Instant::now();
    let conf_name = format!("{}.conf", name);
    let avail_path = ctx.probe.sites_available.join(&conf_name);
    let link_path = ctx.probe.sites_enabled.join(&conf_name);

    if !avail_path.exists() {
        return Err(NgToolError::FileOperationFailed {
            path: avail_path,
            message: "站点配置不存在".into(),
        });
    }

    // 若已启用：先停用（删链接 + nginx -t + reload）
    if link_path.symlink_metadata().is_ok() {
        disable(ctx.clone(), name).await?;
    }

    // 删除配置文件
    let ap = avail_path.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || std::fs::remove_file(&ap))
        .await
        .map_err(|e| NgToolError::FileOperationFailed {
            path: avail_path.clone(),
            message: format!("任务异常：{}", e),
        })?
    {
        log_audit(
            &ctx,
            "site.delete",
            name,
            AuditResult::Failure,
            started,
            json!({"stage": "remove_config", "error": e.to_string()}),
        );
        return Err(NgToolError::FileOperationFailed {
            path: avail_path,
            message: e.to_string(),
        });
    }

    // nginx -t（配置已删除，确保剩余配置仍合法）
    if let Err(e) = ctx.nginx.test_config().await {
        log_audit(
            &ctx,
            "site.delete",
            name,
            AuditResult::Failure,
            started,
            json!({"stage": "test", "error": e.to_string()}),
        );
        return Err(e);
    }

    // reload
    if let Err(e) = ctx.systemd.reload("nginx").await {
        log_audit(
            &ctx,
            "site.delete",
            name,
            AuditResult::Failure,
            started,
            json!({"stage": "reload", "error": e.to_string()}),
        );
        return Err(e);
    }

    log_audit(
        &ctx,
        "site.delete",
        name,
        AuditResult::Success,
        started,
        json!({}),
    );
    Ok(())
}

/// 启用站点：建链接 → nginx -t → reload。任一步失败按反向次序回滚。
/// 详见 architecture.md §15.3 启用流程。
pub async fn enable(ctx: Arc<AppContext>, name: &str) -> Result<(), NgToolError> {
    let started = Instant::now();
    let avail = ctx.probe.sites_available.join(format!("{}.conf", name));
    let link = ctx.probe.sites_enabled.join(format!("{}.conf", name));

    // 已启用：幂等
    if link.symlink_metadata().is_ok() {
        log_audit(
            &ctx,
            "site.enable",
            name,
            AuditResult::Success,
            started,
            json!({"already": true}),
        );
        return Ok(());
    }
    if !avail.exists() {
        return Err(NgToolError::FileOperationFailed {
            path: avail,
            message: "站点配置不存在".into(),
        });
    }

    // 创建符号链接
    if let Err(e) = std::os::unix::fs::symlink(&avail, &link) {
        let err = NgToolError::FileOperationFailed {
            path: link.clone(),
            message: format!("创建符号链接失败：{}", e),
        };
        log_audit(
            &ctx,
            "site.enable",
            name,
            AuditResult::Failure,
            started,
            json!({"stage": "symlink", "error": err.to_string()}),
        );
        return Err(err);
    }

    // nginx -t
    if let Err(e) = ctx.nginx.test_config().await {
        let _ = std::fs::remove_file(&link);
        // 二次 test 用于校验回滚后状态，但不影响错误返回
        let _ = ctx.nginx.test_config().await;
        log_audit(
            &ctx,
            "site.enable",
            name,
            AuditResult::Failure,
            started,
            json!({"stage": "test", "error": e.to_string()}),
        );
        return Err(e);
    }

    // reload
    if let Err(e) = ctx.systemd.reload("nginx").await {
        let _ = std::fs::remove_file(&link);
        let _ = ctx.nginx.test_config().await;
        log_audit(
            &ctx,
            "site.enable",
            name,
            AuditResult::Failure,
            started,
            json!({"stage": "reload", "error": e.to_string()}),
        );
        return Err(e);
    }

    log_audit(
        &ctx,
        "site.enable",
        name,
        AuditResult::Success,
        started,
        json!({}),
    );
    Ok(())
}

/// 停用站点：记录原 target → 删链接 → nginx -t → reload。
/// 任一步失败时依次按反向恢复。详见 architecture.md §15.3 停用流程。
pub async fn disable(ctx: Arc<AppContext>, name: &str) -> Result<(), NgToolError> {
    let started = Instant::now();
    let link = ctx.probe.sites_enabled.join(format!("{}.conf", name));

    if link.symlink_metadata().is_err() {
        log_audit(
            &ctx,
            "site.disable",
            name,
            AuditResult::Success,
            started,
            json!({"already": true}),
        );
        return Ok(());
    }

    let original_target = link.read_link().ok();

    if let Err(e) = std::fs::remove_file(&link) {
        let err = NgToolError::FileOperationFailed {
            path: link.clone(),
            message: e.to_string(),
        };
        log_audit(
            &ctx,
            "site.disable",
            name,
            AuditResult::Failure,
            started,
            json!({"stage": "unlink", "error": err.to_string()}),
        );
        return Err(err);
    }

    if let Err(e) = ctx.nginx.test_config().await {
        if let Some(target) = &original_target {
            let _ = std::os::unix::fs::symlink(target, &link);
            let _ = ctx.nginx.test_config().await;
        }
        log_audit(
            &ctx,
            "site.disable",
            name,
            AuditResult::Failure,
            started,
            json!({"stage": "test", "error": e.to_string()}),
        );
        return Err(e);
    }

    if let Err(e) = ctx.systemd.reload("nginx").await {
        if let Some(target) = &original_target {
            let _ = std::os::unix::fs::symlink(target, &link);
        }
        log_audit(
            &ctx,
            "site.disable",
            name,
            AuditResult::Failure,
            started,
            json!({"stage": "reload", "error": e.to_string()}),
        );
        return Err(e);
    }

    log_audit(
        &ctx,
        "site.disable",
        name,
        AuditResult::Success,
        started,
        json!({}),
    );
    Ok(())
}

fn log_audit(
    ctx: &AppContext,
    action: &str,
    target: &str,
    result: AuditResult,
    started: Instant,
    details: serde_json::Value,
) {
    ctx.audit.log(
        action,
        target,
        result,
        started.elapsed().as_millis() as u64,
        details,
    );
}

/// 新建站点输入参数
#[derive(Debug, Clone)]
pub struct CreateSiteInput {
    pub name: String,
    pub domain: String,
    pub domain_aliases: String,
    pub kind: crate::template::renderer::SiteKind,
    pub upstream_scheme: String,
    pub upstream_target: String,
    pub static_root: String,
    pub feature_streaming: bool,
    pub feature_websocket: bool,
    pub feature_large_body: bool,
    pub feature_cors: bool,
    pub feature_long_timeout: bool,
    pub feature_spa_mode: bool,
    pub feature_static_cache: bool,
    pub feature_block_sensitive: bool,
    pub enable_now: bool,
    pub request_cert: bool,
}

/// 保存已有站点配置的输入参数。
#[derive(Debug, Clone)]
pub struct SaveSiteInput {
    pub name: String,
    pub content: String,
    /// true = 写入后执行 nginx -t 并 reload；失败时恢复原文件。
    pub test_and_reload: bool,
    /// 进入编辑时记录的目标文件 mtime，用于 §15.0 mtime 并发保护。
    /// `None` 表示不做并发校验（如刚创建后立即保存的极端情况）。
    pub expected_mtime: Option<SystemTime>,
}

/// 新建站点结果
#[derive(Debug, Clone)]
pub enum CreateSiteOutcome {
    /// 全部成功
    Ok { cert_requested: bool },
    /// 站点创建成功但证书申请失败
    CertFailed { error: String },
}

fn cert_paths_for_name(cert_name: &str) -> (String, String) {
    (
        format!("/etc/letsencrypt/live/{}/fullchain.pem", cert_name),
        format!("/etc/letsencrypt/live/{}/privkey.pem", cert_name),
    )
}

fn render_create_site_config(
    input: &CreateSiteInput,
    ssl: Option<(&str, &str)>,
) -> Result<String, NgToolError> {
    let params = crate::template::renderer::RenderParams {
        site_name: input.name.clone(),
        domain_name: input.domain.clone(),
        domain_aliases: input.domain_aliases.clone(),
        upstream_scheme: input.upstream_scheme.clone(),
        upstream_target: input.upstream_target.clone(),
        static_root: input.static_root.clone(),
        feature_streaming: input.feature_streaming,
        feature_websocket: input.feature_websocket,
        feature_large_body: input.feature_large_body,
        feature_cors: input.feature_cors,
        feature_long_timeout: input.feature_long_timeout,
        feature_spa_mode: input.feature_spa_mode,
        feature_static_cache: input.feature_static_cache,
        feature_block_sensitive: input.feature_block_sensitive,
        ssl_enabled: ssl.is_some(),
        ssl_cert_path: ssl.map(|(cert, _)| cert.to_string()).unwrap_or_default(),
        ssl_key_path: ssl.map(|(_, key)| key.to_string()).unwrap_or_default(),
        ..Default::default()
    };
    crate::template::renderer::render(input.kind, &params)
        .map_err(|e| NgToolError::TemplateFailed { message: e })
}

/// 新建站点：渲染模板 → 写入配置 → 可选启用 → 可选证书申请。
/// 任一步失败按反向次序回滚。详见 design.md 子模式 B 创建流程。
pub async fn create_site(
    ctx: Arc<AppContext>,
    input: CreateSiteInput,
) -> Result<CreateSiteOutcome, NgToolError> {
    let started = Instant::now();
    let conf_name = format!("{}.conf", input.name);
    let avail_path = ctx.probe.sites_available.join(&conf_name);
    let link_path = ctx.probe.sites_enabled.join(&conf_name);

    // 渲染模板
    let content = render_create_site_config(&input, None)?;

    // 写入：tmp 文件落在 sites-available 同目录（保证 hard_link 在同一文件系统）
    // 用 O_CREAT|O_EXCL 创建 tmp（防 tmp race），用 hard_link 替代 rename 做 race-safe 创建：
    // 若目标已存在则 EEXIST，避免 rename 静默覆盖。详见 architecture.md §15.1。
    let tmp_path = ctx.probe.sites_available.join(format!(
        ".{}.conf.{}.{}.tmp",
        input.name,
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));

    let write_result = tokio::task::spawn_blocking({
        let tmp = tmp_path.clone();
        let dst = avail_path.clone();
        let bytes = content.into_bytes();
        let site_name = input.name.clone();
        move || -> Result<(), NgToolError> {
            use std::io::Write as _;
            use std::os::unix::fs::OpenOptionsExt as _;

            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true) // O_CREAT | O_EXCL：tmp 不可被并发创建
                .mode(0o644)
                .open(&tmp)
                .map_err(|e| NgToolError::FileOperationFailed {
                    path: tmp.clone(),
                    message: format!("创建临时文件失败：{}", e),
                })?;
            f.write_all(&bytes)
                .map_err(|e| NgToolError::FileOperationFailed {
                    path: tmp.clone(),
                    message: format!("写入临时文件失败：{}", e),
                })?;
            f.sync_all().map_err(|e| NgToolError::FileOperationFailed {
                path: tmp.clone(),
                message: format!("fsync 失败：{}", e),
            })?;
            drop(f);

            // hard_link 是 race-safe 的：目标存在时 EEXIST
            match std::fs::hard_link(&tmp, &dst) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&tmp);
                    Ok(())
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let _ = std::fs::remove_file(&tmp);
                    Err(NgToolError::InvalidInput {
                        field: "site_name".into(),
                        message: format!("站点 {} 已存在", site_name),
                    })
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    Err(NgToolError::FileOperationFailed {
                        path: dst,
                        message: e.to_string(),
                    })
                }
            }
        }
    })
    .await
    .map_err(|e| NgToolError::FileOperationFailed {
        path: avail_path.clone(),
        message: format!("任务异常：{}", e),
    })?;

    write_result?;

    // 跟踪已完成的步骤用于回滚
    let config_written = true;
    let mut link_created = false;

    // 可选启用
    if input.enable_now {
        // 创建符号链接
        if let Err(e) = tokio::task::spawn_blocking({
            let target = avail_path.clone();
            let link = link_path.clone();
            move || std::os::unix::fs::symlink(&target, &link)
        })
        .await
        .map_err(|e| NgToolError::FileOperationFailed {
            path: link_path.clone(),
            message: format!("任务异常：{}", e),
        })? {
            rollback_site(&ctx, &avail_path, &link_path, config_written, link_created).await;
            return Err(NgToolError::FileOperationFailed {
                path: link_path.clone(),
                message: e.to_string(),
            });
        }
        link_created = true;

        // nginx -t
        if let Err(e) = ctx.nginx.test_config().await {
            rollback_site(&ctx, &avail_path, &link_path, config_written, link_created).await;
            log_audit(
                &ctx,
                "site.create",
                &input.name,
                AuditResult::Failure,
                started,
                json!({"stage": "test", "error": e.to_string()}),
            );
            return Err(e);
        }

        // reload
        if let Err(e) = ctx.systemd.reload("nginx").await {
            rollback_site(&ctx, &avail_path, &link_path, config_written, link_created).await;
            log_audit(
                &ctx,
                "site.create",
                &input.name,
                AuditResult::Failure,
                started,
                json!({"stage": "reload", "error": e.to_string()}),
            );
            return Err(e);
        }
        // reloaded = true;
    }

    // 可选证书申请（启用成功后）
    let mut cert_requested = false;
    if input.request_cert && input.enable_now {
        let certbot = &ctx.settings.certbot;
        if certbot.email.trim().is_empty() && !certbot.allow_unsafe_without_email {
            return Ok(CreateSiteOutcome::CertFailed {
                error: "未配置 certbot 邮箱，且未允许无邮箱注册".into(),
            });
        }
        cert_requested = true;
        let cert_domains = cert_domains_for_input(&input);
        let mut cert_cmd = crate::infra::certbot::apply_registration_args(
            CommandSpec::new("certbot")
                .arg("certonly")
                .arg("--nginx")
                .arg("--cert-name")
                .arg(&input.name),
            &certbot.email,
            certbot.allow_unsafe_without_email,
        );
        for domain in &cert_domains {
            cert_cmd = cert_cmd.arg("-d").arg(domain);
        }
        let cert_result = ctx
            .executor
            .run(cert_cmd.timeout(Duration::from_secs(120)))
            .await;

        match cert_result {
            Ok(out) if out.ok() => {
                let (cert_path, key_path) = cert_paths_for_name(&input.name);
                let ssl_content =
                    match render_create_site_config(&input, Some((&cert_path, &key_path))) {
                        Ok(content) => content,
                        Err(e) => {
                            return Ok(CreateSiteOutcome::CertFailed {
                                error: format!("证书已签发，但渲染 SSL 配置失败：{}", e),
                            });
                        }
                    };
                if let Err(e) =
                    replace_site_config_checked(&ctx, &input.name, ssl_content.as_bytes()).await
                {
                    log_audit(
                        &ctx,
                        "site.ssl.apply",
                        &input.name,
                        AuditResult::Failure,
                        started,
                        json!({"cert_name": input.name, "error": e.to_string()}),
                    );
                    return Ok(CreateSiteOutcome::CertFailed {
                        error: format!("证书已签发，但写入 SSL 配置失败：{}", e),
                    });
                }
                log_audit(
                    &ctx,
                    "cert.request",
                    &input.name,
                    AuditResult::Success,
                    started,
                    json!({"domains": cert_domains}),
                );
            }
            Ok(out) => {
                let combined = out.combined();
                log_audit(
                    &ctx,
                    "cert.request",
                    &input.name,
                    AuditResult::Failure,
                    started,
                    json!({"domains": cert_domains, "exit": out.code()}),
                );
                log_audit(
                    &ctx,
                    "site.create",
                    &input.name,
                    AuditResult::Success,
                    started,
                    json!({"cert": "failed"}),
                );
                return Ok(CreateSiteOutcome::CertFailed { error: combined });
            }
            Err(e) => {
                // 证书失败不回滚站点
                log_audit(
                    &ctx,
                    "cert.request",
                    &input.name,
                    AuditResult::Failure,
                    started,
                    json!({"domains": cert_domains, "error": e.to_string()}),
                );
                log_audit(
                    &ctx,
                    "site.create",
                    &input.name,
                    AuditResult::Success,
                    started,
                    json!({"cert": "failed"}),
                );
                return Ok(CreateSiteOutcome::CertFailed {
                    error: e.to_string(),
                });
            }
        }
    }

    log_audit(
        &ctx,
        "site.create",
        &input.name,
        AuditResult::Success,
        started,
        json!({"enable": input.enable_now, "cert": cert_requested}),
    );
    Ok(CreateSiteOutcome::Ok { cert_requested })
}

async fn replace_site_config_checked(
    ctx: &AppContext,
    site_name: &str,
    bytes: &[u8],
) -> Result<(), NgToolError> {
    let conf_name = format!("{}.conf", site_name);
    let target_path = ctx.probe.sites_available.join(&conf_name);
    let original =
        tokio::fs::read(&target_path)
            .await
            .map_err(|e| NgToolError::FileOperationFailed {
                path: target_path.clone(),
                message: format!("读取原配置失败：{}", e),
            })?;

    atomic_replace(ctx, site_name, &target_path, bytes).await?;

    if let Err(e) = ctx.nginx.test_config().await {
        let _ = atomic_replace(ctx, site_name, &target_path, &original).await;
        let _ = ctx.nginx.test_config().await;
        return Err(e);
    }

    if let Err(e) = ctx.systemd.reload("nginx").await {
        let _ = atomic_replace(ctx, site_name, &target_path, &original).await;
        let _ = ctx.nginx.test_config().await;
        return Err(e);
    }

    Ok(())
}

pub async fn apply_cert_to_site_config(
    ctx: Arc<AppContext>,
    site_name: &str,
    cert_name: &str,
) -> Result<(), NgToolError> {
    let started = Instant::now();
    let conf_name = format!("{}.conf", site_name);
    let target_path = ctx.probe.sites_available.join(&conf_name);
    let original = tokio::fs::read_to_string(&target_path).await.map_err(|e| {
        NgToolError::FileOperationFailed {
            path: target_path.clone(),
            message: format!("读取原配置失败：{}", e),
        }
    })?;

    let parsed = crate::template::config_parser::parse_for_edit(&original);
    let Some(managed_type) = parsed.managed_type else {
        return Err(NgToolError::InvalidInput {
            field: "site_config".into(),
            message: "仅支持自动更新由 nginx-tools 管理的站点配置".into(),
        });
    };
    if !parsed.markers_intact {
        return Err(NgToolError::InvalidInput {
            field: "site_config".into(),
            message: "站点注入槽标记不完整，无法安全自动写入 SSL 配置".into(),
        });
    }
    let kind = match managed_type {
        SiteType::Emby => crate::template::renderer::SiteKind::Emby,
        SiteType::Static => crate::template::renderer::SiteKind::Static,
        SiteType::Proxy | SiteType::Unknown => crate::template::renderer::SiteKind::Proxy,
    };
    let site_type = managed_type;
    let feature_enabled = |name: &str| parsed.managed_features.iter().any(|item| item == name);
    let (cert_path, key_path) = cert_paths_for_name(cert_name);
    let params = crate::template::renderer::RenderParams {
        site_name: site_name.to_string(),
        domain_name: parsed.domains.first().cloned().unwrap_or_default(),
        domain_aliases: parsed
            .domains
            .iter()
            .skip(1)
            .cloned()
            .collect::<Vec<_>>()
            .join(" "),
        upstream_scheme: parsed.upstream_scheme.unwrap_or_else(|| "http".into()),
        upstream_target: parsed.upstream_target.unwrap_or_default(),
        static_root: parsed.static_root.unwrap_or_default(),
        feature_streaming: site_type == SiteType::Proxy && feature_enabled("streaming"),
        feature_websocket: (site_type == SiteType::Proxy && feature_enabled("websocket"))
            || site_type == SiteType::Emby,
        feature_large_body: (site_type == SiteType::Proxy && feature_enabled("large_body"))
            || site_type == SiteType::Emby,
        feature_cors: site_type == SiteType::Proxy && feature_enabled("cors"),
        feature_long_timeout: (site_type == SiteType::Proxy && feature_enabled("long_timeout"))
            || site_type == SiteType::Emby,
        feature_spa_mode: site_type == SiteType::Static && feature_enabled("spa_mode"),
        feature_static_cache: site_type == SiteType::Static && feature_enabled("static_cache"),
        feature_block_sensitive: site_type == SiteType::Static
            && feature_enabled("block_sensitive"),
        custom_before_location: parsed
            .injection_slots
            .get(&crate::template::config_parser::InjectionSlot::BeforeLocation)
            .cloned()
            .unwrap_or_default(),
        custom_inside_location: parsed
            .injection_slots
            .get(&crate::template::config_parser::InjectionSlot::InsideLocation)
            .cloned()
            .unwrap_or_default(),
        custom_after_location: parsed
            .injection_slots
            .get(&crate::template::config_parser::InjectionSlot::AfterLocation)
            .cloned()
            .unwrap_or_default(),
        ssl_enabled: true,
        ssl_cert_path: cert_path,
        ssl_key_path: key_path,
    };

    let content = crate::template::renderer::render(kind, &params)
        .map_err(|e| NgToolError::TemplateFailed { message: e })?;
    match replace_site_config_checked(&ctx, site_name, content.as_bytes()).await {
        Ok(()) => {
            log_audit(
                &ctx,
                "site.ssl.apply",
                site_name,
                AuditResult::Success,
                started,
                json!({"cert_name": cert_name}),
            );
            Ok(())
        }
        Err(e) => {
            log_audit(
                &ctx,
                "site.ssl.apply",
                site_name,
                AuditResult::Failure,
                started,
                json!({"cert_name": cert_name, "error": e.to_string()}),
            );
            Err(e)
        }
    }
}

pub fn validate_managed_site_for_ssl(ctx: &AppContext, site_name: &str) -> Result<(), NgToolError> {
    let conf_name = format!("{}.conf", site_name);
    let target_path = ctx.probe.sites_available.join(&conf_name);
    let original =
        std::fs::read_to_string(&target_path).map_err(|e| NgToolError::FileOperationFailed {
            path: target_path.clone(),
            message: format!("读取原配置失败：{}", e),
        })?;
    let parsed = crate::template::config_parser::parse_for_edit(&original);
    if parsed.managed_type.is_none() {
        return Err(NgToolError::InvalidInput {
            field: "site_config".into(),
            message: "仅支持为由 nginx-tools 管理的站点自动写入 SSL 配置".into(),
        });
    }
    if !parsed.markers_intact {
        return Err(NgToolError::InvalidInput {
            field: "site_config".into(),
            message: "站点注入槽标记不完整，无法安全自动写入 SSL 配置".into(),
        });
    }
    Ok(())
}

fn cert_domains_for_input(input: &CreateSiteInput) -> Vec<String> {
    std::iter::once(input.domain.trim())
        .chain(
            input
                .domain_aliases
                .split(|c: char| c == ',' || c.is_ascii_whitespace())
                .map(str::trim),
        )
        .filter(|domain| !domain.is_empty())
        .map(String::from)
        .collect()
}

/// 回滚新建站点的已执行步骤
async fn rollback_site(
    ctx: &AppContext,
    avail_path: &std::path::Path,
    link_path: &std::path::Path,
    config_written: bool,
    link_created: bool,
) {
    if link_created {
        let lp = link_path.to_path_buf();
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(&lp)).await;
        let _ = ctx.nginx.test_config().await;
    }
    if config_written {
        let ap = avail_path.to_path_buf();
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(&ap)).await;
    }
}

/// 保存已有站点配置：临时文件原子替换，必要时 `nginx -t` + reload，失败恢复原内容。
/// 详见 risks.md R1 与 architecture.md §15.2。
/// 写入前比较 expected_mtime 与目标文件实际 mtime（架构 §15.0 mtime 并发保护），
/// 不一致时中止保存并提示外部修改。
pub async fn save_site_config(
    ctx: Arc<AppContext>,
    input: SaveSiteInput,
) -> Result<(), NgToolError> {
    let started = Instant::now();
    let conf_name = format!("{}.conf", input.name);
    let target_path = ctx.probe.sites_available.join(&conf_name);

    if !target_path.exists() {
        let err = NgToolError::FileOperationFailed {
            path: target_path.clone(),
            message: "站点配置不存在".into(),
        };
        log_audit(
            &ctx,
            "site.edit",
            &input.name,
            AuditResult::Failure,
            started,
            json!({"stage": "precheck", "error": err.to_string()}),
        );
        return Err(err);
    }

    // mtime 并发保护（架构 §15.0）：写入前比较预期 mtime 与当前 mtime
    if let Some(expected) = input.expected_mtime {
        let actual = tokio::fs::metadata(&target_path)
            .await
            .ok()
            .and_then(|m| m.modified().ok());
        if actual != Some(expected) {
            let err = NgToolError::FileOperationFailed {
                path: target_path.clone(),
                message:
                    "目标文件已被外部修改（mtime 变化），保存被取消。请按 Esc 返回后重新进入编辑"
                        .into(),
            };
            log_audit(
                &ctx,
                "site.edit",
                &input.name,
                AuditResult::Failure,
                started,
                json!({"stage": "mtime_check", "error": err.to_string()}),
            );
            return Err(err);
        }
    }

    let original =
        tokio::fs::read(&target_path)
            .await
            .map_err(|e| NgToolError::FileOperationFailed {
                path: target_path.clone(),
                message: format!("读取原配置失败：{}", e),
            })?;

    if let Err(e) = atomic_replace(&ctx, &input.name, &target_path, input.content.as_bytes()).await
    {
        log_audit(
            &ctx,
            "site.edit",
            &input.name,
            AuditResult::Failure,
            started,
            json!({"stage": "write", "error": e.to_string()}),
        );
        return Err(e);
    }

    if input.test_and_reload {
        if let Err(e) = ctx.nginx.test_config().await {
            let _ = atomic_replace(&ctx, &input.name, &target_path, &original).await;
            let _ = ctx.nginx.test_config().await;
            log_audit(
                &ctx,
                "site.edit",
                &input.name,
                AuditResult::Failure,
                started,
                json!({"stage": "test", "error": e.to_string(), "rolled_back": true}),
            );
            return Err(e);
        }

        if let Err(e) = ctx.systemd.reload("nginx").await {
            let _ = atomic_replace(&ctx, &input.name, &target_path, &original).await;
            let _ = ctx.nginx.test_config().await;
            log_audit(
                &ctx,
                "site.edit",
                &input.name,
                AuditResult::Failure,
                started,
                json!({"stage": "reload", "error": e.to_string(), "rolled_back": true}),
            );
            return Err(e);
        }
    }

    log_audit(
        &ctx,
        "site.edit",
        &input.name,
        AuditResult::Success,
        started,
        json!({"test_and_reload": input.test_and_reload}),
    );
    Ok(())
}

async fn atomic_replace(
    ctx: &AppContext,
    site_name: &str,
    target_path: &std::path::Path,
    content: &[u8],
) -> Result<(), NgToolError> {
    let tmp_path = ctx.paths.tmp.join(format!(
        "{}.{}.{}.tmp",
        site_name,
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));

    tokio::fs::write(&tmp_path, content)
        .await
        .map_err(|e| NgToolError::FileOperationFailed {
            path: tmp_path.clone(),
            message: e.to_string(),
        })?;

    let result = tokio::task::spawn_blocking({
        let src = tmp_path.clone();
        let dst = target_path.to_path_buf();
        move || std::fs::rename(&src, &dst)
    })
    .await
    .map_err(|e| NgToolError::FileOperationFailed {
        path: target_path.to_path_buf(),
        message: format!("任务异常：{}", e),
    })?;

    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(NgToolError::FileOperationFailed {
            path: target_path.to_path_buf(),
            message: e.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proxy_config() {
        let text = r#"
server {
    listen 80;
    server_name app.example.com www.app.example.com;
    access_log /srv/log/nginx/app.access.log main;
    error_log /srv/log/nginx/app.error.log warn;
    location / {
        proxy_pass http://127.0.0.1:8080;
    }
}
"#;
        let p = parse_config(text);
        assert_eq!(
            p.server_names,
            vec!["app.example.com", "www.app.example.com"]
        );
        assert_eq!(p.proxy_pass.as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(
            p.access_log_path,
            Some(PathBuf::from("/srv/log/nginx/app.access.log"))
        );
        assert_eq!(
            p.error_log_path,
            Some(PathBuf::from("/srv/log/nginx/app.error.log"))
        );
        assert!(p.static_root.is_none());
        assert!(!p.has_emby_marker);
        assert_eq!(infer_type(&p), SiteType::Proxy);
    }

    #[test]
    fn parse_static_config() {
        let text = r#"
server {
    listen 80;
    server_name blog.example.com;
    root /var/www/blog;
}
"#;
        let p = parse_config(text);
        assert_eq!(p.server_names, vec!["blog.example.com"]);
        assert_eq!(p.static_root.as_deref(), Some("/var/www/blog"));
        assert_eq!(infer_type(&p), SiteType::Static);
    }

    #[test]
    fn parse_emby_marker_detected() {
        let text = r#"
# nginx-tools:tool-marker: type=emby
server {
    listen 80;
    server_name emby.example.com;
    location / {
        proxy_pass http://192.168.1.5:8096;
    }
}
"#;
        let p = parse_config(text);
        assert!(p.has_emby_marker);
        assert_eq!(infer_type(&p), SiteType::Emby);
    }

    #[test]
    fn comments_are_ignored_in_directive_parsing() {
        let text = r#"
server {
    # server_name commented.example.com;
    server_name real.example.com;
    # proxy_pass http://commented;
    proxy_pass http://127.0.0.1:9090;
}
"#;
        let p = parse_config(text);
        assert_eq!(p.server_names, vec!["real.example.com"]);
        assert_eq!(p.proxy_pass.as_deref(), Some("http://127.0.0.1:9090"));
    }

    #[test]
    fn parse_config_supports_access_log_off() {
        let text = r#"
server {
    server_name app.example.com;
    access_log off;
    error_log /var/log/nginx/app.error.log error;
}
"#;
        let p = parse_config(text);
        assert_eq!(p.access_log_path, None);
        assert_eq!(
            p.error_log_path,
            Some(PathBuf::from("/var/log/nginx/app.error.log"))
        );
    }

    #[test]
    fn repeated_server_names_are_deduped() {
        let text = r#"
server {
    listen 80;
    server_name app.example.com;
}

server {
    listen 443 ssl http2;
    server_name app.example.com;
}
"#;
        let p = parse_config(text);
        assert_eq!(p.server_names, vec!["app.example.com"]);
    }

    #[test]
    fn certbot_domains_parsed() {
        let text = "
Certificate Name: app
  Domains: app.example.com www.app.example.com
    Expiry Date: 2026-07-04 ... (VALID: 67 days)
Certificate Name: api
  Domains: api.example.com
    Expiry Date: 2026-05-04 ... (VALID: 5 days)
";
        let map = parse_certbot_domains(text);
        assert_eq!(map.get("app.example.com"), Some(&67));
        assert_eq!(map.get("www.app.example.com"), Some(&67));
        assert_eq!(map.get("api.example.com"), Some(&5));
    }

    #[test]
    fn cert_domains_include_aliases() {
        let input = CreateSiteInput {
            name: "app".into(),
            domain: "app.example.com".into(),
            domain_aliases: "www.app.example.com, m.app.example.com".into(),
            kind: crate::template::renderer::SiteKind::Proxy,
            upstream_scheme: "http".into(),
            upstream_target: "127.0.0.1:8080".into(),
            static_root: String::new(),
            feature_streaming: false,
            feature_websocket: false,
            feature_large_body: false,
            feature_cors: false,
            feature_long_timeout: false,
            feature_spa_mode: false,
            feature_static_cache: true,
            feature_block_sensitive: false,
            enable_now: true,
            request_cert: true,
        };

        assert_eq!(
            cert_domains_for_input(&input),
            vec![
                "app.example.com",
                "www.app.example.com",
                "m.app.example.com"
            ]
        );
    }
}
