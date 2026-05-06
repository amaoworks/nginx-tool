use std::path::{Path, PathBuf};

/// 启动时的环境与依赖探测结果。详见 architecture.md §9。
#[derive(Debug, Clone)]
pub struct EnvironmentProbe {
    pub is_root: bool,
    pub nginx_root_readable: bool,
    pub nginx_root_writable: bool,
    pub sites_available: PathBuf,
    pub sites_enabled: PathBuf,
    pub deps: Dependencies,
}

#[derive(Debug, Clone)]
pub struct Dependencies {
    pub nginx: bool,
    pub systemctl: bool,
    pub certbot: bool,
}

const DEFAULT_NGINX_ROOT: &str = "/etc/nginx";

pub fn probe() -> EnvironmentProbe {
    let is_root = nix::unistd::geteuid().is_root();
    let nginx_root = Path::new(DEFAULT_NGINX_ROOT);
    let sites_available = nginx_root.join("sites-available");
    let sites_enabled = nginx_root.join("sites-enabled");
    let nginx_root_readable = sites_available.is_dir() && sites_enabled.is_dir();
    let nginx_root_writable = is_root && nginx_root_readable;

    EnvironmentProbe {
        is_root,
        nginx_root_readable,
        nginx_root_writable,
        sites_available,
        sites_enabled,
        deps: Dependencies {
            nginx: which("nginx"),
            systemctl: which("systemctl"),
            certbot: which("certbot"),
        },
    }
}

/// 简易 which：在 PATH 各目录中查找可执行文件。不引入 which crate。
pub fn which(cmd: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let p = Path::new(dir).join(cmd);
        if is_executable(&p) {
            return true;
        }
    }
    false
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    meta.permissions().mode() & 0o111 != 0
}

pub fn whoami() -> String {
    nix::unistd::User::from_uid(nix::unistd::geteuid())
        .ok()
        .flatten()
        .map(|u| u.name)
        .unwrap_or_else(|| "unknown".into())
}
