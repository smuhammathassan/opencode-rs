/// From reference/packages/opencode/src/session/instruction.ts
///
/// AGENTS.md / CLAUDE.md / CONTEXT.md instruction discovery and attachment.
use crate::v1::{Part, WithParts};

#[derive(Debug, Clone)]
pub struct InstructionConfig {
    pub instructions: Vec<String>,
    pub disable_project_config: bool,
    pub disable_claude_code_prompt: bool,
}

/// Global instruction files, in priority order.
pub fn global_files(
    global_config_dir: &str,
    home: &str,
    disable_claude_code_prompt: bool,
) -> Vec<String> {
    let mut files = vec![format!("{global_config_dir}/AGENTS.md")];
    if !disable_claude_code_prompt {
        files.push(format!("{home}/.claude/CLAUDE.md"));
    }
    files
}

/// Project instruction files, in priority order.
pub fn instruction_files(disable_claude_code_prompt: bool) -> Vec<String> {
    let mut files = vec!["AGENTS.md".to_string()];
    if !disable_claude_code_prompt {
        files.push("CLAUDE.md".to_string());
    }
    files.push("CONTEXT.md".to_string());
    files
}

/// From reference `instruction.ts:extract` — paths loaded by completed `read`
/// tool calls.
pub fn extract(messages: &[WithParts]) -> std::collections::HashSet<String> {
    let mut paths = std::collections::HashSet::new();
    for msg in messages {
        for part in &msg.parts {
            if let Part::Tool(tool) = part {
                if tool.tool != "read" {
                    continue;
                }
                if let crate::v1::ToolState::Completed(state) = &tool.state {
                    if state.time.compacted.is_some() {
                        continue;
                    }
                    if let Some(loaded) = state.metadata.get("loaded").and_then(|v| v.as_array()) {
                        for item in loaded {
                            if let Some(path) = item.as_str() {
                                paths.insert(path.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    paths
}

pub trait InstructionDeps {
    fn exists(&self, path: &str) -> bool;
    /// `findUp(file, directory, stop)` — ancestor matches from `directory` up
    /// to (and including) `stop`.
    fn find_up(&self, file: &str, directory: &str, stop: &str) -> Vec<String>;
    fn glob(&self, file: &str, cwd: &str) -> Vec<String>;
    fn read(&self, path: &str) -> String;
    fn fetch(&self, url: &str) -> String;
}

/// From reference `instruction.ts:systemPaths`.
pub fn system_paths(
    config: &InstructionConfig,
    directory: &str,
    worktree: &str,
    global_config_dir: &str,
    home: &str,
    deps: &dyn InstructionDeps,
) -> Vec<String> {
    let mut paths = std::collections::HashSet::new();
    for file in global_files(global_config_dir, home, config.disable_claude_code_prompt) {
        if deps.exists(&file) {
            paths.insert(resolve(&file));
            break;
        }
    }
    if !config.disable_project_config {
        for file in instruction_files(config.disable_claude_code_prompt) {
            let matches = deps.find_up(&file, directory, worktree);
            if !matches.is_empty() {
                for item in matches {
                    paths.insert(resolve(&item));
                }
                break;
            }
        }
    }
    for raw in &config.instructions {
        if raw.starts_with("https://") || raw.starts_with("http://") {
            continue;
        }
        let instruction = if let Some(rest) = raw.strip_prefix("~/") {
            format!("{home}/{rest}")
        } else {
            raw.clone()
        };
        let instruction_path = std::path::Path::new(&instruction);
        let matches: Vec<String> = if instruction_path.is_absolute() {
            match instruction_path.file_name() {
                Some(name) => {
                    let name = name.to_string_lossy().to_string();
                    let cwd = instruction_path
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    deps.glob(&name, &cwd)
                }
                None => Vec::new(),
            }
        } else {
            // `globUp(instruction, directory, worktree)`
            deps.find_up(&instruction, directory, worktree)
        };
        for item in matches {
            paths.insert(resolve(&item));
        }
    }
    paths.into_iter().collect()
}

/// From reference `instruction.ts:system` — reads local files and fetches
/// remote instruction URLs.
pub fn system_instructions(
    config: &InstructionConfig,
    directory: &str,
    worktree: &str,
    global_config_dir: &str,
    home: &str,
    deps: &dyn InstructionDeps,
) -> Vec<String> {
    let paths = system_paths(config, directory, worktree, global_config_dir, home, deps);
    let urls: Vec<String> = config
        .instructions
        .iter()
        .filter(|item| item.starts_with("https://") || item.starts_with("http://"))
        .cloned()
        .collect();
    let mut result = Vec::new();
    for path in &paths {
        let content = deps.read(path);
        if !content.is_empty() {
            result.push(format!("Instructions from: {path}\n{content}"));
        }
    }
    for url in &urls {
        let content = deps.fetch(url);
        if !content.is_empty() {
            result.push(format!("Instructions from: {url}\n{content}"));
        }
    }
    result
}

/// From reference `instruction.ts:find` — first instruction file in a dir.
pub fn find(
    dir: &str,
    disable_claude_code_prompt: bool,
    deps: &dyn InstructionDeps,
) -> Option<String> {
    for file in instruction_files(disable_claude_code_prompt) {
        let filepath = std::path::Path::new(dir).join(&file);
        if deps.exists(&filepath.to_string_lossy()) {
            return Some(filepath.to_string_lossy().to_string());
        }
    }
    None
}

/// From reference `instruction.ts:resolve` — walk up from the read file and
/// attach nearby instruction files once per message. Mirrors the reference's
/// `resolve` parameter list.
#[allow(clippy::too_many_arguments)]
pub fn resolve_instructions(
    messages: &[WithParts],
    filepath: &str,
    message_id: &str,
    directory: &str,
    disable_claude_code_prompt: bool,
    system_paths: &std::collections::HashSet<String>,
    deps: &dyn InstructionDeps,
    claims: &mut std::collections::HashMap<String, std::collections::HashSet<String>>,
) -> Vec<(String, String)> {
    let already = extract(messages);
    let mut results = Vec::new();
    let root = std::path::Path::new(directory).to_path_buf();
    let target = std::path::Path::new(filepath).to_path_buf();
    let mut current = target.parent().map(|p| p.to_path_buf()).unwrap_or_default();

    while current.starts_with(&root) && current != root {
        let found = find(&current.to_string_lossy(), disable_claude_code_prompt, deps);
        if let Some(found) = found {
            if found == filepath
                || system_paths.contains(&found)
                || already.contains(&found)
                || claims
                    .get(message_id)
                    .is_some_and(|set| set.contains(&found))
            {
                current = current
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default();
                continue;
            }
            let set = claims.entry(message_id.to_string()).or_default();
            set.insert(found.clone());
            let content = deps.read(&found);
            if !content.is_empty() {
                results.push((
                    found.clone(),
                    format!("Instructions from: {found}\n{content}"),
                ));
            }
        }
        current = current
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
    }
    results
}

pub fn resolve(path: &str) -> String {
    std::path::Path::new(path)
        .canonicalize()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDeps;

    impl InstructionDeps for FakeDeps {
        fn exists(&self, path: &str) -> bool {
            path.contains("AGENTS.md") || path.contains("CLAUDE.md")
        }
        fn find_up(&self, file: &str, directory: &str, _stop: &str) -> Vec<String> {
            if file == "AGENTS.md" && directory.contains("sub") {
                vec!["/work/AGENTS.md".to_string()]
            } else {
                vec![]
            }
        }
        fn glob(&self, file: &str, _cwd: &str) -> Vec<String> {
            vec![format!("/custom/{file}")]
        }
        fn read(&self, path: &str) -> String {
            format!("content of {path}")
        }
        fn fetch(&self, url: &str) -> String {
            if url.starts_with("http") {
                format!("remote content of {url}")
            } else {
                String::new()
            }
        }
    }

    #[test]
    fn extract_collects_loaded_read_paths() {
        let mut metadata = crate::JsonMap::new();
        metadata.insert("loaded".into(), serde_json::json!(["/a.ts", "/b.ts"]));
        let part = Part::Tool(crate::v1::ToolPart {
            base: crate::v1::PartBase {
                id: "p".into(),
                session_id: "s".into(),
                message_id: "m".into(),
            },
            type_: "tool".into(),
            call_id: "c".into(),
            tool: "read".into(),
            state: crate::v1::ToolState::Completed(crate::v1::ToolStateCompleted {
                status: "completed".into(),
                input: Default::default(),
                output: String::new(),
                title: String::new(),
                metadata,
                time: crate::v1::CompletedTime {
                    start: 0,
                    end: 1,
                    compacted: None,
                },
                attachments: None,
            }),
            metadata: None,
        });
        let messages = vec![WithParts {
            info: crate::v1::Info::User(Box::new(crate::v1::User {
                id: "m".into(),
                session_id: "s".into(),
                role: "user".into(),
                time: crate::v1::UserTime { created: 0 },
                format: None,
                summary: None,
                agent: "primary".into(),
                model: crate::v1::UserModel {
                    provider_id: "p".into(),
                    model_id: "m".into(),
                    variant: None,
                },
                system: None,
                tools: None,
            })),
            parts: vec![part],
        }];
        let loaded = extract(&messages);
        assert!(loaded.contains("/a.ts"));
        assert!(loaded.contains("/b.ts"));
    }

    #[test]
    fn system_instructions_skip_remote_in_paths() {
        let config = InstructionConfig {
            instructions: vec!["https://example.com/x.md".into(), "AGENTS.md".into()],
            disable_project_config: false,
            disable_claude_code_prompt: true,
        };
        let result = system_instructions(&config, "/work/sub", "/work", "/cfg", "/home", &FakeDeps);
        assert!(result
            .iter()
            .any(|item| item.contains("Instructions from: /work/AGENTS.md")));
        assert!(result
            .iter()
            .any(|item| item.contains("Instructions from: https://example.com/x.md")));
    }
}
