//! 站点托管编辑视图

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::state::{AppState, EditFocus};
use crate::ui::theme;
use crate::ui::views::site_tuning::{self, FocusedLines};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let edit = &state.site_edit;
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(" 📁 站点 ▸ {} ▸ 托管编辑 ", edit.site_name),
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    let mut focused_lines = FocusedLines { start: 0, end: 1 };
    lines.push(Line::from(Span::styled(
        "切换：F5 高级（注入槽）  F6 原始配置",
        Style::default().fg(theme::FG_DIM),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("类型: {}", edit.site_type.label()),
        Style::default().fg(theme::FG_NORMAL),
    )));
    lines.push(Line::from(""));

    let domain_focused = edit.focused == EditFocus::Domain;
    if domain_focused {
        focused_lines = field_focus(lines.len());
    }
    lines.push(field_label("域名:", domain_focused));
    lines.push(input_field(&edit.domain, domain_focused, "example.com"));
    if let Some(err) = edit.field_errors.get("domain") {
        lines.push(error_line(err));
    }
    lines.push(Line::from(""));

    let aliases_focused = edit.focused == EditFocus::DomainAliases;
    if aliases_focused {
        focused_lines = field_focus(lines.len());
    }
    lines.push(field_label("附加域名:", aliases_focused));
    lines.push(input_field(
        &edit.domain_aliases,
        aliases_focused,
        "www.example.com, m.example.com（可选）",
    ));
    if let Some(err) = edit.field_errors.get("domain_aliases") {
        lines.push(error_line(err));
    }
    lines.push(Line::from(""));

    if edit.site_type != crate::domain::site::SiteType::Static {
        let target_focused = edit.focused == EditFocus::Target;
        if target_focused {
            focused_lines = field_focus(lines.len());
        }
        lines.push(field_label("目标:", target_focused));
        lines.push(input_field(&edit.target, target_focused, "127.0.0.1:8080"));
        if let Some(err) = edit.field_errors.get("target") {
            lines.push(error_line(err));
        }
        lines.push(Line::from(""));

        let scheme_focused = edit.focused == EditFocus::Scheme;
        if scheme_focused {
            focused_lines = field_focus(lines.len());
        }
        lines.push(field_label("协议:", scheme_focused));
        lines.push(scheme_line(&edit.upstream_scheme, scheme_focused));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "关键选项:",
        Style::default().fg(theme::FG_NORMAL),
    )));
    match edit.site_type {
        crate::domain::site::SiteType::Proxy => {
            if edit.focused == EditFocus::ProxyFeatureStreaming {
                focused_lines = one_line_focus(lines.len());
            }
            lines.push(toggle_line(
                "流式响应 / AI API",
                edit.feature_streaming,
                edit.focused == EditFocus::ProxyFeatureStreaming,
            ));
            if edit.focused == EditFocus::ProxyFeatureWebsocket {
                focused_lines = one_line_focus(lines.len());
            }
            lines.push(toggle_line(
                "WebSocket",
                edit.feature_websocket,
                edit.focused == EditFocus::ProxyFeatureWebsocket,
            ));
            if edit.focused == EditFocus::ProxyFeatureCors {
                focused_lines = one_line_focus(lines.len());
            }
            lines.push(toggle_line(
                "浏览器跨域 CORS",
                edit.feature_cors,
                edit.focused == EditFocus::ProxyFeatureCors,
            ));
        }
        crate::domain::site::SiteType::Static => {
            if edit.focused == EditFocus::StaticMode {
                focused_lines = field_focus(lines.len());
            }
            lines.push(field_label(
                "站点模式:",
                edit.focused == EditFocus::StaticMode,
            ));
            lines.push(static_mode_line(
                edit.feature_spa_mode,
                edit.focused == EditFocus::StaticMode,
            ));
            if edit.focused == EditFocus::StaticFeatureCache {
                focused_lines = one_line_focus(lines.len());
            }
            lines.push(toggle_line(
                "静态资源缓存",
                edit.feature_static_cache,
                edit.focused == EditFocus::StaticFeatureCache,
            ));
            if edit.focused == EditFocus::StaticFeatureBlockSensitive {
                focused_lines = one_line_focus(lines.len());
            }
            lines.push(toggle_line(
                "敏感路径保护",
                edit.feature_block_sensitive,
                edit.focused == EditFocus::StaticFeatureBlockSensitive,
            ));
        }
        crate::domain::site::SiteType::Emby => {
            lines.push(Line::from(Span::styled(
                "  Emby/Jellyfin 继续使用内置优化代理预设",
                Style::default().fg(theme::FG_DIM),
            )));
        }
        crate::domain::site::SiteType::Unknown => {
            lines.push(Line::from(Span::styled(
                "  未识别站点类型，建议切到原始模式确认",
                Style::default().fg(theme::FG_WARN),
            )));
        }
    }

    lines.push(Line::from(""));
    let tuning_focus = match edit.focused {
        EditFocus::Tuning(field) => Some(field),
        _ => None,
    };
    if let Some(range) = site_tuning::append_tuning_fields(
        &mut lines,
        &edit.tuning,
        matches!(
            edit.site_type,
            crate::domain::site::SiteType::Proxy | crate::domain::site::SiteType::Emby
        ),
        state.ctx.nginx.http2_support(),
        edit.ssl_enabled,
        tuning_focus,
        &edit.field_errors,
    ) {
        focused_lines = range;
    }

    lines.push(Line::from(Span::styled(
        "高级模式用于维护注入槽和模板片段；更深的定制仍建议直接编辑配置。",
        Style::default().fg(theme::FG_DIM),
    )));

    if edit.dirty {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " ⚠ 有未保存的修改",
            Style::default().fg(theme::FG_WARN),
        )));
    }

    let scroll_offset =
        site_tuning::scroll_offset(&lines, inner.width, inner.height, focused_lines);
    let p = Paragraph::new(site_tuning::wrap_lines(&lines, inner.width)).scroll((scroll_offset, 0));
    frame.render_widget(p, inner);
}

fn one_line_focus(line: usize) -> FocusedLines {
    FocusedLines {
        start: line,
        end: line + 1,
    }
}

fn field_focus(line: usize) -> FocusedLines {
    FocusedLines {
        start: line,
        end: line + 2,
    }
}

fn field_label(label: &str, focused: bool) -> Line<'static> {
    let style = if focused {
        Style::default().fg(theme::FG_HINT)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    Line::from(Span::styled(label.to_string(), style))
}

fn input_field(value: &str, focused: bool, placeholder: &str) -> Line<'static> {
    let display = if value.is_empty() && !focused {
        placeholder.to_string()
    } else {
        let mut s = value.to_string();
        if focused {
            s.push('▏');
        }
        s
    };

    let style = if focused {
        Style::default()
            .fg(theme::FG_NORMAL)
            .add_modifier(Modifier::UNDERLINED)
    } else if value.is_empty() {
        Style::default().fg(theme::FG_DIM)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };

    let bracket_style = if focused {
        Style::default().fg(theme::FG_HINT)
    } else {
        Style::default().fg(theme::BORDER)
    };

    Line::from(vec![
        Span::styled("[", bracket_style),
        Span::styled(display, style),
        Span::styled("]", bracket_style),
    ])
}

fn scheme_line(current: &str, focused: bool) -> Line<'static> {
    let http_selected = current == "http";
    let selected_style = if focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_HINT)
    };
    let normal_style = Style::default().fg(theme::FG_NORMAL);

    Line::from(vec![
        Span::styled(
            if http_selected {
                "◉ http"
            } else {
                "○ http"
            },
            if http_selected {
                selected_style
            } else {
                normal_style
            },
        ),
        Span::raw("   "),
        Span::styled(
            if !http_selected {
                "◉ https"
            } else {
                "○ https"
            },
            if !http_selected {
                selected_style
            } else {
                normal_style
            },
        ),
    ])
}

fn static_mode_line(spa_mode: bool, focused: bool) -> Line<'static> {
    let selected_style = if focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_HINT)
    };
    let normal_style = Style::default().fg(theme::FG_NORMAL);

    Line::from(vec![
        Span::styled(
            if !spa_mode {
                "◉ 普通静态"
            } else {
                "○ 普通静态"
            },
            if !spa_mode {
                selected_style
            } else {
                normal_style
            },
        ),
        Span::raw("   "),
        Span::styled(
            if spa_mode {
                "◉ SPA 单页"
            } else {
                "○ SPA 单页"
            },
            if spa_mode {
                selected_style
            } else {
                normal_style
            },
        ),
    ])
}

fn toggle_line(label: &str, enabled: bool, focused: bool) -> Line<'static> {
    let marker = if enabled { "[✕]" } else { "[ ]" };
    let style = if focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else if enabled {
        Style::default().fg(theme::FG_HINT)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    Line::from(Span::styled(format!("  {} {}", marker, label), style))
}

fn error_line(message: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  ⚠ {}", message),
        Style::default().fg(theme::FG_ERR),
    ))
}
