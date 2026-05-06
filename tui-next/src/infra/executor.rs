use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::NgToolError;

/// 命令规格，详见 architecture.md §13.1。
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
    pub timeout: Option<Duration>,
    pub require_root: bool,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: vec![],
            stdin: None,
            timeout: None,
            require_root: false,
        }
    }

    pub fn arg(mut self, a: impl Into<String>) -> Self {
        self.args.push(a.into());
        self
    }

    #[allow(dead_code)]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = Some(t);
        self
    }

    #[allow(dead_code)]
    pub fn with_stdin(mut self, s: impl Into<String>) -> Self {
        self.stdin = Some(s.into());
        self
    }

    #[allow(dead_code)]
    pub fn require_root(mut self) -> Self {
        self.require_root = true;
        self
    }

    pub fn cmdline(&self) -> String {
        let mut s = self.program.clone();
        for a in &self.args {
            s.push(' ');
            s.push_str(a);
        }
        s
    }
}

/// 命令执行结果。
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

impl CommandOutput {
    pub fn ok(&self) -> bool {
        self.status.success()
    }

    #[allow(dead_code)]
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }

    /// 取标准输出与标准错误的合并（标准输出在前），用于 nginx -t 等
    /// 把诊断写到 stderr、把成功提示写到 stderr 的命令。
    pub fn combined(&self) -> String {
        let mut s = self.stdout.clone();
        if !self.stderr.is_empty() {
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(&self.stderr);
        }
        s
    }
}

/// 统一外部命令执行器。所有外部命令必须经此封装，便于审计、超时与取消。
#[derive(Debug, Clone)]
pub struct CommandExecutor {
    default_timeout: Duration,
    is_root: bool,
}

impl CommandExecutor {
    pub fn new(default_timeout: Duration, is_root: bool) -> Self {
        Self {
            default_timeout,
            is_root,
        }
    }

    pub fn is_root(&self) -> bool {
        self.is_root
    }

    pub async fn run(&self, spec: CommandSpec) -> Result<CommandOutput, NgToolError> {
        if spec.require_root && !self.is_root {
            return Err(NgToolError::PermissionDenied {
                operation: spec.cmdline(),
            });
        }
        let timeout = spec.timeout.unwrap_or(self.default_timeout);
        let started = Instant::now();

        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        if spec.stdin.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| NgToolError::CommandFailed {
            command: spec.cmdline(),
            code: None,
            stderr: format!("启动失败：{}", e),
        })?;

        if let (Some(input), Some(mut stdin)) = (spec.stdin.clone(), child.stdin.take()) {
            let _ = stdin.write_all(input.as_bytes()).await;
            drop(stdin);
        }

        let result = tokio::time::timeout(timeout, child.wait_with_output()).await;
        let duration = started.elapsed();

        match result {
            Ok(Ok(out)) => Ok(CommandOutput {
                status: out.status,
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                duration,
            }),
            Ok(Err(e)) => Err(NgToolError::CommandFailed {
                command: spec.cmdline(),
                code: None,
                stderr: format!("等待失败：{}", e),
            }),
            Err(_) => Err(NgToolError::CommandFailed {
                command: spec.cmdline(),
                code: None,
                stderr: format!("超时（{}ms）", timeout.as_millis()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_echo() {
        let exec = CommandExecutor::new(Duration::from_secs(2), false);
        let out = exec
            .run(CommandSpec::new("/bin/echo").arg("hello"))
            .await
            .unwrap();
        assert!(out.ok());
        assert_eq!(out.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn run_timeout() {
        let exec = CommandExecutor::new(Duration::from_millis(50), false);
        let res = exec
            .run(
                CommandSpec::new("/bin/sleep")
                    .arg("2")
                    .timeout(Duration::from_millis(50)),
            )
            .await;
        assert!(res.is_err());
        match res.unwrap_err() {
            NgToolError::CommandFailed { stderr, .. } => assert!(stderr.contains("超时")),
            other => panic!("unexpected error {:?}", other),
        }
    }

    #[tokio::test]
    async fn require_root_blocked() {
        let exec = CommandExecutor::new(Duration::from_secs(2), false);
        let res = exec.run(CommandSpec::new("/bin/true").require_root()).await;
        assert!(matches!(res, Err(NgToolError::PermissionDenied { .. })));
    }

    #[tokio::test]
    async fn captures_nonzero_exit() {
        let exec = CommandExecutor::new(Duration::from_secs(2), false);
        let out = exec
            .run(CommandSpec::new("/bin/sh").arg("-c").arg("exit 7"))
            .await
            .unwrap();
        assert!(!out.ok());
        assert_eq!(out.status.code(), Some(7));
    }
}
