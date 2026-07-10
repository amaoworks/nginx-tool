//! AppState service 相关按键与动作处理。

use crate::app::state::app::AppState;
use crate::app::state::common::Notification;
use crate::app::state::service::ServiceButton;
use crate::ui::modal::Modal;

impl AppState {
    pub(crate) fn request_service_action(&mut self) {
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
}
