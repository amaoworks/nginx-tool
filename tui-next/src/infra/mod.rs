// 脚手架阶段：infra 模块作为后续 P3-P9 的 API 表面构建，
// 这里允许“尚未被业务代码调用”的项；每接入一阶段，对应函数即变为已用。
#![allow(dead_code)]

pub mod archive;
pub mod audit;
pub mod certbot;
pub mod executor;
pub mod filesystem;
pub mod flock;
pub mod log_tail;
pub mod nginx;
pub mod paths;
pub mod permission;
pub mod systemd;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;

use crate::config::settings::AppSettings;
use crate::infra::audit::AuditLogger;
use crate::infra::executor::CommandExecutor;
use crate::infra::flock::{LockState, SingleInstanceLock};
use crate::infra::nginx::NginxAdapter;
use crate::infra::paths::AppPaths;
use crate::infra::permission::{Dependencies, EnvironmentProbe};
use crate::infra::systemd::SystemdAdapter;

/// 启动期对外部环境与依赖的一次性探测结果，
/// 在主循环全程作为只读上下文使用。
pub struct AppContext {
    pub paths: AppPaths,
    pub settings: AppSettings,
    pub probe: EnvironmentProbe,
    pub readonly_reason: Option<String>,
    pub executor: CommandExecutor,
    pub nginx: NginxAdapter,
    pub systemd: SystemdAdapter,
    pub audit: Arc<AuditLogger>,
    /// flock 实例，Drop 即释放。None = 让位给已有实例（降级只读）。
    pub _lock: Option<SingleInstanceLock>,
}

impl AppContext {
    pub fn deps(&self) -> &Dependencies {
        &self.probe.deps
    }

    pub fn is_root(&self) -> bool {
        self.probe.is_root
    }

    pub fn readonly(&self) -> bool {
        self.readonly_reason.is_some()
    }
}

pub struct BootstrapOptions {
    pub force_readonly: bool,
    pub config_override: Option<std::path::PathBuf>,
}

/// 启动期一次性初始化：建目录、回收 tmp、抢 flock、探依赖、构建 executor + audit。
/// 任何步骤失败都不应直接退出 TUI；目录/锁失败时自动降级为只读模式并保留原因。
pub fn bootstrap(opts: BootstrapOptions) -> anyhow::Result<AppContext> {
    let paths = AppPaths::detect().context("解析应用目录失败")?;
    let config_path = opts
        .config_override
        .clone()
        .unwrap_or_else(|| paths.config_file.clone());
    let settings = AppSettings::load(&config_path);

    let mut readonly_reason: Option<String> = if opts.force_readonly {
        Some("启动参数 --readonly".into())
    } else {
        None
    };

    let dirs_ok = paths.ensure_dirs().is_ok();
    if !dirs_ok && readonly_reason.is_none() {
        readonly_reason = Some(format!("无法创建数据目录 {}", paths.root.display()));
    }

    if let Err(e) = paths.cleanup_tmp() {
        tracing::warn!("tmp 目录回收失败: {}", e);
    }

    // 单实例 flock：失败/冲突均降级为只读，但仍允许进入 TUI
    let lock = match SingleInstanceLock::try_acquire(&paths.lock) {
        Ok(LockState::Acquired(l)) => Some(l),
        Ok(LockState::Busy) => {
            if readonly_reason.is_none() {
                readonly_reason = Some("已有实例运行（tui.lock 被占用）".into());
            }
            None
        }
        Err(e) => {
            if readonly_reason.is_none() {
                readonly_reason = Some(format!("flock 失败：{}", e));
            }
            None
        }
    };

    let probe = permission::probe();

    if !probe.is_root && readonly_reason.is_none() {
        readonly_reason = Some("非 root 用户".into());
    }
    if !probe.nginx_root_readable && readonly_reason.is_none() {
        readonly_reason = Some(format!(
            "Nginx 目录不可读：{}",
            probe.sites_available.display()
        ));
    }

    let is_root_for_exec = probe.is_root && readonly_reason.is_none();
    let executor = CommandExecutor::new(Duration::from_secs(3), is_root_for_exec);
    let nginx = NginxAdapter::new(executor.clone());
    let systemd = SystemdAdapter::new(executor.clone());

    let mode_label = if readonly_reason.is_some() {
        "read-only"
    } else {
        "read-write"
    };
    let audit = Arc::new(AuditLogger::new(
        paths.audit_log.clone(),
        permission::whoami(),
        mode_label.to_string(),
    ));

    Ok(AppContext {
        paths,
        settings,
        probe,
        readonly_reason,
        executor,
        nginx,
        systemd,
        audit,
        _lock: lock,
    })
}
