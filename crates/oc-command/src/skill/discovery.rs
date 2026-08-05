//! Skill discovery from remote `cfg.skills.urls` indexes.
//!
//! From reference/packages/opencode/src/skill/discovery.ts.

use futures::future::BoxFuture;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// HTTP GET returning raw bytes. Injectable so tests avoid the network.
pub type HttpGet = dyn Fn(String) -> BoxFuture<'static, anyhow::Result<Vec<u8>>> + Send + Sync;

#[derive(Debug, Clone, Deserialize)]
pub struct IndexSkill {
    pub name: String,
    pub files: Vec<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Index {
    pub skills: Vec<IndexSkill>,
}

pub struct Discovery {
    cache: PathBuf,
    get: Arc<HttpGet>,
}

impl Discovery {
    pub fn new(cache: PathBuf) -> Self {
        let client = reqwest::Client::new();
        Discovery {
            cache,
            get: Arc::new(move |url| {
                let client = client.clone();
                Box::pin(async move {
                    let response = client.get(&url).send().await?;
                    Ok(response.bytes().await?.to_vec())
                })
            }),
        }
    }

    pub fn with_getter(cache: PathBuf, get: Arc<HttpGet>) -> Self {
        Discovery { cache, get }
    }

    /// Mirror of `Discovery.pull`. Downloads each skill into
    /// `<cache>/<name>` and returns the directories that contain a `SKILL.md`.
    pub async fn pull(&self, url: &str) -> anyhow::Result<Vec<PathBuf>> {
        let base = if url.ends_with('/') {
            url.to_string()
        } else {
            format!("{url}/")
        };
        let index_url = format!("{base}index.json");
        let host = base.trim_end_matches('/');

        let Some(index) = self.fetch_index(&index_url).await else {
            return Ok(Vec::new());
        };

        for skill in index.skills.iter().filter(|skill| !has_skill_md(skill)) {
            tracing::warn!(url = %index_url, skill = %skill.name, "skill entry missing SKILL.md");
        }

        let mut dirs: Vec<PathBuf> = Vec::new();
        for skill in index.skills.iter().filter(|skill| has_skill_md(skill)) {
            let root = self.cache.join(&skill.name);
            let version_file = root.join(".opencode-version");
            let version = skill.version.clone();
            let current = match &version {
                None => None,
                Some(_) => tokio::fs::read_to_string(&version_file).await.ok(),
            };

            if version.is_none() || current.as_deref() == version.as_deref() {
                for file in &skill.files {
                    let dest = root.join(file);
                    let url = format!("{host}/{}/{}", skill.name, file);
                    if !self.download(&url, &dest).await {
                        break;
                    }
                }
            } else {
                let token = uuid::Uuid::new_v4().to_string();
                let staging = PathBuf::from(format!("{}.tmp-{token}", root.display()));
                let backup = PathBuf::from(format!("{}.old-{token}", root.display()));
                self.refresh(skill, &root, &staging, &backup, host).await;
                let _ = tokio::fs::remove_dir_all(&staging).await;
            }

            if tokio::fs::try_exists(root.join("SKILL.md"))
                .await
                .unwrap_or(false)
            {
                dirs.push(root);
            }
        }
        Ok(dirs)
    }

    async fn fetch_index(&self, url: &str) -> Option<Index> {
        match (self.get)(url.to_string()).await {
            Ok(body) => match serde_json::from_slice(&body) {
                Ok(index) => Some(index),
                Err(error) => {
                    tracing::error!(url = %url, error = %error, "failed to parse index");
                    None
                }
            },
            Err(error) => {
                tracing::error!(url = %url, error = %error, "failed to fetch index");
                None
            }
        }
    }

    async fn download(&self, url: &str, dest: &Path) -> bool {
        if tokio::fs::try_exists(dest).await.unwrap_or(false) {
            return true;
        }
        match (self.get)(url.to_string()).await {
            Ok(body) => {
                if let Some(parent) = dest.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                tokio::fs::write(dest, body).await.is_ok()
            }
            Err(error) => {
                tracing::error!(url = %url, error = %error, "failed to download");
                false
            }
        }
    }

    /// Swap in a freshly downloaded skill when the cached version differs.
    async fn refresh(
        &self,
        skill: &IndexSkill,
        root: &Path,
        staging: &Path,
        backup: &Path,
        host: &str,
    ) {
        let mut ok = true;
        for file in &skill.files {
            let dest = staging.join(file);
            let url = format!("{host}/{}/{}", skill.name, file);
            if !self.download(&url, &dest).await {
                ok = false;
            }
        }
        if !ok {
            return;
        }
        if !tokio::fs::try_exists(staging.join("SKILL.md"))
            .await
            .unwrap_or(false)
        {
            return;
        }
        let _ = tokio::fs::write(
            staging.join(".opencode-version"),
            skill.version.clone().expect("version branch"),
        )
        .await;

        let cached = tokio::fs::try_exists(root).await.unwrap_or(false);
        if cached {
            let _ = tokio::fs::rename(root, backup).await;
        }
        if let Err(error) = tokio::fs::rename(staging, root).await {
            tracing::error!(skill = %skill.name, error = %error, "failed to refresh skill");
            if cached {
                let _ = tokio::fs::rename(backup, root).await;
            }
            return;
        }
        if cached {
            let _ = tokio::fs::remove_dir_all(backup).await;
        }
    }
}

fn has_skill_md(skill: &IndexSkill) -> bool {
    skill.files.iter().any(|file| file == "SKILL.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_index_schema() {
        let index: Index = serde_json::from_str(
            r#"{"skills":[{"name":"a","files":["SKILL.md","refs/x.md"]},{"name":"b","files":["SKILL.md"],"version":"1.2"}]}"#,
        )
        .unwrap();
        assert_eq!(index.skills.len(), 2);
        assert!(has_skill_md(&index.skills[0]));
        assert_eq!(index.skills[1].version.as_deref(), Some("1.2"));
    }
}
