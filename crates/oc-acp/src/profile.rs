//! ACP profiling helpers.
//!
//! From reference/packages/opencode/src/acp/profile.ts. Writes
//! `[acp-profile] ...` lines to stderr when `OPENCODE_ACP_PROFILE=1`.

use std::sync::OnceLock;
use std::time::Instant;

/// A value that may be attached to a profile field.
#[derive(Debug, Clone)]
pub enum ProfileValue {
    Str(String),
    Num(i64),
    Bool(bool),
}

impl From<&str> for ProfileValue {
    fn from(value: &str) -> Self {
        ProfileValue::Str(value.to_string())
    }
}

impl From<String> for ProfileValue {
    fn from(value: String) -> Self {
        ProfileValue::Str(value)
    }
}

impl From<i64> for ProfileValue {
    fn from(value: i64) -> Self {
        ProfileValue::Num(value)
    }
}

impl From<u64> for ProfileValue {
    fn from(value: u64) -> Self {
        ProfileValue::Num(value as i64)
    }
}

impl From<bool> for ProfileValue {
    fn from(value: bool) -> Self {
        ProfileValue::Bool(value)
    }
}

fn enabled() -> &'static bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    ENABLED.get_or_init(|| std::env::var("OPENCODE_ACP_PROFILE").as_deref() == Ok("1"))
}

fn started() -> &'static Instant {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    STARTED.get_or_init(Instant::now)
}

/// `mark` from reference/packages/opencode/src/acp/profile.ts.
pub fn mark(name: &str, fields: &[(&str, ProfileValue)]) {
    if !enabled() {
        return;
    }
    write(name, started().elapsed(), fields);
}

/// `duration` from reference/packages/opencode/src/acp/profile.ts.
pub fn duration(name: &str, started_at: Instant, fields: &[(&str, ProfileValue)]) {
    if !enabled() {
        return;
    }
    write(name, started_at.elapsed(), fields);
}

/// `measure` from reference/packages/opencode/src/acp/profile.ts.
pub fn measure<T>(name: &str, fields: &[(&str, ProfileValue)], f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let result = f();
    write(name, start.elapsed(), fields);
    result
}

fn write(name: &str, duration: std::time::Duration, fields: &[(&str, ProfileValue)]) {
    let extra = if fields.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = fields
            .iter()
            .map(|(key, value)| {
                let rendered = match value {
                    ProfileValue::Str(value) => value.clone(),
                    ProfileValue::Num(value) => value.to_string(),
                    ProfileValue::Bool(value) => value.to_string(),
                };
                format!("{key}={rendered}")
            })
            .collect();
        format!(" {}", parts.join(" "))
    };
    eprintln!("[acp-profile] {name} {}ms{extra}", duration.as_millis());
}
