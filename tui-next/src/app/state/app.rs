//! 全局 AppState 与事件 / 按键处理。

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::event::AppEvent;
use crate::app::route::{MenuItem, Route, SitesRoute};
use crate::app::state::backup::BackupState;
use crate::app::state::certs::{CertsAction, CertsState};
use crate::app::state::common::{FocusArea, Notification, RunMode};
use crate::app::state::dashboard::DashboardState;
use crate::app::state::logs::LogsState;
use crate::app::state::service::{ServiceButton, ServiceState};
use crate::app::state::site_edit::SiteEditState;
use crate::app::state::site_form::SiteFormState;
use crate::app::state::sites::SitesState;
use crate::infra::AppContext;
use crate::ui::modal::Modal;

/// 仪表盘自动刷新间隔
pub(crate) const DASHBOARD_AUTO_REFRESH: Duration = Duration::from_secs(30);
/// 站点列表自动刷新间隔
pub(crate) const SITES_AUTO_REFRESH: Duration = Duration::from_secs(60);
/// 证书页自动刷新间隔
pub(crate) const CERTS_AUTO_REFRESH: Duration = Duration::from_secs(120);
pub(crate) const LOGS_HORIZONTAL_SCROLL_STEP: i16 = 8;
pub(crate) const LOGS_PAGE_SCROLL_FACTOR: usize = 1;

/// 全局应用状态。详见 architecture.md §7.1。
pub struct AppState {
    pub run_mode: RunMode,
    pub route: Route,
    pub focus: FocusArea,
    pub notification: Option<Notification>,
    pub modal: Option<Modal>,
    pub should_quit: bool,
    pub last_tick: Instant,
    pub ctx: Arc<AppContext>,
    pub dashboard: DashboardState,
    pub sites: SitesState,
    pub service: ServiceState,
    pub site_form: SiteFormState,
    pub logs: LogsState,
    pub site_edit: SiteEditState,
    pub certs: CertsState,
    pub backup: BackupState,
}

impl AppState {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        let run_mode = match &ctx.readonly_reason {
            Some(reason) => RunMode::ReadOnly {
                reason: reason.clone(),
            },
            None => RunMode::ReadWrite,
        };
        Self {
            run_mode,
            route: Route::Dashboard,
            focus: FocusArea::Sidebar,
            notification: None,
            modal: None,
            should_quit: false,
            last_tick: Instant::now(),
            ctx,
            dashboard: DashboardState {
                pending_refresh: true,
                ..Default::default()
            },
            sites: SitesState::default(),
            service: ServiceState::default(),
            site_form: SiteFormState::default(),
            logs: LogsState::default(),
            site_edit: SiteEditState::default(),
            certs: CertsState::default(),
            backup: BackupState::default(),
        }
    }

    pub fn current_menu(&self) -> MenuItem {
        match self.route {
            Route::Dashboard => MenuItem::Dashboard,
            Route::Sites(_) => MenuItem::Sites,
            Route::Certs => MenuItem::Certs,
            Route::Logs => MenuItem::Logs,
            Route::Service => MenuItem::Service,
            Route::Backup => MenuItem::Backup,
        }
    }

    pub(crate) fn logs_visible_lines_estimate(&self) -> usize {
        crossterm::terminal::size()
            .map(|(_, rows)| rows.saturating_sub(7) as usize)
            .unwrap_or(10)
            .max(1)
    }

    pub fn handle_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Key(k) => self.handle_key(k),
            AppEvent::Tick => {
                self.last_tick = Instant::now();
                self.maybe_auto_refresh_dashboard();
                self.maybe_auto_refresh_sites();
                self.maybe_auto_refresh_certs();
            }
            AppEvent::Resize => {
                let visible_lines = self.logs_visible_lines_estimate();
                self.logs.clamp_scroll(visible_lines);
                if !self.logs.paused {
                    self.logs.follow_tail(visible_lines);
                }
            }
            AppEvent::QuitRequested => self.should_quit = true,
            AppEvent::DashboardSnapshot(s) => {
                self.dashboard.snapshot = Some(*s);
                self.dashboard.refreshing = false;
                self.dashboard.last_refresh = Some(Instant::now());
            }
            AppEvent::SitesLoaded(b) => {
                self.sites.refreshing = false;
                self.sites.last_refresh = Some(Instant::now());
                match *b {
                    Ok(list) => {
                        self.sites.last_error = None;
                        let list_len = list.len();
                        self.sites.replace_list(list);
                        if self.certs.site_selector_index >= list_len {
                            self.certs.site_selector_index = list_len.saturating_sub(1);
                        }
                    }
                    Err(msg) => {
                        self.sites.last_error = Some(msg);
                    }
                }
            }
            AppEvent::SiteToggleResult {
                site,
                target_enabled,
                result,
            } => {
                self.sites.action_in_flight = None;
                match *result {
                    Ok(()) => {
                        let verb = if target_enabled {
                            "已启用"
                        } else {
                            "已停用"
                        };
                        self.notification =
                            Some(Notification::success(format!("站点 {} {}", site, verb)));
                        self.sites.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("操作失败：{}", e)));
                        self.sites.pending_refresh = true;
                    }
                }
            }
            AppEvent::SiteDeleteResult { site_name, result } => {
                self.sites.action_in_flight = None;
                match *result {
                    Ok(()) => {
                        self.notification =
                            Some(Notification::success(format!("站点 {} 已删除", site_name)));
                        self.sites.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("删除失败：{}", e)));
                        self.sites.pending_refresh = true;
                    }
                }
            }
            AppEvent::ServiceTestResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(out) => {
                        self.service.push_output(["── 测试配置 ──".into()]);
                        self.service.push_output(out.lines().map(String::from));
                        self.notification = Some(Notification::success("测试通过".to_string()));
                    }
                    Err(e) => {
                        self.service.push_output(["── 测试失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification =
                            Some(Notification::failure("nginx -t 失败".to_string()));
                    }
                }
            }
            AppEvent::ServiceReloadResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(out) => {
                        self.service.push_output(["── 重载完成 ──".into()]);
                        self.service.push_output(out.lines().map(String::from));
                        self.notification = Some(Notification::success("Nginx 已重载".to_string()));
                    }
                    Err(e) => {
                        self.service.push_output(["── 重载失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("重载失败".to_string()));
                    }
                }
            }
            AppEvent::ServiceRestartResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(()) => {
                        self.service.push_output(["── 重启完成 ──".into()]);
                        self.notification = Some(Notification::success("Nginx 已重启".to_string()));
                    }
                    Err(e) => {
                        self.service.push_output(["── 重启失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("重启失败".to_string()));
                    }
                }
            }
            AppEvent::ServiceStatusResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(out) => {
                        self.service.push_output(["── systemctl status ──".into()]);
                        self.service.push_output(out.lines().map(String::from));
                    }
                    Err(e) => {
                        self.service.push_output(["── 状态查询失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                    }
                }
            }
            AppEvent::ServiceUpdateCheckResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(info) => {
                        self.service.push_output(["── 版本检查 ──".into()]);
                        self.service.push_output([
                            format!("当前版本：{}", info.current_version),
                            format!("最新版本：{}", info.latest_version),
                            format!("发布页面：{}", info.release_url),
                        ]);
                        if let Some(published_at) = info.published_at.clone() {
                            self.service
                                .push_output([format!("发布时间：{}", published_at)]);
                        }
                        self.service.update_info = Some(info.clone());
                        if info.has_update {
                            self.modal = Some(crate::ui::modal::Modal::confirm_upgrade_tui(
                                &info.current_version,
                                &info.latest_version,
                            ));
                        } else {
                            self.notification =
                                Some(Notification::success("当前已是最新版本".to_string()));
                        }
                    }
                    Err(e) => {
                        self.service.push_output(["── 版本检查失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("检查更新失败".to_string()));
                    }
                }
            }
            AppEvent::ServiceUpgradeResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(outcome) => {
                        self.service.update_info = Some(outcome.info.clone());
                        self.service.push_output(["── TUI 更新 ──".into()]);
                        if outcome.updated {
                            self.service.push_output([
                                format!(
                                    "已更新：{} -> {}",
                                    outcome.info.current_version, outcome.info.latest_version
                                ),
                                format!("二进制：{}", outcome.binary_path.display()),
                                "请退出并重新启动 ngtool 以运行新版本。".to_string(),
                            ]);
                            self.notification = Some(Notification::success(
                                "TUI 已更新，重启 ngtool 后生效".to_string(),
                            ));
                        } else {
                            self.service.push_output([
                                format!("当前版本：{}", outcome.info.current_version),
                                format!("最新版本：{}", outcome.info.latest_version),
                                "无需更新。".to_string(),
                            ]);
                            self.notification =
                                Some(Notification::success("当前已是最新版本".to_string()));
                        }
                    }
                    Err(e) => {
                        self.service.push_output(["── TUI 更新失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("TUI 更新失败".to_string()));
                    }
                }
            }
            AppEvent::SiteCreateResult { site_name, result } => {
                self.site_form.submitting = false;
                match *result {
                    Ok(crate::domain::site::CreateSiteOutcome::Ok { cert_requested }) => {
                        let msg = if cert_requested {
                            format!("站点 {} 已创建并已申请证书", site_name)
                        } else {
                            format!("站点 {} 已创建", site_name)
                        };
                        self.notification = Some(Notification::success(msg));
                        self.site_form = SiteFormState::default(); // 清空表单
                        self.route = Route::Sites(SitesRoute::List);
                        self.sites.pending_refresh = true;
                        if cert_requested {
                            self.certs.pending_refresh = true;
                        }
                    }
                    Ok(crate::domain::site::CreateSiteOutcome::CertFailed { error }) => {
                        self.notification = Some(Notification::info(format!(
                            "站点 {} 已创建，但证书申请失败：{}",
                            site_name, error
                        )));
                        self.site_form = SiteFormState::default();
                        self.route = Route::Sites(SitesRoute::List);
                        self.sites.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("创建失败：{}", e)));
                    }
                }
            }
            AppEvent::LogTailLine { line } => {
                let visible_lines = self.logs_visible_lines_estimate();
                let follow_tail = self.logs.is_following_tail(visible_lines);
                self.logs.push_line(line);
                if follow_tail && !self.logs.paused {
                    self.logs.follow_tail(visible_lines);
                }
                // 如果有搜索，重新计算匹配
                let query = self.logs.search_query.clone();
                if let Some(q) = query {
                    self.logs.search(&q);
                }
            }
            AppEvent::SiteEditResult {
                site_name,
                saved_content,
                result,
            } => {
                self.site_edit.saving = false;
                match *result {
                    Ok(()) => {
                        self.notification =
                            Some(Notification::success(format!("站点 {} 已保存", site_name)));
                        self.sites.pending_refresh = true;
                        self.certs.pending_refresh = true;
                        if self.site_edit.exit_after_save {
                            self.site_edit = SiteEditState::default();
                            self.route = Route::Sites(SitesRoute::List);
                        } else {
                            let mtime = std::fs::metadata(
                                self.ctx
                                    .probe
                                    .sites_available
                                    .join(format!("{}.conf", site_name)),
                            )
                            .ok()
                            .and_then(|m| m.modified().ok());
                            self.site_edit.mark_saved(&saved_content, mtime);
                        }
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("保存失败：{}", e)));
                        self.site_edit.exit_after_save = false;
                    }
                }
            }
            AppEvent::CertsSnapshot(snap) => {
                self.certs.refreshing = false;
                self.certs.last_refresh = Some(Instant::now());
                let snap = *snap;
                self.certs.list = snap.items;
                self.certs.raw_output = snap.raw_output;
                self.certs.auto_renew = Some(snap.auto_renew);
                self.certs.last_error = snap.error;
            }
            AppEvent::CertRequestResult { site_name, result } => {
                self.certs.running = None;
                match *result {
                    Ok(out) => {
                        self.certs.push_output(["── 证书申请输出 ──".into()]);
                        self.certs.push_output(out.lines().map(String::from));
                        self.notification = Some(Notification::success(format!(
                            "已为站点 {} 申请证书",
                            site_name
                        )));
                        self.certs.pending_refresh = true;
                    }
                    Err(e) => {
                        self.certs.push_output(["── 证书申请失败 ──".into()]);
                        self.certs
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure(format!("申请失败：{}", e)));
                    }
                }
            }
            AppEvent::CertRenewAllResult(b) => {
                self.certs.running = None;
                match *b {
                    Ok(out) => {
                        self.certs.push_output(["── certbot renew ──".into()]);
                        self.certs.push_output(out.lines().map(String::from));
                        self.notification =
                            Some(Notification::success("续期流程已完成".to_string()));
                        self.certs.pending_refresh = true;
                    }
                    Err(e) => {
                        self.certs.push_output(["── 续期失败 ──".into()]);
                        self.certs
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("续期失败".to_string()));
                    }
                }
            }
            AppEvent::CertAutoRenewResult(b) => {
                self.certs.running = None;
                let status = *b;
                self.certs.push_output(["── 自动续签状态 ──".into()]);
                self.certs.push_output([
                    format!(
                        "{} 定时器：{}",
                        if status.timer_active { "✓" } else { "⚠" },
                        status.timer_unit
                    ),
                    format!(
                        "{} deploy hook：{}",
                        if status.deploy_hook_present {
                            "✓"
                        } else {
                            "⚠"
                        },
                        status.deploy_hook_path
                    ),
                ]);
                if let Some(next) = status.next_run.clone() {
                    self.certs.push_output([format!("  下次执行：{}", next)]);
                }
                if let Some(err) = status.last_check_error.clone() {
                    self.certs.push_output([format!("  探测错误：{}", err)]);
                }
                for tip in status.advice() {
                    self.certs.push_output([format!("• {}", tip)]);
                }
                self.certs.auto_renew = Some(status);
            }
            AppEvent::CertInstallHookResult(b) => {
                self.certs.running = None;
                match *b {
                    Ok(()) => {
                        self.certs.push_output([
                            "── 安装 deploy hook ──".into(),
                            "已创建 /etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh".into(),
                        ]);
                        self.notification =
                            Some(Notification::success("deploy hook 安装成功".to_string()));
                        // 立即刷新自动续签状态以更新 UI
                        self.certs.pending_refresh = true;
                    }
                    Err(e) => {
                        self.certs
                            .push_output(["── 安装 deploy hook 失败 ──".into()]);
                        self.certs
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification =
                            Some(Notification::failure("deploy hook 安装失败".to_string()));
                    }
                }
            }
            AppEvent::CertDeleteResult { cert_name, result } => match *result {
                Ok(out) => {
                    self.certs.delete_in_flight = None;
                    self.certs
                        .push_output([format!("── 删除证书 {} ──", cert_name)]);
                    self.certs.push_output(out.lines().map(String::from));
                    if self.certs.pending_delete.is_empty() {
                        self.certs.running = None;
                        self.notification =
                            Some(Notification::success(format!("证书 {} 已删除", cert_name)));
                        self.certs.pending_refresh = true;
                    } else {
                        self.notification = Some(Notification::info(format!(
                            "证书 {} 已删除，继续处理剩余 {} 个",
                            cert_name,
                            self.certs.pending_delete.len()
                        )));
                    }
                }
                Err(e) => {
                    self.certs.running = None;
                    self.certs.delete_in_flight = None;
                    self.certs.pending_delete.clear();
                    self.certs
                        .push_output([format!("── 删除证书 {} 失败 ──", cert_name)]);
                    self.certs
                        .push_output(e.to_string().lines().map(String::from));
                    self.notification = Some(Notification::failure(format!("删除证书失败：{}", e)));
                }
            },
            AppEvent::BackupListLoaded(b) => {
                self.backup.refreshing = false;
                self.backup.last_refresh = Some(Instant::now());
                match *b {
                    Ok(list) => {
                        self.backup.last_error = None;
                        if !list.is_empty() && self.backup.selected >= list.len() {
                            self.backup.selected = list.len() - 1;
                        }
                        self.backup.list = list;
                    }
                    Err(msg) => {
                        self.backup.last_error = Some(msg);
                    }
                }
            }
            AppEvent::BackupCreateResult(b) => {
                self.backup.running = false;
                match *b {
                    Ok(p) => {
                        let name = p
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("(unnamed)")
                            .to_string();
                        self.notification =
                            Some(Notification::success(format!("已创建备份 {}", name)));
                        self.backup
                            .push_output([format!("✓ 已创建：{}", p.display())]);
                        self.backup.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("备份失败：{}", e)));
                        self.backup.push_output([format!("✗ 备份失败：{}", e)]);
                    }
                }
            }
            AppEvent::BackupDeleteResult(b) => {
                self.backup.running = false;
                match *b {
                    Ok(()) => {
                        self.notification = Some(Notification::success("已删除备份".to_string()));
                        self.backup.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("删除失败：{}", e)));
                    }
                }
            }
            AppEvent::BackupRestoreResult(b) => {
                self.backup.running = false;
                match *b {
                    Ok(crate::domain::backup::RestoreOutcome::Ok { pre_restore }) => {
                        self.notification = Some(Notification::success("还原完成".to_string()));
                        self.backup.push_output([
                            "✓ 还原成功".into(),
                            format!("  pre-restore 备份：{}", pre_restore.display()),
                        ]);
                        self.backup.pending_refresh = true;
                    }
                    Ok(crate::domain::backup::RestoreOutcome::FailedRolledBack {
                        error,
                        pre_restore,
                    }) => {
                        self.notification = Some(Notification::failure(
                            "还原失败，已回滚到 pre-restore".to_string(),
                        ));
                        self.backup.push_output([
                            "⚠ 还原失败，已自动回滚".into(),
                            format!("  错误：{}", error),
                            format!("  pre-restore 备份保留：{}", pre_restore.display()),
                        ]);
                        self.backup.pending_refresh = true;
                    }
                    Ok(crate::domain::backup::RestoreOutcome::FailedRollbackFailed {
                        error,
                        rollback_error,
                        pre_restore,
                    }) => {
                        self.notification = Some(Notification::failure(
                            "还原失败且回滚失败，需手工干预".to_string(),
                        ));
                        self.backup.push_output([
                            "✗ 还原失败且回滚也失败，请人工干预".into(),
                            format!("  原始错误：{}", error),
                            format!("  回滚错误：{}", rollback_error),
                            format!("  pre-restore 备份：{}", pre_restore.display()),
                        ]);
                        self.backup.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("还原失败：{}", e)));
                        self.backup.push_output([format!("✗ 还原失败：{}", e)]);
                    }
                }
            }
        }
    }

    fn maybe_auto_refresh_dashboard(&mut self) {
        if !matches!(self.route, Route::Dashboard) {
            return;
        }
        if self.dashboard.refreshing || self.dashboard.pending_refresh {
            return;
        }
        let due = match self.dashboard.last_refresh {
            None => true,
            Some(t) => t.elapsed() >= DASHBOARD_AUTO_REFRESH,
        };
        if due {
            self.dashboard.pending_refresh = true;
        }
    }

    fn maybe_auto_refresh_sites(&mut self) {
        if !matches!(self.route, Route::Sites(_)) {
            return;
        }
        if self.sites.refreshing || self.sites.pending_refresh {
            return;
        }
        let due = match self.sites.last_refresh {
            None => true,
            Some(t) => t.elapsed() >= SITES_AUTO_REFRESH,
        };
        if due {
            self.sites.pending_refresh = true;
        }
    }

    fn maybe_auto_refresh_certs(&mut self) {
        if !matches!(self.route, Route::Certs) {
            return;
        }
        if self.certs.refreshing || self.certs.pending_refresh {
            return;
        }
        let due = match self.certs.last_refresh {
            None => true,
            Some(t) => t.elapsed() >= CERTS_AUTO_REFRESH,
        };
        if due {
            self.certs.pending_refresh = true;
        }
    }

    /// 主循环消费：取出仪表盘待刷新意图。
    pub fn take_dashboard_refresh_request(&mut self) -> bool {
        if self.dashboard.pending_refresh && !self.dashboard.refreshing {
            self.dashboard.pending_refresh = false;
            self.dashboard.refreshing = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出站点待刷新意图。
    pub fn take_sites_refresh_request(&mut self) -> bool {
        if self.sites.pending_refresh && !self.sites.refreshing {
            self.sites.pending_refresh = false;
            self.sites.refreshing = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出站点启停意图。返回 (站点名, 目标启用状态)。
    pub fn take_site_toggle_request(&mut self) -> Option<(String, bool)> {
        self.sites.pending_toggle.take()
    }

    /// 主循环消费：取出待删除站点请求
    pub fn take_site_delete_request(&mut self) -> Option<String> {
        self.sites.pending_delete.take()
    }

    /// 主循环消费：取出服务页待执行按钮
    pub fn take_service_action(&mut self) -> Option<ServiceButton> {
        self.service.pending_action.take()
    }

    /// 主循环消费：是否执行 TUI 自升级
    pub fn take_service_upgrade(&mut self) -> bool {
        if self.service.pending_upgrade {
            self.service.pending_upgrade = false;
            true
        } else {
            false
        }
    }

    /// 通知到期后清除
    pub fn expire_notification_if_due(&mut self) {
        if let Some(n) = &self.notification {
            if Instant::now() >= n.expires_at {
                self.notification = None;
            }
        }
    }

    /// 主循环消费：取出待创建站点请求
    pub fn take_site_create_request(&mut self) -> Option<crate::domain::site::CreateSiteInput> {
        self.site_form.pending_create.take()
    }

    /// 主循环消费：取出待保存站点请求
    pub fn take_site_save_request(&mut self) -> Option<crate::domain::site::SaveSiteInput> {
        self.site_edit.pending_save.take()
    }

    /// 主循环消费：取出日志源变更请求（用于启动新的 tail 任务）
    pub fn take_logs_tail_change_request(&mut self) -> bool {
        if self.logs.pending_tail_change {
            self.logs.pending_tail_change = false;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出证书页刷新意图。
    pub fn take_certs_refresh_request(&mut self) -> bool {
        if self.certs.pending_refresh && !self.certs.refreshing {
            self.certs.pending_refresh = false;
            self.certs.refreshing = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出证书申请请求 (站点名, 域名列表)
    pub fn take_cert_request(&mut self) -> Option<(String, Vec<String>)> {
        if self.certs.pending_request.is_some() {
            self.certs.running = Some(CertsAction::Request);
        }
        self.certs.pending_request.take()
    }

    /// 主循环消费：取出续期所有证书请求
    pub fn take_cert_renew_all(&mut self) -> bool {
        if self.certs.pending_renew {
            self.certs.pending_renew = false;
            self.certs.running = Some(CertsAction::RenewAll);
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出自动续签状态检查请求
    pub fn take_cert_check_auto_renew(&mut self) -> bool {
        if self.certs.pending_check_renew {
            self.certs.pending_check_renew = false;
            self.certs.running = Some(CertsAction::CheckAutoRenew);
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出安装 deploy hook 请求
    pub fn take_cert_install_hook(&mut self) -> bool {
        if self.certs.pending_install_hook {
            self.certs.pending_install_hook = false;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出证书删除请求。
    pub fn take_cert_delete(&mut self) -> Option<String> {
        if self.certs.delete_in_flight.is_some() {
            return None;
        }
        let cert_name = self.certs.pending_delete.pop_front()?;
        self.certs.delete_in_flight = Some(cert_name.clone());
        Some(cert_name)
    }

    /// 主循环消费：取出备份页刷新意图。
    pub fn take_backup_refresh_request(&mut self) -> bool {
        if self.backup.pending_refresh && !self.backup.refreshing {
            self.backup.pending_refresh = false;
            self.backup.refreshing = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出创建备份请求
    pub fn take_backup_create(&mut self) -> bool {
        if self.backup.pending_create {
            self.backup.pending_create = false;
            self.backup.running = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出删除备份请求
    pub fn take_backup_delete(&mut self) -> Option<std::path::PathBuf> {
        if self.backup.pending_delete.is_some() {
            self.backup.running = true;
        }
        self.backup.pending_delete.take()
    }

    /// 主循环消费：取出还原备份请求
    pub fn take_backup_restore(&mut self) -> Option<std::path::PathBuf> {
        if self.backup.pending_restore.is_some() {
            self.backup.running = true;
        }
        self.backup.pending_restore.take()
    }
}

