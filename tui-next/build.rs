use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=NGTOOL_BUILD_VERSION");

    let version = std::env::var("NGTOOL_BUILD_VERSION")
        .ok()
        .map(|v| normalize_version(&v))
        .filter(|v| !v.is_empty())
        .or_else(version_from_git_tag)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    println!("cargo:rustc-env=NGTOOL_BUILD_VERSION={}", version);
}

fn version_from_git_tag() -> Option<String> {
    let out = Command::new("git")
        .args(["describe", "--tags", "--exact-match"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if tag.is_empty() {
        None
    } else {
        Some(normalize_version(&tag))
    }
}

fn normalize_version(input: &str) -> String {
    input.trim().trim_start_matches('v').to_string()
}
