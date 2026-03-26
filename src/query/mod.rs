//! DataFusion-based SQL query engine over JSONL ledger files.
//!
//! Provides [`QueryEngine`] which embeds Apache DataFusion to run SQL queries
//! over one or more JSONL ledger files. Each file is parsed into Arrow
//! RecordBatches and registered as an in-memory table called `events`.
//!
//! For glob queries, each matched file is unioned together with a virtual
//! `_source` column containing the originating file path.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use datafusion::arrow::array::{Int32Array, Int64Array, RecordBatch, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::datasource::MemTable;
use datafusion::prelude::*;

use crate::ledger::chain::parse_file_entries;
use crate::ledger::entry::LedgerEntry;

/// SQL query engine over JSONL ledger files, powered by Apache DataFusion.
pub struct QueryEngine;

impl QueryEngine {
    /// Create a new query engine instance.
    pub fn new() -> Self {
        Self
    }

    /// Query a single JSONL ledger file.
    ///
    /// The file is registered as an in-memory table called `events` with
    /// columns: schema, seq, prev, hash, ts, type, engine, protocol, fields.
    pub async fn query_file(
        &self,
        path: &Path,
        sql: &str,
    ) -> Result<Vec<BTreeMap<String, String>>, Box<dyn std::error::Error>> {
        let entries = parse_file_entries(path)?;
        let batch = entries_to_batch(&entries, None)?;
        let schema = batch.schema();

        let ctx = SessionContext::new();
        let table = MemTable::try_new(schema, vec![vec![batch]])?;
        ctx.register_table("events", Arc::new(table))?;

        execute_sql(&ctx, sql).await
    }

    /// Query multiple JSONL files matching a glob pattern (UNION ALL).
    ///
    /// Each file is loaded and combined with a virtual `_source` column
    /// containing the file path. The unified table is called `events`.
    pub async fn query_glob(
        &self,
        pattern: &str,
        sql: &str,
    ) -> Result<Vec<BTreeMap<String, String>>, Box<dyn std::error::Error>> {
        let paths: Vec<_> = glob::glob(pattern)?
            .filter_map(|r| r.ok())
            .collect();

        if paths.is_empty() {
            return Err(format!("no files matched pattern: {}", pattern).into());
        }

        let ctx = SessionContext::new();
        let mut union_parts: Vec<String> = Vec::new();

        for (i, path) in paths.iter().enumerate() {
            let entries = parse_file_entries(path)?;
            let source_str = path.to_string_lossy().to_string();
            let batch = entries_to_batch(&entries, Some(&source_str))?;
            let schema = batch.schema();

            let table_name = format!("_events_{}", i);
            let table = MemTable::try_new(schema, vec![vec![batch]])?;
            ctx.register_table(table_name.as_str(), Arc::new(table))?;

            union_parts.push(format!("SELECT * FROM {}", table_name));
        }

        // Create a UNION ALL view called `events`
        let union_sql = union_parts.join(" UNION ALL ");
        let create_view = format!("CREATE VIEW events AS {}", union_sql);
        ctx.sql(&create_view).await?;

        execute_sql(&ctx, sql).await
    }
}

impl Default for QueryEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Arrow schema for the `events` table.
fn events_schema(with_source: bool) -> Schema {
    let mut fields = vec![
        Field::new("schema", DataType::Int32, false),
        Field::new("seq", DataType::Int64, false),
        Field::new("prev", DataType::Utf8, false),
        Field::new("hash", DataType::Utf8, false),
        Field::new("ts", DataType::Utf8, false),
        Field::new("type", DataType::Utf8, false),
        Field::new("engine", DataType::Utf8, false),
        Field::new("protocol", DataType::Utf8, false),
        Field::new("fields", DataType::Utf8, false),
    ];
    if with_source {
        fields.push(Field::new("_source", DataType::Utf8, false));
    }
    Schema::new(fields)
}

/// Convert ledger entries to an Arrow RecordBatch.
///
/// If `source` is Some, a `_source` column is appended with the given value
/// repeated for every row.
fn entries_to_batch(
    entries: &[LedgerEntry],
    source: Option<&str>,
) -> Result<RecordBatch, Box<dyn std::error::Error>> {
    let len = entries.len();

    let schema_col: Vec<i32> = entries.iter().map(|e| e.schema as i32).collect();
    let seq_col: Vec<i64> = entries.iter().map(|e| e.seq as i64).collect();
    let prev_col: Vec<&str> = entries.iter().map(|e| e.prev.as_str()).collect();
    let hash_col: Vec<&str> = entries.iter().map(|e| e.hash.as_str()).collect();
    let ts_col: Vec<&str> = entries.iter().map(|e| e.ts.as_str()).collect();
    let type_col: Vec<&str> = entries.iter().map(|e| e.event_type.as_str()).collect();
    let engine_col: Vec<&str> = entries.iter().map(|e| e.engine.as_str()).collect();
    let protocol_col: Vec<&str> = entries.iter().map(|e| e.protocol.as_str()).collect();

    // Serialize the fields BTreeMap as a JSON string
    let fields_col: Vec<String> = entries
        .iter()
        .map(|e| serde_json::to_string(&e.fields).unwrap_or_default())
        .collect();
    let fields_refs: Vec<&str> = fields_col.iter().map(|s| s.as_str()).collect();

    let arrow_schema = Arc::new(events_schema(source.is_some()));

    let mut columns: Vec<Arc<dyn datafusion::arrow::array::Array>> = vec![
        Arc::new(Int32Array::from(schema_col)),
        Arc::new(Int64Array::from(seq_col)),
        Arc::new(StringArray::from(prev_col)),
        Arc::new(StringArray::from(hash_col)),
        Arc::new(StringArray::from(ts_col)),
        Arc::new(StringArray::from(type_col)),
        Arc::new(StringArray::from(engine_col)),
        Arc::new(StringArray::from(protocol_col)),
        Arc::new(StringArray::from(fields_refs)),
    ];

    if let Some(src) = source {
        let source_col: Vec<&str> = vec![src; len];
        columns.push(Arc::new(StringArray::from(source_col)));
    }

    let batch = RecordBatch::try_new(arrow_schema, columns)?;
    Ok(batch)
}

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
fn array_value_to_string(
    array: &dyn datafusion::arrow::array::Array,
    index: usize,
) -> String {
    use datafusion::arrow::array::{
        BooleanArray, Float32Array, Float64Array, Int8Array, Int16Array,
        Int32Array as I32, Int64Array as I64, UInt8Array, UInt16Array, UInt32Array,
        UInt64Array,
    };

    if array.is_null(index) {
        return "NULL".to_string();
    }

    // Try common types in order of likelihood
    if let Some(a) = array.as_any().downcast_ref::<StringArray>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<I64>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<I32>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<Float64Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<Float32Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<BooleanArray>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<UInt64Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<UInt32Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<UInt16Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<UInt8Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<Int16Array>() {
        return a.value(index).to_string();
    }
    if let Some(a) = array.as_any().downcast_ref::<Int8Array>() {
        return a.value(index).to_string();
    }

    // Fallback: use Arrow's display formatting
    datafusion::arrow::util::display::array_value_to_string(array, index)
        .unwrap_or_else(|_| "<unsupported>".to_string())
}
