/// From reference/packages/opencode/src/format/index.ts
///
/// The `Format` service state machine: builds a formatter registry from the
/// `formatter` config, computes `status()`, and formats a file by running every
/// enabled formatter whose extensions match. The reference keeps per-directory
/// state via `InstanceState`; here the caller constructs one `Format` per
/// project directory. `TODO(integration): wire to oc-config's Config and
/// InstanceState`.
pub mod formatter;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use indexmap::IndexMap;

use futures::FutureExt;

use crate::util::process::{Env, RunOptions, Stdio};

use formatter::{Context, EnabledFn, Info};

pub struct Status {
    pub name: String,
    pub extensions: Vec<String>,
    pub enabled: bool,
}

#[derive(Default, Clone)]
pub struct FormatterItem {
    pub command: Option<Vec<String>>,
    pub disabled: bool,
    pub extensions: Option<Vec<String>>,
    pub environment: Option<HashMap<String, String>>,
}

pub enum FormatterConfig {
    Disabled,
    All,
    Custom(IndexMap<String, FormatterItem>),
}

#[derive(Clone)]
struct RegistryEntry {
    name: String,
    environment: Option<HashMap<String, String>>,
    extensions: Vec<String>,
    enabled: EnabledFn,
}

impl RegistryEntry {
    fn from_info(info: Info) -> Self {
        RegistryEntry {
            name: info.name.to_string(),
            environment: info.environment,
            extensions: info.extensions.iter().map(|e| e.to_string()).collect(),
            enabled: info.enabled,
        }
    }
}

pub struct Format {
    config: FormatterConfig,
    directory: String,
    worktree: String,
    experimental_oxfmt: bool,
    commands: RefCell<HashMap<String, Option<Vec<String>>>>,
    registry: RefCell<Option<Vec<RegistryEntry>>>,
}

impl Format {
    pub fn new(
        config: FormatterConfig,
        directory: String,
        worktree: String,
        experimental_oxfmt: bool,
    ) -> Self {
        Format {
            config,
            directory,
            worktree,
            experimental_oxfmt,
            commands: RefCell::new(HashMap::new()),
            registry: RefCell::new(None),
        }
    }

    fn context(&self) -> Context {
        Context {
            directory: self.directory.clone(),
            worktree: self.worktree.clone(),
            experimental_oxfmt: self.experimental_oxfmt,
        }
    }

    fn build_registry(&self) -> Vec<RegistryEntry> {
        let mut registry: Vec<RegistryEntry> = Vec::new();
        match &self.config {
            FormatterConfig::Disabled => return registry,
            FormatterConfig::All => {
                for info in formatter::all() {
                    registry.push(RegistryEntry::from_info(info));
                }
            }
            FormatterConfig::Custom(items) => {
                for info in formatter::all() {
                    registry.push(RegistryEntry::from_info(info));
                }
                for (name, item) in items {
                    // Ruff and uv are both the same formatter, so disabling
                    // either should disable both.
                    if (name == "ruff" || name == "uv")
                        && (items.get("ruff").is_some_and(|i| i.disabled)
                            || items.get("uv").is_some_and(|i| i.disabled))
                    {
                        registry.retain(|e| e.name != "ruff" && e.name != "uv");
                        continue;
                    }
                    if item.disabled {
                        registry.retain(|e| e.name != *name);
                        continue;
                    }
                    let built_in = formatter::by_name(name);
                    let extensions = item
                        .extensions
                        .clone()
                        .or_else(|| {
                            built_in
                                .as_ref()
                                .map(|info| info.extensions.iter().map(|e| e.to_string()).collect())
                        })
                        .unwrap_or_default();
                    let environment = item
                        .environment
                        .clone()
                        .or_else(|| built_in.as_ref().and_then(|info| info.environment.clone()));
                    let enabled: EnabledFn = if let Some(command) = &item.command {
                        let command = command.clone();
                        Arc::new(move |_ctx| {
                            let command = command.clone();
                            async move { Ok(Some(command)) }.boxed()
                        })
                    } else if let Some(info) = &built_in {
                        Arc::clone(&info.enabled)
                    } else {
                        Arc::new(|_ctx| async move { Ok(None) }.boxed())
                    };
                    match registry.iter_mut().find(|e| e.name == *name) {
                        Some(entry) => {
                            entry.extensions = extensions;
                            entry.environment = environment;
                            entry.enabled = enabled;
                        }
                        None => registry.push(RegistryEntry {
                            name: name.clone(),
                            environment,
                            extensions,
                            enabled,
                        }),
                    }
                }
            }
        }
        registry
    }

    fn registry(&self) -> Vec<RegistryEntry> {
        let mut registry = self.registry.borrow_mut();
        if registry.is_none() {
            *registry = Some(self.build_registry());
        }
        registry.clone().unwrap()
    }

    async fn get_command(&self, entry: &RegistryEntry) -> anyhow::Result<Option<Vec<String>>> {
        if let Some(cmd) = self.commands.borrow().get(&entry.name) {
            return Ok(cmd.clone());
        }
        let context = self.context();
        let cmd = (entry.enabled)(context).await?;
        self.commands
            .borrow_mut()
            .insert(entry.name.clone(), cmd.clone());
        Ok(cmd)
    }

    /// Mirrors `Format.init`.
    pub async fn init(&self) {
        self.registry();
    }

    /// Mirrors `Format.status`.
    pub async fn status(&self) -> anyhow::Result<Vec<Status>> {
        let mut result = Vec::new();
        for entry in self.registry() {
            let enabled = self.get_command(&entry).await?.is_some();
            result.push(Status {
                name: entry.name,
                extensions: entry.extensions,
                enabled,
            });
        }
        Ok(result)
    }

    /// Mirrors `Format.file`: runs every enabled formatter matching the file's
    /// extension, in registry order. Enabled checks run concurrently.
    pub async fn file(&self, filepath: &str) -> anyhow::Result<bool> {
        let registry = self.registry();
        let ext = PathBuf::from(filepath)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let matching: Vec<RegistryEntry> = registry
            .into_iter()
            .filter(|e| e.extensions.iter().any(|x| x == &ext))
            .collect();
        if matching.is_empty() {
            return Ok(false);
        }

        let checks: Vec<(RegistryEntry, Option<Vec<String>>)> =
            futures::future::join_all(matching.into_iter().map(|entry| {
                let entry = Arc::new(entry);
                let self_ref = self;
                async move {
                    let cmd = self_ref.get_command(&entry).await.unwrap_or_default();
                    (Arc::try_unwrap(entry).unwrap_or_else(|e| (*e).clone()), cmd)
                }
            }))
            .await;

        let mut ran = false;
        for (entry, cmd) in checks {
            let Some(cmd) = cmd else { continue };
            ran = true;
            let replaced: Vec<String> = cmd.iter().map(|x| x.replace("$FILE", filepath)).collect();
            let mut env = HashMap::new();
            if let Some(extra) = &entry.environment {
                env.extend(extra.clone());
            }
            let result = crate::util::process::run(
                &replaced,
                &RunOptions {
                    cwd: Some(PathBuf::from(&self.directory)),
                    env: Env::Override(env),
                    stdin: Stdio::Ignore,
                    nothrow: true,
                    ..Default::default()
                },
            )
            .await;
            match result {
                Ok(out) if out.code != 0 => {
                    tracing::warn!("format command failed: {:?} (code {})", replaced, out.code);
                }
                Err(e) => {
                    tracing::warn!("format command spawn failed: {:?}: {e}", replaced);
                }
                _ => {}
            }
        }
        Ok(ran)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-util-format-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn status_empty_when_disabled() {
        let dir = tmp_dir("empty");
        let fmt = Format::new(
            FormatterConfig::Disabled,
            dir.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            false,
        );
        assert!(fmt.status().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn status_lists_all_builtins_when_enabled() {
        let dir = tmp_dir("all");
        let fmt = Format::new(
            FormatterConfig::All,
            dir.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            false,
        );
        let statuses = fmt.status().await.unwrap();
        assert!(statuses
            .iter()
            .any(|s| s.name == "gofmt" && s.extensions.contains(&".go".to_string())));
        assert!(statuses.iter().any(|s| s.name == "mix"));
    }

    #[tokio::test]
    async fn custom_config_keeps_builtins_and_adds_custom() {
        let dir = tmp_dir("custom");
        let mut items = IndexMap::new();
        items.insert(
            "gofmt".to_string(),
            FormatterItem {
                disabled: true,
                ..Default::default()
            },
        );
        items.insert(
            "custom".to_string(),
            FormatterItem {
                command: Some(vec!["true".to_string()]),
                extensions: Some(vec![".xyz".to_string()]),
                ..Default::default()
            },
        );
        let fmt = Format::new(
            FormatterConfig::Custom(items),
            dir.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            false,
        );
        let statuses = fmt.status().await.unwrap();
        assert!(statuses.iter().all(|s| s.name != "gofmt"));
        assert!(statuses.iter().any(|s| s.name == "mix"));
        let custom = statuses.iter().find(|s| s.name == "custom").unwrap();
        assert_eq!(custom.extensions, vec![".xyz"]);
    }

    #[tokio::test]
    async fn disabling_ruff_also_disables_uv() {
        let dir = tmp_dir("ruff");
        let mut items = IndexMap::new();
        items.insert(
            "ruff".to_string(),
            FormatterItem {
                disabled: true,
                ..Default::default()
            },
        );
        let fmt = Format::new(
            FormatterConfig::Custom(items),
            dir.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            false,
        );
        let statuses = fmt.status().await.unwrap();
        assert!(statuses.iter().all(|s| s.name != "ruff" && s.name != "uv"));
    }

    #[tokio::test]
    async fn file_returns_false_with_no_matching_formatter() {
        let dir = tmp_dir("nomatch");
        std::fs::write(dir.join("test.txt"), "x").unwrap();
        let fmt = Format::new(
            FormatterConfig::Disabled,
            dir.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            false,
        );
        assert!(!fmt
            .file(&dir.join("test.txt").to_string_lossy())
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn file_runs_custom_commands_sequentially() {
        let dir = tmp_dir("seq");
        let file = dir.join("test.seq");
        std::fs::write(&file, "x").unwrap();
        let mut items = IndexMap::new();
        items.insert(
            "first".to_string(),
            FormatterItem {
                command: Some(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    "f=$1; printf 'A' >> \"$f\"".to_string(),
                    "sh".to_string(),
                    "$FILE".to_string(),
                ]),
                extensions: Some(vec![".seq".to_string()]),
                ..Default::default()
            },
        );
        items.insert(
            "second".to_string(),
            FormatterItem {
                command: Some(vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    "f=$1; printf 'B' >> \"$f\"".to_string(),
                    "sh".to_string(),
                    "$FILE".to_string(),
                ]),
                extensions: Some(vec![".seq".to_string()]),
                ..Default::default()
            },
        );
        let fmt = Format::new(
            FormatterConfig::Custom(items),
            dir.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            false,
        );
        let ran = fmt.file(&file.to_string_lossy()).await.unwrap();
        assert!(ran);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "xAB");
    }

    #[tokio::test]
    async fn command_cache_is_reused() {
        let dir = tmp_dir("cache");
        let mut items = IndexMap::new();
        items.insert(
            "custom".to_string(),
            FormatterItem {
                command: Some(vec!["echo".to_string()]),
                extensions: Some(vec![".c".to_string()]),
                ..Default::default()
            },
        );
        let fmt = Format::new(
            FormatterConfig::Custom(items),
            dir.to_string_lossy().into_owned(),
            dir.to_string_lossy().into_owned(),
            false,
        );
        let first = fmt.status().await.unwrap();
        let second = fmt.status().await.unwrap();
        assert_eq!(first.len(), second.len());
    }
}
