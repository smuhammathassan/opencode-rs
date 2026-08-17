//! Port of `reference/packages/opencode/src/tool/edit.ts`.
//!
//! Implements the exact-replacement chain (`SimpleReplacer` through
//! `MultiOccurrenceReplacer`) and the `replace` dispatcher verbatim.

use crate::diff::{count_lines, create_two_files_patch, diff_lines, trim_diff};
use crate::model::{ExecuteResult, PermissionRequest, ToolContext, ToolError};
use crate::prompts;
use crate::schema::{opt_prop, prop, Schema};
use crate::tool::external_directory;
use crate::util::bom;

/// `normalizeLineEndings` from `reference/packages/opencode/src/tool/edit.ts:22`.
pub fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// `detectLineEnding` from `reference/packages/opencode/src/tool/edit.ts:26`.
pub fn detect_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// `convertToLineEnding` from `reference/packages/opencode/src/tool/edit.ts:30`.
pub fn convert_to_line_ending(text: &str, ending: &str) -> String {
    if ending == "\n" {
        text.to_string()
    } else {
        text.replace('\n', "\r\n")
    }
}

/// `Parameters` from `reference/packages/opencode/src/tool/edit.ts:47`.
pub fn parameters() -> Schema {
    Schema::struct_(
        vec![
            prop(
                "filePath",
                Schema::string("The absolute path to the file to modify"),
            ),
            prop("oldString", Schema::string("The text to replace")),
            prop(
                "newString",
                Schema::string("The text to replace it with (must be different from oldString)"),
            ),
            opt_prop(
                "replaceAll",
                Schema::boolean("Replace all occurrences of oldString (default false)"),
            ),
        ],
        "edit",
    )
}

/// `EditTool` from `reference/packages/opencode/src/tool/edit.ts:58`.
pub fn def() -> crate::tool::tool::Def {
    crate::tool::tool::def("edit", prompts::EDIT, parameters(), |args, ctx| {
        let filepath = args
            .get("filePath")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if filepath.is_empty() {
            return Err(ToolError::Other("filePath is required".to_string()));
        }
        let old_string = args
            .get("oldString")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let new_string = args
            .get("newString")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let replace_all = args
            .get("replaceAll")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if old_string == new_string {
            return Err(ToolError::Other(
                "No changes to apply: oldString and newString are identical.".to_string(),
            ));
        }

        let instance = ctx.instance.clone().ok_or_else(|| {
            ToolError::Other("InstanceState.context is required for the edit tool".to_string())
        })?;
        let filepath = if std::path::Path::new(&filepath).is_absolute() {
            filepath
        } else {
            std::path::Path::new(&instance.directory)
                .join(&filepath)
                .to_string_lossy()
                .to_string()
        };
        external_directory::assert_external_directory_file(ctx, &filepath)?;

        let (mut content_old, mut content_new, mut diff, desired_bom) = apply_edit(
            &filepath,
            &old_string,
            &new_string,
            replace_all,
            ctx,
            &instance.worktree,
        )?;

        let mut additions = 0;
        let mut deletions = 0;
        for part in diff_lines(&content_old, &content_new) {
            if part.added {
                additions += part.count;
            }
            if part.removed {
                deletions += part.count;
            }
        }
        let filediff = serde_json::json!({
            "file": filepath,
            "patch": diff,
            "additions": additions,
            "deletions": deletions,
        });

        ctx.metadata(crate::model::Metadata {
            title: None,
            metadata: serde_json::json!({
                "diff": diff,
                "filediff": filediff,
                "diagnostics": {},
            }),
        })?;

        let mut output = "Edit applied successfully.".to_string();
        // TODO(integration): run formatter (`format.file`), publish edit
        // events, and surface LSP diagnostics from the real LSP runtime.
        if let Some(block) = ctx.services.lsp_diagnostics(&filepath)? {
            output.push_str(&format!(
                "\n\nLSP errors detected in this file, please fix:\n{block}"
            ));
        }

        let _ = &mut content_old;
        let _ = &mut content_new;
        let _ = &mut diff;
        let _ = desired_bom;

        Ok(ExecuteResult {
            title: crate::util::path_relative(&instance.worktree, &filepath),
            metadata: serde_json::json!({
                "diagnostics": {},
                "diff": filediff["patch"],
                "filediff": filediff,
            }),
            output,
            attachments: None,
        })
    })
}

fn apply_edit(
    filepath: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
    ctx: &mut ToolContext,
    worktree: &str,
) -> Result<(String, String, String, bool), ToolError> {
    if old_string.is_empty() {
        let existed = std::path::Path::new(filepath).exists();
        if existed {
            return Err(ToolError::Other(
                "oldString cannot be empty when editing an existing file. Provide the exact text to replace, or use write for an intentional full-file replacement.".to_string(),
            ));
        }
        let next = bom::split(new_string);
        let desired_bom = next.0;
        let content_old = String::new();
        let content_new = next.1;
        let diff = trim_diff(&create_two_files_patch(
            filepath,
            filepath,
            "",
            &content_new,
        ));
        ctx.ask(PermissionRequest {
            permission: "edit".to_string(),
            patterns: vec![crate::util::path_relative(worktree, filepath)],
            always: vec!["*".to_string()],
            metadata: serde_json::json!({
                "filepath": filepath,
                "diff": diff,
            }),
        })?;
        crate::tool::write::write_with_dirs(filepath, &bom::join(&content_new, desired_bom))?;
        return Ok((content_old, content_new, diff, desired_bom));
    }

    let info = std::fs::symlink_metadata(filepath)
        .map_err(|_| ToolError::Other(format!("File {filepath} not found")))?;
    if info.is_dir() {
        return Err(ToolError::Other(format!(
            "Path is a directory, not a file: {filepath}"
        )));
    }
    let (source_bom, source_text) = bom::read_file(filepath)
        .map_err(|error| ToolError::Other(format!("Unable to edit {filepath}: {error}")))?;
    let content_old = source_text.clone();

    let ending = detect_line_ending(&content_old);
    let old = convert_to_line_ending(&normalize_line_endings(old_string), ending);
    let replacement = convert_to_line_ending(&normalize_line_endings(new_string), ending);

    let next = bom::split(&replace(&content_old, &old, &replacement, replace_all)?);
    let desired_bom = source_bom || next.0;
    let content_new = next.1;

    let diff = trim_diff(&create_two_files_patch(
        filepath,
        filepath,
        &normalize_line_endings(&content_old),
        &normalize_line_endings(&content_new),
    ));
    ctx.ask(PermissionRequest {
        permission: "edit".to_string(),
        patterns: vec![crate::util::path_relative(worktree, filepath)],
        always: vec!["*".to_string()],
        metadata: serde_json::json!({
            "filepath": filepath,
            "diff": diff,
        }),
    })?;
    crate::tool::write::write_with_dirs(filepath, &bom::join(&content_new, desired_bom))?;
    let diff = trim_diff(&create_two_files_patch(
        filepath,
        filepath,
        &normalize_line_endings(&content_old),
        &normalize_line_endings(&content_new),
    ));
    Ok((content_old, content_new, diff, desired_bom))
}

type Replacer = Box<dyn Fn(&str, &str) -> Vec<String> + Send + Sync>;

/// `SimpleReplacer` from `reference/packages/opencode/src/tool/edit.ts:244`.
fn simple_replacer(_content: &str, find: &str) -> Vec<String> {
    vec![find.to_string()]
}

/// `LineTrimmedReplacer` from `reference/packages/opencode/src/tool/edit.ts:248`.
fn line_trimmed_replacer(content: &str, find: &str) -> Vec<String> {
    let original_lines: Vec<&str> = content.split('\n').collect();
    let mut search_lines: Vec<&str> = find.split('\n').collect();
    if search_lines.last() == Some(&"") {
        search_lines.pop();
    }
    if search_lines.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if original_lines.len() < search_lines.len() {
        return out;
    }
    for i in 0..=original_lines.len() - search_lines.len() {
        let mut matches = true;
        for j in 0..search_lines.len() {
            if original_lines[i + j].trim() != search_lines[j].trim() {
                matches = false;
                break;
            }
        }
        if matches {
            let mut match_start_index = 0;
            for k in 0..i {
                match_start_index += original_lines[k].len() + 1;
            }
            let mut match_end_index = match_start_index;
            for k in 0..search_lines.len() {
                match_end_index += original_lines[i + k].len();
                if k < search_lines.len() - 1 {
                    match_end_index += 1;
                }
            }
            out.push(content[match_start_index..match_end_index].to_string());
        }
    }
    out
}

/// `BlockAnchorReplacer` from `reference/packages/opencode/src/tool/edit.ts:288`.
fn block_anchor_replacer(content: &str, find: &str) -> Vec<String> {
    let original_lines: Vec<&str> = content.split('\n').collect();
    let mut search_lines: Vec<&str> = find.split('\n').collect();
    if search_lines.len() < 3 {
        return Vec::new();
    }
    if search_lines.last() == Some(&"") {
        search_lines.pop();
    }
    if search_lines.is_empty() {
        return Vec::new();
    }
    let first_line_search = search_lines[0].trim();
    let last_line_search = search_lines[search_lines.len() - 1].trim();
    let search_block_size = search_lines.len();
    let max_line_delta = ((search_block_size as f64) * 0.25).floor().max(1.0) as usize;

    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for i in 0..original_lines.len() {
        if original_lines[i].trim() != first_line_search {
            continue;
        }
        for j in (i + 2)..original_lines.len() {
            if original_lines[j].trim() == last_line_search {
                let actual_block_size = j - i + 1;
                if actual_block_size.abs_diff(search_block_size) <= max_line_delta {
                    candidates.push((i, j));
                }
                break;
            }
        }
    }
    if candidates.is_empty() {
        return Vec::new();
    }

    let levenshtein = levenshtein_distance;
    let mut out = Vec::new();

    if candidates.len() == 1 {
        let (start_line, end_line) = candidates[0];
        let actual_block_size = end_line - start_line + 1;
        let mut similarity = 0.0;
        let lines_to_check = (search_block_size - 2).min(actual_block_size - 2);
        if lines_to_check > 0 {
            for j in 1..(search_block_size - 1).min(actual_block_size - 1) {
                let original_line = original_lines[start_line + j].trim();
                let search_line = search_lines[j].trim();
                let max_len = original_line.len().max(search_line.len());
                if max_len == 0 {
                    continue;
                }
                let distance = levenshtein(original_line, search_line);
                similarity += (1.0 - distance as f64 / max_len as f64) / lines_to_check as f64;
                if similarity >= SINGLE_CANDIDATE_SIMILARITY_THRESHOLD {
                    break;
                }
            }
        } else {
            similarity = 1.0;
        }
        if similarity >= SINGLE_CANDIDATE_SIMILARITY_THRESHOLD {
            out.push(slice_by_lines(
                content,
                &original_lines,
                start_line,
                end_line,
            ));
        }
        return out;
    }

    let mut best_match: Option<(usize, usize)> = None;
    let mut max_similarity = -1.0f64;
    for (start_line, end_line) in candidates {
        let actual_block_size = end_line - start_line + 1;
        let mut similarity = 0.0;
        let lines_to_check = (search_block_size - 2).min(actual_block_size - 2);
        if lines_to_check > 0 {
            for j in 1..(search_block_size - 1).min(actual_block_size - 1) {
                let original_line = original_lines[start_line + j].trim();
                let search_line = search_lines[j].trim();
                let max_len = original_line.len().max(search_line.len());
                if max_len == 0 {
                    continue;
                }
                let distance = levenshtein(original_line, search_line);
                similarity += 1.0 - distance as f64 / max_len as f64;
            }
            similarity /= lines_to_check as f64;
        } else {
            similarity = 1.0;
        }
        if similarity > max_similarity {
            max_similarity = similarity;
            best_match = Some((start_line, end_line));
        }
    }
    if max_similarity >= MULTIPLE_CANDIDATES_SIMILARITY_THRESHOLD {
        if let Some((start_line, end_line)) = best_match {
            out.push(slice_by_lines(
                content,
                &original_lines,
                start_line,
                end_line,
            ));
        }
    }
    out
}

fn slice_by_lines(
    content: &str,
    original_lines: &[&str],
    start_line: usize,
    end_line: usize,
) -> String {
    let mut match_start_index = 0;
    for k in 0..start_line {
        match_start_index += original_lines[k].len() + 1;
    }
    let mut match_end_index = match_start_index;
    for k in start_line..=end_line {
        match_end_index += original_lines[k].len();
        if k < end_line {
            match_end_index += 1;
        }
    }
    content[match_start_index.min(content.len())..match_end_index.min(content.len())].to_string()
}

/// `WhitespaceNormalizedReplacer` from `reference/packages/opencode/src/tool/edit.ts:427`.
fn whitespace_normalized_replacer(content: &str, find: &str) -> Vec<String> {
    let normalize =
        |text: &str| -> String { text.split_whitespace().collect::<Vec<_>>().join(" ") };
    let normalized_find = normalize(find);
    let lines: Vec<&str> = content.split('\n').collect();
    let mut out = Vec::new();

    for line in &lines {
        if normalize(line) == normalized_find {
            out.push((*line).to_string());
        } else {
            let normalized_line = normalize(line);
            if normalized_line.contains(&normalized_find) {
                let words: Vec<&str> = find.split_whitespace().collect();
                if !words.is_empty() {
                    let escaped: Vec<String> =
                        words.iter().map(|word| regex::escape(word)).collect();
                    let pattern = escaped.join("\\s+");
                    if let Ok(re) = regex::Regex::new(&pattern) {
                        if let Some(matched) = re.find(line) {
                            out.push(matched.as_str().to_string());
                        }
                    }
                }
            }
        }
    }

    let find_lines: Vec<&str> = find.split('\n').collect();
    if find_lines.len() > 1 && !lines.is_empty() {
        for i in 0..=lines.len().saturating_sub(find_lines.len()) {
            let block = lines[i..i + find_lines.len()].join("\n");
            if normalize(&block) == normalized_find {
                out.push(block);
            }
        }
    }
    out
}

/// `IndentationFlexibleReplacer` from `reference/packages/opencode/src/tool/edit.ts:471`.
fn indentation_flexible_replacer(content: &str, find: &str) -> Vec<String> {
    let remove_indentation = |text: &str| -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let non_empty: Vec<&&str> = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .collect();
        if non_empty.is_empty() {
            return text.to_string();
        }
        let min_indent = non_empty
            .iter()
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        lines
            .iter()
            .map(|line| {
                if line.trim().is_empty() {
                    line.to_string()
                } else {
                    line[min_indent.min(line.len())..].to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let normalized_find = remove_indentation(find);
    let content_lines: Vec<&str> = content.split('\n').collect();
    let find_lines: Vec<&str> = find.split('\n').collect();
    if find_lines.is_empty() || content_lines.len() < find_lines.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..=content_lines.len() - find_lines.len() {
        let block = content_lines[i..i + find_lines.len()].join("\n");
        if remove_indentation(&block) == normalized_find {
            out.push(block);
        }
    }
    out
}

/// `EscapeNormalizedReplacer` from `reference/packages/opencode/src/tool/edit.ts:499`.
fn escape_normalized_replacer(content: &str, find: &str) -> Vec<String> {
    let unescape = |value: &str| -> String {
        let mut out = String::new();
        let mut chars = value.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('\'') => out.push('\''),
                    Some('"') => out.push('"'),
                    Some('`') => out.push('`'),
                    Some('\\') => out.push('\\'),
                    Some('\n') => out.push('\n'),
                    Some('$') => out.push('$'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(ch);
            }
        }
        out
    };

    let unescaped_find = unescape(find);
    let mut out = Vec::new();
    if content.contains(&unescaped_find) {
        out.push(unescaped_find.clone());
    }

    let lines: Vec<&str> = content.split('\n').collect();
    let find_lines: Vec<&str> = unescaped_find.split('\n').collect();
    if find_lines.is_empty() || lines.len() < find_lines.len() {
        return out;
    }
    for i in 0..=lines.len() - find_lines.len() {
        let block = lines[i..i + find_lines.len()].join("\n");
        if unescape(&block) == unescaped_find {
            out.push(block);
        }
    }
    out
}

/// `MultiOccurrenceReplacer` from `reference/packages/opencode/src/tool/edit.ts:548`.
fn multi_occurrence_replacer(content: &str, find: &str) -> Vec<String> {
    if find.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start_index = 0;
    while let Some(index) = content[start_index..].find(find) {
        out.push(find.to_string());
        start_index += index + find.len();
    }
    out
}

/// `TrimmedBoundaryReplacer` from `reference/packages/opencode/src/tool/edit.ts:562`.
fn trimmed_boundary_replacer(content: &str, find: &str) -> Vec<String> {
    let trimmed_find = find.trim();
    if trimmed_find == find {
        return Vec::new();
    }
    let mut out = Vec::new();
    if content.contains(trimmed_find) {
        out.push(trimmed_find.to_string());
    }
    let lines: Vec<&str> = content.split('\n').collect();
    let find_lines: Vec<&str> = find.split('\n').collect();
    if find_lines.is_empty() || lines.len() < find_lines.len() {
        return out;
    }
    for i in 0..=lines.len() - find_lines.len() {
        let block = lines[i..i + find_lines.len()].join("\n");
        if block.trim() == trimmed_find {
            out.push(block);
        }
    }
    out
}

/// `ContextAwareReplacer` from `reference/packages/opencode/src/tool/edit.ts:588`.
fn context_aware_replacer(content: &str, find: &str) -> Vec<String> {
    let mut find_lines: Vec<&str> = find.split('\n').collect();
    if find_lines.len() < 3 {
        return Vec::new();
    }
    if find_lines.last() == Some(&"") {
        find_lines.pop();
    }
    let content_lines: Vec<&str> = content.split('\n').collect();
    let first_line = find_lines[0].trim();
    let last_line = find_lines[find_lines.len() - 1].trim();
    let mut out = Vec::new();
    for i in 0..content_lines.len() {
        if content_lines[i].trim() != first_line {
            continue;
        }
        let mut j = i + 2;
        while j < content_lines.len() {
            if content_lines[j].trim() == last_line {
                let block_lines = &content_lines[i..j + 1];
                let block = block_lines.join("\n");
                if block_lines.len() == find_lines.len() {
                    let mut matching_lines = 0;
                    let mut total_non_empty = 0;
                    for k in 1..block_lines.len() - 1 {
                        let block_line = block_lines[k].trim();
                        let find_line = find_lines[k].trim();
                        if !block_line.is_empty() || !find_line.is_empty() {
                            total_non_empty += 1;
                            if block_line == find_line {
                                matching_lines += 1;
                            }
                        }
                    }
                    if total_non_empty == 0 || matching_lines as f64 / total_non_empty as f64 >= 0.5
                    {
                        out.push(block);
                        break;
                    }
                }
                break;
            }
            j += 1;
        }
    }
    out
}

const SINGLE_CANDIDATE_SIMILARITY_THRESHOLD: f64 = 0.65;
const MULTIPLE_CANDIDATES_SIMILARITY_THRESHOLD: f64 = 0.65;

fn levenshtein_distance(a: &str, b: &str) -> usize {
    if a.is_empty() || b.is_empty() {
        return a.len().max(b.len());
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + cost);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// `replace` from `reference/packages/opencode/src/tool/edit.ts:682`.
pub fn replace(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<String, ToolError> {
    if old_string == new_string {
        return Err(ToolError::Other(
            "No changes to apply: oldString and newString are identical.".to_string(),
        ));
    }
    if old_string.is_empty() {
        return Err(ToolError::Other(
            "oldString cannot be empty when editing an existing file. Provide the exact text to replace, or use write for an intentional full-file replacement.".to_string(),
        ));
    }

    let replacers: Vec<Replacer> = vec![
        Box::new(simple_replacer),
        Box::new(line_trimmed_replacer),
        Box::new(block_anchor_replacer),
        Box::new(whitespace_normalized_replacer),
        Box::new(indentation_flexible_replacer),
        Box::new(escape_normalized_replacer),
        Box::new(trimmed_boundary_replacer),
        Box::new(context_aware_replacer),
        Box::new(multi_occurrence_replacer),
    ];

    let mut not_found = true;
    for replacer in &replacers {
        for search in replacer(content, old_string) {
            let Some(index) = content.find(&search) else {
                continue;
            };
            not_found = false;
            if is_disproportionate_match(&search, old_string) {
                return Err(ToolError::Other(
                    "Refusing replacement because the matched span is much larger than oldString. Re-read the file and provide the full exact oldString for the intended replacement.".to_string(),
                ));
            }
            if replace_all {
                return Ok(content.replace(&search, new_string));
            }
            let last_index = content.rfind(&search).unwrap_or(index);
            if index != last_index {
                continue;
            }
            let mut next = String::with_capacity(content.len() + new_string.len() - search.len());
            next.push_str(&content[..index]);
            next.push_str(new_string);
            next.push_str(&content[index + search.len()..]);
            return Ok(next);
        }
    }

    if not_found {
        return Err(ToolError::Other(
            "Could not find oldString in the file. It must match exactly, including whitespace, indentation, and line endings.".to_string(),
        ));
    }
    Err(ToolError::Other(
        "Found multiple matches for oldString. Provide more surrounding context to make the match unique.".to_string(),
    ))
}

/// `isDisproportionateMatch` from `reference/packages/opencode/src/tool/edit.ts:731`.
fn is_disproportionate_match(search: &str, old_string: &str) -> bool {
    let old_lines = count_lines(old_string);
    let search_lines = count_lines(search);
    if search_lines >= old_lines.max(old_lines + 3) || search_lines >= old_lines * 2 {
        return true;
    }
    if old_lines == 1 {
        return false;
    }
    let search_trim = search.trim().len();
    let old_trim = old_string.trim().len();
    search_trim > (old_trim + 500).max(old_trim * 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonschema;
    use crate::model::ToolContext;

    #[test]
    fn schema_matches_reference_snapshot() {
        let schema = jsonschema::from_schema(&parameters());
        assert_eq!(
            schema,
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "properties": {
                    "filePath": { "description": "The absolute path to the file to modify", "type": "string" },
                    "newString": { "description": "The text to replace it with (must be different from oldString)", "type": "string" },
                    "oldString": { "description": "The text to replace", "type": "string" },
                    "replaceAll": { "description": "Replace all occurrences of oldString (default false)", "type": "boolean" }
                },
                "required": ["filePath", "oldString", "newString"],
                "type": "object"
            })
        );
    }

    #[test]
    fn replace_simple() {
        assert_eq!(
            replace("hello world", "world", "rust", false).unwrap(),
            "hello rust"
        );
    }

    #[test]
    fn replace_requires_unique() {
        assert!(replace("a a a", "a", "b", false).is_err());
        assert_eq!(replace("a a a", "a", "b", true).unwrap(), "b b b");
    }

    #[test]
    fn replace_matches_with_trimmed_lines() {
        let content = "fn main() {\n    let x = 1;\n}\n";
        let result = replace(content, "let x = 1;", "let y = 2;", false).unwrap();
        assert_eq!(result, "fn main() {\n    let y = 2;\n}\n");
    }

    #[test]
    fn replace_rejects_identical() {
        assert!(replace("abc", "x", "x", false).is_err());
    }

    #[test]
    fn edit_applies_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "line one\nline two\n").unwrap();
        let mut ctx = ToolContext::default();
        ctx.instance = Some(crate::model::InstanceContext {
            directory: dir.path().to_string_lossy().to_string(),
            worktree: dir.path().to_string_lossy().to_string(),
        });
        let def = crate::tool::tool::wrap("edit", def());
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(def.execute(
                serde_json::json!({
                    "filePath": file.to_string_lossy(),
                    "oldString": "line two",
                    "newString": "line two edited",
                }),
                &mut ctx,
            ))
            .unwrap();
        assert_eq!(result.output, "Edit applied successfully.");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "line one\nline two edited\n"
        );
        assert_eq!(ctx.asks[0].permission, "edit");
    }
}
