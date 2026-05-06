//! 仪表盘数据采集，对应 design.md 视图 1、architecture.md §11.2。
//! 七项数据并发执行，单命令 3s 超时，整次刷新 5s 上限；
//! 单项失败不影响其他项，失败原因保留以便 UI 展示。

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::time::timeout;

use crate::infra::executor::CommandSpec;
use crate::infra::AppContext;

const PER_PROBE: Duration = Duration::from_secs(3);

/// 单项采集结果。`Ok` 携带数据，`Err` 携带可读原因，`Timeout` 表示采集超时。
#[derive(Debug, Clone, Serialize)]
pub enum ProbeOutcome<T> {
    Ok(T),
    Err(String),
    Timeout,
}

impl<T> ProbeOutcome<T> {
    pub fn as_ok(&self) -> Option<&T> {
        if let ProbeOutcome::Ok(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn warning_glyph(&self) -> &'static str {
        match self {
            ProbeOutcome::Ok(_) => "",
            ProbeOutcome::Err(_) | ProbeOutcome::Timeout => " ⚠",
        }
    }

    pub fn error_message(&self) -> Option<&str> {
        match self {
            ProbeOutcome::Err(m) => Some(m.as_str()),
            ProbeOutcome::Timeout => Some("超时"),
            ProbeOutcome::Ok(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardSnapshot {
    pub nginx_active: ProbeOutcome<bool>,
    pub nginx_version: ProbeOutcome<String>,
    pub enabled_count: ProbeOutcome<usize>,
    pub certs: ProbeOutcome<CertSummary>,
    pub disk: ProbeOutcome<DiskInfo>,
    pub memory: ProbeOutcome<MemoryInfo>,
    pub recent_errors: ProbeOutcome<Vec<String>>,
    /// 完成本次采集所用毫秒数
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct CertSummary {
    pub total: usize,
    pub critical: usize, // < 7d
    pub warning: usize,  // 7-30d
    pub ok: usize,       // > 30d
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskInfo {
    pub used: String,
    pub total: String,
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryInfo {
    pub used: String,
    pub total: String,
}

/// 并发采集仪表盘所有数据项。每项各自带 3s 超时，整体调用方应再用 5s 兜底。
pub async fn collect(ctx: Arc<AppContext>) -> DashboardSnapshot {
    let started = Instant::now();

    let f1 = with_timeout(probe_nginx_active(ctx.clone()));
    let f2 = with_timeout(probe_nginx_version(ctx.clone()));
    let f3 = with_timeout(probe_enabled_count(ctx.clone()));
    let f4 = with_timeout(probe_certs(ctx.clone()));
    let f5 = with_timeout(probe_disk(ctx.clone()));
    let f6 = with_timeout(probe_memory(ctx.clone()));
    let f7 = with_timeout(probe_recent_errors(ctx.clone()));

    let (a, b, c, d, e, f, g) = tokio::join!(f1, f2, f3, f4, f5, f6, f7);

    DashboardSnapshot {
        nginx_active: a,
        nginx_version: b,
        enabled_count: c,
        certs: d,
        disk: e,
        memory: f,
        recent_errors: g,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

async fn with_timeout<F, T>(fut: F) -> ProbeOutcome<T>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    match timeout(PER_PROBE, fut).await {
        Ok(Ok(v)) => ProbeOutcome::Ok(v),
        Ok(Err(e)) => ProbeOutcome::Err(e),
        Err(_) => ProbeOutcome::Timeout,
    }
}

async fn probe_nginx_active(ctx: Arc<AppContext>) -> Result<bool, String> {
    if !ctx.deps().systemctl {
        return Err("systemctl 缺失".into());
    }
    ctx.systemd
        .is_active("nginx")
        .await
        .map_err(|e| e.to_string())
}

async fn probe_nginx_version(ctx: Arc<AppContext>) -> Result<String, String> {
    if !ctx.deps().nginx {
        return Err("nginx 缺失".into());
    }
    ctx.nginx.version().await.map_err(|e| e.to_string())
}

async fn probe_enabled_count(ctx: Arc<AppContext>) -> Result<usize, String> {
    let path = ctx.probe.sites_enabled.clone();
    tokio::task::spawn_blocking(move || -> Result<usize, String> {
        if !path.is_dir() {
            return Err(format!("目录不存在：{}", path.display()));
        }
        let mut n = 0;
        for entry in std::fs::read_dir(&path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) == Some("conf") {
                n += 1;
            }
        }
        Ok(n)
    })
    .await
    .map_err(|e| format!("任务异常：{}", e))?
}

async fn probe_certs(ctx: Arc<AppContext>) -> Result<CertSummary, String> {
    if !ctx.deps().certbot {
        return Err("certbot 缺失".into());
    }
    let out = ctx
        .executor
        .run(
            CommandSpec::new("certbot")
                .arg("certificates")
                .timeout(PER_PROBE),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(parse_certbot_summary(&out.stdout))
}

pub(crate) fn parse_certbot_summary(text: &str) -> CertSummary {
    // 形如 "    Expiry Date: 2026-07-04 08:30:14+00:00 (VALID: 67 days)"
    let re = regex::Regex::new(r"VALID:\s*(\d+)\s*day").unwrap();
    let mut total = 0;
    let mut critical = 0;
    let mut warning = 0;
    let mut ok = 0;
    for cap in re.captures_iter(text) {
        total += 1;
        if let Some(m) = cap.get(1) {
            if let Ok(d) = m.as_str().parse::<i64>() {
                if d < 7 {
                    critical += 1;
                } else if d < 30 {
                    warning += 1;
                } else {
                    ok += 1;
                }
            }
        }
    }
    CertSummary {
        total,
        critical,
        warning,
        ok,
    }
}

async fn probe_disk(ctx: Arc<AppContext>) -> Result<DiskInfo, String> {
    let out = ctx
        .executor
        .run(CommandSpec::new("df").arg("-h").arg("/").timeout(PER_PROBE))
        .await
        .map_err(|e| e.to_string())?;
    parse_df(&out.stdout)
}

pub(crate) fn parse_df(text: &str) -> Result<DiskInfo, String> {
    // df -h / 第二行：Filesystem Size Used Avail Use% Mounted
    let line = text.lines().nth(1).ok_or("df 输出无第二行")?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 5 {
        return Err(format!("df 输出列数不足：{}", line));
    }
    let total = cols[1].to_string();
    let used = cols[2].to_string();
    let percent = cols[4]
        .trim_end_matches('%')
        .parse::<u8>()
        .map_err(|e| e.to_string())?;
    Ok(DiskInfo {
        used,
        total,
        percent,
    })
}

async fn probe_memory(ctx: Arc<AppContext>) -> Result<MemoryInfo, String> {
    let out = ctx
        .executor
        .run(CommandSpec::new("free").arg("-h").timeout(PER_PROBE))
        .await
        .map_err(|e| e.to_string())?;
    parse_free(&out.stdout)
}

pub(crate) fn parse_free(text: &str) -> Result<MemoryInfo, String> {
    // 第二行：Mem: total used free shared buff/cache available
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("Mem:") {
            let cols: Vec<&str> = trimmed.split_whitespace().collect();
            if cols.len() >= 3 {
                return Ok(MemoryInfo {
                    used: cols[2].to_string(),
                    total: cols[1].to_string(),
                });
            }
        }
    }
    Err("无法解析 free 输出".into())
}

async fn probe_recent_errors(_ctx: Arc<AppContext>) -> Result<Vec<String>, String> {
    let path = std::path::PathBuf::from("/var/log/nginx/error.log");
    tokio::task::spawn_blocking(move || -> Result<Vec<String>, String> {
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("{}：{}", path.display(), e))?;
        // 取最后 3 行非空
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        let take = lines.len().saturating_sub(3);
        Ok(lines[take..].iter().map(|s| s.to_string()).collect())
    })
    .await
    .map_err(|e| format!("任务异常：{}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_certbot_buckets() {
        let text = "
        Certificate Name: app
            Expiry Date: 2026-07-04 08:30:14+00:00 (VALID: 67 days)
        Certificate Name: api
            Expiry Date: 2026-05-21 08:30:14+00:00 (VALID: 23 days)
        Certificate Name: legacy
            Expiry Date: 2026-05-04 08:30:14+00:00 (VALID: 5 days)
        ";
        let s = parse_certbot_summary(text);
        assert_eq!(s.total, 3);
        assert_eq!(s.ok, 1);
        assert_eq!(s.warning, 1);
        assert_eq!(s.critical, 1);
    }

    #[test]
    fn parse_df_basic() {
        let text =
            "Filesystem      Size  Used Avail Use% Mounted on\n/dev/root        58G   12G   44G  21% /\n";
        let info = parse_df(text).unwrap();
        assert_eq!(info.total, "58G");
        assert_eq!(info.used, "12G");
        assert_eq!(info.percent, 21);
    }

    #[test]
    fn parse_free_basic() {
        let text = "              total        used        free      shared  buff/cache   available\nMem:           3.8G        1.2G        450M         12M        2.1G        2.4G\nSwap:          1.0G        100M        924M\n";
        let info = parse_free(text).unwrap();
        assert_eq!(info.total, "3.8G");
        assert_eq!(info.used, "1.2G");
    }
}
