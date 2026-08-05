//! TypeScript stripping + ESM-to-script transformation so plugin files can run
//! on QuickJS (which only evaluates global scripts).
//!
//! The reference runs `.ts` plugin files directly via Bun's native TypeScript
//! support. There is no JS toolchain in the environment, so this module
//! implements a lexer-based transformer covering the common subset of TS/ESM
//! used by plugins. It is deliberately conservative: unrecognized constructs
//! are passed through, and obvious errors raise a [`TranspileError`].

use std::fmt;

#[derive(Debug)]
pub struct TranspileError {
    message: String,
}

impl fmt::Display for TranspileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TranspileError {}

impl TranspileError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Template(Vec<String>),
    Num(String),
    Punct(String),
    Regex(String),
    Comment(String),
}

impl Token {
    fn is_ident(&self, name: &str) -> bool {
        matches!(self, Token::Ident(id) if id == name)
    }

    fn is_punct(&self, p: &str) -> bool {
        matches!(self, Token::Punct(actual) if actual == p)
    }
}

/// Transpile a TypeScript/ESM module into a global script. Imports become
/// `__oc_require(...)` calls against the polyfill module registry; exports are
/// registered via `__oc_define(name, value)`.
pub fn transpile_module(source: &str) -> Result<String, TranspileError> {
    let tokens = lex(source)?;
    let stripped = strip_types(&tokens)?;
    render(&stripped)
}

#[allow(dead_code)]
pub fn dbg_strip(source: &str) -> String {
    let tokens = lex(source).unwrap();
    let stripped = strip_types(&tokens).unwrap();
    format!("{stripped:?}")
}

#[allow(dead_code)]
pub fn dbg_lex(source: &str) -> String {
    format!("{:?}", lex(source).unwrap())
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

fn lex(source: &str) -> Result<Vec<Token>, TranspileError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' => {
                i += 1;
            }
            '\n' => {
                // Automatic semicolon insertion: a newline separates
                // statements unless the next token continues the expression.
                i += 1;
                if should_insert_semicolon(&chars, i, tokens.last()) {
                    tokens.push(Token::Punct(";".into()));
                }
            }
            '/' if i + 1 < chars.len() && chars[i + 1] == '/' => {
                let start = i;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                tokens.push(Token::Comment(chars[start..i].iter().collect()));
            }
            '/' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                let start = i;
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i = (i + 2).min(chars.len());
                tokens.push(Token::Comment(chars[start..i].iter().collect()));
            }
            '/' => {
                let (text, next) = lex_regex_or_div(&chars, i);
                tokens.push(Token::Regex(text));
                i = next;
            }
            '"' | '\'' => {
                let (text, next) = lex_string(&chars, i, c)?;
                tokens.push(Token::Str(text));
                i = next;
            }
            '`' => {
                let (parts, next) = lex_template(&chars, i)?;
                tokens.push(Token::Template(parts));
                i = next;
            }
            '0'..='9' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_digit()
                        || matches!(
                            chars[i],
                            '.' | 'e' | 'E' | 'x' | 'X' | 'b' | 'B' | 'o' | 'O' | '_'
                        ))
                {
                    i += 1;
                }
                tokens.push(Token::Num(chars[start..i].iter().collect()));
            }
            c if c.is_ascii_alphabetic() || c == '_' || c == '$' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '$')
                {
                    i += 1;
                }
                tokens.push(Token::Ident(chars[start..i].iter().collect()));
            }
            _ => {
                let start = i;
                if matches!(
                    c,
                    '<' | '>' | '!' | '=' | '&' | '|' | '+' | '-' | '*' | '%' | '^' | '?'
                ) {
                    i += 1;
                    while i < chars.len()
                        && matches!(
                            chars[i],
                            '=' | '<' | '>' | '&' | '|' | '+' | '-' | '*' | '%' | '^' | '?' | '!'
                        )
                    {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
                tokens.push(Token::Punct(chars[start..i].iter().collect()));
            }
        }
    }
    Ok(tokens)
}

/// Decide whether a newline at `at` (after the last emitted token) should
/// become an automatic semicolon.
fn should_insert_semicolon(chars: &[char], at: usize, prev: Option<&Token>) -> bool {
    let Some(prev) = prev else { return false };
    // Restricted productions: `return`/`throw`/`break`/`continue` followed by a
    // line terminator end the statement (unless the next token is a
    // terminator itself).
    let restricted = match prev {
        Token::Ident(id) => matches!(
            id.as_str(),
            "return" | "throw" | "break" | "continue" | "debugger"
        ),
        _ => false,
    };
    let next = next_significant(chars, at);
    if restricted {
        return !matches!(
            next.as_deref(),
            Some(";") | Some("}") | Some(",") | Some(")") | Some("]") | None
        );
    }
    // Otherwise only when the previous token can end an expression and the
    // next token starts a new one.
    if !can_end_expression(prev) {
        return false;
    }
    let Some(next) = next else { return false };
    match next.as_str() {
        // Continuation operators: keep parsing the same expression.
        "+" | "-" | "*" | "/" | "%" | "**" | "&&" | "||" | "??" | "?" | "==" | "===" | "!=" | "!=="
        | "<" | ">" | "<=" | ">=" | "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&" | "|" | "^" | "<<" | ">>"
        | ">>>" | "&&=" | "||=" | "??=" | "=>" | "." | "," | ":" | ";" | ")" | "]" | "(" | "[" | "}"
        // Keywords that can never start a statement (no `;` before them).
        | "else" | "catch" | "finally" => {
            false
        }
        // Anything else (identifiers, `{`, `!`, `++`, ...) starts a new
        // statement.
        _ => true,
    }
}

fn next_significant(chars: &[char], mut at: usize) -> Option<String> {
    while at < chars.len() {
        let c = chars[at];
        match c {
            ' ' | '\t' | '\r' | '\n' => at += 1,
            '/' if at + 1 < chars.len() && chars[at + 1] == '/' => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
            }
            '/' if at + 1 < chars.len() && chars[at + 1] == '*' => {
                at += 2;
                while at + 1 < chars.len() && !(chars[at] == '*' && chars[at + 1] == '/') {
                    at += 1;
                }
                at = (at + 2).min(chars.len());
            }
            c if c.is_ascii_alphanumeric() || c == '_' || c == '$' => {
                let start = at;
                while at < chars.len()
                    && (chars[at].is_ascii_alphanumeric() || chars[at] == '_' || chars[at] == '$')
                {
                    at += 1;
                }
                return Some(chars[start..at].iter().collect());
            }
            _ => {
                let start = at;
                if matches!(
                    c,
                    '<' | '>' | '!' | '=' | '&' | '|' | '+' | '-' | '*' | '%' | '^' | '?'
                ) {
                    at += 1;
                    while at < chars.len()
                        && matches!(
                            chars[at],
                            '=' | '<' | '>' | '&' | '|' | '+' | '-' | '*' | '%' | '^' | '?' | '!'
                        )
                    {
                        at += 1;
                    }
                } else {
                    at += 1;
                }
                return Some(chars[start..at].iter().collect());
            }
        }
    }
    None
}

/// Can `token` end an expression statement (making a following newline a
/// statement boundary)?
fn can_end_expression(token: &Token) -> bool {
    match token {
        Token::Str(_) | Token::Num(_) | Token::Template(_) | Token::Regex(_) => true,
        Token::Punct(p) => matches!(p.as_str(), "}" | ")" | "]"),
        Token::Ident(id) => !matches!(
            id.as_str(),
            "if" | "for"
                | "while"
                | "function"
                | "class"
                | "const"
                | "let"
                | "var"
                | "return"
                | "import"
                | "export"
                | "switch"
                | "catch"
                | "do"
                | "else"
                | "try"
                | "finally"
                | "new"
                | "delete"
                | "typeof"
                | "void"
                | "yield"
                | "await"
                | "case"
                | "default"
                | "in"
                | "instanceof"
                | "throw"
                | "break"
                | "continue"
                | "debugger"
                | "async"
                | "interface"
                | "type"
                | "extends"
                | "this"
                | "super"
                | "static"
                | "get"
                | "set"
                | "of"
                | "as"
                | "satisfies"
                | "enum"
                | "namespace"
                | "declare"
                | "abstract"
        ),
        Token::Comment(_) => false,
    }
}

fn lex_string(
    chars: &[char],
    start: usize,
    quote: char,
) -> Result<(String, usize), TranspileError> {
    let mut i = start + 1;
    let mut out = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            if i + 1 >= chars.len() {
                break;
            }
            out.push('\\');
            out.push(chars[i + 1]);
            i += 2;
        } else if c == quote {
            return Ok((out, i + 1));
        } else {
            out.push(c);
            i += 1;
        }
    }
    Err(TranspileError::new("unterminated string literal"))
}

fn lex_template(chars: &[char], start: usize) -> Result<(Vec<String>, usize), TranspileError> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut i = start + 1;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            current.push('\\');
            current.push(chars[i + 1]);
            i += 2;
        } else if c == '`' {
            parts.push(current);
            return Ok((parts, i + 1));
        } else if c == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
            parts.push(current);
            current = String::new();
            let mut depth = 1;
            i += 2;
            while i < chars.len() && depth > 0 {
                let c = chars[i];
                if c == '{' {
                    depth += 1;
                    current.push(c);
                } else if c == '}' {
                    depth -= 1;
                    if depth > 0 {
                        current.push(c);
                    }
                } else {
                    current.push(c);
                }
                i += 1;
            }
            parts.push(current);
            current = String::new();
        } else {
            current.push(c);
            i += 1;
        }
    }
    Err(TranspileError::new("unterminated template literal"))
}

fn lex_regex_or_div(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    let mut in_class = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if c == '[' {
            in_class = true;
        } else if c == ']' {
            in_class = false;
        } else if c == '/' && !in_class {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            return (chars[start..j].iter().collect(), j);
        }
        i += 1;
    }
    (chars[start..].iter().collect(), chars.len())
}

// ---------------------------------------------------------------------------
// Type stripping
// ---------------------------------------------------------------------------

/// Remove TypeScript-only syntax from the token stream: type annotations,
/// `interface`/`type`/`declare` declarations, `as`/`satisfies` casts, generic
/// type arguments in annotation position, and `import type`/`export type`.
fn strip_types(tokens: &[Token]) -> Result<Vec<Token>, TranspileError> {
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        let token = &tokens[i];
        if token.is_ident("import") {
            if is_import_type(tokens, i) {
                i = skip_import_statement(tokens, i);
                continue;
            }
            out.push(token.clone());
            i += 1;
            continue;
        }
        if token.is_ident("export") {
            if is_export_type(tokens, i) {
                i = skip_ts_declaration(tokens, i)?;
                continue;
            }
            out.push(token.clone());
            i += 1;
            continue;
        }
        if token.is_ident("interface")
            || token.is_ident("declare")
            || token.is_ident("enum")
            || token.is_ident("namespace")
            || (token.is_ident("abstract") && is_class_keyword(tokens, i + 1))
        {
            i = skip_ts_declaration(tokens, i)?;
            continue;
        }
        if token.is_ident("type") {
            // `type X = ...` declaration, but not `typeof`.
            if let Some(Token::Ident(id)) = tokens.get(i + 1) {
                if id != "of" {
                    i = skip_ts_declaration(tokens, i)?;
                    continue;
                }
            }
        }
        out.push(token.clone());
        i += 1;
    }
    // Second pass: erase inline type annotations inside the remaining stream.
    let mut i = 0usize;
    let mut result: Vec<Token> = Vec::new();
    while i < out.len() {
        let token = &out[i];
        if token.is_ident("import") {
            // Import statements are opaque to type erasure: `as` inside
            // `{ a as b }` is a rename, not a type cast. Copy them through so
            // the render pass can transform them.
            let end = skip_import_statement(&out, i);
            result.extend_from_slice(&out[i..end]);
            i = end;
            continue;
        }
        if token.is_ident("export") && matches!(out.get(i + 1), Some(Token::Punct(p)) if p == "{") {
            // `export { a as b }` — the `as` is a rename.
            let close = matching(&out, i + 1, "{", "}");
            let end = close.map(|c| c + 1).unwrap_or(i + 1);
            result.extend_from_slice(&out[i..end]);
            i = end;
            continue;
        }
        if token.is_ident("export") && matches!(out.get(i + 1), Some(Token::Punct(p)) if p == "*") {
            // `export * from "..."` — opaque.
            let end = skip_import_statement(&out, i);
            result.extend_from_slice(&out[i..end]);
            i = end;
            continue;
        }
        if token.is_ident("as") || token.is_ident("satisfies") {
            if let Ok((_skipped, next)) = skip_inline_type(&out, i + 1) {
                i = next;
                continue;
            }
            result.push(token.clone());
            i += 1;
            continue;
        }
        if matches!(token, Token::Punct(p) if p == "!") {
            // A postfix `!` is a TypeScript non-null assertion (invalid JS);
            // a prefix `!` is logical negation and must be kept.
            let prev = i.checked_sub(1).and_then(|j| out.get(j));
            let postfix = prev.map(can_end_expression).unwrap_or(false);
            if !postfix {
                result.push(token.clone());
            }
            i += 1;
            continue;
        }
        if is_paren_param_list(&out, i) {
            let close = matching(&out, i, "(", ")");
            if let Some(_close) = close {
                let mut j = i;
                let mut written_close = false;
                result.push(Token::Punct("(".into()));
                j += 1;
                let mut depth = 0i32;
                while j < out.len() {
                    let t = &out[j];
                    if t.is_punct("(") || t.is_punct("{") || t.is_punct("[") {
                        depth += 1;
                        result.push(t.clone());
                        j += 1;
                    } else if t.is_punct(")") {
                        if depth == 0 {
                            result.push(t.clone());
                            j += 1;
                            written_close = true;
                            break;
                        }
                        depth -= 1;
                        result.push(t.clone());
                        j += 1;
                    } else if t.is_punct("}") || t.is_punct("]") {
                        depth -= 1;
                        result.push(t.clone());
                        j += 1;
                    } else if t.is_punct(":") && depth == 0 {
                        // type annotation on a parameter
                        let (_skipped, next) = skip_inline_type(&out, j + 1)?;
                        j = next;
                    } else {
                        result.push(t.clone());
                        j += 1;
                    }
                }
                if written_close {
                    // optional return type: `) : Type =>` or `) : Type {`
                    if let Some(Token::Punct(p)) = out.get(j) {
                        if p == ":" {
                            let (skipped, next) = skip_inline_type(&out, j + 1)?;
                            if skipped {
                                j = next;
                            }
                        }
                    }
                }
                i = j;
                continue;
            }
        }
        if token.is_ident("const") || token.is_ident("let") || token.is_ident("var") {
            // const/let/var NAME: Type = ...
            result.push(token.clone());
            i += 1;
            if let Some(Token::Ident(_)) = out.get(i) {
                result.push(out[i].clone());
                i += 1;
                if let Some(Token::Punct(p)) = out.get(i) {
                    if p == ":" {
                        let (_skipped, next) = skip_inline_type(&out, i + 1)?;
                        i = next;
                        continue;
                    }
                }
            }
            continue;
        }
        if token.is_ident("function") || token.is_ident("class") || token.is_ident("async") {
            // function NAME(...): Type {  /  async NAME(...)  /  class NAME<G> ...
            result.push(token.clone());
            i += 1;
            if token.is_ident("async") {
                // async function / async NAME( ...
                if let Some(Token::Ident(id)) = out.get(i) {
                    if id == "function" {
                        result.push(out[i].clone());
                        i += 1;
                    }
                }
                if let Some(Token::Ident(_)) = out.get(i) {
                    result.push(out[i].clone());
                    i += 1;
                }
                if let Some(Token::Punct(p)) = out.get(i) {
                    if p == "(" {
                        let close = matching(&out, i, "(", ")");
                        if let Some(close) = close {
                            result.push(Token::Punct("(".into()));
                            let mut j = i + 1;
                            let mut depth = 0i32;
                            while j < close {
                                let t = &out[j];
                                if t.is_punct("(") || t.is_punct("{") || t.is_punct("[") {
                                    depth += 1;
                                    result.push(t.clone());
                                    j += 1;
                                } else if t.is_punct(")") {
                                    if depth == 0 {
                                        result.push(t.clone());
                                        j += 1;
                                    } else {
                                        depth -= 1;
                                        result.push(t.clone());
                                        j += 1;
                                    }
                                } else if t.is_punct("}") || t.is_punct("]") {
                                    depth -= 1;
                                    result.push(t.clone());
                                    j += 1;
                                } else if t.is_punct(":") && depth == 0 {
                                    let (_s, next) = skip_inline_type(&out, j + 1)?;
                                    j = next;
                                } else {
                                    result.push(t.clone());
                                    j += 1;
                                }
                            }
                            result.push(Token::Punct(")".into()));
                            i = close + 1;
                            // return type
                            if let Some(Token::Punct(p)) = out.get(i) {
                                if p == ":" {
                                    let (_s, next) = skip_inline_type(&out, i + 1)?;
                                    i = next;
                                    continue;
                                }
                            }
                            continue;
                        }
                    }
                }
            }
            continue;
        }
        // export function NAME( ... ) : Type
        if token.is_ident("export") {
            let next_is_fn = matches!(out.get(i + 1), Some(Token::Ident(id)) if id == "function" || id == "class" || id == "async" || id == "const" || id == "let" || id == "var");
            result.push(token.clone());
            i += 1;
            if next_is_fn {
                continue;
            }
            continue;
        }
        result.push(token.clone());
        i += 1;
    }
    Ok(result)
}

fn is_class_keyword(tokens: &[Token], at: usize) -> bool {
    matches!(tokens.get(at), Some(Token::Ident(id)) if id == "class")
}

fn is_import_type(tokens: &[Token], at: usize) -> bool {
    let i = at + 1;
    // `import type ...` — type-only regardless of braces.
    if matches!(tokens.get(i), Some(Token::Ident(id)) if id == "type") {
        return true;
    }
    if matches!(tokens.get(i), Some(Token::Punct(p)) if p == "{") {
        // `import { type A, b }` — type-only only if every named binding is
        // preceded by the `type` keyword.
        let mut j = i + 1;
        let mut pending_type = false;
        let mut all_type = true;
        while let Some(t) = tokens.get(j) {
            if t.is_punct("}") {
                break;
            }
            if t.is_ident("type") {
                pending_type = true;
            } else if matches!(t, Token::Ident(_)) {
                if !pending_type {
                    all_type = false;
                    break;
                }
                pending_type = false;
            } else if t.is_punct(",") {
                pending_type = false;
            }
            j += 1;
        }
        return all_type;
    }
    false
}

fn skip_import_statement(tokens: &[Token], at: usize) -> usize {
    let mut i = at + 1;
    while let Some(t) = tokens.get(i) {
        if t.is_punct(";") {
            return i + 1;
        }
        if matches!(t, Token::Str(_)) {
            // end of specifier; skip `with { ... }` if present
            let mut j = i + 1;
            while let Some(t2) = tokens.get(j) {
                if t2.is_punct(";") {
                    return j + 1;
                }
                j += 1;
            }
            return i + 1;
        }
        i += 1;
    }
    tokens.len()
}

fn is_export_type(tokens: &[Token], at: usize) -> bool {
    let i = at + 1;
    matches!(
        tokens.get(i),
        Some(Token::Ident(id)) if id == "type" || id == "interface"
    )
}

/// Skip an inline type annotation (`: Type` or after `as`/`satisfies`). Returns
/// `(true, next_index)` when a type was consumed, else `(false, at)`.
fn skip_inline_type(tokens: &[Token], at: usize) -> Result<(bool, usize), TranspileError> {
    if at >= tokens.len() {
        return Ok((false, at));
    }
    let mut i = at;
    let mut depth = 0i32;
    while i < tokens.len() {
        let t = &tokens[i];
        match t {
            Token::Punct(p) => match p.as_str() {
                "<" | "(" | "{" | "[" => depth += 1,
                ">" | ")" | "}" | "]" if depth == 0 => return Ok((true, i)),
                ">" | ")" | "}" | "]" => depth -= 1,
                "," | "=" | ";" if depth == 0 => return Ok((true, i)),
                "," | "=" | ";" => {}
                _ => {}
            },
            Token::Ident(id) if id == "=>" => return Ok((true, i)),
            Token::Ident(_) => {}
            _ => {}
        }
        i += 1;
    }
    Ok((true, i))
}

/// Skip a TS declaration block (`interface`/`type`/`declare`/`namespace`).
fn skip_ts_declaration(tokens: &[Token], at: usize) -> Result<usize, TranspileError> {
    let mut i = at;
    let mut brace_depth = 0i32;
    let mut paren_depth = 0i32;
    let mut angle_depth = 0i32;
    let mut seen_brace = false;
    while i < tokens.len() {
        if let Token::Punct(p) = &tokens[i] {
            match p.as_str() {
                "{" => {
                    seen_brace = true;
                    brace_depth += 1;
                }
                "}" => {
                    brace_depth -= 1;
                    if seen_brace && brace_depth == 0 {
                        return Ok(i + 1);
                    }
                }
                "(" => paren_depth += 1,
                ")" => {
                    paren_depth -= 1;
                }
                "<" => angle_depth += 1,
                ">" => {
                    angle_depth -= 1;
                }
                ";" if !seen_brace && paren_depth == 0 && angle_depth <= 0 => {
                    return Ok(i + 1);
                }
                ";" => {}
                _ => {}
            }
        }
        i += 1;
    }
    Ok(i)
}

/// Find the index of the matching closing punct for the pair at `at`.
fn matching(tokens: &[Token], at: usize, open: &str, close: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate().skip(at) {
        if t.is_punct(open) {
            depth += 1;
        } else if t.is_punct(close) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Heuristic: is the `(` at `at` the parameter list of a function/arrow?
fn is_paren_param_list(tokens: &[Token], at: usize) -> bool {
    if !matches!(tokens.get(at), Some(Token::Punct(p)) if p == "(") {
        return false;
    }
    if at == 0 {
        return true;
    }
    let prev = &tokens[at - 1];
    match prev {
        Token::Punct(p) => matches!(
            p.as_str(),
            "=" | "(" | "{" | "[" | "," | ":" | "=>" | "&" | "|" | "return"
        ),
        Token::Ident(id) => {
            if id == "function" || id == "async" || id == "of" || id == "in" {
                return true;
            }
            // `function NAME (`
            if at >= 2 && matches!(&tokens[at - 2], Token::Ident(id2) if id2 == "function") {
                return true;
            }
            false
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Rendering with import/export transforms
// ---------------------------------------------------------------------------

fn render(tokens: &[Token]) -> Result<String, TranspileError> {
    let mut out = String::new();
    let mut i = 0usize;
    while i < tokens.len() {
        let token = &tokens[i];
        if token.is_ident("import") {
            let (code, consumed) = render_import(tokens, i)?;
            out.push_str(&code);
            i += consumed;
            continue;
        }
        if token.is_ident("export") {
            let (code, consumed) = render_export(tokens, i)?;
            out.push_str(&code);
            i += consumed;
            continue;
        }
        match token {
            Token::Comment(text) => {
                if text.starts_with("//") {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
                i += 1;
            }
            _ => {
                let (text, consumed) = render_expr(tokens, i)?;
                out.push_str(&text);
                i += consumed;
            }
        }
    }
    Ok(out)
}

fn render_import(tokens: &[Token], at: usize) -> Result<(String, usize), TranspileError> {
    let mut i = at + 1;
    let mut default: Option<String> = None;
    let mut names: Vec<(String, Option<String>)> = Vec::new();
    let mut namespace: Option<String> = None;
    let mut spec: Option<String> = None;
    let mut saw_open = false;
    let mut saw_star = false;
    let mut last_was_as = false;

    while i < tokens.len() {
        match &tokens[i] {
            Token::Str(s) => {
                spec = Some(s.clone());
                i += 1;
                break;
            }
            Token::Ident(id) if id == "from" => {
                i += 1;
                if let Some(Token::Str(s)) = tokens.get(i) {
                    spec = Some(s.clone());
                    i += 1;
                    break;
                }
                return Err(TranspileError::new("import 'from' without specifier"));
            }
            Token::Ident(id) if id == "type" => {
                i += 1;
            }
            Token::Ident(id) if id == "as" && saw_star => {
                last_was_as = true;
                i += 1;
            }
            Token::Ident(id) if id == "as" => {
                last_was_as = true;
                i += 1;
            }
            Token::Ident(id) => {
                if last_was_as {
                    if saw_star {
                        namespace = Some(id.clone());
                    } else if let Some((export_name, alias)) = names.last_mut() {
                        *alias = Some(id.clone());
                        let _ = export_name;
                    }
                    last_was_as = false;
                    i += 1;
                } else if saw_open {
                    names.push((id.clone(), None));
                    i += 1;
                } else if default.is_none() && names.is_empty() && namespace.is_none() && !saw_star
                {
                    default = Some(id.clone());
                    i += 1;
                } else {
                    names.push((id.clone(), None));
                    i += 1;
                }
            }
            Token::Punct(p) => match p.as_str() {
                "{" => {
                    saw_open = true;
                    i += 1;
                }
                "}" => {
                    i += 1;
                }
                "," => {
                    i += 1;
                }
                "*" => {
                    saw_star = true;
                    i += 1;
                }
                _ => {
                    return Err(TranspileError::new(format!(
                        "unexpected token '{p}' in import"
                    )));
                }
            },
            _ => {
                i += 1;
            }
        }
    }

    // Skip trailing `with { ... }` / `assert { ... }`.
    while i < tokens.len() {
        match &tokens[i] {
            Token::Ident(id) if id == "with" || id == "assert" => {
                let Some(close) = matching(tokens, i + 1, "{", "}") else {
                    return Err(TranspileError::new("unterminated import attributes"));
                };
                i = close + 1;
            }
            Token::Punct(p) if p == ";" => {
                i += 1;
                break;
            }
            _ => break,
        }
    }

    let Some(spec) = spec else {
        return Err(TranspileError::new("import without specifier"));
    };

    let mut code = String::new();
    if !saw_open && !saw_star && default.is_none() && names.is_empty() {
        // side-effect import
        code.push_str(&format!("__oc_require({spec:?});"));
        return Ok((code, i - at));
    }
    if let Some(ns) = &namespace {
        code.push_str(&format!("const {ns} = __oc_require({spec:?});"));
    }
    if let Some(default) = &default {
        code.push_str(&format!(
            "const {default} = __oc_require({spec:?}).default;"
        ));
    }
    for (name, alias) in &names {
        let target = alias.as_deref().unwrap_or(name);
        code.push_str(&format!("const {target} = __oc_require({spec:?}).{name};"));
    }
    Ok((code, i - at))
}

fn render_export(tokens: &[Token], at: usize) -> Result<(String, usize), TranspileError> {
    let mut i = at + 1;
    let Some(next) = tokens.get(i) else {
        return Err(TranspileError::new("unterminated export"));
    };
    match next {
        Token::Punct(p) if p == "{" => {
            // export { a, b as c }
            let close = matching(tokens, i, "{", "}")
                .ok_or_else(|| TranspileError::new("unterminated export list"))?;
            let mut items: Vec<(String, String)> = Vec::new();
            let mut j = i + 1;
            let mut pending: Option<String> = None;
            let mut last_punct = "";
            while j < close {
                match &tokens[j] {
                    Token::Ident(id) => {
                        if last_punct == "as" {
                            if let Some(name) = pending.take() {
                                items.push((name, id.clone()));
                            }
                        } else {
                            pending = Some(id.clone());
                        }
                        last_punct = "";
                    }
                    Token::Punct(p2) if p2 == "as" => {
                        last_punct = "as";
                    }
                    Token::Punct(p2) if p2 == "," => {
                        if let Some(name) = pending.take() {
                            items.push((name.clone(), name));
                        }
                        last_punct = "";
                    }
                    _ => {}
                }
                j += 1;
            }
            if let Some(name) = pending.take() {
                items.push((name.clone(), name));
            }
            i = close + 1;
            // Re-export from another module?
            let mut code = String::new();
            if let Some(Token::Ident(id)) = tokens.get(i) {
                if id == "from" {
                    if let Some(Token::Str(spec)) = tokens.get(i + 1) {
                        for (name, alias) in &items {
                            code.push_str(&format!(
                                "__oc_define({alias:?}, __oc_require({spec:?}).{name});"
                            ));
                        }
                        i += 2;
                    }
                    return Ok((code, i - at));
                }
            }
            for (name, alias) in &items {
                code.push_str(&format!("__oc_define({alias:?}, {name});"));
            }
            Ok((code, i - at))
        }
        Token::Punct(p) if p == "*" => {
            // export * from "..."
            let mut j = i + 1;
            while j < tokens.len() {
                if let Token::Ident(id) = &tokens[j] {
                    if id == "from" {
                        if let Some(Token::Str(spec)) = tokens.get(j + 1) {
                            return Ok((
                                format!("__oc_export_all(__oc_require({spec:?}));"),
                                j + 2 - at,
                            ));
                        }
                    }
                }
                j += 1;
            }
            Err(TranspileError::new("export * without specifier"))
        }
        Token::Ident(id) if id == "default" => {
            // export default <expr>
            let j = i + 1;
            // Skip type-only trailing: export default <expr> as Foo
            let (text, consumed) = render_expr(tokens, j)?;
            let text = text.trim_end();
            let text = text
                .strip_suffix(';')
                .unwrap_or(text)
                .trim_end()
                .to_string();
            let mut code = format!("__oc_define(\"default\", {text});");
            // export default async function name() {...}
            if text.starts_with("function") || text.starts_with("async function") {
                code = format!("{text}\n__oc_define(\"default\", {});", fn_name(&text));
            } else if text.starts_with("class") {
                code = format!("{text}\n__oc_define(\"default\", {});", class_name(&text));
            }
            Ok((code, j + consumed - at))
        }
        Token::Ident(id)
            if id == "const"
                || id == "let"
                || id == "var"
                || id == "function"
                || id == "class"
                || id == "async" =>
        {
            let (text, consumed) = render_expr(tokens, i)?;
            let names = declared_names(&text);
            let mut code = text;
            // Separate the declaration from the export registration (the source
            // may not end with a `;`).
            for name in names {
                code.push_str(&format!(";\n__oc_define({name:?}, {name});"));
            }
            Ok((code, i + consumed - at))
        }
        _ => Err(TranspileError::new("unsupported export form")),
    }
}

fn fn_name(text: &str) -> String {
    let rest = text
        .trim_start()
        .strip_prefix("async")
        .map(str::trim_start)
        .unwrap_or(text.trim_start());
    let rest = rest.strip_prefix("function").unwrap_or(rest).trim_start();
    rest.split(|c: char| c.is_whitespace() || c == '(' || c == '<')
        .next()
        .unwrap_or("default")
        .trim()
        .to_string()
}

fn class_name(text: &str) -> String {
    let rest = text
        .trim_start()
        .strip_prefix("class")
        .unwrap_or(text.trim_start())
        .trim_start();
    rest.split(|c: char| c.is_whitespace() || c == '{' || c == '<')
        .next()
        .unwrap_or("default")
        .trim()
        .to_string()
}

fn declared_names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim_start();
    for keyword in ["const", "let", "var", "function", "async function", "class"] {
        if let Some(after) = rest.strip_prefix(keyword) {
            rest = after.trim_start();
            break;
        }
    }
    // Skip `async function` prefix handled above.
    let name = rest
        .split(|c: char| {
            c.is_whitespace() || c == '(' || c == '{' || c == '<' || c == '=' || c == ':'
        })
        .next()
        .unwrap_or("");
    if !name.is_empty() {
        out.push(name.to_string());
    }
    out
}

fn render_expr(tokens: &[Token], at: usize) -> Result<(String, usize), TranspileError> {
    let mut out = String::new();
    let mut i = at;
    let mut depth = 0i32;
    let mut word = false;
    while i < tokens.len() {
        let token = &tokens[i];
        match token {
            Token::Punct(p) => {
                match p.as_str() {
                    "{" | "(" | "[" => depth += 1,
                    "}" | ")" | "]" => depth -= 1,
                    ";" if depth <= 0 => {
                        out.push(';');
                        i += 1;
                        break;
                    }
                    _ => {}
                }
                out.push_str(p);
                word = false;
                i += 1;
            }
            Token::Ident(id) => {
                // At statement level, a new `import`/`export` keyword starts a
                // new statement (sources often omit the trailing `;`).
                if depth <= 0 && (id == "export" || id == "import") {
                    break;
                }
                if id == "import" && matches!(tokens.get(i + 1), Some(Token::Punct(p)) if p == "(")
                {
                    let (text, consumed) = render_dynamic_import(tokens, i)?;
                    out.push_str(&text);
                    word = false;
                    i += consumed;
                    continue;
                }
                if word {
                    out.push(' ');
                }
                out.push_str(id);
                word = true;
                i += 1;
            }
            Token::Str(s) => {
                out.push('"');
                out.push_str(&escape_quote(s));
                out.push('"');
                i += 1;
            }
            Token::Template(parts) => {
                out.push('`');
                for (idx, part) in parts.iter().enumerate() {
                    if idx % 2 == 0 {
                        out.push_str(part);
                    } else {
                        out.push_str("${");
                        out.push_str(part);
                        out.push('}');
                    }
                }
                out.push('`');
                i += 1;
            }
            Token::Num(n) => {
                out.push_str(n);
                i += 1;
            }
            Token::Regex(r) => {
                out.push('/');
                out.push_str(r);
                out.push('/');
                i += 1;
            }
            Token::Comment(text) => {
                if text.starts_with("//") {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
                i += 1;
            }
        }
    }
    Ok((out, i - at))
}

fn render_dynamic_import(tokens: &[Token], at: usize) -> Result<(String, usize), TranspileError> {
    let Some(Token::Punct(p)) = tokens.get(at + 1) else {
        return Err(TranspileError::new("invalid dynamic import"));
    };
    if p != "(" {
        return Err(TranspileError::new("invalid dynamic import"));
    }
    let Some(Token::Str(spec)) = tokens.get(at + 2) else {
        return Err(TranspileError::new("dynamic import requires a string"));
    };
    if !matches!(tokens.get(at + 3), Some(Token::Punct(p2)) if p2 == ")") {
        return Err(TranspileError::new("unterminated dynamic import"));
    }
    Ok((format!("__oc_import({spec:?})"), 4))
}

fn escape_quote(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
        transpile_module(s).unwrap()
    }

    #[test]
    fn strips_type_annotations() {
        let out = strip("const x: number = 1;");
        assert!(!out.contains("number"));
        assert!(out.contains("const x=1;"));

        let out = strip("export const ExamplePlugin: Plugin = async (_ctx) => { return 1; }");
        assert!(!out.contains(": Plugin"));
        assert!(out.contains("async(_ctx)"));
        assert!(out.contains("__oc_define(\"ExamplePlugin\", ExamplePlugin);"));
    }

    #[test]
    fn strips_interfaces() {
        let out = strip("interface Foo { a: string }\nconst x: Foo = { a: \"b\" };");
        assert!(!out.contains("interface"));
        assert!(out.contains("const x={a:\"b\"};"));
    }

    #[test]
    fn erases_import_type() {
        let out = strip("import type { Plugin } from \"opencode/plugin\"\nconst x: Plugin = 1;");
        assert!(!out.contains("opencode/plugin"));
        assert!(out.contains("const x=1;"));
    }

    #[test]
    fn transforms_imports() {
        let out = strip("import { tool } from \"opencode/plugin/tool\"\nimport { mkdir } from \"node:fs/promises\"");
        assert!(out.contains("const tool = __oc_require(\"opencode/plugin/tool\").tool;"));
        assert!(out.contains("const mkdir = __oc_require(\"node:fs/promises\").mkdir;"));
    }

    #[test]
    fn transforms_import_alias() {
        let out = strip("import { a as b } from \"x\"");
        assert!(out.contains("const b = __oc_require(\"x\").a;"));
    }

    #[test]
    fn transforms_default_export_function() {
        let out = strip("export default async ({ experimental_workspace }) => { return {}; }");
        assert!(
            out.contains("__oc_define(\"default\", async({experimental_workspace})=>{return{};});")
        );
    }

    #[test]
    fn transforms_export_const() {
        let out = strip("export const foo = 1;");
        assert!(out.contains("const foo=1;"));
        assert!(out.contains("__oc_define(\"foo\", foo);"));
    }

    #[test]
    fn keeps_arrow_bodies_intact() {
        let out = strip("export const f = async (args) => {\n  return `Hello ${args.foo}!`\n}");
        assert!(out.contains("return`Hello ${args.foo}!`"));
        assert!(out.contains("__oc_define(\"f\", f);"));
    }

    #[test]
    fn applies_asi_on_newline_statements() {
        let out = strip("const a = 1\nconst b = 2\nreturn a");
        assert!(out.contains("const a=1;const b=2;return a"));
    }

    #[test]
    fn applies_asi_after_return() {
        let out = strip("async function f() {\n  if (!x) return\n  await g()\n}");
        assert!(out.contains("return;await g()"));
    }

    #[test]
    fn handles_reference_example() {
        let source = r#"
import { Plugin } from "./index.js"
import { tool } from "./tool.js"

export const ExamplePlugin: Plugin = async (_ctx) => {
  return {
    tool: {
      mytool: tool({
        description: "This is a custom tool",
        args: {
          foo: tool.schema.string().describe("foo"),
        },
        async execute(args) {
          return `Hello ${args.foo}!`
        },
      }),
    },
  }
}
"#;
        let out = strip(source);
        assert!(out.contains("const tool = __oc_require(\"./tool.js\").tool;"));
        assert!(out.contains("__oc_define(\"ExamplePlugin\", ExamplePlugin);"));
        assert!(out.contains("async execute(args)"));
        assert!(out.contains("Hello"));
    }

    #[test]
    fn handles_reference_example_workspace() {
        let source = r#"
import type { Plugin } from "@opencode-ai/plugin"
import { mkdir, rm } from "node:fs/promises"

export const FolderWorkspacePlugin: Plugin = async ({ experimental_workspace }) => {
  experimental_workspace.register("folder", {
    name: "Folder",
    description: "Create a blank folder",
    configure(config) {
      const rand = "" + Math.random()

      return {
        ...config,
        directory: `/tmp/folder/folder-${rand}`,
      }
    },
    async create(config) {
      if (!config.directory) return
      await mkdir(config.directory, { recursive: true })
    },
    async remove(config) {
      await rm(config.directory!, { recursive: true, force: true })
    },
    target(config) {
      return {
        type: "local",
        directory: config.directory!,
      }
    },
  })

  return {}
}

export default FolderWorkspacePlugin
"#;
        let out = strip(source);
        assert!(!out.contains("@opencode-ai/plugin"));
        assert!(out.contains("const mkdir = __oc_require(\"node:fs/promises\").mkdir;"));
        assert!(out.contains("experimental_workspace.register(\"folder\""));
        assert!(out.contains("__oc_define(\"default\", FolderWorkspacePlugin);"));
    }
}
