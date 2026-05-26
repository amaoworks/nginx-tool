//! 证书管理视图，对应 design.md 视图 3 / architecture.md §11.4。

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::state::{AppState, CertsAction, CertsFocus};
use crate::domain::cert::{CertLevel, CertWithSite};
use crate::domain::site::Site;
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(
            " 🔐 证书管理  [{} 个站点 / {} 个证书] ",
            state.sites.list.len(),
            state.certs.list.len()
        ),
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
        spans.push(Span::styled(
            "等待首次刷新…",
            Style::default().fg(theme::FG_DIM),
        ));
    }
    if state.certs.raw_output.is_some() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "⚠ certbot 输出未能完整解析",
            Style::default().fg(theme::FG_WARN),
        ));
    }
    let orphan_count = state.certs.list.iter().filter(|item| item.orphan).count();
    let cleanup_count = crate::domain::cert::cleanup_candidates(&state.certs.list).len();
    if orphan_count > 0 || cleanup_count > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("孤立 {} 个 / 可清理多余 {} 个", orphan_count, cleanup_count),
            Style::default().fg(theme::FG_WARN),
        ));
    }
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
    if state.sites.list.is_empty() {
        let body = if state.certs.refreshing {
            "采集中…"
        } else if !state.ctx.deps().certbot {
            "certbot 未安装，但仍可先到「站点管理」创建站点"
        } else if !state.certs.list.is_empty() {
            "未发现站点配置。当前存在证书，但它们未关联到任何站点。"
        } else if !state.ctx.deps().certbot {
            "certbot 未安装，证书表格不可用"
        } else {
            "未发现站点配置。先到「站点管理」创建站点后再申请证书。"
        };
        let p = Paragraph::new(vec![Line::from(""), Line::from(body)])
            .style(Style::default().fg(theme::FG_DIM));
        frame.render_widget(p, area);
        return;
    }

    let header_cells = ["站点", "域名", "证书", "状态"]
        .iter()
        .map(|h| {
            Cell::from(Span::styled(
                *h,
                Style::default().add_modifier(Modifier::BOLD),
            ))
        })
        .collect::<Vec<_>>();
    let header = Row::new(header_cells).height(1);

    let rows = state.sites.list.iter().enumerate().map(|(i, s)| {
        let selected = i == state.certs.site_selector_index;
        let row_style = if selected {
            let style = Style::default()
                .bg(theme::BG_SELECTED)
                .fg(theme::FG_SELECTED);
            if state.certs.focused == CertsFocus::Table {
                style.add_modifier(Modifier::BOLD)
            } else {
                style
            }
        } else {
            Style::default()
        };

        let certs = certs_for_site(state, s);
        let cert_label = certs_label(&certs);
        let status_span = site_status_span(&certs);
        let domains = compact_domains(&s.all_domains);

        Row::new(vec![
            Cell::from(s.name.clone()),
            Cell::from(truncate(&domains, 28)),
            Cell::from(truncate(&cert_label, 20)),
            Cell::from(status_span),
        ])
        .style(row_style)
    });

    let widths = [
        Constraint::Length(12),
        Constraint::Min(20),
        Constraint::Length(18),
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

    let site = state.sites.list.get(state.certs.site_selector_index);
    let detail_lines = match site {
        Some(site) => selected_site_lines(state, site),
        None => vec![
            Line::from(Span::styled(
                "当前没有可操作的站点",
                Style::default().fg(theme::FG_DIM),
            )),
            Line::from(Span::styled(
                "先到「站点管理」创建并启用站点",
                Style::default().fg(theme::FG_DIM),
            )),
        ],
    };
    frame.render_widget(
        Paragraph::new(detail_lines).wrap(Wrap { trim: false }),
        chunks[0],
    );

    // 操作按钮行
    let buttons_focused = state.certs.focused == CertsFocus::ActionButtons;
    let readonly = state.run_mode.is_readonly() || !state.ctx.deps().certbot;

    let cols = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
        Constraint::Percentage(20),
    ])
    .split(chunks[1]);

    for (i, action) in CertsAction::ALL.iter().enumerate() {
        let focused = buttons_focused && state.certs.action_focus == *action;
        let busy = state.certs.running == Some(*action);
        let disabled = readonly
            && matches!(
                action,
                CertsAction::Request
                    | CertsAction::RenewAll
                    | CertsAction::InstallDeployHook
                    | CertsAction::DeleteOrphan
            );
        // 钩子已安装时显示为已就绪，不可点击
        let hook_ok = matches!(action, CertsAction::InstallDeployHook)
            && state
                .certs
                .auto_renew
                .as_ref()
                .is_some_and(|s| s.deploy_hook_present);

        // 没有可安全清理的孤立/冗余证书时，删除按钮不可用
        let no_cleanup_candidates = matches!(action, CertsAction::DeleteOrphan)
            && crate::domain::cert::cleanup_candidates(&state.certs.list).is_empty();

        let label = if busy {
            format!("[ {}（执行中）]", action.label())
        } else if hook_ok {
            "[ ✓ 钩子已就绪 ]".to_string()
        } else if no_cleanup_candidates {
            "[ 无多余证书 ]".to_string()
        } else {
            format!("[ {} ]", action.label())
        };
        let style = if disabled || hook_ok || no_cleanup_candidates {
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

fn certs_for_site<'a>(state: &'a AppState, site: &Site) -> Vec<&'a CertWithSite> {
    let mut certs: Vec<&CertWithSite> = state
        .certs
        .list
        .iter()
        .filter(|item| item.site_names.iter().any(|name| name == &site.name))
        .collect();
    certs.sort_by_key(|item| cert_rank_for_site(item, site));
    certs
}

fn cert_rank_for_site(
    item: &CertWithSite,
    site: &Site,
) -> (u8, usize, std::cmp::Reverse<i64>, String) {
    let primary = site.primary_domain.as_deref();
    let primary_matches = primary.is_some_and(|d| item.cert.primary_domain() == Some(d));
    let contains_primary = primary.is_some_and(|d| item.cert.domains.iter().any(|cd| cd == d));
    let rank = if primary_matches {
        0
    } else if contains_primary {
        1
    } else {
        2
    };
    (
        rank,
        item.cert.domains.len(),
        std::cmp::Reverse(item.cert.days_left.unwrap_or(i64::MIN)),
        item.cert.name.clone(),
    )
}

fn compact_domains(domains: &[String]) -> String {
    if domains.is_empty() {
        "(无 server_name)".to_string()
    } else if domains.len() == 1 {
        domains[0].clone()
    } else {
        format!("{} +{}", domains[0], domains.len() - 1)
    }
}

fn certs_label(certs: &[&CertWithSite]) -> String {
    match certs {
        [] => "未覆盖".to_string(),
        [cert] => cert.cert.name.clone(),
        [first, rest @ ..] => format!("{} +{}", first.cert.name, rest.len()),
    }
}

fn site_status_span<'a>(certs: &[&CertWithSite]) -> Span<'a> {
    let Some(best) = certs.first().copied() else {
        return Span::styled("无证书", Style::default().fg(theme::FG_DIM));
    };
    render_status_span(best)
}

fn selected_site_lines(state: &AppState, site: &Site) -> Vec<Line<'static>> {
    let certs = certs_for_site(state, site);
    let focus_hint = if state.certs.focused == CertsFocus::Table {
        "  [Enter] 进入操作"
    } else {
        ""
    };
    let domains = if site.all_domains.is_empty() {
        "(无 server_name)".to_string()
    } else {
        site.all_domains.join(", ")
    };
    let cert_summary = if certs.is_empty() {
        "证书: 未发现匹配证书".to_string()
    } else {
        let mut parts = Vec::new();
        for (idx, cert) in certs.iter().take(2).enumerate() {
            let days = cert
                .cert
                .days_left
                .map(|days| format!("{days}天"))
                .unwrap_or_else(|| "未知".to_string());
            let overlap = if idx == 0 || cert.cert.domains.len() == 1 {
                ""
            } else {
                "/重叠"
            };
            parts.push(format!("{}({}{})", cert.cert.name, days, overlap));
        }
        if certs.len() > 2 {
            parts.push(format!("+{}", certs.len() - 2));
        }
        format!("证书: {}", parts.join("  "))
    };
    let status_span = site_status_span(&certs);

    vec![
        Line::from(vec![
            Span::styled("当前站点: ", Style::default().fg(theme::FG_DIM)),
            Span::styled(
                site.name.clone(),
                Style::default()
                    .fg(theme::FG_HINT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("   域名: {}{}", domains, focus_hint)),
        ]),
        Line::from(vec![
            Span::raw(cert_summary),
            Span::raw("   状态: "),
            status_span,
        ]),
    ]
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
                "（尚未检查）",
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
            " 操作输出 ",
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
