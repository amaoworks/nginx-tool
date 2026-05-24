use crossterm::event::KeyEvent;

use crate::domain::backup::{Backup, RestoreOutcome};
use crate::domain::cert::CertsSnapshot;
use crate::domain::dashboard::DashboardSnapshot;
use crate::domain::site::{CreateSiteOutcome, Site};
use crate::domain::update::{SelfUpdateOutcome, UpdateInfo};
use crate::error::NgToolError;

/// 应用层统一事件，详见 architecture.md §8.2。
/// 大型负载（snapshot、site list）通过 Box 装载，控制 enum 体积。
#[derive(Debug, Clone)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Resize,
    QuitRequested,

    /// 仪表盘异步采集完成
    DashboardSnapshot(Box<DashboardSnapshot>),

    /// 站点列表异步加载完成
    SitesLoaded(Box<Result<Vec<Site>, String>>),

    /// 启用/停用站点的异步结果
    SiteToggleResult {
        site: String,
        target_enabled: bool,
        result: Box<Result<(), NgToolError>>,
    },

    /// 新建站点异步结果
    SiteCreateResult {
        site_name: String,
        result: Box<Result<CreateSiteOutcome, NgToolError>>,
    },

    /// 站点编辑保存结果
    SiteEditResult {
        site_name: String,
        saved_content: String,
        result: Box<Result<(), NgToolError>>,
    },

    /// 服务控制：测试配置完成
    ServiceTestResult(Box<Result<String, NgToolError>>),

    /// 服务控制：重载完成
    ServiceReloadResult(Box<Result<String, NgToolError>>),

    /// 服务控制：重启完成
    ServiceRestartResult(Box<Result<(), NgToolError>>),

    /// 服务控制：状态查询完成
    ServiceStatusResult(Box<Result<String, NgToolError>>),

    /// 服务控制：更新检查完成
    ServiceUpdateCheckResult(Box<Result<UpdateInfo, NgToolError>>),

    /// 服务控制：TUI 自升级完成
    ServiceUpgradeResult(Box<Result<SelfUpdateOutcome, NgToolError>>),

    /// 日志行到达
    LogTailLine {
        line: String,
    },

    /// 证书页：采集完成（证书列表 + 自动续签状态）
    CertsSnapshot(Box<CertsSnapshot>),

    /// 证书页：申请证书结果
    CertRequestResult {
        site_name: String,
        result: Box<Result<String, NgToolError>>,
    },

    /// 证书页：续期所有结果
    CertRenewAllResult(Box<Result<String, NgToolError>>),

    /// 证书页：仅自动续签状态刷新
    CertAutoRenewResult(Box<crate::domain::cert::AutoRenewStatus>),

    /// 证书页：安装 deploy hook 结果
    CertInstallHookResult(Box<Result<(), NgToolError>>),

    /// 备份列表加载完成
    BackupListLoaded(Box<Result<Vec<Backup>, String>>),

    /// 创建备份完成
    BackupCreateResult(Box<Result<std::path::PathBuf, NgToolError>>),

    /// 删除备份完成
    BackupDeleteResult(Box<Result<(), NgToolError>>),

    /// 还原备份完成
    BackupRestoreResult(Box<Result<RestoreOutcome, NgToolError>>),

    /// 删除站点完成
    SiteDeleteResult {
        site_name: String,
        result: Box<Result<(), NgToolError>>,
    },
}
