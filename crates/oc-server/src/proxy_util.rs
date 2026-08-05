//! Proxy header utilities. From reference/packages/opencode/src/server/proxy-util.ts.

/// Hop-by-hop headers stripped when proxying. From reference/.../proxy-util.ts.
pub const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
];

/// Sanitize outgoing proxy headers. From reference/.../proxy-util.ts (`sanitize`).
pub fn sanitize_headers(headers: &mut axum::http::HeaderMap) {
    for key in HOP_BY_HOP {
        headers.remove(*key);
    }
    headers.remove("accept-encoding");
    headers.remove("x-opencode-directory");
    headers.remove("x-opencode-workspace");
}

/// Parse the `Sec-WebSocket-Protocol` header into a list of protocols.
pub fn websocket_protocols(value: Option<&str>) -> Vec<String> {
    value
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Convert an `http:`/`https:` URL to `ws:`/`wss:` for WebSocket proxying.
pub fn websocket_target_url(url: &str) -> String {
    let mut next = url.to_string();
    if next.starts_with("http:") {
        next = next.replacen("http:", "ws:", 1);
    } else if next.starts_with("https:") {
        next = next.replacen("https:", "wss:", 1);
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_protocol_header() {
        assert_eq!(
            websocket_protocols(Some("graphql-ws,  subprotocol ")),
            vec!["graphql-ws", "subprotocol"]
        );
        assert_eq!(websocket_protocols(None), Vec::<String>::new());
    }

    #[test]
    fn rewrites_ws_target() {
        assert_eq!(
            websocket_target_url("http://localhost:4096/ws"),
            "ws://localhost:4096/ws"
        );
        assert_eq!(
            websocket_target_url("https://example.com/ws"),
            "wss://example.com/ws"
        );
        assert_eq!(
            websocket_target_url("ws://localhost:4096"),
            "ws://localhost:4096"
        );
    }

    #[test]
    fn strips_hop_by_hop() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("host", "opencode.ai".parse().unwrap());
        headers.insert("accept-encoding", "gzip".parse().unwrap());
        headers.insert("x-opencode-directory", "/tmp".parse().unwrap());
        sanitize_headers(&mut headers);
        assert!(headers.is_empty());
    }
}
