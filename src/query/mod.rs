//! DataFusion-based SQL query engine over JSONL ledger files.
//!
//! Provides [`QueryEngine`] which embeds Apache DataFusion to run SQL queries
//! over one or more JSONL ledger files. Each file is parsed into Arrow
//! RecordBatches and registered as an in-memory table called `events`.
//!
//! Field columns are derived from the protocol's event definitions (`events.toml`).
//! Each declared field becomes a native nullable Arrow column, giving DataFusion
//! full columnar access for filtering, grouping, and aggregation — no runtime
//! JSON parsing during queries.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use datafusion::arrow::array::{
    BooleanArray, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::datasource::MemTable;
use datafusion::prelude::*;

use crate::config::EventConfig;
use crate::ledger::chain::parse_file_entries;
use crate::ledger::entry::LedgerEntry;

/// SQL query engine over JSONL ledger files, powered by Apache DataFusion.
///
/// Built from the protocol's event definitions so that every declared field
/// becomes a first-class Arrow column.
pub struct QueryEngine {
    /// Unique field names across all event types, sorted for deterministic schema.
    field_names: Vec<String>,
    /// Maps field name → declared type ("string", "number", "boolean").
    field_types: BTreeMap<String, String>,
}

impl QueryEngine {
    /// Create a query engine from the protocol's event definitions.
    ///
    /// Collects all unique field names across every event type and creates
    /// native Arrow columns for each. System event fields (state_transition,
    /// genesis, _checkpoint) are included automatically.
    pub fn from_config(events: &std::collections::HashMap<String, EventConfig>) -> Self {
        let mut field_types = BTreeMap::new();

        // Declared fields from events.toml
        for event in events.values() {
            for field in &event.fields {
                field_types
                    .entry(field.name.clone())
                    .or_insert_with(|| field.field_type.clone());
            }
        }

        // System event fields (always string)
        for name in &[
            "command",
            "from",
            "to",
            "protocol_name",
            "protocol_version",
            "scope",
            "snapshot",
        ] {
            field_types
                .entry(name.to_string())
                .or_insert_with(|| "string".to_string());
        }

        let field_names: Vec<String> = field_types.keys().cloned().collect();

        Self {
            field_names,
            field_types,
        }
    }

    /// Query a single JSONL ledger file.
    pub async fn query_file(
        &self,
        path: &Path,
        sql: &str,
    ) -> Result<Vec<BTreeMap<String, String>>, Box<dyn std::error::Error>> {
        let entries = parse_file_entries(path)?;
        let batch = self.entries_to_batch(&entries, None)?;
        let schema = batch.schema();

        let ctx = SessionContext::new();
        let table = MemTable::try_new(schema, vec![vec![batch]])?;
        ctx.register_table("events", Arc::new(table))?;

        execute_sql(&ctx, sql).await
    }

    /// Query multiple JSONL files matching a glob pattern (UNION ALL).
    ///
    /// A virtual `_source` column contains the originating file path.
    pub async fn query_glob(
        &self,
        pattern: &str,
        sql: &str,
    ) -> Result<Vec<BTreeMap<String, String>>, Box<dyn std::error::Error>> {
        let paths: Vec<_> = glob::glob(pattern)?.filter_map(|r| r.ok()).collect();

        if paths.is_empty() {
            return Err(format!("no files matched pattern: {}", pattern).into());
        }

        let ctx = SessionContext::new();
        let mut union_parts: Vec<String> = Vec::new();

        for (i, path) in paths.iter().enumerate() {
            let entries = parse_file_entries(path)?;
            let source_str = path.to_string_lossy().to_string();
            let batch = self.entries_to_batch(&entries, Some(&source_str))?;
            let schema = batch.schema();

            let table_name = format!("_events_{}", i);
            let table = MemTable::try_new(schema, vec![vec![batch]])?;
            ctx.register_table(table_name.as_str(), Arc::new(table))?;

            union_parts.push(format!("SELECT * FROM {}", table_name));
        }

        let union_sql = union_parts.join(" UNION ALL ");
        let create_view = format!("CREATE VIEW events AS {}", union_sql);
        ctx.sql(&create_view).await?;

        execute_sql(&ctx, sql).await
    }

    /// Build the Arrow schema: 8 envelope columns + one column per declared field.
    fn build_schema(&self, with_source: bool) -> Schema {
        let mut fields = vec![
            Field::new("schema", DataType::Int32, false),
            Field::new("seq", DataType::Int64, false),
            Field::new("prev", DataType::Utf8, false),
            Field::new("hash", DataType::Utf8, false),
            Field::new("ts", DataType::Utf8, false),
            Field::new("type", DataType::Utf8, false),
            Field::new("engine", DataType::Utf8, false),
            Field::new("protocol", DataType::Utf8, false),
        ];

        // One nullable column per declared field
        for name in &self.field_names {
            let dt = match self.field_types.get(name).map(|s| s.as_str()) {
                Some("number") => DataType::Float64,
                Some("boolean") => DataType::Boolean,
                _ => DataType::Utf8, // "string" or unknown → Utf8
            };
            fields.push(Field::new(name, dt, true)); // nullable
        }

        if with_source {
            fields.push(Field::new("_source", DataType::Utf8, false));
        }

        Schema::new(fields)
    }

    /// Convert ledger entries to an Arrow RecordBatch with native field columns.
    fn entries_to_batch(
        &self,
        entries: &[LedgerEntry],
        source: Option<&str>,
    ) -> Result<RecordBatch, Box<dyn std::error::Error>> {
        let len = entries.len();

        // Envelope columns
        let schema_col: Vec<i32> = entries.iter().map(|e| e.schema as i32).collect();
        let seq_col: Vec<i64> = entries.iter().map(|e| e.seq as i64).collect();
        let prev_col: Vec<&str> = entries.iter().map(|e| e.prev.as_str()).collect();
        let hash_col: Vec<&str> = entries.iter().map(|e| e.hash.as_str()).collect();
        let ts_col: Vec<&str> = entries.iter().map(|e| e.ts.as_str()).collect();
        let type_col: Vec<&str> = entries.iter().map(|e| e.event_type.as_str()).collect();
        let engine_col: Vec<&str> = entries.iter().map(|e| e.engine.as_str()).collect();
        let protocol_col: Vec<&str> = entries.iter().map(|e| e.protocol.as_str()).collect();

        let arrow_schema = Arc::new(self.build_schema(source.is_some()));

        let mut columns: Vec<Arc<dyn datafusion::arrow::array::Array>> = vec![
            Arc::new(Int32Array::from(schema_col)),
            Arc::new(Int64Array::from(seq_col)),
            Arc::new(StringArray::from(prev_col)),
            Arc::new(StringArray::from(hash_col)),
            Arc::new(StringArray::from(ts_col)),
            Arc::new(StringArray::from(type_col)),
            Arc::new(StringArray::from(engine_col)),
            Arc::new(StringArray::from(protocol_col)),
        ];

        // Field columns — one per declared field name
        for name in &self.field_names {
            let dt = self.field_types.get(name).map(|s| s.as_str());
            match dt {
                Some("number") => {
                    let vals: Vec<Option<f64>> = entries
                        .iter()
                        .map(|e| e.fields.get(name).and_then(|v| v.parse::<f64>().ok()))
                        .collect();
                    columns.push(Arc::new(Float64Array::from(vals)));
                }
                Some("boolean") => {
                    let vals: Vec<Option<bool>> = entries
                        .iter()
                        .map(|e| {
                            e.fields.get(name).and_then(|v| match v.as_str() {
                                "true" => Some(true),
                                "false" => Some(false),
                                _ => None,
                            })
                        })
                        .collect();
                    columns.push(Arc::new(BooleanArray::from(vals)));
                }
                _ => {
                    // String (default)
                    let vals: Vec<Option<&str>> = entries
                        .iter()
                        .map(|e| e.fields.get(name).map(|v| v.as_str()))
                        .collect();
                    columns.push(Arc::new(StringArray::from(vals)));
                }
            }
        }

        if let Some(src) = source {
            let source_col: Vec<&str> = vec![src; len];
            columns.push(Arc::new(StringArray::from(source_col)));
        }

        let batch = RecordBatch::try_new(arrow_schema, columns)?;
        Ok(batch)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Execute SQL against a SessionContext and collect results as stringified
/// BTreeMaps.
async fn execute_sql(
    ctx: &SessionContext,
    sql: &str,
) -> Result<Vec<BTreeMap<String, String>>, Box<dyn std::error::Error>> {
    let df = ctx.sql(sql).await?;
    let batches = df.collect().await?;

    let mut rows: Vec<BTreeMap<String, String>> = Vec::new();

    for batch in &batches {
        let schema = batch.schema();
        let num_rows = batch.num_rows();

        for row_idx in 0..num_rows {
            let mut row = BTreeMap::new();
            for (col_idx, field) in schema.fields().iter().enumerate() {
                let col = batch.column(col_idx);
                let value = array_value_to_string(col, row_idx);
                row.insert(field.name().clone(), value);
            }
            rows.push(row);
        }
    }

    Ok(rows)
}

/// Convert a single cell in an Arrow array to a String.
fn array_value_to_string(array: &dyn datafusion::arrow::array::Array, index: usize) -> String {
    if array.is_null(index) {
        return "NULL".to_string();
    }

    if let Some(a) = array.as_any().downcast_ref::<StringArray>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<Int64Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<Int32Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<Float64Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<BooleanArray>() {
        return a.value(index).to_string();
    }

    // Fallback
    datafusion::arrow::util::display::array_value_to_string(array, index)
        .unwrap_or_else(|_| "<unsupported>".to_string())
}
