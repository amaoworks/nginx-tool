use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use crossterm::event::{Event as CtEvent, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;

mod app;
mod config;
mod domain;
mod error;
mod infra;
mod template;
mod ui;
mod version;

use crate::app::event::AppEvent;
use crate::app::state::AppState;
use crate::infra::{bootstrap, AppContext, BootstrapOptions};

type Tui = Terminal<CrosstermBackend<Stdout>>;

#[derive(Debug, Parser)]
#[command(
    name = "ngtool",
    version = crate::version::APP_VERSION,
    about = "Nginx-Tools TUI - 交互式 Nginx 站点管理工具",
    disable_help_subcommand = true
)]
struct Cli {
    /// 以只读模式启动，禁止所有写操作
    #[arg(long, default_value_t = false)]
    readonly: bool,

    /// 指定配置文件路径，默认为 ~/.local/ngtool/config.toml
    #[arg(long, value_name = "PATH")]
    config: Option<std::path::PathBuf>,
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        original(info);
    }));
}

fn enter_terminal() -> anyhow::Result<Tui> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    terminal.clear().ok();
    Ok(terminal)
}

fn restore_terminal() -> anyhow::Result<()> {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen);
    disable_raw_mode()?;
    Ok(())
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    install_panic_hook();

    let ctx = Arc::new(bootstrap(BootstrapOptions {
        force_readonly: cli.readonly,
        config_override: cli.config.clone(),
    })?);

    let mut terminal = enter_terminal()?;
    let result = run(&mut terminal, ctx).await;
    let _ = restore_terminal();
    result
}

/// 仪表盘整体超时上限：架构 §11.2。
const DASHBOARD_TOTAL_BUDGET: Duration = Duration::from_secs(5);

async fn run(terminal: &mut Tui, ctx: Arc<AppContext>) -> anyhow::Result<()> {
    let mut state = AppState::new(ctx.clone());
    let mut events = EventStream::new();

    // 异步任务结果回灌：用 mpsc 通道，由后台任务向主循环投递 AppEvent
    let (task_tx, mut task_rx) = mpsc::unbounded_channel::<AppEvent>();

    let mut tick = tokio::time::interval(Duration::from_millis(250));

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sighup = signal(SignalKind::hangup())?;

    loop {
        terminal.draw(|f| ui::draw(f, &state))?;

        let app_event: Option<AppEvent> = tokio::select! {
            evt = events.next() => match evt {
                Some(Ok(CtEvent::Key(k))) if k.kind == KeyEventKind::Press => Some(AppEvent::Key(k)),
                Some(Ok(CtEvent::Resize(_, _))) => Some(AppEvent::Resize),
                Some(Ok(_)) => None,
                Some(Err(_)) => None,
                None => Some(AppEvent::QuitRequested),
            },
            _ = tick.tick() => Some(AppEvent::Tick),
            ev = task_rx.recv() => ev,
            _ = sigint.recv() => Some(AppEvent::QuitRequested),
            _ = sigterm.recv() => Some(AppEvent::QuitRequested),
            _ = sighup.recv() => Some(AppEvent::QuitRequested),
        };

        if let Some(ev) = app_event {
            state.handle_event(ev);
        }

        state.expire_notification_if_due();

        // 派发待执行的异步意图：仪表盘刷新
        if state.take_dashboard_refresh_request() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let snap = match tokio::time::timeout(
                    DASHBOARD_TOTAL_BUDGET,
                    domain::dashboard::collect(ctx_clone.clone()),
                )
                .await
                {
                    Ok(s) => s,
                    Err(_) => domain::dashboard::collect(ctx_clone).await, // 极端情况：内层超时已生效；保留兜底
                };
                let _ = tx_clone.send(AppEvent::DashboardSnapshot(Box::new(snap)));
            });
        }

        // 派发待执行的异步意图：站点列表加载
        if state.take_sites_refresh_request() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let result = domain::site::list_sites(ctx_clone)
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx_clone.send(AppEvent::SitesLoaded(Box::new(result)));
            });
        }

        // 派发待执行的异步意图：站点启停
        if let Some((site_name, target_enabled)) = state.take_site_toggle_request() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            let name = site_name.clone();
            tokio::spawn(async move {
                let result = if target_enabled {
                    domain::site::enable(ctx_clone, &name).await
                } else {
                    domain::site::disable(ctx_clone, &name).await
                };
                let _ = tx_clone.send(AppEvent::SiteToggleResult {
                    site: name,
                    target_enabled,
                    result: Box::new(result),
                });
            });
        }

        // 派发待执行的异步意图：删除站点
        if let Some(site_name) = state.take_site_delete_request() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            let name = site_name.clone();
            tokio::spawn(async move {
                let result = domain::site::delete_site(ctx_clone, &name).await;
                let _ = tx_clone.send(AppEvent::SiteDeleteResult {
                    site_name: name,
                    result: Box::new(result),
                });
            });
        }

        // 派发待执行的异步意图：服务控制
        if let Some(btn) = state.take_service_action() {
            use crate::app::state::ServiceButton;
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                match btn {
                    ServiceButton::Test => {
                        let r = domain::service::test_config(&ctx_clone).await;
                        let _ = tx_clone.send(AppEvent::ServiceTestResult(Box::new(r)));
                    }
                    ServiceButton::Reload => {
                        let r = domain::service::reload(&ctx_clone)
                            .await
                            .map(|o| o.test_output);
                        let _ = tx_clone.send(AppEvent::ServiceReloadResult(Box::new(r)));
                    }
                    ServiceButton::Restart => {
                        let r = domain::service::restart(&ctx_clone).await;
                        let _ = tx_clone.send(AppEvent::ServiceRestartResult(Box::new(r)));
                    }
                    ServiceButton::Status => {
                        let r = domain::service::status(&ctx_clone).await;
                        let _ = tx_clone.send(AppEvent::ServiceStatusResult(Box::new(r)));
                    }
                    ServiceButton::CheckUpdate => {
                        let r = domain::update::check_latest_release().await;
                        let _ = tx_clone.send(AppEvent::ServiceUpdateCheckResult(Box::new(r)));
                    }
                }
            });
        }

        // 派发弹窗确认后的 TUI 升级
        if state.take_service_upgrade() {
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let r = domain::update::upgrade_to_latest_release().await;
                let _ = tx_clone.send(AppEvent::ServiceUpgradeResult(Box::new(r)));
            });
        }

        // 派发待执行的异步意图：新建站点
        if let Some(input) = state.take_site_create_request() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            let name = input.name.clone();
            tokio::spawn(async move {
                let result = domain::site::create_site(ctx_clone, input).await;
                let _ = tx_clone.send(AppEvent::SiteCreateResult {
                    site_name: name,
                    result: Box::new(result),
                });
            });
        }

        // 派发待执行的异步意图：保存站点配置
        if let Some(input) = state.take_site_save_request() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            let name = input.name.clone();
            let saved_content = input.content.clone();
            tokio::spawn(async move {
                let result = domain::site::save_site_config(ctx_clone, input).await;
                let _ = tx_clone.send(AppEvent::SiteEditResult {
                    site_name: name,
                    saved_content,
                    result: Box::new(result),
                });
            });
        }

        // 派发待执行的异步意图：证书页一次刷新
        if state.take_certs_refresh_request() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let snap = domain::cert::collect_snapshot(ctx_clone).await;
                let _ = tx_clone.send(AppEvent::CertsSnapshot(Box::new(snap)));
            });
        }

        // 派发待执行的异步意图：证书申请
        if let Some((site_name, domains)) = state.take_cert_request() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            let name = site_name.clone();
            tokio::spawn(async move {
                let result = domain::cert::request_cert(ctx_clone, &name, &domains).await;
                let _ = tx_clone.send(AppEvent::CertRequestResult {
                    site_name: name,
                    result: Box::new(result),
                });
            });
        }

        // 派发待执行的异步意图：续期所有证书
        if state.take_cert_renew_all() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let result = domain::cert::renew_all(ctx_clone).await;
                let _ = tx_clone.send(AppEvent::CertRenewAllResult(Box::new(result)));
            });
        }

        // 派发待执行的异步意图：自动续签状态检查
        if state.take_cert_check_auto_renew() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let status = domain::cert::check_auto_renew(ctx_clone).await;
                let _ = tx_clone.send(AppEvent::CertAutoRenewResult(Box::new(status)));
            });
        }

        if state.take_cert_install_hook() {
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let result = domain::cert::install_deploy_hook();
                let _ = tx_clone.send(AppEvent::CertInstallHookResult(Box::new(result)));
            });
        }

        // 派发待执行的异步意图：删除证书
        if let Some(cert_name) = state.take_cert_delete() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            let name = cert_name.clone();
            tokio::spawn(async move {
                let result = domain::cert::delete_cert(ctx_clone, &name).await;
                let _ = tx_clone.send(AppEvent::CertDeleteResult {
                    cert_name: name,
                    result: Box::new(result),
                });
            });
        }

        // 派发待执行的异步意图：备份列表刷新
        if state.take_backup_refresh_request() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let result =
                    tokio::task::spawn_blocking(move || domain::backup::list_backups(&ctx_clone))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r.map_err(|e| e.to_string()));
                let _ = tx_clone.send(AppEvent::BackupListLoaded(Box::new(result)));
            });
        }

        // 派发待执行的异步意图：创建备份
        if state.take_backup_create() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let result = domain::backup::create_backup(
                    ctx_clone,
                    domain::backup::CreateBackupInput {
                        source: domain::backup::BackupSource::Manual,
                    },
                )
                .await;
                let _ = tx_clone.send(AppEvent::BackupCreateResult(Box::new(result)));
            });
        }

        // 派发待执行的异步意图：删除备份
        if let Some(path) = state.take_backup_delete() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let result = domain::backup::delete_backup(ctx_clone, path).await;
                let _ = tx_clone.send(AppEvent::BackupDeleteResult(Box::new(result)));
            });
        }

        // 派发待执行的异步意图：还原备份
        if let Some(path) = state.take_backup_restore() {
            let ctx_clone = ctx.clone();
            let tx_clone = task_tx.clone();
            tokio::spawn(async move {
                let result = domain::backup::restore_backup(ctx_clone, path).await;
                let _ = tx_clone.send(AppEvent::BackupRestoreResult(Box::new(result)));
            });
        }

        // 处理日志源变更：停止旧 tail，启动新 tail
        if state.take_logs_tail_change_request() {
            // 停止旧任务
            state.logs.stop_tail();
            // 创建新通道
            let (tail_tx, tail_rx) = mpsc::unbounded_channel::<crate::infra::log_tail::TailLine>();
            state.logs.tail_rx = Some(tail_rx);
            // 启动新 tail 任务
            let path = state.logs.source.path();
            let handle = crate::infra::log_tail::spawn_tail(path, tail_tx);
            state.logs.tail_handle = Some(handle);
        }

        // 从 tail 通道接收新行
        let lines_to_process: Vec<String> = {
            let mut lines = Vec::new();
            if let Some(ref mut rx) = state.logs.tail_rx {
                while let Ok(line) = rx.try_recv() {
                    lines.push(line.content);
                }
            }
            lines
        };
        for line in lines_to_process {
            state.handle_event(AppEvent::LogTailLine { line });
        }

        if state.should_quit {
            break;
        }
    }

    Ok(())
}
