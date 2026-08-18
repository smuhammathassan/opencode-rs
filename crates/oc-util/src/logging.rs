//! Tracing/logging subsystem, mirroring
//! `reference/packages/core/src/observability/logging.ts`.
//!
//! - `--print-logs` (`OPENCODE_PRINT_LOGS=1`) mirrors `loggers()`: emits the
//!   same lines to stderr that are written to the log file. Off by default so
//!   normal CLI output is never spammed.
//! - The file layer always appends to `<data>/log/opencode.log`
//!   (`Global.Path.log`), like the reference's unconditional `fileLogger`.
//! - `--log-level` (`OPENCODE_LOG_LEVEL`: DEBUG/INFO/WARN/ERROR, default INFO)
//!   mirrors `minimumLogLevel()`.
//! - Line shape mirrors the reference `formatter()`:
//!   `timestamp=... level=... run=<run-id> message=...`, with values JSON-quoted
//!   when they contain whitespace / `=` / `"` / `\`.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::sync::Mutex;

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::field::Visit;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

use crate::global;

/// An 8-hex-character run correlation id, like the reference `runID`.
pub fn run_id() -> String {
    static RUN_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    RUN_ID
        .get_or_init(|| {
            let pid = std::process::id();
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let seed = (pid as u64).rotate_left(32) ^ (nanos as u64);
            format!("{:08x}", seed & 0xffff_ffff)
        })
        .clone()
}

/// `minimumLogLevel()`: `OPENCODE_LOG_LEVEL` (DEBUG/INFO/WARN/ERROR) with a
/// default of INFO; unknown values fall back to INFO.
pub fn minimum_log_level() -> Level {
    match std::env::var("OPENCODE_LOG_LEVEL") {
        Ok(value) => match value.to_uppercase().as_str() {
            "DEBUG" => Level::DEBUG,
            "INFO" => Level::INFO,
            "WARN" => Level::WARN,
            "ERROR" => Level::ERROR,
            _ => Level::INFO,
        },
        Err(_) => Level::INFO,
    }
}

fn is_print_logs() -> bool {
    std::env::var("OPENCODE_PRINT_LOGS").as_deref() == Ok("1")
}

/// JSON-quote a value unless it is a bare token (matches the reference
/// `format()`: `^[^\s="\\]+$`).
fn format_value(value: &str) -> String {
    let bare = !value.is_empty()
        && value
            .chars()
            .all(|c| !c.is_whitespace() && c != '=' && c != '"' && c != '\\');
    if bare {
        value.to_string()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"))
    }
}

fn timestamp() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.6fZ")
        .to_string()
}

fn level_name(level: &Level) -> &'static str {
    match *level {
        Level::DEBUG => "Debug",
        Level::INFO => "Info",
        Level::WARN => "Warn",
        Level::ERROR => "Error",
        Level::TRACE => "Trace",
    }
}

/// Collects event fields as `(name, value)` pairs, matching the reference's
/// flattened `[key, value]` tuples.
#[derive(Default)]
struct FieldCollector {
    entries: Vec<(String, String)>,
}

impl Visit for FieldCollector {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.entries
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.entries
            .push((field.name().to_string(), format!("{value:?}")));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.entries
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.entries
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.entries
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.entries
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        self.entries
            .push((field.name().to_string(), value.to_string()));
    }
}

/// `formatter()` equivalent: writes `timestamp=... level=... run=... <fields>`.
#[derive(Default, Clone)]
struct ReferenceFormatter;

impl<S, N> FormatEvent<S, N> for ReferenceFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let mut fields = FieldCollector::default();
        event.record(&mut fields);

        let mut parts = Vec::with_capacity(3 + fields.entries.len());
        parts.push(format!("timestamp={}", timestamp()));
        parts.push(format!("level={}", level_name(event.metadata().level())));
        parts.push(format!("run={}", run_id()));
        for (key, value) in &fields.entries {
            parts.push(format!("{key}={}", format_value(value)));
        }
        writer.write_str(&parts.join(" "))
    }
}

/// Path of the append log file: `<data>/log/opencode.log`.
pub fn log_file_path() -> std::path::PathBuf {
    global::path::log().join("opencode.log")
}

/// Opens the log file in append mode (reference `{ flag: "a" }`), creating the
/// directory if needed. Returns `None` when the file cannot be opened; logging
/// then degrades to the stderr layer only.
fn open_log_file() -> Option<Mutex<File>> {
    let path = log_file_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    Some(Mutex::new(file))
}

/// Initializes the process-wide tracing subscriber. Safe to call once; later
/// calls are no-ops. Never panics; a missing log file degrades silently.
pub fn init() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let level = minimum_log_level();
        let print = is_print_logs();

        match open_log_file() {
            Some(file) if print => {
                tracing_subscriber::registry()
                    .with(LevelFilter::from_level(level))
                    .with(file_layer(file))
                    .with(stderr_layer())
                    .init();
            }
            Some(file) => {
                tracing_subscriber::registry()
                    .with(LevelFilter::from_level(level))
                    .with(file_layer(file))
                    .init();
            }
            None if print => {
                tracing_subscriber::registry()
                    .with(LevelFilter::from_level(level))
                    .with(stderr_layer())
                    .init();
            }
            None => {
                // No writable log file and no stderr logging requested: drop
                // all events. Keep a subscriber installed so `tracing!` stays
                // cheap and consistent.
                tracing_subscriber::registry().init();
            }
        }
    });
}

type FileLayer<S> = tracing_subscriber::fmt::Layer<
    S,
    tracing_subscriber::fmt::format::DefaultFields,
    ReferenceFormatter,
    Mutex<File>,
>;

type StderrLayer<S> = tracing_subscriber::fmt::Layer<
    S,
    tracing_subscriber::fmt::format::DefaultFields,
    ReferenceFormatter,
    fn() -> io::Stderr,
>;

/// The file layer: append to `<data>/log/opencode.log` (reference
/// `fileLogger`), unconditionally active.
fn file_layer<S>(file: Mutex<File>) -> FileLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    tracing_subscriber::fmt::layer()
        .event_format(ReferenceFormatter)
        .with_writer(file)
        .with_ansi(false)
}

/// The stderr layer (reference `stderrLogger`), active only with
/// `OPENCODE_PRINT_LOGS=1` so normal CLI output is never spammed.
fn stderr_layer<S>() -> StderrLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    tracing_subscriber::fmt::layer()
        .event_format(ReferenceFormatter)
        .with_writer(io::stderr as fn() -> io::Stderr)
        .with_ansi(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("logging test environment lock is not poisoned")
    }

    #[test]
    fn run_id_is_eight_hex_chars() {
        let id = run_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn format_value_bare_tokens_are_not_quoted() {
        assert_eq!(format_value("hello"), "hello");
        assert_eq!(format_value("1.18.13"), "1.18.13");
    }

    #[test]
    fn format_value_whitespace_is_json_quoted() {
        assert_eq!(format_value("hello world"), "\"hello world\"");
        assert_eq!(format_value(""), "\"\"");
    }

    #[test]
    fn format_value_equals_dquote_backslash_are_json_quoted() {
        assert_eq!(format_value("a=b"), "\"a=b\"");
        assert_eq!(format_value("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(format_value("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn minimum_log_level_defaults_to_info() {
        let _lock = env_lock();
        std::env::remove_var("OPENCODE_LOG_LEVEL");
        assert_eq!(minimum_log_level(), Level::INFO);
    }

    #[test]
    fn minimum_log_level_parses_env() {
        let _lock = env_lock();
        std::env::set_var("OPENCODE_LOG_LEVEL", "DEBUG");
        assert_eq!(minimum_log_level(), Level::DEBUG);
        std::env::set_var("OPENCODE_LOG_LEVEL", "warn");
        assert_eq!(minimum_log_level(), Level::WARN);
        std::env::set_var("OPENCODE_LOG_LEVEL", "BOGUS");
        assert_eq!(minimum_log_level(), Level::INFO);
        std::env::remove_var("OPENCODE_LOG_LEVEL");
    }

    #[test]
    fn log_file_path_under_data_log() {
        // Another test in this binary mutates OPENCODE_TEST_HOME concurrently;
        // take the lock and snapshot the expected path once so the two sides
        // of the comparison cannot straddle an environment mutation (flaky on
        // Windows where scheduling differs).
        let _lock = env_lock();
        let expected = global::path::log().join("opencode.log");
        assert_eq!(log_file_path(), expected);
    }

    #[test]
    fn log_file_is_created_and_appendable() {
        let _lock = env_lock();
        let home = std::env::temp_dir().join(format!(
            "opencode-logging-test-{}-{}",
            std::process::id(),
            run_id()
        ));
        std::fs::create_dir_all(&home).expect("test home");
        std::env::set_var("OPENCODE_TEST_HOME", &home);
        let path = log_file_path();
        let file = open_log_file().expect("log file should be created");
        file.lock()
            .expect("log file lock")
            .write_all(b"test-log-line\n")
            .expect("log line should be written");
        let contents = std::fs::read_to_string(&path).expect("log file should be readable");
        assert!(contents.contains("test-log-line"));
        std::env::remove_var("OPENCODE_TEST_HOME");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn file_layer_writes_reference_formatted_events() {
        let _lock = env_lock();
        let path = std::env::temp_dir().join(format!(
            "opencode-logging-event-test-{}-{}.log",
            std::process::id(),
            run_id()
        ));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("test log file should open");
        let subscriber = tracing_subscriber::registry()
            .with(LevelFilter::INFO)
            .with(file_layer(Mutex::new(file)));

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(attempt = 2, "hello world");
        });

        let contents = std::fs::read_to_string(&path).expect("test log file should be readable");
        assert!(contents.contains("level=Info"));
        assert!(contents.contains("run="));
        assert!(contents.contains("message=\"hello world\""));
        assert!(contents.contains("attempt=2"));
        let _ = std::fs::remove_file(path);
    }
}
