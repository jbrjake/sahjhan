// src/gates/query.rs
//
// ## Index
// - [eval-query-gate]  eval_query_gate()  — run a SQL query via DataFusion against the ledger; pass if result matches expected

use crate::config::GateConfig;

use super::evaluator::{GateContext, GateResult};

// [eval-query-gate]
pub(super) fn eval_query_gate(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let sql = match gate.params.get("sql").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return GateResult {
                passed: false,
                gate_type: "query".to_string(),
                description: "SQL query against ledger".to_string(),
                reason: Some("gate missing required 'sql' param".to_string()),
            }
        }
    };

    let expect = gate
        .params
        .get("expect")
        .and_then(|v| v.as_str())
        .unwrap_or("true")
        .to_string();

    let description = format!("SQL: {}", sql);

    // Build a minimal single-threaded tokio runtime — gates are sync but
    // DataFusion is async.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            return GateResult {
                passed: false,
                gate_type: "query".to_string(),
                description,
                reason: Some(format!("failed to build tokio runtime: {}", e)),
            }
        }
    };

    let ledger_path = ctx.ledger.path().to_path_buf();
    let sql_clone = sql.clone();
    let events_config = ctx.config.events.clone();
    let results = rt.block_on(async {
        let engine = crate::query::QueryEngine::from_config(&events_config);
        engine.query_file(&ledger_path, &sql_clone).await
    });

    let rows = match results {
        Ok(r) => r,
        Err(e) => {
            return GateResult {
                passed: false,
                gate_type: "query".to_string(),
                description,
                reason: Some(format!("query execution failed: {}", e)),
            }
        }
    };

    // Expect a single row; take the value of the first column.
    let actual = rows
        .first()
        .and_then(|row| row.values().next())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string());

    let passed = actual == expect;

    GateResult {
        passed,
        gate_type: "query".to_string(),
        description,
        reason: Some(if passed {
            format!("query returned '{}'", actual)
        } else {
            format!("query returned '{}', expected '{}'", actual, expect)
        }),
    }
}
