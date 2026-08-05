//! End-to-end plugin host tests using the reference example plugins
//! (reference/packages/plugin/src/example.ts and example-workspace.ts).

use std::sync::Arc;

use oc_plugin::js::transpile::transpile_module;
use oc_plugin::{ModuleResolver, NoopHost, PluginBuilder, PluginHost};

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
    let host: Arc<dyn PluginHost> = Arc::new(NoopHost);
    let resolver = make_resolver();
    let builder = PluginBuilder::new(host, resolver);
    builder.build().expect("failed to build plugin context")
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
