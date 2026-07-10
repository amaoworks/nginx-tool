//! 证书管理视图：站点表 + 全局维护扁平布局。

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::state::{AppState, CertsAction, CertsFocus, FocusArea};
use crate::domain::cert::{CertLevel, CertWithSite};
use crate::domain::site::Site;
use crate::ui::focus;
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(
            " 🔐 证书  [{} 个站点 / {} 个证书] ",
            state.sites.list.len(),
            state.certs.list.len()
        ),
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // 状态(含自动续签) | 表格 | 当前站点摘要 | 全局维护 | 输出
    let chunks = Layout::vertical([
        Constraint::Length(2), // 状态 + 自动续签摘要
        Constraint::Min(6),    // 站点证书表
        Constraint::Length(1), // 当前站点一行
        Constraint::Length(2), // 全局维护按钮
        Constraint::Min(3),    // 操作输出
    ])
    .split(inner);

    render_status_block(frame, chunks[0], state);
    render_table(frame, chunks[1], state);
    render_site_summary(frame, chunks[2], state);
    render_global_actions(frame, chunks[3], state);
    render_output(frame, chunks[4], state);
}

fn render_status_block(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut line1: Vec<Span> = Vec::new();
    if !state.ctx.deps().certbot {
        line1.push(Span::styled(
            "⚠ certbot 未安装，证书操作不可用",
            Style::default().fg(theme::FG_WARN),
        ));
    } else if state.certs.refreshing {
        line1.push(Span::styled("采集中…", Style::default().fg(theme::FG_HINT)));
    } else if let Some(t) = state.certs.last_refresh {
        line1.push(Span::styled(
            format!("最近刷新 {}s 前", t.elapsed().as_secs()),
            Style::default().fg(theme::FG_DIM),
        ));
    } else {
        line1.push(Span::styled(
            "等待首次刷新…",
            Style::default().fg(theme::FG_DIM),
        ));
    }

    let orphan_count = state.certs.list.iter().filter(|item| item.orphan).count();
    let cleanup_count = crate::domain::cert::cleanup_candidates(&state.certs.list).len();
    if orphan_count > 0 || cleanup_count > 0 {
        line1.push(Span::raw("  ·  "));
        line1.push(Span::styled(
            format!("孤立 {} / 可清理 {}", orphan_count, cleanup_count),
            Style::default().fg(theme::FG_WARN),
        ));
    }
    if state.certs.raw_output.is_some() {
        line1.push(Span::raw("  ·  "));
        line1.push(Span::styled(
            "⚠ 输出未能完整解析",
            Style::default().fg(theme::FG_WARN),
        ));
    }
    if let Some(err) = &state.certs.last_error {
        line1.push(Span::raw("  ·  "));
        line1.push(Span::styled(
            format!("⚠ {}", truncate(err, 40)),
            Style::default().fg(theme::FG_WARN),
        ));
    }

    // 第二行：自动续签状态（只读摘要，不占独立框）
    let line2 = auto_renew_summary_spans(state);

    frame.render_widget(
        Paragraph::new(vec![Line::from(line1), Line::from(line2)]).wrap(Wrap { trim: false }),
        area,
    );
}

fn auto_renew_summary_spans(state: &AppState) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        "自动续签  ",
        Style::default().fg(theme::FG_DIM),
    )];
    match &state.certs.auto_renew {
        Some(s) => {
            let (glyph, label, style) = if s.timer_active {
                ("●", "已启用", Style::default().fg(theme::FG_OK))
            } else {
                ("○", "未启用", Style::default().fg(theme::FG_WARN))
            };
            spans.push(Span::styled(format!("{} {}", glyph, label), style));
            if let Some(next) = &s.next_run {
                spans.push(Span::styled(
                    format!(" · 下次 {}", next),
                    Style::default().fg(theme::FG_DIM),
                ));
            }
            if s.deploy_hook_present {
                spans.push(Span::styled(
                    " · ✓ 重载钩子",
                    Style::default().fg(theme::FG_OK),
                ));
            } else {
                spans.push(Span::styled(
                    " · ✗ 重载钩子缺失",
                    Style::default().fg(theme::FG_WARN),
                ));
            }
        }
        None => {
            spans.push(Span::styled(
                "（尚未检查）",
                Style::default().fg(theme::FG_DIM),
            ));
        }
    }
    spans
}

fn render_table(frame: &mut Frame, area: Rect, state: &AppState) {
    let table_focused =
        state.focus == FocusArea::Content && state.certs.focused == CertsFocus::Table;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(focus::panel_border_style(table_focused))
        .title(Span::styled(
            " 站点证书 ",
            Style::default().fg(theme::FG_PATH),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.sites.list.is_empty() {
        let body = if state.certs.refreshing {
            "采集中…"
        } else if !state.ctx.deps().certbot {
            "certbot 未安装，但仍可先到「站点」页创建站点"
        } else if !state.certs.list.is_empty() {
            "未发现站点配置。当前存在证书，但它们未关联到任何站点。"
        } else {
            "未发现站点配置。先到「站点」页创建站点后再申请证书。"
        };
        let p = Paragraph::new(vec![Line::from(""), Line::from(body)])
            .style(Style::default().fg(theme::FG_DIM));
        frame.render_widget(p, inner);
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
        let site_name = if selected {
            format!("▶ {}", s.name)
        } else {
            format!("  {}", s.name)
        };

        let certs = certs_for_site(state, s);
        let cert_label = certs_label(&certs);
        let status_span = site_status_span(&certs);
        let domains = compact_domains(&s.all_domains);

        Row::new(vec![
            Cell::from(site_name),
            Cell::from(truncate(&domains, 28)),
            Cell::from(truncate(&cert_label, 20)),
            Cell::from(status_span),
        ])
        .style(row_style)
    });

    let widths = [
        Constraint::Length(14),
        Constraint::Min(20),
        Constraint::Length(18),
        Constraint::Length(14),
    ];
    let table = Table::new(rows, widths).header(header);
    frame.render_widget(table, inner);
}

fn render_site_summary(frame: &mut Frame, area: Rect, state: &AppState) {
    let Some(site) = state.sites.list.get(state.certs.site_selector_index) else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "当前: （无站点）",
                Style::default().fg(theme::FG_DIM),
            ))),
            area,
        );
        return;
    };

    let certs = certs_for_site(state, site);
    let domains = if site.all_domains.is_empty() {
        "(无 server_name)".to_string()
    } else {
        truncate(&site.all_domains.join(", "), 42)
    };
    let cert_part = if certs.is_empty() {
        "无证书".to_string()
    } else {
        certs_label(&certs)
    };
    let status = site_status_span(&certs);
    let hint = if state.certs.focused == CertsFocus::Table
        && state.focus == FocusArea::Content
        && state.certs.running.is_none()
    {
        "  [Enter] 申请证书"
    } else {
        ""
    };

    let line = Line::from(vec![
        Span::styled("当前: ", Style::default().fg(theme::FG_DIM)),
        Span::styled(
            site.name.clone(),
            Style::default()
                .fg(theme::FG_HINT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" · {} · {} · ", domains, cert_part)),
        status,
        Span::styled(hint, Style::default().fg(theme::FG_DIM)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_global_actions(frame: &mut Frame, area: Rect, state: &AppState) {
    let focused_group = state.focus == FocusArea::Content
        && state.certs.focused == CertsFocus::GlobalActions;
    let readonly = state.run_mode.is_readonly() || !state.ctx.deps().certbot;

    let title_style = if focused_group {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_DIM)
    };

    let cols = Layout::horizontal(
        std::iter::once(Constraint::Length(12))
            .chain(
                CertsAction::GLOBAL_ACTIONS
                    .iter()
                    .map(|_| Constraint::Percentage(22)),
            )
            .collect::<Vec<_>>(),
    )
    .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("全局维护", title_style))),
        cols[0],
    );

    for (i, action) in CertsAction::GLOBAL_ACTIONS.iter().enumerate() {
        let focused = focused_group && state.certs.action_focus == *action;
        let busy = state.certs.running == Some(*action);
        let disabled = readonly
            && matches!(
                action,
                CertsAction::RenewAll
                    | CertsAction::InstallDeployHook
                    | CertsAction::DeleteOrphan
            );
        let hook_ok = matches!(action, CertsAction::InstallDeployHook)
            && state
                .certs
                .auto_renew
                .as_ref()
                .is_some_and(|s| s.deploy_hook_present);
        let no_cleanup_candidates = matches!(action, CertsAction::DeleteOrphan)
            && crate::domain::cert::cleanup_candidates(&state.certs.list).is_empty();

        let label = if busy {
            format!("{}（执行中）", action.label())
        } else if hook_ok {
            "✓ 钩子已就绪".to_string()
        } else if no_cleanup_candidates {
            "无多余".to_string()
        } else {
            action.label().to_string()
        };
        let style = if disabled || hook_ok || no_cleanup_candidates {
            Style::default()
                .fg(theme::FG_DIM)
                .add_modifier(Modifier::DIM)
        } else if focused {
            focus::focused_button_style()
        } else {
            Style::default().fg(theme::FG_NORMAL)
        };
        let surrounded = if focused {
            format!("▶ {} ◀", label)
        } else {
            format!("[ {} ]", label)
        };
        // 垂直居中一点：第二行有内容时第一行空
        let lines = if area.height >= 2 {
            vec![Line::from(""), Line::from(Span::styled(surrounded, style))]
        } else {
            vec![Line::from(Span::styled(surrounded, style))]
        };
        frame.render_widget(Paragraph::new(lines), cols[i + 1]);
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
    match (best.cert.level, best.cert.days_left) {
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

fn render_output(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
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
