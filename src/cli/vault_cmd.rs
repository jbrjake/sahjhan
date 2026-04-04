// src/cli/vault_cmd.rs
//
// Vault operations — in-memory secret store managed by the daemon.
//
// ## Index
// - [cmd-vault-store]           cmd_vault_store()         — store file contents in daemon vault
// - [cmd-vault-read]            cmd_vault_read()          — read vault entry to stdout
// - [cmd-vault-delete]          cmd_vault_delete()        — delete vault entry
// - [cmd-vault-list]            cmd_vault_list()          — list vault entry names

use std::io::Write;

use base64::Engine;

use super::commands::{EXIT_CONFIG_ERROR, EXIT_INTEGRITY_ERROR, EXIT_SUCCESS};
use super::daemon_cmd;

// [cmd-vault-store]
pub fn cmd_vault_store(config_dir: &str, name: &str, file_path: &str) -> i32 {
    let socket_path = match daemon_cmd::resolve_socket_path(config_dir) {
        Ok(p) => p,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let contents = match std::fs::read(file_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot read file '{}': {}", file_path, e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let encoded = base64::engine::general_purpose::STANDARD.encode(&contents);

    let request = serde_json::json!({
        "op": "vault_store",
        "name": name,
        "data": encoded,
    });

    let request_json = match serde_json::to_string(&request) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("cannot serialize vault_store request: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    match daemon_cmd::connect_and_request(&socket_path, &request_json) {
        Ok(response) => match serde_json::from_str::<serde_json::Value>(&response) {
            Ok(val) => {
                if val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                    eprintln!("OK");
                    EXIT_SUCCESS
                } else {
                    let msg = val
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    eprintln!("vault store failed: {}", msg);
                    EXIT_INTEGRITY_ERROR
                }
            }
            Err(e) => {
                eprintln!("cannot parse daemon response: {}", e);
                EXIT_INTEGRITY_ERROR
            }
        },
        Err(e) => {
            eprintln!("vault store: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// [cmd-vault-read]
pub fn cmd_vault_read(config_dir: &str, name: &str) -> i32 {
    let socket_path = match daemon_cmd::resolve_socket_path(config_dir) {
        Ok(p) => p,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let request = serde_json::json!({
        "op": "vault_read",
        "name": name,
    });

    let request_json = match serde_json::to_string(&request) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("cannot serialize vault_read request: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    match daemon_cmd::connect_and_request(&socket_path, &request_json) {
        Ok(response) => match serde_json::from_str::<serde_json::Value>(&response) {
            Ok(val) => {
                if val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                    if let Some(data) = val.get("data").and_then(|v| v.as_str()) {
                        match base64::engine::general_purpose::STANDARD.decode(data) {
                            Ok(bytes) => {
                                let stdout = std::io::stdout();
                                let mut handle = stdout.lock();
                                if let Err(e) = handle.write_all(&bytes) {
                                    eprintln!("cannot write to stdout: {}", e);
                                    return EXIT_INTEGRITY_ERROR;
                                }
                                EXIT_SUCCESS
                            }
                            Err(e) => {
                                eprintln!("cannot decode vault data: {}", e);
                                EXIT_INTEGRITY_ERROR
                            }
                        }
                    } else {
                        eprintln!("daemon response missing data field");
                        EXIT_INTEGRITY_ERROR
                    }
                } else {
                    let msg = val
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    eprintln!("vault read failed: {}", msg);
                    EXIT_INTEGRITY_ERROR
                }
            }
            Err(e) => {
                eprintln!("cannot parse daemon response: {}", e);
                EXIT_INTEGRITY_ERROR
            }
        },
        Err(e) => {
            eprintln!("vault read: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// [cmd-vault-delete]
pub fn cmd_vault_delete(config_dir: &str, name: &str) -> i32 {
    let socket_path = match daemon_cmd::resolve_socket_path(config_dir) {
        Ok(p) => p,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let request = serde_json::json!({
        "op": "vault_delete",
        "name": name,
    });

    let request_json = match serde_json::to_string(&request) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("cannot serialize vault_delete request: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    match daemon_cmd::connect_and_request(&socket_path, &request_json) {
        Ok(response) => match serde_json::from_str::<serde_json::Value>(&response) {
            Ok(val) => {
                if val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                    eprintln!("OK");
                    EXIT_SUCCESS
                } else {
                    let msg = val
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    eprintln!("vault delete failed: {}", msg);
                    EXIT_INTEGRITY_ERROR
                }
            }
            Err(e) => {
                eprintln!("cannot parse daemon response: {}", e);
                EXIT_INTEGRITY_ERROR
            }
        },
        Err(e) => {
            eprintln!("vault delete: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// [cmd-vault-list]
pub fn cmd_vault_list(config_dir: &str) -> i32 {
    let socket_path = match daemon_cmd::resolve_socket_path(config_dir) {
        Ok(p) => p,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let request = serde_json::json!({
        "op": "vault_list",
    });

    let request_json = match serde_json::to_string(&request) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("cannot serialize vault_list request: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    match daemon_cmd::connect_and_request(&socket_path, &request_json) {
        Ok(response) => match serde_json::from_str::<serde_json::Value>(&response) {
            Ok(val) => {
                if val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                    if let Some(names) = val.get("names").and_then(|v| v.as_array()) {
                        for name in names {
                            if let Some(s) = name.as_str() {
                                println!("{}", s);
                            }
                        }
                    }
                    EXIT_SUCCESS
                } else {
                    let msg = val
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown error");
                    eprintln!("vault list failed: {}", msg);
                    EXIT_INTEGRITY_ERROR
                }
            }
            Err(e) => {
                eprintln!("cannot parse daemon response: {}", e);
                EXIT_INTEGRITY_ERROR
            }
        },
        Err(e) => {
            eprintln!("vault list: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}
