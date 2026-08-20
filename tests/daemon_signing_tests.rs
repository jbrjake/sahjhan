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

    // No trusted-callers.toml — caller auth unconfigured; the daemon accepts
    // test connections (a present manifest is enforced; an empty one denies).

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

/// Spawn the daemon with extra CLI args.
fn start_daemon_with_args(dir: &std::path::Path, extra_args: &[&str]) -> std::process::Child {
    let mut args = vec!["--config-dir", "enforcement", "daemon", "start"];
    args.extend_from_slice(extra_args);
    std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(&args)
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
fn test_daemon_socket_env_override() {
    let dir = setup_dir();

    // Socket lives outside the project directory. Keep the path short:
    // AF_UNIX paths are capped at 104 bytes on macOS, and tempdir paths
    // under /var/folders easily exceed that.
    let sock_dir = tempfile::Builder::new()
        .prefix("sjc1")
        .tempdir_in("/tmp")
        .unwrap();
    let sock_path = sock_dir.path().join("d.sock");

    let mut daemon = std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(["--config-dir", "enforcement", "daemon", "start"])
        .current_dir(dir.path())
        .env("SAHJHAN_DAEMON_SOCKET", &sock_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start daemon");

    for _ in 0..50 {
        if sock_path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(
        sock_path.exists(),
        "socket should appear at the override path"
    );
    assert!(
        !dir.path().join("output/.sahjhan/daemon.sock").exists(),
        "no socket should appear at the default in-project path"
    );
    // The PID file stays in data_dir regardless of the socket override.
    assert!(dir.path().join("output/.sahjhan/daemon.pid").exists());

    // `daemon status` resolves the same override and reaches the daemon.
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "daemon", "status"])
        .current_dir(dir.path())
        .env("SAHJHAN_DAEMON_SOCKET", &sock_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"));

    // `daemon stop` cleans up the override socket.
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "daemon", "stop"])
        .current_dir(dir.path())
        .env("SAHJHAN_DAEMON_SOCKET", &sock_path)
        .assert()
        .success();
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!sock_path.exists(), "override socket removed after stop");

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

    // No trusted-callers.toml — caller auth unconfigured; the daemon accepts
    // test connections (a present manifest is enforced; an empty one denies).

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

// ---------------------------------------------------------------------------
// Idle timeout tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_daemon_idle_timeout_clean_shutdown() {
    let dir = setup_dir();
    let mut daemon = start_daemon_with_args(dir.path(), &["--idle-timeout", "1"]);
    wait_for_socket(dir.path());

    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");
    let pid_path = dir.path().join("output/.sahjhan/daemon.pid");

    // Confirm daemon is running.
    assert!(socket_path.exists());
    assert!(pid_path.exists());

    // Wait for idle timeout to fire (1s timeout + margin).
    std::thread::sleep(std::time::Duration::from_secs(3));

    // Daemon should have exited and cleaned up.
    let status = daemon.try_wait().expect("failed to check daemon status");
    assert!(
        status.is_some(),
        "daemon should have exited after idle timeout"
    );

    assert!(
        !socket_path.exists(),
        "socket file should be removed after idle timeout"
    );
    assert!(
        !pid_path.exists(),
        "PID file should be removed after idle timeout"
    );
}

#[test]
#[ignore]
fn test_daemon_status_includes_idle_fields() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");

    // Connect and send status request.
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

    assert_eq!(val["ok"], true);
    // idle_seconds should be present and small (we just connected).
    let idle_secs = val["idle_seconds"]
        .as_u64()
        .expect("idle_seconds should be a number");
    assert!(
        idle_secs < 5,
        "idle_seconds should be small, got {}",
        idle_secs
    );
    // idle_timeout should be 0 (default — no timeout).
    assert_eq!(
        val["idle_timeout"].as_u64().unwrap(),
        0,
        "idle_timeout should be 0 (default)"
    );

    stop_daemon(&mut daemon);
}

// ---------------------------------------------------------------------------
// Reset authentication tests (#26)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_reset_requires_proof() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Get a proof for reset from the daemon
    let sign_output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "sign",
            "--event-type",
            "reset",
        ])
        .current_dir(dir.path())
        .output()
        .expect("sign command failed");
    assert!(sign_output.status.success(), "sign should succeed");
    let proof = String::from_utf8_lossy(&sign_output.stdout)
        .trim()
        .to_string();

    // Reset with valid proof should succeed
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "reset",
            "--confirm",
            "--proof",
            &proof,
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_reset_without_proof_is_rejected() {
    let dir = setup_dir();

    // Reset without --proof should fail at arg parsing (proof is required)
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "reset", "--confirm"])
        .current_dir(dir.path())
        .output()
        .expect("reset command failed to run");

    assert!(
        !output.status.success(),
        "reset without proof should be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--proof"),
        "should mention --proof requirement, got: {}",
        stderr
    );
}

#[test]
#[ignore]
fn test_reset_with_wrong_proof_is_rejected() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Reset with bogus proof should fail
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "reset",
            "--confirm",
            "--proof",
            "deadbeef",
        ])
        .current_dir(dir.path())
        .output()
        .expect("reset command failed to run");

    assert!(
        !output.status.success(),
        "reset with wrong proof should be rejected"
    );

    stop_daemon(&mut daemon);
}

// ---------------------------------------------------------------------------
// Auth error reason code tests (#26)
// ---------------------------------------------------------------------------

/// Setup dir with a non-empty trusted-callers.toml so auth is enforced.
fn setup_dir_with_callers() -> tempfile::TempDir {
    setup_dir_with_callers_content(
        "[callers]\n\"hooks/nonexistent.py\" = \"sha256:0000000000000000000000000000000000000000000000000000000000000000\"\n",
    )
}

/// Setup dir with a caller manifest of the given content (written before
/// `init`, so it is covered by the config seal like a real deployment's).
fn setup_dir_with_callers_content(callers_toml: &str) -> tempfile::TempDir {
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

    // A present callers table — auth is enforced
    std::fs::write(config_dir.join("trusted-callers.toml"), callers_toml).unwrap();

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
fn test_auth_error_includes_reason_code() {
    let dir = setup_dir_with_callers();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");

    // Connect directly and try to sign — should fail auth with reason
    let mut stream = UnixStream::connect(&socket_path).expect("connect to daemon socket");
    writeln!(
        stream,
        r#"{{"op":"sign","event_type":"test","fields":{{}}}}"#
    )
    .expect("write sign request");

    let reader = BufReader::new(&stream);
    let line = reader
        .lines()
        .next()
        .expect("should get a response")
        .expect("response should be readable");

    let val: serde_json::Value =
        serde_json::from_str(&line).expect("response should be valid JSON");

    assert_eq!(val["ok"], false, "auth should fail");
    assert_eq!(
        val["error"], "auth_failed",
        "error code should be auth_failed"
    );

    // The response MUST include a reason field with a known code
    assert!(
        val["reason"].is_string(),
        "auth error response must include 'reason' field, got: {}",
        serde_json::to_string_pretty(&val).unwrap()
    );
    let reason = val["reason"].as_str().unwrap();
    let known_reasons = [
        "pid_resolution_failed",
        "hash_mismatch",
        "peer_cred_unavailable",
    ];
    assert!(
        known_reasons.contains(&reason),
        "reason should be one of {:?}, got: {}",
        known_reasons,
        reason
    );

    stop_daemon(&mut daemon);
}

// ---------------------------------------------------------------------------
// Direct-peer auth tests
// ---------------------------------------------------------------------------

/// Setup dir with two real hook scripts in trusted-callers.toml: one that
/// speaks the socket protocol directly (must authenticate) and one that
/// shells out to `sahjhan sign` (must NOT — manifest authority is not
/// inheritable by a trusted script's descendants, and the CLI never
/// authenticates).
fn setup_dir_with_real_hook() -> (tempfile::TempDir, String) {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"[protocol]
name = "test-direct-peer"
version = "1.0.0"
description = "Direct-peer auth test"

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

    let hooks_dir = config_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();

    // direct_hook.py speaks the socket protocol itself — the authenticated
    // peer IS the trusted script.
    let direct_content = r#"#!/usr/bin/env python3
import json, socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
s.sendall((json.dumps({"op": "sign", "event_type": "test", "fields": {"x": "1"}}) + "\n").encode())
resp = s.makefile().readline()
print(resp, end="")
sys.exit(0 if json.loads(resp).get("ok") else 1)
"#;
    std::fs::write(hooks_dir.join("direct_hook.py"), direct_content).unwrap();

    // cli_hook.py shells out to `sahjhan sign` — the peer on the socket is
    // the CLI binary, not this script.
    let sahjhan_bin = env!("CARGO_BIN_EXE_sahjhan");
    let cli_content = format!(
        r#"#!/usr/bin/env python3
import subprocess, sys, os
os.chdir(sys.argv[1])
result = subprocess.run(
    ["{bin}", "--config-dir", "enforcement", "sign",
     "--event-type", "test", "--field", "x=1"],
    capture_output=True, text=True
)
print(result.stdout, end='')
if result.returncode != 0:
    print(result.stderr, end='', file=sys.stderr)
sys.exit(result.returncode)
"#,
        bin = sahjhan_bin
    );
    std::fs::write(hooks_dir.join("cli_hook.py"), &cli_content).unwrap();

    use sha2::{Digest, Sha256};
    let direct_hash = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(
            std::fs::read(hooks_dir.join("direct_hook.py")).unwrap()
        ))
    );
    let cli_hash = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(
            std::fs::read(hooks_dir.join("cli_hook.py")).unwrap()
        ))
    );

    // Both scripts are listed — the CLI-mediated test must fail on the
    // *peer*, not on a missing manifest entry.
    std::fs::write(
        config_dir.join("trusted-callers.toml"),
        format!(
            "[callers]\n\"hooks/direct_hook.py\" = \"{}\"\n\"hooks/cli_hook.py\" = \"{}\"\n",
            direct_hash, cli_hash
        ),
    )
    .unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    (dir, direct_hash)
}

#[test]
#[ignore]
fn test_direct_peer_hook_authenticates() {
    let (dir, _hash) = setup_dir_with_real_hook();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // The trusted script connects to the socket itself: the direct peer's
    // cmdline names it, it canonicalizes under --config-dir, and its hash
    // matches the manifest.
    let hook_script = dir.path().join("enforcement/hooks/direct_hook.py");
    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");
    let output = std::process::Command::new("python3")
        .arg(&hook_script)
        .arg(&socket_path)
        .output()
        .expect("hook script should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let _ = daemon.kill();
    let daemon_output = daemon.wait_with_output().unwrap();
    let daemon_stderr = String::from_utf8_lossy(&daemon_output.stderr);

    assert!(
        output.status.success(),
        "direct-peer hook should authenticate\nstdout: {}\nstderr: {}\ndaemon stderr:\n{}",
        stdout,
        stderr,
        daemon_stderr
    );
    assert!(
        stdout.contains("\"proof\""),
        "should get a proof back from sign, stdout: {}\ndaemon stderr:\n{}",
        stdout,
        daemon_stderr
    );
}

#[test]
#[ignore]
fn test_cli_mediated_connection_is_denied() {
    let (dir, _hash) = setup_dir_with_real_hook();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // cli_hook.py is trusted and listed — but it reaches the daemon through
    // `sahjhan sign`, so the process on the socket is the CLI binary. With
    // the ancestor walk gone, a trusted ancestor confers nothing: manifest
    // authority is not inheritable, and the CLI never authenticates.
    let hook_script = dir.path().join("enforcement/hooks/cli_hook.py");
    let output = std::process::Command::new("python3")
        .arg(&hook_script)
        .arg(dir.path())
        .output()
        .expect("hook script should run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let _ = daemon.kill();
    let daemon_output = daemon.wait_with_output().unwrap();
    let daemon_stderr = String::from_utf8_lossy(&daemon_output.stderr);

    assert!(
        !output.status.success(),
        "CLI-mediated sign must NOT authenticate\nstdout: {}\nstderr: {}\ndaemon stderr:\n{}",
        stdout,
        stderr,
        daemon_stderr
    );
}

#[test]
#[ignore]
fn test_empty_manifest_denies_every_caller() {
    // A present trusted-callers.toml with an empty [callers] table means
    // "no one", not "anyone". Only status still answers.
    let dir = setup_dir_with_callers_content("[callers]\n");
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");
    let mut stream = UnixStream::connect(&socket_path).expect("connect to daemon socket");
    writeln!(
        stream,
        r#"{{"op":"sign","event_type":"test","fields":{{}}}}"#
    )
    .expect("write sign request");
    let reader = BufReader::new(&stream);
    let line = reader
        .lines()
        .next()
        .expect("should get a response")
        .expect("response should be readable");
    let val: serde_json::Value =
        serde_json::from_str(&line).expect("response should be valid JSON");
    assert_eq!(val["ok"], false, "empty manifest must deny, got: {}", line);
    assert_eq!(val["error"], "auth_failed");

    // Close our connection before asking for status: the single-threaded
    // daemon serves connections sequentially, and a held-open stream blocks
    // the accept loop (the wedge the connection read timeout exists for).
    drop(stream);

    // Status is exempt (health check).
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "daemon", "status"])
        .current_dir(dir.path())
        .assert()
        .success();

    stop_daemon(&mut daemon);
}
