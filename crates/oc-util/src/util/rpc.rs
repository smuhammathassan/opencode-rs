use std::collections::HashMap;
/// From reference/packages/opencode/src/util/rpc.ts
///
/// Worker-style JSON RPC. The wire contract is identical to the reference:
/// `{"type":"rpc.request","method":...,"input":...,"id":N}`,
/// `{"type":"rpc.result","result":...,"id":N}`, and
/// `{"type":"rpc.event","event":...,"data":...}`.
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    #[serde(rename = "rpc.request")]
    Request {
        method: String,
        input: Value,
        id: u64,
    },
    #[serde(rename = "rpc.result")]
    Result { result: Value, id: u64 },
    #[serde(rename = "rpc.event")]
    Event { event: String, data: Value },
}

/// The server-side request handler (the reference's `Definition` object).
pub trait RpcHandler {
    fn call(&mut self, method: &str, input: Value) -> Result<Value, Box<dyn std::error::Error>>;
}

/// Server side: mirrors `listen(rpc)`. Parses an incoming message and, for
/// `rpc.request`, invokes the handler and posts a `rpc.result` (or nothing, if
/// the handler fails — as when the reference's method throws).
pub fn listen<H>(rpc: &mut H, raw: &str, post: &mut impl FnMut(&str))
where
    H: RpcHandler,
{
    let Ok(Message::Request { method, input, id }) = serde_json::from_str(raw) else {
        return;
    };
    if let Ok(result) = rpc.call(&method, input) {
        let response =
            serde_json::to_string(&Message::Result { result, id }).expect("serialize rpc result");
        post(&response);
    }
}

/// Server side: mirrors `emit(event, data)`.
pub fn emit(event: &str, data: &Value) -> String {
    serde_json::to_string(&Message::Event {
        event: event.to_string(),
        data: data.clone(),
    })
    .expect("serialize rpc event")
}

type Handler = Arc<dyn Fn(&Value) + Send + Sync>;

struct Inner {
    pending: HashMap<u64, oneshot::Sender<Value>>,
    listeners: HashMap<String, Vec<Handler>>,
    next_id: u64,
}

/// Client side: mirrors `client(target)`. `post` is the outbound
/// `postMessage`; the transport must call `on_message` with inbound strings.
pub struct Client {
    post: Box<dyn Fn(&str) + Send + Sync>,
    inner: Arc<Mutex<Inner>>,
}

impl Client {
    pub fn new(post: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Client {
            post: Box::new(post),
            inner: Arc::new(Mutex::new(Inner {
                pending: HashMap::new(),
                listeners: HashMap::new(),
                next_id: 0,
            })),
        }
    }

    pub fn call(&self, method: &str, input: Value) -> impl Future<Output = Value> + Send + 'static {
        let id = {
            let mut inner = self.inner.lock().expect("rpc poisoned");
            inner.next_id += 1;
            inner.next_id - 1
        };
        let (tx, rx) = oneshot::channel();
        self.inner
            .lock()
            .expect("rpc poisoned")
            .pending
            .insert(id, tx);
        let request = serde_json::to_string(&Message::Request {
            method: method.to_string(),
            input,
            id,
        })
        .expect("serialize rpc request");
        (self.post)(&request);
        async move { rx.await.unwrap_or(Value::Null) }
    }

    pub fn on_message(&self, raw: &str) {
        let Ok(parsed) = serde_json::from_str::<Message>(raw) else {
            return;
        };
        match parsed {
            Message::Result { result, id } => {
                if let Some(tx) = self.inner.lock().expect("rpc poisoned").pending.remove(&id) {
                    let _ = tx.send(result);
                }
            }
            Message::Event { event, data } => {
                let handlers = self
                    .inner
                    .lock()
                    .expect("rpc poisoned")
                    .listeners
                    .get(&event)
                    .cloned()
                    .unwrap_or_default();
                for handler in handlers {
                    handler(&data);
                }
            }
            Message::Request { .. } => {}
        }
    }

    pub fn on(
        &self,
        event: &str,
        handler: impl Fn(&Value) + Send + Sync + 'static,
    ) -> Subscription {
        let handler: Handler = Arc::new(handler);
        self.inner
            .lock()
            .expect("rpc poisoned")
            .listeners
            .entry(event.to_string())
            .or_default()
            .push(Arc::clone(&handler));
        Subscription {
            inner: Arc::clone(&self.inner),
            event: event.to_string(),
            handler,
        }
    }
}

/// Removing the handler on drop mirrors the unsubscribe function returned by
/// the reference's `on`.
pub struct Subscription {
    inner: Arc<Mutex<Inner>>,
    event: String,
    handler: Handler,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(handlers) = inner.listeners.get_mut(&self.event) {
                handlers.retain(|h| !Arc::ptr_eq(h, &self.handler));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex as StdMutex;

    #[test]
    fn message_serialization_matches_reference() {
        let request = Message::Request {
            method: "add".to_string(),
            input: json!([1, 2]),
            id: 0,
        };
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            r#"{"type":"rpc.request","method":"add","input":[1,2],"id":0}"#
        );
        let result = Message::Result {
            result: json!(3),
            id: 0,
        };
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            r#"{"type":"rpc.result","result":3,"id":0}"#
        );
        let event = Message::Event {
            event: "changed".to_string(),
            data: json!({ "a": 1 }),
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"type":"rpc.event","event":"changed","data":{"a":1}}"#
        );
    }

    struct Server {
        calls: StdMutex<Vec<(String, Value)>>,
    }

    impl RpcHandler for Server {
        fn call(
            &mut self,
            method: &str,
            input: Value,
        ) -> Result<Value, Box<dyn std::error::Error>> {
            self.calls
                .lock()
                .unwrap()
                .push((method.to_string(), input.clone()));
            if method == "fail" {
                return Err("nope".into());
            }
            Ok(json!({ "echo": input }))
        }
    }

    #[tokio::test]
    async fn request_result_round_trip() {
        let mailbox: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let client = Client::new({
            let mailbox = Arc::clone(&mailbox);
            move |message| mailbox.lock().unwrap().push(message.to_string())
        });
        let mut server = Server {
            calls: StdMutex::new(Vec::new()),
        };

        let fut = client.call("add", json!([1, 2]));
        let posted = mailbox.lock().unwrap().clone();
        assert_eq!(posted.len(), 1);
        let mut received = Vec::new();
        listen(&mut server, &posted[0], &mut |response| {
            received.push(response.to_string())
        });
        for response in &received {
            client.on_message(response);
        }
        assert_eq!(fut.await, json!({ "echo": [1, 2] }));
        assert_eq!(server.calls.lock().unwrap()[0].0, "add");
    }

    #[tokio::test]
    async fn failed_handler_posts_no_result() {
        let mailbox: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let client = Client::new({
            let mailbox = Arc::clone(&mailbox);
            move |message| mailbox.lock().unwrap().push(message.to_string())
        });
        let mut server = Server {
            calls: StdMutex::new(Vec::new()),
        };

        let fut = client.call("fail", json!(1));
        let posted = mailbox.lock().unwrap().clone();
        let mut received = Vec::new();
        listen(&mut server, &posted[0], &mut |response| {
            received.push(response.to_string())
        });
        assert!(received.is_empty());
        let _ = fut;
    }

    #[tokio::test]
    async fn events_reach_subscribers() {
        let mailbox: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let client = Client::new(move |message| mailbox.lock().unwrap().push(message.to_string()));
        let received: Arc<StdMutex<Vec<Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let subscription = client.on("update", {
            let received = Arc::clone(&received);
            move |data| received.lock().unwrap().push(data.clone())
        });
        client.on_message(&emit("update", &json!({ "n": 1 })));
        assert_eq!(received.lock().unwrap().len(), 1);
        drop(subscription);
        client.on_message(&emit("update", &json!({ "n": 2 })));
        assert_eq!(received.lock().unwrap().len(), 1);
    }
}
