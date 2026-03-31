// src/cli/log.rs
//
// Ledger log inspection commands.
//
// ## Index
// - [cmd-log-dump] cmd_log_dump() — human-readable ledger dump
// - [cmd-log-verify] cmd_log_verify() — validate hash chain integrity
// - [cmd-log-tail] cmd_log_tail() — show last N events

use super::commands::{
    load_config, open_targeted_ledger, resolve_config_dir, LedgerTargeting, EXIT_INTEGRITY_ERROR,
    EXIT_SUCCESS,
};
use super::output::{CommandOutput, CommandResult, EntryData, LogData};

// ---------------------------------------------------------------------------
// log dump (E19)
// ---------------------------------------------------------------------------

// [cmd-log-dump]
pub fn cmd_log_dump(config_dir: &str, targeting: &LedgerTargeting) -> Box<dyn CommandOutput> {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            return Box::new(CommandResult::<LogData>::err(
                "log_dump",
                code,
                "config_error",
                msg,
            ));
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            let error_code = if code == EXIT_INTEGRITY_ERROR {
                "integrity_error"
            } else {
                "config_error"
            };
            return Box::new(CommandResult::<LogData>::err(
                "log_dump", code, error_code, msg,
            ));
        }
    };

    Box::new(CommandResult::ok(
        "log_dump",
        entries_to_log_data(ledger.entries()),
    ))
}

// ---------------------------------------------------------------------------
// log verify
// ---------------------------------------------------------------------------

// [cmd-log-verify]
pub fn cmd_log_verify(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    match ledger.verify() {
        Ok(()) => {
            println!("chain valid ({} events)", ledger.len());
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("error: chain invalid: {} \u{2014} tampering detected", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// log tail
// ---------------------------------------------------------------------------

// [cmd-log-tail]
pub fn cmd_log_tail(
    config_dir: &str,
    n: usize,
    targeting: &LedgerTargeting,
) -> Box<dyn CommandOutput> {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            return Box::new(CommandResult::<LogData>::err(
                "log_tail",
                code,
                "config_error",
                msg,
            ));
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            let error_code = if code == EXIT_INTEGRITY_ERROR {
                "integrity_error"
            } else {
                "config_error"
            };
            return Box::new(CommandResult::<LogData>::err(
                "log_tail", code, error_code, msg,
            ));
        }
    };

    Box::new(CommandResult::ok(
        "log_tail",
        entries_to_log_data(ledger.tail(n)),
    ))
}

// ---------------------------------------------------------------------------
// Helper: convert ledger entries to LogData
// ---------------------------------------------------------------------------

fn entries_to_log_data(entries: &[crate::ledger::entry::LedgerEntry]) -> LogData {
    LogData {
        entries: entries
            .iter()
            .map(|e| EntryData {
                seq: e.seq,
                timestamp: e.ts.clone(),
                event_type: e.event_type.clone(),
                hash: e.hash.clone(),
                fields: e.fields.clone(),
            })
            .collect(),
    }
}
