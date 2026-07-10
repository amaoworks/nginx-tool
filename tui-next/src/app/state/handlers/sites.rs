//! AppState 站点列表、新建表单与编辑相关处理。

use crate::app::route::{Route, SitesRoute};
use crate::app::state::app::AppState;
use crate::app::state::common::{FocusArea, Notification};
use crate::app::state::site_edit::{char_to_byte, EditFocus, SiteEditState};
use crate::app::state::site_form::FormField;
use crate::domain::log::LogKind;
use crate::ui::modal::Modal;

impl AppState {
    pub(crate) fn toggle_site_edit_scheme(&mut self) {
        self.site_edit.upstream_scheme = if self.site_edit.upstream_scheme == "http" {
            "https".into()
        } else {
            "http".into()
        };
        self.site_edit.dirty = true;
    }

    pub(crate) fn toggle_site_edit_current_flag(&mut self) {
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

    pub(crate) fn adjust_site_edit_managed_focus(&mut self, forward: bool) {
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

    pub(crate) fn request_site_delete(&mut self) {
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

    pub(crate) fn request_cert_for_current_site(&mut self) {
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

    pub(crate) fn goto_site_log(&mut self) {
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

    pub(crate) fn request_site_toggle(&mut self) {
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

    pub(crate) fn handle_site_form_key(&mut self, k: crossterm::event::KeyEvent) {
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

    pub(crate) fn submit_site_form(&mut self) {
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

    pub(crate) fn enter_site_edit(&mut self) {
        let Some(site) = self.sites.current() else {
            return;
        };
        let name = site.name.clone();
        let config_path = site.config_path.clone();
        self.open_site_edit(&name, &config_path, true);
    }

    /// 打开站点编辑页；`check_health` 为 true 时若发现可修复问题会弹出确认框。
    pub(crate) fn open_site_edit(
        &mut self,
        name: &str,
        config_path: &std::path::Path,
        check_health: bool,
    ) {
        let mtime_at_load = std::fs::metadata(config_path)
            .ok()
            .and_then(|m| m.modified().ok());

        let content = match std::fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(e) => {
                self.notification =
                    Some(Notification::failure(format!("无法读取配置文件：{}", e)));
                return;
            }
        };

        let parsed = crate::template::config_parser::parse_for_edit(&content);
        self.site_edit = SiteEditState::from_parsed(name, &parsed);
        self.site_edit.mtime_at_load = mtime_at_load;
        self.site_edit.seal_original();
        self.route = Route::Sites(SitesRoute::EditManaged {
            site_name: name.to_string(),
        });
        self.focus = FocusArea::Content;

        if !check_health || self.run_mode.is_readonly() {
            return;
        }

        if let Ok(issues) = crate::domain::config_health::scan_config_file(config_path) {
            if !issues.is_empty() {
                let lines: Vec<String> = issues.iter().map(|i| i.description()).collect();
                self.modal = Some(Modal::confirm_config_health_fix(
                    name,
                    config_path.to_path_buf(),
                    lines,
                ));
            }
        }
    }

    /// 对站点配置执行健康修复后重新载入编辑器
    pub(crate) fn apply_site_config_health_fix(
        &mut self,
        site_name: &str,
        path: &std::path::Path,
    ) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，无法修改配置文件".to_string(),
            ));
            return;
        }
        match crate::domain::config_health::apply_all_fixes(path) {
            Ok(report) if report.applied.is_empty() => {
                self.notification = Some(Notification::info("没有需要修复的问题".to_string()));
                self.open_site_edit(site_name, path, false);
            }
            Ok(report) => {
                let summary = report.applied.join("；");
                self.notification = Some(Notification::success(format!(
                    "已修复 {} 项：{}",
                    report.applied.len(),
                    summary
                )));
                // 修复后建议用户自行「保存并测试」，这里只重载编辑内容
                self.open_site_edit(site_name, path, false);
            }
            Err(e) => {
                self.notification = Some(Notification::failure(format!("自动修复失败：{}", e)));
            }
        }
    }

    pub(crate) fn handle_site_edit_managed_key(&mut self, k: crossterm::event::KeyEvent) {
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

    pub(crate) fn handle_site_edit_advanced_key(&mut self, k: crossterm::event::KeyEvent) {
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

    pub(crate) fn handle_raw_edit_key(&mut self, k: crossterm::event::KeyEvent) {
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

    pub(crate) fn handle_slot_full_edit_key(&mut self, k: crossterm::event::KeyEvent) {
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

    pub(crate) fn save_site_edit(&mut self, test_and_reload: bool) {
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

    pub(crate) fn save_raw_edit(&mut self, test_and_reload: bool) {
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
}
