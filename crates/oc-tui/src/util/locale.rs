//! Locale formatting helpers.
//! From reference/packages/tui/src/util/locale.ts

/// Uppercase the first character of each word.
/// From reference/packages/tui/src/util/locale.ts (`titlecase`)
pub fn titlecase(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut at_word_start = true;
    for c in input.chars() {
        if c.is_alphanumeric() {
            if at_word_start {
                result.extend(c.to_uppercase());
            } else {
                result.push(c);
            }
            at_word_start = false;
        } else {
            result.push(c);
            at_word_start = true;
        }
    }
    result
}

/// Local short time, e.g. `12:30 PM`.
/// From reference/packages/tui/src/util/locale.ts (`time`)
pub fn time(ms: i64) -> String {
    let secs = (ms / 1000) as i64;
    let (h, m) = ((secs / 3600) % 24, (secs / 60) % 60);
    let period = if h >= 12 { "PM" } else { "AM" };
    let hour12 = h % 12;
    let hour12 = if hour12 == 0 { 12 } else { hour12 };
    format!("{hour12}:{m:02} {period}")
}

/// `todayTimeOrDateTime`: short time for today, else `time · date`.
/// From reference/packages/tui/src/util/locale.ts
pub fn today_time_or_datetime(ms: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    if same_day(ms, now) {
        time(ms)
    } else {
        datetime(ms)
    }
}

fn same_day(a: i64, b: i64) -> bool {
    let (y1, m1, d1) = ymd(a);
    let (y2, m2, d2) = ymd(b);
    y1 == y2 && m1 == m2 && d1 == d2
}

fn ymd(ms: i64) -> (i64, i64, i64) {
    use chrono::{DateTime, Datelike};
    let dt = DateTime::from_timestamp_millis(ms).unwrap_or_default();
    (dt.year() as i64, dt.month() as i64, dt.day() as i64)
}

/// `datetime`: `time · date`.
/// From reference/packages/tui/src/util/locale.ts
pub fn datetime(ms: i64) -> String {
    use chrono::{DateTime, Datelike};
    let dt = DateTime::from_timestamp_millis(ms).unwrap_or_default();
    format!("{} · {}/{}/{}", time(ms), dt.month(), dt.day(), dt.year())
}

/// Compact number: 1.0K / 1.0M.
/// From reference/packages/tui/src/util/locale.ts (`number`)
pub fn number(num: u64) -> String {
    if num >= 1_000_000 {
        format!("{:.1}M", num as f64 / 1_000_000.0)
    } else if num >= 1_000 {
        format!("{:.1}K", num as f64 / 1_000.0)
    } else {
        num.to_string()
    }
}

/// Human duration from a millisecond count.
/// From reference/packages/tui/src/util/locale.ts (`duration`)
pub fn duration(ms: i64) -> String {
    if ms < 1000 {
        return format!("{ms}ms");
    }
    if ms < 60_000 {
        return format!("{:.1}s", ms as f64 / 1000.0);
    }
    if ms < 3_600_000 {
        let minutes = ms / 60_000;
        let seconds = (ms % 60_000) / 1000;
        return format!("{minutes}m {seconds}s");
    }
    if ms < 86_400_000 {
        let hours = ms / 3_600_000;
        let minutes = (ms % 3_600_000) / 60_000;
        return format!("{hours}h {minutes}m");
    }
    let days = ms / 86_400_000;
    let hours = (ms % 86_400_000) / 3_600_000;
    format!("{days}d {hours}h")
}

/// Truncate with trailing ellipsis.
/// From reference/packages/tui/src/util/locale.ts (`truncate`)
pub fn truncate(s: &str, len: usize) -> String {
    if s.chars().count() <= len {
        return s.to_string();
    }
    let mut out: String = s.chars().take(len.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Truncate with leading ellipsis.
/// From reference/packages/tui/src/util/locale.ts (`truncateLeft`)
pub fn truncate_left(s: &str, len: usize) -> String {
    if s.chars().count() <= len {
        return s.to_string();
    }
    let mut out = String::new();
    out.push('…');
    let keep = len.saturating_sub(1);
    let start = s.chars().count() - keep;
    out.extend(s.chars().skip(start));
    out
}

/// Truncate keeping both ends.
/// From reference/packages/tui/src/util/locale.ts (`truncateMiddle`)
pub fn truncate_middle(s: &str, max_len: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_len {
        return s.to_string();
    }
    let ellipsis = '…';
    let keep_start = (max_len.saturating_sub(1)).div_ceil(2);
    let keep_end = (max_len.saturating_sub(1)) / 2;
    let mut out: String = chars.iter().take(keep_start).collect();
    out.push(ellipsis);
    if keep_end > 0 {
        out.extend(chars.iter().skip(chars.len() - keep_end));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titlecase_words() {
        assert_eq!(titlecase("build agent"), "Build Agent");
        assert_eq!(titlecase("hello"), "Hello");
        assert_eq!(titlecase(""), "");
    }

    #[test]
    fn truncation_forms() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate_left("hello world", 5), "…orld");
        assert_eq!(truncate_middle("hello world", 7), "hel…rld");
    }

    #[test]
    fn number_compact() {
        assert_eq!(number(999), "999");
        assert_eq!(number(1200), "1.2K");
        assert_eq!(number(2_000_000), "2.0M");
    }

    #[test]
    fn duration_forms() {
        assert_eq!(duration(500), "500ms");
        assert_eq!(duration(1500), "1.5s");
        assert_eq!(duration(90_000), "1m 30s");
        assert_eq!(duration(3_700_000), "1h 1m");
        assert_eq!(duration(90_000_000), "1d 1h");
    }
}
