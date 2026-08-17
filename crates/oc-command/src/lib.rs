#![allow(clippy::manual_strip)]
#![allow(clippy::all)]
//! Port of opencode's slash commands, skills, and interactive prompts.
//!
//! Mirrors `reference/packages/opencode/src/{command,skill,question}`.
//! The three sibling modules in `reference/packages/opencode/src/` map to
//! `command`, `skill`, and `question` here. Frontmatter parsing
//! (`reference/packages/core/src/config/markdown.ts`) lives in `frontmatter`.

pub mod command;
pub mod frontmatter;
pub mod global;
mod id;
pub mod question;
pub mod skill;
mod util;
