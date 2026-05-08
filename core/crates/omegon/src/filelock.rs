//! Advisory file locking and atomic writes for concurrent instance safety.
//!
//! Two Omegon instances (ACP in Flynt, TUI in terminal) may run in the same
//! directory. Shared mutable files (profile, extension state, workspace
//! registry) need coordination to prevent last-write-wins data loss.
//!
//! This module provides two primitives:
//!   - `atomic_write(path, content)` — temp file + rename (crash-safe)
//!   - `atomic_write_locked(path, content)` — advisory flock + atomic write
//!     (crash-safe AND concurrent-safe)

use std::path::{Path, PathBuf};

/// RAII guard for an advisory file lock. Releases the lock on drop.
#[cfg(unix)]
pub struct FileLockGuard {
    fd: std::os::unix::io::RawFd,
    _lock_path: PathBuf,
}

#[cfg(unix)]
impl Drop for FileLockGuard {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.fd, libc::LOCK_UN);
            libc::close(self.fd);
        }
    }
}

/// Acquire an exclusive advisory lock on `<path>.lock`.
///
/// Non-blocking with retry: tries `LOCK_EX | LOCK_NB`, retries with
/// exponential backoff (100ms → 200ms → 400ms → 800ms → 2000ms ≈ 3.5s).
/// The kernel releases the lock automatically if the process exits or crashes.
#[cfg(unix)]
pub fn acquire_lock(path: &Path) -> anyhow::Result<FileLockGuard> {
    use std::os::unix::io::IntoRawFd;

    let lock_path = lock_path_for(path);
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    let fd = file.into_raw_fd();

    let backoffs = [100, 200, 400, 800, 2000];
    for (attempt, delay_ms) in backoffs.iter().enumerate() {
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if ret == 0 {
            return Ok(FileLockGuard { fd, _lock_path: lock_path });
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::WouldBlock {
            unsafe { libc::close(fd); }
            return Err(anyhow::anyhow!("flock failed on {}: {err}", lock_path.display()));
        }
        tracing::debug!(
            path = %lock_path.display(),
            attempt = attempt + 1,
            "lock contended, retrying in {delay_ms}ms"
        );
        std::thread::sleep(std::time::Duration::from_millis(*delay_ms));
    }

    // Final blocking attempt after all retries exhausted
    let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
    if ret == 0 {
        Ok(FileLockGuard { fd, _lock_path: lock_path })
    } else {
        let err = std::io::Error::last_os_error();
        unsafe { libc::close(fd); }
        Err(anyhow::anyhow!("flock timed out on {}: {err}", lock_path.display()))
    }
}

#[cfg(not(unix))]
pub struct FileLockGuard {
    _lock_path: PathBuf,
}

#[cfg(not(unix))]
pub fn acquire_lock(path: &Path) -> anyhow::Result<FileLockGuard> {
    tracing::warn!(
        path = %path.display(),
        "advisory file locking not available on this platform"
    );
    Ok(FileLockGuard { _lock_path: lock_path_for(path) })
}

fn lock_path_for(path: &Path) -> PathBuf {
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    PathBuf::from(lock)
}

/// Atomic write: write to `<path>.tmp`, rename to `<path>`.
///
/// rename() is atomic on POSIX — readers never see a partial file.
/// No inter-process coordination; use `atomic_write_locked` when
/// concurrent writers are possible.
pub fn atomic_write(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Atomic write with advisory lock: acquire flock, write temp, rename, release.
///
/// This is the recommended write primitive for any shared mutable file
/// (profile.json, extension state.toml, workspace registry).
pub fn atomic_write_locked(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    let _guard = acquire_lock(path)?;
    atomic_write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn atomic_write_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.json");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn atomic_write_no_temp_file_left() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.json");
        atomic_write(&path, b"content").unwrap();
        let tmp = path.with_extension("tmp");
        assert!(!tmp.exists());
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.json");
        std::fs::write(&path, "old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn atomic_write_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("deep").join("test.json");
        atomic_write(&path, b"nested content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "nested content");
    }

    #[cfg(unix)]
    #[test]
    fn lock_acquire_and_release() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("target.json");
        std::fs::write(&path, "data").unwrap();

        let guard = acquire_lock(&path).unwrap();
        let lock_file = lock_path_for(&path);
        assert!(lock_file.exists());
        drop(guard);
    }

    #[cfg(unix)]
    #[test]
    fn lock_reacquire_after_release() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("target.json");

        let guard = acquire_lock(&path).unwrap();
        drop(guard);

        let guard2 = acquire_lock(&path).unwrap();
        drop(guard2);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_locked_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("locked.json");

        atomic_write_locked(&path, b"{\"key\": \"value\"}").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "{\"key\": \"value\"}");

        atomic_write_locked(&path, b"{\"key\": \"updated\"}").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "{\"key\": \"updated\"}");
    }

    #[test]
    fn lock_path_for_appends_lock_extension() {
        let p = PathBuf::from("/some/file.json");
        assert_eq!(lock_path_for(&p), PathBuf::from("/some/file.json.lock"));
    }

    #[test]
    fn lock_path_for_handles_no_extension() {
        let p = PathBuf::from("/some/file");
        assert_eq!(lock_path_for(&p), PathBuf::from("/some/file.lock"));
    }
}
