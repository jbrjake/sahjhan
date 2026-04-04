// src/cli/verify_cmd.rs
//
// CLI handler for `sahjhan verify`.
//
// ## Index
// - [cmd-verify]              cmd_verify()  — verify HMAC proof via daemon

use crate::cli::commands;
use crate::cli::daemon_cmd;
use std::collections::HashMap;

// [cmd-verify]
pub fn cmd_verify(config_dir: &str, event_type: &str, fields: &[String], proof: &str) -> i32 {
    let socket_path = match daemon_cmd::resolve_socket_path(config_dir) {
        Ok(p) => p,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let mut field_map = HashMap::new();
    for f in fields {
        if let Some((k, v)) = f.split_once('=') {
            field_map.insert(k.to_string(), v.to_string());
        } else {
            eprintln!("error: invalid field format '{}', expected key=value", f);
            return commands::EXIT_USAGE_ERROR;
        }
    }

    let request = serde_json::json!({
        "op": "verify",
        "event_type": event_type,
        "fields": field_map,
        "proof": proof,
    });

    match daemon_cmd::connect_and_request(&socket_path, &request.to_string()) {
        Ok(response) => {
            let v: serde_json::Value = match serde_json::from_str(&response) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: invalid response from daemon: {}", e);
                    return commands::EXIT_CONFIG_ERROR;
                }
            };
            if v["ok"] == true {
                commands::EXIT_SUCCESS
            } else {
                eprintln!(
                    "error: {}",
                    v["message"].as_str().unwrap_or("invalid proof")
                );
                commands::EXIT_INTEGRITY_ERROR
            }
        }
        Err(msg) => {
            eprintln!("error: {}", msg);
            commands::EXIT_CONFIG_ERROR
        }
    }
}
