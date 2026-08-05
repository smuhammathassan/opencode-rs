// Loader pipeline tests: global + project + env merge and discovery.
// Mirrors the behaviors in the reference `config-service.test.ts` /
// `config/config.test.ts`.

mod common;

use common::TestHome;
use oc_config::load::{load_instance_state, LoadOptions};
use oc_config::v1::config::{Compaction, Share};
use serde_json::json;

#[test]
fn loads_with_defaults_and_seeds_global_config() {
    let home = TestHome::new();
    let state = home.load();
    assert_eq!(state.config.username.as_deref(), Some("testuser"));
    // Global config file is seeded with the schema.
    let seeded =
        std::fs::read_to_string(home.global_config.join("opencode.jsonc")).expect("seeded file");
    assert!(seeded.contains("$schema"));
    // Defaults: agent/mode/plugin/command are always present.
    assert_eq!(state.config.agent, Some(Default::default()));
    assert_eq!(state.config.mode, Some(Default::default()));
    assert_eq!(state.config.plugin, Some(vec![]));
    assert_eq!(state.config.command, Some(Default::default()));
}

#[test]
fn global_config_is_loaded() {
    let home = TestHome::new();
    home.write_global("opencode.json", json!({ "model": "global/model" }));
    let state = home.load();
    assert_eq!(state.config.model.as_deref(), Some("global/model"));
}

#[test]
fn project_overrides_global() {
    let home = TestHome::new();
    home.write_global(
        "opencode.json",
        json!({ "model": "global/model", "shell": "bash" }),
    );
    home.write_project(json!({ "model": "project/model" }), "opencode.json");
    let state = home.load();
    assert_eq!(state.config.model.as_deref(), Some("project/model"));
    assert_eq!(state.config.shell.as_deref(), Some("bash"));
}

#[test]
fn jsonc_overrides_json_in_same_directory() {
    let home = TestHome::new();
    home.write_project(json!({ "model": "from-json" }), "opencode.json");
    home.write_project(
        json!({ "model": "from-jsonc", "username": "jsonc-user" }),
        "opencode.jsonc",
    );
    let state = home.load();
    assert_eq!(state.config.model.as_deref(), Some("from-jsonc"));
    assert_eq!(state.config.username.as_deref(), Some("jsonc-user"));
}

#[test]
fn local_opencode_overrides_project() {
    let home = TestHome::new();
    home.write_project(
        json!({ "model": "project/model", "mcp": { "docs": { "type": "remote", "url": "https://x", "enabled": false } } }),
        "opencode.json",
    );
    home.write_in_project(
        ".opencode/opencode.json",
        r#"{"$schema": "https://opencode.ai/config.json", "mcp": { "docs": { "type": "remote", "url": "https://x", "enabled": true } }}"#,
    );
    let state = home.load();
    let mcp = serde_json::to_value(&state.config.mcp).expect("serialize");
    assert_eq!(mcp["docs"]["enabled"], true);
    assert_eq!(mcp["docs"]["type"], "remote");
}

#[test]
fn mcp_deep_merges_preserving_base_properties() {
    let home = TestHome::new();
    home.write_project(
        json!({
            "mcp": {
                "myserver": {
                    "type": "remote",
                    "url": "https://myserver.example.com/mcp",
                    "enabled": false,
                    "headers": { "X-Custom-Header": "value" }
                }
            }
        }),
        "opencode.json",
    );
    home.write_project(
        json!({
            "mcp": {
                "myserver": { "type": "remote", "url": "https://myserver.example.com/mcp", "enabled": true }
            }
        }),
        "opencode.jsonc",
    );
    let state = home.load();
    let mcp = serde_json::to_value(&state.config.mcp).expect("serialize");
    assert_eq!(mcp["myserver"]["enabled"], true);
    assert_eq!(mcp["myserver"]["headers"]["X-Custom-Header"], "value");
}

#[test]
fn instructions_are_concatenated_and_deduplicated() {
    let home = TestHome::new();
    home.write_global(
        "opencode.json",
        json!({ "instructions": ["duplicate.md", "global-only.md"] }),
    );
    home.write_project(
        json!({ "instructions": ["duplicate.md", "local-only.md"] }),
        "opencode.json",
    );
    let state = home.load();
    assert_eq!(
        state.config.instructions.as_deref(),
        Some(
            &[
                "duplicate.md".to_string(),
                "global-only.md".to_string(),
                "local-only.md".to_string()
            ][..]
        )
    );
}

#[test]
fn plugins_merge_and_dedupe_keeping_last() {
    let home = TestHome::new();
    home.write_global(
        "opencode.json",
        json!({ "plugin": ["duplicate-plugin@1.0.0", "global-plugin-1@1.0.0"] }),
    );
    home.write_project(
        json!({ "plugin": ["duplicate-plugin@2.0.0", "local-plugin-1@1.0.0"] }),
        "opencode.json",
    );
    let state = home.load();
    let plugins = state.config.plugin.expect("plugin");
    let names: Vec<String> = plugins
        .iter()
        .map(|p| oc_config::load::plugin_specifier(p).to_string())
        .collect();
    assert!(names.contains(&"global-plugin-1@1.0.0".to_string()));
    assert!(names.contains(&"local-plugin-1@1.0.0".to_string()));
    assert!(names.contains(&"duplicate-plugin@2.0.0".to_string()));
    assert!(!names.contains(&"duplicate-plugin@1.0.0".to_string()));
    let dupes = names
        .iter()
        .filter(|n| n.starts_with("duplicate-plugin"))
        .count();
    assert_eq!(dupes, 1);
}

#[test]
fn discovers_agents_from_opencode_directory() {
    let home = TestHome::new();
    home.write_in_project(
        ".opencode/agent/test.md",
        "---\nmodel: test/model\n---\nTest agent prompt",
    );
    home.write_in_project(
        ".opencode/agents/nested/child.md",
        "---\nmodel: test/model\nmode: subagent\n---\nNested agent prompt",
    );
    let state = home.load();
    let agents = state.config.agent.expect("agent");
    let test = agents.get("test").expect("agent test");
    assert_eq!(test.model.as_deref(), Some("test/model"));
    assert_eq!(test.prompt.as_deref(), Some("Test agent prompt"));
    assert_eq!(test.rest.get("name"), Some(&json!("test")));
    let child = agents.get("nested/child").expect("agent child");
    assert_eq!(child.prompt.as_deref(), Some("Nested agent prompt"));
}

#[test]
fn agent_markdown_permission_preserves_key_order() {
    let home = TestHome::new();
    home.write_in_project(
        ".opencode/agent/ordered.md",
        "---\npermission:\n  bash: allow\n  \"*\": deny\n  edit: ask\n---\nOrdered permissions",
    );
    let state = home.load();
    let permission = state
        .config
        .agent
        .as_ref()
        .and_then(|a| a.get("ordered"))
        .map(|a| a.permission.clone())
        .expect("permission");
    let keys: Vec<String> = permission.entries().map(|(k, _)| k.clone()).collect();
    assert_eq!(keys, ["bash", "*", "edit"]);
}

#[test]
fn discovers_commands_from_opencode_directory() {
    let home = TestHome::new();
    home.write_in_project(
        ".opencode/command/hello.md",
        "---\ndescription: Test command\n---\nHello from singular command",
    );
    home.write_in_project(
        ".opencode/commands/nested/child.md",
        "---\ndescription: Nested command\n---\nNested command template",
    );
    let state = home.load();
    let commands = state.config.command.expect("command");
    assert_eq!(
        commands.get("hello").expect("hello").template,
        "Hello from singular command"
    );
    assert_eq!(
        commands.get("hello").expect("hello").description.as_deref(),
        Some("Test command")
    );
    assert_eq!(
        commands.get("nested/child").expect("nested").template,
        "Nested command template"
    );
}

#[test]
fn discovers_local_plugins_as_file_urls() {
    let home = TestHome::new();
    home.write_in_project(".opencode/plugin/my-plugin.js", "export default {}");
    let state = home.load();
    let plugins = state.config.plugin.expect("plugin");
    assert!(plugins
        .iter()
        .any(|p| oc_config::load::plugin_specifier(p).starts_with("file://")));
    assert!(state
        .plugin_origins
        .iter()
        .any(|o| o.specifier().ends_with("my-plugin.js")));
}

#[test]
fn mode_field_migrates_to_agent_with_primary_mode() {
    let home = TestHome::new();
    home.write_project(
        json!({ "mode": { "test_mode": { "model": "test/model", "temperature": 0.5 } } }),
        "opencode.json",
    );
    let state = home.load();
    let agent = state
        .config
        .agent
        .as_ref()
        .and_then(|a| a.get("test_mode"))
        .expect("agent");
    assert_eq!(agent.model.as_deref(), Some("test/model"));
    assert_eq!(agent.mode, Some(oc_config::v1::agent::Mode::Primary));
    assert_eq!(agent.temperature, Some(0.5));
}

#[test]
fn autoshare_migrates_to_share() {
    let home = TestHome::new();
    home.write_project(json!({ "autoshare": true }), "opencode.json");
    let state = home.load();
    assert_eq!(state.config.share, Some(Share::Auto));
    assert_eq!(state.config.autoshare, Some(true));
}

#[test]
fn legacy_tui_keys_are_stripped() {
    let home = TestHome::new();
    home.write_project(
        json!({ "model": "test/model", "theme": "legacy", "tui": { "scroll_speed": 4 } }),
        "opencode.json",
    );
    let state = home.load();
    assert_eq!(state.config.model.as_deref(), Some("test/model"));
    let serialized = serde_json::to_value(&state.config).expect("serialize");
    assert!(serialized.get("theme").is_none());
    assert!(serialized.get("tui").is_none());
}

#[test]
fn environment_variable_substitution() {
    let home = TestHome::new();
    let _guard = common::EnvGuard::set("TEST_CONFIG_VAR", "secret-value");
    home.write_project(
        json!({ "username": "{env:TEST_CONFIG_VAR}" }),
        "opencode.json",
    );
    let state = home.load();
    assert_eq!(state.config.username.as_deref(), Some("secret-value"));
}

#[test]
fn file_inclusion_substitution() {
    let home = TestHome::new();
    home.write_in_project("included.txt", "test-user");
    home.write_project(
        json!({ "username": "{file:included.txt}" }),
        "opencode.json",
    );
    let state = home.load();
    assert_eq!(state.config.username.as_deref(), Some("test-user"));
}

#[test]
fn schema_added_to_project_config_preserves_env_var() {
    let home = TestHome::new();
    let _guard = common::EnvGuard::set("PRESERVE_VAR", "secret_value");
    home.write_project(json!({ "username": "{env:PRESERVE_VAR}" }), "opencode.json");
    let state = home.load();
    assert_eq!(state.config.username.as_deref(), Some("secret_value"));
    let content = std::fs::read_to_string(home.project.join("opencode.json")).expect("read");
    assert!(content.contains("{env:PRESERVE_VAR}"));
    assert!(!content.contains("secret_value"));
    assert!(content.contains("$schema"));
}

#[test]
fn tools_field_migrates_to_permission() {
    let home = TestHome::new();
    home.write_project(
        json!({ "tools": { "bash": true, "webfetch": false, "write": true } }),
        "opencode.json",
    );
    let state = home.load();
    let permission = state.config.permission.as_ref().expect("permission");
    assert_eq!(
        permission.get("bash"),
        Some(&oc_config::v1::Rule::Action(oc_config::v1::Action::Allow))
    );
    assert_eq!(
        permission.get("edit"),
        Some(&oc_config::v1::Rule::Action(oc_config::v1::Action::Allow))
    );
    assert_eq!(
        permission.get("webfetch"),
        Some(&oc_config::v1::Rule::Action(oc_config::v1::Action::Deny))
    );
}

#[test]
fn disable_project_config_skips_project_files() {
    let home = TestHome::new();
    home.write_project(json!({ "model": "project/model" }), "opencode.json");
    let _guard = common::EnvGuard::set("OPENCODE_DISABLE_PROJECT_CONFIG", "true");
    let state = home.load();
    assert!(state.config.model.is_none());
}

#[test]
fn config_content_flag() {
    let home = TestHome::new();
    home.write_project(json!({ "model": "project/model" }), "opencode.json");
    let _guard = common::EnvGuard::set(
        "OPENCODE_CONFIG_CONTENT",
        r#"{ "$schema": "https://opencode.ai/config.json", "username": "{env:TEST_CONFIG_VAR}" }"#,
    );
    let _env = common::EnvGuard::set("TEST_CONFIG_VAR", "content-user");
    let state = home.load();
    assert_eq!(state.config.username.as_deref(), Some("content-user"));
}

#[test]
fn invalid_permission_flag_is_ignored() {
    let home = TestHome::new();
    home.write_project(json!({ "model": "project/model" }), "opencode.json");
    let _guard = common::EnvGuard::set("OPENCODE_PERMISSION", "{invalid");
    let state = home.load();
    assert_eq!(state.config.model.as_deref(), Some("project/model"));
}

#[test]
fn permission_flag_overrides() {
    let home = TestHome::new();
    let _guard = common::EnvGuard::set("OPENCODE_PERMISSION", r#"{"bash": "ask"}"#);
    let state = home.load();
    assert_eq!(
        state.config.permission.as_ref().and_then(|p| p.get("bash")),
        Some(&oc_config::v1::Rule::Action(oc_config::v1::Action::Ask))
    );
}

#[test]
fn disable_autocompact_and_prune_flags() {
    let home = TestHome::new();
    let _guard = common::EnvGuard::set_all(&[
        ("OPENCODE_DISABLE_AUTOCOMPACT", Some("true")),
        ("OPENCODE_DISABLE_PRUNE", Some("true")),
    ]);
    let state = home.load();
    let compaction = state.config.compaction.as_ref().expect("compaction");
    assert_eq!(compaction.auto, Some(false));
    assert_eq!(compaction.prune, Some(false));
}

#[test]
fn username_comes_from_config_when_set() {
    let home = TestHome::new();
    home.write_project(json!({ "username": "custom-user" }), "opencode.json");
    let state = home.load();
    assert_eq!(state.config.username.as_deref(), Some("custom-user"));
}

#[test]
fn managed_config_overrides_user_config() {
    let home = TestHome::new();
    home.write_project(
        json!({ "model": "user/model", "share": "auto" }),
        "opencode.json",
    );
    std::fs::write(
        home.managed.join("opencode.json"),
        r#"{"$schema": "https://opencode.ai/config.json", "model": "managed/model", "share": "disabled"}"#,
    )
    .expect("write managed");
    let state = home.load();
    assert_eq!(state.config.model.as_deref(), Some("managed/model"));
    assert_eq!(state.config.share, Some(Share::Disabled));
}

#[test]
fn compaction_defaults_preserved() {
    let home = TestHome::new();
    home.write_project(json!({ "compaction": { "auto": true } }), "opencode.json");
    let state = home.load();
    let compaction: Compaction = serde_json::from_value(json!({ "auto": true })).expect("parse");
    assert_eq!(state.config.compaction.as_ref(), Some(&compaction));
}

#[test]
fn load_instance_state_directories_include_opencode_dirs() {
    let home = TestHome::new();
    let state = home.load();
    assert!(state
        .directories
        .contains(&home.global_config.to_string_lossy().into_owned()));
}

#[test]
fn legacy_toml_config_is_migrated() {
    let home = TestHome::new();
    std::fs::write(
        home.global_config.join("config"),
        "provider = \"anthropic\"\nmodel = \"claude-sonnet-4\"\nshell = \"zsh\"\n",
    )
    .expect("write legacy toml");
    let state = home.load();
    assert_eq!(
        state.config.model.as_deref(),
        Some("anthropic/claude-sonnet-4")
    );
    assert_eq!(state.config.shell.as_deref(), Some("zsh"));
    // The legacy file is migrated away and config.json is written.
    assert!(!home.global_config.join("config").exists());
    let migrated =
        std::fs::read_to_string(home.global_config.join("config.json")).expect("config.json");
    assert!(migrated.contains("anthropic/claude-sonnet-4"));
}
