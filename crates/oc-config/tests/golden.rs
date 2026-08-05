// Golden tests: parse opencode.json fixtures and assert the parsed struct plus
// the re-serialized JSON match the reference (`ConfigV1.Info`) output.
//
// Field order inside agent objects matches the reference when input keys are
// written in struct declaration order (options/permission append at the end),
// mirroring the reference's `{ ...agent, options, permission }` normalize.

mod common;

use oc_config::parse::schema;
use oc_config::v1::permission;
use serde_json::{json, Value};

fn parse(text: &str) -> Value {
    let value = oc_config::parse::jsonc(text, "test:config").expect("jsonc parse");
    let config = schema(value, "test:config").expect("schema");
    serde_json::to_value(&config).expect("serialize")
}

#[test]
fn golden_full_config() {
    let input = r##"{
      "$schema": "https://opencode.ai/config.json",
      "shell": "bash",
      "logLevel": "DEBUG",
      "model": "anthropic/claude-sonnet-4",
      "small_model": "openai/gpt-4o-mini",
      "default_agent": "build",
      "subagent_depth": 2,
      "username": "testuser",
      "share": "auto",
      "autoupdate": "notify",
      "disabled_providers": ["openai"],
      "enabled_providers": ["anthropic", "google"],
      "snapshot": true,
      "instructions": ["AGENTS.md"],
      "server": { "port": 4567, "hostname": "127.0.0.1" },
      "command": {
        "test": { "template": "Run the tests", "description": "Run tests", "subtask": true }
      },
      "skills": { "paths": ["skills"] },
      "references": {
        "docs": { "repository": "github.com/example/docs", "branch": "main", "description": "Docs" },
        "local": { "path": "../lib" },
        "short": "github.com/example/short"
      },
      "agent": {
        "build": {
          "model": "anthropic/claude-opus-4",
          "temperature": 0.7,
          "description": "Primary agent",
          "color": "#FFA500",
          "permission": { "edit": "allow", "webfetch": "ask" }
        },
        "plan": {
          "model": "anthropic/claude-sonnet-4",
          "tools": { "bash": true, "webfetch": false },
          "description": "Plan mode"
        }
      },
      "provider": {
        "anthropic": {
          "name": "Anthropic",
          "api": "anthropic",
          "env": ["ANTHROPIC_API_KEY"],
          "options": { "apiKey": "{env:ANTHROPIC_API_KEY}", "timeout": 60000 },
          "models": {
            "claude-opus-4": {
              "cost": { "input": 15, "output": 75, "cache_read": 1.5 },
              "limit": { "context": 200000, "output": 8192 },
              "status": "active"
            }
          }
        }
      },
      "mcp": {
        "local": { "type": "local", "command": ["node", "server.js"], "enabled": true },
        "remote": { "type": "remote", "url": "https://example.com/mcp", "enabled": false },
        "disabled": { "enabled": true }
      },
      "formatter": true,
      "lsp": true,
      "layout": "stretch",
      "permission": { "bash": "allow", "*": "deny", "edit": "ask" },
      "attachment": { "image": { "auto_resize": true, "max_width": 1024 } },
      "enterprise": { "url": "https://enterprise.example.com" },
      "tool_output": { "max_lines": 5000, "max_bytes": 100000 },
      "compaction": { "auto": true, "prune": true, "tail_turns": 2, "reserved": 1000 },
      "experimental": {
        "batch_tool": true,
        "openTelemetry": true,
        "primary_tools": ["websearch"],
        "policies": [{ "action": "provider.use", "effect": "deny", "resource": "openai" }]
      }
    }"##;

    let parsed = parse(input);

    let expected = json!({
        "$schema": "https://opencode.ai/config.json",
        "shell": "bash",
        "logLevel": "DEBUG",
        "model": "anthropic/claude-sonnet-4",
        "small_model": "openai/gpt-4o-mini",
        "default_agent": "build",
        "subagent_depth": 2,
        "username": "testuser",
        "share": "auto",
        "autoupdate": "notify",
        "disabled_providers": ["openai"],
        "enabled_providers": ["anthropic", "google"],
        "snapshot": true,
        "instructions": ["AGENTS.md"],
        "server": { "port": 4567, "hostname": "127.0.0.1" },
        "command": {
            "test": { "template": "Run the tests", "description": "Run tests", "subtask": true }
        },
        "skills": { "paths": ["skills"] },
        "references": {
            "docs": { "repository": "github.com/example/docs", "branch": "main", "description": "Docs" },
            "local": { "path": "../lib" },
            "short": "github.com/example/short"
        },
        "agent": {
            "build": {
                "model": "anthropic/claude-opus-4",
                "temperature": 0.7,
                "description": "Primary agent",
                "color": "#FFA500",
                "options": {},
                "permission": { "edit": "allow", "webfetch": "ask" }
            },
            "plan": {
                "model": "anthropic/claude-sonnet-4",
                "tools": { "bash": true, "webfetch": false },
                "description": "Plan mode",
                "options": {},
                "permission": { "bash": "allow", "webfetch": "deny" }
            }
        },
        "provider": {
            "anthropic": {
                "name": "Anthropic",
                "api": "anthropic",
                "env": ["ANTHROPIC_API_KEY"],
                "options": { "apiKey": "{env:ANTHROPIC_API_KEY}", "timeout": 60000 },
                "models": {
                    "claude-opus-4": {
                        "cost": { "input": 15, "output": 75, "cache_read": 1.5 },
                        "limit": { "context": 200000, "output": 8192 },
                        "status": "active"
                    }
                }
            }
        },
        "mcp": {
            "local": { "type": "local", "command": ["node", "server.js"], "enabled": true },
            "remote": { "type": "remote", "url": "https://example.com/mcp", "enabled": false },
            "disabled": { "enabled": true }
        },
        "formatter": true,
        "lsp": true,
        "layout": "stretch",
        "permission": { "bash": "allow", "*": "deny", "edit": "ask" },
        "attachment": { "image": { "auto_resize": true, "max_width": 1024 } },
        "enterprise": { "url": "https://enterprise.example.com" },
        "tool_output": { "max_lines": 5000, "max_bytes": 100000 },
        "compaction": { "auto": true, "prune": true, "tail_turns": 2, "reserved": 1000 },
        "experimental": {
            "batch_tool": true,
            "openTelemetry": true,
            "primary_tools": ["websearch"],
            "policies": [{ "action": "provider.use", "effect": "deny", "resource": "openai" }]
        }
    });

    assert_eq!(parsed, expected);
}

#[test]
fn golden_exact_serialization() {
    let input = r#"{
      "$schema": "https://opencode.ai/config.json",
      "model": "anthropic/claude-sonnet-4",
      "agent": {
        "build": { "model": "anthropic/claude-opus-4", "temperature": 0.7 }
      },
      "permission": "deny"
    }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("parse");
    let config = schema(value, "test").expect("schema");
    let serialized = serde_json::to_string(&config).expect("serialize");
    assert_eq!(
        serialized,
        r#"{"$schema":"https://opencode.ai/config.json","model":"anthropic/claude-sonnet-4","agent":{"build":{"model":"anthropic/claude-opus-4","temperature":0.7,"options":{},"permission":{}}},"permission":{"*":"deny"}}"#
    );
}

#[test]
fn jsonc_comments_and_trailing_commas() {
    let input = r#"{
        // leading comment
        "$schema": "https://opencode.ai/config.json",
        "model": "test/model", // trailing comment
        "username": "testuser",
        "agent": {
            "test": { "model": "test/model" },  // trailing comma
        },
    }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    assert_eq!(config.model.as_deref(), Some("test/model"));
    assert_eq!(config.username.as_deref(), Some("testuser"));
}

#[test]
fn unknown_top_level_keys_error() {
    let input = r#"{ "invalid_field": true, "model": "x" }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let error = schema(value, "test").expect_err("should error");
    match error {
        oc_config::ConfigError::Invalid { issues, path, .. } => {
            assert_eq!(path, "test");
            assert_eq!(issues.len(), 1);
            assert_eq!(issues[0].code.as_deref(), Some("unrecognized_keys"));
            assert_eq!(
                issues[0].keys.as_deref(),
                Some(&["invalid_field".to_string()][..])
            );
            assert!(issues[0].path.is_empty());
            assert_eq!(issues[0].message, "Unrecognized key: invalid_field");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn permission_scalar_normalizes_to_star() {
    let input = r#"{ "permission": "deny" }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    let permission = config.permission.as_ref().expect("permission");
    assert_eq!(
        permission.get("*"),
        Some(&permission::Rule::Action(permission::Action::Deny))
    );
    let serialized = serde_json::to_value(&config).expect("serialize");
    assert_eq!(serialized["permission"], json!({ "*": "deny" }));
}

#[test]
fn permission_preserves_key_order() {
    let input = r#"{
      "permission": {
        "bash": "allow",
        "*": "deny",
        "edit": "ask",
        "read": "allow",
        "todowrite": "allow",
        "thoughts_*": "allow"
      }
    }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    let keys: Vec<&str> = config
        .permission
        .as_ref()
        .expect("permission")
        .entries()
        .map(|(key, _)| key.as_str())
        .collect();
    assert_eq!(
        keys,
        ["bash", "*", "edit", "read", "todowrite", "thoughts_*"]
    );
    // Nested bash rules with patterns.
    let input2 = r#"{
      "permission": { "bash": { "*": "ask", "rm -rf *": "deny", "curl *": "deny" } }
    }"#;
    let value = oc_config::parse::jsonc(input2, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    match config.permission.as_ref().expect("permission").get("bash") {
        Some(permission::Rule::Object(map)) => {
            assert_eq!(map.get("rm -rf *"), Some(&permission::Action::Deny));
            assert_eq!(map.get("curl *"), Some(&permission::Action::Deny));
            assert_eq!(map.get("*"), Some(&permission::Action::Ask));
        }
        other => panic!("expected object rule, got {other:?}"),
    }
}

#[test]
fn agent_unknown_keys_move_to_options() {
    let input = r#"{
      "agent": {
        "test_agent": { "model": "openai/gpt-5.2", "variant": "xhigh", "max_tokens": 123 }
      }
    }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    let agent = config
        .agent
        .as_ref()
        .expect("agent")
        .get("test_agent")
        .expect("agent");
    assert_eq!(agent.variant.as_deref(), Some("xhigh"));
    assert_eq!(agent.options.get("max_tokens"), Some(&json!(123)));
    assert!(!agent.options.contains_key("variant"));
    assert_eq!(agent.rest.get("max_tokens"), Some(&json!(123)));
}

#[test]
fn agent_tools_migrate_to_permission() {
    let input = r#"{
      "agent": {
        "test": { "tools": { "bash": true, "write": true, "read": false, "webfetch": true } }
      }
    }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    let permission = config
        .agent
        .as_ref()
        .expect("agent")
        .get("test")
        .expect("agent")
        .permission
        .clone();
    let keys: Vec<&str> = permission.entries().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["bash", "edit", "read", "webfetch"]);
    assert_eq!(
        permission.get("bash"),
        Some(&permission::Rule::Action(permission::Action::Allow))
    );
    assert_eq!(
        permission.get("edit"),
        Some(&permission::Rule::Action(permission::Action::Allow))
    );
    assert_eq!(
        permission.get("read"),
        Some(&permission::Rule::Action(permission::Action::Deny))
    );
}

#[test]
fn agent_steps_falls_back_to_max_steps() {
    let input = r#"{
      "agent": {
        "test": { "model": "x/y", "maxSteps": 5 }
      }
    }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    let agent = config
        .agent
        .as_ref()
        .expect("agent")
        .get("test")
        .expect("agent");
    assert_eq!(agent.steps.map(|s| s.get()), Some(5));
    assert_eq!(agent.max_steps.map(|s| s.get()), Some(5));
}

#[test]
fn rejects_unknown_top_level_permission_action() {
    let input = r#"{ "permission": { "bash": "sometimes" } }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    assert!(schema(value, "test").is_err());
}

#[test]
fn rejects_invalid_agent_color() {
    let input = r##"{ "agent": { "build": { "color": "#12345" } } }"##;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    assert!(schema(value, "test").is_err());
}

#[test]
fn accepts_theme_color() {
    let input = r#"{ "agent": { "build": { "color": "primary" } } }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    assert_eq!(
        config
            .agent
            .as_ref()
            .and_then(|a| a.get("build"))
            .and_then(|a| a.color.as_deref()),
        Some("primary")
    );
}

#[test]
fn lsp_requires_extensions_for_custom_servers() {
    let input = r#"{
      "lsp": {
        "my-server": { "command": ["my-lsp"] }
      }
    }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let error = schema(value, "test").expect_err("should error");
    match error {
        oc_config::ConfigError::Invalid { issues, .. } => {
            assert!(issues[0].message.contains("extensions"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn lsp_accepts_builtin_without_extensions() {
    let input = r#"{
      "lsp": {
        "typescript": { "command": ["typescript-language-server", "--stdio"] }
      }
    }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    assert!(schema(value, "test").is_ok());
}

#[test]
fn invalid_jsonc_errors() {
    let input = "{ invalid json }";
    let error = oc_config::parse::jsonc(input, "test").expect_err("should error");
    match error {
        oc_config::ConfigError::Json { path, .. } => assert_eq!(path, "test"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn config_parse_type_accessors() {
    let input = r#"{
      "agent": { "build": { "model": "x/y" } },
      "command": { "hello": { "template": "Hi" } }
    }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    assert!(config.agents().contains_key("build"));
    assert!(config.commands().contains_key("hello"));
    assert!(config.agents().get("build").is_some());
}

#[test]
fn mode_and_agent_share_agent_schema() {
    // `mode` is deprecated but parses like `agent`.
    let input = r#"{ "mode": { "test_mode": { "model": "test/model", "temperature": 0.5 } } }"#;
    let value = oc_config::parse::jsonc(input, "test").expect("jsonc parse");
    let config = schema(value, "test").expect("schema");
    assert!(config
        .mode
        .as_ref()
        .expect("mode")
        .contains_key("test_mode"));
}

#[test]
fn golden_v2_types_round_trip() {
    use oc_config::v2::{experimental, mcp, plugin, provider, reference};

    let experimental = serde_json::json!({
        "policies": [{ "action": "provider.use", "effect": "deny", "resource": "openai" }]
    });
    let parsed: experimental::Experimental = serde_json::from_value(experimental).expect("parse");
    assert_eq!(
        parsed.policies.as_ref().expect("policies")[0].action,
        experimental::PolicyAction::ProviderUse
    );
    let out = serde_json::to_value(&parsed).expect("serialize");
    assert_eq!(out["policies"][0]["action"], "provider.use");

    let local = serde_json::json!({
        "type": "local",
        "command": ["node", "server.js"],
        "environment": { "KEY": "value" }
    });
    let parsed: mcp::Server = serde_json::from_value(local).expect("parse local mcp");
    assert!(matches!(parsed, mcp::Server::Local(_)));
    let out = serde_json::to_value(&parsed).expect("serialize");
    assert_eq!(out["type"], "local");
    assert_eq!(out["environment"]["KEY"], "value");

    let plugin_value = serde_json::json!([["@scope/plugin", { "apiKey": "x" }], "plain-plugin"]);
    let parsed: Vec<plugin::Plugin> = serde_json::from_value(plugin_value).expect("parse plugin");
    assert!(matches!(&parsed[0], plugin::Plugin::Entry(e) if e.package == "@scope/plugin"));
    assert!(matches!(&parsed[1], plugin::Plugin::Package(p) if p == "plain-plugin"));

    let model = serde_json::json!({
        "family": "claude",
        "api": { "type": "native", "settings": { "baseURL": "https://x" } },
        "cost": { "input": 0.3, "output": 0.6 },
        "limit": { "context": 200000 }
    });
    let parsed: provider::Model = serde_json::from_value(model).expect("parse model");
    let out = serde_json::to_value(&parsed).expect("serialize");
    assert_eq!(out["cost"]["input"], 0.3);
    assert_eq!(out["limit"]["context"], 200000);

    let references = serde_json::json!({
        "local": { "path": "../library" },
        "sdk": { "repository": "github.com/example/sdk", "branch": "main" },
        "shorthand": "github.com/example/docs"
    });
    let parsed: reference::Info = serde_json::from_value(references).expect("parse references");
    assert!(parsed["sdk"].is_git());
    assert!(parsed["local"].is_local());
    assert!(parsed["shorthand"].is_url());
}
