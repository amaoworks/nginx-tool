//! AppState certs 相关按键与动作处理。

use crate::app::state::app::AppState;
use crate::app::state::certs::{CertsAction, CertsFocus};
use crate::app::state::common::{FocusArea, Notification};
use crate::ui::modal::Modal;

impl AppState {
    pub(crate) fn handle_certs_key(&mut self, k: crossterm::event::KeyEvent) {
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
                        self.certs.ensure_global_action_focus();
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
                // 站点级：表上 Enter 直接申请当前站点证书
                KeyCode::Enter => {
                    self.certs.action_focus = CertsAction::Request;
                    self.request_certs_action();
                }
                _ => {}
            },
            CertsFocus::GlobalActions => match k.code {
                KeyCode::Left => self.certs.cycle_global_action(-1),
                KeyCode::Right => self.certs.cycle_global_action(1),
                KeyCode::Up => self.certs.focused = CertsFocus::Table,
                KeyCode::Enter => {
                    self.certs.ensure_global_action_focus();
                    self.request_certs_action();
                }
                _ => {}
            },
        }
    }

    pub(crate) fn certs_site_selector_move(&mut self, delta: i32) {
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

    pub(crate) fn request_certs_action(&mut self) {
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
                        "请先选定一个有 server_name 的站点",
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
                    self.notification = Some(Notification::info("重载钩子已安装".to_string()));
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
}
