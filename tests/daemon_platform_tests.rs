//! Platform API compile and basic smoke tests.

/// Verify that check_preload_env returns None in a clean test environment.
#[test]
fn test_check_preload_env_clean() {
    let result = sahjhan::daemon::platform::check_preload_env();
    assert!(
        result.is_none(),
        "Expected no preload env, got {:?}",
        result
    );
}

/// Verify that get_exe_path works for the current process.
#[test]
fn test_get_exe_path_self() {
    let pid = std::process::id();
    let path = sahjhan::daemon::platform::get_exe_path(pid).unwrap();
    assert!(path.exists(), "Exe path {:?} should exist", path);
}

/// Verify that get_cmdline works for the current process.
#[test]
fn test_get_cmdline_self() {
    let pid = std::process::id();
    let args = sahjhan::daemon::platform::get_cmdline(pid).unwrap();
    assert!(!args.is_empty(), "Should have at least one arg");
}

/// Verify that get_parent_pid works for the current process.
#[test]
fn test_get_parent_pid_self() {
    let pid = std::process::id();
    let ppid = sahjhan::daemon::platform::get_parent_pid(pid).unwrap();
    assert!(ppid > 0, "Parent PID should be positive");
}

/// Verify try_mlock best-effort behavior (may fail in CI containers).
#[test]
fn test_try_mlock_best_effort() {
    let data = [0u8; 64];
    let _ = sahjhan::daemon::platform::try_mlock(data.as_ptr(), data.len());
}
