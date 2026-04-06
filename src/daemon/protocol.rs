// src/daemon/protocol.rs
//
// Wire protocol types for the daemon Unix socket.
// Newline-delimited JSON over SOCK_STREAM.
//
// ## Index
// - Request                   — tagged enum for incoming operations
// - Response                  — output envelope with constructors; ok_status includes idle_seconds/idle_timeout

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// [request]
#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
pub enum Request {
    #[serde(rename = "sign")]
    Sign {
        event_type: String,
        fields: HashMap<String, String>,
    },
    #[serde(rename = "vault_store")]
    VaultStore { name: String, data: String },
    #[serde(rename = "vault_read")]
    VaultRead { name: String },
    #[serde(rename = "vault_delete")]
    VaultDelete { name: String },
    #[serde(rename = "vault_list")]
    VaultList,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "verify")]
    Verify {
        event_type: String,
        fields: HashMap<String, String>,
        proof: String,
    },
}

// [response]
#[derive(Debug, Serialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_entries: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Response {
    pub fn ok_sign(proof: &str) -> Self {
        Self {
            ok: true,
            proof: Some(proof.to_string()),
            data: None,
            names: None,
            pid: None,
            uptime_seconds: None,
            vault_entries: None,
            idle_seconds: None,
            idle_timeout: None,
            verified: None,
            error: None,
            message: None,
        }
    }

    pub fn ok_data(data: &str) -> Self {
        Self {
            ok: true,
            proof: None,
            data: Some(data.to_string()),
            names: None,
            pid: None,
            uptime_seconds: None,
            vault_entries: None,
            idle_seconds: None,
            idle_timeout: None,
            verified: None,
            error: None,
            message: None,
        }
    }

    pub fn ok_names(names: Vec<String>) -> Self {
        Self {
            ok: true,
            proof: None,
            data: None,
            names: Some(names),
            pid: None,
            uptime_seconds: None,
            vault_entries: None,
            idle_seconds: None,
            idle_timeout: None,
            verified: None,
            error: None,
            message: None,
        }
    }

    pub fn ok_status(
        pid: u32,
        uptime_seconds: u64,
        vault_entries: usize,
        idle_seconds: u64,
        idle_timeout: u64,
    ) -> Self {
        Self {
            ok: true,
            proof: None,
            data: None,
            names: None,
            pid: Some(pid),
            uptime_seconds: Some(uptime_seconds),
            vault_entries: Some(vault_entries),
            idle_seconds: Some(idle_seconds),
            idle_timeout: Some(idle_timeout),
            verified: None,
            error: None,
            message: None,
        }
    }

    pub fn ok_empty() -> Self {
        Self {
            ok: true,
            proof: None,
            data: None,
            names: None,
            pid: None,
            uptime_seconds: None,
            vault_entries: None,
            idle_seconds: None,
            idle_timeout: None,
            verified: None,
            error: None,
            message: None,
        }
    }

    pub fn err(error: &str, message: &str) -> Self {
        Self {
            ok: false,
            proof: None,
            data: None,
            names: None,
            pid: None,
            uptime_seconds: None,
            vault_entries: None,
            idle_seconds: None,
            idle_timeout: None,
            verified: None,
            error: Some(error.to_string()),
            message: Some(message.to_string()),
        }
    }

    pub fn ok_verified() -> Self {
        Self {
            ok: true,
            proof: None,
            data: None,
            names: None,
            pid: None,
            uptime_seconds: None,
            vault_entries: None,
            idle_seconds: None,
            idle_timeout: None,
            verified: Some(true),
            error: None,
            message: None,
        }
    }
}
