//! Instrumentation for collecting simulation data into column-oriented storage.
//!
//! Uses the `tracing` crate with a custom subscriber that dynamically builds
//! columns from event fields. Schema emerges from recorded events.
//!
//! # Usage
//!
//! ```ignore
//! // In simulation code:
//! tracing::info!(target: "fill", tick, agent_id, qty, price);
//!
//! // In test:
//! instrument::install_subscriber();
//! // ... run simulation ...
//! let recorder = instrument::drain();
//! let fills = &recorder.tables["fill"];
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Record};
use tracing::{Event, Id, Metadata, Subscriber};

/// A column of typed values.
#[derive(Debug, Clone)]
pub enum TypedColumn {
    U64(Vec<u64>),
    I64(Vec<i64>),
    F64(Vec<f64>),
    Bool(Vec<bool>),
    Str(Vec<String>),
}

impl TypedColumn {
    pub fn len(&self) -> usize {
        match self {
            TypedColumn::U64(v) => v.len(),
            TypedColumn::I64(v) => v.len(),
            TypedColumn::F64(v) => v.len(),
            TypedColumn::Bool(v) => v.len(),
            TypedColumn::Str(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A table with dynamically-typed columns.
#[derive(Debug, Clone, Default)]
pub struct DynamicTable {
    pub columns: HashMap<String, TypedColumn>,
    pub row_count: usize,
}

impl DynamicTable {
    /// Pad all columns to the current row count with default values.
    /// Called before adding a new row to ensure all columns stay aligned.
    fn pad_columns_to_row_count(&mut self) {
        for col in self.columns.values_mut() {
            let current_len = col.len();
            if current_len < self.row_count {
                let padding = self.row_count - current_len;
                match col {
                    TypedColumn::U64(v) => v.extend(std::iter::repeat_n(0, padding)),
                    TypedColumn::I64(v) => v.extend(std::iter::repeat_n(0, padding)),
                    TypedColumn::F64(v) => v.extend(std::iter::repeat_n(0.0, padding)),
                    TypedColumn::Bool(v) => v.extend(std::iter::repeat_n(false, padding)),
                    TypedColumn::Str(v) => v.extend(std::iter::repeat_n(String::new(), padding)),
                }
            }
        }
    }
}

/// Collection of tables, keyed by tracing target.
#[derive(Debug, Clone, Default)]
pub struct Recorder {
    pub tables: HashMap<String, DynamicTable>,
}

thread_local! {
    static RECORDER: RefCell<Recorder> = RefCell::default();
}

/// Visitor that extracts event fields into table columns.
struct ColumnVisitor<'a> {
    table: &'a mut DynamicTable,
    /// Current row count - used to pre-pad new columns
    row_count: usize,
}

impl Visit for ColumnVisitor<'_> {
    fn record_u64(&mut self, field: &Field, value: u64) {
        let name = field.name().to_string();
        let col = self.table.columns.entry(name).or_insert_with(|| {
            // New column: pre-pad with zeros for all previous rows
            TypedColumn::U64(vec![0; self.row_count])
        });
        if let TypedColumn::U64(v) = col {
            v.push(value);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        let name = field.name().to_string();
        let col = self
            .table
            .columns
            .entry(name)
            .or_insert_with(|| TypedColumn::I64(vec![0; self.row_count]));
        if let TypedColumn::I64(v) = col {
            v.push(value);
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        let name = field.name().to_string();
        let col = self
            .table
            .columns
            .entry(name)
            .or_insert_with(|| TypedColumn::F64(vec![0.0; self.row_count]));
        if let TypedColumn::F64(v) = col {
            v.push(value);
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        let name = field.name().to_string();
        let col = self
            .table
            .columns
            .entry(name)
            .or_insert_with(|| TypedColumn::Bool(vec![false; self.row_count]));
        if let TypedColumn::Bool(v) = col {
            v.push(value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name().to_string();
        let col = self
            .table
            .columns
            .entry(name)
            .or_insert_with(|| TypedColumn::Str(vec![String::new(); self.row_count]));
        if let TypedColumn::Str(v) = col {
            v.push(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // Convert debug values to strings
        self.record_str(field, &format!("{:?}", value));
    }
}

/// Tracing subscriber that collects events into column-oriented tables.
pub struct DataFrameSubscriber;

impl Subscriber for DataFrameSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        // Only collect info-level events (not spans, not debug/trace)
        metadata.is_event() && *metadata.level() <= tracing::Level::INFO
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        // We don't track spans, just return a dummy ID
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {
        // No-op for spans
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {
        // No-op
    }

    fn event(&self, event: &Event<'_>) {
        let target = event.metadata().target().to_string();

        RECORDER.with(|r| {
            let mut recorder = r.borrow_mut();
            let table = recorder.tables.entry(target).or_default();

            // Pad existing columns to current row count before adding new row
            table.pad_columns_to_row_count();

            // Record all fields from the event
            // Pass current row_count so new columns get pre-padded
            let row_count = table.row_count;
            event.record(&mut ColumnVisitor { table, row_count });

            // Increment row count
            table.row_count += 1;

            // Pad any columns that didn't get a value this row
            table.pad_columns_to_row_count();
        });
    }

    fn enter(&self, _span: &Id) {
        // No-op
    }

    fn exit(&self, _span: &Id) {
        // No-op
    }
}

/// Install the DataFrameSubscriber as the global default.
/// Call this once at the start of a test.
pub fn install_subscriber() {
    let _ = tracing::subscriber::set_global_default(DataFrameSubscriber);
}

/// Drain all recorded data from the thread-local recorder.
/// Returns the Recorder with all tables and their columns.
pub fn drain() -> Recorder {
    RECORDER.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

/// Clear all recorded data without returning it.
pub fn clear() {
    RECORDER.with(|r| *r.borrow_mut() = Recorder::default());
}

// === Polars Integration ===

use polars::prelude::*;

impl DynamicTable {
    /// Convert this table to a polars DataFrame.
    pub fn to_dataframe(&self) -> PolarsResult<DataFrame> {
        let mut columns: Vec<Column> = Vec::new();

        for (name, col) in &self.columns {
            let series = match col {
                TypedColumn::U64(v) => Column::new(name.into(), v),
                TypedColumn::I64(v) => Column::new(name.into(), v),
                TypedColumn::F64(v) => Column::new(name.into(), v),
                TypedColumn::Bool(v) => Column::new(name.into(), v),
                TypedColumn::Str(v) => Column::new(name.into(), v),
            };
            columns.push(series);
        }

        DataFrame::new(columns)
    }
}

impl Recorder {
    /// Convert all tables to polars DataFrames.
    pub fn to_dataframes(&self) -> HashMap<String, DataFrame> {
        self.tables
            .iter()
            .filter_map(|(name, table)| {
                table.to_dataframe().ok().map(|df| (name.clone(), df))
            })
            .collect()
    }
}

/// Drain all recorded data and convert to polars DataFrames.
/// Returns a HashMap of table name -> DataFrame.
pub fn drain_to_dataframes() -> HashMap<String, DataFrame> {
    drain().to_dataframes()
}

/// Save all DataFrames as parquet files in the given directory.
/// Each table becomes `{dir}/{name}.parquet`.
pub fn save_parquet(dfs: &mut HashMap<String, DataFrame>, dir: &std::path::Path) -> PolarsResult<()> {
    std::fs::create_dir_all(dir).map_err(|e| PolarsError::IO {
        error: e.into(),
        msg: None,
    })?;
    for (name, df) in dfs.iter_mut() {
        let path = dir.join(format!("{}.parquet", name));
        let file = std::fs::File::create(&path).map_err(|e| PolarsError::IO {
            error: e.into(),
            msg: None,
        })?;
        ParquetWriter::new(file).finish(df)?;
    }
    Ok(())
}

const MONTH_ABBREVS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Format a SystemTime as `Feb06_20_13` (Mon DD _ HH _ MM) for human-readable run directory names.
fn timestamp_str(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_per_day = 86400u64;
    let secs_today = secs % secs_per_day;
    let hour = secs_today / 3600;
    let minute = (secs_today % 3600) / 60;

    let mut days = (secs / secs_per_day) as i64;
    let mut year = 1970i64;
    loop {
        let days_in_year = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            366
        } else {
            365
        };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut month = 0usize;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = days as u32 + 1;

    format!(
        "{}{:02}_{:02}_{:02}",
        MONTH_ABBREVS[month], day, hour, minute
    )
}

/// Replace non-alphanumeric chars with `_` and truncate for use in directory names.
fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.len() > 60 {
        s[..60].to_string()
    } else {
        s
    }
}

/// RAII guard that clears instrumentation data on creation and saves to parquet on drop.
///
/// Each run gets its own timestamped subdirectory under the parent dir.
/// Call `.get()` after the simulation to drain and access the DataFrames for analysis.
/// On drop, parquet files and a `_ready` sentinel are written.
///
/// ```ignore
/// let mut rec = instrument::ScopedRecorder::new("data", "basic_convergence");
/// // ... run simulation ...
/// let dfs = rec.get();
/// analyze(&dfs);
/// // rec drops → writes data/0206_1430_basic_convergence/*.parquet + _ready
/// ```
pub struct ScopedRecorder {
    run_dir: std::path::PathBuf,
    run_name: String,
    dfs: Option<HashMap<String, DataFrame>>,
}

impl ScopedRecorder {
    /// Create a new recorder. Writes to `{parent}/{MMDD_HHMM}_{name}/`.
    pub fn new(parent: impl Into<std::path::PathBuf>, name: &str) -> Self {
        let run_name = format!("{}_{}", timestamp_str(std::time::SystemTime::now()), sanitize(name));
        let run_dir = parent.into().join(&run_name);
        clear();
        install_subscriber();
        Self {
            run_dir,
            run_name,
            dfs: None,
        }
    }

    /// Drain recorded data and return a reference to the DataFrames.
    /// First call drains from the thread-local recorder; subsequent calls return the cached data.
    pub fn get(&mut self) -> &HashMap<String, DataFrame> {
        self.dfs.get_or_insert_with(drain_to_dataframes)
    }

    pub fn run_name(&self) -> &str {
        &self.run_name
    }

    pub fn run_dir(&self) -> &std::path::Path {
        &self.run_dir
    }
}

impl Drop for ScopedRecorder {
    fn drop(&mut self) {
        let mut dfs = self.dfs.take().unwrap_or_else(drain_to_dataframes);
        if dfs.is_empty() {
            return;
        }
        if let Err(e) = save_parquet(&mut dfs, &self.run_dir) {
            eprintln!("ScopedRecorder({}): failed to write parquet: {}", self.run_name, e);
            return;
        }
        // Write sentinel so watchers know all parquets are complete
        let sentinel = self.run_dir.join("_ready");
        if let Err(e) = std::fs::File::create(&sentinel) {
            eprintln!("ScopedRecorder({}): failed to write _ready sentinel: {}", self.run_name, e);
        } else {
            eprintln!(
                "ScopedRecorder: wrote {} tables to {}",
                dfs.len(),
                self.run_dir.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_recording() {
        clear();

        // Simulate some events
        RECORDER.with(|r| {
            let mut recorder = r.borrow_mut();

            // Manually create a table and add data (bypassing tracing for unit test)
            let table = recorder.tables.entry("test".to_string()).or_default();
            table
                .columns
                .insert("tick".to_string(), TypedColumn::U64(vec![1, 2, 3]));
            table
                .columns
                .insert("value".to_string(), TypedColumn::F64(vec![1.0, 2.0, 3.0]));
            table.row_count = 3;
        });

        let recorder = drain();
        let table = &recorder.tables["test"];

        assert_eq!(table.row_count, 3);
        if let TypedColumn::U64(ticks) = &table.columns["tick"] {
            assert_eq!(ticks, &vec![1, 2, 3]);
        } else {
            panic!("Expected U64 column");
        }
    }

    #[test]
    fn test_column_padding() {
        let mut table = DynamicTable::default();

        // Simulate row 1: has tick and price
        // (visitor would create columns pre-padded, but row_count=0 so no padding)
        table
            .columns
            .insert("tick".to_string(), TypedColumn::U64(vec![1]));
        table
            .columns
            .insert("price".to_string(), TypedColumn::F64(vec![10.0]));
        table.row_count = 1;
        table.pad_columns_to_row_count(); // Ensure all columns have 1 element

        // Simulate row 2: has tick and qty (new column), no price
        // Pad existing columns first
        table.pad_columns_to_row_count();
        // Add tick value for row 2
        if let TypedColumn::U64(v) = table.columns.get_mut("tick").unwrap() {
            v.push(2);
        }
        // New column "qty" - created with pre-padding for row 1, then value for row 2
        // This simulates what the visitor does: vec![0.0; row_count] then push(value)
        table
            .columns
            .insert("qty".to_string(), TypedColumn::F64(vec![0.0, 5.0]));
        table.row_count = 2;
        // Pad all columns to align (price didn't get a value for row 2)
        table.pad_columns_to_row_count();

        // All columns should have 2 elements
        assert_eq!(table.columns["tick"].len(), 2, "tick should have 2 values");
        assert_eq!(
            table.columns["price"].len(),
            2,
            "price should have 2 values (one padded)"
        );
        assert_eq!(table.columns["qty"].len(), 2, "qty should have 2 values");

        // Price row 2 should have been padded with 0.0
        if let TypedColumn::F64(prices) = &table.columns["price"] {
            assert_eq!(prices[1], 0.0, "price[1] should be padded with 0.0");
        }

        // Qty: row 1 was pre-padded with 0.0, row 2 has the actual value
        if let TypedColumn::F64(qtys) = &table.columns["qty"] {
            assert_eq!(qtys[0], 0.0, "qty[0] should be pre-padded with 0.0");
            assert_eq!(qtys[1], 5.0, "qty[1] should be the actual value");
        }
    }

    #[test]
    fn test_tracing_integration() {
        use tracing::subscriber::with_default;

        clear();

        // Use scoped subscriber to avoid global state issues between tests
        with_default(DataFrameSubscriber, || {
            // Emit some events
            tracing::info!(target: "test_events", tick = 1u64, value = 10.5f64, name = "first");
            tracing::info!(target: "test_events", tick = 2u64, value = 20.5f64, name = "second");
            tracing::info!(target: "test_events", tick = 3u64, value = 30.5f64);
        });

        let recorder = drain();

        // Check that events were recorded
        assert!(
            recorder.tables.contains_key("test_events"),
            "test_events table should exist"
        );

        let table = &recorder.tables["test_events"];
        assert_eq!(table.row_count, 3, "should have 3 rows");

        // Check tick column
        if let TypedColumn::U64(ticks) = &table.columns["tick"] {
            assert_eq!(ticks, &vec![1, 2, 3], "tick values should match");
        } else {
            panic!("tick should be U64 column");
        }

        // Check value column
        if let TypedColumn::F64(values) = &table.columns["value"] {
            assert_eq!(values, &vec![10.5, 20.5, 30.5], "value values should match");
        } else {
            panic!("value should be F64 column");
        }

        // Check name column - third row should be padded with empty string
        if let TypedColumn::Str(names) = &table.columns["name"] {
            assert_eq!(names.len(), 3, "name should have 3 values");
            assert_eq!(names[0], "first");
            assert_eq!(names[1], "second");
            assert_eq!(names[2], "", "name[2] should be padded empty string");
        } else {
            panic!("name should be Str column");
        }
    }
}
