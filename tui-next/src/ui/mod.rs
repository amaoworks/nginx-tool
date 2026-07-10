pub mod cursor;
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
use crate::app::state::{AppState, CertsFocus, EditFocus, FocusArea, FormField, LogsFocus};

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
    let sidebar_focused = state.focus == FocusArea::Sidebar;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(focus::panel_border_style(sidebar_focused))
        .title(Span::styled(
            " 菜单 ",
            focus::selected_text_style(sidebar_focused),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cur = state.current_menu();

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
            focus::focused_button_style()
        } else if is_current {
            focus::selected_text_style(false)
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
    let content_focused = state.focus == FocusArea::Content;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(focus::panel_border_style(content_focused))
        .title(Span::styled(
            " 内容 ",
            focus::selected_text_style(content_focused),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    match &state.route {
        Route::Dashboard => views::dashboard::render(frame, inner, state),
        Route::Sites(SitesRoute::List) => views::sites::render_list(frame, inner, state),
        Route::Sites(SitesRoute::New) => views::site_form::render(frame, inner, state),
        Route::Sites(SitesRoute::EditManaged { .. }) => {
            views::site_edit::render(frame, inner, state)
        }
        Route::Sites(SitesRoute::EditAdvanced { .. }) => {
            views::site_edit_advanced::render(frame, inner, state)
        }
        Route::Sites(SitesRoute::EditRaw { .. }) => {
            views::site_edit_raw::render(frame, inner, state)
        }
        Route::Sites(SitesRoute::EditSlotFull { .. }) => {
            views::site_edit_slot_full::render(frame, inner, state)
        }
        Route::Certs => views::certs::render(frame, inner, state),
        Route::Logs => views::logs::render(frame, inner, state),
        Route::Service => views::service::render(frame, inner, state),
        Route::Backup => views::backup::render(frame, inner, state),
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
    if state.focus == FocusArea::Sidebar {
        return " 菜单 | [↑↓] 选择 [Enter] 进入 [1-6] 直达 | [q] 退出".to_string();
    }

    let route_label = match &state.route {
        Route::Dashboard => "总览",
        Route::Sites(SitesRoute::List) => "站点列表",
        Route::Sites(SitesRoute::New) => "新建站点",
        Route::Sites(SitesRoute::EditManaged { .. }) => "编辑 · 托管模式",
        Route::Sites(SitesRoute::EditAdvanced { .. }) => "编辑 · 高级模式",
        Route::Sites(SitesRoute::EditRaw { .. }) => "编辑 · 原始配置",
        Route::Sites(SitesRoute::EditSlotFull { .. }) => "编辑 · 注入槽全屏",
        Route::Certs => "证书",
        Route::Logs => "日志",
        Route::Service => "服务",
        Route::Backup => "备份",
    };

    let tips: Vec<&'static str> = match &state.route {
        Route::Dashboard => vec!["[r] 刷新", "[Tab] 切区域", "[Esc] 返回侧栏"],
        Route::Sites(SitesRoute::List) => vec![
            "[↑↓] 选择",
            "[Enter] 编辑",
            "[s] 启停",
            "[o] 排序",
            "[p] 顺/逆",
            "[n] 新建",
            "[d] 删除",
            "[c] 申请证书",
            "[l] 查看日志",
            "[r] 刷新",
        ],
        Route::Sites(SitesRoute::New) => footer_hints_for_site_form(state),
        Route::Sites(SitesRoute::EditManaged { .. }) => footer_hints_for_site_edit(state),
        Route::Sites(SitesRoute::EditAdvanced { .. }) => footer_hints_for_site_advanced(state),
        Route::Sites(SitesRoute::EditRaw { .. }) => footer_hints_for_site_raw(),
        Route::Sites(SitesRoute::EditSlotFull { .. }) => footer_hints_for_slot_full(),
        Route::Certs => footer_hints_for_certs(state),
        Route::Logs => footer_hints_for_logs(state),
        Route::Service => vec![
            "[←→/Tab] 选按钮",
            "[Enter] 执行",
            "[c] 清空",
            "[Esc] 返回侧栏",
        ],
        Route::Backup => vec![
            "[↑↓] 选择",
            "[n] 新建",
            "[r] 还原",
            "[d] 删除",
            "[R] 刷新",
            "[c] 清空",
            "[Esc] 返回侧栏",
        ],
    };

    let mut s = format!(" {} | ", route_label);
    s.push_str(&tips.join(" "));
    s.push_str(" | [q] 退出");
    s
}

fn footer_hints_for_site_form(state: &AppState) -> Vec<&'static str> {
    match state.site_form.focused {
        FormField::SiteName | FormField::Domain | FormField::DomainAliases | FormField::Target => {
            vec!["[Tab] 下一项", "[Shift+Tab] 上一项", "[Esc] 返回"]
        }
        FormField::SiteType => vec!["[↑↓←→] 选择", "[Tab] 切区域", "[Esc] 返回"],
        FormField::EnableCheckbox | FormField::CertCheckbox => {
            vec!["[Space] 切换", "[Tab] 切区域", "[Esc] 返回"]
        }
        FormField::SubmitButton => vec!["[Enter] 创建", "[F2] 创建", "[Esc] 返回"],
        _ => vec!["[←→] 切换", "[Space] 开关", "[Esc] 返回"],
    }
}

fn footer_hints_for_site_edit(state: &AppState) -> Vec<&'static str> {
    match state.site_edit.focused {
        EditFocus::Domain | EditFocus::DomainAliases | EditFocus::Target => vec![
            "[Tab] 下一项",
            "[Shift+Tab] 上一项",
            "[F2] 保存",
            "[F3] 保存测试",
            "[F4] 重置",
            "[F5/F6] 高级/原始",
            "[Esc] 返回",
        ],
        EditFocus::Scheme | EditFocus::StaticMode => vec![
            "[←→] 切换",
            "[Enter] 确认",
            "[F2] 保存",
            "[F3] 保存测试",
            "[F4] 重置",
            "[F5/F6] 高级/原始",
            "[Esc] 返回",
        ],
        _ => vec![
            "[Space] 开关",
            "[Tab] 下一项",
            "[F2] 保存",
            "[F3] 保存测试",
            "[F4] 重置",
            "[F5/F6] 高级/原始",
            "[Esc] 返回",
        ],
    }
}

fn footer_hints_for_site_advanced(state: &AppState) -> Vec<&'static str> {
    let _ = state;
    vec![
        "[←→] 切槽位",
        "[↑↓] 选模板",
        "[Enter] 追加",
        "[F7/F8] 替换/全屏",
        "[F5/F6] 托管/原始",
        "[Esc] 返回",
    ]
}

fn footer_hints_for_site_raw() -> Vec<&'static str> {
    vec![
        "[F2] 保存",
        "[F3] 保存测试",
        "[F5] 托管",
        "[F9/F10] 撤销/重做",
        "[Esc] 返回",
    ]
}

fn footer_hints_for_slot_full() -> Vec<&'static str> {
    vec!["[F2] 完成", "[F4] 清空", "[F9/F10] 撤销/重做", "[Esc] 取消"]
}

fn footer_hints_for_certs(state: &AppState) -> Vec<&'static str> {
    match state.certs.focused {
        CertsFocus::Table => vec![
            "[↑↓] 选择",
            "[Enter] 站点操作",
            "[Tab] 切区域",
            "[r] 刷新",
            "[Esc] 返回侧栏",
        ],
        CertsFocus::SiteActions => vec![
            "[↑] 回站点",
            "[↓] 全局维护",
            "[Enter] 执行",
            "[c] 清空输出",
            "[Esc] 返回侧栏",
        ],
        CertsFocus::GlobalActions => vec![
            "[←→] 选按钮",
            "[↑] 站点操作",
            "[Enter] 执行",
            "[c] 清空输出",
            "[Esc] 返回侧栏",
        ],
    }
}

fn footer_hints_for_logs(state: &AppState) -> Vec<&'static str> {
    match state.logs.focused {
        LogsFocus::SearchInput => vec!["输入关键字", "[Enter] 搜索", "[Esc] 取消"],
        LogsFocus::SiteSelector | LogsFocus::KindSelector => {
            vec!["[←→] 切换", "[Tab] 切区域", "[/] 搜索", "[Esc] 返回侧栏"]
        }
        LogsFocus::LogContent => vec![
            "[↑↓←→] 滚动",
            "[PgUp/PgDn] 翻页",
            "[Home/End] 顶部/底部",
            "[Space] 跟随",
            "[c] 清屏",
            "[/] 搜索",
            "[n/N] 上/下一个",
            "[Esc] 返回侧栏",
        ],
    }
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
