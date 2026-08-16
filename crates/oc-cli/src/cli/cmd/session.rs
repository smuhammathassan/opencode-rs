//! `opencode session`
//! From reference/packages/opencode/src/cli/cmd/session.ts.

use crate::cli::args::{Cli, SessionArgs, SessionCommand};
use oc_database::database::path;
use oc_database::{Database, Value};

pub async fn run(_cli: &Cli, args: &SessionArgs) -> anyhow::Result<i32> {
    let database = Database::open(path())?;
    match &args.command {
        SessionCommand::List { max_count, format } => {
            let mut sessions = database.list_sessions(false)?;
            if let Some(max_count) = max_count {
                sessions.truncate(*max_count as usize);
            }
            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&sessions)?);
            } else {
                println!("{:<28} {:<32} {:<12} DIRECTORY", "ID", "TITLE", "UPDATED");
                for session in sessions {
                    println!(
                        "{:<28} {:<32} {:<12} {}",
                        session.id,
                        truncate(&session.title, 32),
                        session.time_updated,
                        session.directory
                    );
                }
            }
            Ok(0)
        }
        SessionCommand::Delete { session_id } => {
            let deleted = database.delete_by("session", "id", &Value::Text(session_id.clone()))?;
            if deleted == 0 {
                anyhow::bail!("session not found: {session_id}");
            }
            println!("deleted {session_id}");
            Ok(0)
        }
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncates_session_titles_without_splitting_utf8() {
        assert_eq!(truncate("short", 8), "short");
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
        assert_eq!(truncate("ééééé", 4), "ééé…");
    }
}
