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
      draft.added = true
      draft.agent = "primary"
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
        .v2_transform("agent", serde_json::json!({ "id": "build" }))
        .expect("v2 transform failed");
    assert_eq!(draft["added"], true);
    assert_eq!(draft["agent"], "primary");

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
