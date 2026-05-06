use std::path::PathBuf;
use thiserror::Error;

// 错误类型在 P2 系统适配层及之后逐步引入，此处先建立类型契约
#[allow(dead_code)]
#[derive(Debug, Clone, Error)]
pub enum NgToolError {
    #[error("权限不足，无法执行操作: {operation}")]
    PermissionDenied { operation: String },

    #[error("缺少依赖: {name}")]
    DependencyMissing { name: String },

    #[error("输入无效（{field}）：{message}")]
    InvalidInput { field: String, message: String },

    #[error("Nginx 配置测试失败：{output}")]
    NginxTestFailed { output: String },

    #[error("命令执行失败：{command}（退出码 {code:?}）{stderr}")]
    CommandFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },

    #[error("文件操作失败：{path}：{message}")]
    FileOperationFailed { path: PathBuf, message: String },

    #[error("模板渲染失败：{message}")]
    TemplateFailed { message: String },

    #[error("解析失败（{target}）：{message}")]
    ParseFailed { target: String, message: String },

    #[error("操作已取消")]
    Cancelled,
}

#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, NgToolError>;
