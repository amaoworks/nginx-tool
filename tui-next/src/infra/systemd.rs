use std::time::Duration;

use crate::error::NgToolError;
use crate::infra::executor::{CommandExecutor, CommandSpec};

/// systemd 适配。所有命令统一带 `--no-pager`。
/// 详见 architecture.md §11.6。
#[derive(Debug, Clone)]
pub struct SystemdAdapter {
    exec: CommandExecutor,
}

impl SystemdAdapter {
    pub fn new(exec: CommandExecutor) -> Self {
        Self { exec }
    }

    pub async fn is_active(&self, unit: &str) -> Result<bool, NgToolError> {
        // is-active 返回非零并不等于失败：失败状态字符串依然可读
        let out = self
            .exec
            .run(
                CommandSpec::new("systemctl")
                    .arg("is-active")
                    .arg(unit)
                    .timeout(Duration::from_secs(3)),
            )
            .await?;
        Ok(out.stdout.trim() == "active")
    }

    pub async fn status(&self, unit: &str) -> Result<String, NgToolError> {
        let out = self
            .exec
            .run(
                CommandSpec::new("systemctl")
                    .arg("status")
                    .arg("--no-pager")
                    .arg(unit)
                    .timeout(Duration::from_secs(5)),
            )
            .await?;
        Ok(out.combined())
    }

    pub async fn reload(&self, unit: &str) -> Result<(), NgToolError> {
        let out = self
            .exec
            .run(
                CommandSpec::new("systemctl")
                    .arg("reload")
                    .arg(unit)
                    .timeout(Duration::from_secs(10))
                    .require_root(),
            )
            .await?;
        if !out.ok() {
            return Err(NgToolError::CommandFailed {
                command: format!("systemctl reload {}", unit),
                code: out.code(),
                stderr: out.combined(),
            });
        }
        Ok(())
    }

    pub async fn restart(&self, unit: &str) -> Result<(), NgToolError> {
        let out = self
            .exec
            .run(
                CommandSpec::new("systemctl")
                    .arg("restart")
                    .arg(unit)
                    .timeout(Duration::from_secs(15))
                    .require_root(),
            )
            .await?;
        if !out.ok() {
            return Err(NgToolError::CommandFailed {
                command: format!("systemctl restart {}", unit),
                code: out.code(),
                stderr: out.combined(),
            });
        }
        Ok(())
    }
}
