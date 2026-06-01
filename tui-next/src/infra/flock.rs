use std::fs::{File, OpenOptions};
use std::path::Path;

/// 单实例软锁。架构 §15.0：启动时获取 ~/.local/ngtool/tui.lock 上的独占非阻塞 flock。
/// 占用方释放（进程退出 / Drop）时自动失效。
#[derive(Debug)]
pub struct SingleInstanceLock {
    _file: File,
}

#[derive(Debug)]
pub enum LockState {
    Acquired(SingleInstanceLock),
    Busy,
}

impl SingleInstanceLock {
    pub fn try_acquire(path: &Path) -> std::io::Result<LockState> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;

        // 使用 libc::flock 直接调用，避免随 nix 版本变迁的 API 差异
        use std::os::unix::io::AsRawFd;
        const LOCK_EX: i32 = 2;
        const LOCK_NB: i32 = 4;
        let fd = file.as_raw_fd();
        // SAFETY: 调用 libc 提供的 flock，参数语义见 flock(2)
        let r = unsafe { flock_raw(fd, LOCK_EX | LOCK_NB) };
        if r == 0 {
            Ok(LockState::Acquired(Self { _file: file }))
        } else {
            let err = std::io::Error::last_os_error();
            // EWOULDBLOCK / EAGAIN：他人占用
            if matches!(
                err.raw_os_error(),
                Some(libc_ewouldblock) if libc_ewouldblock == 11 || libc_ewouldblock == 35
            ) {
                Ok(LockState::Busy)
            } else {
                Err(err)
            }
        }
    }
}

extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

#[inline]
unsafe fn flock_raw(fd: i32, op: i32) -> i32 {
    flock(fd, op)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_is_busy() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("test.lock");
        let first = SingleInstanceLock::try_acquire(&lock).unwrap();
        assert!(matches!(first, LockState::Acquired(_)));
        let second = SingleInstanceLock::try_acquire(&lock).unwrap();
        assert!(matches!(second, LockState::Busy));
        drop(second);
        drop(first);
        // 第一把释放后第二次可再次获取
        let third = SingleInstanceLock::try_acquire(&lock).unwrap();
        assert!(matches!(third, LockState::Acquired(_)));
    }
}
