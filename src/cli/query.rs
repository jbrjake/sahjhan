// src/cli/query.rs
//
// SQL query commands and output formatting.
//
// ## Index
// - [cmd-query] cmd_query() — SQL queries over ledger events
// - [build-convenience-sql] build_convenience_sql() — build SQL from convenience flags
// - [format-output] format_output() — format query output (table, json, csv, jsonl)

use std::collections::BTreeMap;

use crate::query::QueryEngine;

use super::commands::{
    load_config, resolve_config_dir, resolve_ledger_from_targeting, LedgerTargeting,
    EXIT_INTEGRITY_ERROR, EXIT_SUCCESS,
};

// ---------------------------------------------------------------------------
// query (Task 13)
// ---------------------------------------------------------------------------

// [cmd-query]
#[allow(clippy::too_many_arguments)]
pub fn cmd_query(
    config_dir: &str,
    sql: Option<&str>,
    targeting: &LedgerTargeting,
    glob_pattern: Option<&str>,
    event_type: Option<&str>,
    field_filters: &[String],
    count: bool,
    format: &str,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let engine = QueryEngine::from_config(&config.events);

    // Build SQL from convenience flags if no raw SQL provided
    let effective_sql = if let Some(raw) = sql {
        raw.to_string()
    } else {
        build_convenience_sql(event_type, field_filters, count)
    };

    // Execute query via tokio runtime
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Cannot create async runtime: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    let result = if let Some(pattern) = glob_pattern {
        rt.block_on(engine.query_glob(pattern, &effective_sql))
    } else {
        // Resolve ledger path
        let (ledger_file, _mode) = match resolve_ledger_from_targeting(&config, targeting) {
            Ok(lm) => lm,
            Err((code, msg)) => {
                eprintln!("{}", msg);
                return code;
            }
        };
        rt.block_on(engine.query_file(&ledger_file, &effective_sql))
    };

    match result {
        Ok(rows) => {
            format_output(&rows, format);
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Query failed: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: build SQL from convenience flags
// ---------------------------------------------------------------------------

// [build-convenience-sql]
fn build_convenience_sql(
    event_type: Option<&str>,
    field_filters: &[String],
    count: bool,
) -> String {
    let select = if count {
        "SELECT count(*) as count"
    } else {
        "SELECT *"
    };

    let mut conditions: Vec<String> = Vec::new();

    if let Some(et) = event_type {
        conditions.push(format!("type = '{}'", et.replace('\'', "''")));
    }

    for f in field_filters {
        if let Some((key, value)) = f.split_once('=') {
            conditions.push(format!("{} = '{}'", key, value.replace('\'', "''")));
        }
    }

    if conditions.is_empty() {
        format!("{} FROM events", select)
    } else {
        format!("{} FROM events WHERE {}", select, conditions.join(" AND "))
    }
}

// ---------------------------------------------------------------------------
// Helper: format query output
// ---------------------------------------------------------------------------

// [format-output]
fn format_output(rows: &[BTreeMap<String, String>], format: &str) {
    match format {
        "json" => {
            let json = serde_json::to_string_pretty(rows).unwrap_or_else(|_| "[]".to_string());
            println!("{}", json);
        }
        "jsonl" => {
            for row in rows {
                let line = serde_json::to_string(row).unwrap_or_else(|_| "{}".to_string());
                println!("{}", line);
            }
        }
        "csv" => {
            if rows.is_empty() {
                return;
            }
            // Header from first row's keys
            let keys: Vec<&String> = rows[0].keys().collect();
            println!(
                "{}",
                keys.iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            for row in rows {
                let vals: Vec<&str> = keys
                    .iter()
                    .map(|k| row.get(*k).map(|v| v.as_str()).unwrap_or(""))
                    .collect();
                println!("{}", vals.join(","));
            }
        }
        _ => {
            // table (default)
            if rows.is_empty() {
                println!("(no results)");
                return;
            }

            let keys: Vec<&String> = rows[0].keys().collect();

            // Compute column widths
            let mut widths: Vec<usize> = keys.iter().map(|k| k.len()).collect();
            for row in rows {
                for (i, key) in keys.iter().enumerate() {
                    let val_len = row.get(*key).map(|v| v.len()).unwrap_or(0);
                    if val_len > widths[i] {
                        widths[i] = val_len;
                    }
                }
            }

            // Cap column widths at 40 for readability
            for w in &mut widths {
                if *w > 40 {
                    *w = 40;
                }
            }

            // Print header
            let header: Vec<String> = keys
                .iter()
                .enumerate()
                .map(|(i, k)| format!("{:<width$}", k, width = widths[i]))
                .collect();
            println!("{}", header.join("  "));
            let separator: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            println!("{}", separator.join("  "));

            // Print rows
            for row in rows {
                let vals: Vec<String> = keys
                    .iter()
                    .enumerate()
                    .map(|(i, k)| {
                        let v = row.get(*k).map(|v| v.as_str()).unwrap_or("");
                        let truncated = if v.len() > widths[i] {
                            format!("{}...", &v[..widths[i].saturating_sub(3)])
                        } else {
                            v.to_string()
                        };
                        format!("{:<width$}", truncated, width = widths[i])
                    })
                    .collect();
                println!("{}", vals.join("  "));
            }
        }
    }
}
