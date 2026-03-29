// tests/config_integrity_tests.rs
//
// Tests for config integrity sealing, verification, and reseal.

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
