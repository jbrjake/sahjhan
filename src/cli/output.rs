// src/cli/output.rs
//
// Structured command output for JSON and text formatting.
//
// ## Index
// - SCHEMA_VERSION              — current output schema version
// - CommandOutput               — trait for type-erased command dispatch
// - CommandResult<T>            — typed command result with envelope
// - ErrorData                   — structured error info
// - LegacyResult                — shim for unconverted commands

use std::fmt::Display;

use serde::Serialize;

pub const SCHEMA_VERSION: u64 = 1;

pub trait CommandOutput {
    fn to_json(&self) -> String;
    fn to_text(&self) -> String;
    fn exit_code(&self) -> i32;
}

pub struct CommandResult<T: Serialize + Display> {
    ok: bool,
    command: String,
    data: Option<T>,
    error: Option<ErrorData>,
    exit_code: i32,
}

#[derive(Serialize, Clone)]
pub struct ErrorData {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl<T: Serialize + Display> CommandResult<T> {
    pub fn ok(command: &str, data: T) -> Self {
        Self {
            ok: true,
            command: command.to_string(),
            data: Some(data),
            error: None,
            exit_code: 0,
        }
    }

    pub fn ok_with_exit_code(command: &str, data: T, exit_code: i32) -> Self {
        Self {
            ok: exit_code == 0,
            command: command.to_string(),
            data: Some(data),
            error: None,
            exit_code,
        }
    }

    pub fn err(command: &str, exit_code: i32, code: &str, message: String) -> Self {
        Self {
            ok: false,
            command: command.to_string(),
            data: None,
            error: Some(ErrorData {
                code: code.to_string(),
                message,
                details: None,
            }),
            exit_code,
        }
    }

    pub fn err_with_details(
        command: &str,
        exit_code: i32,
        code: &str,
        message: String,
        details: serde_json::Value,
    ) -> Self {
        Self {
            ok: false,
            command: command.to_string(),
            data: None,
            error: Some(ErrorData {
                code: code.to_string(),
                message,
                details: Some(details),
            }),
            exit_code,
        }
    }
}

impl<T: Serialize + Display> CommandOutput for CommandResult<T> {
    fn to_json(&self) -> String {
        let mut map = serde_json::Map::new();
        map.insert(
            "schema_version".to_string(),
            serde_json::Value::Number(SCHEMA_VERSION.into()),
        );
        map.insert("ok".to_string(), serde_json::Value::Bool(self.ok));
        map.insert(
            "command".to_string(),
            serde_json::Value::String(self.command.clone()),
        );
        if let Some(ref data) = self.data {
            map.insert(
                "data".to_string(),
                serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(ref error) = self.error {
            map.insert(
                "error".to_string(),
                serde_json::to_value(error).unwrap_or(serde_json::Value::Null),
            );
        }
        serde_json::to_string(&map)
            .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
    }

    fn to_text(&self) -> String {
        if self.ok {
            if let Some(ref data) = self.data {
                data.to_string()
            } else {
                String::new()
            }
        } else if let Some(ref error) = self.error {
            format!("error: {}\n", error.message)
        } else {
            "error: unknown\n".to_string()
        }
    }

    fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

pub struct LegacyResult {
    command: String,
    exit_code: i32,
    error: Option<ErrorData>,
}

impl LegacyResult {
    pub fn new(command: &str, exit_code: i32) -> Self {
        Self {
            command: command.to_string(),
            exit_code,
            error: None,
        }
    }

    pub fn with_error(command: &str, exit_code: i32, code: &str, message: &str) -> Self {
        Self {
            command: command.to_string(),
            exit_code,
            error: Some(ErrorData {
                code: code.to_string(),
                message: message.to_string(),
                details: None,
            }),
        }
    }
}

impl CommandOutput for LegacyResult {
    fn to_json(&self) -> String {
        let mut map = serde_json::Map::new();
        map.insert(
            "schema_version".to_string(),
            serde_json::Value::Number(SCHEMA_VERSION.into()),
        );
        map.insert(
            "ok".to_string(),
            serde_json::Value::Bool(self.exit_code == 0),
        );
        map.insert(
            "command".to_string(),
            serde_json::Value::String(self.command.clone()),
        );
        if let Some(ref error) = self.error {
            map.insert(
                "error".to_string(),
                serde_json::to_value(error).unwrap_or(serde_json::Value::Null),
            );
        }
        serde_json::to_string(&map)
            .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
    }

    fn to_text(&self) -> String {
        String::new()
    }

    fn exit_code(&self) -> i32 {
        self.exit_code
    }
}
