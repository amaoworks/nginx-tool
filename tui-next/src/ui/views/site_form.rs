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

    // 站点名称
    let name_focused = form.focused == FormField::SiteName;
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

    // 创建后操作
    let enable_focused = form.focused == FormField::EnableCheckbox;
    let cert_focused = form.focused == FormField::CertCheckbox;
    lines.push(Line::from(Span::styled(
        "创建后操作:",
        Style::default().fg(theme::FG_NORMAL),
    )));

    // 立即启用复选框
    let enable_marker = if form.enable_now { "[✕]" } else { "[ ]" };
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

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
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
