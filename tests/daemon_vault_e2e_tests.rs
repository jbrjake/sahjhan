//! End-to-end integration tests for daemon vault operations.
//!
//! Each test starts a real daemon process, exercises vault operations via the
//! CLI, then tears it down. Tests are `#[ignore]` by default because they
//! spawn background processes and use real sockets.

use assert_cmd::Command;
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
// Vault tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_vault_store_and_read() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Write a file to store in the vault.
    let secret_path = dir.path().join("secret.json");
    std::fs::write(&secret_path, r#"{"answers": [1, 2, 3]}"#).unwrap();

    // Store it.
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "store",
            "--name",
            "quiz-bank",
            "--file",
        ])
        .arg(secret_path.to_str().unwrap())
        .current_dir(dir.path())
        .assert()
        .success();

    // Read it back.
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "read",
            "--name",
            "quiz-bank",
        ])
        .current_dir(dir.path())
        .output()
        .expect("vault read command failed");

    assert!(output.status.success(), "vault read should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        r#"{"answers": [1, 2, 3]}"#,
        "vault read should return stored data"
    );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_vault_list() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Store two entries.
    let file_a = dir.path().join("alpha.txt");
    let file_b = dir.path().join("beta.txt");
    std::fs::write(&file_a, "data-a").unwrap();
    std::fs::write(&file_b, "data-b").unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "store",
            "--name",
            "alpha",
            "--file",
        ])
        .arg(file_a.to_str().unwrap())
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "store",
            "--name",
            "beta",
            "--file",
        ])
        .arg(file_b.to_str().unwrap())
        .current_dir(dir.path())
        .assert()
        .success();

    // List entries.
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "vault", "list"])
        .current_dir(dir.path())
        .output()
        .expect("vault list command failed");

    assert!(output.status.success(), "vault list should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut names: Vec<&str> = stdout.lines().collect();
    names.sort();
    assert_eq!(names, vec!["alpha", "beta"]);

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_vault_delete() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Store an entry.
    let file = dir.path().join("temp.txt");
    std::fs::write(&file, "disposable").unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "store",
            "--name",
            "doomed",
            "--file",
        ])
        .arg(file.to_str().unwrap())
        .current_dir(dir.path())
        .assert()
        .success();

    // Delete it.
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "delete",
            "--name",
            "doomed",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Read should now fail.
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "read",
            "--name",
            "doomed",
        ])
        .current_dir(dir.path())
        .output()
        .expect("vault read command failed to run");

    assert!(
        !output.status.success(),
        "reading a deleted entry should fail"
    );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_vault_read_nonexistent() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Read a name that was never stored.
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "read",
            "--name",
            "does-not-exist",
        ])
        .current_dir(dir.path())
        .output()
        .expect("vault read command failed to run");

    assert!(
        !output.status.success(),
        "reading a nonexistent entry should fail"
    );

    stop_daemon(&mut daemon);
}
