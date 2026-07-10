//! 仪表盘子状态。

use std::time::Instant;

use crate::domain::dashboard::DashboardSnapshot;

#[derive(Debug, Default)]
pub struct DashboardState {
    pub snapshot: Option<DashboardSnapshot>,
    pub last_refresh: Option<Instant>,
    pub refreshing: bool,
    pub pending_refresh: bool,
}
