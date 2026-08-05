//! Session message list rendering.
//!
//! Port of `reference/packages/tui/src/routes/session/index.tsx` (UserMessage,
//! AssistantMessage, TextPart, ReasoningPart, ToolPart and its per-tool
//! renderers) plus `component/todo-item.tsx`. Produces styled lines consumed
//! by the session route's scrollable message list.

use std::collections::HashSet;

use ratatui::style::{Color, Modifier, Style};

use crate::components::text::{styled, wrap_plain, StyledLine};
use crate::sync::SyncState;
use crate::theme::{selected_foreground, Theme};
use crate::types::{Message, Part, ToolPart, ToolState};
use crate::util::display::*;
use crate::util::format::collapse_tool_output;
use crate::util::locale;
use crate::util::markdown;
use crate::util::path_format::{format_path, normalize_path};

pub struct SessionRender<'a> {
    pub width: usize,
    pub sync: &'a SyncState,
    pub theme: &'a Theme,
    pub conceal: bool,
    pub thinking_mode: &'a str,
    pub show_timestamps: bool,
    pub show_details: bool,
    pub show_generic_tool_output: bool,
    pub expanded_tools: &'a HashSet<String>,
    pub reasoning_expanded: &'a HashSet<String>,
    pub session_id: &'a str,
    pub cwd: &'a str,
}

impl SessionRender<'_> {
    fn t(&self) -> &Theme {
        self.theme
    }
    fn fg(&self, color: Color) -> Style {
        Style::default().fg(color)
    }
    fn content_width(&self) -> usize {
        self.width.saturating_sub(4)
    }
}

/// One rendered message-list line, tagged with the message it belongs to (for
/// scroll-to-message navigation).
pub struct MessageLine {
    pub line: StyledLine,
    pub owner: Option<String>,
}

impl MessageLine {
    fn new(line: StyledLine) -> Self {
        MessageLine { line, owner: None }
    }
    fn for_message(line: StyledLine, message_id: &str) -> Self {
        MessageLine {
            line,
            owner: Some(message_id.to_string()),
        }
    }
}

/// Render the full message list for a session.
/// From reference/packages/tui/src/routes/session/index.tsx (`Session`)
pub fn render_messages(render: &SessionRender) -> Vec<MessageLine> {
    let messages = render.sync.messages_for(render.session_id);
    let theme = render.t();
    let mut out: Vec<MessageLine> = Vec::new();
    let mut first = true;

    let session = render.sync.session(render.session_id);
    let revert_id = session.and_then(|s| s.revert.as_ref().map(|r| r.message_id.clone()));

    let pending_id = find_pending(messages);
    let last_assistant_id = messages.iter().rev().find_map(|m| match m {
        Message::Assistant(a) => Some(a.id.clone()),
        _ => None,
    });

    for message in messages {
        if let Some(revert) = &revert_id {
            if &message.id() == revert {
                if !first {
                    out.push(MessageLine::new(empty_line()));
                }
                let reverted = messages
                    .iter()
                    .filter(|m| m.id() >= revert.as_str() && m.role() == "user")
                    .count();
                out.push(MessageLine::new(styled(
                    format!(
                        "{reverted} message{} reverted",
                        if reverted == 1 { "" } else { "s" }
                    ),
                    render.fg(theme.text_muted),
                )));
            }
            if message.id() >= revert.as_str() {
                continue;
            }
        }

        match message {
            Message::User(user) => {
                if !first {
                    out.push(MessageLine::new(empty_line()));
                }
                first = false;
                let queued = pending_id
                    .as_ref()
                    .is_some_and(|p| user.id.as_str() > p.as_str());
                render_user_message(render, user, queued, &mut out);
            }
            Message::Assistant(assistant) => {
                if !first {
                    out.push(MessageLine::new(empty_line()));
                }
                first = false;
                let last = last_assistant_id.as_deref() == Some(assistant.id.as_str());
                render_assistant_message(render, assistant, last, &mut out);
            }
        }
    }
    out
}

fn find_pending(messages: &[Message]) -> Option<String> {
    let completed = messages
        .iter()
        .rev()
        .find(|m| {
            m.role() == "assistant"
                && matches!(m, Message::Assistant(a) if a.time.completed.is_some())
        })
        .map(|m| m.id().to_string());
    messages
        .iter()
        .rev()
        .find(|m| {
            if m.role() != "assistant" {
                return false;
            }
            match m {
                Message::Assistant(a) => {
                    a.time.completed.is_none()
                        && completed.as_deref().is_none_or(|c| a.id.as_str() > c)
                }
                _ => false,
            }
        })
        .map(|m| m.id().to_string())
}

fn empty_line() -> StyledLine {
    Vec::new()
}

/// From reference/packages/tui/src/routes/session/index.tsx (`UserMessage`)
fn render_user_message(
    render: &SessionRender,
    user: &crate::types::UserMessage,
    queued: bool,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let parts = render.sync.parts_for(&user.id);
    let texts: Vec<String> = parts
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) if !t.is_synthetic() => Some(t.text.clone()),
            _ => None,
        })
        .collect();
    let text = texts.join("\n\n");
    let files: Vec<&crate::types::FilePart> = parts
        .iter()
        .filter_map(|p| match p {
            Part::File(f) => Some(f),
            _ => None,
        })
        .collect();
    let agent_color = render.theme.agent_color(&user.agent, &render.sync.agents);
    let has_compaction = parts.iter().any(|p| matches!(p, Part::Compaction(_)));

    if !text.trim().is_empty() {
        let border = render.fg(agent_color);
        for line in wrap_plain(&text, render.content_width().saturating_sub(2)) {
            let mut spans = vec![
                ("┃".to_string(), border),
                ("  ".to_string(), Style::default()),
            ];
            spans.extend(line);
            let mut msg = MessageLine::for_message(spans, &user.id);
            bg_panel(theme, &mut msg.line);
            out.push(msg);
        }
        if !files.is_empty() {
            let mut spans = vec![("┃  ".to_string(), render.fg(agent_color))];
            let mut first_file = true;
            for file in files {
                if !first_file {
                    spans.push((" ".to_string(), Style::default()));
                }
                first_file = false;
                let directory = file.mime == "application/x-directory";
                let label = if directory { " Directory " } else { " File " };
                spans.push((
                    label.to_string(),
                    render.fg(theme.background).bg(theme.secondary),
                ));
                spans.push((
                    format!(" {} ", file.filename.as_deref().unwrap_or("")),
                    render.fg(theme.text_muted).bg(theme.background_element),
                ));
            }
            let mut msg = MessageLine::for_message(spans, &user.id);
            bg_panel(theme, &mut msg.line);
            out.push(msg);
        }
        // Metadata line.
        let mut spans = vec![("┃  ".to_string(), render.fg(agent_color))];
        if queued {
            spans.push((
                " QUEUED ".to_string(),
                render
                    .fg(selected_foreground(agent_color))
                    .bg(agent_color)
                    .add_modifier(Modifier::BOLD),
            ));
        } else if render.show_timestamps {
            spans.push((
                locale::today_time_or_datetime(user.time.created),
                render.fg(theme.text_muted),
            ));
        }
        let mut msg = MessageLine::for_message(spans, &user.id);
        bg_panel(theme, &mut msg.line);
        out.push(msg);
    }

    if has_compaction {
        let sep = "─".repeat(render.width.saturating_sub(2));
        out.push(MessageLine::new(styled(
            format!("{sep} Compaction {sep}"),
            render.fg(theme.border_active),
        )));
    }
}

fn bg_panel(theme: &Theme, line: &mut StyledLine) {
    for span in line {
        if span.0 != "┃" && span.0 != "  " && span.0 != "┃  " {
            span.1 = span.1.bg(theme.background_panel);
        }
    }
}

/// From reference/packages/tui/src/routes/session/index.tsx (`AssistantMessage`)
fn render_assistant_message(
    render: &SessionRender,
    assistant: &crate::types::AssistantMessage,
    last: bool,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let parts = render.sync.parts_for(&assistant.id);
    let mut first_part = true;
    for part in parts {
        if !first_part {
            out.push(MessageLine::new(empty_line()));
        }
        first_part = false;
        match part {
            Part::Text(t) => render_text_part(render, t, &assistant.id, out),
            Part::Reasoning(r) => render_reasoning_part(render, r, &assistant.id, out),
            Part::Tool(t) => {
                if !should_hide_tool(render, t) {
                    render_tool_part(render, t, out);
                }
            }
            _ => {}
        }
    }

    // Subagent hint row.
    let has_task = parts
        .iter()
        .any(|p| matches!(p, Part::Tool(t) if t.tool == "task"));
    if has_task {
        out.push(MessageLine::new(styled(
            format!("   {} view subagents", crate::keymap::leader_key_name()),
            render.fg(theme.text_muted),
        )));
    }

    // Error block.
    if let Some(error) = &assistant.error {
        if error.name != "MessageAbortedError" {
            if !first_part {
                out.push(MessageLine::new(empty_line()));
            }
            for line in wrap_plain(&error.message(), render.content_width().saturating_sub(3)) {
                let mut spans = vec![("┃  ".to_string(), render.fg(theme.error))];
                spans.extend(line);
                let mut msg = MessageLine::for_message(spans, &assistant.id);
                for span in &mut msg.line {
                    if span.0 != "┃  " {
                        span.1 = span.1.bg(theme.background_panel);
                    }
                }
                out.push(msg);
            }
        }
    }

    // Footer.
    let interrupted = assistant
        .error
        .as_ref()
        .is_some_and(|e| e.name == "MessageAbortedError");
    let final_finish = assistant
        .finish
        .as_deref()
        .is_some_and(|f| !matches!(f, "tool-calls" | "unknown"));
    if last || final_finish || interrupted {
        let model = model_label(render, &assistant.provider_id, &assistant.model_id);
        let mut spans: StyledLine = Vec::new();
        spans.push((
            "▣ ".to_string(),
            render.fg(if interrupted {
                theme.text_muted
            } else {
                render
                    .theme
                    .agent_color(&assistant.agent, &render.sync.agents)
            }),
        ));
        spans.push((" ".to_string(), Style::default()));
        spans.push((locale::titlecase(&assistant.mode), render.fg(theme.text)));
        if !model.is_empty() {
            spans.push((" · ".to_string(), render.fg(theme.text_muted)));
            spans.push((model, render.fg(theme.text_muted)));
        }
        if let Some(completed) = assistant.time.completed {
            let duration = (completed - assistant.time.created).max(0);
            if duration > 0 {
                spans.push((" · ".to_string(), render.fg(theme.text_muted)));
                spans.push((locale::duration(duration), render.fg(theme.text_muted)));
            }
        }
        if interrupted {
            spans.push((" · ".to_string(), render.fg(theme.text_muted)));
            spans.push(("interrupted".to_string(), render.fg(theme.text_muted)));
        }
        let mut line = vec![("   ".to_string(), Style::default())];
        line.extend(spans);
        out.push(MessageLine::for_message(line, &assistant.id));
    }
}

fn model_label(render: &SessionRender, provider_id: &str, model_id: &str) -> String {
    let provider = render.sync.providers.iter().find(|p| p.id == provider_id);
    let model = provider
        .and_then(|p| p.models.get(model_id))
        .map(|m| m.name.clone())
        .unwrap_or_else(|| model_id.to_string());
    format!(
        "{}/{}",
        provider.map(|p| p.name.as_str()).unwrap_or(provider_id),
        model
    )
}

fn should_hide_tool(render: &SessionRender, tool: &ToolPart) -> bool {
    !render.show_details && tool.state_status() == "completed"
}

/// From reference/packages/tui/src/routes/session/index.tsx (`TextPart`)
fn render_text_part(
    render: &SessionRender,
    part: &crate::types::TextPart,
    message_id: &str,
    out: &mut Vec<MessageLine>,
) {
    let text = part.text.trim();
    if text.is_empty() {
        return;
    }
    let theme = render.t();
    let options = markdown::MarkdownOptions {
        width: render.content_width().saturating_sub(3).max(20),
        conceal: render.conceal,
        fg: theme.markdown_text,
        heading: theme.markdown_heading,
        code: theme.markdown_code,
        muted: theme.text_muted,
    };
    let rendered = markdown::render(text, &options);
    for line in rendered {
        if line.spans.is_empty() {
            continue;
        }
        let mut spans = vec![("   ".to_string(), Style::default())];
        for span in line.spans {
            let mut style = Style::default().fg(span.style.fg.unwrap_or(theme.markdown_text));
            if span.style.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if span.style.italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if span.style.dim {
                style = style.add_modifier(Modifier::DIM);
            }
            spans.push((span.text, style));
        }
        out.push(MessageLine::for_message(spans, message_id));
    }
}

/// From reference/packages/tui/src/routes/session/index.tsx (`ReasoningPart`)
fn render_reasoning_part(
    render: &SessionRender,
    part: &crate::types::ReasoningPart,
    message_id: &str,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let content = part.text.replace("[REDACTED]", "").trim().to_string();
    if content.is_empty() {
        return;
    }
    let minimal = render.thinking_mode == "hide";
    let expanded = render.reasoning_expanded.contains(&part.id);
    let (title, body) = reasoning_summary(&content);
    let duration = part
        .time
        .end
        .map(|end| locale::duration((end - part.time.start).max(0)));

    let mut header: StyledLine = vec![("   ".to_string(), Style::default())];
    if minimal {
        header.push((
            if expanded { "- " } else { "+ " }.to_string(),
            render.fg(theme.warning),
        ));
    }
    header.push(("Thought".to_string(), render.fg(theme.warning)));
    if title.is_some() || duration.is_some() {
        header.push((": ".to_string(), render.fg(theme.warning)));
    }
    if let Some(title) = &title {
        header.push((title.clone(), render.fg(theme.warning)));
    }
    if let Some(duration) = &duration {
        let prefix = if title.is_some() { " · " } else { "" };
        header.push((format!("{prefix}{duration}"), render.fg(theme.warning)));
    }
    out.push(MessageLine::for_message(header, message_id));

    if (!minimal || expanded) && !body.is_empty() {
        let body_lines = markdown::render(
            &body,
            &markdown::MarkdownOptions {
                width: render.content_width().saturating_sub(5).max(20),
                conceal: render.conceal,
                fg: theme.text_muted,
                heading: theme.markdown_heading,
                code: theme.markdown_code,
                muted: theme.text_muted,
            },
        );
        let extra = if minimal { "  " } else { "" };
        for line in body_lines {
            if line.spans.is_empty() {
                continue;
            }
            let mut spans = vec![(format!("   {extra} "), Style::default())];
            for span in line.spans {
                spans.push((span.text, render.fg(theme.text_muted)));
            }
            out.push(MessageLine::for_message(spans, message_id));
        }
    }
}

/// From reference/packages/tui/src/routes/session/index.tsx (`ToolPart` +
/// per-tool components)
fn render_tool_part(render: &SessionRender, part: &ToolPart, out: &mut Vec<MessageLine>) {
    let display = tool_display(&part.tool);
    let (metadata, input_map) = match &part.state {
        ToolState::Pending(s) => (None, Some(&s.input)),
        ToolState::Running(s) => (s.metadata.as_ref(), Some(&s.input)),
        ToolState::Completed(s) => (Some(&s.metadata), Some(&s.input)),
        ToolState::Error(s) => (s.metadata.as_ref(), Some(&s.input)),
    };
    let output = match &part.state {
        ToolState::Completed(s) => Some(s.output.as_str()),
        _ => None,
    };
    let error = match &part.state {
        ToolState::Error(s) => Some(s.error.as_str()),
        _ => None,
    };

    match display {
        "bash" => render_shell(render, part, metadata, input_map, output, error, out),
        "glob" => render_glob(render, part, metadata, input_map, output, error, out),
        "read" => render_read(render, part, metadata, input_map, output, error, out),
        "grep" => render_grep(render, part, metadata, input_map, output, error, out),
        "webfetch" => render_inline(
            render,
            part,
            "%",
            "Fetching from the web...",
            string_value(value(input_map, "url")).unwrap_or_default(),
            error,
            out,
        ),
        "websearch" => render_websearch(render, part, metadata, input_map, error, out),
        "write" => render_write(render, part, metadata, input_map, error, out),
        "edit" => render_edit(render, part, metadata, input_map, error, out),
        "task" => render_task(render, part, metadata, input_map, error, out),
        "execute" => render_execute(render, part, metadata, input_map, output, error, out),
        "apply_patch" => render_apply_patch(render, part, metadata, input_map, error, out),
        "todowrite" => render_todowrite(render, part, metadata, input_map, error, out),
        "question" => render_question(render, part, metadata, input_map, error, out),
        "skill" => render_inline(
            render,
            part,
            "→",
            "Loading skill...",
            string_value(value(input_map, "name")).unwrap_or_default(),
            error,
            out,
        ),
        _ => render_generic(render, part, input_map, output, error, out),
    }
}

fn value<'a>(
    map: Option<&'a serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<&'a serde_json::Value> {
    map.and_then(|m| m.get(key))
}

/// An inline tool row. Mirrors `InlineTool`/`InlineToolRow` from
/// reference/packages/tui/src/routes/session/index.tsx.
fn inline_row(
    render: &SessionRender,
    icon: &str,
    icon_color: Option<Color>,
    color: Option<Color>,
    pending: &str,
    complete: bool,
    failed: bool,
    content: &[StyledLine],
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let fg = color.unwrap_or(if failed {
        theme.error
    } else if complete {
        theme.text_muted
    } else {
        theme.text
    });
    let mut line: StyledLine = vec![("   ".to_string(), Style::default())];
    if !complete && !failed {
        line.push(("~ ".to_string(), render.fg(fg)));
        line.push((pending.to_string(), render.fg(fg)));
    } else {
        line.push((format!("{icon} "), render.fg(icon_color.unwrap_or(fg))));
        for content_line in content {
            if content_line.is_empty() {
                continue;
            }
            line.extend(content_line.iter().cloned());
            break;
        }
    }
    out.push(MessageLine::new(line));
    // Additional content lines (e.g. subagent details).
    if complete || failed {
        let first = content.first().cloned().unwrap_or_default();
        let _ = first;
        for (idx, content_line) in content.iter().enumerate() {
            if idx == 0 || content_line.is_empty() {
                continue;
            }
            let mut sub: StyledLine = vec![("   ".to_string(), Style::default())];
            sub.push(("  ".to_string(), Style::default()));
            sub.extend(content_line.iter().cloned());
            out.push(MessageLine::new(sub));
        }
    }
    if failed {
        out.push(MessageLine::new(styled(
            format!("     {error}", error = "…"),
            render.fg(theme.error),
        )));
    }
}

fn render_inline(
    render: &SessionRender,
    part: &ToolPart,
    icon: &str,
    pending: &str,
    complete_text: String,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let status = part.state_status();
    let complete = matches!(status, "completed");
    let failed = matches!(status, "error");
    let denied = error.is_some_and(|e| {
        e.contains("QuestionRejectedError")
            || e.contains("rejected permission")
            || e.contains("specified a rule")
            || e.contains("user dismissed")
    });
    let mut content: Vec<StyledLine> = Vec::new();
    content.push(plain_line(
        &complete_text,
        if denied { theme.text_muted } else { theme.text },
    ));
    inline_row(
        render,
        icon,
        None,
        None,
        pending,
        complete,
        failed && !denied,
        &content,
        out,
    );
    if failed && !denied {
        if let Some(error) = error {
            let error_lines = wrap_plain(error, render.content_width().saturating_sub(6));
            for line in error_lines {
                let mut spans = vec![("      ".to_string(), Style::default())];
                spans.extend(line);
                let mut msg = MessageLine::new(spans);
                for span in &mut msg.line {
                    span.1 = render.fg(theme.error);
                }
                out.push(msg);
            }
        }
    }
}

fn render_glob(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    _output: Option<&str>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let pattern = string_value(value(input, "pattern")).unwrap_or_default();
    let path = string_value(value(input, "path"));
    let count = number_value(value(metadata, "count"));
    let mut text = format!("Glob \"{pattern}\"");
    if let Some(path) = path {
        text.push_str(&format!(" in {}", format_path(&path, render.cwd, "")));
    }
    if let Some(count) = count {
        text.push_str(&format!(
            " ({count} {})",
            if count == 1 { "match" } else { "matches" }
        ));
    }
    render_inline(render, part, "✱", "Finding files...", text, error, out);
    let _ = theme;
}

fn render_grep(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    _output: Option<&str>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let pattern = string_value(value(input, "pattern")).unwrap_or_default();
    let path = string_value(value(input, "path"));
    let matches = number_value(value(metadata, "matches"));
    let mut text = format!("Grep \"{pattern}\"");
    if let Some(path) = path {
        text.push_str(&format!(" in {}", format_path(&path, render.cwd, "")));
    }
    if let Some(matches) = matches {
        text.push_str(&format!(
            " ({matches} {})",
            if matches == 1 { "match" } else { "matches" }
        ));
    }
    render_inline(render, part, "✱", "Searching content...", text, error, out);
}

fn render_websearch(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let query = string_value(value(input, "query")).unwrap_or_default();
    let provider = value(metadata, "provider")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let num_results = number_value(value(metadata, "numResults"));
    let mut text = format!("{} \"{query}\"", web_search_provider_label(&provider));
    if let Some(num_results) = num_results {
        text.push_str(&format!(" ({num_results} results)"));
    }
    render_inline(render, part, "◈", "Searching web...", text, error, out);
}

/// From reference/packages/tui/src/routes/session/index.tsx (`Shell`)
fn render_shell(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    output: Option<&str>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let command = string_value(value(input, "command")).unwrap_or_default();
    let running = part.state_status() == "running";
    let workdir = string_value(value(input, "workdir"));
    let has_output = output.is_some_and(|o| !o.trim().is_empty());

    if has_output {
        // Block tool with optional collapsed output.
        let mut title = String::new();
        if let Some(workdir) = workdir {
            if workdir != "." {
                let formatted = format_path(&workdir, render.cwd, "");
                if !formatted.is_empty() && formatted != "." {
                    title = format!("# Running in {formatted}");
                }
            }
        }
        let mut spans: StyledLine = vec![
            ("┃ ".to_string(), render.fg(theme.border_subtle)),
            ("  ".to_string(), Style::default()),
        ];
        spans.push((
            if running {
                format!("{command}")
            } else {
                format!("$ {command}")
            },
            render.fg(theme.text),
        ));
        out.push(MessageLine::new(spans));
        if !title.is_empty() {
            let mut t = vec![
                ("┃ ".to_string(), render.fg(theme.border_subtle)),
                ("  ".to_string(), Style::default()),
            ];
            t.push((title, render.fg(theme.text_muted)));
            out.push(MessageLine::new(t));
        }
        let output = output.unwrap_or("").trim();
        let collapsed = collapse_tool_output(output, 10, 10 * render.content_width().max(20));
        if !collapsed.output.is_empty() {
            let expanded = render.expanded_tools.contains(&part.id);
            let shown = if expanded || !collapsed.overflow {
                output.to_string()
            } else {
                collapsed.output
            };
            for line in shown.lines() {
                let mut spans = vec![
                    ("┃ ".to_string(), render.fg(theme.border_subtle)),
                    ("  ".to_string(), Style::default()),
                ];
                spans.push((line.to_string(), render.fg(theme.text)));
                out.push(MessageLine::new(spans));
            }
            if collapsed.overflow {
                let mut spans = vec![
                    ("┃ ".to_string(), render.fg(theme.border_subtle)),
                    ("  ".to_string(), Style::default()),
                ];
                spans.push((
                    if expanded {
                        "Click to collapse"
                    } else {
                        "Click to expand"
                    }
                    .to_string(),
                    render.fg(theme.text_muted),
                ));
                out.push(MessageLine::new(spans));
            }
        }
    } else {
        render_inline(
            render,
            part,
            "$",
            "Writing command...",
            command.clone(),
            error,
            out,
        );
    }
}

fn render_read(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    _output: Option<&str>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let file_path = string_value(value(input, "filePath")).unwrap_or_default();
    let mut content: Vec<StyledLine> = Vec::new();
    let extra = format_tool_input(input.unwrap_or(&serde_json::Map::new()), &["filePath"]);
    let mut text = format!("Read {}", format_path(&file_path, render.cwd, ""));
    if !extra.is_empty() {
        text.push(' ');
        text.push_str(&extra);
    }
    content.push(plain_line(&text, theme.text));
    inline_row(
        render,
        "→",
        None,
        None,
        "Reading file...",
        true,
        false,
        &content,
        out,
    );
    // Loaded files.
    let loaded: Vec<String> = if part.state_status() == "completed" {
        value(metadata, "loaded")
            .and_then(serde_json::Value::as_array)
            .map(|arr| arr.iter().filter_map(|v| string_value(Some(v))).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    for filepath in loaded {
        out.push(MessageLine::new(styled(
            format!("      ↳ Loaded {}", format_path(&filepath, render.cwd, "")),
            render.fg(theme.text_muted),
        )));
    }
}

fn render_write(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let file_path = string_value(value(input, "filePath")).unwrap_or_default();
    if metadata.is_some() && value(metadata, "diagnostics").is_some() {
        let mut spans: StyledLine = vec![
            ("┃ ".to_string(), render.fg(theme.border_subtle)),
            ("  ".to_string(), Style::default()),
        ];
        spans.push((
            format!("# Wrote {}", format_path(&file_path, render.cwd, "")),
            render.fg(theme.text_muted),
        ));
        out.push(MessageLine::new(spans));
        render_diagnostics(render, metadata, &file_path, out);
    } else {
        let text = format!("Write {}", format_path(&file_path, render.cwd, ""));
        render_inline(render, part, "←", "Preparing write...", text, error, out);
    }
}

fn render_edit(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let file_path = string_value(value(input, "filePath")).unwrap_or_default();
    let diff = string_value(value(metadata, "diff"));
    if diff.is_some() {
        let mut spans: StyledLine = vec![
            ("┃ ".to_string(), render.fg(theme.border_subtle)),
            ("  ".to_string(), Style::default()),
        ];
        spans.push((
            format!("← Edit {}", format_path(&file_path, render.cwd, "")),
            render.fg(theme.text_muted),
        ));
        out.push(MessageLine::new(spans));
        if let Some(diff) = diff {
            render_diff(render, diff.as_str(), out);
        }
        render_diagnostics(render, metadata, &file_path, out);
    } else {
        let replace_all = value(input, "replaceAll").and_then(serde_json::Value::as_bool);
        let mut text = format!("Edit {}", format_path(&file_path, render.cwd, ""));
        if let Some(replace_all) = replace_all {
            text.push_str(&format!(" [replaceAll={replace_all}]"));
        }
        render_inline(render, part, "←", "Preparing edit...", text, error, out);
    }
}

fn render_diff(render: &SessionRender, diff: &str, out: &mut Vec<MessageLine>) {
    let theme = render.t();
    let width = render.content_width().saturating_sub(4).max(20);
    for line in diff.lines() {
        let style = if line.starts_with('+') {
            theme.diff_added
        } else if line.starts_with('-') {
            theme.diff_removed
        } else if line.starts_with("@@") {
            theme.diff_context
        } else {
            theme.text_muted
        };
        let wrapped = wrap_plain(line, width);
        for wl in wrapped {
            let mut spans = vec![
                ("┃ ".to_string(), render.fg(theme.border_subtle)),
                ("    ".to_string(), Style::default()),
            ];
            spans.extend(wl);
            for span in &mut spans {
                if !span.0.starts_with("┃") {
                    span.1 = span.1.fg(style);
                }
            }
            out.push(MessageLine::new(spans));
        }
    }
}

fn render_diagnostics(
    render: &SessionRender,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    file_path: &str,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let diagnostics = metadata
        .and_then(|m| m.get("diagnostics"))
        .map(|v| parse_diagnostics(v, &normalize_path(file_path, "posix")))
        .unwrap_or_default();
    for diagnostic in diagnostics {
        out.push(MessageLine::new(styled(
            format!(
                "      Error [{}:{}] {}",
                diagnostic.line + 1,
                diagnostic.character + 1,
                diagnostic.message
            ),
            render.fg(theme.error),
        )));
    }
}

fn render_task(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let description = string_value(value(input, "description")).unwrap_or_default();
    let session_id = string_value(value(metadata, "sessionId"));
    let subagent_type =
        string_value(value(input, "subagent_type")).unwrap_or_else(|| "General".to_string());
    let background = value(metadata, "background")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let running = part.state_status() == "running";
    let completed = part.state_status() == "completed";
    let status = session_id
        .as_deref()
        .and_then(|id| render.sync.session_status.get(id));
    let retrying =
        running && status.is_some_and(|s| matches!(s, crate::types::SessionStatus::Retry(_)));

    let mut content: Vec<StyledLine> = Vec::new();
    content.push(plain_line(
        &format_subagent_title(&locale::titlecase(&subagent_type), &description, background),
        theme.text,
    ));
    if running {
        if let Some(crate::types::SessionStatus::Retry(retry)) = status {
            content.push(plain_line(
                &format!(
                    "↳ {}",
                    format_subagent_retry(retry.attempt, &locale::truncate(&retry.message, 80))
                ),
                theme.text_muted,
            ));
        } else if let Some(session_id) = &session_id {
            let messages = render.sync.messages_for(session_id);
            let tools: Vec<(String, String)> = messages
                .iter()
                .flat_map(|m| render.sync.parts_for(m.id()))
                .filter_map(|p| match p {
                    Part::Tool(t) => Some((t.tool.clone(), t.state_status().to_string())),
                    _ => None,
                })
                .collect();
            if !tools.is_empty() {
                let current = tools
                    .iter()
                    .rev()
                    .find(|(_, s)| s == "running" || s == "completed");
                match current {
                    Some((tool, _)) => content.push(plain_line(
                        &format!(
                            "↳ {} {}",
                            locale::titlecase(tool),
                            current_title(render, tool)
                        ),
                        theme.text_muted,
                    )),
                    None => content.push(plain_line(
                        &format!("↳ {}", format_subagent_toolcalls(tools.len())),
                        theme.text_muted,
                    )),
                }
            }
        }
    }
    if !running && completed {
        let duration = subagent_duration(render, session_id.as_deref());
        let detail = format_completed_subagent_detail(
            count_toolcalls(render, session_id.as_deref()),
            &duration,
        );
        content.push(plain_line(&format!("↳ {detail}"), theme.text_muted));
    }

    let icon = if completed { "✓" } else { "│" };
    inline_row(
        render,
        icon,
        None,
        retrying.then_some(theme.error),
        "Delegating...",
        !description.is_empty(),
        false,
        &content,
        out,
    );
    let _ = error;
}

fn current_title(render: &SessionRender, _tool: &str) -> String {
    // The reference shows the running tool's `state.title` when available.
    String::new()
}

fn count_toolcalls(render: &SessionRender, session_id: Option<&str>) -> usize {
    let Some(session_id) = session_id else {
        return 0;
    };
    render
        .sync
        .messages_for(session_id)
        .iter()
        .flat_map(|m| render.sync.parts_for(m.id()))
        .filter(|p| matches!(p, Part::Tool(_)))
        .count()
}

fn subagent_duration(render: &SessionRender, session_id: Option<&str>) -> String {
    let Some(session_id) = session_id else {
        return String::new();
    };
    let messages = render.sync.messages_for(session_id);
    let first = messages
        .iter()
        .find(|m| m.role() == "user")
        .and_then(|m| match m {
            Message::User(u) => Some(u.time.created),
            _ => None,
        });
    let completed = messages.iter().rev().find_map(|m| match m {
        Message::Assistant(a) => a.time.completed,
        _ => None,
    });
    match (first, completed) {
        (Some(first), Some(completed)) => locale::duration((completed - first).max(0)),
        _ => String::new(),
    }
}

fn render_execute(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    output: Option<&str>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let calls =
        parse_execute_calls(value(metadata, "toolCalls").unwrap_or(&serde_json::Value::Null));
    let runtime_error = value(metadata, "error")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let loading = matches!(part.state_status(), "pending" | "running");
    let mut content: Vec<StyledLine> = vec![plain_line("execute", theme.text)];
    for call in &calls {
        let args = call
            .input
            .as_ref()
            .map(|m| format_tool_input(m, &[]))
            .unwrap_or_default();
        let mut text = format!("↳ {}", call.tool);
        if !args.is_empty() {
            text.push(' ');
            text.push_str(&args);
        }
        if call.status == "error" {
            text.push_str(" (failed)");
        }
        content.push(plain_line(&text, theme.text_muted));
    }
    let icon = if runtime_error {
        "✗"
    } else if part.state_status() == "completed" {
        "✓"
    } else {
        "│"
    };
    inline_row(
        render,
        icon,
        None,
        runtime_error.then_some(theme.error),
        "execute",
        !loading,
        false,
        &content,
        out,
    );
    let _ = error;
    let _ = input;
    let _ = output;
}

fn render_apply_patch(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let files = metadata
        .and_then(|m| m.get("files"))
        .map(parse_apply_patch_files)
        .unwrap_or_default();
    if files.is_empty() {
        render_inline(
            render,
            part,
            "%",
            "Preparing patch...",
            "Patch".to_string(),
            error,
            out,
        );
        return;
    }
    for file in files {
        let title = if file.type_ == "delete" {
            format!("# Deleted {}", file.relative_path)
        } else if file.type_ == "add" {
            format!("# Created {}", file.relative_path)
        } else if file.type_ == "move" {
            format!(
                "# Moved {} → {}",
                format_path(&file.file_path, render.cwd, ""),
                file.relative_path
            )
        } else {
            format!("← Patched {}", file.relative_path)
        };
        let mut spans: StyledLine = vec![
            ("┃ ".to_string(), render.fg(theme.border_subtle)),
            ("  ".to_string(), Style::default()),
        ];
        spans.push((title, render.fg(theme.text_muted)));
        out.push(MessageLine::new(spans));
        if file.type_ != "delete" {
            render_diff(render, &file.patch, out);
        } else {
            out.push(MessageLine::new(styled(
                format!(
                    "      -{} line{}",
                    file.deletions,
                    if file.deletions == 1 { "" } else { "s" }
                ),
                render.fg(theme.diff_removed),
            )));
        }
    }
    let _ = input;
}

fn render_todowrite(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let has_metadata = metadata
        .and_then(|m| m.get("todos"))
        .map(parse_todos)
        .map(|t| !t.is_empty())
        .unwrap_or(false);
    if has_metadata {
        let todos = input
            .and_then(|i| i.get("todos"))
            .map(parse_todos)
            .unwrap_or_default();
        let mut spans: StyledLine = vec![
            ("┃ ".to_string(), render.fg(theme.border_subtle)),
            ("  ".to_string(), Style::default()),
        ];
        spans.push(("# Todos".to_string(), render.fg(theme.text_muted)));
        out.push(MessageLine::new(spans));
        for todo in todos {
            let mark = match todo.status.as_str() {
                "completed" => "✓",
                "in_progress" => "•",
                _ => " ",
            };
            let fg = if todo.status == "in_progress" {
                theme.warning
            } else {
                theme.text_muted
            };
            out.push(MessageLine::new(styled(
                format!("      [{mark}] {}", todo.content),
                render.fg(fg),
            )));
        }
    } else {
        render_inline(
            render,
            part,
            "⚙",
            "Updating todos...",
            "Updating todos...".to_string(),
            error,
            out,
        );
    }
}

fn render_question(
    render: &SessionRender,
    part: &ToolPart,
    metadata: Option<&serde_json::Map<String, serde_json::Value>>,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let questions = input
        .and_then(|i| i.get("questions"))
        .map(parse_questions)
        .unwrap_or_default();
    let answers = metadata
        .and_then(|m| m.get("answers"))
        .and_then(parse_question_answers);
    let count = questions.len();
    if let Some(answers) = answers {
        let mut spans: StyledLine = vec![
            ("┃ ".to_string(), render.fg(theme.border_subtle)),
            ("  ".to_string(), Style::default()),
        ];
        spans.push(("# Questions".to_string(), render.fg(theme.text_muted)));
        out.push(MessageLine::new(spans));
        for (idx, question) in questions.iter().enumerate() {
            let answer = answers.get(idx).cloned().unwrap_or_default();
            let answer = if answer.is_empty() {
                "(no answer)".to_string()
            } else {
                answer.join(", ")
            };
            out.push(MessageLine::new(styled(
                format!("      {}", question.question),
                render.fg(theme.text_muted),
            )));
            out.push(MessageLine::new(styled(
                format!("      {answer}"),
                render.fg(theme.text),
            )));
        }
    } else {
        render_inline(
            render,
            part,
            "→",
            "Asking questions...",
            format!(
                "Asked {count} question{}",
                if count == 1 { "" } else { "s" }
            ),
            error,
            out,
        );
    }
}

fn render_generic(
    render: &SessionRender,
    part: &ToolPart,
    input: Option<&serde_json::Map<String, serde_json::Value>>,
    output: Option<&str>,
    error: Option<&str>,
    out: &mut Vec<MessageLine>,
) {
    let theme = render.t();
    let tool = part.tool.clone();
    let args = input.map(|i| format_tool_input(i, &[])).unwrap_or_default();
    if render.show_generic_tool_output && output.is_some_and(|o| !o.trim().is_empty()) {
        let mut title = format!("# {tool}");
        if !args.is_empty() {
            title.push(' ');
            title.push_str(&args);
        }
        let mut spans: StyledLine = vec![
            ("┃ ".to_string(), render.fg(theme.border_subtle)),
            ("  ".to_string(), Style::default()),
        ];
        spans.push((title, render.fg(theme.text_muted)));
        out.push(MessageLine::new(spans));
        let output = output.unwrap_or("").trim();
        let collapsed = collapse_tool_output(output, 3, 3 * render.content_width().max(20));
        let expanded = render.expanded_tools.contains(&part.id);
        let shown = if expanded || !collapsed.overflow {
            output.to_string()
        } else {
            collapsed.output
        };
        for line in shown.lines() {
            let mut spans = vec![
                ("┃ ".to_string(), render.fg(theme.border_subtle)),
                ("  ".to_string(), Style::default()),
            ];
            spans.push((line.to_string(), render.fg(theme.text)));
            out.push(MessageLine::new(spans));
        }
        if collapsed.overflow {
            out.push(MessageLine::new(styled(
                format!(
                    "      {}",
                    if expanded {
                        "Click to collapse"
                    } else {
                        "Click to expand"
                    }
                ),
                render.fg(theme.text_muted),
            )));
        }
    } else {
        let mut text = format!("{tool}");
        if !args.is_empty() {
            text.push(' ');
            text.push_str(&args);
        }
        render_inline(render, part, "⚙", "Writing command...", text, error, out);
    }
}

fn plain_line(text: &str, fg: Color) -> StyledLine {
    vec![(text.to_string(), Style::default().fg(fg))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use serde_json::json;
    use std::collections::HashSet;

    fn build_sync() -> SyncState {
        let mut sync = SyncState::default();
        let agent = serde_json::from_value(json!({
            "name": "build", "mode": "primary", "permission": [], "options": {}
        }))
        .unwrap();
        sync.agents = vec![agent];
        let provider = serde_json::from_value(json!({
            "id": "openrouter", "name": "OpenRouter", "source": "config", "env": [],
            "options": {}, "models": {
                "model-x": { "id": "model-x", "providerID": "openrouter", "name": "Model X",
                    "capabilities": {"input": {}, "output": {}}, "cost": {"input": 0, "output": 0, "cache": {"read": 0, "write": 0}},
                    "limit": {"context": 1000, "output": 1000}, "status": "active", "options": {}, "headers": {}, "release_date": "" }
            }
        }))
        .unwrap();
        sync.providers = vec![provider];
        sync
    }

    fn user_message_json(id: &str) -> Message {
        serde_json::from_value(json!({
            "id": id, "sessionID": "ses_1", "role": "user", "agent": "build",
            "model": { "id": "model-x", "providerID": "openrouter" }, "time": { "created": 1700000000000u64 }
        }))
        .unwrap()
    }

    fn text_part_json(id: &str, message_id: &str, text: &str) -> Part {
        serde_json::from_value(json!({
            "id": id, "sessionID": "ses_1", "messageID": message_id, "type": "text", "text": text
        }))
        .unwrap()
    }

    fn tool_part_json(id: &str, message_id: &str, tool: &str, state: serde_json::Value) -> Part {
        serde_json::from_value(json!({
            "id": id, "sessionID": "ses_1", "messageID": message_id, "type": "tool", "callID": format!("call_{id}"),
            "tool": tool, "state": state
        }))
        .unwrap()
    }

    fn render(sync: &SyncState, width: usize) -> Vec<String> {
        let render = SessionRender {
            width,
            sync,
            theme: &Theme::dark(),
            conceal: true,
            thinking_mode: "hide",
            show_timestamps: false,
            show_details: true,
            show_generic_tool_output: false,
            expanded_tools: &HashSet::new(),
            reasoning_expanded: &HashSet::new(),
            session_id: "ses_1",
            cwd: "/proj",
        };
        render_messages(&render)
            .into_iter()
            .map(|m| m.line.iter().map(|(s, _)| s.as_str()).collect::<String>())
            .collect()
    }

    #[test]
    fn user_message_text_renders() {
        let mut sync = build_sync();
        sync.upsert_message("ses_1", user_message_json("msg_a"));
        sync.upsert_part("msg_a", text_part_json("pt_1", "msg_a", "hello"));
        let lines = render(&sync, 80);
        assert!(lines.iter().any(|l| l.contains("hello")));
        assert!(lines[0].starts_with('┃'));
    }

    #[test]
    fn assistant_markdown_renders() {
        let mut sync = build_sync();
        let assistant: Message = serde_json::from_value(json!({
            "id": "msg_b", "sessionID": "ses_1", "role": "assistant", "time": { "created": 1, "completed": 2000 },
            "parentID": "msg_a", "modelID": "model-x", "providerID": "openrouter", "mode": "primary",
            "agent": "build", "path": { "cwd": "/proj", "root": "/proj" }, "cost": 0,
            "tokens": { "input": 1, "output": 1, "reasoning": 0, "cache": { "read": 0, "write": 0 } },
            "finish": "end_turn"
        }))
        .unwrap();
        sync.upsert_message("ses_1", assistant);
        sync.upsert_part(
            "msg_b",
            text_part_json("pt_2", "msg_b", "## Heading\n\nSome **bold** text"),
        );
        let lines = render(&sync, 80);
        assert!(lines.iter().any(|l| l.contains("Heading")));
        assert!(lines.iter().any(|l| l.contains("Some")));
        // Footer includes mode and model.
        assert!(lines
            .iter()
            .any(|l| l.contains("▣") && l.contains("OpenRouter")));
    }

    #[test]
    fn tool_inline_completed_hidden_when_details_off() {
        let mut sync = build_sync();
        let assistant: Message = serde_json::from_value(json!({
            "id": "msg_c", "sessionID": "ses_1", "role": "assistant", "time": { "created": 1 },
            "parentID": "msg_a", "modelID": "model-x", "providerID": "openrouter", "mode": "primary",
            "agent": "build", "path": { "cwd": "/proj", "root": "/proj" }, "cost": 0,
            "tokens": { "input": 1, "output": 1, "reasoning": 0, "cache": { "read": 0, "write": 0 } }
        }))
        .unwrap();
        sync.upsert_message("ses_1", assistant);
        sync.upsert_part(
            "msg_c",
            tool_part_json(
                "pt_t",
                "msg_c",
                "glob",
                json!({
                    "status": "completed",
                    "input": { "pattern": "**/*.ts" },
                    "output": "[]",
                    "title": "glob",
                    "metadata": { "count": 3 },
                    "time": { "start": 1, "end": 2 }
                }),
            ),
        );
        // Details shown: glob inline row present.
        let shown = render(&sync, 80);
        assert!(shown
            .iter()
            .any(|l| l.contains("Glob") && l.contains("3 matches")));
        // Details hidden: tool row disappears.
        let render = SessionRender {
            width: 80,
            sync: &sync,
            theme: &Theme::dark(),
            conceal: true,
            thinking_mode: "hide",
            show_timestamps: false,
            show_details: false,
            show_generic_tool_output: false,
            expanded_tools: &HashSet::new(),
            reasoning_expanded: &HashSet::new(),
            session_id: "ses_1",
            cwd: "/proj",
        };
        let hidden = render_messages(&render)
            .into_iter()
            .map(|m| m.line.iter().map(|(s, _)| s.as_str()).collect::<String>())
            .collect::<Vec<_>>();
        assert!(!hidden.iter().any(|l| l.contains("Glob")));
    }

    #[test]
    fn shell_block_shows_command_and_output() {
        let mut sync = build_sync();
        let assistant: Message = serde_json::from_value(json!({
            "id": "msg_d", "sessionID": "ses_1", "role": "assistant", "time": { "created": 1 },
            "parentID": "msg_a", "modelID": "model-x", "providerID": "openrouter", "mode": "primary",
            "agent": "build", "path": { "cwd": "/proj", "root": "/proj" }, "cost": 0,
            "tokens": { "input": 1, "output": 1, "reasoning": 0, "cache": { "read": 0, "write": 0 } }
        }))
        .unwrap();
        sync.upsert_message("ses_1", assistant);
        sync.upsert_part(
            "msg_d",
            tool_part_json(
                "pt_s",
                "msg_d",
                "bash",
                json!({
                    "status": "completed",
                    "input": { "command": "ls -la", "workdir": "/proj/src" },
                    "output": "total 8\ndrwxr-xr-x  file1\n-rw-r--r--  file2",
                    "title": "bash",
                    "metadata": {},
                    "time": { "start": 1, "end": 2 }
                }),
            ),
        );
        let lines = render(&sync, 80);
        assert!(lines.iter().any(|l| l.contains("$ ls -la")));
        assert!(lines.iter().any(|l| l.contains("Running in")));
        assert!(lines.iter().any(|l| l.contains("file2")));
    }

    #[test]
    fn reasoning_collapsed_in_hide_mode() {
        let mut sync = build_sync();
        let assistant: Message = serde_json::from_value(json!({
            "id": "msg_e", "sessionID": "ses_1", "role": "assistant", "time": { "created": 1 },
            "parentID": "msg_a", "modelID": "model-x", "providerID": "openrouter", "mode": "primary",
            "agent": "build", "path": { "cwd": "/proj", "root": "/proj" }, "cost": 0,
            "tokens": { "input": 1, "output": 1, "reasoning": 0, "cache": { "read": 0, "write": 0 } }
        }))
        .unwrap();
        sync.upsert_message("ses_1", assistant);
        sync.upsert_part("msg_e", serde_json::from_value(json!({
            "id": "pt_r", "sessionID": "ses_1", "messageID": "msg_e", "type": "reasoning",
            "text": "**Figuring it out**\n\nlong reasoning body", "time": { "start": 1, "end": 2 }
        })).unwrap());
        let lines = render(&sync, 80);
        // Header visible with + toggle; body hidden in hide mode.
        assert!(lines
            .iter()
            .any(|l| l.contains("Thought") && l.contains("Figuring it out")));
        assert!(lines.iter().any(|l| l.contains("+ ")));
        assert!(!lines.iter().any(|l| l.contains("long reasoning body")));
    }

    #[test]
    fn todos_render() {
        let mut sync = build_sync();
        let assistant: Message = serde_json::from_value(json!({
            "id": "msg_f", "sessionID": "ses_1", "role": "assistant", "time": { "created": 1 },
            "parentID": "msg_a", "modelID": "model-x", "providerID": "openrouter", "mode": "primary",
            "agent": "build", "path": { "cwd": "/proj", "root": "/proj" }, "cost": 0,
            "tokens": { "input": 1, "output": 1, "reasoning": 0, "cache": { "read": 0, "write": 0 } }
        }))
        .unwrap();
        sync.upsert_message("ses_1", assistant);
        sync.upsert_part("msg_f", tool_part_json("pt_todo", "msg_f", "todowrite", json!({
            "status": "completed",
            "input": { "todos": [ { "status": "in_progress", "content": "write tests" } ] },
            "output": "[]", "title": "todos",
            "metadata": { "todos": [ { "status": "in_progress", "content": "write tests" } ] },
            "time": { "start": 1, "end": 2 }
        })));
        let lines = render(&sync, 80);
        assert!(lines.iter().any(|l| l.contains("# Todos")));
        assert!(lines.iter().any(|l| l.contains("[•] write tests")));
    }

    #[test]
    fn queued_user_message_shows_badge() {
        let mut sync = build_sync();
        sync.upsert_message("ses_1", user_message_json("msg_q1"));
        sync.upsert_part("msg_q1", text_part_json("p1", "msg_q1", "first"));
        // Pending assistant after the first user message.
        let assistant: Message = serde_json::from_value(json!({
            "id": "msg_a1", "sessionID": "ses_1", "role": "assistant", "time": { "created": 2 },
            "parentID": "msg_q1", "modelID": "model-x", "providerID": "openrouter", "mode": "primary",
            "agent": "build", "path": { "cwd": "/proj", "root": "/proj" }, "cost": 0,
            "tokens": { "input": 1, "output": 1, "reasoning": 0, "cache": { "read": 0, "write": 0 } }
        }))
        .unwrap();
        sync.upsert_message("ses_1", assistant);
        // A queued user message (id greater than pending).
        sync.upsert_message("ses_1", user_message_json("msg_q2"));
        sync.upsert_part("msg_q2", text_part_json("p2", "msg_q2", "second"));
        let lines = render(&sync, 80);
        assert!(lines.iter().any(|l| l.contains("QUEUED")));
    }
}
