pub mod focus;
pub mod layout;
pub mod modal;
pub mod theme;
pub mod views;
pub mod widgets;

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::route::{MenuItem, Route, SitesRoute};
use crate::app::state::{AppState, FocusArea};

pub const MIN_COLS: u16 = 80;
pub const MIN_ROWS: u16 = 24;

pub fn draw(frame: &mut Frame, state: &AppState) {
    let size = frame.area();

    if size.width < MIN_COLS || size.height < MIN_ROWS {
        draw_too_small(frame, size);
        return;
    }

    let areas = layout::root_layout(size);
    draw_header(frame, areas.header, state);
    draw_sidebar(frame, areas.sidebar, state);
    draw_content(frame, areas.content, state);
    draw_footer(frame, areas.footer, state);

    if let Some(notification) = &state.notification {
        draw_notification(frame, areas.content, notification);
    }

    if let Some(modal) = &state.modal {
        modal::render(frame, size, modal);
    }
}

fn draw_too_small(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "终端尺寸过小",
            Style::default()
                .fg(theme::FG_WARN)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!(
            "当前 {}×{}，请放大窗口至 {}×{} 以上",
            area.width, area.height, MIN_COLS, MIN_ROWS
        )),
        Line::from(""),
        Line::from(Span::styled("[q] 退出", Style::default().fg(theme::FG_DIM))),
    ];
    let p = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(p, area);
}

fn draw_header(frame: &mut Frame, area: Rect, state: &AppState) {
    // 左：品牌；右：模式 + 依赖检测
    let left = Span::styled(
        " 🌐 Nginx-Tools ",
        Style::default()
            .bg(theme::BG_HEADER)
            .fg(theme::FG_HEADER)
            .add_modifier(Modifier::BOLD),
    );
    let deps = state.ctx.deps();
    let glyph = |ok: bool| if ok { "✓" } else { "✗" };
    let right_text = format!(
        "[模式: {}]  nginx {}  systemctl {}  certbot {} ",
        state.run_mode.label(),
        glyph(deps.nginx),
        glyph(deps.systemctl),
        glyph(deps.certbot),
    );
    let right = Span::styled(
        right_text.clone(),
        Style::default().bg(theme::BG_HEADER).fg(theme::FG_HEADER),
    );

    // 计算填充
    let left_w = " 🌐 Nginx-Tools ".width() as u16;
    let right_w = right_text.width() as u16;
    let pad_w = area.width.saturating_sub(left_w + right_w);
    let pad = Span::styled(
        " ".repeat(pad_w as usize),
        Style::default().bg(theme::BG_HEADER),
    );

    let line = Line::from(vec![left, pad, right]);
    let p = Paragraph::new(line);
    frame.render_widget(p, area);
}

fn draw_sidebar(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(theme::BORDER));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cur = state.current_menu();
    let sidebar_focused = state.focus == FocusArea::Sidebar;

    let mut lines: Vec<Line> = Vec::with_capacity(MenuItem::ALL.len() + 2);
    lines.push(Line::from(""));
    for item in MenuItem::ALL {
        let is_current = item == cur;
        let prefix = if is_current && sidebar_focused {
            "▸ "
        } else if is_current {
            "● "
        } else {
            "  "
        };
        let style = if is_current && sidebar_focused {
            Style::default()
                .bg(theme::BG_SELECTED)
                .fg(theme::FG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default()
                .fg(theme::FG_HINT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::FG_NORMAL)
        };
        let label = format!("{}{}", prefix, item.label());
        lines.push(Line::from(Span::styled(label, style)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " 1-6 直达 / Tab 切换 ",
        Style::default().fg(theme::FG_DIM),
    )));

    let p = Paragraph::new(lines);
    frame.render_widget(p, inner);
}

fn draw_content(frame: &mut Frame, area: Rect, state: &AppState) {
    match &state.route {
        Route::Dashboard => views::dashboard::render(frame, area, state),
        Route::Sites(SitesRoute::List) => views::sites::render_list(frame, area, state),
        Route::Sites(SitesRoute::New) => views::site_form::render(frame, area, state),
        Route::Sites(SitesRoute::EditManaged { .. }) => {
            views::site_edit::render(frame, area, state)
        }
        Route::Sites(SitesRoute::EditAdvanced { .. }) => {
            views::site_edit_advanced::render(frame, area, state)
        }
        Route::Sites(SitesRoute::EditRaw { .. }) => {
            views::site_edit_raw::render(frame, area, state)
        }
        Route::Sites(SitesRoute::EditSlotFull { .. }) => {
            views::site_edit_slot_full::render(frame, area, state)
        }
        Route::Certs => views::certs::render(frame, area, state),
        Route::Logs => views::logs::render(frame, area, state),
        Route::Service => views::service::render(frame, area, state),
        Route::Backup => views::backup::render(frame, area, state),
    }
}

fn draw_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let hints = footer_hints(state);
    let line = Line::from(vec![
        Span::styled(" ", Style::default().bg(theme::BG_FOOTER)),
        Span::styled(
            hints,
            Style::default().bg(theme::BG_FOOTER).fg(theme::FG_FOOTER),
        ),
    ]);
    let p = Paragraph::new(line).style(Style::default().bg(theme::BG_FOOTER));
    frame.render_widget(p, area);
}

fn footer_hints(state: &AppState) -> String {
    let route_label = match &state.route {
        Route::Dashboard => "仪表盘",
        Route::Sites(SitesRoute::List) => "站点列表",
        Route::Sites(SitesRoute::New) => "新建站点",
        Route::Sites(SitesRoute::EditManaged { .. }) => "编辑站点 / 托管",
        Route::Sites(SitesRoute::EditAdvanced { .. }) => "编辑站点 / 高级",
        Route::Sites(SitesRoute::EditRaw { .. }) => "编辑站点 / 原始",
        Route::Sites(SitesRoute::EditSlotFull { .. }) => "编辑站点 / 注入槽全屏",
        Route::Certs => "证书管理",
        Route::Logs => "日志查看",
        Route::Service => "服务控制",
        Route::Backup => "备份还原",
    };

    let mut tips: Vec<&'static str> = Vec::new();
    match &state.route {
        Route::Dashboard => {
            tips.push("[r] 刷新");
            tips.push("[Esc] 返回侧栏");
        }
        Route::Sites(SitesRoute::List) => {
            tips.push("[n] 新建");
            tips.push("[Enter] 编辑");
            tips.push("[s] 启用/停用");
            tips.push("[d] 删除");
            tips.push("[c] 证书");
            tips.push("[l] 日志");
            tips.push("[Esc] 返回侧栏");
        }
        Route::Sites(SitesRoute::New) => {
            tips.push("[Tab] 切区域");
            tips.push("[↑↓←→] 移动");
            tips.push("[Space] 开关");
            tips.push("[Enter] 确认/提交");
            tips.push("[Esc] 返回");
        }
        Route::Sites(SitesRoute::EditManaged { .. }) => {
            tips.push("[Ctrl+S] 保存测试");
            tips.push("[Ctrl+W] 仅保存");
            tips.push("[Ctrl+D] 重置");
            tips.push("[a] 高级");
            tips.push("[o] 原始");
            tips.push("[Esc] 返回");
        }
        Route::Sites(SitesRoute::EditAdvanced { .. }) => {
            tips.push("[←→] 切槽位");
            tips.push("[↑↓] 选模板");
            tips.push("[Enter/Space] 追加");
            tips.push("[Ctrl+R] 替换");
            tips.push("[Ctrl+E] 全屏槽");
            tips.push("[a] 托管");
            tips.push("[o] 原始");
            tips.push("[Esc] 返回");
        }
        Route::Sites(SitesRoute::EditRaw { .. }) => {
            tips.push("[Ctrl+S] 保存测试");
            tips.push("[Ctrl+W] 仅保存");
            tips.push("[Ctrl+Z/Y] 撤销/重做");
            tips.push("[o] 托管");
            tips.push("[Esc] 返回");
        }
        Route::Sites(SitesRoute::EditSlotFull { .. }) => {
            tips.push("[Ctrl+S] 完成");
            tips.push("[Ctrl+D] 清空");
            tips.push("[Ctrl+Z/Y] 撤销/重做");
            tips.push("[Esc] 取消");
        }
        Route::Certs => {
            tips.push("[Tab] 切换区域");
            tips.push("[Enter] 执行");
            tips.push("[r] 续签");
            tips.push("[Esc] 返回侧栏");
        }
        Route::Logs => {
            tips.push("[Tab] 切换区域");
            tips.push("[Space] 暂停");
            tips.push("[c] 清屏");
            tips.push("[/] 搜索");
            tips.push("[Esc] 返回侧栏");
        }
        Route::Service => {
            tips.push("[Tab] 切换按钮");
            tips.push("[Enter] 执行");
            tips.push("[c] 清屏");
            tips.push("[Esc] 返回侧栏");
        }
        Route::Backup => {
            tips.push("[n] 新建");
            tips.push("[r] 还原");
            tips.push("[d] 删除");
            tips.push("[R] 刷新");
            tips.push("[c] 清空输出");
            tips.push("[Esc] 返回侧栏");
        }
    }

    let mut s = format!(" {} | ", route_label);
    s.push_str(&tips.join(" "));
    s.push_str(" | [q] 退出");
    s
}

fn draw_notification(frame: &mut Frame, content: Rect, n: &crate::app::state::Notification) {
    if content.height < 2 {
        return;
    }
    let text = format!("{} {}", n.kind.glyph(), n.message);
    let w = (text.width() as u16 + 4).min(content.width);
    let bar = Rect {
        x: content.x + content.width.saturating_sub(w),
        y: content.y + content.height.saturating_sub(1),
        width: w,
        height: 1,
    };
    let span = Span::styled(format!(" {} ", text), Style::default().fg(n.kind.fg()));
    frame.render_widget(Paragraph::new(Line::from(span)), bar);
}
