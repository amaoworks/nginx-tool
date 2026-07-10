//! 日志视图子状态。

use std::collections::VecDeque;

use crate::domain::log::LogSource;

/// 日志视图焦点区域
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogsFocus {
    #[default]
    SiteSelector,
    KindSelector,
    LogContent,
    SearchInput,
}

/// 日志视图子状态
#[derive(Debug)]
pub struct LogsState {
    /// 当前日志源
    pub source: LogSource,
    /// 焦点区域
    pub focused: LogsFocus,
    /// 日志行缓冲（环形队列）
    pub buffer: VecDeque<String>,
    /// 最大行数
    pub max_lines: usize,
    /// 是否暂停滚动（暂停时仍收集但不自动滚动）
    pub paused: bool,
    /// 搜索关键词
    pub search_query: Option<String>,
    /// 当前匹配索引（搜索结果中的位置）
    pub match_index: Option<usize>,
    /// 匹配行号列表
    pub match_lines: Vec<usize>,
    /// 纵向滚动偏移（顶部行为 0）
    pub vertical_scroll: usize,
    /// 横向滚动偏移（左侧列为 0）
    pub horizontal_scroll: u16,
    /// tail 任务句柄（用于取消）
    pub tail_handle: Option<tokio::task::JoinHandle<()>>,
    /// tail 输出接收通道
    pub tail_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::infra::log_tail::TailLine>>,
    /// 日志源变更请求（主循环消费后启动新 tail）
    pub pending_tail_change: bool,
}

impl Default for LogsState {
    fn default() -> Self {
        Self {
            source: LogSource::default(),
            focused: LogsFocus::default(),
            buffer: VecDeque::with_capacity(1000),
            max_lines: 1000,
            paused: false,
            search_query: None,
            match_index: None,
            match_lines: Vec::new(),
            vertical_scroll: 0,
            horizontal_scroll: 0,
            tail_handle: None,
            tail_rx: None,
            pending_tail_change: false,
        }
    }
}

impl LogsState {
    /// 追加日志行，超出限制时丢弃最旧行
    pub fn push_line(&mut self, line: String) {
        let preferred_line = self
            .match_index
            .and_then(|idx| self.match_lines.get(idx).copied());
        let mut adjusted_preferred_line = preferred_line;

        if self.buffer.len() >= self.max_lines {
            self.buffer.pop_front();
            self.vertical_scroll = self.vertical_scroll.saturating_sub(1);
            adjusted_preferred_line = preferred_line.map(|idx| idx.saturating_sub(1));
        }
        self.buffer.push_back(line);
        if self.search_query.is_some() {
            self.rebuild_matches(adjusted_preferred_line);
        }
    }

    /// 清空缓冲区
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
        self.match_lines.clear();
        self.match_index = None;
        self.vertical_scroll = 0;
        self.horizontal_scroll = 0;
    }

    /// 执行搜索，更新 match_lines
    pub fn search(&mut self, query: &str) {
        self.search_query = Some(query.to_string());
        self.rebuild_matches(None);
    }

    /// 跳转到下一个匹配
    pub fn next_match(&mut self) {
        if self.match_lines.is_empty() || self.match_index.is_none() {
            return;
        }
        let cur = self.match_index.unwrap();
        let next = (cur + 1) % self.match_lines.len();
        self.match_index = Some(next);
    }

    /// 跳转到上一个匹配
    pub fn prev_match(&mut self) {
        if self.match_lines.is_empty() || self.match_index.is_none() {
            return;
        }
        let cur = self.match_index.unwrap();
        let prev = if cur == 0 {
            self.match_lines.len() - 1
        } else {
            cur - 1
        };
        self.match_index = Some(prev);
    }

    /// 清除搜索状态
    pub fn clear_search(&mut self) {
        self.search_query = None;
        self.match_lines.clear();
        self.match_index = None;
    }

    fn rebuild_matches(&mut self, preferred_line: Option<usize>) {
        self.match_lines.clear();
        self.match_index = None;

        let Some(query) = self.search_query.as_deref() else {
            return;
        };
        if query.is_empty() {
            return;
        }

        for (i, line) in self.buffer.iter().enumerate() {
            if line.contains(query) {
                self.match_lines.push(i);
            }
        }

        if self.match_lines.is_empty() {
            return;
        }

        self.match_index = preferred_line
            .and_then(|line| self.match_lines.iter().position(|idx| *idx >= line))
            .or(Some(0));
    }

    /// 是否自动跟随到最新日志
    pub fn is_following_tail(&self, visible_lines: usize) -> bool {
        self.vertical_scroll >= self.max_vertical_scroll(visible_lines)
    }

    /// 当前可滚动的最大纵向偏移
    pub fn max_vertical_scroll(&self, visible_lines: usize) -> usize {
        self.buffer.len().saturating_sub(visible_lines)
    }

    /// 规范化滚动偏移，避免越界
    pub fn clamp_scroll(&mut self, visible_lines: usize) {
        self.vertical_scroll = self
            .vertical_scroll
            .min(self.max_vertical_scroll(visible_lines));
    }

    /// 跳到底部并开启自动跟随
    pub fn follow_tail(&mut self, visible_lines: usize) {
        self.vertical_scroll = self.max_vertical_scroll(visible_lines);
        self.paused = false;
    }

    /// 垂直滚动；手动滚动会进入暂停，避免新日志抢回底部
    pub fn scroll_vertical(&mut self, delta: isize, visible_lines: usize) {
        let max_scroll = self.max_vertical_scroll(visible_lines) as isize;
        let next = (self.vertical_scroll as isize + delta).clamp(0, max_scroll) as usize;
        self.vertical_scroll = next;
        self.paused = self.vertical_scroll < self.max_vertical_scroll(visible_lines);
    }

    /// 水平滚动
    pub fn scroll_horizontal(&mut self, delta: i16) {
        self.horizontal_scroll = self.horizontal_scroll.saturating_add_signed(delta);
    }

    /// 将指定日志行滚动到可视区域内
    pub fn ensure_line_visible(&mut self, line_idx: usize, visible_lines: usize) {
        if visible_lines == 0 {
            return;
        }
        let last_visible = self.vertical_scroll + visible_lines.saturating_sub(1);
        if line_idx < self.vertical_scroll {
            self.vertical_scroll = line_idx;
        } else if line_idx > last_visible {
            self.vertical_scroll = line_idx + 1 - visible_lines;
        }
        self.clamp_scroll(visible_lines);
    }

    /// 对齐到当前匹配位置
    pub fn reveal_current_match(&mut self, visible_lines: usize) {
        let Some(current) = self.match_index else {
            return;
        };
        let Some(&line_idx) = self.match_lines.get(current) else {
            return;
        };
        self.ensure_line_visible(line_idx, visible_lines);
        self.paused = true;
    }

    /// 切换暂停状态
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// 停止 tail 任务
    pub fn stop_tail(&mut self) {
        if let Some(handle) = self.tail_handle.take() {
            handle.abort();
        }
        self.tail_rx = None;
    }
}
