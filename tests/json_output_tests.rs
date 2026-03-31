// tests/json_output_tests.rs
//
// Tests for JSON envelope output.

use serde_json::Value;

/// Parse JSON output string and return the parsed value.
fn parse_envelope(json_str: &str) -> Value {
    serde_json::from_str(json_str).expect("valid JSON")
}

#[test]
fn test_ok_envelope_has_schema_version() {
    use sahjhan::cli::output::{CommandOutput, CommandResult};
    let result = CommandResult::ok("status", "test_data".to_string());
    let json_str = result.to_json();
    let v = parse_envelope(&json_str);
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "status");
    assert_eq!(v["data"], "test_data");
    assert!(v.get("error").is_none() || v["error"].is_null());
}

#[test]
fn test_err_envelope_has_error_fields() {
    use sahjhan::cli::output::{CommandOutput, CommandResult};
    let result: CommandResult<String> =
        CommandResult::err("status", 2, "integrity_error", "chain invalid".to_string());
    let json_str = result.to_json();
    let v = parse_envelope(&json_str);
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["ok"], false);
    assert_eq!(v["command"], "status");
    assert!(v.get("data").is_none() || v["data"].is_null());
    assert_eq!(v["error"]["code"], "integrity_error");
    assert_eq!(v["error"]["message"], "chain invalid");
}

#[test]
fn test_err_with_details_envelope() {
    use sahjhan::cli::output::{CommandOutput, CommandResult};
    let details = serde_json::json!({"gate": "file_exists", "path": "/missing"});
    let result: CommandResult<String> = CommandResult::err_with_details(
        "transition",
        1,
        "gate_blocked",
        "gate failed".to_string(),
        details.clone(),
    );
    let json_str = result.to_json();
    let v = parse_envelope(&json_str);
    assert_eq!(v["error"]["details"]["gate"], "file_exists");
}

#[test]
fn test_ok_text_output() {
    use sahjhan::cli::output::{CommandOutput, CommandResult};
    let result = CommandResult::ok("test", "hello world".to_string());
    assert_eq!(result.to_text(), "hello world");
}

#[test]
fn test_err_text_output() {
    use sahjhan::cli::output::{CommandOutput, CommandResult};
    let result: CommandResult<String> =
        CommandResult::err("test", 2, "integrity_error", "chain invalid".to_string());
    assert_eq!(result.to_text(), "error: chain invalid\n");
}

#[test]
fn test_exit_codes() {
    use sahjhan::cli::output::{CommandOutput, CommandResult};
    let ok: CommandResult<String> = CommandResult::ok("test", "data".to_string());
    assert_eq!(ok.exit_code(), 0);
    let err: CommandResult<String> =
        CommandResult::err("test", 2, "integrity_error", "bad".to_string());
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn test_legacy_result_json() {
    use sahjhan::cli::output::{CommandOutput, LegacyResult};
    let legacy = LegacyResult::new("init", 0);
    let v = parse_envelope(&legacy.to_json());
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "init");
}

#[test]
fn test_legacy_result_error_json() {
    use sahjhan::cli::output::{CommandOutput, LegacyResult};
    let legacy = LegacyResult::with_error("init", 3, "config_error", "missing file");
    let v = parse_envelope(&legacy.to_json());
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "config_error");
}

#[test]
fn test_status_data_json_fields() {
    use sahjhan::cli::output::*;
    let data = StatusData {
        state: "idle".to_string(),
        event_count: 1,
        chain_valid: true,
        chain_error: None,
        sets: vec![SetSummaryData {
            name: "check".to_string(),
            completed: 1,
            total: 2,
            members: vec![
                MemberData {
                    name: "tests".to_string(),
                    done: true,
                },
                MemberData {
                    name: "lint".to_string(),
                    done: false,
                },
            ],
        }],
        transitions: vec![TransitionSummaryData {
            command: "begin".to_string(),
            from: "idle".to_string(),
            to: "working".to_string(),
            ready: true,
            gates: vec![],
        }],
    };
    let result = CommandResult::ok("status", data);
    let v = parse_envelope(&result.to_json());
    assert_eq!(v["data"]["state"], "idle");
    assert_eq!(v["data"]["event_count"], 1);
    assert_eq!(v["data"]["chain_valid"], true);
    assert_eq!(v["data"]["sets"][0]["name"], "check");
    assert_eq!(v["data"]["sets"][0]["completed"], 1);
    assert_eq!(v["data"]["sets"][0]["members"][0]["done"], true);
    assert_eq!(v["data"]["transitions"][0]["command"], "begin");
    assert_eq!(v["data"]["transitions"][0]["ready"], true);
}

#[test]
fn test_status_data_text_matches_current_format() {
    use sahjhan::cli::output::*;
    let data = StatusData {
        state: "idle".to_string(),
        event_count: 1,
        chain_valid: true,
        chain_error: None,
        sets: vec![SetSummaryData {
            name: "check".to_string(),
            completed: 1,
            total: 2,
            members: vec![
                MemberData {
                    name: "tests".to_string(),
                    done: true,
                },
                MemberData {
                    name: "lint".to_string(),
                    done: false,
                },
            ],
        }],
        transitions: vec![TransitionSummaryData {
            command: "begin".to_string(),
            from: "idle".to_string(),
            to: "working".to_string(),
            ready: true,
            gates: vec![],
        }],
    };
    let text = data.to_string();
    assert!(text.contains("state: idle (1 events, chain valid)"));
    assert!(text.contains("check: 1/2"));
    assert!(text.contains("\u{2713} tests"));
    assert!(text.contains("\u{00B7} lint"));
    assert!(text.contains("begin: ready"));
}

#[test]
fn test_log_data_json_has_full_hashes() {
    use sahjhan::cli::output::*;
    use std::collections::BTreeMap;
    let mut fields = BTreeMap::new();
    fields.insert("from".to_string(), "idle".to_string());
    let data = LogData {
        entries: vec![EntryData {
            seq: 0,
            timestamp: "2026-03-30T00:00:00.000Z".to_string(),
            event_type: "genesis".to_string(),
            hash: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            fields,
        }],
    };
    let result = CommandResult::ok("log_dump", data);
    let v = parse_envelope(&result.to_json());
    assert_eq!(v["data"]["entries"][0]["hash"].as_str().unwrap().len(), 64);
}

#[test]
fn test_log_data_text_truncates_hashes() {
    use sahjhan::cli::output::*;
    use std::collections::BTreeMap;
    let data = LogData {
        entries: vec![EntryData {
            seq: 0,
            timestamp: "2026-03-30T00:00:00.000Z".to_string(),
            event_type: "genesis".to_string(),
            hash: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            fields: BTreeMap::new(),
        }],
    };
    let text = data.to_string();
    assert!(text.contains("hash=abcdef123456"));
    assert!(!text.contains("abcdef1234567890abcdef1234567890"));
}

#[test]
fn test_gate_check_data_json() {
    use sahjhan::cli::output::*;
    let data = GateCheckData {
        transition: "begin".to_string(),
        current_state: "idle".to_string(),
        candidates: vec![CandidateData {
            from: "idle".to_string(),
            to: "working".to_string(),
            gates: vec![],
            all_passed: true,
        }],
        result: "ready".to_string(),
        would_take: Some("working".to_string()),
    };
    let result = CommandResult::ok("gate_check", data);
    let v = parse_envelope(&result.to_json());
    assert_eq!(v["data"]["transition"], "begin");
    assert_eq!(v["data"]["candidates"][0]["all_passed"], true);
    assert_eq!(v["data"]["would_take"], "working");
}

#[test]
fn test_manifest_verify_data_json() {
    use sahjhan::cli::output::*;
    let data = ManifestVerifyData {
        clean: false,
        tracked_count: 3,
        mismatches: vec![MismatchData {
            path: "output/STATUS.md".to_string(),
            expected: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
                .to_string(),
            actual: Some(
                "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
            ),
        }],
    };
    let result = CommandResult::ok("manifest_verify", data);
    let v = parse_envelope(&result.to_json());
    assert_eq!(v["data"]["clean"], false);
    assert_eq!(v["data"]["tracked_count"], 3);
    assert_eq!(v["data"]["mismatches"][0]["path"], "output/STATUS.md");
    assert_eq!(
        v["data"]["mismatches"][0]["expected"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
}

#[test]
fn test_event_only_status_data_json() {
    use sahjhan::cli::output::*;
    let data = EventOnlyStatusData {
        event_count: 42,
        chain_valid: true,
        chain_error: None,
    };
    let result = CommandResult::ok("status", data);
    let v = parse_envelope(&result.to_json());
    assert_eq!(v["data"]["event_count"], 42);
    assert_eq!(v["data"]["chain_valid"], true);
}

// ---------------------------------------------------------------------------
// CLI integration tests — require a real binary
// ---------------------------------------------------------------------------

use assert_cmd::Command;
use tempfile::tempdir;

fn setup_minimal() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    for file in &[
        "protocol.toml",
        "states.toml",
        "transitions.toml",
        "events.toml",
        "renders.toml",
    ] {
        std::fs::copy(format!("examples/minimal/{}", file), config_dir.join(file)).unwrap();
    }
    let templates_dir = config_dir.join("templates");
    std::fs::create_dir_all(&templates_dir).unwrap();
    for file in &["status.md.tera", "history.md.tera"] {
        std::fs::copy(
            format!("examples/minimal/templates/{}", file),
            templates_dir.join(file),
        )
        .unwrap();
    }
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
fn test_cli_status_json_envelope() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "status");
    assert_eq!(v["data"]["state"], "idle");
    assert!(v["data"]["event_count"].as_u64().unwrap() >= 1);
    assert_eq!(v["data"]["chain_valid"], true);
}

#[test]
fn test_cli_status_text_unchanged() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("state: idle"));
    assert!(stdout.contains("chain valid"));
}

#[test]
fn test_cli_set_status_json() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "set",
            "status",
            "check",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["name"], "check");
    assert_eq!(v["data"]["total"], 2);
    assert_eq!(v["data"]["completed"], 0);
}

#[test]
fn test_cli_log_dump_json() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "log", "dump"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["command"], "log_dump");
    let entries = v["data"]["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    assert_eq!(entries[0]["seq"], 0);
    assert!(entries[0]["hash"].as_str().unwrap().len() == 64);
}

#[test]
fn test_cli_log_tail_json() {
    let dir = setup_minimal();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "log", "tail", "1"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["command"], "log_tail");
    let entries = v["data"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["event_type"], "state_transition");
}

#[test]
fn test_cli_gate_check_json_ready() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "gate",
            "check",
            "begin",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["command"], "gate_check");
    assert_eq!(v["data"]["transition"], "begin");
    assert_eq!(v["data"]["current_state"], "idle");
    assert_eq!(v["data"]["result"], "ready (no gates)");
}

#[test]
fn test_cli_gate_check_json_blocked() {
    let dir = setup_minimal();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "gate",
            "check",
            "complete",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["result"], "blocked");
    let candidates = v["data"]["candidates"].as_array().unwrap();
    assert_eq!(candidates[0]["all_passed"], false);
}

#[test]
fn test_cli_manifest_verify_json_clean() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "manifest",
            "verify",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["command"], "manifest_verify");
    assert_eq!(v["data"]["clean"], true);
}
