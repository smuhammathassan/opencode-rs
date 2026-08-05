use oc_command::command::{expand_shell, hints, render, Registry, Source};
use serde_json::json;

#[test]
fn hints_extracts_numbered_and_arguments_placeholders() {
    let template = "$2 $ARGUMENTS $1 $2 $3";
    assert_eq!(hints(template), vec!["$1", "$2", "$3", "$ARGUMENTS"]);
}

#[test]
fn hints_sorts_lexicographically_and_dedupes() {
    let template = "$10 $2 $1 $2";
    assert_eq!(hints(template), vec!["$1", "$10", "$2"]);
}

#[test]
fn hints_are_empty_without_placeholders() {
    assert!(hints("no placeholders here").is_empty());
}

#[test]
fn render_substitutes_numbered_arguments() {
    assert_eq!(
        render("Summarize $1 and $2", "foo bar"),
        "Summarize foo and bar"
    );
}

#[test]
fn render_last_positional_takes_the_rest() {
    assert_eq!(
        render("a $1 $2", "one two three four"),
        "a one two three four"
    );
}

#[test]
fn render_missing_arguments_become_empty() {
    assert_eq!(render("a $1 $2 $3", "only-one"), "a only-one");
}

#[test]
fn render_substitutes_arguments_placeholder() {
    assert_eq!(
        render("Do this:\n$ARGUMENTS\nplease", "the task"),
        "Do this:\nthe task\nplease"
    );
}

#[test]
fn render_appends_arguments_when_no_placeholders() {
    assert_eq!(
        render("do something", "the task"),
        "do something\n\nthe task"
    );
}

#[test]
fn render_does_not_append_arguments_when_arguments_empty() {
    assert_eq!(render("do something", ""), "do something");
}

#[test]
fn render_trims_quotes_from_arguments() {
    assert_eq!(render("read $1", "\"my file.md\""), "read my file.md");
}

#[test]
fn render_handles_image_attachments_as_one_token() {
    assert_eq!(
        render("look at $1 and $2", "[Image 1] foo"),
        "look at [Image 1] and foo"
    );
}

#[test]
fn render_trims_result() {
    let out = render("  $1  ", "x");
    assert_eq!(out, "x");
}

#[test]
fn expand_shell_replaces_commands_with_output() {
    let out = expand_shell("current dir: !`pwd`", &|cmd| Ok(format!("[{cmd}]"))).unwrap();
    assert_eq!(out, "current dir: [pwd]");
}

#[test]
fn expand_shell_failed_command_yields_empty_string() {
    let out = expand_shell("a !`boom` b", &|_| Err(anyhow::anyhow!("boom"))).unwrap();
    assert_eq!(out, "a  b");
}

#[test]
fn expand_shell_preserves_non_matching_text() {
    let out = expand_shell("no shell here", &|_| Ok("x".to_string())).unwrap();
    assert_eq!(out, "no shell here");
}

#[test]
fn defaults_are_registered() {
    let reg = Registry::new("/work/tree");
    assert_eq!(
        reg.get("init").unwrap().description.as_deref(),
        Some("guided AGENTS.md setup")
    );
    let review = reg.get("review").unwrap();
    assert_eq!(
        review.description.as_deref(),
        Some("review changes [commit|branch|pr], defaults to uncommitted")
    );
    assert_eq!(review.subtask, Some(true));
}

#[test]
fn init_template_is_replaced_with_worktree_path() {
    let reg = Registry::new("/some/worktree");
    assert!(reg
        .get("init")
        .unwrap()
        .template
        .resolve()
        .contains("/some/worktree"));
}

#[test]
fn init_and_review_templates_and_hints_match_reference() {
    let reg = Registry::new("/w");
    let init = reg.get("init").unwrap();
    assert!(init
        .template
        .resolve()
        .starts_with("Create or update `AGENTS.md`"));
    assert_eq!(init.hints, vec!["$ARGUMENTS"]);
    let review = reg.get("review").unwrap();
    assert!(review
        .template
        .resolve()
        .contains("You are a code reviewer"));
    assert!(review.template.resolve().contains("$ARGUMENTS"));
    assert_eq!(review.hints, vec!["$ARGUMENTS"]);
}

#[test]
fn config_commands_are_registered() {
    let mut reg = Registry::new("/w");
    reg.add_config_commands(&json!({
        "fix": {
            "description": "Fix a bug",
            "agent": "coder",
            "model": "anthropic/claude",
            "template": "fix the $ARGUMENTS",
            "subtask": true
        }
    }))
    .unwrap();
    let cmd = reg.get("fix").unwrap();
    assert_eq!(cmd.description.as_deref(), Some("Fix a bug"));
    assert_eq!(cmd.agent.as_deref(), Some("coder"));
    assert_eq!(cmd.model.as_deref(), Some("anthropic/claude"));
    assert_eq!(cmd.source, Some(Source::Command));
    assert_eq!(cmd.subtask, Some(true));
    assert_eq!(cmd.hints, vec!["$ARGUMENTS"]);
    assert_eq!(cmd.template.resolve(), "fix the $ARGUMENTS");
}

#[test]
fn config_commands_override_defaults() {
    let mut reg = Registry::new("/w");
    reg.add_config_commands(&json!({
        "init": { "template": "custom init", "description": "custom" }
    }))
    .unwrap();
    assert_eq!(reg.get("init").unwrap().template.resolve(), "custom init");
}

#[test]
fn config_commands_with_invalid_template_are_errors() {
    let mut reg = Registry::new("/w");
    assert!(reg
        .add_config_commands(&json!({ "bad": { "template": 42 } }))
        .is_err());
}

#[test]
fn info_serializes_matching_schema_order() {
    let mut reg = Registry::new("/w");
    reg.add_config_commands(&json!({
        "fix": {
            "description": "Fix a bug",
            "template": "fix it",
            "subtask": true
        }
    }))
    .unwrap();
    let cmd = reg.get("fix").unwrap();
    let value = serde_json::to_value(cmd).unwrap();
    assert_eq!(
        value,
        json!({
            "name": "fix",
            "description": "Fix a bug",
            "source": "command",
            "template": "fix it",
            "subtask": true,
            "hints": []
        })
    );
}

#[test]
fn info_serializes_exact_json_in_schema_field_order() {
    let mut reg = Registry::new("/w");
    reg.add_config_commands(&json!({
        "fix": { "description": "Fix a bug", "template": "fix it" }
    }))
    .unwrap();
    let cmd = reg.get("fix").unwrap();
    let serialized = serde_json::to_string(cmd).unwrap();
    assert_eq!(
        serialized,
        "{\"name\":\"fix\",\"description\":\"Fix a bug\",\"source\":\"command\",\"template\":\"fix it\",\"hints\":[]}"
    );
}

#[test]
fn skill_info_serialization_matches_reference_schema() {
    let skill = oc_command::skill::Info {
        name: "alpha".to_string(),
        description: Some("Does alpha".to_string()),
        location: "/w/.opencode/skills/alpha/SKILL.md".to_string(),
        content: "body".to_string(),
    };
    assert_eq!(
        serde_json::to_string(&skill).unwrap(),
        "{\"name\":\"alpha\",\"description\":\"Does alpha\",\"location\":\"/w/.opencode/skills/alpha/SKILL.md\",\"content\":\"body\"}"
    );
}

#[test]
fn list_contains_all_registered() {
    let mut reg = Registry::new("/w");
    reg.add_config_commands(&json!({ "fix": { "template": "fix it" } }))
        .unwrap();
    let names: Vec<String> = reg.list().map(|c| c.name.clone()).collect();
    assert!(names.contains(&"init".to_string()));
    assert!(names.contains(&"review".to_string()));
    assert!(names.contains(&"fix".to_string()));
}

#[test]
fn skills_are_registered_as_commands_with_base_dir() {
    let mut reg = Registry::new("/w");
    let skill = oc_command::skill::Info {
        name: "mytool".to_string(),
        description: Some("a skill".to_string()),
        location: "/work/tree/.opencode/skills/mytool/SKILL.md".to_string(),
        content: "do the thing".to_string(),
    };
    reg.add_skills(&[skill]);
    let cmd = reg.get("mytool").unwrap();
    assert_eq!(cmd.source, Some(Source::Skill));
    let template = cmd.template.resolve();
    assert!(template.starts_with("do the thing"));
    assert!(template.contains("Base directory for this skill: /work/tree/.opencode/skills/mytool"));
    assert!(cmd.hints.is_empty());
}

#[test]
fn builtin_skill_has_no_base_dir() {
    let mut reg = Registry::new("/w");
    let skill = oc_command::skill::Info {
        name: "customize-opencode".to_string(),
        description: Some("d".to_string()),
        location: "<built-in>".to_string(),
        content: "content".to_string(),
    };
    reg.add_skills(&[skill]);
    assert_eq!(
        reg.get("customize-opencode").unwrap().template.resolve(),
        "content"
    );
}

#[test]
fn skills_do_not_override_existing_commands() {
    let mut reg = Registry::new("/w");
    reg.add_config_commands(&json!({ "init": { "template": "custom init" } }))
        .unwrap();
    let skill = oc_command::skill::Info {
        name: "init".to_string(),
        description: None,
        location: "/x/SKILL.md".to_string(),
        content: "c".to_string(),
    };
    reg.add_skills(&[skill]);
    assert_eq!(reg.get("init").unwrap().template.resolve(), "custom init");
}

fn write(root: &std::path::Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, content).unwrap();
}

#[test]
fn loads_commands_from_command_and_commands_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    write(
        &proj,
        ".opencode/command/fix.md",
        "---\ndescription: Fix a bug\n---\nfix the $ARGUMENTS\n",
    );
    write(&proj, ".opencode/commands/foo.md", "just a template\n");

    let loaded = oc_command::command::load_from_dir(&proj.join(".opencode")).unwrap();
    let map: std::collections::HashMap<String, oc_command::command::CommandConfig> =
        loaded.into_iter().collect();
    assert_eq!(map.len(), 2);
    let fix = &map["fix"];
    assert_eq!(fix.template, "fix the $ARGUMENTS");
    assert_eq!(fix.description.as_deref(), Some("Fix a bug"));
    let foo = &map["foo"];
    assert_eq!(foo.template, "just a template");
}

#[test]
fn loads_nested_command_with_relative_path_name() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    write(&proj, ".opencode/command/sub/x.md", "template body\n");
    let loaded = oc_command::command::load_from_dir(&proj.join(".opencode")).unwrap();
    assert_eq!(loaded[0].0, "sub/x");
}

#[test]
fn frontmatter_name_overrides_path_derived_name() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    write(
        &proj,
        ".opencode/command/a.md",
        "---\nname: renamed\n---\nbody\n",
    );
    let loaded = oc_command::command::load_from_dir(&proj.join(".opencode")).unwrap();
    assert_eq!(loaded[0].0, "renamed");
}

#[test]
fn command_without_frontmatter_loads_with_template_only() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    write(&proj, ".opencode/command/plain.md", "just a body\n");
    let loaded = oc_command::command::load_from_dir(&proj.join(".opencode")).unwrap();
    let map: std::collections::HashMap<String, oc_command::command::CommandConfig> =
        loaded.into_iter().collect();
    assert_eq!(map["plain"].template, "just a body");
}

#[test]
fn invalid_command_config_aborts_load() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    write(
        &proj,
        ".opencode/command/bad.md",
        "---\ndescription: 42\n---\nbody\n",
    );
    write(
        &proj,
        ".opencode/command/good.md",
        "---\ndescription: ok\n---\nbody\n",
    );
    assert!(oc_command::command::load_from_dir(&proj.join(".opencode")).is_err());
}

#[test]
fn unparseable_frontmatter_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    write(
        &proj,
        ".opencode/command/broken.md",
        "---\n{name: unclosed\n---\nbody\n",
    );
    write(
        &proj,
        ".opencode/command/fine.md",
        "---\ndescription: ok\n---\nbody\n",
    );
    let loaded = oc_command::command::load_from_dir(&proj.join(".opencode")).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].0, "fine");
}
