use oc_command::skill::{fmt, Settings, SkillService};
use std::fs;
use std::path::Path;
use std::path::PathBuf;

fn write(root: &Path, rel: &str, content: &str) -> PathBuf {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    path
}

fn skill_md(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\ncontent for {name}\n")
}

fn settings(home: &Path, project: &Path, worktree: &Path) -> Settings {
    Settings {
        home: home.to_path_buf(),
        directory: project.to_path_buf(),
        worktree: worktree.to_path_buf(),
        disable_external_skills: false,
        disable_claude_code_skills: false,
        paths: vec![],
        pulled_dirs: vec![],
        config_dirs: None,
    }
}

#[test]
fn discovers_singular_and_plural_project_skills() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "proj/.opencode/skill/alpha/SKILL.md",
        &skill_md("alpha", "Does alpha"),
    );
    write(
        tmp.path(),
        "proj/.opencode/skills/beta/SKILL.md",
        &skill_md("beta", "Does beta"),
    );

    let s = settings(
        &tmp.path().join("home"),
        &tmp.path().join("proj"),
        &tmp.path().join("proj"),
    );
    let svc = SkillService::load(&s).unwrap();

    let alpha = svc.get("alpha").unwrap();
    assert_eq!(alpha.description.as_deref(), Some("Does alpha"));
    assert_eq!(
        alpha.location,
        tmp.path()
            .join("proj/.opencode/skill/alpha/SKILL.md")
            .to_str()
            .unwrap()
    );
    assert_eq!(alpha.content, "content for alpha\n");
    assert_eq!(svc.get("beta").unwrap().name, "beta");
}

#[test]
fn discovers_global_and_external_skills() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(
        &home,
        ".config/opencode/skills/g/SKILL.md",
        &skill_md("g", "Global"),
    );
    write(&home, ".claude/skills/c/SKILL.md", &skill_md("c", "Claude"));
    write(&home, ".agents/skills/a/SKILL.md", &skill_md("a", "Agents"));

    let mut s = settings(&home, &tmp.path().join("proj"), &tmp.path().join("proj"));
    s.config_dirs = Some(vec![home.join(".config/opencode")]);
    let svc = SkillService::load(&s).unwrap();
    assert_eq!(svc.get("g").unwrap().description.as_deref(), Some("Global"));
    assert_eq!(svc.get("c").unwrap().description.as_deref(), Some("Claude"));
    assert_eq!(svc.get("a").unwrap().description.as_deref(), Some("Agents"));
}

#[test]
fn discovers_project_external_skills_walking_up() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    let home = tmp.path().join("home");
    write(
        &proj,
        ".claude/skills/pc/SKILL.md",
        &skill_md("pc", "Project claude"),
    );
    write(
        &proj,
        ".agents/skills/pa/SKILL.md",
        &skill_md("pa", "Project agents"),
    );

    let nested = proj.join("sub/dir");
    fs::create_dir_all(&nested).unwrap();
    let s = settings(&home, &nested, &proj);
    let svc = SkillService::load(&s).unwrap();
    assert_eq!(
        svc.get("pc").unwrap().description.as_deref(),
        Some("Project claude")
    );
    assert_eq!(
        svc.get("pa").unwrap().description.as_deref(),
        Some("Project agents")
    );
}

#[test]
fn disable_external_skills_skips_claude_and_agents() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, ".claude/skills/c/SKILL.md", &skill_md("c", "Claude"));
    write(&home, ".agents/skills/a/SKILL.md", &skill_md("a", "Agents"));
    write(
        &home,
        ".config/opencode/skills/g/SKILL.md",
        &skill_md("g", "Global"),
    );

    let mut s = settings(&home, &tmp.path().join("proj"), &tmp.path().join("proj"));
    s.disable_external_skills = true;
    s.config_dirs = Some(vec![home.join(".config/opencode")]);
    let svc = SkillService::load(&s).unwrap();
    assert!(svc.get("c").is_none());
    assert!(svc.get("a").is_none());
    assert_eq!(svc.get("g").unwrap().description.as_deref(), Some("Global"));
}

#[test]
fn disable_claude_code_skills_keeps_agents() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, ".claude/skills/c/SKILL.md", &skill_md("c", "Claude"));
    write(&home, ".agents/skills/a/SKILL.md", &skill_md("a", "Agents"));

    let mut s = settings(&home, &tmp.path().join("proj"), &tmp.path().join("proj"));
    s.disable_claude_code_skills = true;
    let svc = SkillService::load(&s).unwrap();
    assert!(svc.get("c").is_none());
    assert_eq!(svc.get("a").unwrap().description.as_deref(), Some("Agents"));
}

#[test]
fn environment_flags_disable_external_skill_scans() {
    static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    let _guard = ENV_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap();
    let original_external = std::env::var_os("OPENCODE_DISABLE_EXTERNAL_SKILLS");
    let original_claude = std::env::var_os("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS");
    std::env::set_var("OPENCODE_DISABLE_EXTERNAL_SKILLS", "1");
    std::env::remove_var("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS");

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, ".claude/skills/c/SKILL.md", &skill_md("c", "Claude"));
    write(&home, ".agents/skills/a/SKILL.md", &skill_md("a", "Agents"));
    let settings = settings(&home, &tmp.path().join("proj"), &tmp.path().join("proj"));
    let service = SkillService::load_with_environment(&settings).unwrap();
    assert!(service.get("c").is_none());
    assert!(service.get("a").is_none());

    std::env::remove_var("OPENCODE_DISABLE_EXTERNAL_SKILLS");
    std::env::set_var("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS", "1");
    let service = SkillService::load_with_environment(&settings).unwrap();
    assert!(service.get("c").is_none());
    assert!(service.get("a").is_some());

    match original_external {
        Some(value) => std::env::set_var("OPENCODE_DISABLE_EXTERNAL_SKILLS", value),
        None => std::env::remove_var("OPENCODE_DISABLE_EXTERNAL_SKILLS"),
    }
    match original_claude {
        Some(value) => std::env::set_var("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS", value),
        None => std::env::remove_var("OPENCODE_DISABLE_CLAUDE_CODE_SKILLS"),
    }
}

#[test]
fn skill_without_valid_frontmatter_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "proj/.opencode/skills/bad/SKILL.md",
        "no frontmatter here\n",
    );
    write(
        tmp.path(),
        "proj/.opencode/skills/named/SKILL.md",
        "---\ndescription: missing name\n---\nbody\n",
    );
    write(
        tmp.path(),
        "proj/.opencode/skills/typed/SKILL.md",
        "---\nname: typed\ndescription: 5\n---\nbody\n",
    );

    let s = settings(
        &tmp.path().join("home"),
        &tmp.path().join("proj"),
        &tmp.path().join("proj"),
    );
    let svc = SkillService::load(&s).unwrap();
    assert!(svc.get("bad").is_none());
    assert!(svc.get("named").is_none());
    assert!(svc.get("typed").is_none());
}

#[test]
fn builtin_customize_opencode_is_available_and_overridable() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "proj/.opencode/skills/customize-opencode/SKILL.md",
        &skill_md("customize-opencode", "disk override"),
    );

    let s = settings(
        &tmp.path().join("home"),
        &tmp.path().join("proj"),
        &tmp.path().join("proj"),
    );
    let svc = SkillService::load(&s).unwrap();
    let builtin = svc.get("customize-opencode").unwrap();
    assert_eq!(builtin.description.as_deref(), Some("disk override"));
    assert_eq!(
        builtin.location,
        tmp.path()
            .join("proj/.opencode/skills/customize-opencode/SKILL.md")
            .to_str()
            .unwrap()
    );
    assert_eq!(builtin.content, "content for customize-opencode\n");
}

#[test]
fn builtin_skill_present_without_disk_override() {
    let tmp = tempfile::tempdir().unwrap();
    let s = settings(
        &tmp.path().join("home"),
        &tmp.path().join("proj"),
        &tmp.path().join("proj"),
    );
    let svc = SkillService::load(&s).unwrap();
    let builtin = svc.get("customize-opencode").unwrap();
    assert_eq!(builtin.location, "<built-in>");
    assert!(builtin.content.contains("# Customizing opencode"));
}

#[test]
fn skills_paths_are_scanned() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    write(
        &proj,
        "custom-skills/alpha/SKILL.md",
        &skill_md("alpha", "From path"),
    );

    let mut s = settings(&tmp.path().join("home"), &proj, &proj);
    s.paths = vec!["custom-skills".to_string()];
    let svc = SkillService::load(&s).unwrap();
    assert_eq!(
        svc.get("alpha").unwrap().description.as_deref(),
        Some("From path")
    );
}

#[test]
fn missing_skill_path_logs_warning_and_continues() {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    let mut s = settings(&tmp.path().join("home"), &proj, &proj);
    s.paths = vec!["does-not-exist".to_string()];
    let svc = SkillService::load(&s).unwrap();
    assert!(svc.all().iter().any(|x| x.name == "customize-opencode"));
}

#[test]
fn require_returns_not_found_with_available_list() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "proj/.opencode/skills/alpha/SKILL.md",
        &skill_md("alpha", "Does alpha"),
    );
    let s = settings(
        &tmp.path().join("home"),
        &tmp.path().join("proj"),
        &tmp.path().join("proj"),
    );
    let svc = SkillService::load(&s).unwrap();
    let err = svc.require("missing").unwrap_err().to_string();
    assert!(err.starts_with("Skill \"missing\" not found. Available skills:"));
    assert!(err.contains("alpha"));
}

#[test]
fn dirs_returns_discovered_directories() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "proj/.opencode/skills/alpha/SKILL.md",
        &skill_md("alpha", "Does alpha"),
    );
    let s = settings(
        &tmp.path().join("home"),
        &tmp.path().join("proj"),
        &tmp.path().join("proj"),
    );
    let svc = SkillService::load(&s).unwrap();
    assert!(svc
        .dirs()
        .iter()
        .any(|d| d.ends_with("proj/.opencode/skills/alpha")));
}

#[test]
fn available_is_sorted_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "proj/.opencode/skills/zeta/SKILL.md",
        &skill_md("zeta", "Zeta"),
    );
    write(
        tmp.path(),
        "proj/.opencode/skills/alpha/SKILL.md",
        &skill_md("alpha", "Alpha"),
    );
    let s = settings(
        &tmp.path().join("home"),
        &tmp.path().join("proj"),
        &tmp.path().join("proj"),
    );
    let svc = SkillService::load(&s).unwrap();
    let names: Vec<&str> = svc
        .available(None)
        .iter()
        .map(|x| x.name.as_str())
        .collect();
    let mut expected: Vec<&str> = names.clone();
    expected.sort();
    assert_eq!(names, expected);
}

#[test]
fn fmt_lists_skills_non_verbose() {
    let list = vec![
        oc_command::skill::Info {
            name: "alpha".to_string(),
            description: Some("Does alpha".to_string()),
            location: "/w/skills/alpha/SKILL.md".to_string(),
            content: "x".to_string(),
        },
        oc_command::skill::Info {
            name: "undescribed".to_string(),
            description: None,
            location: "/w/skills/u/SKILL.md".to_string(),
            content: "x".to_string(),
        },
    ];
    assert_eq!(
        fmt(&list, false),
        "## Available Skills\n- **alpha**: Does alpha"
    );
}

#[test]
fn fmt_lists_skills_verbose_with_escaped_location() {
    let list = vec![oc_command::skill::Info {
        name: "alpha".to_string(),
        description: Some("Does alpha".to_string()),
        location: "/w/skills/<a>&/SKILL.md".to_string(),
        content: "x".to_string(),
    }];
    assert_eq!(
        fmt(&list, true),
        "<available_skills>\n  <skill>\n    <name>alpha</name>\n    <description>Does alpha</description>\n    <location>/w/skills/&lt;a&gt;&amp;/SKILL.md</location>\n  </skill>\n</available_skills>"
    );
}

#[test]
fn fmt_returns_none_message_when_no_described_skills() {
    let list = vec![oc_command::skill::Info {
        name: "x".to_string(),
        description: None,
        location: "/w/x/SKILL.md".to_string(),
        content: "x".to_string(),
    }];
    assert_eq!(fmt(&list, false), "No skills are currently available.");
}

#[test]
fn pulled_dirs_are_scanned() {
    let tmp = tempfile::tempdir().unwrap();
    let pulled = tmp.path().join("cache/skills/remote");
    write(&pulled, "SKILL.md", &skill_md("remote", "From url"));

    let mut s = settings(
        &tmp.path().join("home"),
        &tmp.path().join("proj"),
        &tmp.path().join("proj"),
    );
    s.pulled_dirs = vec![pulled.clone()];
    let svc = SkillService::load(&s).unwrap();
    assert_eq!(
        svc.get("remote").unwrap().description.as_deref(),
        Some("From url")
    );
}

#[test]
fn global_skills_dirs_walk_up_from_home_only() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    write(&home, ".opencode/skills/h/SKILL.md", &skill_md("h", "Home"));
    let s = settings(&home, &tmp.path().join("proj"), &tmp.path().join("proj"));
    let svc = SkillService::load(&s).unwrap();
    assert_eq!(svc.get("h").unwrap().description.as_deref(), Some("Home"));
}
