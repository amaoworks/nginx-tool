//! 日志实时跟踪（首版封装 tail -F），详见 architecture.md §11.5

use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

/// 日志跟踪任务发送的行消息
#[derive(Debug, Clone)]
pub struct TailLine {
    pub content: String,
}

/// 启动一个 tail -F 进程，持续向 channel 发送新行。
/// 返回一个 JoinHandle，调用方通过 abort() 取消。
/// 文件不存在时发送一条提示然后结束。
pub fn spawn_tail(
    path: PathBuf,
    tx: mpsc::UnboundedSender<TailLine>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let path_display = path.display().to_string();

        if !path.exists() {
            let _ = tx.send(TailLine {
                content: format!("（日志文件不存在：{}）", path_display),
            });
            return;
        }

        let result = Command::new("tail")
            .arg("-F")
            .arg("-n")
            .arg("100")
            .arg(&path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn();

        let mut child = match result {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(TailLine {
                    content: format!("（启动 tail 失败：{}）", e),
                });
                return;
            }
        };

        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                let _ = tx.send(TailLine {
                    content: "（无法获取 tail 输出）".into(),
                });
                return;
            }
        };

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx.send(TailLine { content: line });
            // 如果 channel 已关闭（接收方已丢弃），退出
            if tx.is_closed() {
                break;
            }
        }

        // tail -F 通常不会自行退出（文件被跟踪），如果到这里说明进程被杀或文件不可访问
        let _ = child.kill().await;
    })
}

/// 检查 tail 命令是否可用
pub async fn tail_available() -> bool {
    match Command::new("which").arg("tail").output().await {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}
