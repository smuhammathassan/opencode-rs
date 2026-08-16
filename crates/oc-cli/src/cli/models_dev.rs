//! models.dev provider database.
//! From reference/packages/core/src/models-dev.ts.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

use super::paths::GlobalPaths;

const DEFAULT_SOURCE: &str = "https://models.opencode.ai";

/// A provider entry from the models.dev database.
#[derive(Debug, Clone, Default)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub env: Vec<String>,
    pub models: BTreeMap<String, Value>,
}

/// Loaded models.dev database: provider id -> provider.
#[derive(Debug, Clone, Default)]
pub struct ModelsDev {
    pub providers: BTreeMap<String, Provider>,
}

fn cache_path(paths: &GlobalPaths, source: &str) -> PathBuf {
    if source == DEFAULT_SOURCE {
        paths.cache.join("models.json")
    } else {
        // Mirrors `models-${Hash.fast(source)}.json` in models-dev.ts.
        let digest = {
            use std::hash::{DefaultHasher, Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            source.hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        };
        paths.cache.join(format!("models-{digest}.json"))
    }
}

impl ModelsDev {
    /// Mirrors `ModelsDev.populate()`: read from disk first, then fall back to
    /// a fetch unless `OPENCODE_DISABLE_MODELS_FETCH` is set.
    pub fn load(paths: &GlobalPaths) -> anyhow::Result<ModelsDev> {
        let source =
            std::env::var("OPENCODE_MODELS_URL").unwrap_or_else(|_| DEFAULT_SOURCE.to_string());
        let filepath = std::env::var_os("OPENCODE_MODELS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| cache_path(paths, &source));

        let from_disk = Self::parse(&std::fs::read_to_string(&filepath).unwrap_or_default()).ok();
        if let Some(db) = from_disk {
            if !db.providers.is_empty() {
                return Ok(db);
            }
        }
        Ok(ModelsDev::default())
    }

    fn parse(text: &str) -> anyhow::Result<ModelsDev> {
        let value: Value = serde_json::from_str(text)?;
        let map = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("expected object"))?;
        let mut providers = BTreeMap::new();
        for (id, entry) in map {
            let obj = entry.as_object().cloned().unwrap_or_default();
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string();
            let env = obj
                .get("env")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            let models = obj
                .get("models")
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            providers.insert(
                id.clone(),
                Provider {
                    id: id.clone(),
                    name,
                    env,
                    models,
                },
            );
        }
        Ok(ModelsDev { providers })
    }

    /// Fetch and cache the database, mirroring `ModelsDev.refresh(force)`.
    pub async fn refresh(paths: &GlobalPaths, force: bool) -> anyhow::Result<()> {
        if matches!(
            std::env::var("OPENCODE_DISABLE_MODELS_FETCH")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "1" | "true"
        ) {
            anyhow::bail!("models.dev fetch disabled by OPENCODE_DISABLE_MODELS_FETCH");
        }
        let source =
            std::env::var("OPENCODE_MODELS_URL").unwrap_or_else(|_| DEFAULT_SOURCE.to_string());
        let filepath = cache_path(paths, &source);

        let fresh = std::fs::metadata(&filepath)
            .ok()
            .and_then(|meta| meta.modified().ok())
            .map(|mtime| {
                let age = std::time::SystemTime::now()
                    .duration_since(mtime)
                    .unwrap_or_default();
                age.as_secs() < 5 * 60
            })
            .unwrap_or(false);
        if !force && fresh {
            return Ok(());
        }

        let client = reqwest::Client::new();
        let text = client
            .get(format!("{source}/api.json"))
            .header("User-Agent", format!("opencode/{}", crate::VERSION))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        if let Some(parent) = filepath.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&filepath, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_models_dev_database() {
        let text = r#"{"openai":{"id":"openai","name":"OpenAI","env":["OPENAI_API_KEY"],"models":{"gpt-4o":{"id":"gpt-4o","name":"GPT-4o"}}}}"#;
        let db = ModelsDev::parse(text).unwrap();
        let openai = &db.providers["openai"];
        assert_eq!(openai.name, "OpenAI");
        assert_eq!(openai.env, vec!["OPENAI_API_KEY"]);
        assert!(openai.models.contains_key("gpt-4o"));
    }

    #[test]
    fn empty_on_missing_file() {
        let db = ModelsDev::parse("").unwrap_or_default();
        assert!(db.providers.is_empty());
    }

    #[tokio::test]
    async fn refresh_is_blocked_by_disable_fetch_flag() {
        static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap();
        let original = std::env::var_os("OPENCODE_DISABLE_MODELS_FETCH");
        std::env::set_var("OPENCODE_DISABLE_MODELS_FETCH", "1");
        let result = ModelsDev::refresh(&GlobalPaths::default(), true).await;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("OPENCODE_DISABLE_MODELS_FETCH"));
        match original {
            Some(value) => std::env::set_var("OPENCODE_DISABLE_MODELS_FETCH", value),
            None => std::env::remove_var("OPENCODE_DISABLE_MODELS_FETCH"),
        }
    }
}
