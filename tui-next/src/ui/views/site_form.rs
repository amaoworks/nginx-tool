//! 新建站点表单渲染，对应 design.md 子模式 B

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::state::{AppState, FormField, SiteTypeChoice};
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        " 📁 站点管理 ▸ 新建站点 ",
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let form = &state.site_form;
    let mut lines: Vec<Line> = Vec::new();
    let mut focused_line = 0usize;

    // 站点名称
    let name_focused = form.focused == FormField::SiteName;
    if name_focused {
        focused_line = lines.len();
    }
    let name_error = form.get_error("site_name");
    lines.push(field_label("站点名称:", name_focused));
    lines.push(input_field(&form.site_name, name_focused, "my-site"));
    if let Some(err) = name_error {
        lines.push(Line::from(Span::styled(
            format!("  ⚠ {}", err),
            Style::default().fg(theme::FG_ERR),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  仅允许字母、数字、连字符",
            Style::default().fg(theme::FG_DIM),
        )));
    }
    lines.push(Line::from(""));

    // 域名
    let domain_focused = form.focused == FormField::Domain;
    if domain_focused {
        focused_line = lines.len();
    }
    let domain_error = form.get_error("domain");
    lines.push(field_label("域    名:", domain_focused));
    lines.push(input_field(&form.domain, domain_focused, "app.example.com"));
    if let Some(err) = domain_error {
        lines.push(Line::from(Span::styled(
            format!("  ⚠ {}", err),
            Style::default().fg(theme::FG_ERR),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  如: app.example.com（允许 *.example.com）",
            Style::default().fg(theme::FG_DIM),
        )));
    }
    lines.push(Line::from(""));

    // 附加域名
    let aliases_focused = form.focused == FormField::DomainAliases;
    if aliases_focused {
        focused_line = lines.len();
    }
    let aliases_error = form.get_error("domain_aliases");
    lines.push(field_label("附加域名:", aliases_focused));
    lines.push(input_field(
        &form.domain_aliases,
        aliases_focused,
        "www.example.com, m.example.com（可选，逗号分隔）",
    ));
    if let Some(err) = aliases_error {
        lines.push(Line::from(Span::styled(
            format!("  ⚠ {}", err),
            Style::default().fg(theme::FG_ERR),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  可选，nginx server_name 附加域名",
            Style::default().fg(theme::FG_DIM),
        )));
    }
    lines.push(Line::from(""));

    // 站点类型
    let type_focused = form.focused == FormField::SiteType;
    lines.push(field_label("站点类型:", type_focused));
    for kind in SiteTypeChoice::ALL.iter() {
        let is_selected = *kind == form.site_type;
        if type_focused && is_selected {
            focused_line = lines.len();
        }
        let radio = if is_selected { "◉" } else { "○" };
        let style = if type_focused && is_selected {
            Style::default()
                .fg(theme::FG_HINT)
                .add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(theme::FG_HINT)
        } else {
            Style::default().fg(theme::FG_NORMAL)
        };
        let prefix = if type_focused { "  " } else { " " };
        lines.push(Line::from(Span::styled(
            format!("{}{} {}", prefix, radio, kind.label()),
            style,
        )));
    }
    lines.push(Line::from(""));

    // 代理目标（非静态站点时显示）
    if form.site_type != SiteTypeChoice::Static {
        let target_focused = form.focused == FormField::Target;
        if target_focused {
            focused_line = lines.len();
        }
        let target_error = form.get_error("target");
        lines.push(field_label("代理目标:", target_focused));
        lines.push(input_field(
            &form.target,
            target_focused,
            "8080 / IP:端口 / http(s)://地址",
        ));
        if let Some(err) = target_error {
            lines.push(Line::from(Span::styled(
                format!("  ⚠ {}", err),
                Style::default().fg(theme::FG_ERR),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "  端口号 如 8080 / IP:端口 / http(s)://地址",
                Style::default().fg(theme::FG_DIM),
            )));
        }
        lines.push(Line::from(""));
    }

    // 关键选项
    lines.push(Line::from(Span::styled(
        "关键选项:",
        Style::default().fg(theme::FG_NORMAL),
    )));
    if form.site_type == SiteTypeChoice::Proxy {
        if form.focused == FormField::ProxyFeatureStreaming {
            focused_line = lines.len();
        }
        lines.push(toggle_line(
            "流式响应 / AI API",
            form.feature_streaming,
            form.focused == FormField::ProxyFeatureStreaming,
        ));
        if form.focused == FormField::ProxyFeatureWebsocket {
            focused_line = lines.len();
        }
        lines.push(toggle_line(
            "WebSocket",
            form.feature_websocket,
            form.focused == FormField::ProxyFeatureWebsocket,
        ));
        if form.focused == FormField::ProxyFeatureLargeBody {
            focused_line = lines.len();
        }
        lines.push(toggle_line(
            "大请求体 / 上传",
            form.feature_large_body,
            form.focused == FormField::ProxyFeatureLargeBody,
        ));
        if form.focused == FormField::ProxyFeatureCors {
            focused_line = lines.len();
        }
        lines.push(toggle_line(
            "浏览器跨域 CORS",
            form.feature_cors,
            form.focused == FormField::ProxyFeatureCors,
        ));
        if form.focused == FormField::ProxyFeatureLongTimeout {
            focused_line = lines.len();
        }
        lines.push(toggle_line(
            "长超时后端",
            form.feature_long_timeout,
            form.focused == FormField::ProxyFeatureLongTimeout,
        ));
    } else if form.site_type == SiteTypeChoice::Static {
        let mode_focused = form.focused == FormField::StaticMode;
        if mode_focused {
            focused_line = lines.len() + 1;
        }
        lines.push(field_label("站点模式:", mode_focused));
        lines.push(static_mode_line(form.static_spa_mode, mode_focused));
        if form.focused == FormField::StaticFeatureCache {
            focused_line = lines.len();
        }
        lines.push(toggle_line(
            "静态资源缓存",
            form.static_cache,
            form.focused == FormField::StaticFeatureCache,
        ));
        if form.focused == FormField::StaticFeatureBlockSensitive {
            focused_line = lines.len();
        }
        lines.push(toggle_line(
            "敏感路径保护",
            form.static_block_sensitive,
            form.focused == FormField::StaticFeatureBlockSensitive,
        ));
    } else {
        lines.push(Line::from(Span::styled(
            "  Emby/Jellyfin 类型默认使用内置优化代理配置",
            Style::default().fg(theme::FG_DIM),
        )));
    }
    lines.push(Line::from(""));

    // 创建后操作
    let enable_focused = form.focused == FormField::EnableCheckbox;
    let cert_focused = form.focused == FormField::CertCheckbox;
    lines.push(Line::from(Span::styled(
        "创建后操作:",
        Style::default().fg(theme::FG_NORMAL),
    )));

    // 立即启用复选框
    let enable_marker = if form.enable_now { "[✕]" } else { "[ ]" };
    if enable_focused {
        focused_line = lines.len();
    }
    let enable_style = if enable_focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    lines.push(Line::from(Span::styled(
        format!("  {} 立即启用", enable_marker),
        enable_style,
    )));

    // 申请证书复选框
    let cert_marker = if form.request_cert { "[✕]" } else { "[ ]" };
    let cert_enabled = form.enable_now;
    if cert_focused {
        focused_line = lines.len();
    }
    let cert_style = if cert_focused && cert_enabled {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else if cert_enabled {
        Style::default().fg(theme::FG_NORMAL)
    } else {
        Style::default().fg(theme::FG_DIM)
    };
    let cert_label = if cert_enabled {
        format!("  {} 申请 SSL 证书（依赖立即启用）", cert_marker)
    } else {
        "  [ ] 申请 SSL 证书（需先勾选立即启用）".to_string()
    };
    lines.push(Line::from(Span::styled(cert_label, cert_style)));

    if let Some(err) = form.get_error("cert_checkbox") {
        lines.push(Line::from(Span::styled(
            format!("  ⚠ {}", err),
            Style::default().fg(theme::FG_ERR),
        )));
    }
    lines.push(Line::from(""));

    // 提交按钮
    let submit_focused = form.focused == FormField::SubmitButton;
    if submit_focused {
        focused_line = lines.len();
    }
    let btn_label = if form.submitting {
        "  创建中…  "
    } else {
        "   创  建   "
    };
    let btn_style = if submit_focused {
        Style::default()
            .bg(theme::BG_SELECTED)
            .fg(theme::FG_SELECTED)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    let surrounded = if submit_focused {
        format!("[[{}]]", btn_label)
    } else {
        format!("[ {} ]", btn_label)
    };
    lines.push(Line::from(Span::styled(
        format!("          {}", surrounded),
        btn_style,
    )));

    let scroll_offset = compute_scroll_offset(lines.len(), inner.height as usize, focused_line);
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset as u16, 0));
    frame.render_widget(p, inner);
}

fn compute_scroll_offset(total_lines: usize, visible_lines: usize, focused_line: usize) -> usize {
    if total_lines <= visible_lines {
        return 0;
    }

    focused_line
        .saturating_sub(visible_lines / 2)
        .min(total_lines.saturating_sub(visible_lines))
}

fn field_label<'a>(label: &str, focused: bool) -> Line<'a> {
    let style = if focused {
        Style::default().fg(theme::FG_HINT)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    Line::from(Span::styled(label.to_string(), style))
}

fn input_field<'a>(value: &str, focused: bool, placeholder: &str) -> Line<'a> {
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

    // 简化输入框：用 [ ] 包裹
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

fn toggle_line<'a>(label: &str, enabled: bool, focused: bool) -> Line<'a> {
    let marker = if enabled { "[✕]" } else { "[ ]" };
    let style = if focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    Line::from(Span::styled(format!("  {} {}", marker, label), style))
}

fn static_mode_line<'a>(spa_mode: bool, focused: bool) -> Line<'a> {
    let normal_style = if focused && !spa_mode {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else if !spa_mode {
        Style::default().fg(theme::FG_HINT)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    let spa_style = if focused && spa_mode {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else if spa_mode {
        Style::default().fg(theme::FG_HINT)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    Line::from(vec![
        Span::styled(if !spa_mode { "  ◉ " } else { "  ○ " }, normal_style),
        Span::styled("普通静态", normal_style),
        Span::styled(if spa_mode { "   ◉ " } else { "   ○ " }, spa_style),
        Span::styled("SPA 单页", spa_style),
    ])
}

#[cfg(test)]
mod tests {
    use super::compute_scroll_offset;

    #[test]
    fn no_scroll_when_content_fits() {
        assert_eq!(compute_scroll_offset(10, 12, 9), 0);
    }

    #[test]
    fn scroll_centers_around_focus_when_possible() {
        assert_eq!(compute_scroll_offset(30, 10, 15), 10);
    }

    #[test]
    fn scroll_clamps_to_bottom_when_focus_is_near_end() {
        assert_eq!(compute_scroll_offset(30, 10, 29), 20);
    }
}
