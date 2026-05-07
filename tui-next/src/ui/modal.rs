use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::ui::layout::centered_rect;
use crate::ui::theme;

/// 弹窗回调动作枚举。新增高危操作时在此扩展。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ModalAction {
    None,
    Quit,
    /// 确认重启 Nginx 服务
    RestartNginx,
    /// 确认放弃新建站点表单已填写的内容
    DiscardSiteForm,
    /// 确认放弃站点编辑器中的修改
    DiscardSiteEdit,
    /// 确认为指定站点申请证书（携带域名列表，避免后续状态漂移）
    RequestCertForSite {
        site_name: String,
        domains: Vec<String>,
    },
    /// 确认 `certbot renew`
    RenewAllCerts,
    /// 确认创建备份
    CreateBackup,
    /// 确认删除指定备份
    DeleteBackup(std::path::PathBuf),
    /// 确认还原指定备份
    RestoreBackup(std::path::PathBuf),
    /// 确认删除指定站点
    DeleteSite {
        site_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalButton {
    Confirm,
    Cancel,
}

/// 高危操作确认弹窗，详见 design.md §三 弹窗设计规范。
/// 默认聚焦"取消"以防误操作。
#[derive(Debug, Clone)]
pub struct Modal {
    pub title: String,
    pub body: Vec<String>,
    pub confirm_label: String,
    pub cancel_label: String,
    pub focused: ModalButton,
    pub action: ModalAction,
}

impl Modal {
    #[allow(dead_code)]
    pub fn confirm(
        title: impl Into<String>,
        body: Vec<String>,
        confirm_label: impl Into<String>,
        action: ModalAction,
    ) -> Self {
        Self {
            title: title.into(),
            body,
            confirm_label: confirm_label.into(),
            cancel_label: "取消".into(),
            focused: ModalButton::Cancel, // 默认聚焦取消，防止误操作
            action,
        }
    }

    #[allow(dead_code)]
    pub fn confirm_quit() -> Self {
        Self::confirm(
            "⚠️  确认退出",
            vec!["即将退出 Nginx-Tools。".into()],
            "确认退出",
            ModalAction::Quit,
        )
    }

    pub fn confirm_restart_nginx() -> Self {
        Self::confirm(
            "⚠️  确认重启服务",
            vec!["重启 Nginx 服务将导致".into(), "所有连接短暂中断".into()],
            "确认重启",
            ModalAction::RestartNginx,
        )
    }

    /// 新建站点表单有内容时，按 Esc 确认是否放弃
    pub fn confirm_discard_site_form() -> Self {
        Self::confirm(
            "⚠️  确认放弃新建",
            vec![
                "表单已填写的内容将丢失。".into(),
                "确认离开新建站点页面？".into(),
            ],
            "放弃",
            ModalAction::DiscardSiteForm,
        )
    }

    /// 编辑器有未保存修改时，按 Esc 确认是否放弃
    pub fn confirm_discard_site_edit() -> Self {
        Self::confirm(
            "⚠️  确认放弃修改",
            vec!["有尚未保存的修改。".into(), "确认离开编辑页面？".into()],
            "放弃",
            ModalAction::DiscardSiteEdit,
        )
    }

    pub fn toggle_focus(&mut self) {
        self.focused = match self.focused {
            ModalButton::Confirm => ModalButton::Cancel,
            ModalButton::Cancel => ModalButton::Confirm,
        };
    }

    pub fn confirm_action(&self) -> ModalAction {
        match self.focused {
            ModalButton::Confirm => self.action.clone(),
            ModalButton::Cancel => ModalAction::None,
        }
    }
}

pub fn render(frame: &mut Frame, parent: Rect, modal: &Modal) {
    // 计算合适尺寸：宽度按文本长度，高度按行数，限制在 60×16 以内
    let body_lines: u16 = modal.body.len() as u16;
    let h = (body_lines + 6).min(parent.height.saturating_sub(2)).max(7);
    let w = 56u16.min(parent.width.saturating_sub(2));
    let area = centered_rect(parent, w, h);

    // 清空底层内容并绘制对话框
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::FG_ERR))
        .title(Span::styled(
            format!(" {} ", modal.title),
            Style::default()
                .fg(theme::FG_HEADER)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, area);

    let inner = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };

    // 文本区
    let text_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };
    let body_lines: Vec<Line> = modal.body.iter().map(|l| Line::raw(l.as_str())).collect();
    let body = Paragraph::new(body_lines)
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Left);
    frame.render_widget(body, text_area);

    // 按钮行
    let btn_y = inner.y + inner.height.saturating_sub(1);
    let btn_area = Rect {
        x: inner.x,
        y: btn_y,
        width: inner.width,
        height: 1,
    };
    let buttons = button_row(modal);
    frame.render_widget(buttons, btn_area);
}

fn button_row(modal: &Modal) -> Paragraph<'_> {
    let confirm = render_button(&modal.confirm_label, modal.focused == ModalButton::Confirm);
    let cancel = render_button(&modal.cancel_label, modal.focused == ModalButton::Cancel);
    let line = Line::from(vec![Span::raw("    "), confirm, Span::raw("    "), cancel]);
    Paragraph::new(line).alignment(Alignment::Center)
}

fn render_button<'a>(label: &'a str, focused: bool) -> Span<'a> {
    if focused {
        Span::styled(
            format!("[[ {} ]]", label),
            Style::default()
                .fg(theme::FG_HEADER)
                .bg(theme::BG_SELECTED)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(format!("[ {} ]", label), Style::default().fg(theme::FG_DIM))
    }
}
