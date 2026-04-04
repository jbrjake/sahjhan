//! End-to-end integration tests for daemon signing and lifecycle.
//!
//! Each test starts a real daemon process, exercises it via the CLI or
//! raw socket, then tears it down. Tests are `#[ignore]` by default
//! because they spawn background processes and use real sockets.

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Stand up a temp directory with a minimal protocol config, run `sahjhan init`,
/// and return the owned TempDir (drop cleans it up).
fn setup_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"[protocol]
name = "test-daemon"
version = "1.0.0"
description = "Daemon test protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("states.toml"),
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n",
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n",
    )
    .unwrap();

    std::fs::write(config_dir.join("trusted-callers.toml"), "[callers]\n").unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

/// Spawn the daemon as a background process.
fn start_daemon(dir: &std::path::Path) -> std::process::Child {
    std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(["--config-dir", "enforcement", "daemon", "start"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start daemon")
}

/// Block until the daemon socket file appears (up to 5 seconds).
fn wait_for_socket(dir: &std::path::Path) {
    let socket_path = dir.join("output/.sahjhan/daemon.sock");
    for _ in 0..50 {
        if socket_path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("Daemon socket did not appear at {:?}", socket_path);
}

/// Kill the daemon and reap the child process.
fn stop_daemon(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

// ---------------------------------------------------------------------------
// Signing tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_sign_deterministic() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Sign the same inputs twice — proofs must match.
    let proof1 = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "sign",
            "--event-type",
            "test",
            "--field",
            "a=1",
            "--field",
            "b=2",
        ])
        .current_dir(dir.path())
        .output()
        .expect("sign command failed");
    assert!(proof1.status.success(), "sign exited non-zero");
    let p1 = String::from_utf8_lossy(&proof1.stdout).to_string();
    assert!(!p1.is_empty(), "proof should not be empty");

    let proof2 = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "sign",
            "--event-type",
            "test",
            "--field",
            "a=1",
            "--field",
            "b=2",
        ])
        .current_dir(dir.path())
        .output()
        .expect("sign command failed");
    assert!(proof2.status.success());
    let p2 = String::from_utf8_lossy(&proof2.stdout).to_string();

    assert_eq!(p1, p2, "same inputs must produce identical proofs");

    // Different fields → different proof.
    let proof3 = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "sign",
            "--event-type",
            "test",
            "--field",
            "a=1",
            "--field",
            "b=999",
        ])
        .current_dir(dir.path())
        .output()
        .expect("sign command failed");
    assert!(proof3.status.success());
    let p3 = String::from_utf8_lossy(&proof3.stdout).to_string();

    assert_ne!(p1, p3, "different inputs must produce different proofs");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_sign_fails_when_daemon_not_running() {
    let dir = setup_dir();
    // No daemon started — sign should fail.

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "sign",
            "--event-type",
            "test",
            "--field",
            "a=1",
        ])
        .current_dir(dir.path())
        .output()
        .expect("sign command failed to run");

    assert!(
        !output.status.success(),
        "sign should fail when daemon is not running"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("daemon is not running") || stderr.contains("not running"),
        "stderr should mention daemon not running, got: {}",
        stderr
    );
}

// ---------------------------------------------------------------------------
// Lifecycle tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_daemon_start_creates_socket_and_pid() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");
    let pid_path = dir.path().join("output/.sahjhan/daemon.pid");

    assert!(socket_path.exists(), "socket file should exist");
    assert!(pid_path.exists(), "PID file should exist");

    let pid_str = std::fs::read_to_string(&pid_path).unwrap();
    let pid: u32 = pid_str
        .trim()
        .parse()
        .expect("PID file should contain a number");
    assert!(pid > 0, "PID should be positive");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_daemon_stop_cleans_up() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");
    let pid_path = dir.path().join("output/.sahjhan/daemon.pid");

    // Confirm they exist before stopping.
    assert!(socket_path.exists());
    assert!(pid_path.exists());

    // Use CLI to stop the daemon.
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "daemon", "stop"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Wait a moment for cleanup.
    std::thread::sleep(std::time::Duration::from_millis(500));

    assert!(
        !socket_path.exists(),
        "socket file should be removed after stop"
    );
    assert!(!pid_path.exists(), "PID file should be removed after stop");

    // Reap the child process.
    let _ = daemon.wait();
}

#[test]
#[ignore]
fn test_daemon_status_request() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");

    // Connect directly to the socket and send a status request.
    let mut stream = UnixStream::connect(&socket_path).expect("connect to daemon socket");
    writeln!(stream, r#"{{"op": "status"}}"#).expect("write status request");

    let reader = BufReader::new(&stream);
    let response_line = reader
        .lines()
        .next()
        .expect("should get a response")
        .expect("response should be readable");

    let val: serde_json::Value =
        serde_json::from_str(&response_line).expect("response should be valid JSON");

    assert_eq!(val["ok"], true, "status response should be ok");
    let pid = val["pid"].as_u64().expect("pid should be a number");
    assert!(pid > 0, "pid should be positive");
    assert_eq!(
        val["vault_entries"].as_u64().unwrap(),
        0,
        "fresh daemon should have 0 vault entries"
    );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_daemon_rejects_preload_env() {
    let dir = setup_dir();

    // Use LD_PRELOAD only. On Linux this is the real preload variable.
    // On macOS, LD_PRELOAD is ignored by dyld but our check_preload_env()
    // still reads it from the process environment, so the daemon refuses.
    // We do NOT set DYLD_INSERT_LIBRARIES because macOS dyld would try to
    // load the library and terminate the process before main() runs.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(["--config-dir", "enforcement", "daemon", "start"])
        .current_dir(dir.path())
        .env("LD_PRELOAD", "/tmp/evil.so")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("daemon command should run");

    assert!(
        !output.status.success(),
        "daemon should refuse to start with preload env set"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("LD_PRELOAD")
            || stderr.contains("DYLD_INSERT_LIBRARIES")
            || stderr.contains("preload"),
        "stderr should mention preload rejection, got: {}",
        stderr
    );
}

// ---------------------------------------------------------------------------
// Sign → authed-event end-to-end test
// ---------------------------------------------------------------------------

/// Setup dir with events.toml containing a restricted event type.
fn setup_signing_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"[protocol]
name = "test-signing"
version = "1.0.0"
description = "Signing test protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("states.toml"),
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n",
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n",
    )
    .unwrap();

    std::fs::write(
        config_dir.join("events.toml"),
        r#"
[events.quiz_answered]
description = "Quiz result"
restricted = true
fields = [
    { name = "score", type = "string" },
    { name = "pass", type = "string" },
]
"#,
    )
    .unwrap();

    std::fs::write(config_dir.join("trusted-callers.toml"), "[callers]\n").unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

#[test]
#[ignore]
fn test_sign_then_authed_event_full_flow() {
    let dir = setup_signing_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Sign
    let sign_output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "sign",
            "--event-type",
            "quiz_answered",
            "--field",
            "score=5",
            "--field",
            "pass=true",
        ])
        .current_dir(dir.path())
        .output()
        .expect("sign command failed");
    assert!(sign_output.status.success(), "sign should succeed");
    let proof = String::from_utf8_lossy(&sign_output.stdout)
        .trim()
        .to_string();
    assert!(!proof.is_empty(), "proof should not be empty");

    // Authed-event with that proof
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "authed-event",
            "quiz_answered",
            "--field",
            "score=5",
            "--field",
            "pass=true",
            "--proof",
            &proof,
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("recorded: quiz_answered"));

    stop_daemon(&mut daemon);
}
