//! End-to-end plugin host tests using the reference example plugins
//! (reference/packages/plugin/src/example.ts and example-workspace.ts).

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oc_plugin::js::transpile::transpile_module;
use oc_plugin::{
    AuthAuthorizeRequest, AuthCallbackRequest, AuthValidateRequest, ClientRpcRequest,
    InMemoryRegistrationSink, ModuleResolver, NoopHost, PluginBuilder, PluginHost, PluginManager,
    PluginRegistrationSink, PluginToolCancellation,
};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

fn make_resolver() -> Arc<ModuleResolver> {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    Arc::new(ModuleResolver::new(base))
}

fn build_plugin() -> oc_plugin::LoadedPlugin {
    build_plugin_with_host(Arc::new(NoopHost))
}

fn build_plugin_with_host(host: Arc<dyn PluginHost>) -> oc_plugin::LoadedPlugin {
    let resolver = make_resolver();
    let builder = PluginBuilder::new(host, resolver);
    builder.build().expect("failed to build plugin context")
}

#[derive(Default)]
struct RecordingHost {
    logs: Mutex<Vec<String>>,
}

impl PluginHost for RecordingHost {
    fn log(&self, _level: &str, message: &str) {
        self.logs.lock().unwrap().push(message.to_string());
    }
}

fn load_code(plugin: &mut oc_plugin::LoadedPlugin, source: &str, filename: &str) {
    let code = transpile_module(source).expect("failed to transpile plugin");
    let input = serde_json::json!({
        "project": { "id": "test", "directory": "/tmp/test" },
        "directory": "/tmp/test",
        "worktree": "/tmp/test",
        "serverUrl": "http://localhost:4096",
    });
    plugin
        .load(&code, filename, input, None)
        .expect("failed to load plugin");
}

#[test]
fn loads_reference_example_plugin() {
    let source = fixture("example.ts");
    let mut plugin = build_plugin();
    load_code(&mut plugin, &source, "example.ts");

    let summary = plugin.summary();
    assert!(summary.hook_names.contains(&"tool".to_string()));
    assert_eq!(summary.tools.len(), 1);
    let tool = &summary.tools[0];
    assert_eq!(tool.name, "mytool");
    assert_eq!(tool.description, "This is a custom tool");
    assert_eq!(tool.schema["type"], "object");
    assert_eq!(tool.schema["required"], serde_json::json!(["foo"]));
    assert!(tool.schema["properties"]["foo"]["description"].is_string());
}

#[test]
fn auth_summary_and_calls_stay_on_the_plugin_runtime() {
    let mut plugin = build_plugin();
    load_code(&mut plugin, &fixture("auth.ts"), "auth.ts");

    let summary = plugin.summary();
    assert_eq!(summary.auth.len(), 1);
    assert_eq!(summary.auth[0].provider, "fixture-provider");
    assert_eq!(summary.auth[0].methods.len(), 1);
    assert_eq!(summary.auth[0].methods[0].label, "Fixture OAuth");
    assert_eq!(
        summary.auth[0].methods[0].r#type,
        oc_plugin::PluginAuthMethodType::OAuth
    );
    let prompts = summary.auth[0].methods[0]
        .prompts
        .as_ref()
        .expect("fixture auth prompts");
    assert_eq!(prompts.len(), 2);
    assert!(!serde_json::to_string(summary).unwrap().contains("callback"));

    assert_eq!(
        plugin
            .auth_validate("fixture-provider", 0, "account", "nope")
            .unwrap(),
        Some("account is invalid".to_string())
    );
    assert_eq!(
        plugin
            .auth_validate("fixture-provider", 0, "account", "ok")
            .unwrap(),
        None
    );

    let authorization = plugin
        .auth_authorize(
            "fixture-provider",
            0,
            &std::collections::BTreeMap::from([(String::from("account"), String::from("team"))]),
        )
        .expect("auth authorize dispatch");
    assert_eq!(authorization["method"], "code");
    assert_eq!(
        authorization["url"],
        "https://auth.example.test/authorize?account=team"
    );
    assert!(!authorization.to_string().contains("callback"));

    let callback = plugin
        .auth_callback("fixture-provider", 0, Some("abc"))
        .expect("auth callback dispatch");
    assert_eq!(callback["type"], "success");
    assert_eq!(callback["refresh"], "refresh-abc");
    assert_eq!(callback["access"], "access-abc");
}

#[test]
fn manager_auth_requests_dispatch_loaded_fixture_on_owner_thread() {
    let manager = PluginManager::new();
    let spec = format!(
        "file://{}/tests/fixtures/auth.ts",
        env!("CARGO_MANIFEST_DIR")
    );
    let report = manager.load_local(spec, serde_json::json!({}), None);
    assert!(report.error.is_none());
    assert_eq!(report.summary.unwrap().auth[0].provider, "fixture-provider");

    assert_eq!(
        manager
            .auth_validate(AuthValidateRequest {
                provider: "fixture-provider".into(),
                method: 0,
                key: "account".into(),
                value: "ok".into(),
            })
            .unwrap(),
        None
    );

    let authorization = manager
        .auth_authorize(AuthAuthorizeRequest {
            provider: "fixture-provider".into(),
            method: 0,
            inputs: std::collections::BTreeMap::from([("account".into(), "thread".into())]),
        })
        .unwrap();
    assert_eq!(authorization["method"], "code");
    let callback = manager
        .auth_callback(AuthCallbackRequest {
            provider: "fixture-provider".into(),
            method: 0,
            code: Some("owner-thread".into()),
        })
        .unwrap();
    assert_eq!(callback["access"], "access-owner-thread");
    manager.dispose().unwrap();
}

#[test]
fn manager_dispose_runs_plugin_hooks_on_owner_thread() {
    let path = std::env::temp_dir().join(format!("oc-plugin-dispose-{}.ts", std::process::id()));
    std::fs::write(
        &path,
        r#"
export default {
  server: async () => ({
    dispose: async () => { console.log("disposed") },
  }),
}
"#,
    )
    .unwrap();
    let host = Arc::new(RecordingHost::default());
    let manager = PluginManager::with_host(host.clone());
    let report = manager.load_local(
        format!("file://{}", path.display()),
        serde_json::json!({}),
        None,
    );
    assert!(report.error.is_none(), "dispose fixture failed: {report:?}");
    manager.dispose().unwrap();
    assert_eq!(host.logs.lock().unwrap().as_slice(), ["disposed"]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn executes_reference_example_tool() {
    let source = fixture("example.ts");
    let mut plugin = build_plugin();
    load_code(&mut plugin, &source, "example.ts");

    let context = serde_json::json!({
        "sessionID": "s1",
        "messageID": "m1",
        "agent": "build",
        "directory": "/tmp/test",
        "worktree": "/tmp/test",
        "callID": "c1",
    });
    let result = plugin
        .execute_tool("mytool", serde_json::json!({ "foo": "world" }), context)
        .expect("tool execution failed");
    assert_eq!(result, serde_json::json!("Hello world!"));
}

#[test]
fn async_tool_observes_cooperative_cancellation() {
    let path =
        std::env::temp_dir().join(format!("oc-plugin-cancellation-{}.ts", std::process::id()));
    std::fs::write(
        &path,
        r#"
export default {
  server: async () => ({
    tool: {
      cancellable: {
        description: "cancellable",
        args: {},
        execute: async (_args, context) => {
          console.log("cancellation-started")
          let notified = false
          context.abort.addEventListener("abort", () => { notified = true })
          while (!notified) {
            await Promise.resolve()
          }
          return { observed: context.abort.aborted, notified }
        },
      },
    },
  }),
}
"#,
    )
    .unwrap();

    let host = Arc::new(RecordingHost::default());
    let manager = PluginManager::with_host(host.clone());
    let report = manager.load_local(
        format!("file://{}", path.display()),
        serde_json::json!({}),
        None,
    );
    assert!(
        report.error.is_none(),
        "cancellation fixture failed: {report:?}"
    );

    let cancellation = PluginToolCancellation::new();
    let cancellation_for_call = cancellation.clone();
    let manager_for_call = manager.clone();
    let call = thread::spawn(move || {
        manager_for_call.execute_tool_with_cancellation(
            "cancellable",
            serde_json::json!({}),
            serde_json::json!({
                "sessionID": "ses_cancel",
                "messageID": "msg_cancel",
                "agent": "build",
                "directory": "/tmp",
                "worktree": "/tmp",
                "callID": "call_cancel",
            }),
            cancellation_for_call,
        )
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    while !host
        .logs
        .lock()
        .unwrap()
        .iter()
        .any(|message| message == "cancellation-started")
    {
        assert!(Instant::now() < deadline, "async tool did not start");
        thread::yield_now();
    }
    cancellation.cancel();

    let result = call
        .join()
        .expect("plugin tool thread panicked")
        .expect("cancelled async plugin tool failed");
    assert_eq!(
        result,
        serde_json::json!({ "observed": true, "notified": true })
    );
    assert!(cancellation.is_cancelled());

    manager.dispose().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn loads_reference_example_workspace_plugin() {
    let source = fixture("example-workspace.ts");
    let mut plugin = build_plugin();
    load_code(&mut plugin, &source, "example-workspace.ts");

    let summary = plugin.summary();
    assert!(summary.tools.is_empty());

    // The adapter registered a "folder" workspace type; invoke its methods.
    let config = serde_json::json!({
        "id": "w1",
        "type": "local",
        "name": "Folder",
        "branch": null,
        "directory": null,
        "extra": null,
        "projectID": "p1",
    });
    let configured = plugin
        .workspace_adapter("folder", "configure", config.clone())
        .expect("configure failed");
    assert!(configured["directory"]
        .as_str()
        .unwrap()
        .starts_with("/tmp/folder/folder-"));
    let directory = configured["directory"].as_str().unwrap();

    let target = plugin
        .workspace_adapter(
            "folder",
            "target",
            serde_json::json!({ "directory": directory }),
        )
        .expect("target failed");
    assert_eq!(target["type"], "local");
    assert_eq!(target["directory"].as_str().unwrap(), directory);
}

#[derive(Clone)]
struct RegistrationHost {
    sink: InMemoryRegistrationSink,
}

impl PluginHost for RegistrationHost {
    fn registration_sink(&self) -> Option<Arc<dyn PluginRegistrationSink>> {
        Some(Arc::new(self.sink.clone()))
    }
}

#[test]
fn plugin_registrations_reach_typed_sink_with_plugin_id() {
    let source = r#"
import plugin from "opencode/plugin"

export default {
  id: "registration-test",
  server: async () => {
    plugin.command({ name: "review", template: "Review the diff" })
    plugin.skill({ name: "rust", description: "Rust helpers" })
    return {}
  },
}
"#;
    let sink = InMemoryRegistrationSink::default();
    let host = Arc::new(RegistrationHost { sink: sink.clone() });
    let mut plugin = build_plugin_with_host(host);
    load_code(&mut plugin, source, "registration-test.ts");

    let registrations = sink.snapshot();
    assert_eq!(registrations.len(), 2);
    assert_eq!(
        registrations[0].plugin_id.as_deref(),
        Some("registration-test")
    );
    assert_eq!(registrations[0].kind, "command");
    assert_eq!(registrations[0].input["name"], "review");
    assert_eq!(
        registrations[1].plugin_id.as_deref(),
        Some("registration-test")
    );
    assert_eq!(registrations[1].kind, "skill");
    assert_eq!(registrations[1].input["name"], "rust");
}

#[test]
fn client_bridge_uses_typed_rpc_request_contract() {
    struct ClientHost;

    impl PluginHost for ClientHost {
        fn client_rpc(&self, request: &ClientRpcRequest) -> Result<serde_json::Value, String> {
            assert_eq!(request.method, "session.get");
            assert_eq!(request.args["sessionID"], "session-1");
            Ok(serde_json::json!({ "data": { "id": "session-1" } }))
        }
    }

    let resolver = ModuleResolver::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    let result = oc_plugin::bridge::dispatch(
        &ClientHost,
        &resolver,
        "client",
        &serde_json::json!({
            "method": "session.get",
            "args": { "sessionID": "session-1" }
        }),
    )
    .expect("client RPC should reach the typed host contract");
    assert_eq!(result["data"]["id"], "session-1");
}

#[test]
fn client_rpc_matches_async_sdk_boundary() {
    struct ClientHost;

    impl PluginHost for ClientHost {
        fn client_rpc(&self, request: &ClientRpcRequest) -> Result<serde_json::Value, String> {
            match request.method.as_str() {
                "session.get" => {
                    assert_eq!(request.args["sessionID"], "session-async");
                    Ok(serde_json::json!({ "data": { "id": "session-async" } }))
                }
                "session.remove" => Err("not found".into()),
                other => panic!("unexpected client method: {other}"),
            }
        }
    }

    let source = r#"
export default {
  server: async ({ client }) => {
    const pending = client.session.get({ sessionID: "session-async" })
    if (!pending || typeof pending.then !== "function") throw new Error("client call was not thenable")
    const session = await pending
    if (session.id !== "session-async") throw new Error("client response was not unwrapped")
    const rejected = await client.session.remove({ sessionID: "missing" }).then(
      () => false,
      (error) => error.message === "not found",
    )
    if (!rejected) throw new Error("client error did not reject")
    return {}
  },
}
"#;
    let mut plugin = build_plugin_with_host(Arc::new(ClientHost));
    load_code(&mut plugin, source, "async-client.ts");
}

#[test]
fn client_inventory_includes_session_status() {
    struct ClientHost;

    impl PluginHost for ClientHost {
        fn client_rpc(&self, request: &ClientRpcRequest) -> Result<serde_json::Value, String> {
            assert_eq!(request.method, "session.status");
            assert_eq!(request.args, serde_json::Value::Null);
            Ok(serde_json::json!({ "data": { "busy": false } }))
        }
    }

    let source = r#"
export default {
  server: async ({ client }) => {
    const status = await client.session.status()
    if (status.busy !== false) throw new Error("session status was not unwrapped")
    return {}
  },
}
"#;
    let mut plugin = build_plugin_with_host(Arc::new(ClientHost));
    load_code(&mut plugin, source, "session-status.ts");
}

#[test]
fn client_inventory_includes_skill_list() {
    struct ClientHost;

    impl PluginHost for ClientHost {
        fn client_rpc(&self, request: &ClientRpcRequest) -> Result<serde_json::Value, String> {
            assert_eq!(request.method, "skill.list");
            assert_eq!(request.args, serde_json::Value::Null);
            Ok(serde_json::json!({
                "data": [{ "name": "rust", "description": "Rust helpers" }]
            }))
        }
    }

    let source = r#"
export default {
  server: async ({ client }) => {
    const skills = await client.skill.list()
    if (!Array.isArray(skills) || skills[0].name !== "rust") throw new Error("skill list was not unwrapped")
    return {}
  },
}
"#;
    let mut plugin = build_plugin_with_host(Arc::new(ClientHost));
    load_code(&mut plugin, source, "skill-list.ts");
}

#[test]
fn client_inventory_supports_nested_v118_methods_and_unwraps_data() {
    struct ClientHost;

    impl PluginHost for ClientHost {
        fn client_rpc(&self, request: &ClientRpcRequest) -> Result<serde_json::Value, String> {
            match request.method.as_str() {
                "pty.connect" => {
                    assert_eq!(request.args["ptyID"], "pty-1");
                    Ok(serde_json::json!({ "data": { "connected": true } }))
                }
                "path.get" => {
                    assert_eq!(request.args["path"], "README.md");
                    Ok(serde_json::json!({ "data": { "path": "/tmp/test/README.md" } }))
                }
                "provider.oauth.callback" => {
                    assert_eq!(request.args["providerID"], "fixture");
                    Ok(serde_json::json!({ "data": { "access": "token" } }))
                }
                "tui.control.response" => {
                    assert_eq!(request.args["requestID"], "request-1");
                    Ok(serde_json::json!({ "data": { "accepted": true } }))
                }
                other => panic!("unexpected client method: {other}"),
            }
        }
    }

    let source = r#"
export default {
  server: async ({ client }) => {
    const pty = await client.pty.connect({ ptyID: "pty-1" })
    if (pty.connected !== true) throw new Error("pty response was not unwrapped")
    const path = await client.path.get({ path: "README.md" })
    if (path.path !== "/tmp/test/README.md") throw new Error("path response was not unwrapped")
    const oauth = await client.provider.oauth.callback({ providerID: "fixture" })
    if (oauth.access !== "token") throw new Error("oauth response was not unwrapped")
    const response = await client.tui.control.response({ requestID: "request-1" })
    if (response.accepted !== true) throw new Error("tui response was not unwrapped")
    return {}
  },
}
"#;
    let mut plugin = build_plugin_with_host(Arc::new(ClientHost));
    load_code(&mut plugin, source, "nested-client.ts");
}

#[test]
fn triggers_hooks() {
    // A plugin that registers several (input, output) hooks; verify output
    // mutation round-trips through the JSON bridge.
    let source = r#"
export default {
  id: "hook-test",
  server: async (input, options) => {
    return {
      "chat.message": async (input, output) => {
        output.visited = true
        output.count = (output.count || 0) + 1
      },
      "tool.execute.before": async (input, output) => {
        output.args = { ...(output.args || {}), extra: "added" }
      },
      config: async (config) => {},
      event: async ({ event }) => {},
      dispose: async () => {},
    }
  },
}
"#;
    let mut plugin = build_plugin();
    load_code(&mut plugin, source, "hooks.ts");
    let summary = plugin.summary();
    for name in [
        "chat.message",
        "tool.execute.before",
        "config",
        "event",
        "dispose",
    ] {
        assert!(
            summary.hook_names.contains(&name.to_string()),
            "missing hook {name}"
        );
    }

    let output = plugin
        .trigger(
            "chat.message",
            serde_json::json!({ "sessionID": "s" }),
            serde_json::json!({}),
        )
        .expect("trigger failed");
    assert_eq!(output["visited"], true);
    assert_eq!(output["count"], 1);

    let output = plugin
        .trigger(
            "tool.execute.before",
            serde_json::json!({ "tool": "read" }),
            serde_json::json!({}),
        )
        .expect("trigger failed");
    assert_eq!(output["args"]["extra"], "added");

    plugin
        .config(serde_json::json!({ "theme": "dark" }))
        .expect("config hook failed");
    plugin
        .event(serde_json::json!({ "id": "e1", "type": "message.created", "properties": {} }))
        .expect("event hook failed");
    plugin.dispose().expect("dispose failed");
}

#[test]
fn event_trigger_waits_for_async_hook_and_propagates_host_effect() {
    let host = Arc::new(RecordingHost::default());
    let mut plugin = build_plugin_with_host(host.clone());
    load_code(&mut plugin, &fixture("event.ts"), "event.ts");

    plugin
        .trigger(
            "event",
            serde_json::json!({ "event": { "type": "session.updated" } }),
            serde_json::json!({}),
        )
        .expect("event hook failed");

    assert_eq!(
        host.logs.lock().unwrap().as_slice(),
        ["received:session.updated"]
    );
    plugin.dispose().expect("dispose after event failed");
}

#[test]
fn sse_stream_delivers_on_owner_thread_and_done_cancels_subscription() {
    let host = Arc::new(RecordingHost::default());
    let source = r#"
let stream
export default async ({ client }) => {
  stream = client.sse.stream("/global/event")
  stream.on("message", (event) => {
    console.log("stream:" + event.type)
    stream.done()
  })
}
"#;
    let mut plugin = build_plugin_with_host(host.clone());
    load_code(&mut plugin, source, "stream.ts");

    plugin
        .stream_event(serde_json::json!({ "type": "session.updated" }))
        .expect("first stream event failed");
    plugin
        .stream_event(serde_json::json!({ "type": "session.deleted" }))
        .expect("second stream event failed");

    assert_eq!(
        host.logs.lock().unwrap().as_slice(),
        ["stream:session.updated"]
    );
    plugin.dispose().expect("dispose after stream failed");
}

#[test]
fn manager_queues_stream_events_on_the_plugin_owner_thread() {
    let host = Arc::new(RecordingHost::default());
    let manager = PluginManager::with_host(host.clone());
    let spec = format!(
        "file://{}/tests/fixtures/stream.ts",
        env!("CARGO_MANIFEST_DIR")
    );
    let report = manager.load_local(spec, serde_json::json!({}), None);
    assert!(report.error.is_none(), "stream fixture failed: {report:?}");

    manager
        .stream_event(serde_json::json!({ "type": "session.updated" }))
        .expect("stream event should enqueue");
    for _ in 0..100 {
        if host.logs.lock().unwrap().as_slice() == ["stream:session.updated"] {
            manager.dispose().expect("dispose after stream failed");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    manager.dispose().expect("dispose after stream failed");
    panic!("queued stream event was not delivered");
}

#[test]
fn module_resolver_resolves_relative_imports() {
    let resolver = make_resolver();
    let code = resolver
        .resolve("./tool.js")
        .expect("resolve failed")
        .expect("tool.js not resolved");
    assert!(code.contains("__oc_require"));
}

#[test]
fn v2_promise_plugin_transform() {
    // A v2 promise-style plugin (`{ id, setup }`) registering an agent domain
    // transform. The host drives the draft through the JS callback.
    let source = r#"
import { define } from "opencode/plugin/v2/promise"
export default define({
  id: "v2-test",
  setup: async (ctx) => {
    await ctx.agent.transform((draft) => {
      draft.update("build", { model: "primary" })
      draft.added = true
    })
    await ctx.command.transform((draft) => {
      draft.commands = ["init"]
    })
  },
})
"#;
    let mut plugin = build_plugin();
    load_code(&mut plugin, source, "v2.ts");

    let draft = plugin
        .v2_transform("agent", serde_json::json!({ "agents": [] }))
        .expect("v2 transform failed");
    assert_eq!(draft["agents"][0]["id"], "build");
    assert_eq!(draft["agents"][0]["model"], "primary");

    let commands = plugin
        .v2_transform("command", serde_json::json!({}))
        .expect("v2 transform failed");
    assert_eq!(commands["commands"], serde_json::json!(["init"]));
}

#[test]
fn imports_shell_and_tui_polyfills() {
    let source = r#"
import { tool, z } from "opencode/plugin/tool"
import { $ } from "opencode/plugin/shell"
import { createBindingLookup } from "opencode/plugin/tui"

const escaped = $.escape("hello world")
const lookup = createBindingLookup({}, {})

export default {
  id: "polyfill-surface",
  server: async () => ({
    tool: {
      surface: tool({
        description: escaped + ":" + String(lookup.has("missing")),
        args: { value: z.string() },
        execute: async () => "ok",
      }),
    },
  }),
}
"#;
    let mut plugin = build_plugin();
    load_code(&mut plugin, source, "polyfill-surface.ts");

    let tool = plugin
        .summary()
        .tools
        .iter()
        .find(|tool| tool.name == "surface")
        .expect("shell/tui polyfill imports should register a tool");
    assert_eq!(tool.description, "hello\\ world:false");
    assert_eq!(tool.schema["required"], serde_json::json!(["value"]));
}

// ---------------------------------------------------------------------------
// F125: request lifecycle — request ids, multiplexed responses, cancellation
// ---------------------------------------------------------------------------

#[test]
fn client_calls_carry_distinct_request_ids_and_multiplex() {
    struct MultiplexHost {
        ids: Mutex<Vec<String>>,
    }

    impl PluginHost for MultiplexHost {
        fn client_rpc(&self, request: &ClientRpcRequest) -> Result<serde_json::Value, String> {
            assert!(request.request_id.is_some(), "request id must be set");
            let id = request.request_id.clone().unwrap();
            assert!(
                !self.ids.lock().unwrap().contains(&id),
                "request ids must be unique"
            );
            self.ids.lock().unwrap().push(id);
            match request.method.as_str() {
                "session.get" => Ok(serde_json::json!({
                    "data": { "id": request.args["sessionID"] }
                })),
                "config.get" => Ok(serde_json::json!({ "data": { "theme": "dark" } })),
                other => panic!("unexpected client method: {other}"),
            }
        }
    }

    let host = Arc::new(MultiplexHost {
        ids: Mutex::new(Vec::new()),
    });
    let source = r#"
export default {
  server: async ({ client }) => {
    // Two concurrent calls multiplex through the same bridge with distinct
    // request ids and both resolve independently.
    const [session, config] = await Promise.all([
      client.session.get({ sessionID: "ses_mux" }),
      client.config.get(),
    ])
    if (session.id !== "ses_mux") throw new Error("session response mismatch")
    if (config.theme !== "dark") throw new Error("config response mismatch")
    return {}
  },
}
"#;
    let mut plugin = build_plugin_with_host(host.clone());
    load_code(&mut plugin, source, "multiplex.ts");
    let ids = host.ids.lock().unwrap();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn client_cancel_rejects_the_inflight_request() {
    struct CancelHost {
        seen: Mutex<Vec<String>>,
        cancelled: Mutex<Vec<String>>,
    }

    impl PluginHost for CancelHost {
        fn client_rpc(&self, request: &ClientRpcRequest) -> Result<serde_json::Value, String> {
            self.seen
                .lock()
                .unwrap()
                .push(request.request_id.clone().unwrap_or_default());
            match request.method.as_str() {
                "session.get" => Ok(serde_json::json!({
                    "data": { "id": request.args["sessionID"] }
                })),
                other => panic!("unexpected client method: {other}"),
            }
        }
        fn client_cancel(&self, request_id: &str) {
            self.cancelled.lock().unwrap().push(request_id.to_string());
        }
    }

    let host = Arc::new(CancelHost {
        seen: Mutex::new(Vec::new()),
        cancelled: Mutex::new(Vec::new()),
    });
    let source = r#"
export default {
  server: async ({ client }) => {
    // A completed call first, so the host observes its request id.
    const first = await client.session.get({ sessionID: "ses_ok" })
    if (first.id !== "ses_ok") throw new Error("first call failed")
    // A second call is cancelled before its bridge round-trip settles.
    const pending = client.session.get({ sessionID: "ses_cancel" })
    if (!pending.requestID) throw new Error("promise did not expose requestID")
    const cancelled = client.cancel(pending.requestID)
    if (!cancelled) throw new Error("cancel returned false for in-flight request")
    const rejected = await pending.then(
      () => false,
      (error) => error.name === "AbortError",
    )
    if (!rejected) throw new Error("cancelled request did not reject with AbortError")
    return {}
  },
}
"#;
    let mut plugin = build_plugin_with_host(host.clone());
    load_code(&mut plugin, source, "cancel.ts");
    let seen = host.seen.lock().unwrap();
    let cancelled = host.cancelled.lock().unwrap();
    // The completed call reached the host with its request id; the cancelled
    // call was aborted before its bridge round-trip and the host still saw the
    // cancellation routed for the same request id.
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].starts_with("req_"),
        "unexpected request id {seen:?}"
    );
    assert_eq!(cancelled.len(), 1);
    assert_eq!(
        cancelled[0], "req_2",
        "cancel must target the second request"
    );
}

#[test]
fn execute_tool_timeout_aborts_runaway_plugin() {
    let path = std::env::temp_dir().join(format!("oc-plugin-spin-{}.ts", std::process::id()));
    std::fs::write(
        &path,
        r#"
export default {
  server: async () => ({
    tool: {
      spin: {
        description: "spins forever",
        args: {},
        execute: async () => {
          // The loop runs inside a promise job so the QuickJS interrupt
          // handler can abort it cleanly (see js::runtime docs).
          await Promise.resolve()
          while (true) { /* runaway */ }
        },
      },
    },
  }),
}
"#,
    )
    .unwrap();

    let manager = PluginManager::with_host(Arc::new(RecordingHost::default()));
    let report = manager.load_local(
        format!("file://{}", path.display()),
        serde_json::json!({}),
        None,
    );
    assert!(report.error.is_none(), "spin fixture failed: {report:?}");

    // The tool runs through the manager on the owner thread; the QuickJS
    // interrupt handler aborts the infinite loop and the manager surfaces it.
    let result = manager.execute_tool_with_timeout(
        "spin",
        serde_json::json!({}),
        serde_json::json!({ "callID": "spin-1" }),
        std::time::Duration::from_millis(300),
    );
    assert!(result.is_err(), "runaway tool should not return a value");
    assert!(
        result.as_ref().unwrap_err().contains("budget"),
        "limit error should describe the budget, got {result:?}"
    );
    // The manager stays healthy after the abort.
    let summary = manager.load_local(
        format!(
            "file://{}/tests/fixtures/example.ts",
            env!("CARGO_MANIFEST_DIR")
        ),
        serde_json::json!({}),
        None,
    );
    assert!(summary.error.is_none());
    manager.dispose().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn sse_stream_exposes_backpressure_stats() {
    let host = Arc::new(RecordingHost::default());
    let source = r#"
let stream
export default async ({ client }) => {
  stream = client.sse.stream("/global/event")
  const stats = stream.backpressure()
  if (typeof stats.pending !== "number") throw new Error("missing pending stat")
  if (typeof stats.dropped !== "number") throw new Error("missing dropped stat")
  if (!(stats.max >= 1)) throw new Error("missing max stat")
  stream.on("message", () => stream.done())
}
"#;
    let mut plugin = build_plugin_with_host(host.clone());
    load_code(&mut plugin, source, "backpressure.ts");
    plugin
        .stream_event(serde_json::json!({ "type": "session.updated" }))
        .expect("stream event delivery failed");
    plugin.dispose().expect("dispose after stream failed");
}

#[test]
fn manager_stream_event_accepts_backpressure_contract() {
    // The manager's stream queue is bounded; normal delivery still succeeds
    // and repeated delivery does not grow the queue without bound.
    let host = Arc::new(RecordingHost::default());
    let manager = PluginManager::with_host(host.clone());
    let spec = format!(
        "file://{}/tests/fixtures/stream.ts",
        env!("CARGO_MANIFEST_DIR")
    );
    let report = manager.load_local(spec, serde_json::json!({}), None);
    assert!(report.error.is_none(), "stream fixture failed: {report:?}");

    for _ in 0..200 {
        manager
            .stream_event(serde_json::json!({ "type": "session.updated" }))
            .expect("stream event should enqueue");
    }
    for _ in 0..500 {
        let logs = host.logs.lock().unwrap();
        let delivered = logs
            .iter()
            .filter(|message| message.as_str() == "stream:session.updated")
            .count();
        if delivered >= 200 {
            manager.dispose().expect("dispose after stream failed");
            return;
        }
        drop(logs);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    manager.dispose().expect("dispose after stream failed");
    panic!("queued stream events were not delivered");
}

// ---------------------------------------------------------------------------
// F127: remaining SDK surface + v2 effect module semantics
// ---------------------------------------------------------------------------

#[test]
fn client_inventory_covers_remaining_sdk_methods() {
    struct ClientHost;

    impl PluginHost for ClientHost {
        fn client_rpc(&self, request: &ClientRpcRequest) -> Result<serde_json::Value, String> {
            match request.method.as_str() {
                "session.update" => {
                    assert_eq!(request.args["sessionID"], "ses_u");
                    Ok(serde_json::json!({ "data": { "id": "ses_u" } }))
                }
                "session.permission" => {
                    assert_eq!(request.args["sessionID"], "ses_p");
                    assert_eq!(request.args["permissionID"], "perm_1");
                    Ok(serde_json::json!({ "data": { "status": "allow" } }))
                }
                "project.current" => Ok(serde_json::json!({
                    "data": { "id": "prj_1", "directory": "/tmp" }
                })),
                "app.skills" => Ok(serde_json::json!({
                    "data": [{ "name": "rust", "description": "Rust helpers" }]
                })),
                other => panic!("unexpected client method: {other}"),
            }
        }
    }

    let source = r#"
export default {
  server: async ({ client }) => {
    const updated = await client.session.update({ sessionID: "ses_u", info: { title: "t" } })
    if (updated.id !== "ses_u") throw new Error("session.update not unwrapped")
    const permission = await client.session.permission({ sessionID: "ses_p", permissionID: "perm_1", input: {} })
    if (permission.status !== "allow") throw new Error("session.permission not unwrapped")
    const project = await client.project.current()
    if (project.directory !== "/tmp") throw new Error("project.current not unwrapped")
    const skills = await client.app.skills()
    if (!Array.isArray(skills) || skills[0].name !== "rust") throw new Error("app.skills not unwrapped")
    const stream = client.event.subscribe("/global/event")
    if (!stream || typeof stream.on !== "function") throw new Error("event.subscribe is not an SSE stream")
    stream.done()
    return {}
  },
}
"#;
    let mut plugin = build_plugin_with_host(Arc::new(ClientHost));
    load_code(&mut plugin, source, "remaining-client.ts");
}

#[test]
fn v2_agent_transform_exposes_draft_semantics() {
    let source = r#"
import { define } from "opencode/plugin/v2/promise"
export default define({
  id: "v2-agent",
  setup: async (ctx) => {
    await ctx.agent.transform((draft) => {
      if (!Array.isArray(draft.list())) throw new Error("draft.list must be an array")
      draft.update("a1", { model: "small" })
      draft.update("a2", { temperature: 0.2 })
      draft.remove("a1")
      draft.default("a2")
      const got = draft.get("a2")
      if (!got || got.temperature !== 0.2) throw new Error("draft.get did not see the update")
    })
    await ctx.catalog.transform((draft) => {
      draft.provider.update("acme", (provider) => {
        provider.npm = "@acme/provider"
        provider.api = { type: "aisdk", package: "@ai-sdk/openai-compatible", url: "https://api.acme.test/v1" }
        provider.request = { headers: {} }
      })
      draft.model.update("acme", "m1", (model) => { model.cost = { input: 1 } })
      draft.model.default.set("acme", "m1")
      const def = draft.model.default.get()
      if (!def || def.modelID !== "m1") throw new Error("catalog default.get mismatch")
    })
    await ctx.reference.transform((draft) => {
      draft.add("codebase", { type: "directory", path: "/src" })
      if (draft.list().indexOf("codebase") === -1) throw new Error("reference.list missing")
    })
    await ctx.skill.transform((draft) => {
      draft.source({ name: "rust", description: "Rust helpers" })
      if (draft.list().length !== 1) throw new Error("skill.list empty")
    })
  },
})
"#;
    let mut plugin = build_plugin();
    load_code(&mut plugin, source, "v2-semantics.ts");

    let draft = plugin
        .v2_transform("agent", serde_json::json!({ "agents": [] }))
        .expect("agent transform failed");
    // a1 was removed; a2 updated and made default.
    assert_eq!(draft["agents"].as_array().unwrap().len(), 1);
    assert_eq!(draft["agents"][0]["id"], "a2");
    assert_eq!(draft["agents"][0]["temperature"], 0.2);
    assert_eq!(draft["defaultAgent"], "a2");

    let draft = plugin
        .v2_transform("catalog", serde_json::json!({}))
        .expect("catalog transform failed");
    assert_eq!(draft["providers"][0]["provider"]["id"], "acme");
    assert_eq!(draft["providers"][0]["provider"]["npm"], "@acme/provider");
    assert_eq!(draft["providers"][0]["models"]["m1"]["cost"]["input"], 1);
    assert_eq!(draft["defaultModel"]["modelID"], "m1");

    let draft = plugin
        .v2_transform("reference", serde_json::json!({}))
        .expect("reference transform failed");
    assert_eq!(draft["references"]["codebase"]["type"], "directory");

    let draft = plugin
        .v2_transform("skill", serde_json::json!({}))
        .expect("skill transform failed");
    assert_eq!(draft["sources"][0]["name"], "rust");
}

#[test]
fn v2_plugin_add_routes_through_registration_sink() {
    let sink = InMemoryRegistrationSink::default();
    let host = Arc::new(RegistrationHost { sink: sink.clone() });
    let source = r#"
import { define } from "opencode/plugin/v2/promise"
export default define({
  id: "v2-plugin-host",
  setup: async (ctx) => {
    await ctx.plugin.add({ id: "child-plugin", effect: {} })
    await ctx.plugin.remove("child-plugin")
  },
})
"#;
    let mut plugin = build_plugin_with_host(host.clone());
    load_code(&mut plugin, source, "v2-plugin.ts");

    let registrations = sink.snapshot();
    assert_eq!(registrations.len(), 2);
    assert_eq!(registrations[0].kind, "plugin");
    assert_eq!(registrations[0].input["id"], "child-plugin");
    assert_eq!(registrations[1].kind, "plugin.remove");
}

// ---------------------------------------------------------------------------
// F128/F129: default plugins — skill lifecycle + provider catalog plugins
// ---------------------------------------------------------------------------

#[test]
fn default_skill_registration_reaches_the_typed_sink() {
    use oc_plugin::default_plugins::customize_opencode_skill_registration;

    let sink = InMemoryRegistrationSink::default();
    let content = "# Customizing opencode\n\nSummary body for the built-in skill.";
    let registration = customize_opencode_skill_registration(Some("skill-plugin"), content);
    sink.register(registration.clone())
        .expect("sink accepts skill");

    let skills = sink.registrations_for("skill");
    assert_eq!(skills.len(), 1);
    assert_eq!(
        skills[0].input["name"],
        oc_plugin::default_plugins::CUSTOMIZE_OPENCODE_SKILL_NAME
    );
    assert_eq!(skills[0].input["content"], content);
    // The skill registration carries the embedded-source hook shape the server
    // projections consume (name/description/location/content).
    for key in ["name", "description", "location", "content"] {
        assert!(
            skills[0].input.get(key).is_some(),
            "skill registration is missing {key}"
        );
    }
}

#[test]
fn default_provider_plugins_match_the_v2_catalog_hook_shape() {
    use oc_plugin::default_plugins::apply_all_provider_plugins;

    // The catalog draft shaped exactly as the v2 `catalog` transform bridge
    // produces (provider items with `provider`/`models`).
    let mut draft = serde_json::json!({
        "providers": [
            {
                "provider": {
                    "id": "nvidia",
                    "api": { "type": "aisdk", "package": "@ai-sdk/openai-compatible", "url": "https://integrate.api.nvidia.com/v1" },
                    "request": { "headers": {} },
                },
                "models": { "gpt-4o": { "enabled": true } },
            },
            {
                "provider": {
                    "id": "openrouter",
                    "api": { "type": "aisdk", "package": "@openrouter/ai-sdk-provider", "url": "https://openrouter.ai/api/v1" },
                    "request": { "headers": {} },
                },
                "models": {
                    "gpt-5-chat-latest": { "enabled": true },
                    "openai/gpt-5-chat": { "enabled": true },
                    "anthropic/claude": { "enabled": true },
                },
            },
            {
                "provider": {
                    "id": "custom",
                    "api": { "type": "aisdk", "package": "@ai-sdk/openai-compatible", "url": "https://custom.test/v1" },
                    "request": { "headers": {} },
                },
                "models": { "x": { "enabled": true } },
            },
        ]
    });
    apply_all_provider_plugins(&mut draft);

    let nvidia = &draft["providers"][0]["provider"]["request"]["headers"];
    assert_eq!(nvidia["HTTP-Referer"], "https://opencode.ai/");
    assert_eq!(nvidia["X-BILLING-INVOKE-ORIGIN"], "OpenCode");

    let openrouter = &draft["providers"][1];
    assert_eq!(
        openrouter["provider"]["request"]["headers"]["X-Title"],
        "opencode"
    );
    assert_eq!(openrouter["models"]["gpt-5-chat-latest"]["enabled"], false);
    assert_eq!(openrouter["models"]["openai/gpt-5-chat"]["enabled"], false);
    assert_eq!(openrouter["models"]["anthropic/claude"]["enabled"], true);

    // Unrelated providers keep their shape.
    assert_eq!(
        draft["providers"][2]["provider"]["request"]["headers"],
        serde_json::json!({})
    );
}
