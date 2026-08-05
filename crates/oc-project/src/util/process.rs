/// Minimal port of `AppProcess.run` (`@opencode-ai/core/process`) used to spawn
/// git and shell children. Returns raw bytes so callers can apply the same
/// trimming/parsing the reference applies to `Result.stdout`.
///
/// TODO(integration): move to oc-core / oc-util once those crates expose a
/// process runner; this local copy exists because they are still stubs.
use std::collections::HashMap;
use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

#[derive(Debug, Default, Clone)]
pub struct SpawnOptions {
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub stdin: Option<String>,
    pub max_output_bytes: Option<usize>,
}

#[derive(Debug)]
pub struct SpawnResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
}

impl SpawnResult {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Runs `program` with `args`, capturing stdout/stderr. Output is capped at
/// `max_output_bytes` (drained past the cap so the child never blocks) and the
/// `truncated` flag mirrors the reference's `stdoutTruncated || stderrTruncated`.
pub async fn run(program: &str, args: &[&str], options: SpawnOptions) -> std::io::Result<SpawnResult> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = &options.cwd {
        command.current_dir(cwd);
    }
    if let Some(env) = &options.env {
        command.envs(env);
    }

    let mut child = command.spawn()?;

    if let Some(input) = options.stdin {
        let mut stdin = child.stdin.take().unwrap();
        let input = input.into_bytes();
        tokio::spawn(async move {
            let _ = stdin.write_all(&input).await;
        });
    } else {
        child.stdin.take();
    }

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let cap = options.max_output_bytes;
    let stdout_task = tokio::spawn(read_capped(stdout, cap));
    let stderr_task = tokio::spawn(read_capped(stderr, cap));

    let status = child.wait().await?;
    let (stdout, stdout_truncated) = stdout_task.await.unwrap_or((Vec::new(), false));
    let (stderr, stderr_truncated) = stderr_task.await.unwrap_or((Vec::new(), false));

    Ok(SpawnResult {
        exit_code: status.code().unwrap_or(1),
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
    })
}

async fn read_capped<R: AsyncRead + Unpin>(mut reader: R, cap: Option<usize>) -> (Vec<u8>, bool) {
    match cap {
        None => {
            let mut buf = Vec::new();
            let _ = reader.read_to_end(&mut buf).await;
            (buf, false)
        }
        Some(cap) if cap == 0 => {
            let mut buf = Vec::new();
            let _ = reader.read_to_end(&mut buf).await;
            (buf, false)
        }
        Some(cap) => {
            let mut buf = Vec::with_capacity(cap);
            let mut limited = (&mut reader).take((cap + 1) as u64);
            let _ = limited.read_to_end(&mut buf).await;
            let truncated = buf.len() > cap;
            if truncated {
                buf.truncate(cap);
                let mut sink = [0u8; 8192];
                loop {
                    match reader.read(&mut sink).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => continue,
                    }
                }
            }
            (buf, truncated)
        }
    }
}
