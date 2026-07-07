//! Shared test-only support utilities.

#[cfg(test)]
pub mod env {
    use std::sync::LazyLock;

    /// Global process environment lock for tests that mutate or depend on
    /// process-wide environment variables such as PATH, HOME, or provider auth.
    ///
    /// Rust tests run concurrently by default, but environment variables are
    /// process-global. Tests that temporarily clear PATH can make unrelated
    /// tests believe tools such as `pkl` are unavailable unless all such tests
    /// share one lock.
    pub static ENV_LOCK: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::new(()));

    pub async fn lock_async() -> tokio::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().await
    }

    pub fn lock() -> tokio::sync::MutexGuard<'static, ()> {
        ENV_LOCK.blocking_lock()
    }
}

#[cfg(test)]
pub mod cwd {
    use std::sync::LazyLock;

    /// Global current-working-directory lock for tests that call
    /// `std::env::set_current_dir`. The process CWD is shared across all test
    /// threads, and tempdir-backed CWDs disappear when their guard drops.
    pub static CWD_LOCK: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::new(()));

    pub async fn lock_async() -> tokio::sync::MutexGuard<'static, ()> {
        CWD_LOCK.lock().await
    }

    pub fn lock() -> tokio::sync::MutexGuard<'static, ()> {
        CWD_LOCK.blocking_lock()
    }
}
