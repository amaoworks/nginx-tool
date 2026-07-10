//! 证书页子状态。

use std::collections::VecDeque;
use std::time::Instant;

/// 证书页焦点（仅两区：站点表 / 全局维护）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CertsFocus {
    /// 站点列表（↑↓ 选中站点，Enter 申请证书）
    #[default]
    Table,
    /// 全局维护操作（续期 / 自动续签 / hook / 清理）
    GlobalActions,
}

/// 证书页操作按钮
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CertsAction {
    #[default]
    Request,
    RenewAll,
    CheckAutoRenew,
    InstallDeployHook,
    DeleteOrphan,
}

impl CertsAction {
    /// 底部「全局维护」按钮（与选中站点无关）
    pub const GLOBAL_ACTIONS: [CertsAction; 4] = [
        CertsAction::RenewAll,
        CertsAction::CheckAutoRenew,
        CertsAction::InstallDeployHook,
        CertsAction::DeleteOrphan,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            CertsAction::Request => "申请证书",
            CertsAction::RenewAll => "续期全部",
            CertsAction::CheckAutoRenew => "检查续签",
            CertsAction::InstallDeployHook => "安装重载钩子",
            CertsAction::DeleteOrphan => "清理多余",
        }
    }
}

/// 证书页子状态
#[derive(Debug, Default)]
pub struct CertsState {
    pub list: Vec<crate::domain::cert::CertWithSite>,
    /// certbot 原始输出，解析失败时供 UI 展示（R2）
    pub raw_output: Option<String>,
    pub auto_renew: Option<crate::domain::cert::AutoRenewStatus>,
    pub last_refresh: Option<Instant>,
    pub refreshing: bool,
    pub pending_refresh: bool,
    pub last_error: Option<String>,
    pub focused: CertsFocus,
    /// 当前选中的站点索引（依赖 SitesState.list）
    pub site_selector_index: usize,
    /// 全局维护按钮当前焦点
    pub action_focus: CertsAction,
    /// 操作输出区，展示 certbot 流式输出
    pub output: Vec<String>,
    /// 操作进行中标志
    pub running: Option<CertsAction>,
    /// 待派发：申请证书（站点名 + 域名列表）
    pub pending_request: Option<(String, Vec<String>)>,
    /// 待派发：续签全部
    pub pending_renew: bool,
    /// 待派发：自动续签检查
    pub pending_check_renew: bool,
    /// 待派发：安装 deploy hook
    pub pending_install_hook: bool,
    /// 待派发：删除证书队列（证书名）
    pub pending_delete: VecDeque<String>,
    /// 删除队列中当前正在执行的证书名
    pub delete_in_flight: Option<String>,
}

impl CertsState {
    pub fn cycle_global_action(&mut self, delta: i32) {
        let actions = &CertsAction::GLOBAL_ACTIONS;
        let len = actions.len() as i32;
        let cur = actions
            .iter()
            .position(|a| *a == self.action_focus)
            .map(|x| x as i32)
            .unwrap_or(0);
        let next = (cur + delta).rem_euclid(len) as usize;
        self.action_focus = actions[next];
    }

    /// 确保全局焦点落在 GLOBAL_ACTIONS 内（从表切过来时可能还是 Request）
    pub fn ensure_global_action_focus(&mut self) {
        if !CertsAction::GLOBAL_ACTIONS.contains(&self.action_focus) {
            self.action_focus = CertsAction::RenewAll;
        }
    }

    pub fn push_output(&mut self, lines: impl IntoIterator<Item = String>) {
        let limit = 200usize;
        for line in lines {
            self.output.push(line);
        }
        if self.output.len() > limit {
            let drop = self.output.len() - limit;
            self.output.drain(0..drop);
        }
    }

    pub fn clear_output(&mut self) {
        self.output.clear();
    }
}
