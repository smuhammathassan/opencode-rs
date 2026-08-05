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
    let mut parser = Parser { chars, pos: 0 };
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
    pos: usize,
}

impl Parser {
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
                    span: Span { start, end },
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
                start,
                end: self.pos,
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
                start,
                end: self.pos,
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
            span: Span { start, end },
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
}
