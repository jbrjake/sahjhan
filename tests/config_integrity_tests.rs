// tests/config_integrity_tests.rs
//
// Tests for config integrity sealing, verification, and reseal.

use std::collections::BTreeMap;
use tempfile::tempdir;

#[test]
fn test_compute_config_seals_all_files_present() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("protocol.toml"), b"[protocol]\nname = \"test\"\n").unwrap();
    std::fs::write(dir.path().join("states.toml"), b"[states.idle]\nlabel = \"Idle\"\n").unwrap();
    std::fs::write(dir.path().join("transitions.toml"), b"[[transitions]]\nfrom = \"idle\"\n").unwrap();
    std::fs::write(dir.path().join("events.toml"), b"[events.e1]\ndescription = \"E1\"\n").unwrap();
    std::fs::write(dir.path().join("renders.toml"), b"[[renders]]\ntarget = \"out.md\"\n").unwrap();

    let seals = sahjhan::config::compute_config_seals(dir.path());

    assert_eq!(seals.len(), 5);
    assert!(seals.contains_key("config_seal_protocol"));
    assert!(seals.contains_key("config_seal_states"));
    assert!(seals.contains_key("config_seal_transitions"));
    assert!(seals.contains_key("config_seal_events"));
    assert!(seals.contains_key("config_seal_renders"));

    // Each value should be a 64-char hex SHA-256
    for (_key, hash) in &seals {
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

    assert_eq!(seals.len(), 5);
    // Missing files should get the SHA-256 of empty bytes
    let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_eq!(seals["config_seal_events"], empty_hash);
    assert_eq!(seals["config_seal_renders"], empty_hash);
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
    assert_eq!(genesis.fields.get("config_seal_protocol").unwrap(), "aaa111");
    assert_eq!(genesis.fields.get("config_seal_states").unwrap(), "bbb222");
    assert_eq!(genesis.fields.get("config_seal_transitions").unwrap(), "ccc333");
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
