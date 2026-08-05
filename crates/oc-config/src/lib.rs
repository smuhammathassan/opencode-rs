//! `oc-config` — opencode.json/.jsonc parsing, validation, and deep merge.
//!
//! A 1:1 Rust port of the reference config layer:
//! - `reference/packages/core/src/config/` (v2 `ConfigV2.*` schema types)
//! - `reference/packages/core/src/v1/config/` (the `ConfigV1.Info` opencode.json schema)
//! - `reference/packages/opencode/src/config/` (parsing, merging, discovery)

// Field names intentionally match the reference's camelCase JSON keys.
#![allow(non_snake_case)]

pub mod entry_name;
pub mod error;
pub mod glob;
pub mod jsnum;
pub mod load;
pub mod merge;
pub mod parse;
pub mod paths;
pub mod v1;
pub mod v2;
pub mod variable;

pub use error::{ConfigError, Issue, Result};
pub use load::{
    load_config, load_file, load_global, load_instance_state, LoadOptions, PluginOrigin, Scope,
};
pub use parse::schema as parse_schema;
pub use v1::Info as Config;
