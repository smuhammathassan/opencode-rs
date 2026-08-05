//! Opens a URL in the system browser for the MCP OAuth flow.
//!
//! From reference/packages/opencode/src/mcp/browser.ts (which wraps the `open`
//! npm package). Fails if the opener process exits non-zero within 500ms,
//! otherwise returns success and lets the browser take over.

use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

pub struct McpBrowser;

impl McpBrowser {
    pub async fn open(url: &str) -> crate::Result<()> {
        let (program, args): (&str, Vec<String>) = {
            #[cfg(target_os = "macos")]
            {
                ("open", vec![url.to_string()])
            }
            #[cfg(target_os = "windows")]
            {
                (
                    "cmd",
                    vec!["/c".into(), "start".into(), "".into(), url.to_string()],
                )
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                ("xdg-open", vec![url.to_string()])
            }
        };

        let mut child = Command::new(program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| crate::Error::message(format!("failed to open browser: {error}")))?;

        match tokio::time::timeout(Duration::from_millis(500), child.wait()).await {
            Ok(Ok(status)) if !status.success() => {
                let code = status.code().unwrap_or(-1);
                Err(crate::Error::message(format!(
                    "Browser open failed with exit code {code}"
                )))
            }
            _ => Ok(()),
        }
    }
}
