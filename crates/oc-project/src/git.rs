/// From reference/packages/opencode/src/git/index.ts
///
/// Git subprocess wrapper used by Vcs / Worktree / Snapshot. The reference
/// resolves this service from the app layer; here it is a plain async struct.
///
/// TODO(integration): move to oc-core once oc-core's Git service lands.
use std::collections::HashMap;

use crate::util::process::{self, SpawnOptions};

const CFG: &[&str] = &[
    "--no-optional-locks",
    "-c",
    "core.autocrlf=false",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.longpaths=true",
    "-c",
    "core.symlinks=true",
    "-c",
    "core.quotepath=false",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Added,
    Deleted,
    Modified,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Added => "added",
            Kind::Deleted => "deleted",
            Kind::Modified => "modified",
        }
    }

    fn from_code(code: &str) -> Kind {
        if code == "??" {
            Kind::Added
        } else if code.contains('U') {
            Kind::Modified
        } else if code.contains('A') && !code.contains('D') {
            Kind::Added
        } else if code.contains('D') && !code.contains('A') {
            Kind::Deleted
        } else {
            Kind::Modified
        }
    }
}

#[derive(Debug, Clone)]
pub struct Base {
    pub name: String,
    pub r#ref: String,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub file: String,
    pub code: String,
    pub status: Kind,
}

#[derive(Debug, Clone)]
pub struct Stat {
    pub file: String,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Patch {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PatchOptions {
    pub context: Option<usize>,
    pub max_output_bytes: Option<usize>,
}

#[derive(Debug)]
pub struct Result {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
}

impl Result {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub cwd: String,
    pub env: Option<HashMap<String, String>>,
    pub max_output_bytes: Option<usize>,
    pub stdin: Option<String>,
}

fn out(result: &Result) -> String {
    result.text().trim().to_string()
}

fn nuls(text: &str) -> Vec<String> {
    text.split('\0').filter(|s| !s.is_empty()).map(String::from).collect()
}

fn fail(error: &std::io::Error) -> Result {
    Result {
        exit_code: 1,
        stdout: Vec::new(),
        stderr: error.to_string().into_bytes(),
        truncated: false,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Git;

impl Git {
    pub async fn run(&self, args: &[&str], opts: &Options) -> Result {
        let mut full = Vec::with_capacity(CFG.len() + args.len());
        full.extend_from_slice(CFG);
        full.extend_from_slice(args);
        let result = process::run(
            "git",
            &full,
            SpawnOptions {
                cwd: Some(opts.cwd.clone()),
                env: opts.env.clone(),
                stdin: opts.stdin.clone(),
                max_output_bytes: opts.max_output_bytes,
            },
        )
        .await;
        match result {
            Ok(result) => Result {
                exit_code: result.exit_code,
                stdout: result.stdout,
                stderr: result.stderr,
                truncated: result.truncated,
            },
            Err(error) => fail(&error),
        }
    }

    pub async fn text(&self, args: &[&str], opts: &Options) -> String {
        self.run(args, opts).await.text()
    }

    pub async fn lines(&self, args: &[&str], opts: &Options) -> Vec<String> {
        self.text(args, opts)
            .await
            .split('\n')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect()
    }

    pub async fn branch(&self, cwd: &str) -> Option<String> {
        let result = self.run(&["symbolic-ref", "--quiet", "--short", "HEAD"], &Options { cwd: cwd.to_string(), ..Default::default() }).await;
        if result.exit_code != 0 {
            return None;
        }
        let text = out(&result);
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    pub async fn prefix(&self, cwd: &str) -> String {
        let result = self.run(&["rev-parse", "--show-prefix"], &Options { cwd: cwd.to_string(), ..Default::default() }).await;
        if result.exit_code != 0 {
            return String::new();
        }
        out(&result)
    }

    pub async fn default_branch(&self, cwd: &str) -> Option<Base> {
        let primary = self.primary(cwd).await;
        if let Some(remote) = primary {
            let head = self
                .run(&["symbolic-ref", &format!("refs/remotes/{remote}/HEAD")], &Options { cwd: cwd.to_string(), ..Default::default() })
                .await;
            if head.exit_code == 0 {
                let r#ref = out(&head).replace("refs/remotes/", "");
                let name = r#ref.strip_prefix(&format!("{remote}/")).map(String::from);
                if let Some(name) = name {
                    if !name.is_empty() {
                        return Some(Base { name, r#ref });
                    }
                }
            }
        }

        let list = self.refs(cwd).await;
        if let Some(next) = self.configured(cwd, &list).await {
            return Some(next);
        }
        if list.iter().any(|item| item == "main") {
            return Some(Base { name: "main".into(), r#ref: "main".into() });
        }
        if list.iter().any(|item| item == "master") {
            return Some(Base { name: "master".into(), r#ref: "master".into() });
        }
        None
    }

    async fn refs(&self, cwd: &str) -> Vec<String> {
        self.lines(&["for-each-ref", "--format=%(refname:short)", "refs/heads"], &Options { cwd: cwd.to_string(), ..Default::default() }).await
    }

    async fn configured(&self, cwd: &str, list: &[String]) -> Option<Base> {
        let result = self.run(&["config", "init.defaultBranch"], &Options { cwd: cwd.to_string(), ..Default::default() }).await;
        let name = out(&result);
        if name.is_empty() || !list.iter().any(|item| item == &name) {
            return None;
        }
        Some(Base { name: name.clone(), r#ref: name })
    }

    async fn primary(&self, cwd: &str) -> Option<String> {
        let list = self.lines(&["remote"], &Options { cwd: cwd.to_string(), ..Default::default() }).await;
        if list.iter().any(|item| item == "origin") {
            return Some("origin".to_string());
        }
        if list.len() == 1 {
            return list.into_iter().next();
        }
        if list.iter().any(|item| item == "upstream") {
            return Some("upstream".to_string());
        }
        list.into_iter().next()
    }

    pub async fn has_head(&self, cwd: &str) -> bool {
        let result = self.run(&["rev-parse", "--verify", "HEAD"], &Options { cwd: cwd.to_string(), ..Default::default() }).await;
        result.exit_code == 0
    }

    pub async fn merge_base(&self, cwd: &str, base: &str, head: &str) -> Option<String> {
        let result = self.run(&["merge-base", base, head], &Options { cwd: cwd.to_string(), ..Default::default() }).await;
        if result.exit_code != 0 {
            return None;
        }
        let text = out(&result);
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    pub async fn show(&self, cwd: &str, r#ref: &str, file: &str, prefix: &str) -> String {
        let target = if prefix.is_empty() {
            file.to_string()
        } else {
            format!("{prefix}{file}")
        };
        let result = self
            .run(&["show", &format!("{}:{target}", r#ref)], &Options { cwd: cwd.to_string(), ..Default::default() })
            .await;
        if result.exit_code != 0 || result.stdout.contains(&0) {
            return String::new();
        }
        result.text()
    }

    pub async fn status(&self, cwd: &str) -> Vec<Item> {
        nuls(
            &self
                .text(
                    &["status", "--porcelain=v1", "--untracked-files=all", "--no-renames", "-z", "--", "."],
                    &Options { cwd: cwd.to_string(), ..Default::default() },
                )
                .await,
        )
        .iter()
        .filter_map(|item| {
            let file = item.get(3..)?;
            if file.is_empty() {
                return None;
            }
            let code = item.get(0..2).unwrap_or_default();
            Some(Item { file: file.to_string(), code: code.to_string(), status: Kind::from_code(code) })
        })
        .collect()
    }

    pub async fn diff(&self, cwd: &str, r#ref: &str) -> Vec<Item> {
        let list = nuls(
            &self
                .text(
                    &["diff", "--no-ext-diff", "--no-renames", "--name-status", "-z", r#ref, "--", "."],
                    &Options { cwd: cwd.to_string(), ..Default::default() },
                )
                .await,
        );
        let mut items = Vec::new();
        for (idx, code) in list.iter().enumerate() {
            if idx % 2 != 0 {
                continue;
            }
            let file = list.get(idx + 1);
            if let (Some(file), true) = (file, !code.is_empty()) {
                if !file.is_empty() {
                    items.push(Item { file: file.clone(), code: code.clone(), status: Kind::from_code(code) });
                }
            }
        }
        items
    }

    pub async fn stats(&self, cwd: &str, r#ref: &str) -> Vec<Stat> {
        nuls(
            &self
                .text(
                    &["diff", "--no-ext-diff", "--no-renames", "--numstat", "-z", r#ref, "--", "."],
                    &Options { cwd: cwd.to_string(), ..Default::default() },
                )
                .await,
        )
        .iter()
        .filter_map(|item| {
            let a = item.find('\t')?;
            let b = item[a + 1..].find('\t').map(|offset| a + 1 + offset)?;
            let file = item.get(b + 1..)?;
            if file.is_empty() {
                return None;
            }
            let adds = item.get(0..a).unwrap_or_default();
            let dels = item.get(a + 1..b).unwrap_or_default();
            let additions = if adds == "-" { 0 } else { adds.parse::<u64>().unwrap_or(0) };
            let deletions = if dels == "-" { 0 } else { dels.parse::<u64>().unwrap_or(0) };
            Some(Stat { file: file.to_string(), additions, deletions })
        })
        .collect()
    }

    pub async fn patch(&self, cwd: &str, r#ref: &str, file: &str, options: Option<PatchOptions>) -> Patch {
        let options = options.unwrap_or_default();
        let context = options.context.unwrap_or(3);
        let result = self
            .run(
                &["diff", "--patch", "--no-ext-diff", "--no-renames", &format!("--unified={context}"), r#ref, "--", file],
                &Options { cwd: cwd.to_string(), max_output_bytes: options.max_output_bytes, ..Default::default() },
            )
            .await;
        Patch { text: if result.truncated { String::new() } else { result.text() }, truncated: result.truncated }
    }

    pub async fn patch_all(&self, cwd: &str, r#ref: &str, options: Option<PatchOptions>) -> Patch {
        let options = options.unwrap_or_default();
        let context = options.context.unwrap_or(3);
        let result = self
            .run(
                &["diff", "--patch", "--no-ext-diff", "--no-renames", &format!("--unified={context}"), r#ref, "--", "."],
                &Options { cwd: cwd.to_string(), max_output_bytes: options.max_output_bytes, ..Default::default() },
            )
            .await;
        Patch { text: result.text(), truncated: result.truncated }
    }

    pub async fn patch_untracked(&self, cwd: &str, file: &str, options: Option<PatchOptions>) -> Patch {
        let options = options.unwrap_or_default();
        let context = options.context.unwrap_or(3);
        let result = self
            .run(
                &[
                    "diff",
                    "--no-index",
                    "--patch",
                    "--no-ext-diff",
                    "--no-renames",
                    &format!("--unified={context}"),
                    "--",
                    "/dev/null",
                    file,
                ],
                &Options { cwd: cwd.to_string(), max_output_bytes: options.max_output_bytes, ..Default::default() },
            )
            .await;
        Patch { text: if result.truncated { String::new() } else { result.text() }, truncated: result.truncated }
    }

    pub async fn stat_untracked(&self, cwd: &str, file: &str) -> Option<Stat> {
        let result = self
            .run(
                &["diff", "--no-index", "--numstat", "--", "/dev/null", file],
                &Options { cwd: cwd.to_string(), max_output_bytes: Some(4096), ..Default::default() },
            )
            .await;
        if result.truncated {
            return None;
        }
        let text = result.text();
        let parts: Vec<&str> = text.split('\t').collect();
        if parts.len() < 2 {
            return None;
        }
        let additions = if parts[0] == "-" { 0 } else { parts[0].parse::<u64>().unwrap_or(0) };
        let deletions = if parts[1] == "-" { 0 } else { parts[1].parse::<u64>().unwrap_or(0) };
        Some(Stat { file: file.to_string(), additions, deletions })
    }

    pub async fn apply_patch(&self, cwd: &str, patch: &str) -> Result {
        self.run(&["apply", "-"], &Options { cwd: cwd.to_string(), stdin: Some(patch.to_string()), ..Default::default() }).await
    }
}
