//! AppState logs 相关按键与动作处理。

use crate::app::state::app::{
    AppState, LOGS_HORIZONTAL_SCROLL_STEP, LOGS_PAGE_SCROLL_FACTOR,
};
use crate::app::state::common::FocusArea;
use crate::app::state::logs::LogsFocus;
use crate::domain::log::{LogKind, LogPaths, LogSource};
use crate::domain::site::Site;

impl AppState {
    pub(crate) fn detect_global_log_paths(&self) -> LogPaths {
        let nginx_root = self
            .ctx
            .probe
            .sites_available
            .parent()
            .unwrap_or(self.ctx.probe.sites_available.as_path());
        crate::domain::log::detect_global_log_paths(&nginx_root.join("nginx.conf"))
    }

    pub(crate) fn build_global_log_source(&self, kind: LogKind) -> LogSource {
        LogSource::global(kind, self.detect_global_log_paths())
    }

    pub(crate) fn build_site_log_source(&self, site: &Site, kind: LogKind) -> LogSource {
        let global = self.detect_global_log_paths();
        LogSource::site(
            site.name.clone(),
            kind,
            LogPaths {
                access: site.access_log_path.clone().or(global.access),
                error: site.error_log_path.clone().or(global.error),
            },
        )
    }

    pub(crate) fn current_logs_site_name(&self) -> Option<String> {
        match &self.logs.source {
            LogSource::Global { .. } => None,
            LogSource::Site { name, .. } => Some(name.clone()),
        }
    }

    pub(crate) fn rebuild_logs_source(&mut self, site_name: Option<String>, kind: LogKind) {
        self.logs.source = match site_name {
            Some(name) => self
                .sites
                .list
                .iter()
                .find(|site| site.name == name)
                .map(|site| self.build_site_log_source(site, kind))
                .unwrap_or_else(|| self.build_global_log_source(kind)),
            None => self.build_global_log_source(kind),
        };
    }

    pub(crate) fn handle_logs_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let visible_lines = self.logs_visible_lines_estimate();
        let page_step = visible_lines.saturating_sub(LOGS_PAGE_SCROLL_FACTOR).max(1) as isize;

        // 搜索模式优先
        if self.logs.focused == LogsFocus::SearchInput {
            match k.code {
                KeyCode::Esc => {
                    self.logs.clear_search();
                    self.logs.focused = LogsFocus::LogContent;
                    return;
                }
                KeyCode::Enter => {
                    // 执行搜索
                    let query = self.logs.search_query.clone();
                    if let Some(q) = query {
                        self.logs.search(&q);
                        self.logs.reveal_current_match(visible_lines);
                    }
                    self.logs.focused = LogsFocus::LogContent;
                    return;
                }
                KeyCode::Char(c) => {
                    if let Some(ref mut q) = self.logs.search_query {
                        q.push(c);
                    } else {
                        self.logs.search_query = Some(c.to_string());
                    }
                    return;
                }
                KeyCode::Backspace => {
                    if let Some(ref mut q) = self.logs.search_query {
                        q.pop();
                    }
                    return;
                }
                _ => {}
            }
            return;
        }

        // 搜索输入模式：按 / 进入
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('/')) {
            self.logs.search_query = Some(String::new());
            self.logs.focused = LogsFocus::SearchInput;
            return;
        }

        // Space: 暂停/继续
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char(' ')) {
            if self.logs.paused {
                self.logs.follow_tail(visible_lines);
            } else {
                self.logs.toggle_pause();
            }
            return;
        }

        // c: 清屏
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('c')) {
            self.logs.clear_buffer();
            return;
        }

        // Tab: 切换焦点区域
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Tab) {
            self.logs.focused = match self.logs.focused {
                LogsFocus::SiteSelector => LogsFocus::KindSelector,
                LogsFocus::KindSelector => LogsFocus::LogContent,
                LogsFocus::LogContent => LogsFocus::SiteSelector,
                LogsFocus::SearchInput => LogsFocus::LogContent,
            };
            return;
        }

        // n/N: 搜索结果导航（有搜索结果时）
        if self.logs.search_query.is_some() && !self.logs.match_lines.is_empty() {
            if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('n')) {
                self.logs.next_match();
                self.logs.reveal_current_match(visible_lines);
                return;
            }
            if k.modifiers.contains(KeyModifiers::SHIFT) && matches!(k.code, KeyCode::Char('N')) {
                self.logs.prev_match();
                self.logs.reveal_current_match(visible_lines);
                return;
            }
        }

        // Esc: 清除搜索或返回侧边栏
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            if self.logs.search_query.is_some() {
                self.logs.clear_search();
            } else {
                self.focus = FocusArea::Sidebar;
            }
            return;
        }

        // 左右键：在站点选择器切换站点 / 类型选择器切换类型
        if k.modifiers == KeyModifiers::NONE {
            match self.logs.focused {
                LogsFocus::SiteSelector => {
                    match (k.code, !self.sites.list.is_empty()) {
                        (KeyCode::Left | KeyCode::Char('h'), true) => {
                            // 切换到上一个站点（从站点列表）
                            let cur_site = self.current_logs_site_name();
                            let cur_idx = self
                                .sites
                                .list
                                .iter()
                                .position(|s| Some(&s.name) == cur_site.as_ref());
                            let prev_idx = match cur_idx {
                                Some(i) => {
                                    if i == 0 {
                                        None
                                    } else {
                                        Some(i - 1)
                                    }
                                }
                                None => Some(self.sites.list.len() - 1),
                            };
                            let next_site = prev_idx.map(|i| self.sites.list[i].name.clone());
                            self.rebuild_logs_source(next_site, self.logs.source.kind());
                            self.logs.clear_buffer();
                            self.logs.pending_tail_change = true;
                        }
                        (KeyCode::Right | KeyCode::Char('l'), true) => {
                            // 切换到下一个站点
                            let cur_site = self.current_logs_site_name();
                            let cur_idx = self
                                .sites
                                .list
                                .iter()
                                .position(|s| Some(&s.name) == cur_site.as_ref());
                            let next_idx = match cur_idx {
                                Some(i) => {
                                    if i >= self.sites.list.len() - 1 {
                                        None
                                    } else {
                                        Some(i + 1)
                                    }
                                }
                                None => Some(0),
                            };
                            let next_site = next_idx.map(|i| self.sites.list[i].name.clone());
                            self.rebuild_logs_source(next_site, self.logs.source.kind());
                            self.logs.clear_buffer();
                            self.logs.pending_tail_change = true;
                        }
                        (KeyCode::Enter, _) => {
                            self.logs.focused = LogsFocus::KindSelector;
                        }
                        _ => {}
                    }
                }
                LogsFocus::KindSelector => match k.code {
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('t') => {
                        let next_kind = self.logs.source.kind().toggle();
                        let site_name = self.current_logs_site_name();
                        self.rebuild_logs_source(site_name, next_kind);
                        self.logs.clear_buffer();
                        self.logs.pending_tail_change = true;
                    }
                    KeyCode::Enter => {
                        self.logs.focused = LogsFocus::LogContent;
                    }
                    _ => {}
                },
                LogsFocus::LogContent => match k.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.logs.scroll_vertical(-1, visible_lines);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.logs.scroll_vertical(1, visible_lines);
                    }
                    KeyCode::PageUp => {
                        self.logs.scroll_vertical(-page_step, visible_lines);
                    }
                    KeyCode::PageDown => {
                        self.logs.scroll_vertical(page_step, visible_lines);
                    }
                    KeyCode::Home => {
                        self.logs.vertical_scroll = 0;
                        self.logs.paused = true;
                    }
                    KeyCode::End => {
                        self.logs.follow_tail(visible_lines);
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        self.logs.scroll_horizontal(-LOGS_HORIZONTAL_SCROLL_STEP);
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        self.logs.scroll_horizontal(LOGS_HORIZONTAL_SCROLL_STEP);
                    }
                    _ => {}
                },
                LogsFocus::SearchInput => {}
            }
        }
    }
}
