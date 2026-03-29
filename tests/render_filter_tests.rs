// tests/render_filter_tests.rs
//
// Tests for custom Tera filters registered by the render engine:
// - where_eq: keep array items where attribute == value
// - unique_by: deduplicate array by field, keeping last occurrence

use sahjhan::config::ProtocolConfig;
use sahjhan::ledger::chain::Ledger;
use sahjhan::render::engine::RenderEngine;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::tempdir;

/// Helper: init a ledger and record some events for testing.
fn setup_ledger_with_events(dir: &Path) -> Ledger {
    let ledger_path = dir.join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut fields = BTreeMap::new();

    // Record 3 findings
    fields.insert("id".to_string(), "BH-001".to_string());
    fields.insert("severity".to_string(), "HIGH".to_string());
    ledger.append("finding", fields.clone()).unwrap();

    fields.clear();
    fields.insert("id".to_string(), "BH-002".to_string());
    fields.insert("severity".to_string(), "MEDIUM".to_string());
    ledger.append("finding", fields.clone()).unwrap();

    fields.clear();
    fields.insert("id".to_string(), "BH-003".to_string());
    fields.insert("severity".to_string(), "LOW".to_string());
    ledger.append("finding", fields.clone()).unwrap();

    // Record 4 resolutions (BH-001 resolved twice)
    fields.clear();
    fields.insert("id".to_string(), "BH-001".to_string());
    ledger.append("finding_resolved", fields.clone()).unwrap();

    fields.clear();
    fields.insert("id".to_string(), "BH-002".to_string());
    ledger.append("finding_resolved", fields.clone()).unwrap();

    fields.clear();
    fields.insert("id".to_string(), "BH-003".to_string());
    ledger.append("finding_resolved", fields.clone()).unwrap();

    fields.clear();
    fields.insert("id".to_string(), "BH-001".to_string());
    ledger.append("finding_resolved", fields.clone()).unwrap();

    ledger
}

#[test]
fn test_where_eq_filter_in_template() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        "[protocol]\nname = \"test\"\nversion = \"1.0.0\"\ndescription = \"Test\"\n[paths]\nmanaged = []\ndata_dir = \"data\"\nrender_dir = \"rendered\"\n",
    ).unwrap();
    std::fs::write(
        config_dir.join("states.toml"),
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n",
    )
    .unwrap();
    std::fs::write(config_dir.join("transitions.toml"), "transitions = []\n").unwrap();

    // Template that filters events by type and counts them
    std::fs::write(
        config_dir.join("count.md.tera"),
        "{{ events | where_eq(attribute='event_type', value='finding_resolved') | length }}",
    )
    .unwrap();

    std::fs::write(
        config_dir.join("renders.toml"),
        "[[renders]]\ntemplate = \"count.md.tera\"\ntarget = \"count.md\"\ntrigger = \"on_event\"\n",
    ).unwrap();

    let config = ProtocolConfig::load(&config_dir).unwrap();
    let ledger = setup_ledger_with_events(dir.path());

    let engine = RenderEngine::new(&config, &config_dir).unwrap();
    let rendered_dir = dir.path().join("rendered");
    std::fs::create_dir_all(&rendered_dir).unwrap();
    // data_dir must be under a managed path — use the same root so E12 passes.
    let root_str = dir.path().to_string_lossy().to_string();
    let data_dir_str = dir.path().join("data").to_string_lossy().to_string();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    let mut manifest =
        sahjhan::manifest::tracker::Manifest::init(&data_dir_str, vec![root_str]).unwrap();
    engine
        .render_all(&ledger, &rendered_dir, &mut manifest, 1)
        .unwrap();

    let output = std::fs::read_to_string(rendered_dir.join("count.md")).unwrap();
    assert_eq!(
        output.trim(),
        "4",
        "where_eq should return all 4 finding_resolved events, got: {}",
        output.trim()
    );
}

#[test]
fn test_unique_by_filter_in_template() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        "[protocol]\nname = \"test\"\nversion = \"1.0.0\"\ndescription = \"Test\"\n[paths]\nmanaged = []\ndata_dir = \"data\"\nrender_dir = \"rendered\"\n",
    ).unwrap();
    std::fs::write(
        config_dir.join("states.toml"),
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n",
    )
    .unwrap();
    std::fs::write(config_dir.join("transitions.toml"), "transitions = []\n").unwrap();

    // Template: filter to finding_resolved, deduplicate by id, count
    std::fs::write(
        config_dir.join("dedup.md.tera"),
        "{{ events | where_eq(attribute='event_type', value='finding_resolved') | unique_by(attribute='fields.id') | length }}",
    ).unwrap();

    std::fs::write(
        config_dir.join("renders.toml"),
        "[[renders]]\ntemplate = \"dedup.md.tera\"\ntarget = \"dedup.md\"\ntrigger = \"on_event\"\n",
    ).unwrap();

    let config = ProtocolConfig::load(&config_dir).unwrap();
    let ledger = setup_ledger_with_events(dir.path());

    let engine = RenderEngine::new(&config, &config_dir).unwrap();
    let rendered_dir = dir.path().join("rendered");
    std::fs::create_dir_all(&rendered_dir).unwrap();
    let root_str = dir.path().to_string_lossy().to_string();
    let data_dir_str = dir.path().join("data").to_string_lossy().to_string();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    let mut manifest =
        sahjhan::manifest::tracker::Manifest::init(&data_dir_str, vec![root_str]).unwrap();
    engine
        .render_all(&ledger, &rendered_dir, &mut manifest, 1)
        .unwrap();

    let output = std::fs::read_to_string(rendered_dir.join("dedup.md")).unwrap();
    // 4 finding_resolved events but only 3 distinct IDs
    assert_eq!(
        output.trim(),
        "3",
        "unique_by should deduplicate by fields.id, got: {}",
        output.trim()
    );
}

#[test]
fn test_unique_by_preserves_last_occurrence() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        "[protocol]\nname = \"test\"\nversion = \"1.0.0\"\ndescription = \"Test\"\n[paths]\nmanaged = []\ndata_dir = \"data\"\nrender_dir = \"rendered\"\n",
    ).unwrap();
    std::fs::write(
        config_dir.join("states.toml"),
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n",
    )
    .unwrap();
    std::fs::write(config_dir.join("transitions.toml"), "transitions = []\n").unwrap();

    // Template: get the seq of the last BH-001 resolution
    std::fs::write(
        config_dir.join("last.md.tera"),
        r#"{% set resolved = events | where_eq(attribute='event_type', value='finding_resolved') | unique_by(attribute='fields.id') %}{% for e in resolved %}{% if e.fields.id == "BH-001" %}{{ e.seq }}{% endif %}{% endfor %}"#,
    ).unwrap();

    std::fs::write(
        config_dir.join("renders.toml"),
        "[[renders]]\ntemplate = \"last.md.tera\"\ntarget = \"last.md\"\ntrigger = \"on_event\"\n",
    )
    .unwrap();

    let config = ProtocolConfig::load(&config_dir).unwrap();
    let ledger = setup_ledger_with_events(dir.path());

    let engine = RenderEngine::new(&config, &config_dir).unwrap();
    let rendered_dir = dir.path().join("rendered");
    std::fs::create_dir_all(&rendered_dir).unwrap();
    let root_str = dir.path().to_string_lossy().to_string();
    let data_dir_str = dir.path().join("data").to_string_lossy().to_string();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    let mut manifest =
        sahjhan::manifest::tracker::Manifest::init(&data_dir_str, vec![root_str]).unwrap();
    engine
        .render_all(&ledger, &rendered_dir, &mut manifest, 1)
        .unwrap();

    let output = std::fs::read_to_string(rendered_dir.join("last.md")).unwrap();
    let seq: u64 = output.trim().parse().expect("output should be a number");
    // The last BH-001 resolution should have a higher seq than the first (seq 5)
    // Genesis=1, finding=2,3,4, resolved=5,6,7,8 — last BH-001 resolved is seq 8
    assert!(
        seq > 5,
        "unique_by should keep last occurrence, got seq: {}",
        seq
    );
}
