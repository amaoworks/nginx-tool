//! 站点托管编辑视图

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::state::{AppState, EditFocus};
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let edit = &state.site_edit;
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(" 📁 站点 ▸ {} ▸ 托管编辑 ", edit.site_name),
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
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

    lines.push(field_label("域名:", edit.focused == EditFocus::Domain));
    lines.push(input_field(
        &edit.domain,
        edit.focused == EditFocus::Domain,
        "example.com",
    ));
    if let Some(err) = edit.field_errors.get("domain") {
        lines.push(error_line(err));
    }
    lines.push(Line::from(""));

    lines.push(field_label(
        "附加域名:",
        edit.focused == EditFocus::DomainAliases,
    ));
    lines.push(input_field(
        &edit.domain_aliases,
        edit.focused == EditFocus::DomainAliases,
        "www.example.com, m.example.com（可选）",
    ));
    if let Some(err) = edit.field_errors.get("domain_aliases") {
        lines.push(error_line(err));
    }
    lines.push(Line::from(""));

    if edit.site_type != crate::domain::site::SiteType::Static {
        lines.push(field_label("目标:", edit.focused == EditFocus::Target));
        lines.push(input_field(
            &edit.target,
            edit.focused == EditFocus::Target,
            "127.0.0.1:8080",
        ));
        if let Some(err) = edit.field_errors.get("target") {
            lines.push(error_line(err));
        }
        lines.push(Line::from(""));

        lines.push(field_label("协议:", edit.focused == EditFocus::Scheme));
        lines.push(scheme_line(
            &edit.upstream_scheme,
            edit.focused == EditFocus::Scheme,
        ));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "关键选项:",
        Style::default().fg(theme::FG_NORMAL),
    )));
    match edit.site_type {
        crate::domain::site::SiteType::Proxy => {
            lines.push(toggle_line(
                "流式响应 / AI API",
                edit.feature_streaming,
                edit.focused == EditFocus::ProxyFeatureStreaming,
            ));
            lines.push(toggle_line(
                "WebSocket",
                edit.feature_websocket,
                edit.focused == EditFocus::ProxyFeatureWebsocket,
            ));
            lines.push(toggle_line(
                "大请求体 / 上传",
                edit.feature_large_body,
                edit.focused == EditFocus::ProxyFeatureLargeBody,
            ));
            lines.push(toggle_line(
                "浏览器跨域 CORS",
                edit.feature_cors,
                edit.focused == EditFocus::ProxyFeatureCors,
            ));
            lines.push(toggle_line(
                "长超时后端",
                edit.feature_long_timeout,
                edit.focused == EditFocus::ProxyFeatureLongTimeout,
            ));
        }
        crate::domain::site::SiteType::Static => {
            lines.push(field_label(
                "站点模式:",
                edit.focused == EditFocus::StaticMode,
            ));
            lines.push(static_mode_line(
                edit.feature_spa_mode,
                edit.focused == EditFocus::StaticMode,
            ));
            lines.push(toggle_line(
                "静态资源缓存",
                edit.feature_static_cache,
                edit.focused == EditFocus::StaticFeatureCache,
            ));
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

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
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
