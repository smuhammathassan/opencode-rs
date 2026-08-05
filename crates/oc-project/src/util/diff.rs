/// Minimal port of jsdiff's `structuredPatch`/`formatPatch` used by the Vcs and
/// Snapshot services (reference/packages/opencode/src/project/vcs.ts and
/// snapshot/index.ts). Produces unified diffs with configurable context.
///
/// TODO(integration): verify byte parity against jsdiff (line-split edge cases,
/// `\ No newline at end of file` markers) if exact patch text is required.
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<String>,
}

pub struct StructuredPatch {
    pub old_file_name: String,
    pub new_file_name: String,
    pub old_header: String,
    pub new_header: String,
    pub hunks: Vec<Hunk>,
}

pub fn structured_patch(
    old_file_name: &str,
    new_file_name: &str,
    old: &str,
    new: &str,
    context: usize,
) -> StructuredPatch {
    let a = split_lines(old);
    let b = split_lines(new);
    let ops = diff_ops(&a, &b);
    let ranges = hunk_ranges(&ops, context);

    let mut hunks = Vec::new();
    let mut old_idx = 0usize;
    let mut new_idx = 0usize;
    let mut op_pos = 0usize;

    for (start, finish) in ranges {
        while op_pos < start {
            match ops[op_pos] {
                Op::Keep => {
                    old_idx += 1;
                    new_idx += 1;
                }
                Op::Delete => old_idx += 1,
                Op::Insert => new_idx += 1,
            }
            op_pos += 1;
        }

        let old_start = old_idx + 1;
        let new_start = new_idx + 1;
        let mut old_count = 0usize;
        let mut new_count = 0usize;
        let mut lines = Vec::new();

        while op_pos < finish {
            match ops[op_pos] {
                Op::Keep => {
                    lines.push(format!(" {}", a[old_idx]));
                    old_idx += 1;
                    new_idx += 1;
                    old_count += 1;
                    new_count += 1;
                }
                Op::Delete => {
                    lines.push(format!("-{}", a[old_idx]));
                    old_idx += 1;
                    old_count += 1;
                }
                Op::Insert => {
                    lines.push(format!("+{}", b[new_idx]));
                    new_idx += 1;
                    new_count += 1;
                }
            }
            op_pos += 1;
        }

        hunks.push(Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines,
        });
    }

    StructuredPatch {
        old_file_name: old_file_name.to_string(),
        new_file_name: new_file_name.to_string(),
        old_header: String::new(),
        new_header: String::new(),
        hunks,
    }
}

pub fn format_patch(patch: &StructuredPatch) -> String {
    let mut out = Vec::new();
    out.push(format!("--- {}{}", patch.old_file_name, patch.old_header));
    out.push(format!("+++ {}{}", patch.new_file_name, patch.new_header));
    for hunk in &patch.hunks {
        out.push(format!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        ));
        out.extend(hunk.lines.iter().cloned());
    }
    out.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Keep,
    Delete,
    Insert,
}

fn split_lines(input: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = input.split('\n').collect();
    if input.ends_with('\n') && lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

fn hunk_ranges(ops: &[Op], context: usize) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut index = 0;
    while index < ops.len() {
        if ops[index] == Op::Keep {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < ops.len() && ops[end] != Op::Keep {
            end += 1;
        }
        let start = index.saturating_sub(context);
        let finish = end.saturating_add(context).min(ops.len());
        if let Some((_, previous_finish)) = ranges.last_mut() {
            if start <= *previous_finish {
                *previous_finish = finish;
                index = finish;
                continue;
            }
        }
        ranges.push((start, finish));
        index = finish;
    }
    ranges
}

/// Myers diff over line sequences, returning an edit script in order.
fn diff_ops(a: &[&str], b: &[&str]) -> Vec<Op> {
    let n = a.len() as i64;
    let m = b.len() as i64;
    let max = n + m;
    if max == 0 {
        return Vec::new();
    }
    let offset = max;
    let size = (2 * max + 1) as usize;
    let mut v = vec![0i64; size];
    let mut trace: Vec<Vec<i64>> = Vec::new();
    let mut dmax = 0;
    let mut found = false;

    for d in 0..=max {
        for k in (-d..=d).step_by(2) {
            let index = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[index - 1] < v[index + 1]) {
                v[index + 1]
            } else {
                v[index - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[index] = x;
            if x >= n && y >= m {
                found = true;
                break;
            }
        }
        trace.push(v.clone());
        dmax = d;
        if found {
            break;
        }
    }

    let mut ops = Vec::new();
    let mut x = n;
    let mut y = m;
    for d in (1..=dmax).rev() {
        let snapshot = &trace[d as usize];
        let k = x - y;
        let index = (k + offset) as usize;
        let prev_k = if k == -d || (k != d && snapshot[index - 1] < snapshot[index + 1]) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = snapshot[(prev_k + offset) as usize];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            ops.push(Op::Keep);
            x -= 1;
            y -= 1;
        }
        if x == prev_x {
            ops.push(Op::Insert);
        } else {
            ops.push(Op::Delete);
        }
        x = prev_x;
        y = prev_y;
    }
    while x > 0 && y > 0 {
        ops.push(Op::Keep);
        x -= 1;
        y -= 1;
    }
    while x > 0 {
        ops.push(Op::Delete);
        x -= 1;
    }
    while y > 0 {
        ops.push(Op::Insert);
        y -= 1;
    }
    ops.reverse();
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(old: &str, new: &str, context: usize) -> String {
        format_patch(&structured_patch("f.txt", "f.txt", old, new, context))
    }

    #[test]
    fn empty_diff_renders_headers_only() {
        assert_eq!(render("", "", 0), "--- f.txt\n+++ f.txt");
        assert_eq!(render("same\n", "same\n", 3), "--- f.txt\n+++ f.txt");
    }

    #[test]
    fn insertion_renders_single_hunk() {
        assert_eq!(
            render("a\nb\n", "a\nx\nb\n", 0),
            "--- f.txt\n+++ f.txt\n@@ -2,0 +2,1 @@\n+x"
        );
    }

    #[test]
    fn context_lines_are_included() {
        assert_eq!(
            render("a\nb\nc\n", "a\nb\nd\n", 1),
            "--- f.txt\n+++ f.txt\n@@ -2,2 +2,2 @@\n b\n-c\n+d"
        );
    }

    #[test]
    fn deletion_renders() {
        assert_eq!(
            render("a\nb\n", "a\n", 0),
            "--- f.txt\n+++ f.txt\n@@ -2,1 +2,0 @@\n-b"
        );
    }

    #[test]
    fn large_context_merges_into_one_hunk() {
        let rendered = render("1\n2\n3\n4\n5\n", "1\n2\nX\n4\n5\n", usize::MAX);
        assert_eq!(
            rendered,
            "--- f.txt\n+++ f.txt\n@@ -1,5 +1,5 @@\n 1\n 2\n-3\n+X\n 4\n 5"
        );
    }
}
