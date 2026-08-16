// Edge-case tests: markdown frontmatter, variable substitution, plugin spec
// resolution, and the full-load serialization golden.

mod common;

use common::TestHome;
use oc_config::load::{load_instance_state, resolve_plugin_spec, LoadOptions};
use oc_config::v1::plugin::Spec;
use serde_json::{json, Value};

#[test]
fn markdown_parses_frontmatter_and_body() {
    let (data, body) = oc_config::v2::markdown::parse(
        "---\nmodel: test/model\ndescription: Test agent\nmode: subagent\n---\nAgent prompt",
    )
    .expect("parse");
    assert_eq!(data.get("model"), Some(&json!("test/model")));
    assert_eq!(data.get("mode"), Some(&json!("subagent")));
    assert_eq!(body, "Agent prompt");
}

#[test]
fn markdown_parses_lists_and_nested_maps() {
    let (data, _) = oc_config::v2::markdown::parse(
        "---\ntools:\n  - read\n  - bash\npermission:\n  edit: allow\n  \"*\": deny\nsteps: 3\n---\nbody",
    )
    .expect("parse");
    assert_eq!(data.get("tools"), Some(&json!(["read", "bash"])));
    assert_eq!(data.get("steps"), Some(&json!(3)));
    match data.get("permission") {
        Some(Value::Object(map)) => {
            assert_eq!(map.get("edit"), Some(&json!("allow")));
            assert_eq!(map.get("*"), Some(&json!("deny")));
        }
        other => panic!("expected permission object, got {other:?}"),
    }
}

#[test]
fn markdown_sanitize_handles_unquoted_colons() {
    // Reference `sanitize`: values with colons become YAML block scalars.
    let (data, _) = oc_config::v2::markdown::parse(
        "---\nsystem: You are helpful with a focus: on quality\n---\nbody",
    )
    .expect("parse");
    assert_eq!(
        data.get("system"),
        Some(&json!("You are helpful with a focus: on quality"))
    );
}

#[test]
fn markdown_returns_none_for_unparsable_frontmatter() {
    assert!(oc_config::v2::markdown::parse("no frontmatter here").is_none());
    assert!(oc_config::v2::markdown::parse("---\nmodel: [unclosed\n---\nbody").is_none());
}

#[test]
fn variable_substitution_skips_commented_lines() {
    let tmp = tempfile::tempdir().expect("temp");
    let file = tmp.path().join("included.txt");
    std::fs::write(&file, "secret-value").expect("write");
    let file_str = file.to_string_lossy().into_owned();
    let source = oc_config::variable::Source::Path {
        path: "/tmp/opencode/config.json".to_string(),
    };
    // `{file:...}` on a `//` line is kept verbatim (the token resolves relative
    // to a dir that would not contain the file).
    let out = oc_config::variable::substitute(
        &format!("{{\n// {{file:{file_str}}}\n\"key\": \"{{file:{file_str}}}\"\n}}"),
        &source,
        None,
        oc_config::variable::Missing::Error,
    )
    .expect("substitute");
    assert_eq!(
        out,
        format!("{{\n// {{file:{file_str}}}\n\"key\": \"secret-value\"\n}}")
    );
}

#[test]
fn variable_missing_file_errors() {
    let source = oc_config::variable::Source::Path {
        path: "/tmp/opencode/config.json".to_string(),
    };
    let error = oc_config::variable::substitute(
        "{file:does-not-exist.txt}",
        &source,
        None,
        oc_config::variable::Missing::Error,
    )
    .expect_err("should error");
    assert!(error.to_string().contains("bad file reference"));
}

#[test]
fn variable_missing_file_empty_mode() {
    let source = oc_config::variable::Source::Path {
        path: "/tmp/opencode/config.json".to_string(),
    };
    let out = oc_config::variable::substitute(
        "{file:does-not-exist.txt}",
        &source,
        None,
        oc_config::variable::Missing::Empty,
    )
    .expect("substitute");
    assert_eq!(out, "");
}

#[test]
fn resolve_plugin_spec_keeps_package_specs() {
    let file = "/tmp/opencode.json";
    assert_eq!(
        resolve_plugin_spec(Spec::Package("oh-my-opencode@2.4.3".into()), file),
        Spec::Package("oh-my-opencode@2.4.3".into())
    );
    assert_eq!(
        resolve_plugin_spec(Spec::Package("@scope/pkg".into()), file),
        Spec::Package("@scope/pkg".into())
    );
}

#[test]
fn resolve_plugin_spec_resolves_relative_files() {
    let home = TestHome::new();
    home.write_in_project("plugin.ts", "export default {}");
    let config_file = home.project.join("opencode.json");
    let resolved = resolve_plugin_spec(
        Spec::Package("./plugin.ts".into()),
        &config_file.to_string_lossy(),
    );
    let url = oc_config::load::plugin_specifier(&resolved);
    assert!(url.starts_with("file://"));
    assert!(url.ends_with("/plugin.ts"), "got {url}");
}

#[test]
fn resolve_plugin_spec_resolves_directories() {
    let home = TestHome::new();
    // With package.json: resolves to the directory URL.
    home.write_in_project(
        "plugin/package.json",
        r#"{"name":"demo-plugin","type":"module","main":"./index.ts"}"#,
    );
    home.write_in_project("plugin/index.ts", "export default {}");
    let config_file = home.project.join("opencode.json");
    let resolved = resolve_plugin_spec(
        Spec::Package("./plugin".into()),
        &config_file.to_string_lossy(),
    );
    let url = oc_config::load::plugin_specifier(&resolved);
    assert!(url.ends_with("/plugin"), "got {url}");

    // Without package.json: resolves to index.ts.
    home.write_in_project("bare/index.ts", "export default {}");
    let resolved = resolve_plugin_spec(
        Spec::Package("./bare".into()),
        &config_file.to_string_lossy(),
    );
    let url = oc_config::load::plugin_specifier(&resolved);
    assert!(url.ends_with("/bare/index.ts"), "got {url}");
}

#[test]
fn resolve_plugin_spec_preserves_options() {
    let home = TestHome::new();
    home.write_in_project("plugin.ts", "export default {}");
    let config_file = home.project.join("opencode.json");
    let spec = Spec::Entry((
        "./plugin.ts".to_string(),
        [("apiKey".to_string(), json!("x"))].into_iter().collect(),
    ));
    let resolved = resolve_plugin_spec(spec, &config_file.to_string_lossy());
    match resolved {
        Spec::Entry((package, options)) => {
            assert!(package.starts_with("file://"));
            assert_eq!(options.get("apiKey"), Some(&json!("x")));
        }
        other => panic!("expected entry, got {other:?}"),
    }
}

#[test]
fn parses_v2_plugin_object_and_preserves_options() {
    let value = serde_json::json!({
        "plugins": [{
            "package": "opencode-example",
            "options": {"mode": "strict"}
        }]
    });
    let info: oc_config::v1::config::Info = serde_json::from_value(value).unwrap();
    let plugin = info.plugin.unwrap().pop().unwrap();
    assert_eq!(plugin.package(), "opencode-example");
    assert_eq!(plugin.options().unwrap()["mode"], "strict");
}

#[test]
fn full_load_serialization_golden() {
    let home = TestHome::new();
    home.write_global("opencode.json", json!({ "model": "global/model" }));
    home.write_project(
        json!({
            "$schema": "https://opencode.ai/config.json",
            "model": "test/model",
            "username": "testuser",
            "provider": { "custom": { "api": "custom", "options": { "baseURL": "https://x" } } }
        }),
        "opencode.json",
    );
    let state = home.load();
    let serialized = serde_json::to_value(&state.config).expect("serialize");
    assert_eq!(serialized["model"], "test/model");
    assert_eq!(serialized["username"], "testuser");
    assert_eq!(
        serialized["provider"]["custom"]["options"]["baseURL"],
        "https://x"
    );
    // Loader defaults.
    assert_eq!(serialized["agent"], json!({}));
    assert_eq!(serialized["mode"], json!({}));
    assert_eq!(serialized["plugin"], json!([]));
    assert_eq!(serialized["command"], json!({}));
    // $schema comes from the seeded global config.
    assert_eq!(serialized["$schema"], "https://opencode.ai/config.json");
}

#[test]
fn config_from_text_without_filesystem() {
    let text = r#"{
        // comments are fine
        "model": "test/model",
        "agent": { "build": { "model": "test/model", "temperature": 0.7 } }
    }"#;
    let config = oc_config::load::load_config(
        text,
        &oc_config::variable::Source::Virtual {
            source: "test".to_string(),
            dir: "/tmp".to_string(),
        },
        None,
    )
    .expect("load");
    assert_eq!(config.model.as_deref(), Some("test/model"));
    assert_eq!(
        config
            .agent
            .as_ref()
            .and_then(|a| a.get("build"))
            .and_then(|a| a.temperature),
        Some(0.7)
    );
}

#[test]
fn load_errors_on_invalid_top_level_key() {
    let home = TestHome::new();
    home.write_project(json!({ "bogus_key": true }), "opencode.json");
    let error = load_instance_state(&LoadOptions {
        directory: home.project.to_string_lossy().into_owned(),
        worktree: Some(home.home.to_string_lossy().into_owned()),
        env: Default::default(),
        username: Some("u".to_string()),
    })
    .expect_err("should error");
    let message = error.to_string();
    assert!(message.contains("Configuration is invalid"), "{message}");
    assert!(message.contains("bogus_key"), "{message}");
}

#[test]
fn load_errors_on_invalid_json() {
    let home = TestHome::new();
    std::fs::write(home.project.join("opencode.json"), "{ invalid json }").expect("write");
    let error = load_instance_state(&LoadOptions {
        directory: home.project.to_string_lossy().into_owned(),
        worktree: Some(home.home.to_string_lossy().into_owned()),
        env: Default::default(),
        username: Some("u".to_string()),
    })
    .expect_err("should error");
    assert!(
        matches!(error, oc_config::ConfigError::Json { .. }),
        "{error:?}"
    );
}

#[test]
fn plugins_alias_merges_into_plugin_list() {
    let home = TestHome::new();
    std::fs::write(
        home.project.join("opencode.json"),
        r#"{
          "plugin": ["alpha"],
          "plugins": ["beta", "gamma"]
        }"#,
    )
    .expect("write");
    let state = load_instance_state(&LoadOptions {
        directory: home.project.to_string_lossy().into_owned(),
        worktree: Some(home.home.to_string_lossy().into_owned()),
        env: Default::default(),
        username: Some("u".to_string()),
    })
    .expect("load");
    let plugin = state.config.plugin.expect("plugin list");
    let names: Vec<String> = plugin
        .iter()
        .map(|spec| spec.package().to_string())
        .collect();
    assert_eq!(names, vec!["alpha", "beta", "gamma"]);
}

#[test]
fn plugins_alias_alone_is_accepted() {
    let home = TestHome::new();
    std::fs::write(
        home.project.join("opencode.json"),
        r#"{ "plugins": ["delta"] }"#,
    )
    .expect("write");
    let state = load_instance_state(&LoadOptions {
        directory: home.project.to_string_lossy().into_owned(),
        worktree: Some(home.home.to_string_lossy().into_owned()),
        env: Default::default(),
        username: Some("u".to_string()),
    })
    .expect("load");
    let plugin = state.config.plugin.expect("plugin list");
    assert_eq!(plugin[0].package(), "delta");
}
