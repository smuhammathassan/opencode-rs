//! Minimal JSONC parsing and editing for `opencode.json` / `.opencode/opencode.json`.
//!
//! The reference uses `jsonc-parser` (packages/opencode/src/plugin/install.ts).
//! This module provides a small scanner that locates structural positions in
//! the original text so patches preserve formatting instead of rewriting the
//! whole file as strict JSON.

use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum JsoncError {
    #[error("invalid JSONC at offset {offset}: {message}")]
    Parse { offset: usize, message: String },
}

#[derive(Debug, Clone)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub enum Node {
    Object {
        span: Span,
        members: Vec<(String, Node)>,
    },
    Array {
        span: Span,
        items: Vec<Node>,
    },
    Value {
        span: Span,
        value: Value,
    },
}

impl Node {
    pub fn span(&self) -> &Span {
        match self {
            Node::Object { span, .. } | Node::Array { span, .. } | Node::Value { span, .. } => span,
        }
    }

    /// Look up a member by key in an object node.
    pub fn member(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Object { members, .. } => {
                members.iter().find(|(k, _)| k == key).map(|(_, node)| node)
            }
            _ => None,
        }
    }
}

/// A parsed JSONC document with spans into the original text.
pub struct ParsedJsonc {
    pub value: Value,
    pub root: Node,
}

/// Parse JSONC text (comments and trailing commas allowed) into a value plus a
/// span tree.
pub fn parse(text: &str) -> Result<ParsedJsonc, JsoncError> {
    let chars: Vec<char> = text.chars().collect();
    let mut byte_offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
    byte_offsets.push(text.len());
    let mut parser = Parser {
        chars,
        byte_offsets,
        pos: 0,
    };
    let node = parser.parse_value()?;
    let value = json_node(&node);
    Ok(ParsedJsonc { value, root: node })
}

fn json_node(node: &Node) -> Value {
    match node {
        Node::Value { value, .. } => value.clone(),
        Node::Array { items, .. } => Value::Array(items.iter().map(json_node).collect()),
        Node::Object { members, .. } => {
            let map: serde_json::Map<String, Value> = members
                .iter()
                .map(|(k, v)| (k.clone(), json_node(v)))
                .collect();
            Value::Object(map)
        }
    }
}

struct Parser {
    chars: Vec<char>,
    byte_offsets: Vec<usize>,
    pos: usize,
}

impl Parser {
    fn byte_pos(&self, char_pos: usize) -> usize {
        self.byte_offsets[char_pos]
    }

    fn parse_value(&mut self) -> Result<Node, JsoncError> {
        self.skip_ws();
        let Some(&c) = self.chars.get(self.pos) else {
            return Err(JsoncError::Parse {
                offset: self.pos,
                message: "unexpected end of input".into(),
            });
        };
        match c {
            '{' => self.parse_object(),
            '[' => self.parse_array(),
            '"' => {
                let (s, start) = self.parse_string()?;
                let end = self.pos;
                Ok(Node::Value {
                    span: Span {
                        start: self.byte_pos(start),
                        end: self.byte_pos(end),
                    },
                    value: Value::String(s),
                })
            }
            _ => self.parse_scalar(),
        }
    }

    fn parse_object(&mut self) -> Result<Node, JsoncError> {
        let start = self.pos;
        self.pos += 1; // '{'
        let mut members = Vec::new();
        loop {
            self.skip_ws();
            let Some(&c) = self.chars.get(self.pos) else {
                return Err(JsoncError::Parse {
                    offset: self.pos,
                    message: "unterminated object".into(),
                });
            };
            if c == '}' {
                self.pos += 1;
                break;
            }
            if c == ',' {
                self.pos += 1;
                continue;
            }
            if c != '"' {
                return Err(JsoncError::Parse {
                    offset: self.pos,
                    message: "expected object key".into(),
                });
            }
            let (key, _) = self.parse_string()?;
            self.skip_ws();
            if self.chars.get(self.pos) != Some(&':') {
                return Err(JsoncError::Parse {
                    offset: self.pos,
                    message: "expected ':'".into(),
                });
            }
            self.pos += 1;
            let value = self.parse_value()?;
            members.push((key, value));
        }
        Ok(Node::Object {
            span: Span {
                start: self.byte_pos(start),
                end: self.byte_pos(self.pos),
            },
            members,
        })
    }

    fn parse_array(&mut self) -> Result<Node, JsoncError> {
        let start = self.pos;
        self.pos += 1; // '['
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            let Some(&c) = self.chars.get(self.pos) else {
                return Err(JsoncError::Parse {
                    offset: self.pos,
                    message: "unterminated array".into(),
                });
            };
            if c == ']' {
                self.pos += 1;
                break;
            }
            if c == ',' {
                self.pos += 1;
                continue;
            }
            let item = self.parse_value()?;
            items.push(item);
        }
        Ok(Node::Array {
            span: Span {
                start: self.byte_pos(start),
                end: self.byte_pos(self.pos),
            },
            items,
        })
    }

    fn parse_scalar(&mut self) -> Result<Node, JsoncError> {
        let start = self.pos;
        // Skip a token until one of the structural terminators.
        while let Some(&c) = self.chars.get(self.pos) {
            if matches!(c, ',' | '}' | ']' | ':' | '\n' | '\r' | ' ' | '\t') {
                break;
            }
            if c == '/' {
                // Could be a comment after a token.
                if self.chars.get(self.pos + 1) == Some(&'/')
                    || self.chars.get(self.pos + 1) == Some(&'*')
                {
                    break;
                }
            }
            self.pos += 1;
        }
        let end = self.pos;
        let token: String = self.chars[start..end].iter().collect();
        let token = token.trim();
        let value: Value = if token.is_empty() {
            return Err(JsoncError::Parse {
                offset: start,
                message: "expected value".into(),
            });
        } else if token == "true" {
            Value::Bool(true)
        } else if token == "false" {
            Value::Bool(false)
        } else if token == "null" {
            Value::Null
        } else {
            serde_json::from_str(token)
                .or_else(|_| token.parse::<f64>().map(Value::from))
                .or_else(|_| Ok(Value::String(token.to_string())))
                .map_err(|_: serde_json::Error| JsoncError::Parse {
                    offset: start,
                    message: "invalid scalar".into(),
                })?
        };
        Ok(Node::Value {
            span: Span {
                start: self.byte_pos(start),
                end: self.byte_pos(end),
            },
            value,
        })
    }

    fn parse_string(&mut self) -> Result<(String, usize), JsoncError> {
        let start = self.pos;
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let Some(&c) = self.chars.get(self.pos) else {
                return Err(JsoncError::Parse {
                    offset: self.pos,
                    message: "unterminated string".into(),
                });
            };
            if c == '"' {
                self.pos += 1;
                return Ok((out, start));
            }
            if c == '\\' {
                self.pos += 1;
                let Some(&esc) = self.chars.get(self.pos) else {
                    break;
                };
                out.push(match esc {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    'b' => '\u{0008}',
                    'f' => '\u{000c}',
                    'u' => {
                        let hex: String = self.chars[self.pos + 1..self.pos + 5].iter().collect();
                        self.pos += 4;
                        char::from_u32(u32::from_str_radix(&hex, 16).unwrap_or(0xfffd))
                            .unwrap_or('\u{fffd}')
                    }
                    other => other,
                });
                self.pos += 1;
            } else {
                out.push(c);
                self.pos += 1;
            }
        }
        Err(JsoncError::Parse {
            offset: start,
            message: "unterminated string".into(),
        })
    }

    fn skip_ws(&mut self) {
        loop {
            let Some(&c) = self.chars.get(self.pos) else {
                return;
            };
            match c {
                ' ' | '\t' | '\r' | '\n' => self.pos += 1,
                '/' if self.chars.get(self.pos + 1) == Some(&'/') => {
                    self.pos += 2;
                    while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
                        self.pos += 1;
                    }
                }
                '/' if self.chars.get(self.pos + 1) == Some(&'*') => {
                    self.pos += 2;
                    while self.pos + 1 < self.chars.len()
                        && !(self.chars[self.pos] == '*' && self.chars[self.pos + 1] == '/')
                    {
                        self.pos += 1;
                    }
                    self.pos = (self.pos + 2).min(self.chars.len());
                }
                _ => return,
            }
        }
    }
}

/// The result of patching a plugin list.
#[derive(Debug, Clone, PartialEq)]
pub enum PatchMode {
    Noop,
    Add,
    Replace,
}

#[derive(Debug)]
struct TextEdit {
    start: usize,
    end: usize,
    replacement: String,
}

/// Patch one property in a JSONC object while retaining the surrounding
/// document text. `object_key` selects a nested object when present; `None`
/// targets the document root. This is used by CLI config mutations that must
/// not rewrite unrelated comments or trailing commas.
pub fn patch_object_property(
    text: &str,
    object_key: Option<&str>,
    key: &str,
    value: &Value,
) -> Result<(PatchMode, String), JsoncError> {
    let parsed = parse(text)?;
    let target = match object_key {
        Some(parent) => parsed
            .root
            .member(parent)
            .ok_or_else(|| JsoncError::Parse {
                offset: parsed.root.span().start,
                message: format!("object property not found: {parent}"),
            })?,
        None => &parsed.root,
    };
    let Node::Object { members, span } = target else {
        return Err(JsoncError::Parse {
            offset: target.span().start,
            message: "target must be an object".into(),
        });
    };
    if let Some((_, node)) = members.iter().find(|(member, _)| member == key) {
        let (mut out, canonical_fallback) = if let Value::Object(object) = value {
            match patch_object_node(text, node, object)? {
                Some(edits) => (apply_text_edits(text, edits), false),
                None => (text.to_string(), true),
            }
        } else {
            (text.to_string(), true)
        };

        if canonical_fallback {
            let value_text = serde_json::to_string(value).map_err(|error| JsoncError::Parse {
                offset: 0,
                message: error.to_string(),
            })?;
            out.replace_range(node.span().start..node.span().end, &value_text);
        }
        return Ok((PatchMode::Replace, out));
    }
    let value_text = serde_json::to_string(value).map_err(|error| JsoncError::Parse {
        offset: 0,
        message: error.to_string(),
    })?;
    let insert_at = span.end.saturating_sub(1);
    let comma = if members.is_empty()
        || has_trailing_comma(
            text,
            members.last().map(|(_, node)| node.span().end),
            insert_at,
        ) {
        ""
    } else {
        ","
    };
    let snippet = format!("{comma}\"{key}\": {value_text}");
    let mut out = text.to_string();
    out.insert_str(insert_at, &snippet);
    Ok((PatchMode::Add, out))
}

/// Collect leaf replacements and additions for an object update. Returning
/// `None` is intentional: it means the update cannot be expressed using only
/// targeted leaf edits (e.g. the desired object is not an object), so the
/// caller must use the canonical replacement fallback for that object.
fn patch_object_node(
    text: &str,
    node: &Node,
    desired: &serde_json::Map<String, Value>,
) -> Result<Option<Vec<TextEdit>>, JsoncError> {
    let Node::Object { members, span } = node else {
        return Ok(None);
    };

    let mut edits = Vec::new();
    for (index, (member, current)) in members.iter().enumerate() {
        match desired.get(member) {
            Some(next) => {
                if let (Node::Object { .. }, Value::Object(next_object)) = (current, next) {
                    let Some(nested_edits) = patch_object_node(text, current, next_object)? else {
                        return Ok(None);
                    };
                    edits.extend(nested_edits);
                } else if json_node(current) != *next {
                    edits.push(TextEdit {
                        start: current.span().start,
                        end: current.span().end,
                        replacement: serde_json::to_string(next).map_err(|error| {
                            JsoncError::Parse {
                                offset: current.span().start,
                                message: error.to_string(),
                            }
                        })?,
                    });
                }
            }
            // The member exists in the document but is absent from the desired
            // object: delete it in place, preserving comments around it.
            None => {
                if let Some((start, end)) = delete_member_range(text, members, index) {
                    edits.push(TextEdit {
                        start,
                        end,
                        replacement: String::new(),
                    });
                }
            }
        }
    }

    let additions: Vec<_> = desired
        .iter()
        .filter(|(member, _)| !members.iter().any(|(current, _)| current == *member))
        .collect();
    if !additions.is_empty() {
        let insert_at = span.end.saturating_sub(1);
        let comma = if members.is_empty()
            || has_trailing_comma(
                text,
                members.last().map(|(_, current)| current.span().end),
                insert_at,
            ) {
            ""
        } else {
            ","
        };
        let mut snippet = comma.to_string();
        for (index, (member, value)) in additions.into_iter().enumerate() {
            if index > 0 {
                snippet.push(',');
            }
            let value_text = serde_json::to_string(value).map_err(|error| JsoncError::Parse {
                offset: insert_at,
                message: error.to_string(),
            })?;
            snippet.push_str(&format!("\"{member}\": {value_text}"));
        }
        edits.push(TextEdit {
            start: insert_at,
            end: insert_at,
            replacement: snippet,
        });
    }

    Ok(Some(edits))
}

/// Compute the byte range that removes the member at `index` together with one
/// adjacent separator comma. Deletion never touches comments that are not part
/// of the removed member's own text.
fn delete_member_range(
    text: &str,
    members: &[(String, Node)],
    index: usize,
) -> Option<(usize, usize)> {
    let member = &members[index];
    let (member_start, member_end) = {
        let bytes = text.as_bytes();
        // value.span().start points at the member's value (after `"key": `).
        // Walk back to the separating ':', then to the key's opening quote by
        // counting the two quotes that delimit `"key"`.
        let value_start = member.1.span().start;
        let mut colon = value_start;
        while colon > 0 && bytes[colon - 1] != b':' {
            colon -= 1;
        }
        let mut key_end = colon;
        while key_end > 0 && matches!(bytes[key_end - 1], b' ' | b'\t') {
            key_end -= 1;
        }
        let mut key_start = key_end;
        let mut quotes = 0;
        while key_start > 0 {
            key_start -= 1;
            if bytes[key_start] == b'"' {
                quotes += 1;
                if quotes == 2 {
                    break;
                }
            }
        }
        (key_start, member.1.span().end)
    };

    // Find the separator comma after this member (forward over whitespace).
    let bytes = text.as_bytes();
    let mut forward = member_end;
    while forward < bytes.len() && matches!(bytes[forward], b' ' | b'\t' | b'\r' | b'\n') {
        forward += 1;
    }
    let has_following_comma = forward < bytes.len() && bytes[forward] == b',';
    if has_following_comma {
        // `"key": value,` — drop through the trailing comma.
        return Some((member_start, forward + 1));
    }

    // Last before `}`: extend backward over whitespace to absorb the preceding
    // separator comma, leaving `{` intact.
    let mut backward = member_start;
    while backward > 0 && matches!(bytes[backward - 1], b' ' | b'\t' | b'\r' | b'\n') {
        backward -= 1;
    }
    if backward > 0 && bytes[backward - 1] == b',' {
        backward -= 1;
    }
    Some((backward, member_end))
}

fn apply_text_edits(text: &str, mut edits: Vec<TextEdit>) -> String {
    edits.sort_by(|left, right| right.start.cmp(&left.start).then(right.end.cmp(&left.end)));
    let mut out = text.to_string();
    for edit in edits {
        out.replace_range(edit.start..edit.end, &edit.replacement);
    }
    out
}

fn has_trailing_comma(text: &str, start: Option<usize>, end: usize) -> bool {
    let Some(mut index) = start else {
        return false;
    };
    let bytes = text.as_bytes();
    while index < end {
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b',' => return true,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < end && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < end && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                    index += 1;
                }
                index = (index + 2).min(end);
            }
            _ => return false,
        }
    }
    false
}

/// Patch `spec` into the `plugin` array of a JSONC document.
///
/// Mirrors `patchPluginList` in reference/packages/opencode/src/plugin/install.ts:
/// dedupes on package name, adds when absent, replaces in place when `force`.
pub fn patch_plugin_list(
    text: &str,
    spec: &str,
    item: &Value,
    pkg: &str,
    force: bool,
) -> Result<(PatchMode, String), JsoncError> {
    let parsed = parse(text)?;
    let Node::Object { members, span } = &parsed.root else {
        return Err(JsoncError::Parse {
            offset: 0,
            message: "root is not an object".into(),
        });
    };
    let plugin = members.iter().find(|(k, _)| k == "plugin").map(|(_, v)| v);

    // Duplicate detection: same spec, or same package name.
    let dup = match plugin {
        Some(Node::Array { items, .. }) => items.iter().enumerate().find(|(_, item)| match item {
            Node::Value {
                value: Value::String(s),
                ..
            } => {
                *s == spec
                    || (crate::loader::parse_plugin_specifier(s).0 == pkg
                        && !s.starts_with("file://"))
            }
            Node::Array { items: tuple, .. } => {
                if let Some(Node::Value {
                    value: Value::String(s),
                    ..
                }) = tuple.first()
                {
                    *s == spec
                        || (crate::loader::parse_plugin_specifier(s).0 == pkg
                            && !s.starts_with("file://"))
                } else {
                    false
                }
            }
            _ => false,
        }),
        _ => None,
    };

    let item_text = serde_json::to_string(item).map_err(|e| JsoncError::Parse {
        offset: 0,
        message: e.to_string(),
    })?;

    match plugin {
        None => {
            // Insert "plugin": [...] before the root close brace.
            let insert_at = span.end - 1;
            let comma = if members.is_empty() { "" } else { "," };
            let snippet = format!("{comma}\"plugin\": [{item_text}]");
            let mut out = text.to_string();
            out.insert_str(insert_at, &snippet);
            Ok((PatchMode::Add, out))
        }
        Some(Node::Array {
            span: arr_span,
            items,
        }) => match dup {
            None => {
                let insert_at = arr_span.end - 1;
                let comma = if items.is_empty() { "" } else { ", " };
                let snippet = format!("{comma}{item_text}");
                let mut out = text.to_string();
                out.insert_str(insert_at, &snippet);
                Ok((PatchMode::Add, out))
            }
            Some(_) if !force => Ok((PatchMode::Noop, text.to_string())),
            Some((index, _)) => {
                // Replace the existing element with the new spec.
                let item = &items[index];
                let span = item.span();
                let mut out = text.to_string();
                out.replace_range(span.start..span.end, &item_text);
                Ok((PatchMode::Replace, out))
            }
        },
        Some(other) => Err(JsoncError::Parse {
            offset: other.span().start,
            message: "plugin must be an array".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jsonc_with_comments() {
        let text = "{\n  // a comment\n  \"plugin\": [\"foo\"],\n  \"theme\": \"x\",\n}";
        let parsed = parse(text).unwrap();
        assert_eq!(parsed.value["plugin"][0], "foo");
        assert_eq!(parsed.value["theme"], "x");
    }

    #[test]
    fn adds_plugin_when_missing() {
        let text = "{\"theme\": \"dark\"}";
        let (mode, out) = patch_plugin_list(
            text,
            "my-plugin",
            &Value::String("my-plugin".into()),
            "my-plugin",
            false,
        )
        .unwrap();
        assert_eq!(mode, PatchMode::Add);
        assert!(out.contains("\"plugin\": [\"my-plugin\"]"));
    }

    #[test]
    fn adds_plugin_to_existing_list() {
        let text = "{\"plugin\": [\"a\"]}";
        let (mode, out) =
            patch_plugin_list(text, "b", &Value::String("b".into()), "b", false).unwrap();
        assert_eq!(mode, PatchMode::Add);
        assert!(out.contains("\"plugin\": [\"a\", \"b\"]"));
    }

    #[test]
    fn dedupes_by_package_name() {
        let text = "{\"plugin\": [\"my-plugin@1.0.0\"]}";
        let (mode, _) = patch_plugin_list(
            text,
            "my-plugin@2.0.0",
            &Value::String("my-plugin@2.0.0".into()),
            "my-plugin",
            false,
        )
        .unwrap();
        assert_eq!(mode, PatchMode::Noop);
    }

    #[test]
    fn replaces_with_force() {
        let text = "{\"plugin\": [\"my-plugin@1.0.0\"]}";
        let (mode, out) = patch_plugin_list(
            text,
            "my-plugin@2.0.0",
            &Value::String("my-plugin@2.0.0".into()),
            "my-plugin",
            true,
        )
        .unwrap();
        assert_eq!(mode, PatchMode::Replace);
        assert!(out.contains("my-plugin@2.0.0"));
        assert!(!out.contains("my-plugin@1.0.0"));
    }

    #[test]
    fn spans_use_utf8_byte_offsets_after_unicode() {
        let text = "{\"label\": \"café\", \"plugin\": [\"old\"]}";
        let parsed = parse(text).unwrap();
        let plugin = parsed.root.member("plugin").expect("plugin member");
        let span = plugin.span();
        assert_eq!(&text[span.start..span.end], "[\"old\"]");
    }

    #[test]
    fn property_patch_preserves_root_comments_and_trailing_comma() {
        let text = "{\n  // keep this comment\n  \"theme\": \"dark\",\n}";
        let (mode, out) = patch_object_property(
            text,
            None,
            "mcp",
            &serde_json::json!({"demo": {"type": "remote"}}),
        )
        .unwrap();
        assert_eq!(mode, PatchMode::Add);
        assert!(out.contains("// keep this comment"));
        assert!(out.contains("\"theme\": \"dark\",\n\"mcp\""));
        assert!(!out.contains(",,\"mcp\""));
        assert_eq!(parse(&out).unwrap().value["mcp"]["demo"]["type"], "remote");
    }

    #[test]
    fn nested_object_patch_preserves_provider_comments_and_trailing_commas() {
        let text = r#"{
  "provider": {
    // provider comment
    "openai": {
      // options comment
      "options": {
        // temperature comment
        "temperature": 0.7,
      },
      // models comment
      "models": {
        // model comment
        "gpt-4": {
          "name": "GPT-4",
        },
      },
    },
  },
}"#;
        let (mode, out) = patch_object_property(
            text,
            None,
            "provider",
            &serde_json::json!({
                "openai": {
                    "options": {"temperature": 0.2, "timeout": 30},
                    "models": {"gpt-4": {"name": "GPT-4o"}},
                }
            }),
        )
        .unwrap();

        assert_eq!(mode, PatchMode::Replace);
        assert!(out.contains("// provider comment"));
        assert!(out.contains("// options comment"));
        assert!(out.contains("// temperature comment"));
        assert!(out.contains("// models comment"));
        assert!(out.contains("// model comment"));
        assert!(out.contains("\"temperature\": 0.2,"));
        assert!(out.contains("\"timeout\": 30"));
        assert!(out.contains("\"name\": \"GPT-4o\","));
        let parsed = parse(&out).unwrap();
        assert_eq!(
            parsed.value["provider"]["openai"]["options"]["temperature"],
            0.2
        );
        assert_eq!(parsed.value["provider"]["openai"]["options"]["timeout"], 30);
        assert_eq!(
            parsed.value["provider"]["openai"]["models"]["gpt-4"]["name"],
            "GPT-4o"
        );
    }

    #[test]
    fn nested_object_deletion_preserves_comments() {
        let text = r#"{
  // keep the outer comment
  "provider": {
    // this nested comment is removed with its key
    "openai": {
      "apiKey": "old",
      "options": {
        "temperature": 0.7,
      },
    },
  },
}"#;
        let (mode, out) = patch_object_property(
            text,
            None,
            "provider",
            &serde_json::json!({
                "openai": {"options": {"temperature": 0.7}}
            }),
        )
        .unwrap();

        assert_eq!(mode, PatchMode::Replace);
        assert!(out.contains("// keep the outer comment"));
        // The deleted key is removed while the surviving sibling's structure
        // (and its own comment) is left intact.
        assert!(!out.contains("apiKey"));
        assert!(out.contains("// this nested comment is removed with its key"));
        assert_eq!(
            parse(&out).unwrap().value["provider"]["openai"]["options"]["temperature"],
            0.7
        );
        // The result remains valid JSONC.
        let parsed = parse(&out).unwrap();
        assert!(!parsed
            .value
            .get("provider")
            .and_then(|v| v.get("openai"))
            .and_then(|v| v.get("apiKey"))
            .is_some());
    }

    #[test]
    fn deleting_trailing_member_absorbs_preceding_comma_and_preserves_comments() {
        // Patching the `provider` object to drop its trailing `openai` key
        // must remove the key plus the preceding separator comma, leaving the
        // surviving `anthropic` member and surrounding comments intact.
        let (mode, out) = patch_object_property(
            "{\n  // provider header\n  \"provider\": {\n    \"anthropic\": {\n      // anthropic comment\n      \"apiKey\": \"x\",\n    },\n    // openai comment\n    \"openai\": {\n      \"apiKey\": \"y\",\n    },\n  },\n}",
            None,
            "provider",
            &serde_json::json!({ "anthropic": { "apiKey": "x" } }),
        )
        .unwrap();
        assert_eq!(mode, PatchMode::Replace);
        assert!(out.contains("// provider header"));
        assert!(out.contains("// anthropic comment"));
        assert!(out.contains("\"apiKey\": \"x\""));
        assert!(!out.contains("\"openai\""));
        assert!(!out.contains("\"apiKey\": \"y\""));
        let parsed = parse(&out).unwrap();
        assert_eq!(parsed.value["provider"]["anthropic"]["apiKey"], "x");
        assert!(parsed.value["provider"].get("openai").is_none());
        assert!(parse(&out).is_ok(), "output must remain valid JSONC");
    }
}
