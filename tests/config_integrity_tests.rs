// tests/config_integrity_tests.rs
//
// Tests for config integrity sealing, verification, and reseal.

use std::collections::BTreeMap;
use tempfile::tempdir;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_compute_config_seals_all_files_present() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("protocol.toml"),
        b"[protocol]\nname = \"test\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("states.toml"),
        b"[states.idle]\nlabel = \"Idle\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("transitions.toml"),
        b"[[transitions]]\nfrom = \"idle\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("events.toml"),
        b"[events.e1]\ndescription = \"E1\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("renders.toml"),
        b"[[renders]]\ntarget = \"out.md\"\n",
    )
    .unwrap();

    let seals = sahjhan::config::compute_config_seals(dir.path());

    assert_eq!(seals.len(), 6);
    assert!(seals.contains_key("config_seal_protocol"));
    assert!(seals.contains_key("config_seal_states"));
    assert!(seals.contains_key("config_seal_transitions"));
    assert!(seals.contains_key("config_seal_events"));
    assert!(seals.contains_key("config_seal_renders"));
    assert!(seals.contains_key("config_seal_hooks"));

    // Each value should be a 64-char hex SHA-256
    for hash in seals.values() {
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

#[test]
fn test_compute_config_seals_optional_files_missing() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("protocol.toml"), b"proto").unwrap();
    std::fs::write(dir.path().join("states.toml"), b"states").unwrap();
    std::fs::write(dir.path().join("transitions.toml"), b"trans").unwrap();
    // events.toml and renders.toml intentionally missing

    let seals = sahjhan::config::compute_config_seals(dir.path());

    assert_eq!(seals.len(), 6);
    // Missing files should get the SHA-256 of empty bytes
    let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_eq!(seals["config_seal_events"], empty_hash);
    assert_eq!(seals["config_seal_renders"], empty_hash);
    assert_eq!(seals["config_seal_hooks"], empty_hash);
    // Present files should NOT be the empty hash
    assert_ne!(seals["config_seal_protocol"], empty_hash);
}

#[test]
fn test_compute_config_seals_deterministic() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("protocol.toml"), b"content").unwrap();
    std::fs::write(dir.path().join("states.toml"), b"content2").unwrap();
    std::fs::write(dir.path().join("transitions.toml"), b"content3").unwrap();

    let seals1 = sahjhan::config::compute_config_seals(dir.path());
    let seals2 = sahjhan::config::compute_config_seals(dir.path());
    assert_eq!(seals1, seals2);
}

use sahjhan::ledger::chain::Ledger;

#[test]
fn test_init_with_seals_stores_hashes_in_genesis() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let mut seals = BTreeMap::new();
    seals.insert("config_seal_protocol".to_string(), "aaa111".to_string());
    seals.insert("config_seal_states".to_string(), "bbb222".to_string());
    seals.insert("config_seal_transitions".to_string(), "ccc333".to_string());
    seals.insert("config_seal_events".to_string(), "ddd444".to_string());
    seals.insert("config_seal_renders".to_string(), "eee555".to_string());

    let ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    let genesis = &ledger.entries()[0];
    assert_eq!(
        genesis.fields.get("config_seal_protocol").unwrap(),
        "aaa111"
    );
    assert_eq!(genesis.fields.get("config_seal_states").unwrap(), "bbb222");
    assert_eq!(
        genesis.fields.get("config_seal_transitions").unwrap(),
        "ccc333"
    );
    assert_eq!(genesis.fields.get("config_seal_events").unwrap(), "ddd444");
    assert_eq!(genesis.fields.get("config_seal_renders").unwrap(), "eee555");
    // Original fields still present
    assert_eq!(genesis.fields.get("protocol_name").unwrap(), "test");
    assert_eq!(genesis.fields.get("protocol_version").unwrap(), "1.0.0");
}

#[test]
fn test_init_without_seals_unchanged() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let genesis = &ledger.entries()[0];
    assert_eq!(genesis.fields.len(), 2); // Only protocol_name and protocol_version
    assert!(!genesis.fields.contains_key("config_seal_protocol"));
}

#[test]
fn test_find_effective_seal_from_genesis() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let mut seals = BTreeMap::new();
    seals.insert("config_seal_protocol".to_string(), "aaa".to_string());
    seals.insert("config_seal_states".to_string(), "bbb".to_string());

    let ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    let effective = ledger.find_effective_seal().unwrap();
    assert_eq!(effective.get("config_seal_protocol").unwrap(), "aaa");
    assert_eq!(effective.get("config_seal_states").unwrap(), "bbb");
}

#[test]
fn test_find_effective_seal_legacy_ledger_returns_none() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    assert!(ledger.find_effective_seal().is_none());
}

#[test]
fn test_find_effective_seal_prefers_reseal_over_genesis() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let mut seals = BTreeMap::new();
    seals.insert("config_seal_protocol".to_string(), "old_hash".to_string());

    let mut ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    // Append a config_reseal event with new hashes
    let mut reseal_fields = BTreeMap::new();
    reseal_fields.insert("config_seal_protocol".to_string(), "new_hash".to_string());
    ledger.append("config_reseal", reseal_fields).unwrap();

    let effective = ledger.find_effective_seal().unwrap();
    assert_eq!(effective.get("config_seal_protocol").unwrap(), "new_hash");
}

#[test]
fn test_verify_config_seal_happy_path() {
    let config_dir = tempdir().unwrap();
    std::fs::write(config_dir.path().join("protocol.toml"), b"proto content").unwrap();
    std::fs::write(config_dir.path().join("states.toml"), b"states content").unwrap();
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans content").unwrap();

    let seals = sahjhan::config::compute_config_seals(config_dir.path());

    let ledger_dir = tempdir().unwrap();
    let ledger_path = ledger_dir.path().join("ledger.jsonl");
    let ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    // Files unchanged — should pass
    assert!(ledger.verify_config_seal(config_dir.path()).is_ok());
}

#[test]
fn test_verify_config_seal_detects_tamper() {
    let config_dir = tempdir().unwrap();
    std::fs::write(config_dir.path().join("protocol.toml"), b"proto content").unwrap();
    std::fs::write(config_dir.path().join("states.toml"), b"states content").unwrap();
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans content").unwrap();

    let seals = sahjhan::config::compute_config_seals(config_dir.path());

    let ledger_dir = tempdir().unwrap();
    let ledger_path = ledger_dir.path().join("ledger.jsonl");
    let ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    // Tamper with transitions.toml
    std::fs::write(config_dir.path().join("transitions.toml"), b"TAMPERED").unwrap();

    let err = ledger.verify_config_seal(config_dir.path()).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("config integrity violation"));
    assert!(msg.contains("transitions"));
}

#[test]
fn test_verify_config_seal_skips_legacy_ledger() {
    let config_dir = tempdir().unwrap();
    std::fs::write(config_dir.path().join("protocol.toml"), b"proto").unwrap();
    std::fs::write(config_dir.path().join("states.toml"), b"states").unwrap();
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans").unwrap();

    let ledger_dir = tempdir().unwrap();
    let ledger_path = ledger_dir.path().join("ledger.jsonl");
    // Legacy ledger — no seals
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Should pass silently even though config exists
    assert!(ledger.verify_config_seal(config_dir.path()).is_ok());
}

#[test]
fn test_verify_config_seal_after_reseal() {
    let config_dir = tempdir().unwrap();
    std::fs::write(config_dir.path().join("protocol.toml"), b"proto v1").unwrap();
    std::fs::write(config_dir.path().join("states.toml"), b"states v1").unwrap();
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans v1").unwrap();

    let seals_v1 = sahjhan::config::compute_config_seals(config_dir.path());

    let ledger_dir = tempdir().unwrap();
    let ledger_path = ledger_dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals_v1).unwrap();

    // Modify config
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans v2").unwrap();

    // Verify should fail now
    assert!(ledger.verify_config_seal(config_dir.path()).is_err());

    // Reseal with new hashes
    let seals_v2 = sahjhan::config::compute_config_seals(config_dir.path());
    ledger.append("config_reseal", seals_v2).unwrap();

    // Now verify should pass
    assert!(ledger.verify_config_seal(config_dir.path()).is_ok());
}

/// Set up a minimal config dir with all files, run init.
fn setup_sealed_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "test-seal"
version = "1.0.0"
description = "Seal test"

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

#[test]
fn test_cli_tamper_detection_blocks_status() {
    let dir = setup_sealed_dir();

    // Status should work before tampering
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Tamper with transitions.toml
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n# tampered\n",
    )
    .unwrap();

    // Status should now fail
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("config integrity violation"))
        .stderr(predicate::str::contains("transitions"));
}

// ---------------------------------------------------------------------------
// Daemon helpers for reseal tests
// ---------------------------------------------------------------------------

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
    panic!("Daemon socket did not appear at {:?}", socket_path);
}

fn stop_daemon(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
#[ignore]
fn test_cli_reseal_requires_proof() {
    let dir = setup_sealed_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Tamper so we need to reseal
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n# v2\n",
    )
    .unwrap();

    // Reseal with a bogus proof should fail
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "reseal", "--proof", "bad"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("invalid proof")
                .or(predicate::str::contains("proof does not match")),
        );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_cli_reseal_with_valid_proof_succeeds() {
    let dir = setup_sealed_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Tamper
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n# v2\n",
    )
    .unwrap();

    // Use `sahjhan sign` to get a valid proof for config_reseal.
    let sign_output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "sign",
            "--event-type",
            "config_reseal",
        ])
        .current_dir(dir.path())
        .output()
        .expect("sign command failed");
    assert!(sign_output.status.success(), "sign should succeed");
    let proof = String::from_utf8_lossy(&sign_output.stdout)
        .trim()
        .to_string();
    assert!(!proof.is_empty(), "proof should not be empty");

    // Reseal with valid proof
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "reseal", "--proof", &proof])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("resealed"));

    // Status should now work again (need to stop daemon first since
    // status opens the ledger and the daemon may hold the socket).
    stop_daemon(&mut daemon);

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_cli_backward_compat_legacy_ledger() {
    // Manually create a legacy ledger (no seals) and verify commands still work
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    let data_dir = dir.path().join("output/.sahjhan");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        "[protocol]\nname = \"test\"\nversion = \"1.0.0\"\ndescription = \"t\"\n\n[paths]\nmanaged = [\"output\"]\ndata_dir = \"output/.sahjhan\"\nrender_dir = \"output\"\n",
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

    // Create a legacy ledger without seals (using Ledger::init directly)
    let ledger_path = data_dir.join("ledger.jsonl");
    let _ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Create registry
    let reg_path = data_dir.join("ledgers.toml");
    let mut registry = sahjhan::ledger::registry::LedgerRegistry::new(&reg_path).unwrap();
    registry
        .create(
            "default",
            "ledger.jsonl",
            sahjhan::ledger::registry::LedgerMode::Stateful,
        )
        .unwrap();

    // Create manifest
    let mut manifest =
        sahjhan::manifest::tracker::Manifest::init("output/.sahjhan", vec!["output".to_string()])
            .unwrap();
    manifest.save(&data_dir.join("manifest.json")).unwrap();

    // Status should work (no seals = skip verification)
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_config_seals_include_hooks_toml() {
    let dir = std::path::Path::new("examples/minimal");
    let seals = sahjhan::config::compute_config_seals(dir);
    assert!(seals.contains_key("config_seal_hooks"));
    // minimal now has a hooks.toml — seal should be a non-empty-file hash
    let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_ne!(
        seals["config_seal_hooks"], empty_hash,
        "hooks.toml seal should not be the empty-file hash now that hooks.toml exists"
    );
    assert_eq!(
        seals["config_seal_hooks"].len(),
        64,
        "hooks seal should be a 64-char hex SHA-256"
    );
}
