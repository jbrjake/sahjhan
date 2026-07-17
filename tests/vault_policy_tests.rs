//! Tests for per-key, state-based vault access policy (vault.toml).
//!
//! Unit tests cover the pure policy predicate and config load/validate.
//! The `#[ignore]` e2e tests spawn a real daemon and prove the daemon
//! enforces the policy: a `writable_in_states = ["recon"]` key can only be
//! stored while the ledger's current state is `recon`.

use sahjhan::config::vault_policy::{VaultAccess, VaultPolicy};
use sahjhan::config::ProtocolConfig;

// ---------------------------------------------------------------------------
// Pure policy predicate
// ---------------------------------------------------------------------------

fn policy(
    writable: Option<&[&str]>,
    readable: Option<&[&str]>,
    deletable: Option<&[&str]>,
) -> VaultPolicy {
    let conv = |o: Option<&[&str]>| o.map(|xs| xs.iter().map(|s| s.to_string()).collect());
    VaultPolicy {
        name: "k".to_string(),
        writable_in_states: conv(writable),
        readable_in_states: conv(readable),
        deletable_in_states: conv(deletable),
    }
}

#[test]
fn no_whitelist_permits_any_state() {
    let p = policy(None, None, None);
    assert!(p.permits(VaultAccess::Store, "idle"));
    assert!(p.permits(VaultAccess::Read, "anything"));
    assert!(p.permits(VaultAccess::Delete, "recon"));
}

#[test]
fn empty_whitelist_permits_nothing() {
    let p = policy(Some(&[]), None, None);
    assert!(!p.permits(VaultAccess::Store, "recon"));
    assert!(!p.permits(VaultAccess::Store, "idle"));
}

#[test]
fn whitelist_permits_only_listed_states() {
    let p = policy(Some(&["recon"]), Some(&["audit", "fix_loop"]), None);
    assert!(p.permits(VaultAccess::Store, "recon"));
    assert!(!p.permits(VaultAccess::Store, "audit"));
    assert!(p.permits(VaultAccess::Read, "audit"));
    assert!(p.permits(VaultAccess::Read, "fix_loop"));
    assert!(!p.permits(VaultAccess::Read, "recon"));
    // delete has no whitelist -> unrestricted
    assert!(p.permits(VaultAccess::Delete, "idle"));
}

// ---------------------------------------------------------------------------
// Config load + validation
// ---------------------------------------------------------------------------

/// Write a minimal config dir with states idle+recon and the given vault.toml.
fn write_config(vault_toml: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    std::fs::write(
        p.join("protocol.toml"),
        "[protocol]\nname=\"t\"\nversion=\"1.0.0\"\ndescription=\"d\"\n\
         [paths]\nmanaged=[\"output\"]\ndata_dir=\"output/.sahjhan\"\nrender_dir=\"output\"\n",
    )
    .unwrap();
    std::fs::write(
        p.join("states.toml"),
        "[states.idle]\nlabel=\"Idle\"\ninitial=true\n[states.recon]\nlabel=\"Recon\"\n",
    )
    .unwrap();
    std::fs::write(
        p.join("transitions.toml"),
        "[[transitions]]\nfrom=\"idle\"\nto=\"recon\"\ncommand=\"run_start\"\ngates=[]\n",
    )
    .unwrap();
    std::fs::write(p.join("vault.toml"), vault_toml).unwrap();
    dir
}

#[test]
fn loads_vault_policies_from_toml() {
    let dir = write_config(
        "[[policy]]\nname=\"quiz-bank\"\nwritable_in_states=[\"recon\"]\n\
         readable_in_states=[\"audit\"]\n",
    );
    let config = ProtocolConfig::load(dir.path()).unwrap();
    let p = config
        .vault_policies
        .get("quiz-bank")
        .expect("policy should load");
    assert_eq!(
        p.writable_in_states.as_deref(),
        Some(&["recon".to_string()][..])
    );
    assert_eq!(
        p.readable_in_states.as_deref(),
        Some(&["audit".to_string()][..])
    );
    assert!(p.deletable_in_states.is_none());
}

#[test]
fn absent_vault_toml_yields_no_policies() {
    let dir = write_config("");
    std::fs::remove_file(dir.path().join("vault.toml")).unwrap();
    let config = ProtocolConfig::load(dir.path()).unwrap();
    assert!(config.vault_policies.is_empty());
}

#[test]
fn validate_rejects_unknown_state_in_policy() {
    let dir = write_config(
        "[[policy]]\nname=\"quiz-bank\"\nwritable_in_states=[\"nonexistent_state\"]\n",
    );
    let config = ProtocolConfig::load(dir.path()).unwrap();
    let (errors, _) = config.validate_deep(dir.path());
    assert!(
        errors.iter().any(|e| e.contains("quiz-bank")
            && e.contains("nonexistent_state")
            && e.contains("writable")),
        "expected unknown-state error, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// E2e: daemon enforces the policy against the live ledger state
// ---------------------------------------------------------------------------

use assert_cmd::Command;

fn setup_dir_with_policy() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("protocol.toml"),
        "[protocol]\nname=\"t\"\nversion=\"1.0.0\"\ndescription=\"d\"\n\
         [paths]\nmanaged=[\"output\"]\ndata_dir=\"output/.sahjhan\"\nrender_dir=\"output\"\n",
    )
    .unwrap();
    std::fs::write(
        config_dir.join("states.toml"),
        "[states.idle]\nlabel=\"Idle\"\ninitial=true\n[states.recon]\nlabel=\"Recon\"\n",
    )
    .unwrap();
    std::fs::write(
        config_dir.join("transitions.toml"),
        "[[transitions]]\nfrom=\"idle\"\nto=\"recon\"\ncommand=\"run_start\"\ngates=[]\n",
    )
    .unwrap();
    // quiz-bank is writable/deletable only in recon; a control key is unpoliced.
    std::fs::write(
        config_dir.join("vault.toml"),
        "[[policy]]\nname=\"quiz-bank\"\nwritable_in_states=[\"recon\"]\n\
         deletable_in_states=[\"recon\"]\n",
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
    // No `ledger create` needed: transitions/events append to the default
    // ledger (output/.sahjhan/ledger.jsonl) and state derives from it. A fresh
    // ledger with no state_transition derives the initial state (idle).
    dir
}

fn start_daemon(dir: &std::path::Path) -> std::process::Child {
    std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(["--config-dir", "enforcement", "daemon", "start"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start daemon")
}

fn wait_for_socket(dir: &std::path::Path) {
    let socket_path = dir.join("output/.sahjhan/daemon.sock");
    for _ in 0..50 {
        if socket_path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("daemon socket did not appear");
}

fn vault_store(dir: &std::path::Path, name: &str, file: &std::path::Path) -> bool {
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "store",
            "--name",
            name,
            "--file",
        ])
        .arg(file.to_str().unwrap())
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
#[ignore]
fn store_gated_by_recon_state() {
    let dir = setup_dir_with_policy();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let secret = dir.path().join("s.json");
    std::fs::write(&secret, r#"[{"q":"x"}]"#).unwrap();

    // State is idle (fresh ledger) -> quiz-bank store must be rejected.
    assert!(
        !vault_store(dir.path(), "quiz-bank", &secret),
        "store must be forbidden outside recon"
    );

    // An unpoliced key is still storable in idle (backward compatible).
    assert!(
        vault_store(dir.path(), "scratch", &secret),
        "unpoliced key must remain unrestricted"
    );

    // Advance to recon.
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "run_start"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Now the store must succeed.
    assert!(
        vault_store(dir.path(), "quiz-bank", &secret),
        "store must be permitted in recon"
    );

    let _ = daemon.kill();
    let _ = daemon.wait();
}

#[test]
#[ignore]
fn delete_gated_by_recon_state() {
    let dir = setup_dir_with_policy();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let secret = dir.path().join("s.json");
    std::fs::write(&secret, r#"[{"q":"x"}]"#).unwrap();

    // Move to recon and store.
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "run_start"])
        .current_dir(dir.path())
        .assert()
        .success();
    assert!(vault_store(dir.path(), "quiz-bank", &secret));

    // delete IS permitted in recon.
    let ok = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "vault",
            "delete",
            "--name",
            "quiz-bank",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "delete must be permitted in recon");

    let _ = daemon.kill();
    let _ = daemon.wait();
}
