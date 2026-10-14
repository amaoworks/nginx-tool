//! 证书领域模型，对应 design.md 视图 3 / architecture.md §11.4。
//!
//! 数据来源：`certbot certificates` 输出文本解析；解析失败保留原始输出（R2 闭环）。
//! 关联站点：基于 server_name 与证书 Domains 字段交叉匹配。

use std::collections::{HashMap, HashSet};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;

use crate::error::NgToolError;
use crate::infra::audit::AuditResult;
use crate::infra::executor::CommandSpec;
use crate::infra::AppContext;

/// 证书等级，与 design.md 视图 1/3 的颜色阈值对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CertLevel {
    Ok,
    Warning,
    Critical,
    Expired,
}

impl CertLevel {
    pub fn from_days(days: i64) -> Self {
        if days < 0 {
            CertLevel::Expired
        } else if days < 7 {
            CertLevel::Critical
        } else if days < 30 {
            CertLevel::Warning
        } else {
            CertLevel::Ok
        }
    }

    pub fn glyph(&self) -> &'static str {
        match self {
            CertLevel::Ok => "✓",
            CertLevel::Warning => "⚠",
            CertLevel::Critical => "🔴",
            CertLevel::Expired => "✗",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CertLevel::Ok => "正常",
            CertLevel::Warning => "即将到期",
            CertLevel::Critical => "紧急",
            CertLevel::Expired => "已过期",
        }
    }
}

/// 单条证书。从 `certbot certificates` 解析得到。
#[derive(Debug, Clone, Serialize)]
pub struct Certificate {
    pub name: String,
    pub domains: Vec<String>,
    pub expiry: Option<DateTime<Utc>>,
    pub days_left: Option<i64>,
    pub level: Option<CertLevel>,
    pub cert_path: Option<String>,
}

impl Certificate {
    pub fn primary_domain(&self) -> Option<&str> {
        self.domains.first().map(String::as_str)
    }
}

/// 证书 + 站点关联视图。
#[derive(Debug, Clone, Serialize)]
pub struct CertWithSite {
    pub cert: Certificate,
    /// 关联的站点名（一个证书可关联多个站点，但首版按主站点展示）。
    pub site_names: Vec<String>,
    /// 是否为孤立证书：证书存在但无任何匹配的站点。
    pub orphan: bool,
    /// 是否仍被 nginx 配置中的 ssl_certificate 引用。引用中的孤立证书不应自动清理。
    pub nginx_referenced: bool,
}

/// 解析 `certbot certificates` 输出为证书列表。
/// 解析失败时返回空列表，调用方可保留原始 stdout 用于错误展示。
pub fn parse_certificates(text: &str) -> Vec<Certificate> {
    let name_re = regex::Regex::new(r"(?m)^\s*Certificate Name:\s*(.+)$").unwrap();
    let domains_re = regex::Regex::new(r"(?m)^\s*Domains:\s*(.+)$").unwrap();
    let expiry_re = regex::Regex::new(
        r"(?m)^\s*Expiry Date:\s*([0-9T:\-+ ]+)\s*\((?:.*?(\-?\d+)\s*days?|INVALID:\s*(EXPIRED))\)",
    )
    .unwrap();
    let path_re = regex::Regex::new(r"(?m)^\s*Certificate Path:\s*(.+)$").unwrap();

    // 按 Certificate Name 锚点切块，逐块在内部正则提取字段。
    let mut blocks: Vec<&str> = Vec::new();
    let mut last = 0usize;
    for cap in name_re.captures_iter(text) {
        if let Some(m) = cap.get(0) {
            if m.start() > last {
                blocks.push(&text[last..m.start()]);
            }
            last = m.start();
        }
    }
    if last < text.len() {
        blocks.push(&text[last..]);
    }
    if !blocks.is_empty() && blocks[0].trim().is_empty() {
        blocks.remove(0);
    }

    let mut certs = Vec::new();
    for block in blocks {
        let name = name_re
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }

        let domains: Vec<String> = domains_re
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().split_whitespace().map(String::from).collect())
            .unwrap_or_default();

        let (expiry, days_left) = match expiry_re.captures(block) {
            Some(cap) => {
                let raw_ts = cap.get(1).map(|m| m.as_str().trim().to_string());
                let dt = raw_ts.as_deref().and_then(parse_certbot_timestamp);
                let days = cap
                    .get(2)
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .or_else(|| {
                        // Certbot 的过期输出没有天数；不足一天也必须标记为负数。
                        cap.get(3).map(|_| {
                            dt.map_or(-1, |expiry| {
                                expiry.signed_duration_since(Utc::now()).num_days().min(-1)
                            })
                        })
                    });
                (dt, days)
            }
            None => (None, None),
        };

        let level = days_left.map(CertLevel::from_days);

        let cert_path = path_re
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string());

        certs.push(Certificate {
            name,
            domains,
            expiry,
            days_left,
            level,
            cert_path,
        });
    }
    certs
}

/// certbot 时间戳形如 `2026-07-04 08:30:14+00:00`。
/// 兼容 `T` 分隔的 RFC3339 与空格分隔两种格式。
fn parse_certbot_timestamp(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if !s.contains('T') {
        let alt = s.replacen(' ', "T", 1);
        if let Ok(dt) = DateTime::parse_from_rfc3339(&alt) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    None
}

/// 调用 certbot 列出全部证书；缺失依赖时返回 DependencyMissing。
pub async fn list_certificates(ctx: Arc<AppContext>) -> Result<Vec<Certificate>, NgToolError> {
    if !ctx.deps().certbot {
        return Err(NgToolError::DependencyMissing {
            name: "certbot".into(),
        });
    }
    let out = ctx
        .executor
        .run(
            CommandSpec::new("certbot")
                .arg("certificates")
                .timeout(Duration::from_secs(8)),
        )
        .await?;
    Ok(parse_certificates(&out.stdout))
}

/// 把证书与站点列表交叉匹配。任意一个证书 domain 命中任意站点的 server_name 即视为关联。
pub fn associate_with_sites(
    certs: Vec<Certificate>,
    sites: &[crate::domain::site::Site],
) -> Vec<CertWithSite> {
    certs
        .into_iter()
        .map(|cert| {
            let mut matched = Vec::new();
            for s in sites {
                let hit = s
                    .all_domains
                    .iter()
                    .any(|d| cert.domains.iter().any(|cd| cd == d));
                if hit {
                    matched.push(s.name.clone());
                }
            }
            let orphan = matched.is_empty();
            CertWithSite {
                cert,
                site_names: matched,
                orphan,
                nginx_referenced: false,
            }
        })
        .collect()
}

/// 扫描 nginx 配置，标记证书是否仍被 `ssl_certificate` 指令引用。
pub fn mark_nginx_references(items: &mut [CertWithSite], nginx_root: &Path) {
    let refs = collect_ssl_certificate_refs(nginx_root);
    if refs.is_empty() {
        return;
    }
    for item in items {
        item.nginx_referenced = cert_is_referenced(&item.cert, &refs);
    }
}

/// 可自动清理的证书：未被 nginx 引用，且要么孤立，要么其全部域名都由其他更合适的证书覆盖。
pub fn cleanup_candidates(items: &[CertWithSite]) -> Vec<&CertWithSite> {
    let keepers = domain_keepers(items);
    items
        .iter()
        .filter(|item| {
            !item.nginx_referenced && (item.orphan || is_redundant_cert(item, items, &keepers))
        })
        .collect()
}

/// 不可直接清理但值得提示的证书：看起来多余，不过 nginx 配置仍在引用。
pub fn referenced_cleanup_skips(items: &[CertWithSite]) -> Vec<&CertWithSite> {
    let keepers = domain_keepers(items);
    items
        .iter()
        .filter(|item| {
            item.nginx_referenced && (item.orphan || is_redundant_cert(item, items, &keepers))
        })
        .collect()
}

fn is_redundant_cert(
    item: &CertWithSite,
    items: &[CertWithSite],
    keepers: &HashMap<String, String>,
) -> bool {
    !item.orphan
        && !item.cert.domains.is_empty()
        && item.cert.domains.iter().all(|domain| {
            let covered_by_other = items.iter().any(|other| {
                other.cert.name != item.cert.name && other.cert.domains.iter().any(|d| d == domain)
            });
            covered_by_other
                && keepers
                    .get(domain)
                    .is_some_and(|keeper| keeper != &item.cert.name)
        })
}

fn domain_keepers(items: &[CertWithSite]) -> HashMap<String, String> {
    let mut keepers = HashMap::new();
    let domains: HashSet<String> = items
        .iter()
        .flat_map(|item| item.cert.domains.iter().cloned())
        .collect();
    for domain in domains {
        if let Some(best) = items
            .iter()
            .filter(|item| item.cert.domains.iter().any(|d| d == &domain))
            .min_by_key(|item| cert_keep_rank(item, &domain))
        {
            keepers.insert(domain, best.cert.name.clone());
        }
    }
    keepers
}

fn cert_keep_rank(
    item: &CertWithSite,
    domain: &str,
) -> (u8, usize, std::cmp::Reverse<i64>, String) {
    let primary_matches = item.cert.primary_domain() == Some(domain);
    (
        if item.nginx_referenced {
            0
        } else if primary_matches {
            1
        } else {
            2
        },
        item.cert.domains.len(),
        std::cmp::Reverse(item.cert.days_left.unwrap_or(i64::MIN)),
        item.cert.name.clone(),
    )
}

fn collect_ssl_certificate_refs(nginx_root: &Path) -> HashSet<String> {
    let mut refs = HashSet::new();
    let re = regex::Regex::new(r"\bssl_certificate\s+([^;#\s]+)").unwrap();
    for entry in walkdir::WalkDir::new(nginx_root)
        .follow_links(true) // 必须跟随软链接，否则无法读取 sites-enabled 下的配置
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        // Nginx include 不要求扩展名，例如 Debian 默认站点名为 default。
        // 清理证书时保守扫描所有文本文件，避免漏掉仍在使用的证书。
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for cap in re.captures_iter(&text) {
            if let Some(value) = cap.get(1).map(|m| m.as_str().trim_matches('"')) {
                refs.insert(value.to_string());
            }
        }
    }
    refs
}

fn ensure_cert_unreferenced(nginx_root: &Path, cert_name: &str) -> Result<(), NgToolError> {
    let reference = format!("/live/{cert_name}/");
    if collect_ssl_certificate_refs(nginx_root)
        .iter()
        .any(|path| path.contains(&reference))
    {
        return Err(NgToolError::InvalidInput {
            field: "cert_name".into(),
            message: format!("证书 {cert_name} 仍被 Nginx 配置引用，拒绝删除"),
        });
    }
    Ok(())
}

fn cert_is_referenced(cert: &Certificate, refs: &HashSet<String>) -> bool {
    let cert_path = cert.cert_path.as_deref();
    refs.iter().any(|r| {
        Some(r.as_str()) == cert_path
            || r.contains(&format!("/live/{}/", cert.name))
            || cert_path.is_some_and(|p| same_letsencrypt_live_cert(r, p))
    })
}

fn same_letsencrypt_live_cert(left: &str, right: &str) -> bool {
    letsencrypt_live_name(left)
        .is_some_and(|l| letsencrypt_live_name(right).is_some_and(|r| l == r))
}

fn letsencrypt_live_name(path: &str) -> Option<&str> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let live_pos = parts.iter().position(|part| *part == "live")?;
    if parts.get(live_pos + 2).copied() != Some("fullchain.pem") {
        return None;
    }
    parts.get(live_pos + 1).copied()
}

/// 站点证书怎么落地。先决定计划，再决定要不要调 certbot。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertIssuePlan {
    /// 已有未过期证书覆盖全部域名，直接写回 Nginx。
    Reuse { cert_name: String },
    /// 扩展已有 lineage，使它覆盖站点的全部域名。
    Expand { cert_name: String },
    /// 新建以站点名命名的证书。
    Issue { cert_name: String },
}

fn normalize_domain(domain: &str) -> String {
    domain.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn wanted_domains(domains: &[String]) -> Vec<String> {
    let mut wanted = Vec::new();
    for domain in domains {
        let normalized = normalize_domain(domain);
        if !normalized.is_empty() && !wanted.contains(&normalized) {
            wanted.push(normalized);
        }
    }
    wanted
}

fn cert_domain_set(cert: &Certificate) -> HashSet<String> {
    cert.domains
        .iter()
        .map(|domain| normalize_domain(domain.as_str()))
        .filter(|domain| !domain.is_empty())
        .collect()
}

impl CertIssuePlan {
    fn cert_name(&self) -> &str {
        match self {
            CertIssuePlan::Reuse { cert_name }
            | CertIssuePlan::Expand { cert_name }
            | CertIssuePlan::Issue { cert_name } => cert_name,
        }
    }
}

fn cert_is_usable(cert: &Certificate) -> bool {
    !matches!(cert.level, Some(CertLevel::Expired))
        && cert.days_left.is_some_and(|days| days >= 0)
        && cert.expiry.map_or(true, |expiry| expiry > Utc::now())
}

fn cert_covers(cert: &Certificate, wanted: &[String]) -> bool {
    let have = cert_domain_set(cert);
    wanted.iter().all(|domain| have.contains(domain))
}

/// 选择证书动作：
/// 1. 一张未过期证书已经覆盖全部域名时直接复用，优先站点同名 lineage；
/// 2. 否则扩展站点同名 lineage，再否则扩展交集最大的已有证书；
/// 3. 都没有时才新签一张以站点名命名的证书。
pub fn plan_certificate(
    certs: &[Certificate],
    site_name: &str,
    domains: &[String],
) -> CertIssuePlan {
    let wanted = wanted_domains(domains);
    if wanted.is_empty() {
        return CertIssuePlan::Issue {
            cert_name: site_name.to_string(),
        };
    }
    let mut covering: Vec<&Certificate> = certs
        .iter()
        .filter(|cert| cert_is_usable(cert) && cert_covers(cert, &wanted))
        .collect();
    if !covering.is_empty() {
        covering.sort_by_key(|cert| {
            let extra = cert_domain_set(cert).len().saturating_sub(wanted.len());
            (
                cert.name != site_name,
                extra != 0,
                extra,
                std::cmp::Reverse(cert.days_left.unwrap_or(0)),
                cert.name.clone(),
            )
        });
        return CertIssuePlan::Reuse {
            cert_name: covering[0].name.clone(),
        };
    }

    if let Some(cert) = certs.iter().find(|cert| cert.name == site_name) {
        return CertIssuePlan::Expand {
            cert_name: cert.name.clone(),
        };
    }

    let mut partial: Vec<&Certificate> = certs
        .iter()
        .filter(|cert| {
            let have = cert_domain_set(cert);
            wanted.iter().any(|domain| have.contains(domain))
        })
        .collect();
    partial.sort_by_key(|cert| {
        let have = cert_domain_set(cert);
        let overlap = wanted
            .iter()
            .filter(|domain| have.contains(*domain))
            .count();
        let primary = wanted
            .first()
            .is_some_and(|domain| cert.name.eq_ignore_ascii_case(domain));
        (
            std::cmp::Reverse(overlap),
            !primary,
            std::cmp::Reverse(cert.days_left.unwrap_or(0)),
            cert.name.clone(),
        )
    });
    if let Some(cert) = partial.first() {
        return CertIssuePlan::Expand {
            cert_name: cert.name.clone(),
        };
    }

    CertIssuePlan::Issue {
        cert_name: site_name.to_string(),
    }
}

fn cert_issue_spec(
    certbot_email: &str,
    allow_unsafe_without_email: bool,
    plan: &CertIssuePlan,
    domains: &[String],
    certs: &[Certificate],
) -> CommandSpec {
    let mut spec = crate::infra::certbot::apply_registration_args(
        CommandSpec::new("certbot")
            .arg("certonly")
            .arg("--nginx")
            .arg("--cert-name")
            .arg(plan.cert_name()),
        certbot_email,
        allow_unsafe_without_email,
    )
    .timeout(Duration::from_secs(180));
    let mut issue_domains = domains.to_vec();
    if let CertIssuePlan::Expand { cert_name } = plan {
        spec = spec.arg("--expand");
        // 扩展共享 lineage 时保留原有 SAN，不能只传入当前站点的域名。
        if let Some(cert) = certs.iter().find(|cert| cert.name == *cert_name) {
            issue_domains.extend(cert.domains.iter().cloned());
        }
    }
    for domain in wanted_domains(&issue_domains) {
        spec = spec.arg("-d").arg(domain);
    }
    spec
}

async fn bind_certificate(
    ctx: Arc<AppContext>,
    site_name: &str,
    cert_name: &str,
) -> Result<(), NgToolError> {
    crate::domain::site::apply_cert_to_site_config(ctx, site_name, cert_name).await
}

/// 为站点准备证书并写回 SSL 配置。
///
/// 已有证书能覆盖全部 `server_name` 时不再签发；域名被拆成多张单域名证书时，
/// 扩展交集最大的那一张，而不是再挂一次失败的新签发。只有拿到可用证书后才写 Nginx。
pub async fn request_cert(
    ctx: Arc<AppContext>,
    site_name: &str,
    domains: &[String],
) -> Result<String, NgToolError> {
    let started = std::time::Instant::now();
    if !ctx.deps().certbot {
        return Err(NgToolError::DependencyMissing {
            name: "certbot".into(),
        });
    }
    if domains.is_empty() {
        return Err(NgToolError::InvalidInput {
            field: "domains".into(),
            message: "申请证书需要至少一个域名".into(),
        });
    }
    let certbot = &ctx.settings.certbot;
    if certbot.email.trim().is_empty() && !certbot.allow_unsafe_without_email {
        return Err(NgToolError::InvalidInput {
            field: "certbot.email".into(),
            message: "未配置 certbot 邮箱，且未允许无邮箱注册".into(),
        });
    }
    crate::domain::site::validate_managed_site_for_ssl(&ctx, site_name)?;

    let existing = list_certificates(ctx.clone()).await?;
    let plan = plan_certificate(&existing, site_name, domains);
    if let CertIssuePlan::Reuse { cert_name } = &plan {
        bind_certificate(ctx.clone(), site_name, cert_name).await?;
        ctx.audit.log(
            "cert.request",
            site_name,
            AuditResult::Success,
            started.elapsed().as_millis() as u64,
            json!({"domains": domains, "reused": true, "cert_name": cert_name}),
        );
        return Ok(format!("已复用证书 {cert_name}，并写入站点 SSL 配置"));
    }

    let spec = cert_issue_spec(
        &certbot.email,
        certbot.allow_unsafe_without_email,
        &plan,
        domains,
        &existing,
    );
    let issued_name = plan.cert_name().to_string();

    match ctx.executor.run(spec).await {
        Ok(out) => {
            let combined = out.combined();
            if !out.ok() {
                if let Ok(certs) = list_certificates(ctx.clone()).await {
                    if let CertIssuePlan::Reuse { cert_name } =
                        plan_certificate(&certs, site_name, domains)
                    {
                        bind_certificate(ctx.clone(), site_name, &cert_name).await?;
                        ctx.audit.log(
                            "cert.request",
                            site_name,
                            AuditResult::Success,
                            started.elapsed().as_millis() as u64,
                            json!({
                                "domains": domains,
                                "reused": true,
                                "recovered": true,
                                "cert_name": cert_name,
                            }),
                        );
                        return Ok(format!(
                            "签发未完成，已改用现有证书 {cert_name} 写入站点 SSL 配置"
                        ));
                    }
                }
                ctx.audit.log(
                    "cert.request",
                    site_name,
                    AuditResult::Failure,
                    started.elapsed().as_millis() as u64,
                    json!({"domains": domains, "exit": out.code(), "cert_name": issued_name}),
                );
                return Err(NgToolError::CommandFailed {
                    command: "certbot certonly --nginx".into(),
                    code: out.code(),
                    stderr: combined,
                });
            }
            bind_certificate(ctx.clone(), site_name, &issued_name).await?;
            ctx.audit.log(
                "cert.request",
                site_name,
                AuditResult::Success,
                started.elapsed().as_millis() as u64,
                json!({"domains": domains, "cert_name": issued_name}),
            );
            Ok(combined)
        }
        Err(e) => {
            ctx.audit.log(
                "cert.request",
                site_name,
                AuditResult::Failure,
                started.elapsed().as_millis() as u64,
                json!({"domains": domains, "error": e.to_string()}),
            );
            Err(e)
        }
    }
}

/// `certbot renew`：续期所有证书。
pub async fn renew_all(ctx: Arc<AppContext>) -> Result<String, NgToolError> {
    let started = std::time::Instant::now();
    if !ctx.deps().certbot {
        return Err(NgToolError::DependencyMissing {
            name: "certbot".into(),
        });
    }
    match ctx
        .executor
        .run(
            CommandSpec::new("certbot")
                .arg("renew")
                .arg("--non-interactive")
                .timeout(Duration::from_secs(300)),
        )
        .await
    {
        Ok(out) => {
            let combined = out.combined();
            let result = if out.ok() {
                AuditResult::Success
            } else {
                AuditResult::Failure
            };
            ctx.audit.log(
                "cert.renew",
                "all",
                result,
                started.elapsed().as_millis() as u64,
                json!({"exit": out.code()}),
            );
            if out.ok() {
                Ok(combined)
            } else {
                Err(NgToolError::CommandFailed {
                    command: "certbot renew".into(),
                    code: out.code(),
                    stderr: combined,
                })
            }
        }
        Err(e) => {
            ctx.audit.log(
                "cert.renew",
                "all",
                AuditResult::Failure,
                started.elapsed().as_millis() as u64,
                json!({"error": e.to_string()}),
            );
            Err(e)
        }
    }
}

/// `certbot delete --cert-name <name>`：删除指定证书。
/// 用于清理孤立证书或不再需要的证书。
pub async fn delete_cert(ctx: Arc<AppContext>, cert_name: &str) -> Result<String, NgToolError> {
    let started = std::time::Instant::now();
    if !ctx.deps().certbot {
        return Err(NgToolError::DependencyMissing {
            name: "certbot".into(),
        });
    }
    if cert_name.trim().is_empty() {
        return Err(NgToolError::InvalidInput {
            field: "cert_name".into(),
            message: "证书名称不能为空".into(),
        });
    }
    // 确认弹窗与队列执行之间配置可能变化，不能仅信任证书页的候选缓存。
    let nginx_root =
        ctx.probe
            .sites_available
            .parent()
            .ok_or_else(|| NgToolError::InvalidInput {
                field: "nginx_root".into(),
                message: "无法确定 Nginx 配置目录，拒绝删除证书".into(),
            })?;
    ensure_cert_unreferenced(nginx_root, cert_name)?;
    match ctx
        .executor
        .run(
            CommandSpec::new("certbot")
                .arg("delete")
                .arg("--cert-name")
                .arg(cert_name)
                .arg("--non-interactive")
                .timeout(Duration::from_secs(30)),
        )
        .await
    {
        Ok(out) => {
            let combined = out.combined();
            let result = if out.ok() {
                AuditResult::Success
            } else {
                AuditResult::Failure
            };
            ctx.audit.log(
                "cert.delete",
                cert_name,
                result,
                started.elapsed().as_millis() as u64,
                json!({"exit": out.code()}),
            );
            if out.ok() {
                Ok(combined)
            } else {
                Err(NgToolError::CommandFailed {
                    command: format!("certbot delete --cert-name {}", cert_name),
                    code: out.code(),
                    stderr: combined,
                })
            }
        }
        Err(e) => {
            ctx.audit.log(
                "cert.delete",
                cert_name,
                AuditResult::Failure,
                started.elapsed().as_millis() as u64,
                json!({"error": e.to_string()}),
            );
            Err(e)
        }
    }
}

/// 自动续签状态检查结果（execution.md P8-8）。
#[derive(Debug, Clone, Serialize)]
pub struct AutoRenewStatus {
    pub timer_unit: String,
    pub timer_active: bool,
    pub next_run: Option<String>,
    pub deploy_hook_path: String,
    pub deploy_hook_present: bool,
    pub last_check_error: Option<String>,
}

impl AutoRenewStatus {
    pub fn healthy(&self) -> bool {
        self.timer_active && self.deploy_hook_present
    }

    /// 给出修复建议，缺失项越多列得越多。不自动修复（仅提示）。
    pub fn advice(&self) -> Vec<String> {
        let mut tips = Vec::new();
        if !self.timer_active {
            tips.push(format!(
                "启用自动续签定时器：sudo systemctl enable --now {}",
                self.timer_unit
            ));
        }
        if !self.deploy_hook_present {
            tips.push("点击 [安装 deploy hook] 按钮自动创建钩子脚本".into());
        }
        tips
    }
}

/// 证书页一次刷新的聚合快照：证书列表 + 关联 + 自动续签状态。
#[derive(Debug, Clone)]
pub struct CertsSnapshot {
    pub items: Vec<CertWithSite>,
    /// 解析失败时保留的 certbot 原始输出
    pub raw_output: Option<String>,
    pub auto_renew: AutoRenewStatus,
    pub error: Option<String>,
}

/// 一次性采集证书页所需全部数据。failure 模式下尽量保留可展示信息（R2 闭环）。
pub async fn collect_snapshot(ctx: Arc<AppContext>) -> CertsSnapshot {
    let mut error: Option<String> = None;
    let mut raw_output: Option<String> = None;

    let items = if !ctx.deps().certbot {
        error = Some("certbot 未安装".into());
        Vec::new()
    } else {
        match ctx
            .executor
            .run(
                CommandSpec::new("certbot")
                    .arg("certificates")
                    .timeout(Duration::from_secs(8)),
            )
            .await
        {
            Ok(out) => {
                let parsed = parse_certificates(&out.stdout);
                if parsed.is_empty() {
                    raw_output = Some(out.combined());
                }
                let sites = crate::domain::site::list_sites(ctx.clone())
                    .await
                    .unwrap_or_default();
                let mut items = associate_with_sites(parsed, &sites);
                if let Some(nginx_root) = ctx.probe.sites_available.parent() {
                    mark_nginx_references(&mut items, nginx_root);
                }
                items
            }
            Err(e) => {
                error = Some(e.to_string());
                Vec::new()
            }
        }
    };

    let auto_renew = check_auto_renew(ctx).await;

    CertsSnapshot {
        items,
        raw_output,
        auto_renew,
        error,
    }
}

/// 探测 certbot.timer 与 deploy hook 状态。任一探测失败时降级到默认值并记录原因。
pub async fn check_auto_renew(ctx: Arc<AppContext>) -> AutoRenewStatus {
    let timer_unit = "certbot.timer";
    let deploy_hook_path = "/etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh";

    let mut last_check_error: Option<String> = None;

    let timer_active = match ctx.systemd.is_active(timer_unit).await {
        Ok(b) => b,
        Err(e) => {
            last_check_error = Some(format!("timer 状态查询失败：{}", e));
            false
        }
    };

    let next_run = match ctx
        .executor
        .run(
            CommandSpec::new("systemctl")
                .arg("list-timers")
                .arg("--no-pager")
                .arg(timer_unit)
                .timeout(Duration::from_secs(3)),
        )
        .await
    {
        Ok(out) => parse_next_run(&out.stdout),
        Err(e) => {
            last_check_error = Some(format!("list-timers 查询失败：{}", e));
            None
        }
    };

    let deploy_hook_present = match std::fs::metadata(deploy_hook_path) {
        Ok(meta) => meta.is_file(),
        Err(_) => false,
    };

    AutoRenewStatus {
        timer_unit: timer_unit.into(),
        timer_active,
        next_run,
        deploy_hook_path: deploy_hook_path.into(),
        deploy_hook_present,
        last_check_error,
    }
}

/// 安装 certbot deploy hook，确保证书续期后自动重载 Nginx。
pub fn install_deploy_hook() -> Result<(), NgToolError> {
    let path = std::path::Path::new("/etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| NgToolError::FileOperationFailed {
            path: parent.to_path_buf(),
            message: e.to_string(),
        })?;
    }
    let script = "#!/bin/sh\nnginx -t && systemctl reload nginx\n";
    std::fs::write(path, script).map_err(|e| NgToolError::FileOperationFailed {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| {
        NgToolError::FileOperationFailed {
            path: path.to_path_buf(),
            message: e.to_string(),
        }
    })?;
    Ok(())
}

/// 从 `systemctl list-timers` 输出解析 NEXT 列。
fn parse_next_run(text: &str) -> Option<String> {
    for line in text.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('-') || trimmed.starts_with("NEXT") {
            continue;
        }
        if let Some(idx) = trimmed.find(" UTC ") {
            return Some(trimmed[..idx + 4].trim().to_string());
        }
        if let Some(idx) = trimmed.find(" CST ") {
            return Some(trimmed[..idx + 4].trim().to_string());
        }
        let cut: String = trimmed.chars().take(28).collect();
        return Some(cut.trim().to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_certbot_basic() {
        let text = r#"
Saving debug log to /var/log/letsencrypt/letsencrypt.log

Found the following certs:
  Certificate Name: app
    Serial Number: abcd
    Domains: app.example.com www.app.example.com
    Expiry Date: 2026-07-04 08:30:14+00:00 (VALID: 67 days)
    Certificate Path: /etc/letsencrypt/live/app/fullchain.pem
    Private Key Path: /etc/letsencrypt/live/app/privkey.pem
  Certificate Name: api
    Serial Number: efgh
    Domains: api.example.com
    Expiry Date: 2026-05-04 08:30:14+00:00 (VALID: 5 days)
    Certificate Path: /etc/letsencrypt/live/api/fullchain.pem
"#;
        let certs = parse_certificates(text);
        assert_eq!(certs.len(), 2);
        let app = &certs[0];
        assert_eq!(app.name, "app");
        assert_eq!(app.domains, vec!["app.example.com", "www.app.example.com"]);
        assert_eq!(app.days_left, Some(67));
        assert_eq!(app.level, Some(CertLevel::Ok));
        assert!(app.cert_path.as_deref().unwrap().contains("/app/"));

        let api = &certs[1];
        assert_eq!(api.name, "api");
        assert_eq!(api.days_left, Some(5));
        assert_eq!(api.level, Some(CertLevel::Critical));
    }

    #[test]
    fn expired_certbot_output_is_not_reused() {
        let text = r#"
  Certificate Name: blog
    Domains: yiym.site www.yiym.site
    Expiry Date: 2024-01-01 00:00:00+00:00 (INVALID: EXPIRED)
    Certificate Path: /etc/letsencrypt/live/blog/fullchain.pem
"#;
        let certs = parse_certificates(text);
        assert_eq!(certs.len(), 1);
        assert!(certs[0].expiry.is_some());
        assert_eq!(certs[0].level, Some(CertLevel::Expired));
        assert!(!cert_is_usable(&certs[0]));
        assert!(matches!(
            plan_certificate(&certs, "blog", &["yiym.site".into()]),
            CertIssuePlan::Expand { .. }
        ));
    }

    #[test]
    fn certificate_with_unknown_validity_is_not_reused() {
        let text = r#"
  Certificate Name: blog
    Domains: yiym.site
    Expiry Date: 2099-01-01 00:00:00+00:00 (UNKNOWN)
    Certificate Path: /etc/letsencrypt/live/blog/fullchain.pem
"#;
        let certs = parse_certificates(text);
        assert_eq!(certs.len(), 1);
        assert_eq!(certs[0].days_left, None);
        assert!(!cert_is_usable(&certs[0]));
        assert!(matches!(
            plan_certificate(&certs, "blog", &["yiym.site".into()]),
            CertIssuePlan::Expand { .. }
        ));
    }

    #[test]
    fn certificate_with_zero_days_must_still_be_unexpired() {
        let mut cert = sample_cert("blog", &["yiym.site"], 0);
        cert.expiry = Some(Utc::now() - chrono::Duration::minutes(1));
        assert!(!cert_is_usable(&cert));
        cert.expiry = Some(Utc::now() + chrono::Duration::hours(1));
        assert!(cert_is_usable(&cert));
    }

    #[test]
    fn parse_handles_garbage_input() {
        let certs = parse_certificates("");
        assert!(certs.is_empty());
        let certs = parse_certificates("No certs found");
        assert!(certs.is_empty());
    }

    #[test]
    fn level_thresholds() {
        assert_eq!(CertLevel::from_days(60), CertLevel::Ok);
        assert_eq!(CertLevel::from_days(30), CertLevel::Ok);
        assert_eq!(CertLevel::from_days(29), CertLevel::Warning);
        assert_eq!(CertLevel::from_days(7), CertLevel::Warning);
        assert_eq!(CertLevel::from_days(6), CertLevel::Critical);
        assert_eq!(CertLevel::from_days(0), CertLevel::Critical);
        assert_eq!(CertLevel::from_days(-1), CertLevel::Expired);
    }

    fn sample_cert(name: &str, domains: &[&str], days_left: i64) -> Certificate {
        Certificate {
            name: name.into(),
            domains: domains.iter().map(|domain| (*domain).to_string()).collect(),
            expiry: None,
            days_left: Some(days_left),
            level: Some(CertLevel::from_days(days_left)),
            cert_path: Some(format!("/etc/letsencrypt/live/{name}/fullchain.pem")),
        }
    }

    #[test]
    fn plan_reuses_certificate_that_already_covers_every_domain() {
        let certs = vec![
            sample_cert("yiym.site", &["yiym.site"], 60),
            sample_cert("bundle", &["yiym.site", "www.yiym.site"], 40),
        ];
        let plan = plan_certificate(
            &certs,
            "blog",
            &["yiym.site".into(), "www.yiym.site".into()],
        );
        assert_eq!(
            plan,
            CertIssuePlan::Reuse {
                cert_name: "bundle".into()
            }
        );
    }

    #[test]
    fn plan_prefers_site_named_lineage_when_several_certs_cover() {
        let certs = vec![
            sample_cert("bundle", &["yiym.site", "www.yiym.site"], 80),
            sample_cert("blog", &["yiym.site", "www.yiym.site"], 20),
        ];
        let plan = plan_certificate(
            &certs,
            "blog",
            &["yiym.site".into(), "www.yiym.site".into()],
        );
        assert_eq!(
            plan,
            CertIssuePlan::Reuse {
                cert_name: "blog".into()
            }
        );
    }

    #[test]
    fn plan_expands_primary_domain_when_names_are_split() {
        let certs = vec![
            sample_cert("www.yiym.site", &["www.yiym.site"], 70),
            sample_cert("yiym.site", &["yiym.site"], 60),
        ];
        let plan = plan_certificate(
            &certs,
            "blog",
            &["yiym.site".into(), "www.yiym.site".into()],
        );
        assert_eq!(
            plan,
            CertIssuePlan::Expand {
                cert_name: "yiym.site".into()
            }
        );
    }

    #[test]
    fn plan_expands_site_lineage_before_a_partial_domain_cert() {
        let certs = vec![
            sample_cert("yiym.site", &["yiym.site"], 60),
            sample_cert("blog", &["yiym.site"], 10),
        ];
        let plan = plan_certificate(
            &certs,
            "blog",
            &["yiym.site".into(), "www.yiym.site".into()],
        );
        assert_eq!(
            plan,
            CertIssuePlan::Expand {
                cert_name: "blog".into()
            }
        );
    }

    #[test]
    fn plan_does_not_reuse_an_expired_covering_certificate() {
        let certs = vec![sample_cert("blog", &["yiym.site", "www.yiym.site"], -1)];
        let plan = plan_certificate(
            &certs,
            "blog",
            &["yiym.site".into(), "www.yiym.site".into()],
        );
        assert_eq!(
            plan,
            CertIssuePlan::Expand {
                cert_name: "blog".into()
            }
        );
    }

    #[test]
    fn plan_issues_a_new_site_certificate_when_nothing_matches() {
        let plan = plan_certificate(&[], "blog", &["yiym.site".into()]);
        assert_eq!(
            plan,
            CertIssuePlan::Issue {
                cert_name: "blog".into()
            }
        );
    }

    fn issue_command_domains(spec: &CommandSpec) -> Vec<&str> {
        spec.args
            .windows(2)
            .filter(|args| args[0] == "-d")
            .map(|args| args[1].as_str())
            .collect()
    }

    #[test]
    fn expansion_command_preserves_existing_domains_and_deduplicates() {
        let certs = vec![
            sample_cert("bundle", &["a.example", "B.example."], 60),
            sample_cert("unrelated", &["other.example"], 60),
        ];
        let domains = vec!["A.example.".into(), "c.example".into(), "C.example.".into()];
        let plan = plan_certificate(&certs, "new-site", &domains);
        assert_eq!(
            plan,
            CertIssuePlan::Expand {
                cert_name: "bundle".into()
            }
        );
        let spec = cert_issue_spec("ops@example.com", false, &plan, &domains, &certs);
        assert!(spec.args.iter().any(|arg| arg == "--expand"));
        assert!(spec
            .args
            .windows(2)
            .any(|args| { args[0] == "--cert-name" && args[1] == "bundle" }));
        assert_eq!(
            issue_command_domains(&spec),
            vec!["a.example", "c.example", "b.example"]
        );
    }

    #[test]
    fn expansion_command_preserves_site_lineage_domains_without_overlap() {
        let certs = vec![sample_cert("blog", &["a.example", "b.example"], 60)];
        let domains = vec!["c.example".into()];
        let plan = plan_certificate(&certs, "blog", &domains);
        assert!(matches!(plan, CertIssuePlan::Expand { .. }));
        let spec = cert_issue_spec("ops@example.com", false, &plan, &domains, &certs);
        assert_eq!(
            issue_command_domains(&spec),
            vec!["c.example", "a.example", "b.example"]
        );
    }

    #[test]
    fn new_issue_command_does_not_include_unrelated_certificate_domains() {
        let certs = vec![sample_cert("unrelated", &["other.example"], 60)];
        let domains = vec!["a.example".into()];
        let plan = plan_certificate(&certs, "blog", &domains);
        assert!(matches!(plan, CertIssuePlan::Issue { .. }));
        let spec = cert_issue_spec("ops@example.com", false, &plan, &domains, &certs);
        assert!(!spec.args.iter().any(|arg| arg == "--expand"));
        assert_eq!(issue_command_domains(&spec), vec!["a.example"]);
    }

    #[test]
    fn associate_marks_orphan() {
        use crate::domain::site::{Site, SiteType, SslStatus};
        let certs = vec![
            Certificate {
                name: "app".into(),
                domains: vec!["app.example.com".into()],
                expiry: None,
                days_left: Some(60),
                level: Some(CertLevel::Ok),
                cert_path: None,
            },
            Certificate {
                name: "legacy".into(),
                domains: vec!["legacy.example.com".into()],
                expiry: None,
                days_left: Some(60),
                level: Some(CertLevel::Ok),
                cert_path: None,
            },
        ];
        let sites = vec![Site {
            name: "app".into(),
            primary_domain: Some("app.example.com".into()),
            all_domains: vec!["app.example.com".into()],
            access_log_path: None,
            error_log_path: None,
            site_type: SiteType::Proxy,
            target: None,
            enabled: true,
            ssl: SslStatus::None,
            config_path: std::path::PathBuf::from("/etc/nginx/sites-available/app.conf"),
        }];
        let assoc = associate_with_sites(certs, &sites);
        assert_eq!(assoc.len(), 2);
        assert_eq!(assoc[0].site_names, vec!["app"]);
        assert!(!assoc[0].orphan);
        assert!(!assoc[0].nginx_referenced);
        assert!(assoc[1].orphan);
        assert!(assoc[1].site_names.is_empty());
    }

    #[test]
    fn cleanup_protects_default_and_arbitrary_include_names() -> anyhow::Result<()> {
        for filename in ["default", "gpt.brs.qzz.io", "app.conf"] {
            let tmp = tempfile::tempdir()?;
            let nginx_root = tmp.path().join("nginx");
            let sites_enabled = nginx_root.join("sites-enabled");
            std::fs::create_dir_all(&sites_enabled)?;
            // 目标位于扫描根目录外，必须通过无扩展名的启用链接发现引用。
            let config = tmp.path().join("external-site");
            std::fs::write(
                &config,
                "server { ssl_certificate /etc/letsencrypt/live/gpt.brs.qzz.io/fullchain.pem; }",
            )?;
            std::os::unix::fs::symlink(&config, sites_enabled.join(filename))?;
            let mut items = vec![CertWithSite {
                cert: Certificate {
                    name: "gpt.brs.qzz.io".into(),
                    domains: vec!["gpt.brs.qzz.io".into()],
                    expiry: None,
                    days_left: None,
                    level: None,
                    cert_path: Some("/etc/letsencrypt/live/gpt.brs.qzz.io/fullchain.pem".into()),
                },
                site_names: Vec::new(),
                orphan: true,
                nginx_referenced: false,
            }];
            mark_nginx_references(&mut items, &nginx_root);
            assert!(cleanup_candidates(&items).is_empty(), "{filename}");
            assert!(ensure_cert_unreferenced(&nginx_root, "gpt.brs.qzz.io").is_err());
            assert!(ensure_cert_unreferenced(&nginx_root, "gpt").is_ok());
        }
        Ok(())
    }

    #[test]
    fn delete_guard_reads_current_references() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        assert!(ensure_cert_unreferenced(tmp.path(), "gpt").is_ok());
        std::fs::write(
            tmp.path().join("default"),
            "ssl_certificate /etc/letsencrypt/live/gpt/fullchain.pem;",
        )?;
        assert!(ensure_cert_unreferenced(tmp.path(), "gpt").is_err());
        Ok(())
    }

    #[test]
    fn mark_nginx_references_detects_live_cert_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let nginx_root = tmp.path().join("nginx");
        let sites_available = nginx_root.join("sites-available");
        std::fs::create_dir_all(&sites_available).unwrap();
        std::fs::write(
            sites_available.join("app.conf"),
            "server { ssl_certificate /etc/letsencrypt/live/legacy/fullchain.pem; }",
        )
        .unwrap();

        let mut items = vec![CertWithSite {
            cert: Certificate {
                name: "legacy".into(),
                domains: vec!["legacy.example.com".into()],
                expiry: None,
                days_left: Some(60),
                level: Some(CertLevel::Ok),
                cert_path: Some("/etc/letsencrypt/live/legacy/fullchain.pem".into()),
            },
            site_names: Vec::new(),
            orphan: true,
            nginx_referenced: false,
        }];

        mark_nginx_references(&mut items, &nginx_root);
        assert!(items[0].nginx_referenced);
    }

    #[test]
    fn cleanup_candidates_include_unreferenced_redundant_multidomain_cert() {
        let items = vec![
            CertWithSite {
                cert: Certificate {
                    name: "old-bundle".into(),
                    domains: vec!["app.example.com".into(), "api.example.com".into()],
                    expiry: None,
                    days_left: Some(80),
                    level: Some(CertLevel::Ok),
                    cert_path: Some("/etc/letsencrypt/live/old-bundle/fullchain.pem".into()),
                },
                site_names: vec!["app".into(), "api".into()],
                orphan: false,
                nginx_referenced: false,
            },
            CertWithSite {
                cert: Certificate {
                    name: "app.example.com".into(),
                    domains: vec!["app.example.com".into()],
                    expiry: None,
                    days_left: Some(89),
                    level: Some(CertLevel::Ok),
                    cert_path: Some("/etc/letsencrypt/live/app.example.com/fullchain.pem".into()),
                },
                site_names: vec!["app".into()],
                orphan: false,
                nginx_referenced: true,
            },
            CertWithSite {
                cert: Certificate {
                    name: "api.example.com".into(),
                    domains: vec!["api.example.com".into()],
                    expiry: None,
                    days_left: Some(79),
                    level: Some(CertLevel::Ok),
                    cert_path: Some("/etc/letsencrypt/live/api.example.com/fullchain.pem".into()),
                },
                site_names: vec!["api".into()],
                orphan: false,
                nginx_referenced: true,
            },
        ];

        let names: Vec<_> = cleanup_candidates(&items)
            .iter()
            .map(|item| item.cert.name.as_str())
            .collect();
        assert_eq!(names, vec!["old-bundle"]);
    }

    #[test]
    fn parse_next_run_line() {
        let text = "NEXT                        LEFT       LAST PASSED  UNIT          ACTIVATES\n\
             Wed 2026-04-29 03:00:00 UTC 5h 12min   - -          certbot.timer certbot.service\n\
             1 timers listed.\n";
        let next = parse_next_run(text);
        assert_eq!(next.as_deref(), Some("Wed 2026-04-29 03:00:00 UTC"));
    }
}
