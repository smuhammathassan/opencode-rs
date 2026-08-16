use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::{mpsc, oneshot, Mutex};

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
    for _ in 0..100 {
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

#[tokio::test]
async fn request_timeout_emits_cancellation_and_late_response_is_ignored() {
    let (transport, incoming_tx, outbound) = MockTransport::new();
    let client = Client::spawn(transport, client_info(), client_capabilities())
        .await
        .unwrap();

    let result = client.request("tools/list", None, 25).await;
    assert!(matches!(
        result,
        Err(oc_mcp::Error::Timeout {
            ms: 25,
            label
        }) if label == "tools/list"
    ));

    let cancellation = wait_for_message(&outbound, |message| {
        matches!(
            message,
            Message::Notification(notification)
                if notification.method == "notifications/cancelled"
        )
    })
    .await;
    assert_eq!(
        cancellation.to_line(),
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1,"reason":"Request timed out"}}"#
    );

    // A response arriving after cancellation has no waiter and must not
    // affect a subsequent request with a different JSON-RPC id.
    let mut second = tokio::spawn({
        let client = Arc::clone(&client);
        async move { client.request("tools/list", None, 250).await }
    });
    wait_for_message(&outbound, |message| {
        matches!(
            message,
            Message::Request(request)
                if request.id == RequestId::Number(2) && request.method == "tools/list"
        )
    })
    .await;
    incoming_tx
        .send(Message::response(
            RequestId::Number(1),
            json!({"late": true}),
        ))
        .unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut second)
        .await
        .is_err());

    incoming_tx
        .send(Message::response(RequestId::Number(2), json!({"ok": true})))
        .unwrap();
    assert_eq!(second.await.unwrap().unwrap(), json!({"ok": true}));
}

#[tokio::test]
async fn explicit_cancellation_emits_request_id_and_reason() {
    let (transport, incoming_tx, outbound) = MockTransport::new();
    let client = Client::spawn(transport, client_info(), client_capabilities())
        .await
        .unwrap();
    let (cancel_tx, cancel_rx) = oneshot::channel();

    let request = tokio::spawn(async move {
        client
            .request_cancellable(
                "resources/list",
                None,
                1_000,
                async move {
                    let _ = cancel_rx.await;
                },
                "User requested cancellation",
            )
            .await
    });

    wait_for_message(&outbound, |message| {
        matches!(
            message,
            Message::Request(request)
                if request.id == RequestId::Number(1) && request.method == "resources/list"
        )
    })
    .await;
    cancel_tx.send(()).unwrap();

    assert!(matches!(
        request.await.unwrap(),
        Err(oc_mcp::Error::Message(message)) if message == "request cancelled"
    ));
    let cancellation = wait_for_message(&outbound, |message| {
        matches!(
            message,
            Message::Notification(notification)
                if notification.method == "notifications/cancelled"
        )
    })
    .await;
    assert_eq!(
        cancellation.to_line(),
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1,"reason":"User requested cancellation"}}"#
    );

    // The late response is accepted by the transport but ignored by the
    // client because the pending waiter was removed before notification send.
    incoming_tx
        .send(Message::response(
            RequestId::Number(1),
            json!({"late": true}),
        ))
        .unwrap();
}

#[tokio::test]
async fn initialize_timeout_removes_waiter_without_wire_cancellation() {
    let (transport, _incoming_tx, outbound) = MockTransport::new();
    let client = Client::spawn(transport, client_info(), client_capabilities())
        .await
        .unwrap();

    let result = client.initialize(25).await;
    assert!(matches!(result, Err(oc_mcp::Error::Timeout { .. })));
    tokio::time::sleep(Duration::from_millis(10)).await;

    let messages = outbound.lock().await;
    assert_eq!(messages.len(), 1);
    assert!(matches!(
        &messages[0],
        Message::Request(request) if request.method == "initialize"
    ));
}

#[tokio::test]
async fn call_tool_timeout_emits_cancellation_with_tool_request_id() {
    let (transport, _incoming_tx, outbound) = MockTransport::new();
    let client = Client::spawn(transport, client_info(), client_capabilities())
        .await
        .unwrap();

    let result = client.call_tool("slow", json!({}), 25).await;
    assert!(matches!(result, Err(oc_mcp::Error::Timeout { .. })));
    let cancellation = wait_for_message(&outbound, |message| {
        matches!(
            message,
            Message::Notification(notification)
                if notification.method == "notifications/cancelled"
        )
    })
    .await;
    assert_eq!(
        cancellation.to_line(),
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1,"reason":"Request timed out"}}"#
    );
}
