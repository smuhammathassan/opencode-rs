//! Golden tests: assert exact system-prompt text and session/message JSON for
//! a fixed cwd/os/date, matching the reference logic in
//! packages/opencode/src/session/system.ts and session.ts.
use oc_session::session;
use oc_session::system;
use oc_session::v1;

fn model(api_id: &str, provider_id: &str) -> oc_session::provider::ProviderModel {
    oc_session::provider::ProviderModel {
        id: api_id.into(),
        provider_id: provider_id.into(),
        api: oc_session::provider::ProviderApiInfo {
            id: api_id.into(),
            npm: None,
            type_: "native".into(),
        },
        name: api_id.into(),
        family: None,
        capabilities: Default::default(),
        cost: Default::default(),
        limit: oc_session::provider::ProviderLimit {
            context: 0.0,
            input: None,
            output: 0.0,
        },
        status: "active".into(),
        options: Default::default(),
        headers: Default::default(),
        release_date: String::new(),
        variants: None,
    }
}

#[test]
fn system_prompt_environment_block_is_verbatim() {
    let model = model("claude-sonnet-4-5", "anthropic");
    let ctx = system::EnvironmentContext {
        directory: "/home/user/project".into(),
        worktree: "/home/user/project".into(),
        vcs_is_git: true,
        platform: "linux".into(),
        today: "Mon Aug 05 2026".into(),
    };
    let blocks = system::environment(&model, &ctx, &[]);
    let expected = "\
You are powered by the model named claude-sonnet-4-5. The exact model ID is anthropic/claude-sonnet-4-5
Here is some useful information about the environment you are running in:
<env>
  Working directory: /home/user/project
  Workspace root folder: /home/user/project
  Is directory a git repo: yes
  Platform: linux
  Today's date: Mon Aug 05 2026
</env>";
    assert_eq!(blocks, vec![expected.to_string()]);
}

fn empty_ruleset() -> v1::Ruleset {
    Vec::new()
}

#[test]
fn system_prompt_assembles_env_instructions_mcp_skills_in_order() {
    let model = model("gpt-4o-mini", "openai");
    let ctx = system::EnvironmentContext {
        directory: "/w".into(),
        worktree: "/w".into(),
        vcs_is_git: false,
        platform: "darwin".into(),
        today: "Tue Aug 06 2026".into(),
    };
    let env = system::environment(&model, &ctx, &[]);
    let instructions = vec!["Instructions from: /w/AGENTS.md\nBe brief".to_string()];
    let mcp = system::mcp(
        &empty_ruleset(),
        None,
        &[system::McpInstruction {
            name: "playwright".into(),
            tools: vec![],
            instructions: "Use the browser to inspect pages.".into(),
        }],
    );
    let skills = system::skills(
        &empty_ruleset(),
        &[system::SkillInfo {
            name: "debugging".into(),
            description: Some("Find and fix bugs".into()),
            location: "/w/.opencode/skill/debugging/SKILL.md".into(),
        }],
    );
    let system_prompt = system::assemble(&env, &instructions, mcp.as_deref(), skills.as_deref());
    // Order: env, instructions, mcp, skills
    assert_eq!(system_prompt.len(), 4);
    assert!(system_prompt[0].starts_with("You are powered by the model named gpt-4o-mini."));
    assert_eq!(
        system_prompt[1],
        "Instructions from: /w/AGENTS.md\nBe brief"
    );
    assert!(system_prompt[2].contains("<mcp_instructions>"));
    assert!(system_prompt[2].contains("  <server name=\"playwright\">"));
    assert!(system_prompt[2].contains("    Use the browser to inspect pages."));
    assert!(system_prompt[3].contains("<available_skills>"));
    assert!(system_prompt[3].contains("    <name>debugging</name>"));
    assert!(
        system_prompt[3].contains("    <location>/w/.opencode/skill/debugging/SKILL.md</location>")
    );
}

#[test]
fn skills_block_omitted_when_denied() {
    let ruleset = vec![v1::PermissionRule {
        permission: "skill".into(),
        pattern: "*".into(),
        action: "deny".into(),
    }];
    assert!(system::skills(&ruleset, &[]).is_none());
}

#[test]
fn mcp_block_omitted_when_all_tools_denied() {
    let ruleset = vec![v1::PermissionRule {
        permission: "mcp__server".into(),
        pattern: "*".into(),
        action: "deny".into(),
    }];
    let instructions = vec![system::McpInstruction {
        name: "server".into(),
        tools: vec!["mcp__server".into()],
        instructions: "hi".into(),
    }];
    assert!(system::mcp(&empty_ruleset(), Some(&ruleset), &instructions).is_none());
}

#[test]
fn session_info_json_matches_reference_shape() {
    let info = session::Info {
        id: "ses_abc".into(),
        slug: "brave-otter".into(),
        project_id: "prj_1".into(),
        workspace_id: Some("wrk_1".into()),
        directory: "/home/user/project".into(),
        path: Some(".".into()),
        parent_id: None,
        summary: Some(v1::SessionSummary {
            additions: 3.0,
            deletions: 1.0,
            files: 2.0,
            diffs: None,
        }),
        cost: Some(0.0001),
        tokens: Some(v1::SessionTokens {
            input: 100.0,
            output: 50.0,
            reasoning: 0.0,
            cache: v1::CacheTokens {
                read: 0.0,
                write: 0.0,
            },
        }),
        share: None,
        title: "Fix the build".into(),
        agent: Some("primary".into()),
        model: Some(v1::SessionModel {
            id: "gpt-4o".into(),
            provider_id: "openai".into(),
            variant: None,
        }),
        version: "v1.18.13".into(),
        metadata: None,
        time: v1::SessionTime {
            created: 1_717_243_200_000,
            updated: 1_717_243_260_000,
            compacting: None,
            archived: None,
        },
        permission: None,
        revert: None,
    };
    let value = serde_json::to_value(&info).unwrap();
    assert_eq!(value["id"], "ses_abc");
    assert_eq!(value["projectID"], "prj_1");
    assert_eq!(value["workspaceID"], "wrk_1");
    assert_eq!(value["summary"]["files"], 2.0);
    assert_eq!(value["model"]["providerID"], "openai");
    assert_eq!(value["time"]["created"], 1_717_243_200_000f64);
    assert_eq!(value["time"]["updated"], 1_717_243_260_000f64);
}

#[test]
fn default_title_uses_iso_string() {
    // 2024-06-01T12:00:00.000Z
    let title = format!(
        "{}{}",
        session::PARENT_TITLE_PREFIX,
        "2024-06-01T12:00:00.000Z"
    );
    assert!(session::is_default_title(&title));
    assert!(!session::is_default_title("Custom"));
}

#[test]
fn default_model_prompt_is_default() {
    assert!(oc_session::system::provider(&model("my-custom-model", "x")).starts_with(
        "You are opencode, an interactive CLI tool that helps users with software engineering tasks."
    ));
}
