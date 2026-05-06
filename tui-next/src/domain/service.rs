//! 服务控制相关 use case，对应 design.md 视图 5、architecture.md §11.6。
//! P7 先行任务：本模块在 P4 站点启停之前必须可用，作为共用依赖。

use std::time::Instant;

use serde_json::json;

use crate::error::NgToolError;
use crate::infra::audit::AuditResult;
use crate::infra::AppContext;

/// `nginx -t` 测试配置。返回测试输出（成功或失败均含完整诊断）。
/// 详见 architecture.md §11.6 / 约束清单 #4：UI 不直接执行系统命令。
pub async fn test_config(ctx: &AppContext) -> Result<String, NgToolError> {
    let started = Instant::now();
    let result = ctx.nginx.test_config().await;
    let duration_ms = started.elapsed().as_millis() as u64;
    match &result {
        Ok(_) => ctx.audit.log(
            "service.test",
            "nginx",
            AuditResult::Success,
            duration_ms,
            json!({}),
        ),
        Err(e) => ctx.audit.log(
            "service.test",
            "nginx",
            AuditResult::Failure,
            duration_ms,
            json!({"error": e.to_string()}),
        ),
    }
    result
}

/// 重载配置：先 `nginx -t`，通过后 `systemctl reload nginx`。
/// **不**通过 shell `&&` 串联（架构约束 §11.6）；分步执行，分别记录。
pub async fn reload(ctx: &AppContext) -> Result<ReloadOutcome, NgToolError> {
    let started = Instant::now();

    // 第一步：测试配置。失败时不进入 reload。
    let test_output = match ctx.nginx.test_config().await {
        Ok(out) => out,
        Err(e) => {
            ctx.audit.log(
                "service.reload",
                "nginx",
                AuditResult::Failure,
                started.elapsed().as_millis() as u64,
                json!({"stage": "test", "error": e.to_string()}),
            );
            return Err(e);
        }
    };

    // 第二步：reload。
    if let Err(e) = ctx.systemd.reload("nginx").await {
        ctx.audit.log(
            "service.reload",
            "nginx",
            AuditResult::Failure,
            started.elapsed().as_millis() as u64,
            json!({"stage": "reload", "error": e.to_string()}),
        );
        return Err(e);
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    ctx.audit.log(
        "service.reload",
        "nginx",
        AuditResult::Success,
        duration_ms,
        json!({}),
    );
    Ok(ReloadOutcome { test_output })
}

/// 重启服务（高危）。UI 必须先经确认弹窗。详见 design.md 视图 5。
pub async fn restart(ctx: &AppContext) -> Result<(), NgToolError> {
    let started = Instant::now();
    let result = ctx.systemd.restart("nginx").await;
    let duration_ms = started.elapsed().as_millis() as u64;
    match &result {
        Ok(_) => ctx.audit.log(
            "service.restart",
            "nginx",
            AuditResult::Success,
            duration_ms,
            json!({}),
        ),
        Err(e) => ctx.audit.log(
            "service.restart",
            "nginx",
            AuditResult::Failure,
            duration_ms,
            json!({"error": e.to_string()}),
        ),
    }
    result
}

/// `systemctl status nginx --no-pager` 输出。
pub async fn status(ctx: &AppContext) -> Result<String, NgToolError> {
    ctx.systemd.status("nginx").await
}

#[derive(Debug, Clone)]
pub struct ReloadOutcome {
    pub test_output: String,
}
