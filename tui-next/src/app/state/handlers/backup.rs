//! AppState backup 相关按键与动作处理。

use crate::app::state::app::AppState;
use crate::app::state::common::{FocusArea, Notification};
use crate::ui::modal::Modal;

impl AppState {
    pub(crate) fn handle_backup_key(&mut self, k: crossterm::event::KeyEvent) {
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
                        "  配置实际引用的 Let's Encrypt 证书依赖".into(),
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

    pub(crate) fn request_backup_delete(&mut self) {
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

    pub(crate) fn request_backup_restore(&mut self) {
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
        let missing_dependencies =
            match crate::domain::backup::missing_dependencies_for_restore(&path) {
                Ok(paths) => paths,
                Err(e) => {
                    self.notification =
                        Some(Notification::failure(format!("无法校验备份依赖：{}", e)));
                    return;
                }
            };
        if !missing_dependencies.is_empty() {
            self.notification = Some(Notification::failure(
                "该备份未携带当前机器缺少的证书，无法安全还原".to_string(),
            ));
            self.backup.push_output([
                "✗ 还原前检查失败：备份缺少证书依赖".into(),
                format!("  缺少：{}", missing_dependencies.join(", ")),
                "  请使用新版 TUI 在源机器重新创建备份后再还原".into(),
            ]);
            return;
        }

        let mut body: Vec<String> = Vec::new();
        body.push(format!("时间：{}", created_at));
        body.push(format!(
            "内容：{} 个 Nginx 文件，{} 个链接，{} 个证书文件",
            manifest.scope.files.len(),
            manifest.scope.symlinks.len(),
            manifest.scope.external_files.len()
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
