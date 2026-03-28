// src/cli/log.rs
//
// Ledger log inspection commands.
//
// ## Index
// - [cmd-log-dump] cmd_log_dump() — human-readable ledger dump
// - [cmd-log-verify] cmd_log_verify() — validate hash chain integrity
// - [cmd-log-tail] cmd_log_tail() — show last N events
// - [print-entries] print_entries() — format and print ledger entries

use super::commands::{
    load_config, open_targeted_ledger, resolve_config_dir, LedgerTargeting, EXIT_INTEGRITY_ERROR,
    EXIT_SUCCESS,
};

// ---------------------------------------------------------------------------
// log dump (E19)
// ---------------------------------------------------------------------------

// [cmd-log-dump]
pub fn cmd_log_dump(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    print_entries(ledger.entries());
    EXIT_SUCCESS
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

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
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
pub fn cmd_log_tail(config_dir: &str, n: usize, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let entries = ledger.tail(n);
    print_entries(entries);
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// Helper: print ledger entries
// ---------------------------------------------------------------------------

// [print-entries]
fn print_entries(entries: &[crate::ledger::entry::LedgerEntry]) {
    for entry in entries {
        // Use the JSONL ts field directly (ISO 8601); trim to readable form.
        let ts = &entry.ts;

        print!(
            "[{}] seq={} type={} hash={}",
            ts,
            entry.seq,
            entry.event_type,
            &entry.hash[..12],
        );

        // Print JSONL fields.
        if !entry.fields.is_empty() {
            let pairs: Vec<String> = entry
                .fields
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            print!(" {{{}}}", pairs.join(", "));
        }

        println!();
    }
}
