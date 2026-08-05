//! Line-based diff helpers mirroring the `diff` npm package usage in the
//! reference (`createTwoFilesPatch`, `diffLines`, and the `trimDiff` helper
//! from `reference/packages/opencode/src/tool/edit.ts:646`).
//!
//! The reference feeds `diffLines` results through a Myers diff; this port uses
//! an LCS line diff. Output is structurally identical for the common cases;
//! exact jsdiff byte parity on pathological inputs is a known TODO.

#[derive(Debug, Clone)]
pub struct DiffPart {
    pub value: String,
    pub added: bool,
    pub removed: bool,
    /// Number of lines in `value` per jsdiff `countLines`.
    pub count: usize,
}

/// `countLines` used by jsdiff.
pub fn count_lines(value: &str) -> usize {
    let lines: Vec<&str> = value.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.len() - 1
    } else {
        lines.len()
    }
}

/// `diffLines(old, new)` — line-level parts.
pub fn diff_lines(old: &str, new: &str) -> Vec<DiffPart> {
    let old_lines: Vec<String> = split_lines(old);
    let new_lines: Vec<String> = split_lines(new);
    let lcs = lcs_indices(&old_lines, &new_lines);
    let mut parts: Vec<DiffPart> = Vec::new();
    let mut old_i = 0;
    let mut new_i = 0;
    let push = |value: String, added: bool, removed: bool, parts: &mut Vec<DiffPart>| {
        let count = count_lines(&value);
        match parts.last_mut() {
            Some(last) if last.added == added && last.removed == removed && (added || removed) => {
                last.value.push_str(&value);
                last.count += count;
            }
            _ => parts.push(DiffPart {
                value,
                added,
                removed,
                count,
            }),
        }
    };
    for (o, n) in lcs {
        while old_i < o {
            push(old_lines[old_i].clone(), false, true, &mut parts);
            old_i += 1;
        }
        while new_i < n {
            push(new_lines[new_i].clone(), true, false, &mut parts);
            new_i += 1;
        }
        let common = old_lines[old_i].clone();
        push(common, false, false, &mut parts);
        old_i += 1;
        new_i += 1;
    }
    while old_i < old_lines.len() {
        push(old_lines[old_i].clone(), false, true, &mut parts);
        old_i += 1;
    }
    while new_i < new_lines.len() {
        push(new_lines[new_i].clone(), true, false, &mut parts);
        new_i += 1;
    }
    parts
}

fn split_lines(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    let mut start = 0;
    for (index, _) in value.match_indices('\n') {
        out.push(value[start..index + 1].to_string());
        start = index + 1;
    }
    if start < value.len() {
        out.push(value[start..].to_string());
    }
    out
}

fn lcs_indices(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let n = a.len();
    let m = b.len();
    if n * m > 4_000_000 {
        // Fall back to a greedy common-prefix/suffix scan for very large inputs.
        return greedy_common(a, b);
    }
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if a[i] == b[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut result = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            result.push((i, j));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    result
}

fn greedy_common(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut ai = 0;
    let mut bi = 0;
    while ai < a.len() && bi < b.len() && a[ai] == b[bi] {
        result.push((ai, bi));
        ai += 1;
        bi += 1;
    }
    let mut asuffix = a.len();
    let mut bsuffix = b.len();
    while asuffix > ai && bsuffix > bi && a[asuffix - 1] == b[bsuffix - 1] {
        result.push((asuffix - 1, bsuffix - 1));
        asuffix -= 1;
        bsuffix -= 1;
    }
    result.sort();
    result
}

/// `createTwoFilesPatch(oldFileName, newFileName, oldStr, newStr)`.
pub fn create_two_files_patch(old_name: &str, new_name: &str, old: &str, new: &str) -> String {
    let mut out = format!("--- {old_name}\n+++ {new_name}\n");
    let parts = diff_lines(old, new);
    let mut hunks: Vec<String> = Vec::new();
    let mut old_row = 1usize;
    let mut new_row = 1usize;
    let mut i = 0;
    let context = 3;

    while i < parts.len() {
        if !parts[i].added && !parts[i].removed {
            let lines = count_lines(&parts[i].value);
            old_row += lines;
            new_row += lines;
            i += 1;
            continue;
        }

        let hunk_start_old = old_row;
        let hunk_start_new = new_row;
        let mut hunk_old_lines: Vec<String> = Vec::new();
        let mut hunk_new_lines: Vec<String> = Vec::new();
        let mut lead_context: Vec<String> = Vec::new();

        // leading context
        if i > 0 {
            let previous = &parts[i - 1];
            if !previous.added && !previous.removed {
                for line in previous.value.split_inclusive('\n') {
                    if line.ends_with('\n') {
                        lead_context.push(format!(" {line}"));
                    }
                }
            }
        }

        let mut j = i;
        let mut trailing = String::new();
        while j < parts.len()
            && (parts[j].added || parts[j].removed || count_lines(&parts[j].value) <= context * 2)
        {
            let part = &parts[j];
            if part.added {
                for line in part.value.split_inclusive('\n') {
                    if line.ends_with('\n') {
                        hunk_new_lines.push(format!("+{line}"));
                        new_row += 1;
                    }
                }
            } else if part.removed {
                for line in part.value.split_inclusive('\n') {
                    if line.ends_with('\n') {
                        hunk_old_lines.push(format!("-{line}"));
                        old_row += 1;
                    }
                }
            } else {
                for line in part.value.split_inclusive('\n') {
                    if line.ends_with('\n') {
                        hunk_old_lines.push(format!(" {line}"));
                        hunk_new_lines.push(format!(" {line}"));
                        old_row += 1;
                        new_row += 1;
                    }
                }
            }
            j += 1;
        }
        if j < parts.len() {
            let next = &parts[j];
            if !next.added && !next.removed {
                let mut lines: Vec<String> = next
                    .value
                    .split_inclusive('\n')
                    .filter(|line| line.ends_with('\n'))
                    .map(|line| format!(" {line}"))
                    .collect();
                let take = lines.len().min(context);
                lines.truncate(take);
                old_row += take;
                new_row += take;
                trailing = lines.concat();
            }
        }

        let old_count = hunk_old_lines.len();
        let new_count = hunk_new_lines.len();
        let mut body = String::new();
        for line in lead_context {
            body.push_str(&line);
        }
        for line in &hunk_old_lines {
            body.push_str(line);
        }
        for line in &hunk_new_lines {
            body.push_str(line);
        }
        body.push_str(&trailing);

        hunks.push(format!(
            "@@ -{},{} +{},{} @@\n{}",
            hunk_start_old, old_count, hunk_start_new, new_count, body
        ));
        i = j;
    }

    for hunk in hunks {
        out.push_str(&hunk);
    }
    out
}

/// `trimDiff` from `reference/packages/opencode/src/tool/edit.ts:646`.
pub fn trim_diff(diff: &str) -> String {
    let lines: Vec<&str> = diff.split('\n').collect();
    let content_lines: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| {
            (line.starts_with('+') || line.starts_with('-') || line.starts_with(' '))
                && !line.starts_with("---")
                && !line.starts_with("+++")
        })
        .collect();
    if content_lines.is_empty() {
        return diff.to_string();
    }

    let mut min = usize::MAX;
    for line in &content_lines {
        let content = &line[1..];
        if !content.trim().is_empty() {
            let indent = content.len() - content.trim_start().len();
            if indent < min {
                min = indent;
            }
        }
    }
    if min == usize::MAX || min == 0 {
        return diff.to_string();
    }

    lines
        .iter()
        .map(|line| {
            if (line.starts_with('+') || line.starts_with('-') || line.starts_with(' '))
                && !line.starts_with("---")
                && !line.starts_with("+++")
            {
                let prefix = &line[..1];
                let content = &line[1..];
                format!("{prefix}{}", &content[min.min(content.len())..])
            } else {
                (*line).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_lines_reports_changes() {
        let parts = diff_lines("a\nb\nc\n", "a\nx\nc\n");
        assert_eq!(parts.len(), 4);
        assert!(!parts[0].removed && !parts[0].added);
        assert!(parts[1].removed && parts[1].value == "b\n");
        assert!(parts[2].added && parts[2].value == "x\n");
        assert!(!parts[3].removed && !parts[3].added);
    }

    #[test]
    fn two_files_patch_headers() {
        let patch = create_two_files_patch("f", "f", "", "hello\n");
        assert!(patch.starts_with("--- f\n+++ f\n"));
        assert!(patch.contains("+hello"));
    }

    #[test]
    fn trim_diff_removes_shared_indent() {
        let diff = "--- f\n+++ f\n@@ -1,1 +1,1 @@\n   a\n-   b\n+   c\n";
        let trimmed = trim_diff(diff);
        assert!(trimmed.contains(" a\n"));
        assert!(trimmed.contains("- b\n"));
        assert!(trimmed.contains("+ c\n"));
    }
}
