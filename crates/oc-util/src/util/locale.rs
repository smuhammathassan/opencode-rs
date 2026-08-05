/// From reference/packages/tui/src/util/locale.ts
///
/// Locale-sensitive formatting. The reference relies on
/// `Intl.DateTimeFormat`; this port produces en-US style output and flags the
/// remaining locale dependency for integration.
use chrono::{Datelike, Local, TimeZone, Timelike};

pub fn titlecase(str: &str) -> String {
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(str.len());
    let mut word_boundary = true;
    for c in str.chars() {
        if word_boundary {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        word_boundary = !is_word_char(c);
    }
    out
}

fn to_fixed1(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    format!("{rounded:.1}")
}

pub fn time(input_ms: i64) -> String {
    let dt = Local
        .timestamp_millis_opt(input_ms)
        .single()
        .expect("valid millis");
    let hour = dt.hour();
    let hour12 = hour % 12;
    let hour12 = if hour12 == 0 { 12 } else { hour12 };
    let ampm = if hour < 12 { "AM" } else { "PM" };
    format!("{hour12}:{:02} {ampm}", dt.minute())
}

pub fn datetime(input_ms: i64) -> String {
    let dt = Local
        .timestamp_millis_opt(input_ms)
        .single()
        .expect("valid millis");
    format!(
        "{} · {}/{}/{}",
        time(input_ms),
        dt.month(),
        dt.day(),
        dt.year()
    )
}

pub fn today_time_or_datetime(input_ms: i64) -> String {
    let date = Local
        .timestamp_millis_opt(input_ms)
        .single()
        .expect("valid millis");
    let now = Local::now();
    let is_today =
        date.year() == now.year() && date.month() == now.month() && date.day() == now.day();
    if is_today {
        time(input_ms)
    } else {
        datetime(input_ms)
    }
}

pub fn number(num: f64) -> String {
    if num >= 1_000_000.0 {
        return format!("{}M", to_fixed1(num / 1_000_000.0));
    }
    if num >= 1000.0 {
        return format!("{}K", to_fixed1(num / 1000.0));
    }
    // JS Number#toString on integers has no fractional part
    if num.fract() == 0.0 {
        format!("{}", num as i64)
    } else {
        format!("{num}")
    }
}

pub fn duration(input_ms: f64) -> String {
    if input_ms < 1000.0 {
        return format!("{input_ms}ms");
    }
    if input_ms < 60_000.0 {
        return format!("{}s", to_fixed1(input_ms / 1000.0));
    }
    if input_ms < 3_600_000.0 {
        let minutes = (input_ms / 60_000.0).floor() as u64;
        let seconds = ((input_ms % 60_000.0) / 1000.0).floor() as u64;
        return format!("{minutes}m {seconds}s");
    }
    if input_ms < 86_400_000.0 {
        let hours = (input_ms / 3_600_000.0).floor() as u64;
        let minutes = ((input_ms % 3_600_000.0) / 60_000.0).floor() as u64;
        return format!("{hours}h {minutes}m");
    }
    let days = (input_ms / 86_400_000.0).floor() as u64;
    let hours = ((input_ms % 86_400_000.0) / 3_600_000.0).floor() as u64;
    format!("{days}d {hours}h")
}

fn slice_chars(str: &str, start: usize, count: usize) -> String {
    str.chars().skip(start).take(count).collect()
}

pub fn truncate(str: &str, len: usize) -> String {
    let char_len = str.chars().count();
    if char_len <= len {
        return str.to_string();
    }
    format!("{}…", slice_chars(str, 0, len - 1))
}

pub fn truncate_left(str: &str, len: usize) -> String {
    let char_len = str.chars().count();
    if char_len <= len {
        return str.to_string();
    }
    let start = char_len - (len - 1);
    format!("…{}", slice_chars(str, start, len - 1))
}

pub fn truncate_middle(str: &str, max_length: usize) -> String {
    let char_len = str.chars().count();
    if char_len <= max_length {
        return str.to_string();
    }
    let keep_start = (max_length - 1).div_ceil(2);
    let keep_end = (max_length - 1) / 2;
    let chars: Vec<char> = str.chars().collect();
    let head: String = chars[..keep_start].iter().collect();
    let tail: String = chars[char_len - keep_end..].iter().collect();
    format!("{head}…{tail}")
}

pub fn pluralize(count: u64, singular: &str, plural: &str) -> String {
    let template = if count == 1 { singular } else { plural };
    template.replace("{}", &count.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titlecase_words() {
        assert_eq!(titlecase("hello world"), "Hello World");
        assert_eq!(titlecase("foo bar-baz"), "Foo Bar-Baz");
    }

    #[test]
    fn time_is_short_format() {
        let dt = Local.with_ymd_and_hms(2026, 1, 1, 16, 5, 0).unwrap();
        assert_eq!(time(dt.timestamp_millis()), "4:05 PM");
    }

    #[test]
    fn datetime_combines_time_and_date() {
        let dt = Local.with_ymd_and_hms(2026, 8, 5, 9, 30, 0).unwrap();
        assert_eq!(datetime(dt.timestamp_millis()), "9:30 AM · 8/5/2026");
    }

    #[test]
    fn number_abbreviates() {
        assert_eq!(number(1234567.0), "1.2M");
        assert_eq!(number(1000000.0), "1.0M");
        assert_eq!(number(2500.0), "2.5K");
        assert_eq!(number(999.0), "999");
    }

    #[test]
    fn duration_units() {
        assert_eq!(duration(500.0), "500ms");
        assert_eq!(duration(1500.0), "1.5s");
        assert_eq!(duration(65_000.0), "1m 5s");
        assert_eq!(duration(3_700_000.0), "1h 1m");
        assert_eq!(duration(90_000_000.0), "1d 1h");
    }

    #[test]
    fn truncate_variants() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello w…");
        assert_eq!(truncate_left("hello world", 8), "…o world");
        assert_eq!(truncate_middle("hello world", 7), "hel…rld");
        assert_eq!(truncate_middle("hello world", 35), "hello world");
        assert_eq!(truncate_middle("abcdefghij", 7), "abc…hij");
    }

    #[test]
    fn pluralize_uses_template() {
        assert_eq!(pluralize(1, "{} item", "{} items"), "1 item");
        assert_eq!(pluralize(2, "{} item", "{} items"), "2 items");
    }
}
