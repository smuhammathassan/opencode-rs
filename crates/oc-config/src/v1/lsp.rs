// From reference/packages/core/src/v1/config/lsp.ts

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Builtin server IDs from `builtinServerIds`; custom servers must declare
/// `extensions`.
pub const BUILTIN_SERVER_IDS: [&str; 38] = [
    "deno",
    "typescript",
    "vue",
    "eslint",
    "oxlint",
    "biome",
    "gopls",
    "ruby-lsp",
    "ty",
    "pyright",
    "elixir-ls",
    "zls",
    "csharp",
    "razor",
    "fsharp",
    "sourcekit-lsp",
    "rust",
    "clangd",
    "svelte",
    "astro",
    "jdtls",
    "kotlin-ls",
    "yaml-ls",
    "lua-ls",
    "php intelephense",
    "prisma",
    "dart",
    "ocaml-lsp",
    "bash",
    "terraform",
    "texlab",
    "dockerfile",
    "gleam",
    "clojure-lsp",
    "nixd",
    "tinymist",
    "haskell-language-server",
    "julials",
];

/// `Entry` = `Union([Disabled, Server])` where `Disabled` is `{ disabled: true }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Disabled(Disabled),
    Server(Server),
}

/// `{ disabled: true }` — the tagged marker for disabling a builtin server.
/// Matches `Schema.Literal(true)`: `{ disabled: false }` must fall through to
/// `Server`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Disabled {
    pub disabled: bool,
}

impl<'de> Deserialize<'de> for Disabled {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = bool::deserialize(deserializer)?;
        if value {
            Ok(Disabled { disabled: true })
        } else {
            Err(serde::de::Error::custom("disabled must be true"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialization: Option<IndexMap<String, Value>>,
}

/// `Info` = `Union([Boolean, Record<String, Entry>])` with a check that custom
/// servers declare extensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Info {
    Bool(bool),
    ByLanguage(IndexMap<String, Entry>),
}

/// Replicates `requiresExtensionsForCustomServers`.
pub fn requires_extensions(servers: &IndexMap<String, Entry>) -> Option<String> {
    for (id, config) in servers {
        match config {
            Entry::Disabled(_) => continue,
            Entry::Server(server) => {
                if BUILTIN_SERVER_IDS.contains(&id.as_str()) {
                    continue;
                }
                if let Some(extensions) = &server.extensions {
                    if !extensions.is_empty() {
                        continue;
                    }
                }
                return Some(format!(
                    "For custom LSP servers, 'extensions' array is required (server \"{id}\")."
                ));
            }
        }
    }
    None
}
