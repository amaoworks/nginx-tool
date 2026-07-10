//! AppState 全局导航、侧栏、弹窗与菜单切换。

use crate::app::route::{MenuItem, Route, SitesRoute};
use crate::app::state::app::{
    AppState, CERTS_AUTO_REFRESH, DASHBOARD_AUTO_REFRESH, SITES_AUTO_REFRESH,
};
use crate::app::state::common::{FocusArea, Notification};
use crate::app::state::logs::LogsFocus;
use crate::app::state::service::ServiceButton;
use crate::app::state::site_edit::{EditFocus, SiteEditState};
use crate::app::state::site_form::{FormField, SiteFormState};
use crate::ui::modal::Modal;

impl AppState {
    pub(crate) fn is_in_text_input(&self) -> bool {
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

    pub(crate) fn handle_key(&mut self, k: crossterm::event::KeyEvent) {
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

    pub(crate) fn handle_sidebar_key(&mut self, k: crossterm::event::KeyEvent) {
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

    pub(crate) fn handle_content_key(&mut self, k: crossterm::event::KeyEvent) {
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

    pub(crate) fn handle_modal_key(&mut self, k: crossterm::event::KeyEvent) {
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

    pub(crate) fn execute_modal_action(&mut self, action: crate::ui::modal::ModalAction) {
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
            ModalAction::FixSiteConfig { site_name, path } => {
                self.apply_site_config_health_fix(&site_name, &path);
            }
        }
    }

    pub(crate) fn go_to_menu(&mut self, item: MenuItem) {
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

    pub(crate) fn move_menu(&mut self, delta: i32) {
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
}
