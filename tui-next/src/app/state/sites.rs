//! 站点列表子状态与排序。

use std::time::Instant;

use crate::domain::site::Site;

/// 站点列表子状态。
#[derive(Debug, Default)]
pub struct SitesState {
    pub list: Vec<Site>,
    pub last_refresh: Option<Instant>,
    pub refreshing: bool,
    pub pending_refresh: bool,
    /// 表格内当前选中行
    pub selected: usize,
    /// 当前正在执行启停操作的站点名（操作期间禁用同一站点的二次触发）
    pub action_in_flight: Option<String>,
    /// 最近一次加载错误（如目录不可读）
    pub last_error: Option<String>,
    /// 待派发的启停请求（站点名 + 目标启用状态）
    pub pending_toggle: Option<(String, bool)>,
    /// 待派发的删除请求（站点名）
    pub pending_delete: Option<String>,
    /// 当前排序字段
    pub sort_by: SitesSortField,
    /// 当前排序方向
    pub sort_order: SortOrder,
}

impl SitesState {
    pub fn move_cursor(&mut self, delta: i32) {
        if self.list.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.list.len() as i32;
        let mut idx = self.selected as i32 + delta;
        if idx < 0 {
            idx = len - 1;
        } else if idx >= len {
            idx = 0;
        }
        self.selected = idx as usize;
    }

    pub fn current(&self) -> Option<&Site> {
        self.list.get(self.selected)
    }

    pub fn cycle_sort_field(&mut self) {
        let current_name = self.current().map(|site| site.name.clone());
        self.sort_by = self.sort_by.next();
        self.sort_preserving_selection(current_name.as_deref());
    }

    pub fn toggle_sort_order(&mut self) {
        let current_name = self.current().map(|site| site.name.clone());
        self.sort_order = self.sort_order.toggle();
        self.sort_preserving_selection(current_name.as_deref());
    }

    pub fn replace_list(&mut self, list: Vec<Site>) {
        let current_name = self.current().map(|site| site.name.clone());
        self.list = list;
        self.sort_preserving_selection(current_name.as_deref());
    }

    pub(crate) fn sort_preserving_selection(&mut self, preferred_name: Option<&str>) {
        self.list.sort_by(|a, b| {
            let cmp = match self.sort_by {
                SitesSortField::Status => site_enabled_rank(a).cmp(&site_enabled_rank(b)),
                SitesSortField::Name => a.name.cmp(&b.name),
                SitesSortField::Type => site_type_rank(a).cmp(&site_type_rank(b)),
                SitesSortField::Ssl => site_ssl_rank(a).cmp(&site_ssl_rank(b)),
            }
            .then_with(|| a.name.cmp(&b.name));

            match self.sort_order {
                SortOrder::Asc => cmp,
                SortOrder::Desc => cmp.reverse(),
            }
        });

        if let Some(name) = preferred_name {
            if let Some(idx) = self.list.iter().position(|site| site.name == name) {
                self.selected = idx;
                return;
            }
        }

        self.selected = self.selected.min(self.list.len().saturating_sub(1));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SitesSortField {
    Status,
    #[default]
    Name,
    Type,
    Ssl,
}

impl SitesSortField {
    pub fn label(self) -> &'static str {
        match self {
            SitesSortField::Status => "状态",
            SitesSortField::Name => "名称",
            SitesSortField::Type => "类型",
            SitesSortField::Ssl => "SSL",
        }
    }

    fn next(self) -> Self {
        match self {
            SitesSortField::Status => SitesSortField::Name,
            SitesSortField::Name => SitesSortField::Type,
            SitesSortField::Type => SitesSortField::Ssl,
            SitesSortField::Ssl => SitesSortField::Status,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

impl SortOrder {
    pub fn glyph(self) -> &'static str {
        match self {
            SortOrder::Asc => "↑",
            SortOrder::Desc => "↓",
        }
    }

    fn toggle(self) -> Self {
        match self {
            SortOrder::Asc => SortOrder::Desc,
            SortOrder::Desc => SortOrder::Asc,
        }
    }
}

fn site_enabled_rank(site: &Site) -> u8 {
    if site.enabled {
        0
    } else {
        1
    }
}

fn site_type_rank(site: &Site) -> u8 {
    match site.site_type {
        crate::domain::site::SiteType::Proxy => 0,
        crate::domain::site::SiteType::Emby => 1,
        crate::domain::site::SiteType::Static => 2,
        crate::domain::site::SiteType::Unknown => 3,
    }
}

fn site_ssl_rank(site: &Site) -> (u8, i64) {
    match &site.ssl {
        crate::domain::site::SslStatus::Active { days_left } => {
            let level = site.ssl.level();
            let rank = match level {
                crate::domain::site::SslLevel::Critical => 0,
                crate::domain::site::SslLevel::Warning => 1,
                crate::domain::site::SslLevel::Ok => 2,
                crate::domain::site::SslLevel::None => 3,
            };
            (rank, *days_left)
        }
        crate::domain::site::SslStatus::None => (3, i64::MAX),
    }
}
