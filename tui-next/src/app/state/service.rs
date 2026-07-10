//! 服务控制子状态。

use crate::domain::update::UpdateInfo;

/// 服务控制视图所选按钮。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ServiceButton {
    #[default]
    Test,
    Reload,
    Restart,
    Status,
    CheckUpdate,
}

impl ServiceButton {
    pub const ALL: [ServiceButton; 5] = [
        ServiceButton::Test,
        ServiceButton::Reload,
        ServiceButton::Restart,
        ServiceButton::Status,
        ServiceButton::CheckUpdate,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            ServiceButton::Test => "测试配置 (nginx -t)",
            ServiceButton::Reload => "重载配置",
            ServiceButton::Restart => "重启服务",
            ServiceButton::Status => "查看状态",
            ServiceButton::CheckUpdate => "检查 ngtool 更新",
        }
    }
}

/// 服务控制视图子状态。
#[derive(Debug, Default)]
pub struct ServiceState {
    pub focused: ServiceButton,
    pub output: Vec<String>,
    pub running: Option<ServiceButton>,
    pub update_info: Option<UpdateInfo>,
    /// 待派发的操作意图
    pub pending_action: Option<ServiceButton>,
    /// 弹窗确认后的升级意图
    pub pending_upgrade: bool,
}

impl ServiceState {
    pub fn move_focus(&mut self, delta: i32) {
        let len = ServiceButton::ALL.len() as i32;
        let cur = ServiceButton::ALL
            .iter()
            .position(|b| *b == self.focused)
            .map(|x| x as i32)
            .unwrap_or(0);
        let next = (cur + delta).rem_euclid(len) as usize;
        self.focused = ServiceButton::ALL[next];
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
