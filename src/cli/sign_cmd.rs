// src/cli/sign_cmd.rs
//
// Request HMAC-SHA256 proof from the daemon.
//
// ## Index
// - [cmd-sign]                  cmd_sign()                — send sign request to daemon, print proof

use std::collections::HashMap;

use super::commands::{EXIT_INTEGRITY_ERROR, EXIT_SUCCESS};
use super::daemon_cmd;

// [cmd-sign]
pub fn cmd_sign(config_dir: &str, event_type: &str, fields: &[String]) -> i32 {
    let socket_path = match daemon_cmd::resolve_socket_path(config_dir) {
        Ok(p) => p,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Parse fields into HashMap (split on first '=')
    let mut field_map = HashMap::new();
    for field in fields {
        if let Some(pos) = field.find('=') {
            let key = &field[..pos];
            let value = &field[pos + 1..];
            field_map.insert(key.to_string(), value.to_string());
        } else {
            eprintln!("invalid field format (expected key=value): {}", field);
            return EXIT_INTEGRITY_ERROR;
        }
    }

    // Build JSON request
    let request = serde_json::json!({
        "op": "sign",
        "event_type": event_type,
        "fields": field_map,
    });

    let request_json = match serde_json::to_string(&request) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("cannot serialize sign request: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    match daemon_cmd::connect_and_request(&socket_path, &request_json) {
        Ok(response) => {
            // Parse the response to extract the proof field
            match serde_json::from_str::<serde_json::Value>(&response) {
                Ok(val) => {
                    if val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                        if let Some(proof) = val.get("proof").and_then(|v| v.as_str()) {
                            print!("{}", proof);
                            EXIT_SUCCESS
                        } else {
                            eprintln!("daemon response missing proof field");
                            EXIT_INTEGRITY_ERROR
                        }
                    } else {
                        let msg = val
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown error");
                        eprintln!("sign failed: {}", msg);
                        EXIT_INTEGRITY_ERROR
                    }
                }
                Err(e) => {
                    eprintln!("cannot parse daemon response: {}", e);
                    EXIT_INTEGRITY_ERROR
                }
            }
        }
        Err(e) => {
            eprintln!("sign: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}
