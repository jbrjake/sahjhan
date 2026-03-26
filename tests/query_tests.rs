use sahjhan::config::EventConfig;
use sahjhan::config::EventFieldConfig;
use sahjhan::ledger::chain::Ledger;
use sahjhan::query::QueryEngine;
use std::collections::{BTreeMap, HashMap};
use tempfile::TempDir;

/// Build a minimal event config with the given event types and field names.
fn test_events() -> HashMap<String, EventConfig> {
    let mut events = HashMap::new();
    events.insert(
        "finding".to_string(),
        EventConfig {
            description: "A finding".to_string(),
            fields: vec![
                EventFieldConfig {
                    name: "id".to_string(),
                    field_type: "string".to_string(),
                    pattern: None,
                    values: None,
                },
                EventFieldConfig {
                    name: "severity".to_string(),
                    field_type: "string".to_string(),
                    pattern: None,
                    values: None,
                },
            ],
        },
    );
    events
}

/// Create a temp ledger with some findings.
fn create_test_ledger(
    dir: &TempDir,
    name: &str,
    findings: &[(&str, &str)],
) -> std::path::PathBuf {
    let path = dir.path().join(name);
    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    for (id, severity) in findings {
        let mut fields = BTreeMap::new();
        fields.insert("id".to_string(), id.to_string());
        fields.insert("severity".to_string(), severity.to_string());
        ledger.append("finding", fields).unwrap();
    }

    path
}

#[tokio::test]
async fn test_query_count_by_type() {
    let dir = TempDir::new().unwrap();
    let path = create_test_ledger(
        &dir,
        "ledger.jsonl",
        &[("BH-001", "HIGH"), ("BH-002", "MEDIUM"), ("BH-003", "LOW")],
    );

    let engine = QueryEngine::from_config(&test_events());
    let results = engine
        .query_file(
            &path,
            "SELECT count(*) as cnt FROM events WHERE type = 'finding'",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["cnt"], "3");
}

#[tokio::test]
async fn test_query_native_field_columns() {
    let dir = TempDir::new().unwrap();
    let path = create_test_ledger(&dir, "ledger.jsonl", &[("BH-001", "CRITICAL")]);

    let engine = QueryEngine::from_config(&test_events());

    // severity is a native Arrow column — query it directly in SQL
    let results = engine
        .query_file(
            &path,
            "SELECT id, severity FROM events WHERE severity = 'CRITICAL'",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"], "BH-001");
    assert_eq!(results[0]["severity"], "CRITICAL");
}

#[tokio::test]
async fn test_query_group_by_field() {
    let dir = TempDir::new().unwrap();
    let path = create_test_ledger(
        &dir,
        "ledger.jsonl",
        &[
            ("BH-001", "HIGH"),
            ("BH-002", "HIGH"),
            ("BH-003", "LOW"),
        ],
    );

    let engine = QueryEngine::from_config(&test_events());
    let results = engine
        .query_file(
            &path,
            "SELECT severity, count(*) as cnt FROM events WHERE type = 'finding' GROUP BY severity ORDER BY severity",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["severity"], "HIGH");
    assert_eq!(results[0]["cnt"], "2");
    assert_eq!(results[1]["severity"], "LOW");
    assert_eq!(results[1]["cnt"], "1");
}

#[tokio::test]
async fn test_query_null_for_missing_fields() {
    let dir = TempDir::new().unwrap();
    let path = create_test_ledger(&dir, "ledger.jsonl", &[("BH-001", "HIGH")]);

    let engine = QueryEngine::from_config(&test_events());

    // Genesis event doesn't have id/severity fields — they should be NULL
    let results = engine
        .query_file(
            &path,
            "SELECT type, id FROM events WHERE type = 'genesis'",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["type"], "genesis");
    assert_eq!(results[0]["id"], "NULL");
}

#[tokio::test]
async fn test_query_glob_multiple_files() {
    let dir = TempDir::new().unwrap();
    create_test_ledger(
        &dir,
        "ledger_a.jsonl",
        &[("BH-001", "HIGH"), ("BH-002", "MEDIUM")],
    );
    create_test_ledger(&dir, "ledger_b.jsonl", &[("BH-003", "LOW")]);

    let pattern = dir.path().join("ledger_*.jsonl");
    let engine = QueryEngine::from_config(&test_events());
    let results = engine
        .query_glob(
            pattern.to_str().unwrap(),
            "SELECT count(*) as cnt FROM events WHERE type = 'finding'",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["cnt"], "3");
}

#[tokio::test]
async fn test_query_source_column() {
    let dir = TempDir::new().unwrap();
    let path_a = create_test_ledger(&dir, "ledger_a.jsonl", &[("BH-001", "HIGH")]);
    let path_b = create_test_ledger(&dir, "ledger_b.jsonl", &[("BH-002", "LOW")]);

    let pattern = dir.path().join("ledger_*.jsonl");
    let engine = QueryEngine::from_config(&test_events());
    let results = engine
        .query_glob(
            pattern.to_str().unwrap(),
            "SELECT DISTINCT _source FROM events ORDER BY _source",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    let sources: Vec<&str> = results.iter().map(|r| r["_source"].as_str()).collect();
    assert!(sources.contains(&path_a.to_str().unwrap()));
    assert!(sources.contains(&path_b.to_str().unwrap()));
}

#[tokio::test]
async fn test_query_envelope_columns() {
    let dir = TempDir::new().unwrap();
    let path = create_test_ledger(&dir, "ledger.jsonl", &[("BH-001", "HIGH")]);

    let engine = QueryEngine::from_config(&test_events());
    let results = engine
        .query_file(
            &path,
            "SELECT schema, seq, prev, hash, ts, type, engine, protocol FROM events WHERE type = 'finding'",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    let row = &results[0];
    assert_eq!(row["schema"], "1");
    assert_eq!(row["seq"], "1");
    assert_eq!(row["prev"].len(), 64);
    assert_eq!(row["hash"].len(), 64);
    assert!(row["ts"].contains('T'));
    assert_eq!(row["type"], "finding");
    assert_eq!(row["engine"], "sahjhan");
    assert_eq!(row["protocol"], "test-proto/1.0.0");
}
