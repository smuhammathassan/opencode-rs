//! Local loopback HTTP server that completes the MCP OAuth authorization code
//! exchange.
//!
//! From reference/packages/opencode/src/mcp/oauth-callback.ts. Listens on
//! `127.0.0.1:19876/mcp/oauth/callback` (or a custom `redirectUri`), resolves
//! pending authorizations keyed by `state`, and serves branded status pages.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};

use crate::oauth_provider::{parse_redirect_uri, OAUTH_CALLBACK_PATH, OAUTH_CALLBACK_PORT};
use crate::Result;

const CALLBACK_TIMEOUT_MS: u64 = 5 * 60 * 1000;
const HOST: &str = "127.0.0.1";

type CallbackOutcome = std::result::Result<String, String>;

struct PendingAuth {
    tx: oneshot::Sender<CallbackOutcome>,
    timeout: tokio::task::JoinHandle<()>,
}

struct CallbackState {
    port: u16,
    path: String,
    running: bool,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
    pending: HashMap<String, PendingAuth>,
    mcp_name_to_state: HashMap<String, String>,
}

fn state() -> &'static Mutex<CallbackState> {
    static STATE: OnceLock<Mutex<CallbackState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(CallbackState {
            port: OAUTH_CALLBACK_PORT,
            path: OAUTH_CALLBACK_PATH.to_string(),
            running: false,
            shutdown: None,
            task: None,
            pending: HashMap::new(),
            mcp_name_to_state: HashMap::new(),
        })
    })
}

fn cleanup_state_index(state: &mut CallbackState, oauth_state: &str) {
    let mut found = None;
    for (name, state_value) in state.mcp_name_to_state.iter() {
        if state_value == oauth_state {
            found = Some(name.clone());
            break;
        }
    }
    if let Some(name) = found {
        state.mcp_name_to_state.remove(&name);
    }
}

/// Start the callback server if not already running on the given redirect URI's
/// port/path. Returns Ok(()) if the port is already served (another process).
/// From reference `McpOAuthCallback.ensureRunning`.
pub async fn ensure_running(redirect_uri: Option<&str>) -> Result<()> {
    let (port, path) = parse_redirect_uri(redirect_uri);
    let mut guard = state().lock().await;

    if guard.running && (guard.port != port || guard.path != path) {
        stop_inner(&mut guard).await;
    }
    if guard.running {
        return Ok(());
    }
    if is_port_in_use(port).await {
        return Ok(());
    }

    let listener = match TcpListener::bind((HOST, port)).await {
        Ok(listener) => listener,
        Err(error) => {
            if is_port_in_use(port).await {
                return Ok(());
            }
            return Err(crate::Error::message(format!(
                "failed to start OAuth callback server: {error}"
            )));
        }
    };

    guard.port = port;
    guard.path = path;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    guard.shutdown = Some(shutdown_tx);
    guard.running = true;
    guard.task_handle(listener, shutdown_rx);
    Ok(())
}

impl CallbackState {
    fn task_handle(&mut self, listener: TcpListener, shutdown: oneshot::Receiver<()>) {
        let task = tokio::spawn(async move {
            let mut shutdown = shutdown;
            loop {
                tokio::select! {
                    _ = &mut shutdown => break,
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((socket, _)) => {
                                tokio::spawn(async move {
                                    let _ = handle_connection(socket).await;
                                });
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        });
        self.task = Some(task);
    }
}

struct Request {
    path_and_query: String,
}

async fn handle_connection(mut socket: TcpStream) -> Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") || buffer.len() > 16 * 1024 {
            break;
        }
    }
    let request = std::str::from_utf8(&buffer).unwrap_or("");
    let path_and_query = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();

    let (status, body) = {
        let mut guard = state().lock().await;
        let mut status = 200u16;
        let mut body = String::new();
        handle_request(
            &mut guard,
            &Request { path_and_query },
            &mut status,
            &mut body,
        );
        (status, body)
    };

    let response = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        if status == 200 { "OK" } else { "Bad Request" },
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.flush().await?;
    Ok(())
}

/// Mirror of `handleRequest` in oauth-callback.ts. Pure for testability.
fn handle_request(
    state: &mut CallbackState,
    request: &Request,
    status: &mut u16,
    body: &mut String,
) {
    let (path, query) = match request.path_and_query.split_once('?') {
        Some((path, query)) => (path, query),
        None => (request.path_and_query.as_str(), ""),
    };

    if path != state.path {
        *status = 404;
        *body = "Not found".to_string();
        return;
    }

    let params: HashMap<String, String> = query
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((url_decode(key), url_decode(value)))
        })
        .collect();

    let code = params.get("code").cloned();
    let state_param = params.get("state").cloned();
    let error = params.get("error").cloned();
    let error_description = params.get("error_description").cloned();

    if state_param.is_none() {
        *status = 400;
        *body = error_page("Missing required state parameter - potential CSRF attack");
        return;
    }
    let state_param = state_param.unwrap();

    if let Some(error) = error {
        let error_msg = error_description.unwrap_or(error);
        if let Some(pending) = state.pending.remove(&state_param) {
            pending.timeout.abort();
            cleanup_state_index(state, &state_param);
            let _ = pending.tx.send(Err(error_msg.clone()));
        }
        *status = 200;
        *body = error_page(&error_msg);
        stop_if_idle(state);
        return;
    }

    let Some(code) = code else {
        *status = 400;
        *body = error_page("No authorization code provided");
        return;
    };

    if !state.pending.contains_key(&state_param) {
        *status = 400;
        *body = error_page("Invalid or expired state parameter - potential CSRF attack");
        return;
    }

    let pending = state.pending.remove(&state_param).unwrap();
    pending.timeout.abort();
    cleanup_state_index(state, &state_param);
    let _ = pending.tx.send(Ok(code));

    *status = 200;
    *body = success_page();
    stop_if_idle(state);
}

fn stop_if_idle(state: &mut CallbackState) {
    if !state.pending.is_empty() || !state.running {
        return;
    }
    if let Some(shutdown) = state.shutdown.take() {
        let _ = shutdown.send(());
    }
    state.running = false;
    state.task = None;
}

/// Register a pending authorization keyed by `oauthState`; returns the receiver
/// that resolves with the authorization code. From reference
/// `McpOAuthCallback.waitForCallback`.
pub async fn wait_for_callback(
    oauth_state: &str,
    mcp_name: Option<&str>,
) -> oneshot::Receiver<CallbackOutcome> {
    let mut guard = state().lock().await;
    if let Some(mcp_name) = mcp_name {
        guard
            .mcp_name_to_state
            .insert(mcp_name.to_string(), oauth_state.to_string());
    }
    let (tx, rx) = oneshot::channel();
    let timeout_state = state();
    let pending_state = oauth_state.to_string();
    let timeout = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(CALLBACK_TIMEOUT_MS)).await;
        let mut guard = timeout_state.lock().await;
        if let Some(pending) = guard.pending.remove(&pending_state) {
            cleanup_state_index(&mut guard, &pending_state);
            let _ = pending.tx.send(Err(
                "OAuth callback timeout - authorization took too long".into()
            ));
            stop_if_idle(&mut guard);
        }
    });
    guard
        .pending
        .insert(oauth_state.to_string(), PendingAuth { tx, timeout });
    rx
}

/// Reject the pending authorization for `mcpName`. From reference
/// `McpOAuthCallback.cancelPending`.
pub async fn cancel_pending(mcp_name: &str) {
    let mut guard = state().lock().await;
    let oauth_state = guard.mcp_name_to_state.get(mcp_name).cloned();
    let key = oauth_state.unwrap_or_else(|| mcp_name.to_string());
    if let Some(pending) = guard.pending.remove(&key) {
        pending.timeout.abort();
        guard.mcp_name_to_state.remove(mcp_name);
        let _ = pending.tx.send(Err("Authorization cancelled".into()));
        stop_if_idle(&mut guard);
    }
}

/// True if a TCP connection can be established on `port`. From reference
/// `McpOAuthCallback.isPortInUse`.
pub async fn is_port_in_use(port: u16) -> bool {
    match tokio::time::timeout(Duration::from_millis(500), TcpStream::connect((HOST, port))).await {
        Ok(Ok(_stream)) => true,
        _ => false,
    }
}

/// Stop the server and reject all pending authorizations.
pub async fn stop() {
    let mut guard = state().lock().await;
    stop_inner(&mut guard).await;
}

async fn stop_inner(state: &mut CallbackState) {
    if let Some(shutdown) = state.shutdown.take() {
        let _ = shutdown.send(());
    }
    state.running = false;
    state.task = None;
    let pending = std::mem::take(&mut state.pending);
    for (_key, pending) in pending {
        pending.timeout.abort();
        let _ = pending.tx.send(Err("OAuth callback server stopped".into()));
    }
    state.mcp_name_to_state.clear();
}

pub fn is_running() -> bool {
    let guard = match state().try_lock() {
        Ok(guard) => guard,
        Err(_) => return false,
    };
    guard.running
}

fn url_decode(value: &str) -> String {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Compact version of `OauthCallbackPage.success` from
/// `reference/packages/core/src/oauth/page.ts`.
/// TODO(integration): full visual parity with the reference page.
fn success_page() -> String {
    let body = format!(
        r#"<main class="card" id="oc-card" data-status="success" role="status" aria-live="polite">
      <h1 class="headline" id="oc-headline">Authorization successful</h1>
      <p class="message" id="oc-message">OpenCode is now connected to MCP.</p>
      <p class="footnote" id="oc-footnote">You can close this window.</p>
    </main>"#
    );
    document("Authorization successful", &body, true)
}

fn error_page(detail: &str) -> String {
    let escaped = escape_html(detail);
    let body = format!(
        r#"<main class="card" id="oc-card" data-status="error" role="status" aria-live="polite">
      <h1 class="headline" id="oc-headline">Authorization failed</h1>
      <p class="message" id="oc-message">OpenCode couldn't finish connecting to MCP.</p>
      <pre class="detail" id="oc-detail">{escaped}</pre>
      <p class="footnote" id="oc-footnote">Close this window and try again from OpenCode.</p>
    </main>"#
    );
    document("Authorization failed", &body, false)
}

fn document(title: &str, body: &str, auto_close: bool) -> String {
    let script = if auto_close {
        r#"<script>setTimeout(function(){try{window.close()}catch(e){}},2500)</script>"#
    } else {
        ""
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n    <meta name=\"robots\" content=\"noindex\" />\n    <title>{title} \u{00b7} OpenCode</title>\n    <style>body{{font-family:system-ui,sans-serif;display:grid;place-items:center;height:100vh;margin:0;background:#f8f8f8;color:#6f6f6f}}.card{{width:min(100%,28rem);padding:2.25rem 2rem 1.75rem;background:#fcfcfc;border:1px solid #e5e5e5;border-radius:14px;text-align:center}}.headline{{font-size:1.1875rem;font-weight:500;color:#171717;margin:0}}.message{{margin:0.5rem 0 0}}.detail{{text-align:left;padding:0.75rem;background:#fff8f6;border:1px solid #fdc3b7;border-radius:8px;white-space:pre-wrap;word-break:break-word;overflow:auto;max-height:9.5rem}}.footnote{{margin:1.5rem 0 0;font-size:0.8125rem}}</style>\n  </head>\n  <body>\n    {body}{script}\n  </body>\n</html>"
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The callback server state is process-global (like the reference's module
    /// singletons), so these tests must run serially.
    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn params(qs: &str) -> Request {
        Request {
            path_and_query: format!("/mcp/oauth/callback?{qs}"),
        }
    }

    async fn lock() -> tokio::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().await
    }

    #[tokio::test]
    async fn resolves_pending_on_callback() {
        let _guard = lock().await;
        let mut guard = state().lock().await;
        guard.pending.clear();
        drop(guard);
        let rx = wait_for_callback("state-1", Some("server-a")).await;
        let mut status = 0;
        let mut body = String::new();
        {
            let mut guard = state().lock().await;
            handle_request(
                &mut guard,
                &params("code=abc&state=state-1"),
                &mut status,
                &mut body,
            );
        }
        assert_eq!(status, 200);
        assert!(body.contains("Authorization successful"));
        assert_eq!(rx.await.unwrap().unwrap(), "abc");
        stop().await;
    }

    #[tokio::test]
    async fn rejects_missing_state() {
        let _guard = lock().await;
        let mut status = 0;
        let mut body = String::new();
        {
            let mut guard = state().lock().await;
            handle_request(&mut guard, &params("code=abc"), &mut status, &mut body);
        }
        assert_eq!(status, 400);
        assert!(body.contains("Missing required state parameter"));
    }

    #[tokio::test]
    async fn rejects_unknown_state() {
        let _guard = lock().await;
        let mut status = 0;
        let mut body = String::new();
        {
            let mut guard = state().lock().await;
            handle_request(
                &mut guard,
                &params("code=abc&state=nope"),
                &mut status,
                &mut body,
            );
        }
        assert_eq!(status, 400);
        assert!(body.contains("Invalid or expired state parameter"));
    }

    #[tokio::test]
    async fn reject_on_error_param() {
        let _guard = lock().await;
        let rx = wait_for_callback("state-2", None).await;
        let mut status = 0;
        let mut body = String::new();
        {
            let mut guard = state().lock().await;
            handle_request(
                &mut guard,
                &params("state=state-2&error=access_denied&error_description=denied"),
                &mut status,
                &mut body,
            );
        }
        assert_eq!(status, 200);
        assert!(body.contains("Authorization failed"));
        assert_eq!(rx.await.unwrap().unwrap_err(), "denied");
        stop().await;
    }

    #[tokio::test]
    async fn cancel_pending_rejects() {
        let _guard = lock().await;
        let rx = wait_for_callback("state-3", Some("server-b")).await;
        cancel_pending("server-b").await;
        assert_eq!(rx.await.unwrap().unwrap_err(), "Authorization cancelled");
        stop().await;
    }

    #[test]
    fn url_decode_handles_encoding() {
        assert_eq!(url_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(url_decode("a+b"), "a b");
    }
}
