//! 全局 AppState 与事件 / 按键处理。

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::event::AppEvent;
use crate::app::route::{MenuItem, Route, SitesRoute};
use crate::app::state::backup::BackupState;
use crate::app::state::certs::{CertsAction, CertsFocus, CertsState};
use crate::app::state::common::{FocusArea, Notification, RunMode};
use crate::app::state::dashboard::DashboardState;
use crate::app::state::logs::{LogsFocus, LogsState};
use crate::app::state::service::{ServiceButton, ServiceState};
use crate::app::state::site_edit::{char_to_byte, EditFocus, SiteEditState};
use crate::app::state::site_form::{FormField, SiteFormState};
use crate::app::state::sites::SitesState;
use crate::domain::log::{LogKind, LogPaths, LogSource};
use crate::domain::site::Site;
use crate::infra::AppContext;
use crate::ui::modal::Modal;

/// 仪表盘自动刷新间隔。详见 design.md 视图 1 与 architecture.md §11.2。
const DASHBOARD_AUTO_REFRESH: Duration = Duration::from_secs(30);
/// 站点列表自动刷新间隔（保守一些，避免频繁打扰 certbot）。
const SITES_AUTO_REFRESH: Duration = Duration::from_secs(60);
/// 证书页自动刷新间隔。certbot certificates 较慢，给久一些。
const CERTS_AUTO_REFRESH: Duration = Duration::from_secs(120);
const LOGS_HORIZONTAL_SCROLL_STEP: i16 = 8;
const LOGS_PAGE_SCROLL_FACTOR: usize = 1;

impl AppState {
    fn toggle_site_edit_scheme(&mut self) {
        self.site_edit.upstream_scheme = if self.site_edit.upstream_scheme == "http" {
            "https".into()
        } else {
            "http".into()
        };
        self.site_edit.dirty = true;
    }

    fn toggle_site_edit_current_flag(&mut self) {
        match self.site_edit.focused {
            EditFocus::ProxyFeatureStreaming => {
                self.site_edit.feature_streaming = !self.site_edit.feature_streaming;
            }
            EditFocus::ProxyFeatureWebsocket => {
                self.site_edit.feature_websocket = !self.site_edit.feature_websocket;
            }
            EditFocus::ProxyFeatureLargeBody => {
                self.site_edit.feature_large_body = !self.site_edit.feature_large_body;
            }
            EditFocus::ProxyFeatureCors => {
                self.site_edit.feature_cors = !self.site_edit.feature_cors;
            }
            EditFocus::ProxyFeatureLongTimeout => {
                self.site_edit.feature_long_timeout = !self.site_edit.feature_long_timeout;
            }
            EditFocus::StaticFeatureCache => {
                self.site_edit.feature_static_cache = !self.site_edit.feature_static_cache;
            }
            EditFocus::StaticFeatureBlockSensitive => {
                self.site_edit.feature_block_sensitive = !self.site_edit.feature_block_sensitive;
            }
            _ => return,
        }
        self.site_edit.dirty = true;
    }

    fn adjust_site_edit_managed_focus(&mut self, forward: bool) {
        match self.site_edit.focused {
            EditFocus::Scheme => {
                self.toggle_site_edit_scheme();
            }
            EditFocus::StaticMode => {
                self.site_edit.feature_spa_mode = forward;
                self.site_edit.dirty = true;
            }
            EditFocus::ProxyFeatureStreaming
            | EditFocus::ProxyFeatureWebsocket
            | EditFocus::ProxyFeatureLargeBody
            | EditFocus::ProxyFeatureCors
            | EditFocus::ProxyFeatureLongTimeout
            | EditFocus::StaticFeatureCache
            | EditFocus::StaticFeatureBlockSensitive => {
                self.toggle_site_edit_current_flag();
            }
            _ => {}
        }
    }
}

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

    fn logs_visible_lines_estimate(&self) -> usize {
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

    /// 当前焦点是否落在"文本输入字段"上。用于在 handle_key 顶部判断
    /// 是否应当让全局快捷键（q 退出 / 1-6 跳菜单）让位给字面输入。
    /// 详见 doc/design.md §五，全局快捷键不应吞掉用户在输入框里键入的字符。
    pub fn is_in_text_input(&self) -> bool {
        match &self.route {
            Route::Sites(SitesRoute::New) => matches!(
                self.site_form.focused,
                FormField::SiteName
                    | FormField::Domain
                    | FormField::DomainAliases
                    | FormField::Target
            ),
            Route::Sites(SitesRoute::EditManaged { .. }) => matches!(
                self.site_edit.focused,
                EditFocus::Domain | EditFocus::DomainAliases | EditFocus::Target
            ),
            Route::Sites(SitesRoute::EditAdvanced { .. }) => true,
            // 原始模式与槽位全屏编辑：整页都在接收文本
            Route::Sites(SitesRoute::EditRaw { .. })
            | Route::Sites(SitesRoute::EditSlotFull { .. }) => true,
            // 日志页只在搜索框激活时算作文本输入
            Route::Logs => matches!(self.logs.focused, LogsFocus::SearchInput),
            _ => false,
        }
    }

    fn handle_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // 弹窗优先：当前有 modal 时所有按键先交给 modal
        if self.modal.is_some() {
            return self.handle_modal_key(k);
        }

        // Ctrl+C：弹出退出确认弹窗
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c')) {
            self.modal = Some(Modal::confirm_quit());
            return;
        }

        let in_text_input = self.is_in_text_input();

        // q：弹出退出确认弹窗（文本输入中仍作为字面字符传递）
        if !in_text_input
            && k.modifiers == KeyModifiers::NONE
            && matches!(k.code, KeyCode::Char('q'))
        {
            self.modal = Some(Modal::confirm_quit());
            return;
        }

        // 数字键 1-6：直接跳转到对应一级菜单。同样在文本输入中让位给字面输入。
        if !in_text_input && k.modifiers == KeyModifiers::NONE {
            if let KeyCode::Char(c @ '1'..='6') = k.code {
                let idx = (c as u8 - b'1') as usize;
                self.go_to_menu(MenuItem::ALL[idx]);
                return;
            }
        }

        // 焦点在侧边栏时，导航键（Up/Down/Enter/Right/Tab）应优先交给侧边栏处理。
        // 否则下方的路由相关 handler（Sites/List 的 Up/Down 移动站点光标、Service 的
        // Up/Down 切换按钮、Certs/Backup 的 Up/Down 等）会抢先吞掉方向键，导致用户
        // 在侧边栏按 ↑/↓ 卡住、无法继续切换菜单项。
        if matches!(self.focus, FocusArea::Sidebar)
            && !in_text_input
            && k.modifiers == KeyModifiers::NONE
            && matches!(
                k.code,
                KeyCode::Up | KeyCode::Down | KeyCode::Enter | KeyCode::Right | KeyCode::Tab
            )
        {
            return self.handle_sidebar_key(k);
        }

        // 路由相关全局快捷键（即使焦点在侧边栏也生效）
        if matches!(self.route, Route::Dashboard)
            && k.modifiers == KeyModifiers::NONE
            && matches!(k.code, KeyCode::Char('r'))
        {
            self.dashboard.pending_refresh = true;
            return;
        }
        if matches!(self.route, Route::Sites(SitesRoute::List)) && k.modifiers == KeyModifiers::NONE
        {
            match k.code {
                KeyCode::Char('r') => {
                    self.sites.pending_refresh = true;
                    return;
                }
                KeyCode::Up => {
                    self.sites.move_cursor(-1);
                    return;
                }
                KeyCode::Down => {
                    self.sites.move_cursor(1);
                    return;
                }
                KeyCode::Enter => {
                    self.enter_site_edit();
                    return;
                }
                KeyCode::Char('s') => {
                    self.request_site_toggle();
                    return;
                }
                KeyCode::Char('o') => {
                    self.sites.cycle_sort_field();
                    return;
                }
                KeyCode::Char('p') => {
                    self.sites.toggle_sort_order();
                    return;
                }
                KeyCode::Char('n') => {
                    if self.run_mode.is_readonly() {
                        self.notification = Some(Notification::failure(
                            "当前为只读模式，需要 root 权限执行此操作".to_string(),
                        ));
                    } else {
                        self.site_form = SiteFormState::default();
                        self.route = Route::Sites(SitesRoute::New);
                    }
                    return;
                }
                KeyCode::Char('d') => {
                    self.request_site_delete();
                    return;
                }
                KeyCode::Char('c') => {
                    self.request_cert_for_current_site();
                    return;
                }
                KeyCode::Char('l') => {
                    self.goto_site_log();
                    return;
                }
                _ => {}
            }
        }

        if matches!(self.route, Route::Service) && k.modifiers == KeyModifiers::NONE {
            match k.code {
                KeyCode::Tab | KeyCode::Right | KeyCode::Down => {
                    self.service.move_focus(1);
                    return;
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Up => {
                    self.service.move_focus(-1);
                    return;
                }
                KeyCode::Enter => {
                    self.request_service_action();
                    return;
                }
                KeyCode::Char('c') => {
                    self.service.clear_output();
                    return;
                }
                _ => {}
            }
        }

        if matches!(self.route, Route::Sites(SitesRoute::New)) {
            return self.handle_site_form_key(k);
        }

        // 站点编辑模式按键处理
        if matches!(self.route, Route::Sites(SitesRoute::EditManaged { .. })) {
            return self.handle_site_edit_managed_key(k);
        }

        if matches!(self.route, Route::Sites(SitesRoute::EditAdvanced { .. })) {
            return self.handle_site_edit_advanced_key(k);
        }

        // 原始配置编辑模式按键处理
        if matches!(self.route, Route::Sites(SitesRoute::EditRaw { .. })) {
            return self.handle_raw_edit_key(k);
        }

        // 注入槽全屏编辑模式按键处理（Ctrl+E 进入）
        if matches!(self.route, Route::Sites(SitesRoute::EditSlotFull { .. })) {
            return self.handle_slot_full_edit_key(k);
        }

        // 日志视图按键处理
        if matches!(self.route, Route::Logs) {
            return self.handle_logs_key(k);
        }

        // 证书视图按键处理
        if matches!(self.route, Route::Certs) {
            return self.handle_certs_key(k);
        }

        // 备份视图按键处理
        if matches!(self.route, Route::Backup) {
            return self.handle_backup_key(k);
        }

        match self.focus {
            FocusArea::Sidebar => self.handle_sidebar_key(k),
            FocusArea::Content => self.handle_content_key(k),
        }
    }

    fn handle_sidebar_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match k.code {
            KeyCode::Up => self.move_menu(-1),
            KeyCode::Down => self.move_menu(1),
            KeyCode::Enter | KeyCode::Right | KeyCode::Tab => {
                self.focus = FocusArea::Content;
            }
            _ => {}
        }
    }

    fn handle_content_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        let in_overlay = !matches!(
            self.route,
            Route::Dashboard
                | Route::Certs
                | Route::Logs
                | Route::Service
                | Route::Backup
                | Route::Sites(SitesRoute::List)
        );

        match k.code {
            KeyCode::Esc => {
                if in_overlay {
                    if matches!(self.route, Route::Sites(_)) {
                        self.route = Route::Sites(SitesRoute::List);
                    }
                } else {
                    self.focus = FocusArea::Sidebar;
                }
            }
            KeyCode::Tab => self.focus = FocusArea::Sidebar,
            KeyCode::Left if !in_overlay => self.focus = FocusArea::Sidebar,
            _ => {}
        }
    }

    fn handle_modal_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        let Some(modal) = self.modal.as_mut() else {
            return;
        };
        match k.code {
            KeyCode::Esc => self.modal = None,
            KeyCode::Tab | KeyCode::Left | KeyCode::Right => modal.toggle_focus(),
            KeyCode::Enter => {
                let action = modal.confirm_action();
                self.modal = None;
                self.execute_modal_action(action);
            }
            _ => {}
        }
    }

    fn execute_modal_action(&mut self, action: crate::ui::modal::ModalAction) {
        use crate::ui::modal::ModalAction;
        match action {
            ModalAction::None => {}
            ModalAction::Quit => self.should_quit = true,
            ModalAction::RestartNginx => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，无法重启服务".to_string(),
                    ));
                    return;
                }
                self.service.pending_action = Some(ServiceButton::Restart);
                self.service.running = Some(ServiceButton::Restart);
            }
            ModalAction::UpgradeTui => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，无法更新 TUI".to_string(),
                    ));
                    return;
                }
                // 使用一个特殊的 pending_action 值来标记升级
                self.service.pending_upgrade = true;
                self.service.running = Some(ServiceButton::CheckUpdate);
            }
            ModalAction::InstallDeployHook => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                self.certs.pending_install_hook = true;
                self.certs.running = Some(crate::app::state::CertsAction::InstallDeployHook);
            }
            ModalAction::DeleteOrphanCerts { cert_names } => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                if !cert_names.is_empty() {
                    self.certs.pending_delete = cert_names.into_iter().collect();
                    self.certs.running = Some(crate::app::state::CertsAction::DeleteOrphan);
                }
            }
            ModalAction::DiscardSiteForm => {
                self.site_form = SiteFormState::default();
                self.route = Route::Sites(SitesRoute::List);
            }
            ModalAction::SaveAndExitSiteEdit => {
                self.site_edit.exit_after_save = true;
                self.save_site_edit(false);
            }
            ModalAction::DiscardSiteEdit => {
                self.site_edit = SiteEditState::default();
                self.route = Route::Sites(SitesRoute::List);
                self.notification = Some(Notification::info("已放弃修改".to_string()));
            }
            ModalAction::RequestCertForSite { site_name, domains } => {
                self.certs.pending_request = Some((site_name, domains));
            }
            ModalAction::RenewAllCerts => {
                self.certs.pending_renew = true;
            }
            ModalAction::CreateBackup => {
                self.backup.pending_create = true;
            }
            ModalAction::DeleteBackup(p) => {
                self.backup.pending_delete = Some(p);
            }
            ModalAction::RestoreBackup(p) => {
                self.backup.pending_restore = Some(p);
            }
            ModalAction::DeleteSite { site_name } => {
                self.sites.action_in_flight = Some(site_name.clone());
                self.sites.pending_delete = Some(site_name);
            }
        }
    }

    fn go_to_menu(&mut self, item: MenuItem) {
        let prev_route = self.route.clone();
        self.route = item.default_route();
        self.focus = FocusArea::Sidebar;
        // 切回仪表盘时若数据陈旧主动触发一次刷新
        if !matches!(prev_route, Route::Dashboard) && matches!(self.route, Route::Dashboard) {
            let stale = self
                .dashboard
                .last_refresh
                .map(|t| t.elapsed() >= DASHBOARD_AUTO_REFRESH)
                .unwrap_or(true);
            if stale {
                self.dashboard.pending_refresh = true;
            }
        }
        // 切到站点列表时若从未加载也触发刷新
        if !matches!(prev_route, Route::Sites(_)) && matches!(self.route, Route::Sites(_)) {
            let stale = self
                .sites
                .last_refresh
                .map(|t| t.elapsed() >= SITES_AUTO_REFRESH)
                .unwrap_or(true);
            if stale {
                self.sites.pending_refresh = true;
            }
        }
        // 切到日志视图时启动 tail（或切换日志源）
        if !matches!(prev_route, Route::Logs) && matches!(self.route, Route::Logs) {
            let kind = self.logs.source.kind();
            let site_name = self.current_logs_site_name();
            self.rebuild_logs_source(site_name, kind);
            self.logs.pending_tail_change = true;
        }
        // 离开日志视图时停止 tail
        if matches!(prev_route, Route::Logs) && !matches!(self.route, Route::Logs) {
            self.logs.stop_tail();
        }
        // 切到证书页时若数据陈旧主动触发刷新
        if !matches!(prev_route, Route::Certs) && matches!(self.route, Route::Certs) {
            let stale = self
                .certs
                .last_refresh
                .map(|t| t.elapsed() >= CERTS_AUTO_REFRESH)
                .unwrap_or(true);
            if stale {
                self.certs.pending_refresh = true;
            }
        }
        // 切到备份页时主动触发首次扫描
        if !matches!(prev_route, Route::Backup) && matches!(self.route, Route::Backup) {
            self.backup.pending_refresh = true;
        }
    }

    fn request_site_delete(&mut self) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        if self.sites.action_in_flight.is_some() {
            return;
        }
        let Some(site) = self.sites.current() else {
            return;
        };
        let name = site.name.clone();
        self.modal = Some(Modal::confirm(
            "⚠️  确认删除站点",
            vec![
                format!("即将删除站点 {} 的配置文件。", name),
                "该操作不可撤销。".into(),
                "确认删除？".into(),
            ],
            "确认删除",
            crate::ui::modal::ModalAction::DeleteSite { site_name: name },
        ));
    }

    fn detect_global_log_paths(&self) -> LogPaths {
        let nginx_root = self
            .ctx
            .probe
            .sites_available
            .parent()
            .unwrap_or(self.ctx.probe.sites_available.as_path());
        crate::domain::log::detect_global_log_paths(&nginx_root.join("nginx.conf"))
    }

    fn build_global_log_source(&self, kind: LogKind) -> LogSource {
        LogSource::global(kind, self.detect_global_log_paths())
    }

    fn build_site_log_source(&self, site: &Site, kind: LogKind) -> LogSource {
        let global = self.detect_global_log_paths();
        LogSource::site(
            site.name.clone(),
            kind,
            LogPaths {
                access: site.access_log_path.clone().or(global.access),
                error: site.error_log_path.clone().or(global.error),
            },
        )
    }

    fn current_logs_site_name(&self) -> Option<String> {
        match &self.logs.source {
            LogSource::Global { .. } => None,
            LogSource::Site { name, .. } => Some(name.clone()),
        }
    }

    fn rebuild_logs_source(&mut self, site_name: Option<String>, kind: LogKind) {
        self.logs.source = match site_name {
            Some(name) => self
                .sites
                .list
                .iter()
                .find(|site| site.name == name)
                .map(|site| self.build_site_log_source(site, kind))
                .unwrap_or_else(|| self.build_global_log_source(kind)),
            None => self.build_global_log_source(kind),
        };
    }

    /// 按 c 键：为当前选中站点申请证书，跳转到证书页
    fn request_cert_for_current_site(&mut self) {
        let Some(site) = self.sites.current() else {
            self.notification = Some(Notification::info("请先选中一个站点".to_string()));
            return;
        };
        if site.all_domains.is_empty() {
            self.notification = Some(Notification::failure(
                "该站点没有配置域名，无法申请证书".to_string(),
            ));
            return;
        }
        let site_name = site.name.clone();
        let domains = site.all_domains.clone();
        self.certs.pending_request = Some((site_name.clone(), domains.clone()));
        self.route = Route::Certs;
        self.focus = FocusArea::Content;
        self.certs.pending_refresh = true;
        self.certs.running = Some(crate::app::state::CertsAction::Request);
    }

    /// 按 l 键：跳转到日志页，自动选中当前站点的日志
    fn goto_site_log(&mut self) {
        let Some(site) = self.sites.current() else {
            self.notification = Some(Notification::info("请先选中一个站点".to_string()));
            return;
        };
        self.logs.source = self.build_site_log_source(site, LogKind::Access);
        self.logs.clear_buffer();
        self.logs.pending_tail_change = true;
        self.route = Route::Logs;
        self.focus = FocusArea::Content;
        self.logs.focused = crate::app::state::LogsFocus::LogContent;
    }

    fn request_site_toggle(&mut self) {
        // 只读：直接给出提示，不派发
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        if self.sites.action_in_flight.is_some() {
            return; // 已有操作在飞中
        }
        let Some(site) = self.sites.current() else {
            return;
        };
        let name = site.name.clone();
        let target_enabled = !site.enabled;
        self.sites.action_in_flight = Some(name.clone());
        self.sites.pending_toggle = Some((name, target_enabled));
    }

    fn request_service_action(&mut self) {
        if self.service.running.is_some() {
            return;
        }
        let btn = self.service.focused;
        let needs_root = matches!(btn, ServiceButton::Reload | ServiceButton::Restart);
        if needs_root && self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        match btn {
            ServiceButton::Restart => {
                self.modal = Some(Modal::confirm_restart_nginx());
            }
            ServiceButton::Test
            | ServiceButton::Reload
            | ServiceButton::Status
            | ServiceButton::CheckUpdate => {
                self.service.running = Some(btn);
                self.service.pending_action = Some(btn);
            }
        }
    }

    pub fn take_service_action(&mut self) -> Option<ServiceButton> {
        self.service.pending_action.take()
    }

    pub fn take_service_upgrade(&mut self) -> bool {
        if self.service.pending_upgrade {
            self.service.pending_upgrade = false;
            true
        } else {
            false
        }
    }

    fn move_menu(&mut self, delta: i32) {
        let cur = self.current_menu();
        let len = MenuItem::ALL.len() as i32;
        let cur_idx = MenuItem::ALL
            .iter()
            .position(|m| *m == cur)
            .map(|x| x as i32)
            .unwrap_or(0);
        let next_idx = (cur_idx + delta).rem_euclid(len) as usize;
        self.go_to_menu(MenuItem::ALL[next_idx]);
    }

    pub fn expire_notification_if_due(&mut self) {
        if let Some(n) = &self.notification {
            if Instant::now() >= n.expires_at {
                self.notification = None;
            }
        }
    }

    /// 处理新建站点表单的按键
    fn handle_site_form_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.site_form.submitting {
            return; // 提交中不接受输入
        }

        // Esc: 返回列表（有内容时弹出确认框，详见 design.md 子模式 B）
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            if self.site_form.site_name.is_empty()
                && self.site_form.domain.is_empty()
                && self.site_form.target.is_empty()
            {
                self.route = Route::Sites(SitesRoute::List);
            } else {
                self.modal = Some(Modal::confirm_discard_site_form());
            }
            return;
        }

        // Tab/Shift+Tab: 字段切换
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Tab) {
            self.site_form.move_focus(1);
            return;
        }
        if k.modifiers.contains(KeyModifiers::SHIFT) && matches!(k.code, KeyCode::Tab) {
            self.site_form.move_focus(-1);
            return;
        }

        // 上下键：字段间导航（特殊选择器留给后续逻辑）
        if k.modifiers == KeyModifiers::NONE
            && !matches!(
                self.site_form.focused,
                FormField::SiteType | FormField::StaticMode
            )
        {
            match k.code {
                KeyCode::Up => {
                    self.site_form.move_focus(-1);
                    return;
                }
                KeyCode::Down => {
                    self.site_form.move_focus(1);
                    return;
                }
                _ => {}
            }
        }

        // 类型选择器切换
        if k.modifiers == KeyModifiers::NONE && self.site_form.focused == FormField::SiteType {
            match k.code {
                KeyCode::Up => self.site_form.toggle_site_type(-1),
                KeyCode::Down => self.site_form.toggle_site_type(1),
                KeyCode::Left => self.site_form.toggle_site_type(-1),
                KeyCode::Right => self.site_form.toggle_site_type(1),
                KeyCode::Enter => self.site_form.move_focus(1),
                _ => {}
            }
            return;
        }

        // 静态模式选择器
        if k.modifiers == KeyModifiers::NONE && self.site_form.focused == FormField::StaticMode {
            match k.code {
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                    self.site_form.static_spa_mode = !self.site_form.static_spa_mode;
                }
                KeyCode::Enter => self.site_form.move_focus(1),
                KeyCode::Up => self.site_form.move_focus(-1),
                KeyCode::Down => self.site_form.move_focus(1),
                _ => {}
            }
            return;
        }

        // Enter: 提交（在提交按钮上） / 下一个字段（在其他字段上）
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Enter) {
            match self.site_form.focused {
                FormField::SubmitButton => self.submit_site_form(),
                FormField::SiteType => self.site_form.move_focus(1),
                FormField::ProxyFeatureStreaming => {
                    self.site_form.feature_streaming = !self.site_form.feature_streaming;
                }
                FormField::ProxyFeatureWebsocket => {
                    self.site_form.feature_websocket = !self.site_form.feature_websocket;
                }
                FormField::ProxyFeatureLargeBody => {
                    self.site_form.feature_large_body = !self.site_form.feature_large_body;
                }
                FormField::ProxyFeatureCors => {
                    self.site_form.feature_cors = !self.site_form.feature_cors;
                }
                FormField::ProxyFeatureLongTimeout => {
                    self.site_form.feature_long_timeout = !self.site_form.feature_long_timeout;
                }
                FormField::StaticMode => {
                    self.site_form.static_spa_mode = !self.site_form.static_spa_mode;
                }
                FormField::StaticFeatureCache => {
                    self.site_form.static_cache = !self.site_form.static_cache;
                }
                FormField::StaticFeatureBlockSensitive => {
                    self.site_form.static_block_sensitive = !self.site_form.static_block_sensitive;
                }
                FormField::EnableCheckbox => {
                    self.site_form.enable_now = !self.site_form.enable_now;
                }
                FormField::CertCheckbox => {
                    // 证书申请依赖立即启用
                    if self.site_form.enable_now {
                        self.site_form.request_cert = !self.site_form.request_cert;
                    }
                }
                _ => self.site_form.move_focus(1),
            }
            return;
        }

        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(2)) {
            self.submit_site_form();
            return;
        }

        // Space: 复选框切换
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char(' ')) {
            match (self.site_form.focused, self.site_form.enable_now) {
                (FormField::ProxyFeatureStreaming, _) => {
                    self.site_form.feature_streaming = !self.site_form.feature_streaming;
                }
                (FormField::ProxyFeatureWebsocket, _) => {
                    self.site_form.feature_websocket = !self.site_form.feature_websocket;
                }
                (FormField::ProxyFeatureLargeBody, _) => {
                    self.site_form.feature_large_body = !self.site_form.feature_large_body;
                }
                (FormField::ProxyFeatureCors, _) => {
                    self.site_form.feature_cors = !self.site_form.feature_cors;
                }
                (FormField::ProxyFeatureLongTimeout, _) => {
                    self.site_form.feature_long_timeout = !self.site_form.feature_long_timeout;
                }
                (FormField::StaticMode, _) => {
                    self.site_form.static_spa_mode = !self.site_form.static_spa_mode;
                }
                (FormField::StaticFeatureCache, _) => {
                    self.site_form.static_cache = !self.site_form.static_cache;
                }
                (FormField::StaticFeatureBlockSensitive, _) => {
                    self.site_form.static_block_sensitive = !self.site_form.static_block_sensitive;
                }
                (FormField::EnableCheckbox, _) => {
                    self.site_form.enable_now = !self.site_form.enable_now;
                }
                (FormField::CertCheckbox, true) => {
                    self.site_form.request_cert = !self.site_form.request_cert;
                }
                _ => {}
            }
            return;
        }

        // 文本输入处理
        if k.modifiers == KeyModifiers::NONE || k.modifiers.contains(KeyModifiers::SHIFT) {
            match self.site_form.focused {
                FormField::SiteName
                | FormField::Domain
                | FormField::DomainAliases
                | FormField::Target => {
                    match k.code {
                        KeyCode::Char(c) => {
                            let field = match self.site_form.focused {
                                FormField::SiteName => &mut self.site_form.site_name,
                                FormField::Domain => &mut self.site_form.domain,
                                FormField::DomainAliases => &mut self.site_form.domain_aliases,
                                FormField::Target => &mut self.site_form.target,
                                _ => return,
                            };
                            field.push(c);
                            // 清除该字段错误
                            let key = match self.site_form.focused {
                                FormField::SiteName => "site_name",
                                FormField::Domain => "domain",
                                FormField::DomainAliases => "domain_aliases",
                                FormField::Target => "target",
                                _ => "",
                            };
                            self.site_form.field_errors.remove(key);
                        }
                        KeyCode::Backspace => {
                            let field = match self.site_form.focused {
                                FormField::SiteName => &mut self.site_form.site_name,
                                FormField::Domain => &mut self.site_form.domain,
                                FormField::DomainAliases => &mut self.site_form.domain_aliases,
                                FormField::Target => &mut self.site_form.target,
                                _ => return,
                            };
                            field.pop();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// 提交新建站点表单
    fn submit_site_form(&mut self) {
        // 只读模式禁用
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }

        // 验证字段
        if !self.site_form.validate() {
            // 有错误时聚焦第一个错误字段
            if let Some(first_err_field) = self.site_form.field_errors.keys().next() {
                self.site_form.focused = match first_err_field.as_str() {
                    "site_name" => FormField::SiteName,
                    "domain" => FormField::Domain,
                    "domain_aliases" => FormField::DomainAliases,
                    "target" => FormField::Target,
                    "cert_checkbox" => FormField::CertCheckbox,
                    _ => FormField::SiteName,
                };
            }
            return;
        }

        // 构建输入
        let input = self.site_form.build_input();
        if input.is_none() {
            self.notification = Some(Notification::failure("无法构建创建参数".to_string()));
            return;
        }

        self.site_form.submitting = true;
        self.site_form.pending_create = input;
    }

    /// 主循环消费：取出待创建站点请求
    pub fn take_site_create_request(&mut self) -> Option<crate::domain::site::CreateSiteInput> {
        self.site_form.pending_create.take()
    }

    /// 主循环消费：取出待保存站点请求
    pub fn take_site_save_request(&mut self) -> Option<crate::domain::site::SaveSiteInput> {
        self.site_edit.pending_save.take()
    }

    /// 进入站点编辑模式
    fn enter_site_edit(&mut self) {
        let Some(site) = self.sites.current() else {
            return;
        };
        let name = site.name.clone();
        let config_path = site.config_path.clone();

        // 记录目标文件的 mtime，用于保存时的并发保护（架构 §15.0）
        let mtime_at_load = std::fs::metadata(&config_path)
            .ok()
            .and_then(|m| m.modified().ok());

        // 读取配置文件
        let content = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(e) => {
                self.notification = Some(Notification::failure(format!("无法读取配置文件：{}", e)));
                return;
            }
        };

        // 解析配置
        let parsed = crate::template::config_parser::parse_for_edit(&content);
        self.site_edit = SiteEditState::from_parsed(&name, &parsed);
        self.site_edit.mtime_at_load = mtime_at_load;
        self.site_edit.seal_original();
        self.route = Route::Sites(SitesRoute::EditManaged {
            site_name: name.clone(),
        });

        // 静态健康检查：仅提示，不阻断进入编辑
        if let Ok(issues) = crate::domain::config_health::scan_config_file(&config_path) {
            if !issues.is_empty() {
                let summary = issues
                    .iter()
                    .map(|i| i.description())
                    .collect::<Vec<_>>()
                    .join("；");
                self.notification = Some(Notification::info(format!(
                    "站点 {} 配置提示：{}",
                    name, summary
                )));
            }
        }
    }

    /// 处理站点编辑（托管模式）按键
    fn handle_site_edit_managed_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.site_edit.saving {
            return;
        }

        // Esc: 返回列表（dirty 时弹确认，详见 design.md 子模式 C）
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            if self.site_edit.dirty {
                self.modal = Some(Modal::confirm_discard_site_edit());
            } else {
                self.route = Route::Sites(SitesRoute::List);
            }
            return;
        }

        // Tab/Shift+Tab: 切换焦点
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Tab) {
            self.site_edit.move_focus_forward();
            return;
        }
        if k.modifiers.contains(KeyModifiers::SHIFT) && matches!(k.code, KeyCode::Tab) {
            self.site_edit.move_focus_backward();
            return;
        }

        if k.modifiers == KeyModifiers::NONE {
            match k.code {
                KeyCode::F(2) => {
                    self.save_site_edit(false);
                    return;
                }
                KeyCode::F(3) => {
                    self.save_site_edit(true);
                    return;
                }
                KeyCode::F(4) => {
                    if self.site_edit.restore_original() {
                        self.notification =
                            Some(Notification::success("已重置为加载时的值".to_string()));
                    } else {
                        self.notification =
                            Some(Notification::failure("无可恢复的原始值".to_string()));
                    }
                    return;
                }
                KeyCode::F(5) => {
                    let name = self.site_edit.site_name.clone();
                    self.route = Route::Sites(SitesRoute::EditAdvanced { site_name: name });
                    return;
                }
                KeyCode::F(6) => {
                    let name = self.site_edit.site_name.clone();
                    self.route = Route::Sites(SitesRoute::EditRaw { site_name: name });
                    return;
                }
                KeyCode::Up => {
                    self.site_edit.move_focus_backward();
                    return;
                }
                KeyCode::Down => {
                    self.site_edit.move_focus_forward();
                    return;
                }
                KeyCode::Left => {
                    self.adjust_site_edit_managed_focus(false);
                    return;
                }
                KeyCode::Right => {
                    self.adjust_site_edit_managed_focus(true);
                    return;
                }
                _ => {}
            }
        }

        // 文本输入
        if k.modifiers == KeyModifiers::NONE || k.modifiers.contains(KeyModifiers::SHIFT) {
            match self.site_edit.focused {
                EditFocus::Domain | EditFocus::DomainAliases | EditFocus::Target => match k.code {
                    KeyCode::Char(c) => {
                        let field = match self.site_edit.focused {
                            EditFocus::Domain => &mut self.site_edit.domain,
                            EditFocus::DomainAliases => &mut self.site_edit.domain_aliases,
                            EditFocus::Target => &mut self.site_edit.target,
                            _ => return,
                        };
                        field.push(c);
                        self.site_edit.dirty = true;
                        self.site_edit
                            .field_errors
                            .remove(match self.site_edit.focused {
                                EditFocus::Domain => "domain",
                                EditFocus::DomainAliases => "domain_aliases",
                                EditFocus::Target => "target",
                                _ => "",
                            });
                    }
                    KeyCode::Backspace => {
                        let field = match self.site_edit.focused {
                            EditFocus::Domain => &mut self.site_edit.domain,
                            EditFocus::DomainAliases => &mut self.site_edit.domain_aliases,
                            EditFocus::Target => &mut self.site_edit.target,
                            _ => return,
                        };
                        field.pop();
                        self.site_edit.dirty = true;
                    }
                    _ => {}
                },
                EditFocus::Scheme => {
                    if matches!(k.code, KeyCode::Enter | KeyCode::Char(' ')) {
                        self.toggle_site_edit_scheme();
                    }
                    if matches!(k.code, KeyCode::Char('h')) {
                        self.site_edit.upstream_scheme = "http".into();
                        self.site_edit.dirty = true;
                    }
                    if matches!(k.code, KeyCode::Char('s')) {
                        self.site_edit.upstream_scheme = "https".into();
                        self.site_edit.dirty = true;
                    }
                }
                EditFocus::ProxyFeatureStreaming
                | EditFocus::ProxyFeatureWebsocket
                | EditFocus::ProxyFeatureLargeBody
                | EditFocus::ProxyFeatureCors
                | EditFocus::ProxyFeatureLongTimeout
                | EditFocus::StaticFeatureCache
                | EditFocus::StaticFeatureBlockSensitive => {
                    if matches!(k.code, KeyCode::Enter | KeyCode::Char(' ')) {
                        self.toggle_site_edit_current_flag();
                    }
                }
                EditFocus::StaticMode => {
                    if matches!(k.code, KeyCode::Enter | KeyCode::Char(' ')) {
                        self.site_edit.feature_spa_mode = !self.site_edit.feature_spa_mode;
                        self.site_edit.dirty = true;
                    }
                }
            }
        }
    }

    /// 处理站点编辑（高级模式）按键
    fn handle_site_edit_advanced_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.site_edit.saving {
            return;
        }

        if k.modifiers == KeyModifiers::NONE {
            match k.code {
                KeyCode::F(5) => {
                    let name = self.site_edit.site_name.clone();
                    self.route = Route::Sites(SitesRoute::EditManaged { site_name: name });
                    return;
                }
                KeyCode::F(6) => {
                    let name = self.site_edit.site_name.clone();
                    self.route = Route::Sites(SitesRoute::EditRaw { site_name: name });
                    return;
                }
                KeyCode::F(7) => {
                    let snippets = crate::template::snippets::get_snippets_for_slot(
                        self.site_edit.current_slot,
                    );
                    if let Some(snippet) = snippets.get(self.site_edit.template_index) {
                        self.site_edit.replace_with_snippet(snippet.content);
                        self.notification = Some(Notification::success("已替换槽位".to_string()));
                    }
                    return;
                }
                KeyCode::F(8) => {
                    self.site_edit.enter_slot_full();
                    let slot = self.site_edit.current_slot;
                    let name = self.site_edit.site_name.clone();
                    self.route = Route::Sites(SitesRoute::EditSlotFull {
                        site_name: name,
                        slot,
                    });
                    return;
                }
                _ => {}
            }
        }

        if k.modifiers == KeyModifiers::NONE && self.site_edit.focused == EditFocus::Domain {
            match k.code {
                KeyCode::Left => {
                    self.site_edit.cycle_slot(-1);
                    return;
                }
                KeyCode::Right => {
                    self.site_edit.cycle_slot(1);
                    return;
                }
                KeyCode::Up => {
                    let snippets = crate::template::snippets::get_snippets_for_slot(
                        self.site_edit.current_slot,
                    );
                    if !snippets.is_empty() {
                        self.site_edit.template_index =
                            self.site_edit.template_index.saturating_sub(1);
                    }
                    return;
                }
                KeyCode::Down => {
                    let snippets = crate::template::snippets::get_snippets_for_slot(
                        self.site_edit.current_slot,
                    );
                    if self.site_edit.template_index + 1 < snippets.len() {
                        self.site_edit.template_index += 1;
                    }
                    return;
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let snippets = crate::template::snippets::get_snippets_for_slot(
                        self.site_edit.current_slot,
                    );
                    if let Some(snippet) = snippets.get(self.site_edit.template_index) {
                        self.site_edit.append_snippet(snippet.content);
                        self.notification = Some(Notification::success("已追加模板".to_string()));
                    }
                    return;
                }
                _ => {}
            }
        }

        self.handle_site_edit_managed_key(k);
    }

    /// 处理原始配置编辑按键
    fn handle_raw_edit_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.site_edit.saving {
            return;
        }

        // Esc: 返回列表（dirty 时弹确认，详见 design.md 子模式 D）
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            if self.site_edit.dirty {
                self.modal = Some(Modal::confirm_discard_site_edit());
            } else {
                self.route = Route::Sites(SitesRoute::List);
            }
            return;
        }

        // F5: 切换到表单模式
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(5)) {
            // 检查标记是否完整
            let content = self.site_edit.raw_lines.join("\n");
            let (_, markers_intact) =
                crate::template::config_parser::extract_injection_slots(&content);
            if !markers_intact {
                self.notification = Some(Notification::failure(
                    "注入槽标记已修改，无法切回表单模式".to_string(),
                ));
                return;
            }
            // 重新解析；保留 mtime_at_load（模式切换不重读文件）和 dirty 状态
            let parsed = crate::template::config_parser::parse_for_edit(&content);
            let name = self.site_edit.site_name.clone();
            let mtime = self.site_edit.mtime_at_load;
            let dirty = self.site_edit.dirty;
            self.site_edit = SiteEditState::from_parsed(&name, &parsed);
            self.site_edit.mtime_at_load = mtime;
            self.site_edit.dirty = dirty;
            self.route = Route::Sites(SitesRoute::EditManaged { site_name: name });
            return;
        }

        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(2)) {
            self.save_raw_edit(false);
            return;
        }

        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(3)) {
            self.save_raw_edit(true);
            return;
        }

        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(9)) {
            if !self.site_edit.raw_undo() {
                self.notification = Some(Notification::info("无可撤销的操作".to_string()));
            }
            return;
        }

        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(10)) {
            if !self.site_edit.raw_redo() {
                self.notification = Some(Notification::info("无可重做的操作".to_string()));
            }
            return;
        }

        // 光标移动
        if k.modifiers == KeyModifiers::NONE {
            match k.code {
                KeyCode::Up => {
                    if self.site_edit.raw_cursor_line > 0 {
                        self.site_edit.raw_cursor_line -= 1;
                        self.site_edit.raw_cursor_col = self.site_edit.raw_cursor_col.min(
                            self.site_edit
                                .raw_lines
                                .get(self.site_edit.raw_cursor_line)
                                .map_or(0, |l| l.len()),
                        );
                    }
                    return;
                }
                KeyCode::Down => {
                    if self.site_edit.raw_cursor_line + 1 < self.site_edit.raw_lines.len() {
                        self.site_edit.raw_cursor_line += 1;
                        self.site_edit.raw_cursor_col = self.site_edit.raw_cursor_col.min(
                            self.site_edit
                                .raw_lines
                                .get(self.site_edit.raw_cursor_line)
                                .map_or(0, |l| l.len()),
                        );
                    }
                    return;
                }
                KeyCode::Left => {
                    if self.site_edit.raw_cursor_col > 0 {
                        self.site_edit.raw_cursor_col -= 1;
                    } else if self.site_edit.raw_cursor_line > 0 {
                        self.site_edit.raw_cursor_line -= 1;
                        self.site_edit.raw_cursor_col = self
                            .site_edit
                            .raw_lines
                            .get(self.site_edit.raw_cursor_line)
                            .map_or(0, |l| l.len());
                    }
                    return;
                }
                KeyCode::Right => {
                    let current_len = self
                        .site_edit
                        .raw_lines
                        .get(self.site_edit.raw_cursor_line)
                        .map_or(0, |l| l.len());
                    if self.site_edit.raw_cursor_col < current_len {
                        self.site_edit.raw_cursor_col += 1;
                    } else if self.site_edit.raw_cursor_line + 1 < self.site_edit.raw_lines.len() {
                        self.site_edit.raw_cursor_line += 1;
                        self.site_edit.raw_cursor_col = 0;
                    }
                    return;
                }
                _ => {}
            }
        }

        // 文本输入
        if k.modifiers == KeyModifiers::NONE || k.modifiers.contains(KeyModifiers::SHIFT) {
            match k.code {
                KeyCode::Char(c) => {
                    self.site_edit.push_raw_undo();
                    if let Some(line) = self
                        .site_edit
                        .raw_lines
                        .get_mut(self.site_edit.raw_cursor_line)
                    {
                        line.insert(self.site_edit.raw_cursor_col, c);
                        self.site_edit.raw_cursor_col += 1;
                        self.site_edit.dirty = true;
                    }
                }
                KeyCode::Backspace => {
                    if self.site_edit.raw_cursor_col > 0 {
                        self.site_edit.push_raw_undo();
                        if let Some(line) = self
                            .site_edit
                            .raw_lines
                            .get_mut(self.site_edit.raw_cursor_line)
                        {
                            line.remove(self.site_edit.raw_cursor_col - 1);
                            self.site_edit.raw_cursor_col -= 1;
                            self.site_edit.dirty = true;
                        }
                    } else if self.site_edit.raw_cursor_line > 0 {
                        self.site_edit.push_raw_undo();
                        // 合并到上一行
                        let current = self
                            .site_edit
                            .raw_lines
                            .remove(self.site_edit.raw_cursor_line);
                        self.site_edit.raw_cursor_line -= 1;
                        let prev_len = self
                            .site_edit
                            .raw_lines
                            .get(self.site_edit.raw_cursor_line)
                            .map_or(0, |l| l.len());
                        if let Some(prev) = self
                            .site_edit
                            .raw_lines
                            .get_mut(self.site_edit.raw_cursor_line)
                        {
                            prev.push_str(&current);
                        }
                        self.site_edit.raw_cursor_col = prev_len;
                        self.site_edit.dirty = true;
                    }
                }
                KeyCode::Enter => {
                    self.site_edit.push_raw_undo();
                    let rest = if let Some(line) = self
                        .site_edit
                        .raw_lines
                        .get_mut(self.site_edit.raw_cursor_line)
                    {
                        let rest: String = line[self.site_edit.raw_cursor_col..].to_string();
                        line.truncate(self.site_edit.raw_cursor_col);
                        rest
                    } else {
                        String::new()
                    };
                    self.site_edit
                        .raw_lines
                        .insert(self.site_edit.raw_cursor_line + 1, rest);
                    self.site_edit.raw_cursor_line += 1;
                    self.site_edit.raw_cursor_col = 0;
                    self.site_edit.dirty = true;
                }
                _ => {}
            }
        }
    }

    /// 处理注入槽全屏编辑按键（design.md 子模式 C，由 Ctrl+E 进入）
    fn handle_slot_full_edit_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // F2: 完成编辑，写回 injection_slots，回到表单模式
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(2)) {
            if self.site_edit.commit_slot_full().is_some() {
                self.notification = Some(Notification::success("已完成槽位编辑".to_string()));
            }
            let name = self.site_edit.site_name.clone();
            self.route = Route::Sites(SitesRoute::EditAdvanced { site_name: name });
            return;
        }

        // Esc: 取消，丢弃槽位编辑缓冲
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            let name = self.site_edit.site_name.clone();
            self.site_edit.slot_edit_target = None;
            self.site_edit.slot_edit_lines.clear();
            self.site_edit.slot_edit_undo.clear();
            self.site_edit.slot_edit_redo.clear();
            self.notification = Some(Notification::info("已取消槽位编辑".to_string()));
            self.route = Route::Sites(SitesRoute::EditAdvanced { site_name: name });
            return;
        }

        // F4: 清空整个槽位
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(4)) {
            self.site_edit.push_slot_undo();
            self.site_edit.slot_edit_lines = vec![String::new()];
            self.site_edit.slot_edit_cursor_line = 0;
            self.site_edit.slot_edit_cursor_col = 0;
            return;
        }

        // F9: 撤销
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(9)) {
            if !self.site_edit.slot_undo() {
                self.notification = Some(Notification::info("无可撤销的操作".to_string()));
            }
            return;
        }

        // F10: 重做
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::F(10)) {
            if !self.site_edit.slot_redo() {
                self.notification = Some(Notification::info("无可重做的操作".to_string()));
            }
            return;
        }

        // 光标移动
        if k.modifiers == KeyModifiers::NONE {
            match k.code {
                KeyCode::Up => {
                    if self.site_edit.slot_edit_cursor_line > 0 {
                        self.site_edit.slot_edit_cursor_line -= 1;
                        let len = self
                            .site_edit
                            .slot_edit_lines
                            .get(self.site_edit.slot_edit_cursor_line)
                            .map_or(0, |l| l.chars().count());
                        self.site_edit.slot_edit_cursor_col =
                            self.site_edit.slot_edit_cursor_col.min(len);
                    }
                    return;
                }
                KeyCode::Down => {
                    if self.site_edit.slot_edit_cursor_line + 1
                        < self.site_edit.slot_edit_lines.len()
                    {
                        self.site_edit.slot_edit_cursor_line += 1;
                        let len = self
                            .site_edit
                            .slot_edit_lines
                            .get(self.site_edit.slot_edit_cursor_line)
                            .map_or(0, |l| l.chars().count());
                        self.site_edit.slot_edit_cursor_col =
                            self.site_edit.slot_edit_cursor_col.min(len);
                    }
                    return;
                }
                KeyCode::Left => {
                    if self.site_edit.slot_edit_cursor_col > 0 {
                        self.site_edit.slot_edit_cursor_col -= 1;
                    } else if self.site_edit.slot_edit_cursor_line > 0 {
                        self.site_edit.slot_edit_cursor_line -= 1;
                        self.site_edit.slot_edit_cursor_col = self
                            .site_edit
                            .slot_edit_lines
                            .get(self.site_edit.slot_edit_cursor_line)
                            .map_or(0, |l| l.chars().count());
                    }
                    return;
                }
                KeyCode::Right => {
                    let len = self
                        .site_edit
                        .slot_edit_lines
                        .get(self.site_edit.slot_edit_cursor_line)
                        .map_or(0, |l| l.chars().count());
                    if self.site_edit.slot_edit_cursor_col < len {
                        self.site_edit.slot_edit_cursor_col += 1;
                    } else if self.site_edit.slot_edit_cursor_line + 1
                        < self.site_edit.slot_edit_lines.len()
                    {
                        self.site_edit.slot_edit_cursor_line += 1;
                        self.site_edit.slot_edit_cursor_col = 0;
                    }
                    return;
                }
                _ => {}
            }
        }

        // 文本输入
        if k.modifiers == KeyModifiers::NONE || k.modifiers.contains(KeyModifiers::SHIFT) {
            match k.code {
                KeyCode::Char(c) => {
                    self.site_edit.push_slot_undo();
                    if let Some(line) = self
                        .site_edit
                        .slot_edit_lines
                        .get_mut(self.site_edit.slot_edit_cursor_line)
                    {
                        // 按字符位置插入（支持 CJK，但目前 col 用 char 计数）
                        let byte_idx = char_to_byte(line, self.site_edit.slot_edit_cursor_col);
                        line.insert(byte_idx, c);
                        self.site_edit.slot_edit_cursor_col += 1;
                    }
                }
                KeyCode::Backspace => {
                    if self.site_edit.slot_edit_cursor_col > 0 {
                        self.site_edit.push_slot_undo();
                        if let Some(line) = self
                            .site_edit
                            .slot_edit_lines
                            .get_mut(self.site_edit.slot_edit_cursor_line)
                        {
                            let prev_byte =
                                char_to_byte(line, self.site_edit.slot_edit_cursor_col - 1);
                            let cur_byte = char_to_byte(line, self.site_edit.slot_edit_cursor_col);
                            line.replace_range(prev_byte..cur_byte, "");
                            self.site_edit.slot_edit_cursor_col -= 1;
                        }
                    } else if self.site_edit.slot_edit_cursor_line > 0 {
                        self.site_edit.push_slot_undo();
                        let current = self
                            .site_edit
                            .slot_edit_lines
                            .remove(self.site_edit.slot_edit_cursor_line);
                        self.site_edit.slot_edit_cursor_line -= 1;
                        let prev_len = self
                            .site_edit
                            .slot_edit_lines
                            .get(self.site_edit.slot_edit_cursor_line)
                            .map_or(0, |l| l.chars().count());
                        if let Some(prev) = self
                            .site_edit
                            .slot_edit_lines
                            .get_mut(self.site_edit.slot_edit_cursor_line)
                        {
                            prev.push_str(&current);
                        }
                        self.site_edit.slot_edit_cursor_col = prev_len;
                    }
                }
                KeyCode::Enter => {
                    self.site_edit.push_slot_undo();
                    let rest = if let Some(line) = self
                        .site_edit
                        .slot_edit_lines
                        .get_mut(self.site_edit.slot_edit_cursor_line)
                    {
                        let byte_idx = char_to_byte(line, self.site_edit.slot_edit_cursor_col);
                        let rest: String = line[byte_idx..].to_string();
                        line.truncate(byte_idx);
                        rest
                    } else {
                        String::new()
                    };
                    self.site_edit
                        .slot_edit_lines
                        .insert(self.site_edit.slot_edit_cursor_line + 1, rest);
                    self.site_edit.slot_edit_cursor_line += 1;
                    self.site_edit.slot_edit_cursor_col = 0;
                }
                _ => {}
            }
        }
    }

    /// 保存表单模式编辑
    fn save_site_edit(&mut self, test_and_reload: bool) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }

        if !self.site_edit.validate() {
            return;
        }

        let params = self.site_edit.build_render_params();
        let kind = self.site_edit.site_kind();
        let mut content = match crate::template::renderer::render(kind, &params) {
            Ok(c) => c,
            Err(e) => {
                self.notification = Some(Notification::failure(format!("渲染失败：{}", e)));
                return;
            }
        };

        // 从原始配置中提取 SSL 配置并注入到新渲染的配置中
        let original_content = &self.site_edit.raw_lines.join("\n");
        let ssl_lines = crate::template::config_parser::extract_ssl_config(original_content);
        if !params.ssl_enabled && !ssl_lines.is_empty() {
            content = crate::template::config_parser::inject_ssl_config(&content, &ssl_lines);
        }

        let site_name = self.site_edit.site_name.clone();
        let expected_mtime = self.site_edit.mtime_at_load;
        self.site_edit.saving = true;
        self.site_edit.pending_save = Some(crate::domain::site::SaveSiteInput {
            name: site_name,
            content,
            test_and_reload,
            expected_mtime,
        });
        self.notification = Some(Notification::info(if test_and_reload {
            "正在保存、测试并重载 Nginx".to_string()
        } else {
            "正在保存配置".to_string()
        }));
    }

    /// 保存原始配置编辑
    fn save_raw_edit(&mut self, test_and_reload: bool) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }

        let content = self.site_edit.raw_lines.join("\n");
        let site_name = self.site_edit.site_name.clone();
        let expected_mtime = self.site_edit.mtime_at_load;
        self.site_edit.saving = true;
        self.site_edit.pending_save = Some(crate::domain::site::SaveSiteInput {
            name: site_name,
            content,
            test_and_reload,
            expected_mtime,
        });
        self.notification = Some(Notification::info(if test_and_reload {
            "正在保存、测试并重载 Nginx".to_string()
        } else {
            "正在保存配置".to_string()
        }));
    }

    /// 处理日志视图按键
    fn handle_logs_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let visible_lines = self.logs_visible_lines_estimate();
        let page_step = visible_lines.saturating_sub(LOGS_PAGE_SCROLL_FACTOR).max(1) as isize;

        // 搜索模式优先
        if self.logs.focused == LogsFocus::SearchInput {
            match k.code {
                KeyCode::Esc => {
                    self.logs.clear_search();
                    self.logs.focused = LogsFocus::LogContent;
                    return;
                }
                KeyCode::Enter => {
                    // 执行搜索
                    let query = self.logs.search_query.clone();
                    if let Some(q) = query {
                        self.logs.search(&q);
                        self.logs.reveal_current_match(visible_lines);
                    }
                    self.logs.focused = LogsFocus::LogContent;
                    return;
                }
                KeyCode::Char(c) => {
                    if let Some(ref mut q) = self.logs.search_query {
                        q.push(c);
                    } else {
                        self.logs.search_query = Some(c.to_string());
                    }
                    return;
                }
                KeyCode::Backspace => {
                    if let Some(ref mut q) = self.logs.search_query {
                        q.pop();
                    }
                    return;
                }
                _ => {}
            }
            return;
        }

        // 搜索输入模式：按 / 进入
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('/')) {
            self.logs.search_query = Some(String::new());
            self.logs.focused = LogsFocus::SearchInput;
            return;
        }

        // Space: 暂停/继续
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char(' ')) {
            if self.logs.paused {
                self.logs.follow_tail(visible_lines);
            } else {
                self.logs.toggle_pause();
            }
            return;
        }

        // c: 清屏
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('c')) {
            self.logs.clear_buffer();
            return;
        }

        // Tab: 切换焦点区域
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Tab) {
            self.logs.focused = match self.logs.focused {
                LogsFocus::SiteSelector => LogsFocus::KindSelector,
                LogsFocus::KindSelector => LogsFocus::LogContent,
                LogsFocus::LogContent => LogsFocus::SiteSelector,
                LogsFocus::SearchInput => LogsFocus::LogContent,
            };
            return;
        }

        // n/N: 搜索结果导航（有搜索结果时）
        if self.logs.search_query.is_some() && !self.logs.match_lines.is_empty() {
            if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('n')) {
                self.logs.next_match();
                self.logs.reveal_current_match(visible_lines);
                return;
            }
            if k.modifiers.contains(KeyModifiers::SHIFT) && matches!(k.code, KeyCode::Char('N')) {
                self.logs.prev_match();
                self.logs.reveal_current_match(visible_lines);
                return;
            }
        }

        // Esc: 清除搜索或返回侧边栏
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            if self.logs.search_query.is_some() {
                self.logs.clear_search();
            } else {
                self.focus = FocusArea::Sidebar;
            }
            return;
        }

        // 左右键：在站点选择器切换站点 / 类型选择器切换类型
        if k.modifiers == KeyModifiers::NONE {
            match self.logs.focused {
                LogsFocus::SiteSelector => {
                    match (k.code, !self.sites.list.is_empty()) {
                        (KeyCode::Left | KeyCode::Char('h'), true) => {
                            // 切换到上一个站点（从站点列表）
                            let cur_site = self.current_logs_site_name();
                            let cur_idx = self
                                .sites
                                .list
                                .iter()
                                .position(|s| Some(&s.name) == cur_site.as_ref());
                            let prev_idx = match cur_idx {
                                Some(i) => {
                                    if i == 0 {
                                        None
                                    } else {
                                        Some(i - 1)
                                    }
                                }
                                None => Some(self.sites.list.len() - 1),
                            };
                            let next_site = prev_idx.map(|i| self.sites.list[i].name.clone());
                            self.rebuild_logs_source(next_site, self.logs.source.kind());
                            self.logs.clear_buffer();
                            self.logs.pending_tail_change = true;
                        }
                        (KeyCode::Right | KeyCode::Char('l'), true) => {
                            // 切换到下一个站点
                            let cur_site = self.current_logs_site_name();
                            let cur_idx = self
                                .sites
                                .list
                                .iter()
                                .position(|s| Some(&s.name) == cur_site.as_ref());
                            let next_idx = match cur_idx {
                                Some(i) => {
                                    if i >= self.sites.list.len() - 1 {
                                        None
                                    } else {
                                        Some(i + 1)
                                    }
                                }
                                None => Some(0),
                            };
                            let next_site = next_idx.map(|i| self.sites.list[i].name.clone());
                            self.rebuild_logs_source(next_site, self.logs.source.kind());
                            self.logs.clear_buffer();
                            self.logs.pending_tail_change = true;
                        }
                        (KeyCode::Enter, _) => {
                            self.logs.focused = LogsFocus::KindSelector;
                        }
                        _ => {}
                    }
                }
                LogsFocus::KindSelector => match k.code {
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('t') => {
                        let next_kind = self.logs.source.kind().toggle();
                        let site_name = self.current_logs_site_name();
                        self.rebuild_logs_source(site_name, next_kind);
                        self.logs.clear_buffer();
                        self.logs.pending_tail_change = true;
                    }
                    KeyCode::Enter => {
                        self.logs.focused = LogsFocus::LogContent;
                    }
                    _ => {}
                },
                LogsFocus::LogContent => match k.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.logs.scroll_vertical(-1, visible_lines);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.logs.scroll_vertical(1, visible_lines);
                    }
                    KeyCode::PageUp => {
                        self.logs.scroll_vertical(-page_step, visible_lines);
                    }
                    KeyCode::PageDown => {
                        self.logs.scroll_vertical(page_step, visible_lines);
                    }
                    KeyCode::Home => {
                        self.logs.vertical_scroll = 0;
                        self.logs.paused = true;
                    }
                    KeyCode::End => {
                        self.logs.follow_tail(visible_lines);
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        self.logs.scroll_horizontal(-LOGS_HORIZONTAL_SCROLL_STEP);
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        self.logs.scroll_horizontal(LOGS_HORIZONTAL_SCROLL_STEP);
                    }
                    _ => {}
                },
                LogsFocus::SearchInput => {}
            }
        }
    }

    /// 处理证书页按键
    fn handle_certs_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if k.modifiers != KeyModifiers::NONE && !k.modifiers.contains(KeyModifiers::SHIFT) {
            return;
        }

        match k.code {
            KeyCode::Esc => {
                self.focus = FocusArea::Sidebar;
                return;
            }
            KeyCode::Tab => {
                self.certs.focused = match self.certs.focused {
                    CertsFocus::Table => {
                        self.certs.action_focus = CertsAction::Request;
                        CertsFocus::SiteActions
                    }
                    CertsFocus::SiteActions => {
                        self.certs.action_focus = CertsAction::RenewAll;
                        CertsFocus::GlobalActions
                    }
                    CertsFocus::GlobalActions => CertsFocus::Table,
                };
                return;
            }
            KeyCode::Char('r') => {
                self.certs.pending_refresh = true;
                return;
            }
            KeyCode::Char('c') => {
                self.certs.clear_output();
                return;
            }
            _ => {}
        }

        match self.certs.focused {
            CertsFocus::Table => match k.code {
                KeyCode::Up => self.certs_site_selector_move(-1),
                KeyCode::Down => self.certs_site_selector_move(1),
                KeyCode::Enter => {
                    self.certs.focused = CertsFocus::SiteActions;
                    self.certs.action_focus = CertsAction::Request;
                }
                _ => {}
            },
            CertsFocus::SiteActions => match k.code {
                KeyCode::Up => self.certs.focused = CertsFocus::Table,
                KeyCode::Down => {
                    self.certs.focused = CertsFocus::GlobalActions;
                    self.certs.action_focus = CertsAction::RenewAll;
                }
                KeyCode::Enter => self.request_certs_action(),
                _ => {}
            },
            CertsFocus::GlobalActions => match k.code {
                KeyCode::Left => self.certs.cycle_action(-1),
                KeyCode::Right => self.certs.cycle_action(1),
                KeyCode::Up => {
                    self.certs.focused = CertsFocus::SiteActions;
                    self.certs.action_focus = CertsAction::Request;
                }
                KeyCode::Enter => self.request_certs_action(),
                _ => {}
            },
        }
    }

    fn certs_site_selector_move(&mut self, delta: i32) {
        if self.sites.list.is_empty() {
            return;
        }
        let len = self.sites.list.len() as i32;
        let mut idx = self.certs.site_selector_index as i32 + delta;
        if idx < 0 {
            idx = len - 1;
        } else if idx >= len {
            idx = 0;
        }
        self.certs.site_selector_index = idx as usize;
    }

    fn request_certs_action(&mut self) {
        if self.certs.running.is_some() {
            return;
        }
        match self.certs.action_focus {
            CertsAction::Request => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                if !self.ctx.deps().certbot {
                    self.notification = Some(Notification::failure("certbot 未安装".to_string()));
                    return;
                }
                let Some(site) = self.sites.list.get(self.certs.site_selector_index) else {
                    self.notification = Some(Notification::failure(
                        "请先在站点选择器中选定一个有 server_name 的站点",
                    ));
                    return;
                };
                if site.all_domains.is_empty() {
                    self.notification = Some(Notification::failure(format!(
                        "站点 {} 未配置 server_name，无法申请证书",
                        site.name
                    )));
                    return;
                }
                let modal = Modal::confirm(
                    "🔐 申请证书",
                    vec![
                        format!("站点: {}", site.name),
                        format!("域名: {}", site.all_domains.join(", ")),
                        "".into(),
                        "将使用 certbot certonly 签发证书，并由 ngtool 写入 SSL 配置。".into(),
                    ],
                    "确认申请",
                    crate::ui::modal::ModalAction::RequestCertForSite {
                        site_name: site.name.clone(),
                        domains: site.all_domains.clone(),
                    },
                );
                self.modal = Some(modal);
            }
            CertsAction::RenewAll => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                if !self.ctx.deps().certbot {
                    self.notification = Some(Notification::failure("certbot 未安装".to_string()));
                    return;
                }
                let modal = Modal::confirm(
                    "🔄 续期所有证书",
                    vec![
                        "将执行 certbot renew，对全部到期证书统一续期。".into(),
                        "进度会输出到下方操作输出区。".into(),
                    ],
                    "确认续期",
                    crate::ui::modal::ModalAction::RenewAllCerts,
                );
                self.modal = Some(modal);
            }
            CertsAction::CheckAutoRenew => {
                self.certs.pending_check_renew = true;
            }
            CertsAction::InstallDeployHook => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                if self
                    .certs
                    .auto_renew
                    .as_ref()
                    .is_some_and(|s| s.deploy_hook_present)
                {
                    self.notification = Some(Notification::info("deploy hook 已安装".to_string()));
                    return;
                }
                self.modal = Some(crate::ui::modal::Modal::confirm_install_deploy_hook());
            }
            CertsAction::DeleteOrphan => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                if !self.ctx.deps().certbot {
                    self.notification = Some(Notification::failure("certbot 未安装".to_string()));
                    return;
                }
                let candidates = crate::domain::cert::cleanup_candidates(&self.certs.list);
                let referenced_skips =
                    crate::domain::cert::referenced_cleanup_skips(&self.certs.list);
                if candidates.is_empty() {
                    let msg = if referenced_skips.is_empty() {
                        "当前没有可清理的多余证书"
                    } else {
                        "多余证书仍被 nginx 配置引用，已跳过"
                    };
                    self.notification = Some(Notification::info(msg.to_string()));
                    return;
                }
                let cert_names: Vec<String> =
                    candidates.iter().map(|c| c.cert.name.clone()).collect();
                let mut lines = vec![
                    "⚠️  这是全局操作，会清理所有站点的多余证书！".into(),
                    "".into(),
                    format!(
                        "发现 {} 个可清理的多余证书（孤立或已被其他证书覆盖，且未被 nginx 引用）：",
                        candidates.len()
                    ),
                    cert_names.join(", "),
                ];
                if !referenced_skips.is_empty() {
                    let skipped = referenced_skips
                        .iter()
                        .map(|c| c.cert.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    lines.push("".into());
                    lines.push(format!("已跳过仍被 nginx 引用的证书：{}", skipped));
                }
                lines.extend([
                    "".into(),
                    "将逐个执行 certbot delete 删除这些证书。".into(),
                    "⚠️  此操作不可撤销！".into(),
                ]);
                let modal = Modal::confirm(
                    "🗑️  清理全局多余证书",
                    lines,
                    "确认删除",
                    crate::ui::modal::ModalAction::DeleteOrphanCerts { cert_names },
                );
                self.modal = Some(modal);
            }
        }
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

    /// 处理备份页按键
    fn handle_backup_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        if k.modifiers != KeyModifiers::NONE && !k.modifiers.contains(KeyModifiers::SHIFT) {
            return;
        }
        match k.code {
            KeyCode::Esc => self.focus = FocusArea::Sidebar,
            KeyCode::Up => self.backup.move_cursor(-1),
            KeyCode::Down => self.backup.move_cursor(1),
            KeyCode::Char('R') => {
                // 大写 R：刷新列表（保留 r 给还原）
                self.backup.pending_refresh = true;
            }
            KeyCode::Char('r') => {
                self.request_backup_restore();
            }
            KeyCode::Char('n') => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                let backup_dir = self.ctx.paths.backups.display().to_string();
                let modal = Modal::confirm(
                    "💾 创建备份",
                    vec![
                        format!("保存目录：{}", backup_dir),
                        "".into(),
                        "Nginx 配置快照范围：".into(),
                        "  /etc/nginx 根目录配置文件（含 nginx.conf）".into(),
                        "  sites-available/、sites-enabled/".into(),
                        "  conf.d/、snippets/、stream-conf.d/".into(),
                        "  modules-enabled/（含符号链接关系）".into(),
                    ],
                    "确认创建",
                    crate::ui::modal::ModalAction::CreateBackup,
                );
                self.modal = Some(modal);
            }
            KeyCode::Char('d') => {
                self.request_backup_delete();
            }
            KeyCode::Char('c') => {
                self.backup.clear_output_buffer();
            }
            // 备份页设计未定义视图内 Tab/Left/Right 子区域循环（design.md §四 视图 6）。
            // 这里显式忽略，避免与"Esc 返回侧栏"的统一约定冲突；状态栏会持续提示 [Esc] 返回侧栏。
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {}
            _ => {}
        }
    }

    fn request_backup_delete(&mut self) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        let Some(b) = self.backup.current() else {
            return;
        };
        let path = b.path.clone();
        let name = b.name.clone();
        let modal = Modal::confirm(
            "⚠️  确认删除备份",
            vec![format!("即将删除：{}", name), "此操作不可撤销".into()],
            "确认删除",
            crate::ui::modal::ModalAction::DeleteBackup(path),
        );
        self.modal = Some(modal);
    }

    fn request_backup_restore(&mut self) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        let Some(b) = self.backup.current() else {
            return;
        };
        if !b.restorable() {
            self.notification = Some(Notification::failure(
                "该备份缺少 manifest 或 schema 不兼容，仅可查看不可还原".to_string(),
            ));
            return;
        }
        let manifest = b.manifest.clone().unwrap();
        let path = b.path.clone();
        let created_at = b.created_at_label();
        let impact = match crate::domain::backup::impact_for_restore(&self.ctx, &manifest) {
            Ok(i) => i,
            Err(e) => {
                self.notification = Some(Notification::failure(format!("无法计算影响摘要：{}", e)));
                return;
            }
        };

        let mut body: Vec<String> = Vec::new();
        body.push(format!("时间：{}", created_at));
        body.push(format!(
            "内容：{} 个文件，{} 个链接",
            manifest.scope.files.len(),
            manifest.scope.symlinks.len()
        ));
        if !impact.will_enable.is_empty() {
            body.push(format!("将启用：{}", impact.will_enable.join(", ")));
        }
        if !impact.will_disable.is_empty() {
            body.push(format!("将停用：{}", impact.will_disable.join(", ")));
        }
        if !impact.missing_in_backup.is_empty() {
            body.push(format!(
                "⚠ 备份中标记启用但 conf 缺失：{}",
                impact.missing_in_backup.join(", ")
            ));
        }
        body.push(String::new());
        body.push("将自动创建 pre-restore 备份。".into());

        let modal = Modal::confirm(
            "⚠️  确认还原备份",
            body,
            "确认还原",
            crate::ui::modal::ModalAction::RestoreBackup(path),
        );
        self.modal = Some(modal);
    }
}

