//! Tests for MCP client backpressure (bounded request concurrency), request
//! multiplexing over a single transport (connection pooling), and the
//! list_changed lifecycle notifications for resources and prompts. All tests
//! use the in-memory mock transport so they run headless.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::{mpsc, Mutex};

use oc_mcp::client::Client;
use oc_mcp::jsonrpc::{Message, RequestId};
use oc_mcp::transport::{MessageReceiver, Transport};
use oc_mcp::types::{ClientCapabilities, Implementation};
use oc_mcp::util::BoxFuture;

struct MockTransport {
    outbound: Arc<Mutex<Vec<Message>>>,
    incoming: Mutex<Option<MessageReceiver>>,
}

impl MockTransport {
    fn new() -> (
        Arc<Self>,
        mpsc::UnboundedSender<Message>,
        Arc<Mutex<Vec<Message>>>,
    ) {
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        let outbound = Arc::new(Mutex::new(Vec::new()));
        let transport = Arc::new(Self {
            outbound: Arc::clone(&outbound),
            incoming: Mutex::new(Some(incoming_rx)),
        });
        (transport, incoming_tx, outbound)
    }
}

impl Transport for MockTransport {
    fn start(&self) -> BoxFuture<'_, oc_mcp::Result<MessageReceiver>> {
        Box::pin(async {
            self.incoming
                .lock()
                .await
                .take()
                .ok_or_else(|| oc_mcp::Error::message("mock transport already started"))
        })
    }

    fn send(&self, message: Message) -> BoxFuture<'_, oc_mcp::Result<()>> {
        Box::pin(async move {
            self.outbound.lock().await.push(message);
            Ok(())
        })
    }

    fn close(&self) -> BoxFuture<'_, oc_mcp::Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

fn client_info() -> Implementation {
    Implementation {
        name: "opencode".into(),
        version: "0.1.0".into(),
    }
}

fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        roots: Some(json!({})),
        sampling: None,
        experimental: None,
    }
}

async fn wait_for_message(
    outbound: &Arc<Mutex<Vec<Message>>>,
    predicate: impl Fn(&Message) -> bool,
) -> Message {
    for _ in 0..200 {
        if let Some(message) = outbound
            .lock()
            .await
            .iter()
            .find(|message| predicate(message))
        {
            return message.clone();
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for mock transport message");
}

/// Requests beyond the configured concurrency bound are rejected (backpressure)
/// rather than allowing the pending map to grow without bound.
#[tokio::test]
async fn request_beyond_concurrency_bound_is_rejected() {
    let (transport, _incoming_tx, outbound) = MockTransport::new();
    let client =
        Client::spawn_with_max_inflight(transport, client_info(), client_capabilities(), 1)
            .await
            .unwrap();

    // First request acquires the only permit and never receives a response.
    let first = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.request("tools/list", None, 2_000).await }
    });
    wait_for_message(&outbound, |message| {
        matches!(
            message,
            Message::Request(request)
                if request.id == RequestId::Number(1) && request.method == "tools/list"
        )
    })
    .await;

    // Second request cannot get a permit because the first is still in flight.
    // With a short timeout the caller is rejected with the backpressure error
    // instead of being queued forever.
    let second = client.request("tools/list", None, 25).await;
    assert!(matches!(
        second,
        Err(oc_mcp::Error::Message(message))
            if message.contains("concurrency limit reached")
    ));

    // Drop the first handle; its timeout will release the permit naturally.
    drop(first);
}

/// Once an in-flight request completes its permit is released, so subsequent
/// requests are not permanently blocked by a full queue.
#[tokio::test]
async fn permit_is_released_after_response_so_queue_drains() {
    let (transport, incoming_tx, outbound) = MockTransport::new();
    let client =
        Client::spawn_with_max_inflight(transport, client_info(), client_capabilities(), 1)
            .await
            .unwrap();

    let first = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.request("tools/list", None, 2_000).await }
    });
    wait_for_message(&outbound, |message| {
        matches!(
            message,
            Message::Request(request)
                if request.id == RequestId::Number(1) && request.method == "tools/list"
        )
    })
    .await;

    // Respond to the first request, releasing its permit.
    incoming_tx
        .send(Message::response(
            RequestId::Number(1),
            json!({ "tools": [{ "name": "a", "inputSchema": { "type": "object" } }] }),
        ))
        .unwrap();
    assert!(first.await.unwrap().is_ok());

    // A second request now succeeds (permit released).
    let second = tokio::spawn({
        let client = Arc::clone(&client);
        let incoming_tx = incoming_tx.clone();
        async move {
            let handle =
                tokio::spawn(async move { client.request("tools/list", None, 2_000).await });
            for _ in 0..200 {
                let id = outbound
                    .lock()
                    .await
                    .iter()
                    .find_map(|message| match message {
                        Message::Request(request)
                            if request.id == RequestId::Number(2)
                                && request.method == "tools/list" =>
                        {
                            Some(request.id.clone())
                        }
                        _ => None,
                    });
                if let Some(id) = id {
                    incoming_tx
                        .send(Message::response(id, json!({ "ok": true })))
                        .unwrap();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            handle.await.unwrap()
        }
    });
    assert_eq!(second.await.unwrap().unwrap(), json!({ "ok": true }));
}

/// Many concurrent requests are multiplexed over a single transport connection
/// (connection pooling): each carries a distinct JSON-RPC id and responses are
/// routed back to the correct caller.
#[tokio::test]
async fn multiplexes_concurrent_requests_over_single_transport() {
    let (transport, incoming_tx, outbound) = MockTransport::new();
    let client = Client::spawn(transport, client_info(), client_capabilities())
        .await
        .unwrap();

    const N: u64 = 16;
    let mut handles = Vec::new();
    for _ in 0..N {
        let client = Arc::clone(&client);
        handles.push(tokio::spawn(async move {
            client.request("tools/list", None, 2_000).await.unwrap()
        }));
    }

    // Wait until all 16 requests have been written to the transport. They are
    // all outstanding on the single connection simultaneously.
    for _ in 0..400 {
        let count = outbound
            .lock()
            .await
            .iter()
            .filter(|message| matches!(message, Message::Request(request) if request.method == "tools/list"))
            .count();
        if count >= N as usize {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // Collect the distinct ids that were sent.
    let ids: Vec<RequestId> = outbound
        .lock()
        .await
        .iter()
        .filter_map(|message| match message {
            Message::Request(request) if request.method == "tools/list" => Some(request.id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), N as usize, "all N requests multiplexed");

    // Respond to each with a value tagged with its id; the client must route
    // each response to the matching caller.
    for id in &ids {
        incoming_tx
            .send(Message::response(id.clone(), json!({ "echo": id })))
            .unwrap();
    }

    let results = futures::future::join_all(handles).await;
    for (handle, expected) in results.into_iter().zip(ids) {
        assert_eq!(handle.unwrap(), json!({ "echo": expected }));
    }
}

/// `notifications/resources/list_changed` and `notifications/prompts/list_changed`
/// are dispatched to registered notification handlers (the same path `MCP.watch`
/// uses to refresh resource/prompt catalogs on the lifecycle notifications).
#[tokio::test]
async fn resources_and_prompts_list_changed_are_dispatched_to_handlers() {
    let (transport, incoming_tx, _outbound) = MockTransport::new();
    let client = Client::spawn(transport, client_info(), client_capabilities())
        .await
        .unwrap();

    let resources_fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let prompts_fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    client
        .set_notification_handler("notifications/resources/list_changed", {
            let fired = Arc::clone(&resources_fired);
            Arc::new(move |_params: Option<serde_json::Value>| {
                fired.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
        })
        .await;
    client
        .set_notification_handler("notifications/prompts/list_changed", {
            let fired = Arc::clone(&prompts_fired);
            Arc::new(move |_params: Option<serde_json::Value>| {
                fired.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
        })
        .await;

    incoming_tx
        .send(Message::notification(
            "notifications/resources/list_changed",
            None,
        ))
        .unwrap();
    incoming_tx
        .send(Message::notification(
            "notifications/prompts/list_changed",
            None,
        ))
        .unwrap();

    for _ in 0..200 {
        if resources_fired.load(std::sync::atomic::Ordering::SeqCst) >= 1
            && prompts_fired.load(std::sync::atomic::Ordering::SeqCst) >= 1
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    assert_eq!(resources_fired.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(prompts_fired.load(std::sync::atomic::Ordering::SeqCst), 1);
}
