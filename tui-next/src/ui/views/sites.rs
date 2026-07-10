use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::state::{AppState, FocusArea, SitesSortField};
use crate::domain::site::{SiteType, SslLevel, SslStatus};
use crate::ui::focus;
use crate::ui::theme;

pub fn render_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(
            " 📁 站点 ▸ 列表  [{} 个站点] ",
            state.sites.list.len()
        ),
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = ratatui::layout::Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .split(inner);
    let table_area = chunks[0];
    let detail_area = chunks[1];
    let hint_area = chunks[2];

    if let Some(err) = &state.sites.last_error {
        let p = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("⚠ 加载失败：{}", err),
                Style::default().fg(theme::FG_ERR),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "[r] 重试加载",
                Style::default().fg(theme::FG_DIM),
            )),
        ]);
        frame.render_widget(p, table_area);
        return;
    }

    if state.sites.list.is_empty() {
        let body = if state.sites.refreshing {
            "加载中…"
        } else {
            "未发现站点配置（/etc/nginx/sites-available 为空或无 .conf 文件）"
        };
        let p = Paragraph::new(vec![Line::from(""), Line::from(body)])
            .style(Style::default().fg(theme::FG_DIM));
        frame.render_widget(p, table_area);
    } else {
        render_table(frame, table_area, state);
    }

    render_detail(frame, detail_area, state);
    render_meta(frame, hint_area, state);
}

fn render_table(frame: &mut Frame, area: Rect, state: &AppState) {
    let header_cells = [
        sortable_header("状态", SitesSortField::Status, state),
        sortable_header("名称", SitesSortField::Name, state),
        Cell::from(Span::styled(
            "域名 → 目标",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        sortable_header("类型", SitesSortField::Type, state),
        sortable_header("SSL", SitesSortField::Ssl, state),
    ];
    let header = Row::new(header_cells).height(1).bottom_margin(0);

    let rows = state.sites.list.iter().enumerate().map(|(i, s)| {
        let selected = i == state.sites.selected;
        let table_focused = state.focus == FocusArea::Content;
        let status_span = if s.enabled {
            Span::styled("● 启用", Style::default().fg(theme::FG_OK))
        } else {
            Span::styled("○ 停用", Style::default().fg(theme::FG_DIM))
        };

        // 优化域名显示：主域名 + 附加域名数量提示
        let domains = if s.all_domains.is_empty() {
            "(无 server_name)".to_string()
        } else if s.all_domains.len() == 1 {
            s.all_domains[0].clone()
        } else {
            format!("{} +{}", s.all_domains[0], s.all_domains.len() - 1)
        };
        let domain_target = format!(
            "{} → {}",
            domains,
            s.target.as_deref().unwrap_or("(未解析)")
        );

        let type_label = match s.site_type {
            SiteType::Proxy => "代理",
            SiteType::Emby => "Emby",
            SiteType::Static => "静态",
            SiteType::Unknown => "未知",
        };

        let ssl_span = render_ssl_span(&s.ssl);

        let row_style = if selected && table_focused {
            Style::default()
                .bg(theme::BG_SELECTED)
                .fg(theme::FG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            focus::selected_text_style(false)
        } else {
            Style::default()
        };
        let name = if selected {
            format!("▶ {}", s.name)
        } else {
            format!("  {}", s.name)
        };

        // 动态计算可用宽度：总宽度 - 固定列宽度 - 边距
        // 固定列：状态(8) + 名称(16) + 类型(8) + SSL(12) = 44
        // 预留边距和分隔符约 10，域名→目标列至少保留 50 字符
        let available_width = area.width.saturating_sub(54).max(50) as usize;

        Row::new(vec![
            Cell::from(status_span),
            Cell::from(name),
            Cell::from(truncate(domain_target, available_width)),
            Cell::from(type_label.to_string()),
            Cell::from(ssl_span),
        ])
        .style(row_style)
    });

    let widths = [
        Constraint::Length(8),
        Constraint::Length(16),
        Constraint::Min(50),
        Constraint::Length(8),
        Constraint::Length(12),
    ];
    let table = Table::new(rows, widths).header(header);
    frame.render_widget(table, area);
}

fn render_ssl_span<'a>(ssl: &SslStatus) -> Span<'a> {
    match ssl {
        SslStatus::None => Span::styled("✗ 无", Style::default().fg(theme::FG_DIM)),
        SslStatus::Active { days_left } => {
            let level = ssl.level();
            let style = match level {
                SslLevel::Critical => Style::default().fg(theme::FG_ERR),
                SslLevel::Warning => Style::default().fg(theme::FG_WARN),
                SslLevel::Ok => Style::default().fg(theme::FG_OK),
                SslLevel::None => Style::default().fg(theme::FG_DIM),
            };
            let glyph = match level {
                SslLevel::Critical => "🔴",
                SslLevel::Warning => "⚠",
                SslLevel::Ok => "✓",
                SslLevel::None => "✗",
            };
            Span::styled(format!("{} {}天", glyph, days_left), style)
        }
    }
}

fn sortable_header<'a>(label: &'static str, field: SitesSortField, state: &AppState) -> Cell<'a> {
    let active = state.sites.sort_by == field;
    let text = if active {
        format!("{}{}", label, state.sites.sort_order.glyph())
    } else {
        label.to_string()
    };
    let style = if active {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    Cell::from(Span::styled(text, style))
}

fn render_detail(frame: &mut Frame, area: Rect, state: &AppState) {
    let Some(s) = state.sites.current() else {
        return;
    };
    let busy = state
        .sites
        .action_in_flight
        .as_deref()
        .map(|n| n == s.name)
        .unwrap_or(false);
    let busy_marker = if busy { "（执行中）" } else { "" };
    let line = Line::from(vec![
        Span::styled(
            format!(" 选中: {} ", s.name),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled("▏ ", Style::default().fg(theme::FG_DIM)),
        Span::raw(format!("类型={} ", s.site_type.label())),
        Span::styled("▏ ", Style::default().fg(theme::FG_DIM)),
        Span::raw(format!(
            "域名={} ",
            if s.all_domains.is_empty() {
                "(无 server_name)".to_string()
            } else if s.all_domains.len() == 1 {
                s.all_domains[0].clone()
            } else {
                format!("{} +{}", s.all_domains[0], s.all_domains.len() - 1)
            }
        )),
        Span::styled("▏ ", Style::default().fg(theme::FG_DIM)),
        Span::raw(format!(
            "目标={} ",
            s.target.as_deref().unwrap_or("(未解析)")
        )),
        Span::styled("▏ ", Style::default().fg(theme::FG_DIM)),
        Span::raw(format!("SSL={}", ssl_compact(&s.ssl))),
        Span::styled(busy_marker, Style::default().fg(theme::FG_HINT)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn ssl_compact(ssl: &SslStatus) -> String {
    match ssl {
        SslStatus::None => "无".to_string(),
        SslStatus::Active { days_left } => format!("{}天", days_left),
    }
}

fn render_meta(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut tips = Vec::new();
    if state.run_mode.is_readonly() {
        tips.push(Span::styled(
            "[只读模式] 写操作不可用",
            Style::default().fg(theme::FG_WARN),
        ));
        tips.push(Span::raw("  "));
    }
    if state.sites.refreshing {
        tips.push(Span::styled("刷新中…", Style::default().fg(theme::FG_PATH)));
        tips.push(Span::raw("  "));
    }
    if let Some(t) = state.sites.last_refresh {
        tips.push(Span::styled(
            format!("最近刷新 {}s 前", t.elapsed().as_secs()),
            Style::default().fg(theme::FG_DIM),
        ));
        tips.push(Span::raw("  "));
    }
    tips.push(Span::styled(
        format!(
            "排序: {}{}",
            state.sites.sort_by.label(),
            state.sites.sort_order.glyph()
        ),
        Style::default().fg(theme::FG_DIM),
    ));
    let p = Paragraph::new(vec![Line::from(""), Line::from(tips)]).alignment(Alignment::Left);
    frame.render_widget(p, area);
}

fn truncate(s: String, max_chars: usize) -> String {
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
        w += cw;
    }
    let _ = out.as_str().width();
    out
}

// 兼容旧入口（其他视图用 super 调用此模块的旧 API 时不破坏）。
// 当前对 sites 视图来说 `render_list` 是唯一公开入口。
#[allow(dead_code)]
pub fn _placeholder() {}
