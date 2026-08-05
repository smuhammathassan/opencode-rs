//! Stdio client transport.
//!
//! From `@modelcontextprotocol/sdk@1.29.0` `client/stdio.js`, configured by
//! `reference/packages/opencode/src/mcp/index.ts` (`connectLocal`): the server
//! is spawned as a child process and JSON-RPC messages flow as newline-delimited
//! JSON on stdin/stdout.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};
use tracing::debug;

use super::{MessageReceiver, Transport};
use crate::jsonrpc::Message;
use crate::util::BoxFuture;

pub struct StdioTransport {
    command: String,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    inner: Mutex<Option<Inner>>,
    pid: AtomicU32,
}

struct Inner {
    child: tokio::process::Child,
    stdin: Option<tokio::process::ChildStdin>,
}

impl StdioTransport {
    pub fn new(
        command: String,
        args: Vec<String>,
        cwd: PathBuf,
        env: Vec<(String, String)>,
    ) -> Self {
        StdioTransport {
            command,
            args,
            cwd,
            env,
            inner: Mutex::new(None),
            pid: AtomicU32::new(0),
        }
    }
}

impl Transport for StdioTransport {
    fn start(&self) -> BoxFuture<'_, crate::Result<MessageReceiver>> {
        Box::pin(async move {
            let mut guard = self.inner.lock().await;
            if guard.is_some() {
                return Err(crate::Error::message("stdio transport already started"));
            }

            let mut child = Command::new(&self.command)
                .args(&self.args)
                .current_dir(&self.cwd)
                .envs(self.env.iter().cloned())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| {
                    crate::Error::message(format!("failed to spawn {}: {error}", self.command))
                })?;

            let pid = child.id().unwrap_or(0);
            self.pid.store(pid, Ordering::SeqCst);

            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| crate::Error::message("no stdout"))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| crate::Error::message("no stderr"))?;
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| crate::Error::message("no stdin"))?;

            let (tx, rx) = mpsc::unbounded_channel();
            let reader_tx = tx.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let trimmed = line.trim_end();
                            if trimmed.is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<Message>(trimmed) {
                                Ok(message) => {
                                    if reader_tx.send(message).is_err() {
                                        break;
                                    }
                                }
                                Err(parse_error) => {
                                    debug!("dropping non-JSON-RPC stdout line: {parse_error}");
                                }
                            }
                        }
                    }
                }
                debug!("MCP server stdout closed");
            });

            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => debug!("MCP server stderr: {}", line.trim_end()),
                    }
                }
            });

            *guard = Some(Inner {
                child,
                stdin: Some(stdin),
            });
            debug!(pid, command = %self.command, "MCP server spawned");
            Ok(rx)
        })
    }

    fn send(&self, message: Message) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async move {
            let mut guard = self.inner.lock().await;
            let inner = guard
                .as_mut()
                .ok_or_else(|| crate::Error::message("transport not started"))?;
            let stdin = inner
                .stdin
                .as_mut()
                .ok_or_else(|| crate::Error::message("stdin closed"))?;
            let mut line = message.to_line();
            line.push('\n');
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await?;
            Ok(())
        })
    }

    fn close(&self) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async move {
            let mut guard = self.inner.lock().await;
            if let Some(mut inner) = guard.take() {
                inner.stdin.take();
                let _ = inner.child.kill().await;
                let _ = inner.child.wait().await;
            }
            self.pid.store(0, Ordering::SeqCst);
            Ok(())
        })
    }

    fn pid(&self) -> Option<u32> {
        let pid = self.pid.load(Ordering::SeqCst);
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
}
