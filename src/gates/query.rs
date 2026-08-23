// src/gates/query.rs
//
// ## Index
// - [eval-query-gate]  eval_query_gate()  — run a SQL query via DataFusion against the ledger; pass if result matches expected
// - [resolve-gate-sql] resolve_gate_sql() — resolve a query gate's predicate from `sql` (inline) or `query` (named)

use crate::config::{GateConfig, ProtocolConfig};

use super::evaluator::{GateContext, GateResult};
use super::template::{find_unresolved_vars, resolve_template_plain};
use super::types::{build_template_vars, validate_template_fields};

// [resolve-gate-sql]
/// Resolve a query gate's SQL from either `sql` (inline) or `query` (a name
/// declared in `[queries]`).
///
/// Exactly one form may be present. Returns `Err(reason)` describing the defect
/// otherwise — the same conditions `validate_deep` rejects statically, repeated
/// here because a gate can also arrive from a hook config at runtime.
pub(crate) fn resolve_gate_sql(
    gate: &GateConfig,
    config: &ProtocolConfig,
) -> Result<String, String> {
    let inline = gate.params.get("sql").and_then(|v| v.as_str());
    let named = gate.params.get("query").and_then(|v| v.as_str());
    match (inline, named) {
        (Some(_), Some(_)) => Err("gate has both 'sql' and 'query' params — use one".to_string()),
        (Some(sql), None) => Ok(sql.to_string()),
        (None, Some(name)) => match config.queries.get(name) {
            Some(q) => Ok(q.sql.clone()),
            None => Err(format!("gate references undeclared query '{}'", name)),
        },
        (None, None) => Err("gate missing required 'sql' or 'query' param".to_string()),
    }
}

// [eval-query-gate]
pub(super) fn eval_query_gate(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let raw_sql = match resolve_gate_sql(gate, ctx.config) {
        Ok(s) => s,
        Err(reason) => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "query".to_string(),
                description: "SQL query against ledger".to_string(),
                reason: Some(reason),
                intent: None,
                attestation: None,
            }
        }
    };

    // A named query keeps its name in the description so `gate check` output
    // names the predicate rather than dumping a wall of SQL twice.
    let query_name = gate.params.get("query").and_then(|v| v.as_str());
    let describe = |sql: &str| match query_name {
        Some(name) => format!("query '{}': {}", name, sql),
        None => format!("SQL: {}", sql),
    };

    // Validate template fields before interpolation.
    if let Err(reason) = validate_template_fields(&raw_sql, ctx) {
        return GateResult {
            passed: false,
            evaluable: true,
            gate_type: "query".to_string(),
            description: describe(&raw_sql),
            reason: Some(reason),
            intent: None,
            attestation: None,
        };
    }

    // Interpolate template variables (plain — no shell escaping for SQL).
    let vars = build_template_vars(ctx);
    let sql = resolve_template_plain(&raw_sql, &vars);

    let unresolved = find_unresolved_vars(&sql);
    if !unresolved.is_empty() {
        return GateResult {
            passed: false,
            evaluable: false,
            gate_type: "query".to_string(),
            description: describe(&raw_sql),
            reason: Some(format!(
                "unevaluable (requires arg: {})",
                unresolved.join(", ")
            )),
            intent: None,
            attestation: None,
        };
    }

    let expect = gate
        .params
        .get("expect")
        .and_then(|v| v.as_str())
        .unwrap_or("true")
        .to_string();

    let description = describe(&sql);

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
                evaluable: true,
                gate_type: "query".to_string(),
                description,
                reason: Some(format!("failed to build tokio runtime: {}", e)),
                intent: None,
                attestation: None,
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
                evaluable: true,
                gate_type: "query".to_string(),
                description,
                reason: Some(format!("query execution failed: {}", e)),
                intent: None,
                attestation: None,
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

    // A named predicate says its name when it refuses. Without it a caller
    // reading "query returned 'false'" — a batch printing one line per refused
    // item, most of all — cannot tell which of a transition's gates stopped it,
    // and a refusal nobody can attribute is a refusal nobody can clear.
    let named = |text: String| match query_name {
        Some(name) => format!("query '{}' {}", name, text),
        None => format!("query {}", text),
    };

    GateResult {
        passed,
        evaluable: true,
        gate_type: "query".to_string(),
        description,
        reason: Some(if passed {
            named(format!("returned '{}'", actual))
        } else {
            named(format!("returned '{}', expected '{}'", actual, expect))
        }),
        intent: None,
        attestation: None,
    }
}
