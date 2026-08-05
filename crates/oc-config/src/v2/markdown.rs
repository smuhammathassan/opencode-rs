// From reference/packages/core/src/config/markdown.ts
//
// Port of `gray-matter` frontmatter parsing: split `---` blocks, parse the
// YAML frontmatter into a JSON value, and fall back to the reference's
// `sanitize` re-parse for the permissive YAML other coding agents produce.

use indexmap::IndexMap;
use serde_json::Value;

/// Splits frontmatter off a markdown document.
fn split_frontmatter(content: &str) -> Option<(String, String)> {
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;
    // Find the closing `---` on its own line.
    let mut lines = rest.split_inclusive('\n');
    let mut frontmatter = String::new();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" || trimmed == "..." {
            let body = lines.collect::<String>();
            return Some((frontmatter, body));
        }
        frontmatter.push_str(line);
    }
    None
}

/// `sanitize` — retry unquoted-colon frontmatter values as YAML block scalars.
pub fn sanitize(content: &str) -> String {
    let re = regex::Regex::new(r"(?m)^---\r?\n([\s\S]*?)\r?\n---").expect("static regex");
    let Some(captures) = re.captures(content) else {
        return content.to_string();
    };
    let frontmatter = &captures[1];
    let result = frontmatter
        .split('\n')
        .flat_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#')
                || trimmed.is_empty()
                || line.starts_with(char::is_whitespace)
            {
                return vec![line.to_string()];
            }
            let Some(entry) = regex::Regex::new(r"^([a-zA-Z_][a-zA-Z0-9_]*)\s*:\s*(.*)$")
                .expect("static regex")
                .captures(line)
            else {
                return vec![line.to_string()];
            };
            let value = entry[2].trim();
            if value.is_empty()
                || value == ">"
                || value == "|"
                || value.starts_with('"')
                || value.starts_with('\'')
            {
                return vec![line.to_string()];
            }
            if !value.contains(':') {
                return vec![line.to_string()];
            }
            vec![format!("{}: |-", &entry[1]), format!("  {value}")]
        })
        .collect::<Vec<_>>()
        .join("\n");
    let full = captures.get(0).map(|m| m.as_str()).unwrap_or("");
    content.replacen(full, &format!("---\n{result}\n---"), 1)
}

/// Parses markdown frontmatter into `(data, body)`. Mirrors `parse`/`parseOption`:
/// the body is the content after the closing `---`.
pub fn parse(content: &str) -> Option<(IndexMap<String, Value>, String)> {
    let (frontmatter, body) = split_frontmatter(content)?;
    let data = parse_yaml(&frontmatter).or_else(|| {
        let sanitized = sanitize(content);
        split_frontmatter(&sanitized).and_then(|(fm, _)| parse_yaml(&fm))
    })?;
    Some((data, body.trim().to_string()))
}

// ---------------------------------------------------------------------------
// Minimal YAML frontmatter parser
// ---------------------------------------------------------------------------

fn parse_yaml(input: &str) -> Option<IndexMap<String, Value>> {
    let lines = input.lines().collect::<Vec<_>>();
    let mut idx = 0;
    let node = parse_block(&lines, &mut idx, 0)?;
    match node {
        Value::Object(map) => Some(map.into_iter().collect()),
        _ => None,
    }
}

fn parse_block(lines: &[&str], idx: &mut usize, indent: usize) -> Option<Value> {
    let mut map = IndexMap::new();
    loop {
        let Some(raw_line) = lines.get(*idx) else {
            break;
        };
        let line = raw_line.trim_end();
        if line.trim().is_empty() || line.trim().starts_with('#') {
            *idx += 1;
            continue;
        }
        let current_indent = leading_spaces(line);
        if current_indent < indent {
            break;
        }
        if current_indent > indent {
            // Deeper indentation without a parent key is invalid.
            return None;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('-') {
            return Some(parse_list(lines, idx, indent)?);
        }
        if trimmed == "---" || trimmed == "..." {
            *idx += 1;
            break;
        }
        let (key, rest) = split_key(trimmed)?;
        if key.is_empty() {
            return None;
        }
        *idx += 1;
        let value = if rest.is_empty() {
            parse_value_block(lines, idx, indent)?
        } else {
            scalar_or_flow(rest)?
        };
        map.insert(key, value);
    }
    Some(Value::Object(map.into_iter().collect()))
}

fn parse_list(lines: &[&str], idx: &mut usize, indent: usize) -> Option<Value> {
    let mut items = Vec::new();
    loop {
        let Some(raw_line) = lines.get(*idx) else {
            break;
        };
        let line = raw_line.trim_end();
        if line.trim().is_empty() || line.trim().starts_with('#') {
            *idx += 1;
            continue;
        }
        let current_indent = leading_spaces(line);
        if current_indent < indent {
            break;
        }
        if current_indent > indent {
            return None;
        }
        let trimmed = line.trim_start();
        if trimmed == "---" || trimmed == "..." {
            *idx += 1;
            break;
        }
        if let Some(rest) = trimmed.strip_prefix('-') {
            *idx += 1;
            let rest = rest.trim_start();
            if rest.is_empty() {
                items.push(parse_value_block(lines, idx, indent + 1)?);
            } else if let Some((key, value)) = split_key(rest) {
                let mut map = IndexMap::new();
                let value = if value.is_empty() {
                    parse_value_block(lines, idx, indent + 1)?
                } else {
                    scalar_or_flow(value)?
                };
                map.insert(key, value);
                items.push(Value::Object(map.into_iter().collect()));
            } else {
                items.push(scalar_or_flow(rest)?);
            }
        } else {
            break;
        }
    }
    Some(Value::Array(items))
}

/// Handles an empty value: either `null` or a nested block / block scalar.
fn parse_value_block(lines: &[&str], idx: &mut usize, parent_indent: usize) -> Option<Value> {
    let line = lines.get(*idx)?.trim_end();
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Some(Value::Null);
    }
    // Block scalar `|` or `>` on the key line.
    if let Some((indicator, rest)) = block_scalar_indicator(trimmed) {
        return parse_block_scalar(lines, idx, parent_indent, indicator, rest);
    }
    let current_indent = leading_spaces(line);
    if current_indent > parent_indent {
        parse_block(lines, idx, current_indent)
    } else {
        Some(Value::Null)
    }
}

fn block_scalar_indicator(trimmed: &str) -> Option<(char, Option<String>)> {
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if first == '|' || first == '>' {
        let rest = chars.as_str().trim();
        Some((
            first,
            if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
        ))
    } else {
        None
    }
}

fn parse_block_scalar(
    lines: &[&str],
    idx: &mut usize,
    parent_indent: usize,
    indicator: char,
    _chomp: Option<String>,
) -> Option<Value> {
    let mut out = String::new();
    let mut content_indent: Option<usize> = None;
    while let Some(line) = lines.get(*idx) {
        if line.trim().is_empty() {
            out.push('\n');
            *idx += 1;
            continue;
        }
        let current_indent = leading_spaces(line);
        if current_indent <= parent_indent {
            break;
        }
        if content_indent.is_none() {
            content_indent = Some(current_indent);
        }
        let indent = content_indent.unwrap();
        let text = if line.len() >= indent {
            &line[indent..]
        } else {
            ""
        };
        if indicator == '>' {
            if out.ends_with('\n') && !text.is_empty() {
                out.pop();
                out.push(' ');
            }
            out.push_str(text);
            out.push('\n');
        } else {
            out.push_str(text);
            out.push('\n');
        }
        *idx += 1;
    }
    let value = out.trim_end_matches('\n').to_string();
    Some(Value::String(value))
}

fn split_key(line: &str) -> Option<(String, &str)> {
    let colon = find_key_colon(line)?;
    let key = line[..colon].trim();
    let rest = line[colon + 1..].trim_start();
    if key.is_empty() || key.contains(' ') {
        if key.starts_with(['"', '\'']) {
            let value = parse_quoted(key)?;
            return Some((value, rest));
        }
        return None;
    }
    if key.starts_with(['"', '\'']) {
        let value = parse_quoted(key)?;
        return Some((value, rest));
    }
    Some((key.to_string(), rest))
}

fn find_key_colon(line: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' | '\'' => {
                if quote == Some(c) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(c);
                }
            }
            ':' if quote.is_none() => return Some(i),
            _ => {}
        }
    }
    None
}

fn scalar_or_flow(value: &str) -> Option<Value> {
    let value = value.trim();
    if value.starts_with('[') && value.ends_with(']') {
        return parse_inline_list(&value[1..value.len() - 1]);
    }
    if value.starts_with('{') && value.ends_with('}') {
        return parse_inline_map(&value[1..value.len() - 1]);
    }
    if value.starts_with('[') || value.starts_with('{') {
        // Unbalanced flow collection — js-yaml would reject this.
        return None;
    }
    if let Some(quoted) = parse_quoted(value) {
        return Some(Value::String(quoted));
    }
    if value.is_empty() {
        return Some(Value::Null);
    }
    match value {
        "true" | "True" | "TRUE" => return Some(Value::Bool(true)),
        "false" | "False" | "FALSE" => return Some(Value::Bool(false)),
        "null" | "Null" | "NULL" | "~" => return Some(Value::Null),
        _ => {}
    }
    if let Ok(int) = value.parse::<i64>() {
        return Some(Value::from(int));
    }
    if let Ok(float) = value.parse::<f64>() {
        if float.is_finite() {
            return Some(Value::from(float));
        }
    }
    Some(Value::String(value.to_string()))
}

fn parse_quoted(value: &str) -> Option<String> {
    let value = value.trim();
    let quote = value.chars().next()?;
    if (quote != '"' && quote != '\'') || value.len() < 2 || !value.ends_with(quote) {
        return None;
    }
    let inner = &value[quote.len_utf8()..value.len() - quote.len_utf8()];
    if quote == '\'' {
        return Some(inner.replace("''", "'"));
    }
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn parse_inline_list(inner: &str) -> Option<Value> {
    let mut items = Vec::new();
    for part in split_flow(inner) {
        items.push(scalar_or_flow(part)?);
    }
    Some(Value::Array(items))
}

fn parse_inline_map(inner: &str) -> Option<Value> {
    let mut map = IndexMap::new();
    for part in split_flow(inner) {
        let (key, value) = split_key(part)?;
        let key = parse_quoted(&key).unwrap_or(key);
        map.insert(key, scalar_or_flow(value)?);
    }
    Some(Value::Object(map.into_iter().collect()))
}

/// Splits flow collection items on commas, respecting quotes and brackets.
fn split_flow(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in inner.char_indices() {
        match c {
            '"' | '\'' => {
                if quote == Some(c) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(c);
                }
            }
            _ if quote.is_some() => {}
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                let part = inner[start..i].trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}
