//! 站点编辑器视图（表单模式），对应 design.md 子模式 C

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::state::{AppState, EditFocus};
use crate::template::config_parser::InjectionSlot;
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let edit = &state.site_edit;
    let site_name = &edit.site_name;

    let block = Block::default().borders(Borders::NONE).title(Span::styled(
        format!(" 📁 站点管理 ▸ 编辑: {} ", site_name),
        Style::default().fg(theme::FG_PATH),
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // 站点信息区
    lines.push(Line::from(Span::styled(
        "═══ 站点信息 ═══",
        Style::default().fg(theme::FG_DIM),
    )));
    lines.push(Line::from(""));

    // 类型（只读显示）
    let type_label = edit.site_type.label();
    lines.push(Line::from(Span::styled(
        format!("类型:   {}", type_label),
        Style::default().fg(theme::FG_NORMAL),
    )));
    lines.push(Line::from(""));

    // 域名
    let domain_focused = edit.focused == EditFocus::Domain;
    lines.push(field_label("域名:", domain_focused));
    lines.push(input_field(&edit.domain, domain_focused, "example.com"));
    if let Some(err) = edit.field_errors.get("domain") {
        lines.push(error_line(err));
    }
    lines.push(Line::from(""));

    // 附加域名
    let aliases_focused = edit.focused == EditFocus::DomainAliases;
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

    // 代理目标（非静态站点）
    if edit.site_type != crate::domain::site::SiteType::Static {
        let target_focused = edit.focused == EditFocus::Target;
        lines.push(field_label("目标:", target_focused));
        lines.push(input_field(&edit.target, target_focused, "127.0.0.1:8080"));
        if let Some(err) = edit.field_errors.get("target") {
            lines.push(error_line(err));
        }
        lines.push(Line::from(""));

        // 协议选择
        let scheme_focused = edit.focused == EditFocus::Scheme;
        lines.push(field_label("协议:", scheme_focused));
        let http_selected = edit.upstream_scheme == "http";
        let scheme_line = Line::from(vec![
            Span::styled(
                if http_selected { " ◉ " } else { " ○ " },
                Style::default().fg(if scheme_focused {
                    theme::FG_HINT
                } else {
                    theme::FG_NORMAL
                }),
            ),
            Span::styled(
                "http",
                Style::default().fg(if http_selected {
                    theme::FG_HINT
                } else {
                    theme::FG_NORMAL
                }),
            ),
            Span::styled(
                if !http_selected { "   ◉ " } else { "   ○ " },
                Style::default().fg(if scheme_focused {
                    theme::FG_HINT
                } else {
                    theme::FG_NORMAL
                }),
            ),
            Span::styled(
                "https",
                Style::default().fg(if !http_selected {
                    theme::FG_HINT
                } else {
                    theme::FG_NORMAL
                }),
            ),
        ]);
        lines.push(scheme_line);
        lines.push(Line::from(""));
    }

    // 注入槽区
    lines.push(Line::from(Span::styled(
        "═══ 自定义注入槽 ═══",
        Style::default().fg(theme::FG_DIM),
    )));
    lines.push(Line::from(""));

    // 槽位选择器
    let slot_focused = edit.focused == EditFocus::SlotSelector;
    let slots = InjectionSlot::ALL;
    let slot_labels: Vec<Span> = slots
        .iter()
        .map(|s| {
            let is_current = *s == edit.current_slot;
            let style = if slot_focused && is_current {
                Style::default()
                    .fg(theme::FG_HINT)
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default().fg(theme::FG_HINT)
            } else {
                Style::default().fg(theme::FG_DIM)
            };
            let marker = if is_current { "◉ " } else { "○ " };
            Span::styled(format!("{}{}", marker, s.label()), style)
        })
        .collect();
    lines.push(Line::from(slot_labels));
    lines.push(Line::from(Span::styled(
        format!("说明: {}", edit.current_slot.description()),
        Style::default().fg(theme::FG_DIM),
    )));
    lines.push(Line::from(""));

    let content_focused = edit.focused == EditFocus::SlotContent;
    let slot_content = edit
        .injection_slots
        .get(&edit.current_slot)
        .cloned()
        .unwrap_or_default();
    lines.push(Line::from(Span::styled(
        format!("{}:", edit.current_slot.label()),
        if content_focused {
            Style::default().fg(theme::FG_HINT)
        } else {
            Style::default().fg(theme::FG_NORMAL)
        },
    )));
    if slot_content.is_empty() {
        lines.push(Line::from(Span::styled(
            "[空]",
            Style::default().fg(theme::FG_DIM),
        )));
    } else {
        for line in slot_content.lines().take(6) {
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(theme::FG_NORMAL),
            )));
        }
    }

    // 模板列表
    let template_focused = edit.focused == EditFocus::TemplateList;
    let snippets = crate::template::snippets::get_snippets_for_slot(edit.current_slot);
    if !snippets.is_empty() {
        let mut template_lines: Vec<Line> = Vec::new();
        template_lines.push(Line::from(Span::styled(
            "相关模板:",
            Style::default().fg(theme::FG_NORMAL),
        )));
        for (i, snippet) in snippets.iter().enumerate() {
            let is_selected = i == edit.template_index;
            let style = if template_focused && is_selected {
                Style::default()
                    .fg(theme::FG_HINT)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(theme::FG_HINT)
            } else {
                Style::default().fg(theme::FG_NORMAL)
            };
            let prefix = if is_selected { "▸ " } else { "  " };
            template_lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, snippet.name),
                style,
            )));
        }
        lines.push(Line::from(""));
        lines.extend(template_lines);
    }

    // 状态提示
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

fn error_line(msg: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  ⚠ {}", msg),
        Style::default().fg(theme::FG_ERR),
    ))
}
