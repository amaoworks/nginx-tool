use crate::infra::permission;

/// certbot 是否可调用。详见 architecture.md §11.4。
/// 实际的 certbot certificates 解析在 P3/P8 阶段填充；此处仅做存在性检测。
pub fn certbot_available() -> bool {
    permission::which("certbot")
}
