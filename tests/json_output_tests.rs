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
