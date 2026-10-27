//! 新建与托管编辑页共用的传输、压缩及代理参数视图。

use std::collections::HashMap;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::template::site_tuning::{Http2Support, SiteTuning, TuningField};
use crate::ui::theme;

#[derive(Debug, Clone, Copy)]
pub struct FocusedLines {
    pub start: usize,
    pub end: usize,
}

pub fn append_tuning_fields<'a>(
    lines: &mut Vec<Line<'a>>,
    tuning: &SiteTuning,
    show_proxy_fields: bool,
    http2_support: Http2Support,
    ssl_enabled: bool,
    focused: Option<TuningField>,
    field_errors: &HashMap<String, String>,
) -> Option<FocusedLines> {
    let mut focused_lines = None;

    lines.push(section_title("传输与压缩:"));
    push_choice(
        lines,
        "HTTP/2",
        tuning.http2.label(),
        TuningField::Http2,
        focused,
        &mut focused_lines,
    );
    lines.push(Line::from(Span::styled(
        format!(
            "  {}{}",
            if ssl_enabled {
                ""
            } else {
                "启用 HTTPS 后生效；"
            },
            http2_capability_hint(http2_support)
        ),
        Style::default().fg(theme::FG_DIM),
    )));
    if focused == Some(TuningField::Http2) {
        if let Some(range) = &mut focused_lines {
            range.end = lines.len();
        }
    }
    push_choice(
        lines,
        "Gzip 压缩",
        tuning.gzip.label(),
        TuningField::Gzip,
        focused,
        &mut focused_lines,
    );
    lines.push(Line::from(""));

    if show_proxy_fields {
        lines.push(section_title("代理参数:"));
        push_input(
            lines,
            "上传大小:",
            &tuning.body_size,
            "继承",
            "  数字默认 MiB（支持 k/m/g，0 不限）；Ctrl+U/Delete 清空并继承",
            TuningField::BodySize,
            focused,
            field_errors,
            &mut focused_lines,
        );
        push_input(
            lines,
            "连接超时:",
            &tuning.connect_timeout,
            "继承",
            "  数字默认秒（支持 ms/s/m/h）；Ctrl+U/Delete 清空并继承 Nginx",
            TuningField::ConnectTimeout,
            focused,
            field_errors,
            &mut focused_lines,
        );
        push_input(
            lines,
            "发送超时:",
            &tuning.send_timeout,
            "继承",
            "  数字默认秒（支持 ms/s/m/h）；Ctrl+U/Delete 清空并继承 Nginx",
            TuningField::SendTimeout,
            focused,
            field_errors,
            &mut focused_lines,
        );
        push_input(
            lines,
            "读取超时:",
            &tuning.read_timeout,
            "继承",
            "  数字默认秒（支持 ms/s/m/h）；Ctrl+U/Delete 清空并使用流式/WebSocket 预设",
            TuningField::ReadTimeout,
            focused,
            field_errors,
            &mut focused_lines,
        );
        lines.push(Line::from(""));
    }

    focused_lines
}

fn http2_capability_hint(support: Http2Support) -> &'static str {
    match support {
        Http2Support::Modern => "自动会启用 HTTP/2（当前 Nginx 支持独立开关）",
        Http2Support::Legacy => "自动会启用 HTTP/2；旧版按监听端口生效，无法仅关闭单个站点",
        Http2Support::Unsupported => "自动不会启用：当前 Nginx 缺少 HTTP/2 模块",
        Http2Support::Unknown => "自动暂不启用：尚未确认 Nginx HTTP/2 能力",
    }
}

#[allow(clippy::too_many_arguments)]
fn push_input<'a>(
    lines: &mut Vec<Line<'a>>,
    label: &str,
    value: &str,
    placeholder: &str,
    hint: &str,
    field: TuningField,
    focused: Option<TuningField>,
    field_errors: &HashMap<String, String>,
    focused_lines: &mut Option<FocusedLines>,
) {
    let is_focused = focused == Some(field);
    let start = lines.len();
    lines.push(field_label(label, is_focused));
    lines.push(input_field(value, is_focused, placeholder));
    if let Some(error) = field_errors.get(field.key()) {
        lines.push(error_line(error));
    } else {
        lines.push(Line::from(Span::styled(
            hint.to_string(),
            Style::default().fg(theme::FG_DIM),
        )));
    }
    if is_focused {
        *focused_lines = Some(FocusedLines {
            start,
            end: lines.len(),
        });
    }
}

fn push_choice<'a>(
    lines: &mut Vec<Line<'a>>,
    label: &str,
    value: &str,
    field: TuningField,
    focused: Option<TuningField>,
    focused_lines: &mut Option<FocusedLines>,
) {
    let is_focused = focused == Some(field);
    let start = lines.len();
    let style = if is_focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    lines.push(Line::from(Span::styled(
        format!("  {}: ◉ {}", label, value),
        style,
    )));
    if is_focused {
        *focused_lines = Some(FocusedLines {
            start,
            end: lines.len(),
        });
    }
}

fn section_title(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default().fg(theme::FG_NORMAL),
    ))
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
        let mut display = value.to_string();
        if focused {
            display.push('▏');
        }
        display
    };
    let value_style = if focused {
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
        Span::styled(display, value_style),
        Span::styled("]", bracket_style),
    ])
}

fn error_line(message: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  ⚠ {}", message),
        Style::default().fg(theme::FG_ERR),
    ))
}

/// 按 Paragraph 的实际换行结果计算滚动位置，并确保整个焦点字段可见。
pub fn scroll_offset(lines: &[Line<'_>], width: u16, height: u16, focused: FocusedLines) -> u16 {
    if width == 0 || height == 0 {
        return 0;
    }
    let wrapped_height = |slice: &[Line<'_>]| wrapped_line_count(slice, width);
    let total = wrapped_height(lines);
    if total <= height {
        return 0;
    }

    let focus_start = wrapped_height(&lines[..focused.start]);
    let focus_end = wrapped_height(&lines[..focused.end]);
    let focus_height = focus_end.saturating_sub(focus_start);
    let max_offset = total.saturating_sub(height);
    if focus_height >= height {
        return focus_start.min(max_offset);
    }

    let centered = focus_start.saturating_sub((height - focus_height) / 2);
    centered
        .max(focus_end.saturating_sub(height))
        .min(focus_start)
        .min(max_offset)
}

fn wrapped_line_count(lines: &[Line<'_>], width: u16) -> u16 {
    wrap_lines(lines, width).len().min(u16::MAX as usize) as u16
}

/// 将逻辑行按终端列宽预先拆成屏幕行，使滚动计算与最终渲染使用同一套换行。
pub fn wrap_lines<'a>(lines: &[Line<'a>], width: u16) -> Vec<Line<'a>> {
    let width = usize::from(width.max(1));
    let mut wrapped = Vec::new();

    for line in lines {
        let mut row = Vec::new();
        let mut row_width = 0usize;
        for span in &line.spans {
            for character in span.content.chars() {
                let character_width = character.width().unwrap_or(0);
                if row_width > 0 && row_width + character_width > width {
                    wrapped.push(Line::from(std::mem::take(&mut row)).style(line.style));
                    row_width = 0;
                }
                row.push(Span::styled(character.to_string(), span.style));
                row_width += character_width;
            }
        }
        wrapped.push(Line::from(row).style(line.style));
    }

    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::Paragraph;
    use ratatui::Terminal;

    #[test]
    fn small_terminal_keeps_focused_numeric_value_and_error_visible(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tuning = SiteTuning {
            read_timeout: "12s".into(),
            ..Default::default()
        };
        let mut errors = HashMap::new();
        errors.insert("read_timeout".into(), "读取超时格式错误".into());
        let mut lines = (0..10).map(|_| Line::from("前置内容")).collect::<Vec<_>>();
        let focused = append_tuning_fields(
            &mut lines,
            &tuning,
            true,
            Http2Support::Modern,
            true,
            Some(TuningField::ReadTimeout),
            &errors,
        )
        .ok_or("missing focused field")?;
        let offset = scroll_offset(&lines, 26, 6, focused);
        let backend = TestBackend::new(26, 6);
        let mut terminal = Terminal::new(backend)?;

        terminal.draw(|frame| {
            frame.render_widget(
                Paragraph::new(wrap_lines(&lines, 26)).scroll((offset, 0)),
                frame.area(),
            );
        })?;

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("12s▏"));
        assert!(rendered.replace(' ', "").contains("读取超时格式错误"));
        Ok(())
    }
}
