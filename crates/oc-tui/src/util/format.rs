//! Formatting helpers.
//! From reference/packages/tui/src/util/format.ts

/// Format a duration in seconds (used for retry countdowns).
/// From reference/packages/tui/src/util/format.ts (`formatDuration`)
pub fn format_duration(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        let mins = secs / 60;
        let remaining = secs % 60;
        return if remaining > 0 {
            format!("{mins}m {remaining}s")
        } else {
            format!("{mins}m")
        };
    }
    if secs < 86_400 {
        let hours = secs / 3600;
        let remaining = (secs % 3600) / 60;
        return if remaining > 0 {
            format!("{hours}h {remaining}m")
        } else {
            format!("{hours}h")
        };
    }
    if secs < 604_800 {
        let days = secs / 86_400;
        return if days == 1 {
            "~1 day".to_string()
        } else {
            format!("~{days} days")
        };
    }
    let weeks = secs / 604_800;
    if weeks == 1 {
        "~1 week".to_string()
    } else {
        format!("~{weeks} weeks")
    }
}

/// Collapse long tool output to a preview.
/// From reference/packages/tui/src/util/collapse-tool-output.ts
pub struct CollapsedOutput {
    pub output: String,
    pub overflow: bool,
}

/// `maxLines` is the number of lines kept; `maxChars` caps the preview length.
/// From reference/packages/tui/src/util/collapse-tool-output.ts
pub fn collapse_tool_output(output: &str, max_lines: usize, max_chars: usize) -> CollapsedOutput {
    let lines: Vec<&str> = output.split('\n').collect();
    if lines.len() <= max_lines && output.chars().count() <= max_chars {
        return CollapsedOutput {
            output: output.to_string(),
            overflow: false,
        };
    }

    let preview: String = lines[..max_lines.min(lines.len())].join("\n");
    if preview.chars().count() > max_chars {
        let truncated: String = preview.chars().take(max_chars.saturating_sub(1)).collect();
        return CollapsedOutput {
            output: format!("{truncated}…"),
            overflow: true,
        };
    }

    let mut out = preview;
    out.push_str("…");
    CollapsedOutput {
        output: out,
        overflow: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_output_not_collapsed() {
        let r = collapse_tool_output("hello\nworld", 10, 1000);
        assert_eq!(r.output, "hello\nworld");
        assert!(!r.overflow);
    }

    #[test]
    fn long_lines_collapsed_to_char_limit() {
        let long = "x".repeat(100);
        let r = collapse_tool_output(&long, 3, 20);
        assert!(r.overflow);
        assert!(r.output.chars().count() <= 20);
        assert!(r.output.ends_with('…'));
    }

    #[test]
    fn too_many_lines_gets_ellipsis_line() {
        let input = "a\nb\nc\nd\ne";
        let r = collapse_tool_output(input, 3, 1000);
        assert!(r.overflow);
        assert_eq!(r.output, "a\nb\nc…");
    }

    #[test]
    fn format_duration_values() {
        assert_eq!(format_duration(0), "");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(125), "2m 5s");
        assert_eq!(format_duration(3900), "1h 5m");
        assert_eq!(format_duration(172800), "~2 days");
    }
}
