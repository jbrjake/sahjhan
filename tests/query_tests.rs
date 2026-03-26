use sahjhan::ledger::chain::Ledger;
use sahjhan::query::QueryEngine;
use std::collections::BTreeMap;
use tempfile::TempDir;

/// Helper: create a temp dir and init a ledger with some findings.
fn create_test_ledger(dir: &TempDir, name: &str, findings: &[(&str, &str)]) -> std::path::PathBuf {
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
    let path = create_test_ledger(&dir, "ledger.jsonl", &[
        ("BH-001", "HIGH"),
        ("BH-002", "MEDIUM"),
        ("BH-003", "LOW"),
    ]);

    let engine = QueryEngine::new();
    let results = engine
        .query_file(&path, "SELECT count(*) as cnt FROM events WHERE type = 'finding'")
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["cnt"], "3");
}

#[tokio::test]
async fn test_query_fields_extraction() {
    let dir = TempDir::new().unwrap();
    let path = create_test_ledger(&dir, "ledger.jsonl", &[
        ("BH-001", "CRITICAL"),
    ]);

    let engine = QueryEngine::new();
    // Use the fields column which is a JSON string; extract severity.
    // DataFusion 51 may support json_extract or get_field or similar.
    // We'll try a couple of approaches; the implementation will document what works.
    let results = engine
        .query_file(
            &path,
            "SELECT fields FROM events WHERE type = 'finding'",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    // The fields column contains JSON; verify it has the severity
    let fields_json: serde_json::Value =
        serde_json::from_str(&results[0]["fields"]).unwrap();
    assert_eq!(fields_json["severity"], "CRITICAL");
}

#[tokio::test]
async fn test_query_glob_multiple_files() {
    let dir = TempDir::new().unwrap();
    create_test_ledger(&dir, "ledger_a.jsonl", &[
        ("BH-001", "HIGH"),
        ("BH-002", "MEDIUM"),
    ]);
    create_test_ledger(&dir, "ledger_b.jsonl", &[
        ("BH-003", "LOW"),
    ]);

    let pattern = dir.path().join("ledger_*.jsonl");
    let engine = QueryEngine::new();
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
    let engine = QueryEngine::new();
    let results = engine
        .query_glob(
            pattern.to_str().unwrap(),
            "SELECT DISTINCT _source FROM events ORDER BY _source",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    let sources: Vec<&str> = results.iter().map(|r| r["_source"].as_str()).collect();
    // Both file paths should appear
    assert!(
        sources.contains(&path_a.to_str().unwrap()),
        "Expected {:?} in sources {:?}",
        path_a, sources
    );
    assert!(
        sources.contains(&path_b.to_str().unwrap()),
        "Expected {:?} in sources {:?}",
        path_b, sources
    );
}

#[tokio::test]
async fn test_query_all_columns() {
    let dir = TempDir::new().unwrap();
    let path = create_test_ledger(&dir, "ledger.jsonl", &[("BH-001", "HIGH")]);

    let engine = QueryEngine::new();
    let results = engine
        .query_file(
            &path,
            "SELECT schema, seq, prev, hash, ts, type, engine, protocol, fields FROM events WHERE type = 'finding'",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    let row = &results[0];

    // schema is always 1
    assert_eq!(row["schema"], "1");
    // seq for the first finding after genesis is 1
    assert_eq!(row["seq"], "1");
    // prev and hash should be 64-char hex strings
    assert_eq!(row["prev"].len(), 64);
    assert_eq!(row["hash"].len(), 64);
    // ts should be an ISO 8601 timestamp
    assert!(row["ts"].contains("T"), "ts should be ISO 8601: {}", row["ts"]);
    // type should be "finding"
    assert_eq!(row["type"], "finding");
    // engine should be "sahjhan"
    assert_eq!(row["engine"], "sahjhan");
    // protocol should be "test-proto/1.0.0"
    assert_eq!(row["protocol"], "test-proto/1.0.0");
    // fields should be valid JSON
    assert!(row.contains_key("fields"));
    let _: serde_json::Value = serde_json::from_str(&row["fields"])
        .expect("fields should be valid JSON");
}
