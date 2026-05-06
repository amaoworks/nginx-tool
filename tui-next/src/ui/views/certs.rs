//! 证书管理视图，对应 design.md 视图 3 / architecture.md §11.4。

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::state::{AppState, CertsAction, CertsFocus};
use crate::domain::cert::{CertLevel, CertWithSite};
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(" 🔐 证书管理  [{} 个证书] ", state.certs.list.len()),
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Length(1), // 状态行
        Constraint::Min(6),    // 证书表格
        Constraint::Length(4), // 站点选择 + 操作按钮
        Constraint::Length(5), // 自动续签状态
        Constraint::Min(3),    // 操作输出
    ])
    .split(inner);

    render_status_line(frame, chunks[0], state);
    render_table(frame, chunks[1], state);
    render_actions(frame, chunks[2], state);
    render_auto_renew(frame, chunks[3], state);
    render_output(frame, chunks[4], state);
}

fn render_status_line(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut spans: Vec<Span> = Vec::new();
    if !state.ctx.deps().certbot {
        spans.push(Span::styled(
            "⚠ certbot 未安装，证书操作不可用",
            Style::default().fg(theme::FG_WARN),
        ));
    } else if state.certs.refreshing {
        spans.push(Span::styled("采集中…", Style::default().fg(theme::FG_HINT)));
    } else if let Some(t) = state.certs.last_refresh {
        spans.push(Span::styled(
            format!("最近刷新 {}s 前", t.elapsed().as_secs()),
            Style::default().fg(theme::FG_DIM),
        ));
    } else {
        spans.push(Span::styled("[r] 刷新", Style::default().fg(theme::FG_DIM)));
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        "[Tab] 切换区域  [Enter] 执行  [c] 清空输出  [Esc] 返回侧栏",
        Style::default().fg(theme::FG_DIM),
    ));
    if let Some(err) = &state.certs.last_error {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("⚠ {}", err),
            Style::default().fg(theme::FG_WARN),
        ));
    }
    let p = Paragraph::new(Line::from(spans));
    frame.render_widget(p, area);
}

fn render_table(frame: &mut Frame, area: Rect, state: &AppState) {
    if let Some(raw) = &state.certs.raw_output {
        let lines: Vec<Line> = std::iter::once(Line::from(Span::styled(
            "⚠ 证书解析失败，展示 certbot 原始输出：",
            Style::default().fg(theme::FG_WARN),
        )))
        .chain(raw.lines().take(area.height as usize - 1).map(|l| {
            Line::from(Span::styled(
                truncate(l, area.width as usize),
                Style::default().fg(theme::FG_DIM),
            ))
        }))
        .collect();
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        return;
    }
    if state.certs.list.is_empty() {
        let body = if state.certs.refreshing {
            "采集中…"
        } else if !state.ctx.deps().certbot {
            "certbot 未安装，证书表格不可用"
        } else {
            "未发现证书。可以通过下方按钮为已有站点申请证书。"
        };
        let p = Paragraph::new(vec![Line::from(""), Line::from(body)])
            .style(Style::default().fg(theme::FG_DIM));
        frame.render_widget(p, area);
        return;
    }

    let focused = state.certs.focused == CertsFocus::Table;
    let header_cells = ["站点", "主域名", "到期", "状态"]
        .iter()
        .map(|h| {
            Cell::from(Span::styled(
                *h,
                Style::default().add_modifier(Modifier::BOLD),
            ))
        })
        .collect::<Vec<_>>();
    let header = Row::new(header_cells).height(1);

    let rows = state.certs.list.iter().enumerate().map(|(i, c)| {
        let selected = focused && i == state.certs.selected;
        let row_style = if selected {
            Style::default()
                .bg(theme::BG_SELECTED)
                .fg(theme::FG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let site_label = if c.orphan {
            Span::styled(
                "(孤立证书)".to_string(),
                Style::default().fg(theme::FG_WARN),
            )
        } else {
            Span::raw(c.site_names.join(","))
        };
        let primary = c.cert.primary_domain().unwrap_or("(无)").to_string();
        let expiry = c
            .cert
            .expiry
            .map(|t| t.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "—".into());
        let status_span = render_status_span(c);

        Row::new(vec![
            Cell::from(site_label),
            Cell::from(truncate(&primary, 28)),
            Cell::from(expiry),
            Cell::from(status_span),
        ])
        .style(row_style)
    });

    let widths = [
        Constraint::Length(12),
        Constraint::Min(20),
        Constraint::Length(12),
        Constraint::Length(14),
    ];
    let table = Table::new(rows, widths).header(header);
    frame.render_widget(table, area);
}

fn render_status_span<'a>(c: &CertWithSite) -> Span<'a> {
    match (c.cert.level, c.cert.days_left) {
        (Some(level), Some(days)) => {
            let style = match level {
                CertLevel::Ok => Style::default().fg(theme::FG_OK),
                CertLevel::Warning => Style::default().fg(theme::FG_WARN),
                CertLevel::Critical => Style::default().fg(theme::FG_ERR),
                CertLevel::Expired => Style::default().fg(theme::FG_ERR),
            };
            Span::styled(format!("{} {} 天", level.glyph(), days), style)
        }
        _ => Span::styled("? 未知", Style::default().fg(theme::FG_DIM)),
    }
}

fn render_actions(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::vertical([Constraint::Length(2), Constraint::Length(2)]).split(area);

    // 站点选择器
    let selector_focused = state.certs.focused == CertsFocus::SiteSelector;
    let site = state.sites.list.get(state.certs.site_selector_index);
    let label = match site {
        Some(s) => format!(
            "{}{} ▼   解析域名: {}",
            if selector_focused { "▸" } else { " " },
            s.name,
            s.all_domains.join(", "),
        ),
        None => "(暂无站点；先到「站点管理」创建)".to_string(),
    };
    let style = if selector_focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("选中站点: ["),
            Span::styled(label, style),
            Span::raw("]"),
        ])),
        chunks[0],
    );

    // 操作按钮行
    let buttons_focused = state.certs.focused == CertsFocus::ActionButtons;
    let readonly = state.run_mode.is_readonly() || !state.ctx.deps().certbot;

    let cols = Layout::horizontal([
        Constraint::Percentage(33),
        Constraint::Percentage(33),
        Constraint::Percentage(34),
    ])
    .split(chunks[1]);

    for (i, action) in CertsAction::ALL.iter().enumerate() {
        let focused = buttons_focused && state.certs.action_focus == *action;
        let busy = state.certs.running == Some(*action);
        let disabled = readonly && matches!(action, CertsAction::Request | CertsAction::RenewAll);

        let label = if busy {
            format!("[ {}（执行中）]", action.label())
        } else {
            format!("[ {} ]", action.label())
        };
        let style = if disabled {
            Style::default()
                .fg(theme::FG_DIM)
                .add_modifier(Modifier::DIM)
        } else if focused {
            Style::default()
                .bg(theme::BG_SELECTED)
                .fg(theme::FG_SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::FG_NORMAL)
        };
        let surrounded = if focused {
            format!("[{}]", label)
        } else {
            label
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(surrounded, style))),
            cols[i],
        );
    }
}

fn render_auto_renew(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(
            " 自动续签状态 ",
            Style::default().fg(theme::FG_PATH),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    match &state.certs.auto_renew {
        Some(s) => {
            let timer_glyph = if s.timer_active { "●" } else { "○" };
            let timer_style = if s.timer_active {
                Style::default().fg(theme::FG_OK)
            } else {
                Style::default().fg(theme::FG_WARN)
            };
            let mut timer_line = vec![
                Span::styled(format!("{} ", timer_glyph), timer_style),
                Span::raw(format!("{}    ", s.timer_unit)),
                Span::styled(
                    if s.timer_active {
                        "已启用"
                    } else {
                        "未启用"
                    },
                    timer_style,
                ),
            ];
            if let Some(next) = &s.next_run {
                timer_line.push(Span::styled(
                    format!("    下次执行：{}", next),
                    Style::default().fg(theme::FG_DIM),
                ));
            }
            lines.push(Line::from(timer_line));

            let hook_glyph = if s.deploy_hook_present { "✓" } else { "✗" };
            let hook_style = if s.deploy_hook_present {
                Style::default().fg(theme::FG_OK)
            } else {
                Style::default().fg(theme::FG_WARN)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", hook_glyph), hook_style),
                Span::raw("deploy hook   "),
                Span::styled(
                    s.deploy_hook_path.clone(),
                    Style::default().fg(theme::FG_DIM),
                ),
            ]));
            for tip in s.advice() {
                lines.push(Line::from(Span::styled(
                    format!("  • {}", tip),
                    Style::default().fg(theme::FG_HINT),
                )));
            }
        }
        None => {
            lines.push(Line::from(Span::styled(
                "（尚未检查；按 [检查自动续签] 或 [r] 触发刷新）",
                Style::default().fg(theme::FG_DIM),
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_output(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(
            " 操作输出（[c] 清空） ",
            Style::default().fg(theme::FG_PATH),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.certs.output.is_empty() {
        let p = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "（暂无输出）",
                Style::default().fg(theme::FG_DIM),
            )),
        ]);
        frame.render_widget(p, inner);
        return;
    }
    let visible = inner.height as usize;
    let total = state.certs.output.len();
    let skip = total.saturating_sub(visible);
    let lines: Vec<Line> = state
        .certs
        .output
        .iter()
        .skip(skip)
        .map(|s| Line::from(s.clone()))
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn truncate(s: &str, max_chars: usize) -> String {
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
