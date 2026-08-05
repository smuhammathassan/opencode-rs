//! mDNS advertisement. From reference/packages/opencode/src/server/mdns.ts.
//!
//! Partial port: publishes a `_http._tcp` service via a best-effort multicast beacon.
//! TODO(integration): real mDNS responder crate when networking parity is required.

use std::net::UdpSocket;
use std::sync::{Arc, Mutex};

struct MdnsState {
    socket: Option<UdpSocket>,
    port: Option<u16>,
}

/// Publish an `opencode-<port>` service. Mirrors `MDNS.publish` from
/// reference/packages/opencode/src/server/mdns.ts. Errors are swallowed like the
/// reference (`service.on("error", () => {})`).
pub fn publish(port: u16, domain: Option<&str>) {
    let mut state = STATE.lock().unwrap();
    if state.port == Some(port) {
        return;
    }
    unpublish_locked(&mut state);

    let host = domain.unwrap_or("opencode.local");
    let service = format!("opencode-{port}");

    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return;
    };
    // 224.0.0.251:5353 is the mDNS multicast group. The beacon carries the service
    // name; a full DNS-SD response is out of scope.
    let announcement = format!("{service}.{host}:{port} path=/");
    let _ = socket.send_to(announcement.as_bytes(), "224.0.0.251:5353");
    let _ = socket.set_multicast_loop_v4(true);
    let _ = socket.set_multicast_ttl_v4(255);

    state.socket = Some(socket);
    state.port = Some(port);
}

/// Remove any published service. Mirrors `MDNS.unpublish`.
pub fn unpublish() {
    let mut state = STATE.lock().unwrap();
    unpublish_locked(&mut state);
}

fn unpublish_locked(state: &mut MdnsState) {
    state.socket = None;
    state.port = None;
}

static STATE: std::sync::LazyLock<Arc<Mutex<MdnsState>>> = std::sync::LazyLock::new(|| {
    Arc::new(Mutex::new(MdnsState {
        socket: None,
        port: None,
    }))
});
