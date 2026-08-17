//! Port of `reference/packages/opencode/src/patch/index.ts`.
//!
//! Parses the `*** Begin Patch` / `*** End Patch` file-oriented diff format and
//! derives new file contents from `@@`-chunks, with the reference's fuzzy
//! `seekSequence` matching (exact → rstrip → trim → unicode-normalized).

/// BOM helpers from `reference/packages/opencode/src/util/bom.ts`.
pub mod bom {
    /// `Bom.split` from `reference/packages/opencode/src/util/bom.ts:4`.
    pub fn split(text: &str) -> (bool, String) {
        if text.starts_with('\u{feff}') {
            (true, text[3..].to_string())
        } else {
            (false, text.to_string())
        }
    }

    /// `Bom.join` from `reference/packages/opencode/src/util/bom.ts:12`.
    pub fn join(text: &str, bom: bool) -> String {
        let stripped = split(text).1;
        if !bom {
            return stripped;
        }
        format!("\u{feff}{stripped}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Hunk {
    Add {
        path: String,
        contents: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_path: Option<String>,
        chunks: Vec<UpdateFileChunk>,
    },
}

impl Hunk {
    pub fn path(&self) -> &str {
        match self {
            Hunk::Add { path, .. } | Hunk::Delete { path } | Hunk::Update { path, .. } => path,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Hunk::Add { .. } => "add",
            Hunk::Delete { .. } => "delete",
            Hunk::Update { .. } => "update",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateFileChunk {
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    pub change_context: Option<String>,
    pub is_end_of_file: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApplyPatchFileUpdate {
    pub unified_diff: String,
    pub content: String,
    pub bom: bool,
}

/// `stripHeredoc` from `reference/packages/opencode/src/patch/index.ts:176`.
///
/// The reference uses a regex with a `\1` backreference; the Rust `regex`
/// crate has none, so this is a manual equivalent. A heredoc spans the whole
/// input: `<<'delim'\n...\ndelim` (optionally prefixed by `cat `).
fn strip_heredoc(input: &str) -> String {
    let lines: Vec<&str> = input.split('\n').collect();
    let Some(first) = lines.first() else {
        return input.to_string();
    };
    let re = regex::Regex::new(r#"^(?:cat\s+)?<<['\"]?(\w+)['\"]?\s*$"#).unwrap();
    let Some(captures) = re.captures(first) else {
        return input.to_string();
    };
    let delimiter = captures.get(1).unwrap().as_str();
    let mut content = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(1) {
        let trimmed = line.trim();
        if trimmed == delimiter {
            let trailing = lines[index + 1..].iter().all(|line| line.trim().is_empty());
            if trailing {
                return content.join("\n");
            }
            return input.to_string();
        }
        content.push((*line).to_string());
    }
    input.to_string()
}

/// Extract the body of an `apply_patch <<'delim'` heredoc embedded in a shell
/// script (backreference-free equivalent of the reference regex).
fn extract_apply_patch_heredoc(script: &str) -> Option<String> {
    let lines: Vec<&str> = script.split('\n').collect();
    let re = regex::Regex::new(r#"^.*apply_patch\s*<<['\"]?(\w+)['\"]?\s*$"#).unwrap();
    for (index, line) in lines.iter().enumerate() {
        let Some(captures) = re.captures(line) else {
            continue;
        };
        let delimiter = captures.get(1).unwrap().as_str();
        let mut content = Vec::new();
        for next in &lines[index + 1..] {
            if next.trim_end() == delimiter {
                return Some(content.join("\n"));
            }
            content.push((*next).to_string());
        }
        return None;
    }
    None
}

/// `parsePatch` from `reference/packages/opencode/src/patch/index.ts:185`.
pub fn parse_patch(patch_text: &str) -> Result<Vec<Hunk>, String> {
    let cleaned = strip_heredoc(patch_text.trim());
    let lines: Vec<&str> = cleaned.split('\n').collect();
    let begin_marker = "*** Begin Patch";
    let end_marker = "*** End Patch";

    let begin_idx = lines.iter().position(|line| line.trim() == begin_marker);
    let end_idx = lines.iter().position(|line| line.trim() == end_marker);
    let (begin_idx, end_idx) = match (begin_idx, end_idx) {
        (Some(begin), Some(end)) if begin < end => (begin, end),
        _ => return Err("Invalid patch format: missing Begin/End markers".to_string()),
    };

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut i = begin_idx + 1;
    while i < end_idx {
        let header = parse_patch_header(&lines, i);
        let header = match header {
            Some(header) => header,
            None => {
                i += 1;
                continue;
            }
        };

        if lines[i].starts_with("*** Add File:") {
            let (content, next_idx) = parse_add_file_content(&lines, header.next_idx);
            hunks.push(Hunk::Add {
                path: header.file_path,
                contents: content,
            });
            i = next_idx;
        } else if lines[i].starts_with("*** Delete File:") {
            hunks.push(Hunk::Delete {
                path: header.file_path,
            });
            i = header.next_idx;
        } else if lines[i].starts_with("*** Update File:") {
            let (chunks, next_idx) = parse_update_file_chunks(&lines, header.next_idx);
            hunks.push(Hunk::Update {
                path: header.file_path,
                move_path: header.move_path,
                chunks,
            });
            i = next_idx;
        } else {
            i += 1;
        }
    }
    Ok(hunks)
}

struct PatchHeader {
    file_path: String,
    move_path: Option<String>,
    next_idx: usize,
}

/// `parsePatchHeader` from `reference/packages/opencode/src/patch/index.ts:70`.
fn parse_patch_header(lines: &[&str], start_idx: usize) -> Option<PatchHeader> {
    let line = lines[start_idx];
    if let Some(path) = line.strip_prefix("*** Add File:") {
        let file_path = path.trim();
        return (!file_path.is_empty()).then(|| PatchHeader {
            file_path: file_path.to_string(),
            move_path: None,
            next_idx: start_idx + 1,
        });
    }
    if let Some(path) = line.strip_prefix("*** Delete File:") {
        let file_path = path.trim();
        return (!file_path.is_empty()).then(|| PatchHeader {
            file_path: file_path.to_string(),
            move_path: None,
            next_idx: start_idx + 1,
        });
    }
    if let Some(path) = line.strip_prefix("*** Update File:") {
        let file_path = path.trim();
        let mut move_path: Option<String> = None;
        let mut next_idx = start_idx + 1;
        if next_idx < lines.len() && lines[next_idx].starts_with("*** Move to:") {
            move_path = Some(lines[next_idx]["*** Move to:".len()..].trim().to_string());
            next_idx += 1;
        }
        if file_path.is_empty() {
            return None;
        }
        return Some(PatchHeader {
            file_path: file_path.to_string(),
            move_path,
            next_idx,
        });
    }
    None
}

/// `parseUpdateFileChunks` from `reference/packages/opencode/src/patch/index.ts:103`.
fn parse_update_file_chunks(lines: &[&str], start_idx: usize) -> (Vec<UpdateFileChunk>, usize) {
    let mut chunks: Vec<UpdateFileChunk> = Vec::new();
    let mut i = start_idx;
    while i < lines.len() && !lines[i].starts_with("***") {
        if lines[i].starts_with("@@") {
            let context_line = lines[i][2..].trim().to_string();
            i += 1;
            let mut old_lines: Vec<String> = Vec::new();
            let mut new_lines: Vec<String> = Vec::new();
            let mut is_end_of_file = false;
            while i < lines.len() && !lines[i].starts_with("@@") && !lines[i].starts_with("***") {
                let change_line = lines[i];
                if change_line == "*** End of File" {
                    is_end_of_file = true;
                    i += 1;
                    break;
                }
                if let Some(content) = change_line.strip_prefix(' ') {
                    old_lines.push(content.to_string());
                    new_lines.push(content.to_string());
                } else if let Some(content) = change_line.strip_prefix('-') {
                    old_lines.push(content.to_string());
                } else if let Some(content) = change_line.strip_prefix('+') {
                    new_lines.push(content.to_string());
                }
                i += 1;
            }
            chunks.push(UpdateFileChunk {
                old_lines,
                new_lines,
                change_context: (!context_line.is_empty()).then_some(context_line),
                is_end_of_file,
            });
        } else {
            i += 1;
        }
    }
    (chunks, i)
}

/// `parseAddFileContent` from `reference/packages/opencode/src/patch/index.ts:157`.
fn parse_add_file_content(lines: &[&str], start_idx: usize) -> (String, usize) {
    let mut content = String::new();
    let mut i = start_idx;
    while i < lines.len() && !lines[i].starts_with("***") {
        if let Some(rest) = lines[i].strip_prefix('+') {
            content.push_str(rest);
            content.push('\n');
        }
        i += 1;
    }
    if content.ends_with('\n') {
        content.pop();
    }
    (content, i)
}

/// `deriveNewContentsFromChunks` from `reference/packages/opencode/src/patch/index.ts:307`.
pub fn derive_new_contents_from_chunks(
    file_path: &str,
    chunks: &[UpdateFileChunk],
    original_text: &str,
) -> Result<ApplyPatchFileUpdate, String> {
    let (original_bom, original_text) = bom::split(original_text);
    let mut original_lines: Vec<String> =
        original_text.split('\n').map(|s| s.to_string()).collect();
    if original_lines.last().map(|line| line.is_empty()) == Some(true) {
        original_lines.pop();
    }

    let replacements = compute_replacements(file_path, &original_lines, chunks)?;
    let mut new_lines = apply_replacements(&original_lines, &replacements);
    let needs_trailing = new_lines.is_empty() || new_lines.last().map(String::as_str) != Some("");
    if needs_trailing {
        new_lines.push(String::new());
    }

    let new_content = new_lines.join("\n");
    let unified_diff = generate_unified_diff(&original_text, &new_content);
    let (next_bom, _) = bom::split(&new_content);
    Ok(ApplyPatchFileUpdate {
        unified_diff,
        content: new_content,
        bom: original_bom || next_bom,
    })
}

type Replacement = (usize, usize, Vec<String>);

/// `computeReplacements` from `reference/packages/opencode/src/patch/index.ts:342`.
fn compute_replacements(
    file_path: &str,
    original_lines: &[String],
    chunks: &[UpdateFileChunk],
) -> Result<Vec<Replacement>, String> {
    let mut replacements: Vec<Replacement> = Vec::new();
    let mut line_index = 0;
    for chunk in chunks {
        if let Some(context) = &chunk.change_context {
            let context_idx = seek_sequence(
                original_lines,
                std::slice::from_ref(context),
                line_index,
                false,
            );
            let context_idx = context_idx
                .ok_or_else(|| format!("Failed to find context '{context}' in {file_path}"))?;
            line_index = context_idx + 1;
        }

        if chunk.old_lines.is_empty() {
            let insertion_idx = if original_lines.last().map(|line| line.is_empty()) == Some(true) {
                original_lines.len() - 1
            } else {
                original_lines.len()
            };
            replacements.push((insertion_idx, 0, chunk.new_lines.clone()));
            continue;
        }

        let mut pattern = chunk.old_lines.clone();
        let mut new_slice = chunk.new_lines.clone();
        let mut found = seek_sequence(original_lines, &pattern, line_index, chunk.is_end_of_file);

        if found.is_none() && pattern.last().map(|line| line.is_empty()) == Some(true) {
            pattern.pop();
            if new_slice.last().map(|line| line.is_empty()) == Some(true) {
                new_slice.pop();
            }
            found = seek_sequence(original_lines, &pattern, line_index, chunk.is_end_of_file);
        }

        if let Some(found) = found {
            replacements.push((found, pattern.len(), new_slice));
            line_index = found + pattern.len();
        } else {
            return Err(format!(
                "Failed to find expected lines in {file_path}:\n{}",
                chunk.old_lines.join("\n")
            ));
        }
    }
    replacements.sort_by_key(|replacement| replacement.0);
    Ok(replacements)
}

/// `applyReplacements` from `reference/packages/opencode/src/patch/index.ts:398`.
fn apply_replacements(lines: &[String], replacements: &[Replacement]) -> Vec<String> {
    let mut result: Vec<String> = lines.to_vec();
    for (start_idx, old_len, new_segment) in replacements.iter().rev() {
        result.drain(*start_idx..start_idx + old_len);
        for (j, line) in new_segment.iter().enumerate() {
            result.insert(start_idx + j, line.clone());
        }
    }
    result
}

/// `normalizeUnicode` from `reference/packages/opencode/src/patch/index.ts:418`.
fn normalize_unicode(text: &str) -> String {
    text.replace(['\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'], "'")
        .replace(['\u{201C}', '\u{201D}', '\u{201E}', '\u{201F}'], "\"")
        .replace(
            [
                '\u{2010}', '\u{2011}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{2015}',
            ],
            "-",
        )
        .replace('\u{2026}', "...")
        .replace('\u{00A0}', " ")
}

type Comparator = dyn Fn(&str, &str) -> bool;

/// `tryMatch` from `reference/packages/opencode/src/patch/index.ts:429`.
fn try_match(
    lines: &[String],
    pattern: &[String],
    start_index: usize,
    compare: &Comparator,
    eof: bool,
) -> Option<usize> {
    if pattern.is_empty() || lines.len() < pattern.len() {
        return None;
    }
    if eof {
        let from_end = lines.len() - pattern.len();
        if from_end >= start_index {
            let mut matches = true;
            for (j, pattern_line) in pattern.iter().enumerate() {
                if !compare(&lines[from_end + j], pattern_line) {
                    matches = false;
                    break;
                }
            }
            if matches {
                return Some(from_end);
            }
        }
    }
    for i in start_index..=lines.len() - pattern.len() {
        let mut matches = true;
        for (j, pattern_line) in pattern.iter().enumerate() {
            if !compare(&lines[i + j], pattern_line) {
                matches = false;
                break;
            }
        }
        if matches {
            return Some(i);
        }
    }
    None
}

/// `seekSequence` from `reference/packages/opencode/src/patch/index.ts:460`.
fn seek_sequence(
    lines: &[String],
    pattern: &[String],
    start_index: usize,
    eof: bool,
) -> Option<usize> {
    if pattern.is_empty() {
        return None;
    }
    let exact = try_match(lines, pattern, start_index, &|a, b| a == b, eof);
    if exact.is_some() {
        return exact;
    }
    let rstrip = try_match(
        lines,
        pattern,
        start_index,
        &|a, b| a.trim_end() == b.trim_end(),
        eof,
    );
    if rstrip.is_some() {
        return rstrip;
    }
    let trim = try_match(
        lines,
        pattern,
        start_index,
        &|a, b| a.trim() == b.trim(),
        eof,
    );
    if trim.is_some() {
        return trim;
    }
    try_match(
        lines,
        pattern,
        start_index,
        &|a, b| normalize_unicode(a.trim()) == normalize_unicode(b.trim()),
        eof,
    )
}

/// `generateUnifiedDiff` from `reference/packages/opencode/src/patch/index.ts:486`.
fn generate_unified_diff(old_content: &str, new_content: &str) -> String {
    let old_lines: Vec<&str> = old_content.split('\n').collect();
    let new_lines: Vec<&str> = new_content.split('\n').collect();
    let mut diff = "@@ -1 +1 @@\n".to_string();
    let max_len = old_lines.len().max(new_lines.len());
    let mut has_changes = false;
    for i in 0..max_len {
        let old_line = old_lines.get(i).copied().unwrap_or("");
        let new_line = new_lines.get(i).copied().unwrap_or("");
        if old_line != new_line {
            if !old_line.is_empty() {
                diff.push_str(&format!("-{old_line}\n"));
            }
            if !new_line.is_empty() {
                diff.push_str(&format!("+{new_line}\n"));
            }
            has_changes = true;
        } else if !old_line.is_empty() {
            diff.push_str(&format!(" {old_line}\n"));
        }
    }
    if has_changes {
        diff
    } else {
        String::new()
    }
}

/// `maybeParseApplyPatch` from `reference/packages/opencode/src/patch/index.ts:244`.
pub fn maybe_parse_apply_patch(argv: &[String]) -> MaybeApplyPatch {
    const APPLY_PATCH_COMMANDS: [&str; 2] = ["apply_patch", "applypatch"];
    if argv.len() == 2 && APPLY_PATCH_COMMANDS.contains(&argv[0].as_str()) {
        return match parse_patch(&argv[1]) {
            Ok(hunks) => MaybeApplyPatch::Body {
                hunks,
                patch: argv[1].clone(),
            },
            Err(error) => MaybeApplyPatch::PatchParseError { error },
        };
    }
    if argv.len() == 3 && argv[0] == "bash" && argv[1] == "-lc" {
        let script = &argv[2];
        if let Some(patch_content) = extract_apply_patch_heredoc(script) {
            return match parse_patch(&patch_content) {
                Ok(hunks) => MaybeApplyPatch::Body {
                    hunks,
                    patch: patch_content,
                },
                Err(error) => MaybeApplyPatch::PatchParseError { error },
            };
        }
    }
    MaybeApplyPatch::NotApplyPatch
}

pub enum MaybeApplyPatch {
    Body { hunks: Vec<Hunk>, patch: String },
    PatchParseError { error: String },
    NotApplyPatch,
}

/// `maybeParseApplyPatchVerified` from `reference/packages/opencode/src/patch/index.ts:575`.
pub fn maybe_parse_apply_patch_verified(
    argv: &[String],
    cwd: &str,
) -> Result<MaybeApplyPatchVerified, String> {
    if argv.len() == 1 && parse_patch(&argv[0]).is_ok() {
        return Err("ImplicitInvocation".to_string());
    }
    let result = maybe_parse_apply_patch(argv);
    match result {
        MaybeApplyPatch::Body { hunks, patch } => {
            let mut changes: Vec<(String, ApplyPatchFileChange)> = Vec::new();
            for hunk in &hunks {
                let (path, move_path) = match hunk {
                    Hunk::Update {
                        path, move_path, ..
                    } => (path.as_str(), move_path.as_deref()),
                    hunk => (hunk.path(), None),
                };
                let resolved_path = resolve_join(cwd, path);
                match hunk {
                    Hunk::Add { contents, .. } => {
                        changes.push((
                            resolved_path,
                            ApplyPatchFileChange::Add {
                                content: contents.clone(),
                            },
                        ));
                    }
                    Hunk::Delete { .. } => {
                        let content = std::fs::read_to_string(&resolved_path).map_err(|error| {
                            format!("Failed to read file for deletion: {error}")
                        })?;
                        changes.push((resolved_path, ApplyPatchFileChange::Delete { content }));
                    }
                    Hunk::Update { chunks, .. } => {
                        let update_path = resolve_join(cwd, hunk.path());
                        let original_text =
                            std::fs::read_to_string(&update_path).map_err(|error| {
                                format!("Failed to read file {update_path}: {error}")
                            })?;
                        let file_update =
                            derive_new_contents_from_chunks(&update_path, chunks, &original_text)?;
                        changes.push((
                            resolved_path,
                            ApplyPatchFileChange::Update {
                                unified_diff: file_update.unified_diff,
                                move_path: move_path
                                    .as_ref()
                                    .map(|target| resolve_join(cwd, target)),
                                new_content: file_update.content,
                            },
                        ));
                    }
                }
            }
            Ok(MaybeApplyPatchVerified::Body(ApplyPatchAction {
                changes,
                patch,
                cwd: cwd.to_string(),
            }))
        }
        MaybeApplyPatch::PatchParseError { error } => {
            Ok(MaybeApplyPatchVerified::CorrectnessError { error })
        }
        MaybeApplyPatch::NotApplyPatch => Ok(MaybeApplyPatchVerified::NotApplyPatch),
    }
}

fn resolve_join(cwd: &str, path: &str) -> String {
    let joined = std::path::Path::new(cwd).join(path);
    joined.to_string_lossy().to_string()
}

pub enum MaybeApplyPatchVerified {
    Body(ApplyPatchAction),
    CorrectnessError { error: String },
    NotApplyPatch,
}

pub struct ApplyPatchAction {
    pub changes: Vec<(String, ApplyPatchFileChange)>,
    pub patch: String,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApplyPatchFileChange {
    Add {
        content: String,
    },
    Delete {
        content: String,
    },
    Update {
        unified_diff: String,
        move_path: Option<String>,
        new_content: String,
    },
}

/// `applyHunksToFiles` from `reference/packages/opencode/src/patch/index.ts:514`.
pub fn apply_hunks_to_files(hunks: &[Hunk], cwd: &str) -> Result<AffectedPaths, String> {
    if hunks.is_empty() {
        return Err("No files were modified.".to_string());
    }
    let mut added: Vec<String> = Vec::new();
    let mut modified: Vec<String> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();

    for hunk in hunks {
        match hunk {
            Hunk::Add { path, contents } => {
                let target = resolve_join(cwd, path);
                write_with_dirs(&target, contents)?;
                added.push(path.clone());
            }
            Hunk::Delete { path } => {
                let target = resolve_join(cwd, path);
                std::fs::remove_file(&target)
                    .map_err(|error| format!("failed to delete {target}: {error}"))?;
                deleted.push(path.clone());
            }
            Hunk::Update {
                path,
                move_path,
                chunks,
            } => {
                let target = resolve_join(cwd, path);
                let original_text = std::fs::read_to_string(&target)
                    .map_err(|error| format!("failed to read {target}: {error}"))?;
                let file_update = derive_new_contents_from_chunks(path, chunks, &original_text)?;
                if let Some(move_path) = move_path {
                    let destination = resolve_join(cwd, move_path);
                    write_with_dirs(
                        &destination,
                        &bom::join(&file_update.content, file_update.bom),
                    )?;
                    std::fs::remove_file(&target)
                        .map_err(|error| format!("failed to remove {target}: {error}"))?;
                    modified.push(destination);
                } else {
                    write_with_dirs(&target, &bom::join(&file_update.content, file_update.bom))?;
                    modified.push(target);
                }
            }
        }
    }
    Ok(AffectedPaths {
        added,
        modified,
        deleted,
    })
}

fn write_with_dirs(path: &str, content: &str) -> Result<(), String> {
    let target = std::path::Path::new(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create dirs {parent:?}: {error}"))?;
    }
    std::fs::write(target, content).map_err(|error| format!("failed to write {path}: {error}"))
}

pub struct AffectedPaths {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

/// `applyPatch` from `reference/packages/opencode/src/patch/index.ts:564`.
pub fn apply_patch(patch_text: &str, cwd: &str) -> Result<AffectedPaths, String> {
    let hunks = parse_patch(patch_text)?;
    apply_hunks_to_files(&hunks, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_add_update_delete() {
        let patch = "*** Begin Patch\n*** Add File: a.txt\n+hello\n*** Update File: b.txt\n@@ context\n- old\n+ new\n*** Delete File: c.txt\n*** End Patch";
        let hunks = parse_patch(patch).unwrap();
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].kind(), "add");
        assert_eq!(hunks[1].kind(), "update");
        assert_eq!(hunks[2].kind(), "delete");
    }

    #[test]
    fn parses_move_directive() {
        let patch = "*** Begin Patch\n*** Update File: a.txt\n*** Move to: b.txt\n@@ c\n-old\n+new\n*** End Patch";
        let hunks = parse_patch(patch).unwrap();
        match &hunks[0] {
            Hunk::Update {
                move_path: Some(target),
                ..
            } => assert_eq!(target, "b.txt"),
            other => panic!("expected update hunk, got {other:?}"),
        }
    }

    #[test]
    fn missing_markers_is_error() {
        assert!(parse_patch("*** Add File: a.txt\n+hello").is_err());
    }

    #[test]
    fn derives_contents_from_chunks() {
        let update = derive_new_contents_from_chunks("f.txt", &[], "line1\nline2\n").unwrap();
        assert_eq!(update.content, "line1\nline2\n");
    }

    #[test]
    fn derives_with_fuzzy_whitespace_match() {
        let chunks = vec![UpdateFileChunk {
            old_lines: vec!["  hello  ".to_string()],
            new_lines: vec!["world".to_string()],
            change_context: None,
            is_end_of_file: false,
        }];
        let update = derive_new_contents_from_chunks("f.txt", &chunks, "hello\n").unwrap();
        assert_eq!(update.content, "world\n");
    }

    #[test]
    fn unicode_normalization_matches() {
        let chunks = vec![UpdateFileChunk {
            old_lines: vec!["\u{201C}quoted\u{201D}".to_string()],
            new_lines: vec!["plain".to_string()],
            change_context: None,
            is_end_of_file: false,
        }];
        let update = derive_new_contents_from_chunks("f.txt", &chunks, "\"quoted\"\n").unwrap();
        assert_eq!(update.content, "plain\n");
    }
}
