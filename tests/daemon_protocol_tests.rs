use sahjhan::daemon::protocol::{Request, Response};

#[test]
fn test_parse_sign_request() {
    let json = r#"{"op": "sign", "event_type": "quiz_answered", "fields": {"score": "5"}}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Sign { event_type, fields } => {
            assert_eq!(event_type, "quiz_answered");
            assert_eq!(fields.get("score").unwrap(), "5");
        }
        _ => panic!("Expected Sign request"),
    }
}

#[test]
fn test_parse_vault_store_request() {
    let json = r#"{"op": "vault_store", "name": "quiz-bank", "data": "aGVsbG8="}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::VaultStore { name, data } => {
            assert_eq!(name, "quiz-bank");
            assert_eq!(data, "aGVsbG8=");
        }
        _ => panic!("Expected VaultStore"),
    }
}

#[test]
fn test_parse_vault_read_request() {
    let json = r#"{"op": "vault_read", "name": "quiz-bank"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::VaultRead { name } => assert_eq!(name, "quiz-bank"),
        _ => panic!("Expected VaultRead"),
    }
}

#[test]
fn test_parse_vault_delete_request() {
    let json = r#"{"op": "vault_delete", "name": "quiz-bank"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::VaultDelete { name } => assert_eq!(name, "quiz-bank"),
        _ => panic!("Expected VaultDelete"),
    }
}

#[test]
fn test_parse_vault_list_request() {
    let json = r#"{"op": "vault_list"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    assert!(matches!(req, Request::VaultList));
}

#[test]
fn test_parse_status_request() {
    let json = r#"{"op": "status"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    assert!(matches!(req, Request::Status));
}

#[test]
fn test_parse_unknown_op() {
    let json = r#"{"op": "unknown_thing"}"#;
    let result = serde_json::from_str::<Request>(json);
    assert!(result.is_err());
}

#[test]
fn test_parse_malformed_json() {
    let json = r#"not json at all"#;
    let result = serde_json::from_str::<Request>(json);
    assert!(result.is_err());
}

#[test]
fn test_serialize_ok_proof_response() {
    let resp = Response::ok_sign("abcdef1234");
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["proof"], "abcdef1234");
}

#[test]
fn test_serialize_ok_data_response() {
    let resp = Response::ok_data("aGVsbG8=");
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"], "aGVsbG8=");
}

#[test]
fn test_serialize_ok_names_response() {
    let resp = Response::ok_names(vec!["a".to_string(), "b".to_string()]);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["names"], serde_json::json!(["a", "b"]));
}

#[test]
fn test_serialize_ok_status_response() {
    let resp = Response::ok_status(12345, 3600, 2, 0, 0, false);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["pid"], 12345);
    assert_eq!(v["uptime_seconds"], 3600);
    assert_eq!(v["vault_entries"], 2);
}

#[test]
fn test_serialize_ok_empty_response() {
    let resp = Response::ok_empty();
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert!(v.get("proof").is_none());
    assert!(v.get("data").is_none());
}

#[test]
fn test_serialize_error_response() {
    let resp = Response::err("auth_failed", "caller not in manifest");
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"], "auth_failed");
    assert_eq!(v["message"], "caller not in manifest");
}

#[test]
fn test_parse_verify_request() {
    let json = r#"{"op": "verify", "event_type": "quiz_answered", "fields": {"score": "5"}, "proof": "abcdef"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Verify {
            event_type,
            fields,
            proof,
        } => {
            assert_eq!(event_type, "quiz_answered");
            assert_eq!(fields.get("score").unwrap(), "5");
            assert_eq!(proof, "abcdef");
        }
        _ => panic!("Expected Verify request"),
    }
}

#[test]
fn test_serialize_ok_verified_response() {
    let resp = Response::ok_verified();
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["verified"], true);
}

#[test]
fn test_serialize_ok_status_includes_idle_fields() {
    let resp = Response::ok_status(12345, 3600, 2, 120, 0, false);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["pid"], 12345);
    assert_eq!(v["uptime_seconds"], 3600);
    assert_eq!(v["vault_entries"], 2);
    assert_eq!(v["idle_seconds"], 120);
    assert_eq!(v["idle_timeout"], 0);
}

#[test]
fn test_serialize_non_status_omits_idle_fields() {
    let resp = Response::ok_sign("abcdef");
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.get("idle_seconds").is_none());
    assert!(v.get("idle_timeout").is_none());
}

#[test]
fn test_serialize_ok_status_includes_enforcement_active() {
    let resp = Response::ok_status(12345, 3600, 2, 0, 0, true);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["enforcement_active"], true);
}

#[test]
fn test_serialize_ok_status_enforcement_inactive() {
    let resp = Response::ok_status(12345, 3600, 2, 0, 0, false);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["enforcement_active"], false);
}

#[test]
fn test_parse_vault_store_reserved_name() {
    let json = r#"{"op": "vault_store", "name": "_enforcement", "data": "aGVsbG8="}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    assert!(matches!(req, Request::VaultStore { .. }));
}

#[test]
fn test_parse_enforcement_read_request() {
    let json = r#"{"op": "enforcement_read"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    assert!(matches!(req, Request::EnforcementRead));
}

#[test]
fn test_parse_enforcement_write_request() {
    let json = r#"{"op": "enforcement_write", "data": "eyJzdGF0ZSI6ICJhY3RpdmUifQ=="}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::EnforcementWrite { data } => {
            assert_eq!(data, "eyJzdGF0ZSI6ICJhY3RpdmUifQ==");
        }
        _ => panic!("Expected EnforcementWrite"),
    }
}

#[test]
fn test_parse_enforcement_update_request() {
    let json = r#"{"op": "enforcement_update", "patch": "eyJhY3RpdmUiOiB0cnVlfQ=="}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::EnforcementUpdate { patch } => {
            assert_eq!(patch, "eyJhY3RpdmUiOiB0cnVlfQ==");
        }
        _ => panic!("Expected EnforcementUpdate"),
    }
}
