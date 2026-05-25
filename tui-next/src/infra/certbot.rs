use crate::infra::executor::CommandSpec;
use crate::infra::permission;

/// certbot 是否可调用。详见 architecture.md §11.4。
/// 实际的 certbot certificates 解析在 P3/P8 阶段填充；此处仅做存在性检测。
pub fn certbot_available() -> bool {
    permission::which("certbot")
}

pub fn apply_registration_args(
    mut spec: CommandSpec,
    email: &str,
    allow_unsafe_without_email: bool,
) -> CommandSpec {
    spec = spec.arg("--non-interactive").arg("--agree-tos");
    let email = email.trim();
    if !email.is_empty() {
        spec.arg("--email").arg(email)
    } else if allow_unsafe_without_email {
        spec.arg("--register-unsafely-without-email")
    } else {
        spec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_args_use_email_when_present() {
        let spec = apply_registration_args(CommandSpec::new("certbot"), "ops@example.com", true);
        assert!(spec.args.iter().any(|arg| arg == "--email"));
        assert!(spec.args.iter().any(|arg| arg == "ops@example.com"));
        assert!(!spec
            .args
            .iter()
            .any(|arg| arg == "--register-unsafely-without-email"));
    }

    #[test]
    fn registration_args_fallback_to_unsafe_without_email() {
        let spec = apply_registration_args(CommandSpec::new("certbot"), "", true);
        assert!(spec
            .args
            .iter()
            .any(|arg| arg == "--register-unsafely-without-email"));
    }
}
