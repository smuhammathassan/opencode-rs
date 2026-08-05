//! `opencode stats`
//! From reference/packages/opencode/src/cli/cmd/stats.ts.

use crate::cli::args::{Cli, StatsArgs};
use crate::cli::effect_cmd::not_wired;

/// Session usage stats mirroring the reference's `SessionStats`.
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub total_sessions: u64,
    pub total_messages: u64,
    pub total_cost: f64,
    pub total_tokens: Tokens,
    pub tool_usage: Vec<(String, u64)>,
    pub model_usage: Vec<(String, ModelUsage)>,
    pub days: u64,
    pub cost_per_day: f64,
    pub tokens_per_session: f64,
    pub median_tokens_per_session: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ModelUsage {
    pub messages: u64,
    pub tokens: Tokens,
    pub cost: f64,
}

fn format_number(num: f64) -> String {
    if num >= 1_000_000.0 {
        format!("{:.1}M", num / 1_000_000.0)
    } else if num >= 1000.0 {
        format!("{:.1}K", num / 1000.0)
    } else {
        format!("{num}")
    }
}

fn render_row(width: usize, label: &str, value: &str) -> String {
    let available = width.saturating_sub(1);
    let padding = available.saturating_sub(label.chars().count() + value.chars().count());
    format!("│{label}{} {value} │", " ".repeat(padding))
}

/// Render the stats table. Mirrors `displayStats` in stats.ts.
pub fn display_stats(
    stats: &SessionStats,
    tool_limit: Option<u64>,
    model_limit: Option<ModelLimit>,
) -> String {
    let width = 56;
    let mut out = String::new();

    out.push_str("┌────────────────────────────────────────────────────────┐\n");
    out.push_str("│                       OVERVIEW                         │\n");
    out.push_str("├────────────────────────────────────────────────────────┤\n");
    out.push_str(&render_row(
        width,
        "Sessions",
        &thousands(stats.total_sessions),
    ));
    out.push('\n');
    out.push_str(&render_row(
        width,
        "Messages",
        &thousands(stats.total_messages),
    ));
    out.push('\n');
    out.push_str(&render_row(width, "Days", &stats.days.to_string()));
    out.push('\n');
    out.push_str("└────────────────────────────────────────────────────────┘\n\n");

    out.push_str("┌────────────────────────────────────────────────────────┐\n");
    out.push_str("│                    COST & TOKENS                       │\n");
    out.push_str("├────────────────────────────────────────────────────────┤\n");
    let cost = if stats.total_cost.is_nan() {
        0.0
    } else {
        stats.total_cost
    };
    let cost_per_day = if stats.cost_per_day.is_nan() {
        0.0
    } else {
        stats.cost_per_day
    };
    let tokens_per_session = if stats.tokens_per_session.is_nan() {
        0.0
    } else {
        stats.tokens_per_session
    };
    out.push_str(&render_row(width, "Total Cost", &format!("${cost:.2}")));
    out.push('\n');
    out.push_str(&render_row(
        width,
        "Avg Cost/Day",
        &format!("${cost_per_day:.2}"),
    ));
    out.push('\n');
    out.push_str(&render_row(
        width,
        "Avg Tokens/Session",
        &format_number(tokens_per_session.round()),
    ));
    out.push('\n');
    let median = if stats.median_tokens_per_session.is_nan() {
        0.0
    } else {
        stats.median_tokens_per_session
    };
    out.push_str(&render_row(
        width,
        "Median Tokens/Session",
        &format_number(median.round()),
    ));
    out.push('\n');
    out.push_str(&render_row(
        width,
        "Input",
        &format_number(stats.total_tokens.input as f64),
    ));
    out.push('\n');
    out.push_str(&render_row(
        width,
        "Output",
        &format_number(stats.total_tokens.output as f64),
    ));
    out.push('\n');
    out.push_str(&render_row(
        width,
        "Cache Read",
        &format_number(stats.total_tokens.cache_read as f64),
    ));
    out.push('\n');
    out.push_str(&render_row(
        width,
        "Cache Write",
        &format_number(stats.total_tokens.cache_write as f64),
    ));
    out.push('\n');
    out.push_str("└────────────────────────────────────────────────────────┘\n");

    if let Some(model_limit) = model_limit {
        if !stats.model_usage.is_empty() {
            let sorted: Vec<&(String, ModelUsage)> = {
                let mut items: Vec<&(String, ModelUsage)> = stats.model_usage.iter().collect();
                items.sort_by(|a, b| b.1.messages.cmp(&a.1.messages));
                items
            };
            let shown: Vec<&&(String, ModelUsage)> = match model_limit {
                ModelLimit::All => sorted.iter().collect(),
                ModelLimit::Top(n) => sorted.iter().take(n as usize).collect(),
            };
            out.push_str("\n┌────────────────────────────────────────────────────────┐\n");
            out.push_str("│                      MODEL USAGE                       │\n");
            out.push_str("├────────────────────────────────────────────────────────┤\n");
            for (model, usage) in shown {
                out.push_str(&format!("│ {} │\n", pad_to(model, 54)));
                out.push_str(&render_row(width, "  Messages", &thousands(usage.messages)));
                out.push('\n');
                out.push_str(&render_row(
                    width,
                    "  Input Tokens",
                    &format_number(usage.tokens.input as f64),
                ));
                out.push('\n');
                out.push_str(&render_row(
                    width,
                    "  Output Tokens",
                    &format_number(usage.tokens.output as f64),
                ));
                out.push('\n');
                out.push_str(&render_row(
                    width,
                    "  Cache Read",
                    &format_number(usage.tokens.cache_read as f64),
                ));
                out.push('\n');
                out.push_str(&render_row(
                    width,
                    "  Cache Write",
                    &format_number(usage.tokens.cache_write as f64),
                ));
                out.push('\n');
                out.push_str(&render_row(width, "  Cost", &format!("${:.4}", usage.cost)));
                out.push('\n');
                out.push_str("├────────────────────────────────────────────────────────┤\n");
            }
            // Move up one line and replace the last separator with a bottom border.
            let text = out
                .strip_suffix("├────────────────────────────────────────────────────────┤\n")
                .unwrap_or(&out);
            let mut next = text.to_string();
            next.push_str("└────────────────────────────────────────────────────────┘\n");
            out = next;
        }
    }

    if !stats.tool_usage.is_empty() {
        let mut sorted = stats.tool_usage.clone();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let shown: Vec<&(String, u64)> = match tool_limit {
            Some(limit) => sorted.iter().take(limit as usize).collect(),
            None => sorted.iter().collect(),
        };
        let max_count = shown
            .iter()
            .map(|(_, count)| *count)
            .max()
            .unwrap_or(1)
            .max(1);
        let total_tool_usage: u64 = stats.tool_usage.iter().map(|(_, count)| count).sum();
        out.push_str("\n┌────────────────────────────────────────────────────────┐\n");
        out.push_str("│                      TOOL USAGE                        │\n");
        out.push_str("├────────────────────────────────────────────────────────┤\n");
        for (tool, count) in shown {
            let bar_length = ((*count as f64 / max_count as f64) * 20.0).floor().max(1.0) as usize;
            let bar = "█".repeat(bar_length);
            let percentage = (*count as f64 / total_tool_usage as f64) * 100.0;
            let truncated: String = tool.chars().take(16).collect();
            let tool_name = if tool.chars().count() > 18 {
                format!("{}..", &tool.chars().take(16).collect::<String>())
            } else {
                truncated
            };
            let content = format!(" {tool_name} {bar} {} ({:.1}%)", count, percentage);
            let padding = width.saturating_sub(content.chars().count() + 1);
            out.push_str(&format!("│{content}{} │\n", " ".repeat(padding)));
        }
        out.push_str("└────────────────────────────────────────────────────────┘\n");
    }
    out.push('\n');
    out
}

fn thousands(value: u64) -> String {
    let s = value.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLimit {
    All,
    Top(u64),
}

pub async fn run(_cli: &Cli, args: &StatsArgs) -> anyhow::Result<i32> {
    let _ = args;
    // TODO(integration): aggregate stats from `oc_database` + `oc_session`
    // (aggregateSessionStats in stats.ts), then `display_stats`.
    Err(not_wired("stats aggregation is not yet wired in this build (TODO(integration): oc-database/oc-session)"))
}

fn pad_to(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width {
        text.chars().take(width).collect()
    } else {
        format!("{text}{}", " ".repeat(width - len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_overview_section() {
        let stats = SessionStats {
            total_sessions: 12,
            total_messages: 340,
            ..Default::default()
        };
        let output = display_stats(&stats, None, None);
        assert!(output.contains("Sessions"));
        assert!(output.contains("12"));
        assert!(output.contains("340"));
        assert!(output.contains("OVERVIEW"));
    }

    #[test]
    fn formats_thousands() {
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(999), "999");
    }
}
