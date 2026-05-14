//! 日志视图渲染，对应 design.md 视图 4

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::state::{AppState, LogsFocus};
use crate::domain::log::LogSource;
use crate::ui::theme;

/// 页面内部仅保留状态信息，动作提示统一交给全局 footer。
const STATUS_BAR_HEIGHT: u16 = 1;

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    // 顶部标题栏
    let header = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(
            " 📋 日志查看 ",
            Style::default().fg(theme::FG_PATH),
        ));
    let header_area = Rect { height: 2, ..area };
    frame.render_widget(header, header_area);

    // 控制栏：站点选择 + 类型选择
    let controls_area = Rect {
        y: area.y + 2,
        height: 2,
        width: area.width,
        x: area.x,
    };
    render_controls(frame, controls_area, state);

    // 日志内容区
    let content_area = Rect {
        y: area.y + 4,
        height: area.height.saturating_sub(4 + STATUS_BAR_HEIGHT),
        width: area.width,
        x: area.x,
    };
    render_content(frame, content_area, state);

    // 状态信息栏
    let status_area = Rect {
        y: area.y + area.height.saturating_sub(STATUS_BAR_HEIGHT),
        height: STATUS_BAR_HEIGHT,
        width: area.width,
        x: area.x,
    };
    render_status(frame, status_area, state);
}

fn render_controls(frame: &mut Frame, area: Rect, state: &AppState) {
    let logs = &state.logs;

    // 站点下拉
    let site_label = match &logs.source {
        LogSource::Global(_) => "全部站点",
        LogSource::Site { name, .. } => name,
    };
    let site_focused = logs.focused == LogsFocus::SiteSelector;
    let site_style = if site_focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    let site_prefix = if site_focused { "▸" } else { " " };
    let site_span = Span::styled(
        format!(" 站点: [{} {} ▼] ", site_prefix, truncate(site_label, 12)),
        site_style,
    );

    // 类型选择
    let kind_label = logs.source.kind().label();
    let kind_focused = logs.focused == LogsFocus::KindSelector;
    let kind_style = if kind_focused {
        Style::default()
            .fg(theme::FG_HINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::FG_NORMAL)
    };
    let kind_prefix = if kind_focused { "▸" } else { " " };
    let kind_span = Span::styled(
        format!(" 类型: [{} {} ▼] ", kind_prefix, kind_label),
        kind_style,
    );

    // 日志源路径提示
    let path_span = Span::styled(
        truncate(&logs.source.path().display().to_string(), 40),
        Style::default().fg(theme::FG_DIM),
    );

    // 文件不存在警告
    let warn_span = if !logs.source.exists() {
        Span::styled(" ⚠ 文件不存在", Style::default().fg(theme::FG_WARN))
    } else {
        Span::raw("")
    };

    let line = Line::from(vec![site_span, kind_span, path_span, warn_span]);
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Left), area);
}

fn render_content(frame: &mut Frame, area: Rect, state: &AppState) {
    let logs = &state.logs;
    let focused = logs.focused == LogsFocus::LogContent;

    // 边框
    let border_style = if focused {
        Style::default().fg(theme::FG_HINT)
    } else {
        Style::default().fg(theme::BORDER)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(
            format!(
                " {} {}",
                logs.source.label(),
                if logs.paused { "（暂停）" } else { "" }
            ),
            Style::default().fg(theme::FG_PATH),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 1 {
        return;
    }

    // 日志不存在时的提示
    if !logs.source.exists() {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "⚠ 日志文件不存在",
                Style::default().fg(theme::FG_WARN),
            )),
            Line::from(Span::styled(
                format!("路径：{}", logs.source.path().display()),
                Style::default().fg(theme::FG_DIM),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "提示：检查 Nginx 是否运行，或使用「站点管理」查看该站点是否已启用",
                Style::default().fg(theme::FG_DIM),
            )),
        ];
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
        return;
    }

    // 空缓冲时的提示
    if logs.buffer.is_empty() {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "等待日志数据…",
                Style::default().fg(theme::FG_DIM),
            )),
        ];
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
        return;
    }

    // 渲染日志行，搜索匹配时高亮
    let lines: Vec<Line> = logs
        .buffer
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let line_idx = i;
            // 检查是否为匹配行
            let is_match = logs.match_lines.contains(&line_idx);
            let is_current_match = logs
                .match_index
                .map(|idx| logs.match_lines.get(idx) == Some(&line_idx))
                .unwrap_or(false);

            if is_match {
                // 高亮匹配行
                let style = if is_current_match {
                    Style::default()
                        .bg(theme::BG_SELECTED)
                        .fg(theme::FG_SELECTED)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().bg(theme::BG_SELECTED).fg(theme::FG_NORMAL)
                };
                // 如果有搜索词，分段高亮
                if let Some(query) = &logs.search_query {
                    highlight_line(line, query, is_current_match)
                } else {
                    Line::from(Span::styled((*line).to_string(), style))
                }
            } else {
                Line::from(Span::styled(
                    (*line).to_string(),
                    Style::default().fg(theme::FG_NORMAL),
                ))
            }
        })
        .collect();

    let p = Paragraph::new(lines).scroll((logs.vertical_scroll as u16, logs.horizontal_scroll));
    frame.render_widget(p, inner);
}

/// 高亮搜索匹配的行
fn highlight_line(line: &str, query: &str, is_current: bool) -> Line<'static> {
    let base_style = Style::default().bg(theme::BG_SELECTED).fg(theme::FG_NORMAL);
    let match_style = if is_current {
        Style::default()
            .bg(theme::FG_WARN)
            .fg(theme::FG_NORMAL)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(theme::FG_WARN).fg(theme::FG_NORMAL)
    };

    let mut spans: Vec<Span> = Vec::new();
    let mut remaining = line;

    while !remaining.is_empty() {
        if let Some(pos) = remaining.find(query) {
            // 前缀部分
            if pos > 0 {
                spans.push(Span::styled(remaining[..pos].to_string(), base_style));
            }
            // 匹配部分
            spans.push(Span::styled(
                remaining[pos..pos + query.len()].to_string(),
                match_style,
            ));
            remaining = &remaining[pos + query.len()..];
        } else {
            // 剩余部分
            spans.push(Span::styled(remaining.to_string(), base_style));
            break;
        }
    }

    Line::from(spans)
}

fn render_status(frame: &mut Frame, area: Rect, state: &AppState) {
    let logs = &state.logs;

    let mut parts: Vec<String> = Vec::new();

    if logs.focused == LogsFocus::SearchInput {
        parts.push("搜索中".to_string());
    } else {
        if logs.paused {
            parts.push("已暂停".to_string());
        }
        parts.push(format!(
            "行 {}/{}",
            logs.vertical_scroll.saturating_add(1),
            logs.buffer.len()
        ));
        if logs.horizontal_scroll > 0 {
            parts.push(format!("列 {}", logs.horizontal_scroll.saturating_add(1)));
        }
        if !logs.match_lines.is_empty() {
            let total = logs.match_lines.len();
            let current = logs.match_index.map(|idx| idx + 1).unwrap_or(0);
            parts.push(format!("匹配 {}/{}", current, total));
        }
    }

    let hint_text = parts.join("  ");
    let line = Line::from(Span::styled(
        format!(" {} ", hint_text),
        Style::default().fg(theme::FG_DIM),
    ));
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Left), area);
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
    out
}
