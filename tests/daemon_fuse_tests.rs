//! End-to-end tests for the daemon sandbox fuse (`[daemon] require_sandbox`).
//!
//! Each test starts a real daemon process and exercises it via the CLI.
//! Tests are `#[ignore]` by default because they spawn background processes
//! and use real sockets.
//!
//! The daemon's HOME is pointed at an empty temp directory so the fuse's
//! user-scope lookup (`~/.claude/settings.json`) cannot pick up settings
//! from the developer's real account.

use std::path::{Path, PathBuf};
use tempfile::tempdir;

/// Stand up a temp project with a minimal protocol config (optionally arming
/// the fuse), run `sahjhan init`, and return the owned TempDir.
fn setup_dir(require_sandbox: bool) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    let daemon_section = if require_sandbox {
        "\n[daemon]\nrequire_sandbox = true\n"
    } else {
        ""
    };
    std::fs::write(
        config_dir.join("protocol.toml"),
        format!(
            r#"[protocol]
name = "test-fuse"
version = "1.0.0"
description = "Sandbox fuse test protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"
{}"#,
            daemon_section
        ),
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

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    assert_cmd::Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

/// A short out-of-project socket path (AF_UNIX paths are capped at 104
/// bytes on macOS).
fn external_socket() -> (tempfile::TempDir, PathBuf) {
    let sock_dir = tempfile::Builder::new()
        .prefix("sjfuse")
        .tempdir_in("/tmp")
        .unwrap();
    let sock = sock_dir.path().join("d.sock");
    (sock_dir, sock)
}

fn start_daemon(dir: &Path, sock: &Path, home: &Path) -> std::process::Child {
    std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(["--config-dir", "enforcement", "daemon", "start"])
        .current_dir(dir)
        .env("SAHJHAN_DAEMON_SOCKET", sock)
        .env("HOME", home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start daemon")
}

fn wait_for_socket(sock: &Path) {
    for _ in 0..50 {
        if sock.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("Daemon socket did not appear at {:?}", sock);
}

fn sign(dir: &Path, sock: &Path) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args([
            "--config-dir",
            "enforcement",
            "sign",
            "--event-type",
            "test",
            "--field",
            "a=1",
        ])
        .current_dir(dir)
        .env("SAHJHAN_DAEMON_SOCKET", sock)
        .output()
        .expect("sign command failed to run")
}

/// The settings body the arming flow is expected to write.
const CONFINING: &str =
    r#"{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false,"failIfUnavailable":true}}"#;

/// The full arming lifecycle against one daemon process: the daemon starts
/// before any sandbox settings exist (the normal order), refuses privileged
/// ops, starts serving the moment the boundary appears, and stops again
/// when it is removed.
#[test]
#[ignore]
fn test_armed_fuse_follows_settings_lifecycle() {
    let dir = setup_dir(true);
    let home = tempdir().unwrap();
    let (_sock_dir, sock) = external_socket();
    let mut daemon = start_daemon(dir.path(), &sock, home.path());
    wait_for_socket(&sock);

    // Phase 1: no settings — refused, and status still answers.
    let out = sign(dir.path(), &sock);
    assert!(
        !out.status.success(),
        "sign must be refused before the sandbox settings exist"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("sandbox.enabled"),
        "refusal should name the missing key, got: {}",
        stderr
    );
    assert_cmd::Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "daemon", "status"])
        .current_dir(dir.path())
        .env("SAHJHAN_DAEMON_SOCKET", &sock)
        .assert()
        .success();

    // Phase 2: the boundary appears — the same daemon now serves.
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings = claude_dir.join("settings.local.json");
    std::fs::write(&settings, CONFINING).unwrap();
    let out = sign(dir.path(), &sock);
    assert!(
        out.status.success(),
        "sign should pass once the boundary is in place, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !out.stdout.is_empty(),
        "sign should print a proof on success"
    );

    // Phase 3: the boundary goes away — refused again.
    std::fs::remove_file(&settings).unwrap();
    let out = sign(dir.path(), &sock);
    assert!(
        !out.status.success(),
        "sign must be refused after the sandbox settings are removed"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
}

/// A weakened settings file (unsandboxed retries allowed) does not satisfy
/// the fuse.
#[test]
#[ignore]
fn test_armed_fuse_rejects_weakened_settings() {
    let dir = setup_dir(true);
    let home = tempdir().unwrap();
    let (_sock_dir, sock) = external_socket();
    let mut daemon = start_daemon(dir.path(), &sock, home.path());
    wait_for_socket(&sock);

    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.local.json"),
        r#"{"sandbox":{"enabled":true,"allowUnsandboxedCommands":true,"failIfUnavailable":true}}"#,
    )
    .unwrap();

    let out = sign(dir.path(), &sock);
    assert!(!out.status.success(), "weakened settings must not serve");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("allowUnsandboxedCommands"),
        "refusal should name the weakened key, got: {}",
        stderr
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
}

/// With the socket left at its default in-project location, the fuse
/// refuses even under otherwise-confining settings — C1 must actually have
/// been applied.
#[test]
#[ignore]
fn test_armed_fuse_rejects_in_project_socket() {
    let dir = setup_dir(true);
    let home = tempdir().unwrap();
    // No SAHJHAN_DAEMON_SOCKET: the daemon binds inside the project.
    let mut daemon = std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(["--config-dir", "enforcement", "daemon", "start"])
        .current_dir(dir.path())
        .env("HOME", home.path())
        .env_remove("SAHJHAN_DAEMON_SOCKET")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start daemon");
    let sock = dir.path().join("output/.sahjhan/daemon.sock");
    wait_for_socket(&sock);

    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.local.json"), CONFINING).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
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
        .env_remove("SAHJHAN_DAEMON_SOCKET")
        .output()
        .expect("sign command failed to run");
    assert!(!out.status.success(), "in-project socket must not serve");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("SAHJHAN_DAEMON_SOCKET"),
        "refusal should point at the socket relocation, got: {}",
        stderr
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
}

/// An unarmed daemon (no `[daemon]` section) behaves exactly as before:
/// privileged ops serve with no sandbox settings anywhere.
#[test]
#[ignore]
fn test_unarmed_daemon_serves_without_settings() {
    let dir = setup_dir(false);
    let home = tempdir().unwrap();
    let (_sock_dir, sock) = external_socket();
    let mut daemon = start_daemon(dir.path(), &sock, home.path());
    wait_for_socket(&sock);

    let out = sign(dir.path(), &sock);
    assert!(
        out.status.success(),
        "unarmed daemon should sign, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
}
