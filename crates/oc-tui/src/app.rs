//! The terminal application: event loop, keymap wiring and command dispatch.
//!
//! Port of `reference/packages/tui/src/app.tsx`, `keymap.tsx` and the route
//! components. The main loop alternates between draining terminal input and
//! server events, then redraws through the route renderers.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;

use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style};
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::client::SdkClient;
use crate::components::dialog::{self, DialogItem, DialogKind, DialogState};
use crate::components::permission::{self, PermissionState};
use crate::components::question::{self, QuestionState};
use crate::components::text::{to_ratatui, StyledLine};
use crate::components::toast::{Toast, ToastStore, ToastVariant};
use crate::config::ResolvedConfig;
use crate::keymap::{Binding, BindingGroup, Keymap, KeymapOptions, MatchResult};
use crate::local::Local;
use crate::prompt::history::PromptHistory;
use crate::prompt::parts::{expand_text_parts, strip_prompt_part_ids};
use crate::prompt::stash::PromptStash;
use crate::prompt::state::{PromptMode, PromptState};
use crate::sync::SyncState;
use crate::theme::{Mode, Theme};
use crate::types::{GlobalEvent, SessionStatus};
use crate::util::locale;

pub const OPENCODE_BASE_MODE: &str = "base";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Home,
    Session { id: String },
}

/// Inputs accepted by the TUI (mirrors `TuiInput` in app.tsx).
pub struct TuiInput {
    pub url: String,
    pub directory: Option<String>,
    pub workspace: Option<String>,
    pub cwd: PathBuf,
    pub home: PathBuf,
    pub state_dir: PathBuf,
    pub config: ResolvedConfig,
    pub continue_session: bool,
    pub session_id: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub prompt: Option<String>,
}

pub(crate) enum ClientMessage {
    Event(GlobalEvent),
    Bootstrap(BootstrapData),
}

#[derive(Default)]
pub(crate) struct BootstrapData {
    providers: Vec<crate::types::Provider>,
    agents: Vec<crate::types::Agent>,
    commands: Vec<crate::types::Command>,
    config: crate::types::Config,
    sessions: Vec<crate::types::Session>,
    capabilities: crate::types::ExperimentalCapabilities,
    console_state: crate::types::ConsoleState,
    session_status: HashMap<String, SessionStatus>,
}

/// Per-session view state.
#[derive(Debug, Clone, Default)]
pub struct SessionView {
    pub scroll: i64,
    pub sticky_bottom: bool,
    pub expanded_tools: HashSet<String>,
    pub reasoning_expanded: HashSet<String>,
    pub sidebar_visible: bool,
    pub question: Option<QuestionState>,
    pub permission: Option<PermissionState>,
    pub message_height: usize,
    pub sidebar_init: bool,
}

pub struct App {
    pub client: Arc<dyn SdkClient>,
    pub config: ResolvedConfig,
    pub sync: SyncState,
    pub route: Route,
    pub theme: Theme,
    pub local: Local,
    pub home_prompt: PromptState,
    pub session_prompt: Option<PromptState>,
    pub views: HashMap<String, SessionView>,
    pub dialog: Option<DialogState>,
    pub toasts: ToastStore,
    pub history: PromptHistory,
    pub stash: PromptStash,
    pub keymap: Keymap,
    pub cwd: PathBuf,
    pub home: PathBuf,
    pub state_dir: PathBuf,
    pub tick: u64,
    pub leader_active: bool,
    pub exiting: bool,
    pub status: StatusMsg,
    pub paste_summary_enabled: bool,
    pub file_context_enabled: bool,
    pub diff_wrap_mode: String,
    pub animations_enabled: bool,
    pub session_directory_filter_enabled: bool,
    pub messages: Vec<crate::components::message::MessageLine>,
    pub needs_autofocus: bool,
    pub terminal_size: (u16, u16),
    pub prompt_ready: bool,
    pub autosubmitted: bool,
    pub kv: HashMap<String, serde_json::Value>,
    pub(crate) pending_session_data: Option<(String, mpsc::Receiver<SessionData>)>,
    pub(crate) pending_create: Option<(
        mpsc::Receiver<Option<String>>,
        String,
        String,
        crate::local::ModelSelection,
        PromptMode,
        Vec<serde_json::Value>,
    )>,
    pub(crate) on_submit: Option<Box<dyn FnOnce()>>,
    pub initial_session_id: Option<String>,
    pub initial_agent: Option<String>,
    pub initial_model: Option<String>,
    pub initial_prompt: Option<String>,
    pub continue_requested: bool,
    pub dirty: bool,
}

pub struct StatusMsg {
    pub kind: StatusKind,
    pub text: String,
}

pub enum StatusKind {
    Working,
    Retry,
}

impl Default for StatusMsg {
    fn default() -> Self {
        StatusMsg {
            kind: StatusKind::Working,
            text: String::new(),
        }
    }
}

impl App {
    pub fn new(input: TuiInput, client: Arc<dyn SdkClient>) -> Self {
        let keymap = Keymap::new(KeymapOptions {
            leader: "ctrl+x".to_string(),
            leader_timeout: Duration::from_millis(input.config.leader_timeout),
        });
        App {
            client,
            config: input.config,
            sync: SyncState::default(),
            route: Route::Home,
            theme: Theme::dark(),
            local: Local::default(),
            home_prompt: PromptState::default(),
            session_prompt: None,
            views: HashMap::new(),
            dialog: None,
            toasts: ToastStore::default(),
            history: PromptHistory::default(),
            stash: PromptStash::default(),
            keymap,
            cwd: input.cwd,
            home: input.home,
            state_dir: input.state_dir,
            tick: 0,
            leader_active: false,
            exiting: false,
            status: StatusMsg::default(),
            paste_summary_enabled: true,
            file_context_enabled: true,
            diff_wrap_mode: "word".to_string(),
            animations_enabled: true,
            session_directory_filter_enabled: true,
            messages: Vec::new(),
            needs_autofocus: true,
            terminal_size: (0, 0),
            prompt_ready: false,
            autosubmitted: false,
            kv: HashMap::new(),
            pending_session_data: None,
            pending_create: None,
            on_submit: None,
            initial_session_id: if input.continue_session {
                None
            } else {
                input.session_id
            },
            initial_agent: input.agent,
            initial_model: input.model,
            initial_prompt: input.prompt,
            continue_requested: input.continue_session,
            dirty: false,
        }
    }

    // ---- active prompt helpers -------------------------------------------------

    pub fn active_prompt(&mut self) -> Option<&mut PromptState> {
        match &self.route {
            Route::Home => Some(&mut self.home_prompt),
            Route::Session { id } => {
                let _ = id;
                self.session_prompt.as_mut()
            }
        }
    }

    pub fn active_prompt_ref(&self) -> Option<&PromptState> {
        match &self.route {
            Route::Home => Some(&self.home_prompt),
            Route::Session { id } => {
                let _ = id;
                self.session_prompt.as_ref()
            }
        }
    }

    fn session_id(&self) -> Option<&str> {
        match &self.route {
            Route::Session { id } => Some(id),
            Route::Home => None,
        }
    }

    fn current_session_id(&self) -> Option<String> {
        self.session_id().map(|s| s.to_string())
    }

    pub fn session_view_mut(&mut self, id: &str) -> &mut SessionView {
        self.views.entry(id.to_string()).or_default()
    }

    // ---- keymap wiring ----------------------------------------------------------

    /// Rebuild the active keybinding groups from current state.
    /// Mirrors the `useBindings` registrations across app.tsx, routes/session,
    /// and component/prompt.
    pub fn rebuild_keymap(&mut self) {
        let mut groups: Vec<BindingGroup> = Vec::new();
        let config = &self.config;
        let dialog_open = self.dialog.is_some();
        let prompt = self.active_prompt_ref();

        // Dialogs (highest priority).
        if dialog_open {
            let mut bindings = Vec::new();
            for (name, cmd) in [
                ("dialog.select.prev", "dialog.select.prev"),
                ("dialog.select.next", "dialog.select.next"),
                ("dialog.select.page_up", "dialog.select.page_up"),
                ("dialog.select.page_down", "dialog.select.page_down"),
                ("dialog.select.home", "dialog.select.home"),
                ("dialog.select.end", "dialog.select.end"),
                ("dialog.select.submit", "dialog.select.submit"),
                ("dialog.prompt.submit", "dialog.prompt.submit"),
            ] {
                if let Some(binding) = config.get(name) {
                    bindings.push(binding.clone());
                }
                let _ = cmd;
            }
            groups.push(BindingGroup {
                priority: 100,
                enabled: true,
                bindings,
            });
        } else {
            // Permission prompt.
            if let Some(Route::Session { id }) = self.session_id().map(|_| &self.route) {
                let _ = id;
                let permissions = self
                    .sync
                    .permissions
                    .get(self.session_id().unwrap_or(""))
                    .map(|v| v.len())
                    .unwrap_or(0);
                if permissions > 0 {
                    let mut bindings: Vec<Binding> = Vec::new();
                    for (key, desc) in [
                        ("left", "Previous option"),
                        ("h", "Previous option"),
                        ("right", "Next option"),
                        ("l", "Next option"),
                        ("return", "Select option"),
                        ("escape", "Reject permission"),
                        ("ctrl+f", "Toggle fullscreen"),
                    ] {
                        if let Some(b) = crate::keymap::Binding::from_string(
                            format!("permission.{}", desc),
                            desc,
                            key,
                        ) {
                            bindings.push(b);
                        }
                    }
                    groups.push(BindingGroup {
                        priority: 90,
                        enabled: true,
                        bindings,
                    });
                }
            }
        }

        // Autocomplete (when visible and input focused).
        if let Some(prompt) = prompt {
            if prompt.autocomplete.visible {
                let mut bindings = Vec::new();
                for (name, _cmd) in [
                    ("prompt.autocomplete.prev", "prompt.autocomplete.prev"),
                    ("prompt.autocomplete.next", "prompt.autocomplete.next"),
                    ("prompt.autocomplete.hide", "prompt.autocomplete.hide"),
                    ("prompt.autocomplete.select", "prompt.autocomplete.select"),
                    (
                        "prompt.autocomplete.complete",
                        "prompt.autocomplete.complete",
                    ),
                ] {
                    if let Some(binding) = config.get(name) {
                        bindings.push(binding.clone());
                    }
                }
                groups.push(BindingGroup {
                    priority: 50,
                    enabled: true,
                    bindings,
                });
            }
        }

        // Input bindings (textarea focused).
        let input_commands = [
            "input.move.left",
            "input.move.right",
            "input.move.up",
            "input.move.down",
            "input.select.left",
            "input.select.right",
            "input.select.up",
            "input.select.down",
            "input.line.home",
            "input.line.end",
            "input.select.line.home",
            "input.select.line.end",
            "input.visual.line.home",
            "input.visual.line.end",
            "input.buffer.home",
            "input.buffer.end",
            "input.select.buffer.home",
            "input.select.buffer.end",
            "input.delete.line",
            "input.delete.to.line.end",
            "input.delete.to.line.start",
            "input.backspace",
            "input.delete",
            "input.newline",
            "input.undo",
            "input.redo",
            "input.word.forward",
            "input.word.backward",
            "input.select.word.forward",
            "input.select.word.backward",
            "input.delete.word.forward",
            "input.delete.word.backward",
            "input.select.all",
            "input.submit",
        ];
        let mut input_bindings = Vec::new();
        for name in input_commands {
            if let Some(binding) = config.get(name) {
                input_bindings.push(binding.clone());
            }
        }
        // Prompt-clear (`ctrl+c` when non-empty) overrides app-exit.
        if let Some(binding) = config.get("input_clear") {
            input_bindings.push(binding.clone());
        }
        if let Some(binding) = config.get("input_paste") {
            input_bindings.push(binding.clone());
        }
        groups.push(BindingGroup {
            priority: 40,
            enabled: !dialog_open && prompt.is_some() && !self.route_is_subagent(),
            bindings: input_bindings,
        });

        // Prompt-level bindings (history, interrupt, shell mode, stash).
        let mut prompt_bindings = Vec::new();
        for (name, cmd) in [
            ("history_previous", "prompt.history.previous"),
            ("history_next", "prompt.history.next"),
            ("session_interrupt", "session.interrupt"),
            ("prompt_submit", "prompt.submit"),
            ("editor_open", "prompt.editor"),
            ("prompt_stash", "prompt.stash"),
            ("prompt_stash_pop", "prompt.stash.pop"),
            ("prompt_stash_list", "prompt.stash.list"),
            ("prompt_skills", "prompt.skills"),
        ] {
            if let Some(binding) = config.get(name) {
                let mut binding = binding.clone();
                binding.command = cmd.to_string();
                prompt_bindings.push(binding);
            }
        }
        // Shell-mode toggle: `!` at cursor 0 and escape/backspace to exit.
        if let Some(binding) = Binding::from_string("prompt.shell.enter", "Shell mode", "!") {
            prompt_bindings.push(binding);
        }
        if let Some(binding) =
            Binding::from_string("prompt.shell.exit", "Exit shell mode", "escape")
        {
            prompt_bindings.push(binding);
        }
        groups.push(BindingGroup {
            priority: 30,
            enabled: !dialog_open && prompt.is_some(),
            bindings: prompt_bindings,
        });

        // Session bindings (base mode).
        if let Route::Session { .. } = &self.route {
            let session_names = [
                ("session_new", "session.new"),
                ("session_list", "session.list"),
                ("session_rename", "session.rename"),
                ("session_delete", "session.delete"),
                ("session_share", "session.share"),
                ("session_unshare", "session.unshare"),
                ("session_compact", "session.compact"),
                ("messages_undo", "session.undo"),
                ("messages_redo", "session.redo"),
                ("sidebar_toggle", "session.sidebar.toggle"),
                ("messages_toggle_conceal", "session.toggle.conceal"),
                ("session_toggle_timestamps", "session.toggle.timestamps"),
                ("display_thinking", "session.toggle.thinking"),
                ("tool_details", "session.toggle.actions"),
                ("scrollbar_toggle", "session.toggle.scrollbar"),
                (
                    "session_toggle_generic_tool_output",
                    "session.toggle.generic_tool_output",
                ),
                ("messages_first", "session.first"),
                ("messages_last", "session.last"),
                ("messages_copy", "messages.copy"),
                ("session_child_first", "session.child.first"),
                ("session_parent", "session.parent"),
                ("session_child_cycle", "session.child.next"),
                ("session_child_cycle_reverse", "session.child.previous"),
                ("session_export", "session.export"),
                ("session_timeline", "session.timeline"),
                ("session_queued_prompts", "session.queued_prompts"),
            ];
            let mut bindings = Vec::new();
            for (name, cmd) in session_names {
                if let Some(binding) = config.get(name) {
                    let mut binding = binding.clone();
                    binding.command = cmd.to_string();
                    bindings.push(binding);
                }
            }
            groups.push(BindingGroup {
                priority: 20,
                enabled: true,
                bindings,
            });
            // Scroll bindings apply even when unfocused.
            let scroll_names = [
                ("messages_page_up", "session.page.up"),
                ("messages_page_down", "session.page.down"),
                ("messages_line_up", "session.line.up"),
                ("messages_line_down", "session.line.down"),
                ("messages_half_page_up", "session.half.page.up"),
                ("messages_half_page_down", "session.half.page.down"),
            ];
            let mut scroll_bindings = Vec::new();
            for (name, cmd) in scroll_names {
                if let Some(binding) = config.get(name) {
                    let mut binding = binding.clone();
                    binding.command = cmd.to_string();
                    scroll_bindings.push(binding);
                }
            }
            groups.push(BindingGroup {
                priority: 10,
                enabled: true,
                bindings: scroll_bindings,
            });
        }

        // App bindings (base mode).
        let app_names = [
            ("command_list", "command.palette.show"),
            ("model_list", "model.list"),
            ("agent_list", "agent.list"),
            ("model_cycle_recent", "model.cycle_recent"),
            ("model_cycle_recent_reverse", "model.cycle_recent_reverse"),
            ("agent_cycle", "agent.cycle"),
            ("agent_cycle_reverse", "agent.cycle.reverse"),
            ("variant_cycle", "variant.cycle"),
            ("provider_connect", "provider.connect"),
            ("status_view", "opencode.status"),
            ("debug_view", "opencode.debug"),
            ("theme_list", "theme.switch"),
            ("theme_switch_mode", "theme.switch_mode"),
            ("help_show", "help.show"),
            ("docs_open", "docs.open"),
            ("session_quick_switch_1", "session.quick_switch.1"),
            ("session_quick_switch_2", "session.quick_switch.2"),
            ("session_quick_switch_3", "session.quick_switch.3"),
            ("session_quick_switch_4", "session.quick_switch.4"),
            ("session_quick_switch_5", "session.quick_switch.5"),
            ("session_quick_switch_6", "session.quick_switch.6"),
            ("session_quick_switch_7", "session.quick_switch.7"),
            ("session_quick_switch_8", "session.quick_switch.8"),
            ("session_quick_switch_9", "session.quick_switch.9"),
            ("tips_toggle", "tips.toggle"),
            ("app_toggle_animations", "app.toggle.animations"),
            ("app_toggle_file_context", "app.toggle.file_context"),
            ("app_toggle_diffwrap", "app.toggle.diffwrap"),
            ("app_toggle_paste_summary", "app.toggle.paste_summary"),
            (
                "app_toggle_session_directory_filter",
                "app.toggle.session_directory_filter",
            ),
            ("mcp_list", "mcp.list"),
            ("terminal_suspend", "terminal.suspend"),
            ("terminal_title_toggle", "terminal.title.toggle"),
        ];
        let mut app_bindings = Vec::new();
        for (name, cmd) in app_names {
            if let Some(binding) = config.get(name) {
                let mut binding = binding.clone();
                binding.command = cmd.to_string();
                app_bindings.push(binding);
            }
        }
        groups.push(BindingGroup {
            priority: 10,
            enabled: !dialog_open,
            bindings: app_bindings,
        });

        // Global + app-exit (lowest priority; app-exit only when input is empty).
        let mut global_bindings = Vec::new();
        if let Some(binding) = config.get("app_exit") {
            global_bindings.push(binding.clone());
        }
        if let Some(binding) = config.get("session_list") {
            let mut binding = binding.clone();
            binding.command = "session.list".to_string();
            global_bindings.push(binding);
        }
        if let Some(binding) = config.get("session_new") {
            let mut binding = binding.clone();
            binding.command = "session.new".to_string();
            global_bindings.push(binding);
        }
        groups.push(BindingGroup {
            priority: 0,
            enabled: self.can_exit(),
            bindings: global_bindings,
        });

        self.keymap.set_groups(groups);
    }

    fn can_exit(&self) -> bool {
        match self.active_prompt_ref() {
            Some(prompt) => prompt.buffer.is_empty(),
            None => true,
        }
    }

    fn route_is_subagent(&self) -> bool {
        match &self.route {
            Route::Session { id } => self
                .sync
                .session(id)
                .map(|s| s.parent_id.is_some())
                .unwrap_or(false),
            Route::Home => false,
        }
    }

    // ---- event handling ---------------------------------------------------------

    pub(crate) fn handle_client_message(&mut self, msg: ClientMessage) {
        match msg {
            ClientMessage::Event(event) => {
                self.sync.apply_event(&event);
                self.handle_tui_event(&event);
            }
            ClientMessage::Bootstrap(data) => {
                let sessions = data.sessions;
                self.sync.providers = data.providers;
                self.sync.agents = data.agents;
                self.sync.commands = data.commands;
                self.sync.config = data.config;
                self.sync.capabilities = data.capabilities;
                self.sync.console_state = data.console_state;
                self.sync.session_status = data.session_status;
                if !sessions.is_empty() {
                    self.sync.replace_sessions(sessions.clone());
                }
                self.sync.status = crate::sync::SyncStatus::Complete;
                self.prompt_ready = true;
                if self.initial_session_id.is_none() && self.continue_requested {
                    if let Some(session) = sessions
                        .iter()
                        .filter(|s| s.parent_id.is_none())
                        .max_by_key(|s| s.time.updated)
                    {
                        self.navigate_session(&session.id);
                    }
                }
            }
        }
    }

    fn handle_tui_event(&mut self, event: &GlobalEvent) {
        match event.payload.r#type.as_str() {
            "session.error" => {
                let error = event.payload.properties.get("error");
                let name = error
                    .and_then(|e| e.get("name"))
                    .and_then(serde_json::Value::as_str);
                if name == Some("MessageAbortedError") {
                    return;
                }
                let message = error
                    .and_then(|e| e.get("data"))
                    .and_then(|d| d.get("message"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Session error");
                self.toasts.show(
                    Toast::new(message)
                        .with_variant(ToastVariant::Error)
                        .with_duration(Duration::from_secs(5)),
                );
            }
            "tui.command.execute" => {
                if let Some(command) = event
                    .payload
                    .properties
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                {
                    self.dispatch(command);
                }
            }
            "tui.toast.show" => {
                let props = &event.payload.properties;
                let message = props
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let variant = match props.get("variant").and_then(serde_json::Value::as_str) {
                    Some("error") => ToastVariant::Error,
                    Some("success") => ToastVariant::Success,
                    Some("warning") => ToastVariant::Warning,
                    _ => ToastVariant::Info,
                };
                let mut toast = Toast::new(message).with_variant(variant);
                if let Some(title) = props.get("title").and_then(serde_json::Value::as_str) {
                    toast = toast.with_title(title);
                }
                self.toasts.show(toast);
            }
            "tui.session.select" => {
                if let Some(id) = event
                    .payload
                    .properties
                    .get("sessionID")
                    .and_then(serde_json::Value::as_str)
                {
                    self.navigate_session(id);
                }
            }
            "session.deleted" => {
                if let Route::Session { id } = &self.route {
                    if let Some(info) = event.payload.properties.get("info") {
                        if info.get("id").and_then(serde_json::Value::as_str) == Some(id.as_str()) {
                            self.route = Route::Home;
                            self.toasts.info("The current session was deleted");
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ---- navigation -------------------------------------------------------------

    pub fn navigate_session(&mut self, session_id: &str) {
        self.route = Route::Session {
            id: session_id.to_string(),
        };
        self.session_prompt = Some(PromptState::default());
        self.dialog = None;
        self.needs_autofocus = true;
        // Request session data if not loaded.
        self.sync_session(session_id);
    }

    pub fn navigate_home(&mut self) {
        self.route = Route::Home;
        self.dialog = None;
    }

    /// Fetch full session data (messages, parts, todo, diff) from the server.
    fn sync_session(&mut self, session_id: &str) {
        let client = self.client.clone();
        let session_id = session_id.to_string();
        let (tx, rx) = mpsc::channel::<SessionData>(1);
        let fetch_id = session_id.clone();
        tokio::spawn(async move {
            let session = client.session_get(&fetch_id).await.ok();
            let messages = client.session_messages(&fetch_id).await.unwrap_or_default();
            let _ = tx.send(SessionData { session, messages }).await;
        });
        // Draining happens in the main loop; store the channel until consumed.
        self.pending_session_data = Some((session_id, rx));
    }
}

pub(crate) struct SessionData {
    pub(crate) session: Option<crate::types::Session>,
    pub(crate) messages: Vec<crate::types::SessionMessageData>,
}

impl App {
    /// Drain pending session sync results.
    pub fn drain_pending(&mut self) {
        if let Some((session_id, mut rx)) = self.pending_session_data.take() {
            match rx.try_recv() {
                Ok(data) => {
                    if let Some(session) = data.session {
                        self.sync.sync_session_data(session, data.messages);
                        self.scroll_to_bottom(&session_id);
                    }
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    self.pending_session_data = Some((session_id, rx));
                }
                Err(_) => {}
            }
        }
    }

    fn scroll_to_bottom(&mut self, session_id: &str) {
        let view = self.session_view_mut(session_id);
        view.sticky_bottom = true;
        view.scroll = 0;
    }

    // ---- input handling ---------------------------------------------------------

    /// Handle a terminal event; returns whether a redraw is needed.
    pub fn handle_input(&mut self, event: crossterm::event::Event) -> bool {
        match event {
            crossterm::event::Event::Key(key) => {
                self.handle_key(key);
                true
            }
            crossterm::event::Event::Paste(text) => {
                let summary_enabled = self.paste_summary_enabled;
                if let Some(prompt) = self.active_prompt() {
                    paste_into_prompt(prompt, &text, summary_enabled);
                    prompt.sync_parts();
                    prompt.update_autocomplete();
                }
                true
            }
            crossterm::event::Event::Resize(w, h) => {
                self.terminal_size = (w, h);
                true
            }
            crossterm::event::Event::Mouse(mouse) => {
                self.handle_mouse(mouse);
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if self.exiting {
            return;
        }
        let now = Instant::now();
        match self.keymap.handle(&key, now) {
            MatchResult::Command(command) => {
                self.leader_active = false;
                self.dispatch(&command);
            }
            MatchResult::Pending => {
                self.leader_active = true;
            }
            MatchResult::None => {
                self.leader_active = self.keymap.leader_active;
                // Raw character input into the focused prompt.
                if self.dialog.is_none() && self.route_is_prompt_focusable() {
                    use crossterm::event::KeyCode;
                    match key.code {
                        KeyCode::Char(c)
                            if key.modifiers == crossterm::event::KeyModifiers::NONE =>
                        {
                            if let Some(prompt) = self.active_prompt() {
                                prompt.buffer.insert(c);
                                prompt.sync_parts();
                                prompt.update_autocomplete();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn route_is_prompt_focusable(&self) -> bool {
        !self.route_is_subagent() && self.active_prompt_ref().is_some()
    }

    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        use crossterm::event::{MouseButton, MouseEventKind};
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_by(-3),
            MouseEventKind::ScrollDown => self.scroll_by(3),
            MouseEventKind::Down(MouseButton::Left) => {
                // Toggle expanded tool output on click.
                self.toggle_tool_at(mouse.row as usize);
            }
            _ => {}
        }
    }

    /// Toggle tool/reasoning expansion for the block under the clicked row.
    fn toggle_tool_at(&mut self, row: usize) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let entry = self.message_owner_at(row);
        let Some(entry) = entry else { return };
        let view = self.session_view_mut(&session_id);
        if entry.starts_with("tool:") {
            let id = entry.trim_start_matches("tool:").to_string();
            if view.expanded_tools.contains(&id) {
                view.expanded_tools.remove(&id);
            } else {
                view.expanded_tools.insert(id);
            }
        } else if entry.starts_with("reasoning:") {
            let id = entry.trim_start_matches("reasoning:").to_string();
            if view.reasoning_expanded.contains(&id) {
                view.reasoning_expanded.remove(&id);
            } else {
                view.reasoning_expanded.insert(id);
            }
        }
    }

    fn message_owner_at(&self, row: usize) -> Option<String> {
        if row >= self.messages.len() {
            return None;
        }
        Some(self.messages[row].owner.clone().unwrap_or_default())
    }

    fn scroll_by(&mut self, delta: i64) {
        let max = self.messages.len() as i64;
        if let Some(id) = self.current_session_id() {
            let view = self.session_view_mut(&id);
            view.scroll = (view.scroll + delta).clamp(0, max.saturating_sub(1));
            if delta > 0 {
                view.sticky_bottom = view.scroll >= max.saturating_sub(1);
            }
        }
    }

    fn scroll_to(&mut self, target: i64) {
        let max = self.messages.len() as i64;
        if let Some(id) = self.current_session_id() {
            let view = self.session_view_mut(&id);
            view.scroll = target.clamp(0, max.saturating_sub(1));
            view.sticky_bottom = view.scroll >= max.saturating_sub(1);
        }
    }

    // ---- command dispatch -------------------------------------------------------

    pub fn dispatch(&mut self, command: &str) {
        match command {
            "app.exit" => self.exit_app(),
            "command.palette.show" => self.open_dialog(DialogKind::CommandPalette),
            "session.new" => self.navigate_home(),
            "session.list" => self.open_dialog(DialogKind::SessionList),
            "model.list" => self.open_dialog(DialogKind::ModelList),
            "agent.list" => self.open_dialog(DialogKind::AgentList),
            "provider.connect" => self.open_dialog(DialogKind::ProviderList),
            "help.show" => self.open_dialog(DialogKind::Help),
            "opencode.status" => self.show_status_dialog(),
            "opencode.debug" => self.show_debug_dialog(),
            "theme.switch" => self.show_theme_dialog(),
            "theme.switch_mode" => {
                self.theme = if self.theme.mode == Mode::Dark {
                    Theme::light()
                } else {
                    Theme::dark()
                };
                self.dialog = None;
            }
            "session.sidebar.toggle" => {
                if let Some(id) = self.current_session_id() {
                    let view = self.session_view_mut(&id);
                    view.sidebar_visible = !view.sidebar_visible;
                }
                self.dialog = None;
            }
            "session.toggle.conceal" => {
                self.kv_set("conceal", !self.kv_get_bool("conceal", true));
                self.dialog = None;
            }
            "session.toggle.timestamps" => {
                self.kv_set("timestamps", !self.kv_get_bool("timestamps", false));
                self.dialog = None;
            }
            "session.toggle.actions" => {
                self.kv_set(
                    "tool_details_visibility",
                    !self.kv_get_bool("tool_details_visibility", true),
                );
                self.dialog = None;
            }
            "session.toggle.thinking" => {
                let next = crate::util::display::next_thinking_mode(
                    &self.kv_get_str("thinking_mode", "hide"),
                );
                self.kv_set("thinking_mode", next);
                self.dialog = None;
            }
            "session.toggle.scrollbar" => {
                self.kv_set(
                    "scrollbar_visible",
                    !self.kv_get_bool("scrollbar_visible", false),
                );
                self.dialog = None;
            }
            "session.toggle.generic_tool_output" => {
                self.kv_set(
                    "generic_tool_output_visibility",
                    !self.kv_get_bool("generic_tool_output_visibility", false),
                );
                self.dialog = None;
            }
            "session.page.up" => self.scroll_by(-(self.message_height() as i64) / 2),
            "session.page.down" => self.scroll_by(self.message_height() as i64 / 2),
            "session.line.up" => self.scroll_by(-1),
            "session.line.down" => self.scroll_by(1),
            "session.half.page.up" => self.scroll_by(-(self.message_height() as i64) / 4),
            "session.half.page.down" => self.scroll_by(self.message_height() as i64 / 4),
            "session.first" => self.scroll_to(0),
            "session.last" => self.scroll_to(i64::MAX),
            "session.messages_last_user" => self.scroll_to_last_user(),
            "model.cycle_recent" => {
                self.local.cycle_recent_model(1);
            }
            "model.cycle_recent_reverse" => {
                self.local.cycle_recent_model(-1);
            }
            "agent.cycle" => {
                self.local.cycle_agent(&self.sync, 1);
            }
            "agent.cycle.reverse" => {
                self.local.cycle_agent(&self.sync, -1);
            }
            "variant.cycle" => self.toasts.info("No variants available"),
            "session.undo" => self.undo_message(),
            "session.redo" => self.redo_message(),
            "session.share" => self.share_session(),
            "session.unshare" => self.unshare_session(),
            "session.compact" => self.compact_session(),
            "session.rename" => self.open_dialog(DialogKind::Rename),
            "session.delete" => self.delete_session(),
            "session.child.first" => self.navigate_child(0),
            "session.parent" => self.navigate_parent(),
            "session.child.next" => self.navigate_child(1),
            "session.child.previous" => self.navigate_child(-1),
            "session.quick_switch.1" => self.quick_switch(0),
            "session.quick_switch.2" => self.quick_switch(1),
            "session.quick_switch.3" => self.quick_switch(2),
            "session.quick_switch.4" => self.quick_switch(3),
            "session.quick_switch.5" => self.quick_switch(4),
            "session.quick_switch.6" => self.quick_switch(5),
            "session.quick_switch.7" => self.quick_switch(6),
            "session.quick_switch.8" => self.quick_switch(7),
            "session.quick_switch.9" => self.quick_switch(8),
            "session.interrupt" => self.interrupt_session(),
            "session.export" => {
                self.toasts
                    .info("TODO(integration): session export to editor");
            }
            "session.timeline" => {
                self.toasts.info("TODO(integration): session timeline");
            }
            "session.queued_prompts" => {
                self.toasts.info("TODO(integration): queued prompts");
            }
            "session.background" => {
                self.toasts.info("TODO(integration): background subagents");
            }
            "messages.copy" => self.copy_last_assistant_message(),
            "prompt.clear" => self.clear_prompt(),
            "prompt.submit" => {
                if self.active_prompt().is_some() {
                    self.submit_prompt();
                }
            }
            "prompt.editor" => self.toasts.info("TODO(integration): external editor"),
            "prompt.paste" => self.toasts.info("TODO(integration): clipboard paste"),
            "prompt.stash" => self.stash_prompt(),
            "prompt.stash.pop" => self.stash_pop(),
            "prompt.stash.list" => self.open_dialog(DialogKind::StashList),
            "prompt.skills" => self.toasts.info("TODO(integration): skill selector"),
            "prompt.history.previous" => self.history_move(-1),
            "prompt.history.next" => self.history_move(1),
            "prompt.shell.enter" => self.set_prompt_mode(PromptMode::Shell),
            "prompt.shell.exit" => self.set_prompt_mode(PromptMode::Normal),
            "prompt.autocomplete.prev" => {
                if let Some(prompt) = self.active_prompt() {
                    prompt.autocomplete.move_selection(-1);
                }
            }
            "prompt.autocomplete.next" => {
                if let Some(prompt) = self.active_prompt() {
                    prompt.autocomplete.move_selection(1);
                }
            }
            "prompt.autocomplete.hide" => {
                if let Some(prompt) = self.active_prompt() {
                    prompt.hide_autocomplete();
                }
            }
            "prompt.autocomplete.select" => {
                if let Some(prompt) = self.active_prompt() {
                    prompt.apply_autocomplete();
                }
            }
            "prompt.autocomplete.complete" => {
                if let Some(prompt) = self.active_prompt() {
                    prompt.apply_autocomplete();
                }
            }
            "tips.toggle" => self.kv_toggle("tips_enabled"),
            "app.toggle.animations" => self.kv_toggle("animations_enabled"),
            "app.toggle.file_context" => self.kv_toggle("file_context_enabled"),
            "app.toggle.diffwrap" => {
                let next = if self.kv_get_str("diff_wrap_mode", "word") == "word" {
                    "none"
                } else {
                    "word"
                };
                self.kv_set("diff_wrap_mode", next);
            }
            "app.toggle.paste_summary" => self.kv_toggle("paste_summary_enabled"),
            "app.toggle.session_directory_filter" => {
                self.kv_toggle("session_directory_filter_enabled")
            }
            "permission.prompt.fullscreen" => {
                if let Some(id) = self.current_session_id() {
                    let view = self.session_view_mut(&id);
                    let _ = view;
                }
            }
            "dialog.select.prev" => self.dialog_move(-1),
            "dialog.select.next" => self.dialog_move(1),
            "dialog.select.page_up" => self.dialog_move_page(-1),
            "dialog.select.page_down" => self.dialog_move_page(1),
            "dialog.select.home" => self.dialog_home(),
            "dialog.select.end" => self.dialog_end(),
            "dialog.select.submit" => self.dialog_submit(),
            "dialog.prompt.submit" => self.dialog_submit(),
            "input.submit" => {
                if self.active_prompt().is_some() {
                    self.submit_prompt();
                }
            }
            _ => {
                // Input movement commands.
                if let Some(prompt) = self.active_prompt() {
                    match command {
                        "input.move.left" => prompt.buffer.move_left(),
                        "input.move.right" => prompt.buffer.move_right(),
                        "input.move.up" => prompt.buffer.move_up(),
                        "input.move.down" => prompt.buffer.move_down(),
                        "input.select.left" => prompt.buffer.select_left(),
                        "input.select.right" => prompt.buffer.select_right(),
                        "input.select.up" => prompt.buffer.select_up(),
                        "input.select.down" => prompt.buffer.select_down(),
                        "input.line.home" | "input.visual.line.home" => prompt.buffer.line_home(),
                        "input.line.end" | "input.visual.line.end" => {
                            prompt.buffer.line_end_cursor()
                        }
                        "input.select.line.home" | "input.select.visual.line.home" => {
                            prompt.buffer.select_line_home()
                        }
                        "input.select.line.end" | "input.select.visual.line.end" => {
                            prompt.buffer.select_line_end()
                        }
                        "input.buffer.home" => prompt.buffer.buffer_home(),
                        "input.buffer.end" => prompt.buffer.buffer_end(),
                        "input.select.buffer.home" => prompt.buffer.select_buffer_home(),
                        "input.select.buffer.end" => prompt.buffer.select_buffer_end(),
                        "input.delete.line" => prompt.buffer.delete_line(),
                        "input.delete.to.line.end" => prompt.buffer.delete_to_line_end(),
                        "input.delete.to.line.start" => prompt.buffer.delete_to_line_start(),
                        "input.backspace" => {
                            if prompt.mode == PromptMode::Shell && prompt.buffer.cursor() == 0 {
                                prompt.mode = PromptMode::Normal;
                            } else {
                                prompt.buffer.backspace();
                            }
                        }
                        "input.delete" => prompt.buffer.delete(),
                        "input.newline" => prompt.buffer.insert('\n'),
                        "input.undo" => prompt.buffer.undo(),
                        "input.redo" => prompt.buffer.redo(),
                        "input.word.forward" => prompt.buffer.word_forward(),
                        "input.word.backward" => prompt.buffer.word_backward(),
                        "input.select.word.forward" => prompt.buffer.select_word_forward(),
                        "input.select.word.backward" => prompt.buffer.select_word_backward(),
                        "input.delete.word.forward" => prompt.buffer.delete_word_forward(),
                        "input.delete.word.backward" => prompt.buffer.delete_word_backward(),
                        "input.select.all" => prompt.buffer.select_all(),
                        _ => {
                            tracing::debug!(command, "unhandled command");
                        }
                    }
                    prompt.sync_parts();
                    prompt.update_autocomplete();
                }
            }
        }
    }
}

// ---- helper commands --------------------------------------------------------

impl App {
    pub fn exit_app(&mut self) {
        self.exiting = true;
    }

    fn kv_get_bool(&self, key: &str, default: bool) -> bool {
        self.kv
            .get(key)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(default)
    }
    fn kv_get_str(&self, key: &str, default: &str) -> String {
        self.kv
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or_else(|| default.to_string())
    }
    fn kv_set(&mut self, key: &str, value: impl Into<serde_json::Value>) {
        self.kv.insert(key.to_string(), value.into());
    }
    fn kv_toggle(&mut self, key: &str) {
        let next = !self.kv_get_bool(key, true);
        self.kv_set(key, next);
    }

    fn open_dialog(&mut self, kind: DialogKind) {
        self.dialog = Some(DialogState::new(kind));
    }

    fn show_status_dialog(&mut self) {
        let mut items = Vec::new();
        for provider in &self.sync.providers {
            items.push(DialogItem::new(format!(
                "{} — {} models",
                provider.name,
                provider.models.len()
            )));
        }
        items.push(DialogItem::new(format!(
            "Sessions: {}",
            self.sync.sessions.len()
        )));
        self.dialog = Some(DialogState::new(DialogKind::InfoItems {
            title: "Status".to_string(),
            items,
        }));
    }

    fn show_debug_dialog(&mut self) {
        let mut items = Vec::new();
        items.push(DialogItem::new(format!("Version: {}", crate::version())));
        items.push(DialogItem::new(format!("CWD: {}", self.cwd.display())));
        self.dialog = Some(DialogState::new(DialogKind::InfoItems {
            title: "Debug".to_string(),
            items,
        }));
    }

    fn show_theme_dialog(&mut self) {
        let mut items = vec![DialogItem::new("opencode")];
        if self.theme.mode == Mode::Dark {
            items.push(DialogItem::new("Switch to light mode"));
        } else {
            items.push(DialogItem::new("Switch to dark mode"));
        }
        self.dialog = Some(DialogState::new(DialogKind::InfoItems {
            title: "Themes".to_string(),
            items,
        }));
    }

    fn message_height(&self) -> usize {
        self.messages.len().max(1)
    }

    fn scroll_to_last_user(&mut self) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let messages = self.sync.messages_for(&session_id);
        let mut target: Option<usize> = None;
        for (idx, message) in messages.iter().enumerate() {
            if message.role() == "user" {
                let has_text = self
                    .sync
                    .parts_for(message.id())
                    .iter()
                    .any(|p| matches!(p, crate::types::Part::Text(t) if !t.is_synthetic() && !t.is_ignored()));
                if has_text {
                    target = Some(idx);
                }
            }
        }
        if let Some(idx) = target {
            let line = self.message_line_for_message(idx);
            let view = self.session_view_mut(&session_id);
            view.scroll = line as i64;
            view.sticky_bottom = false;
        }
    }

    fn message_line_for_message(&self, message_index: usize) -> usize {
        let Some(session_id) = self.session_id() else {
            return 0;
        };
        let target_id = self
            .sync
            .messages_for(session_id)
            .get(message_index)
            .map(|m| m.id())
            .unwrap_or("");
        self.messages
            .iter()
            .position(|l| l.owner.as_deref() == Some(target_id))
            .unwrap_or(0)
    }

    fn undo_message(&mut self) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let client = self.client.clone();
        let sid = session_id.clone();
        let status = self.sync.session_status.get(&sid).cloned();
        let abort = status.is_some_and(|s| s.kind() != "idle");
        let abort_sid = sid.clone();
        tokio::spawn(async move {
            if abort {
                let _ = client.session_abort(&abort_sid).await;
            }
        });
        let messages = self.sync.messages_for(&sid).to_vec();
        let revert = self.sync.session(&sid).and_then(|s| s.revert.clone());
        let target = messages
            .iter()
            .rev()
            .find(|m| {
                m.role() == "user"
                    && (revert
                        .as_ref()
                        .is_none_or(|r| m.id() < r.message_id.as_str()))
            })
            .map(|m| m.id().to_string());
        if let Some(message_id) = target {
            let client = self.client.clone();
            let revert_sid = sid.clone();
            let mid = message_id.clone();
            tokio::spawn(async move {
                let _ = client.session_revert(&revert_sid, &mid).await;
            });
            // Restore the prompt from the reverted message's parts.
            let parts = self.sync.parts_for(&message_id).to_vec();
            if let Some(prompt) = self.active_prompt() {
                let mut input = String::new();
                for part in parts {
                    if let crate::types::Part::Text(t) = part {
                        if !t.is_synthetic() {
                            input.push_str(&t.text);
                        }
                    }
                }
                prompt.buffer.set_text(&input);
                prompt.parts = Vec::new();
                prompt.update_autocomplete();
            }
        }
        self.dialog = None;
    }

    fn redo_message(&mut self) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let Some(revert) = self
            .sync
            .session(&session_id)
            .and_then(|s| s.revert.clone())
        else {
            return;
        };
        let client = self.client.clone();
        let sid = session_id.clone();
        let messages = self.sync.messages_for(&sid).to_vec();
        let next_user = messages
            .iter()
            .find(|m| m.role() == "user" && m.id() > revert.message_id.as_str())
            .map(|m| m.id().to_string());
        if let Some(next_user) = next_user {
            tokio::spawn(async move {
                let _ = client.session_revert(&sid, &next_user).await;
            });
        } else {
            tokio::spawn(async move {
                let _ = client.session_unrevert(&sid).await;
            });
            if let Some(prompt) = self.active_prompt() {
                prompt.buffer.set_text("");
                prompt.parts = Vec::new();
            }
        }
        self.dialog = None;
    }

    fn share_session(&mut self) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let client = self.client.clone();
        let sid = session_id.clone();
        let toast_kind = self.toasts.clone();
        tokio::spawn(async move {
            match client.session_share(&sid).await {
                Ok(()) => {
                    // The session.updated event will surface the share URL.
                }
                Err(error) => {
                    let _ = toast_kind;
                    tracing::warn!(%error, "share failed");
                }
            }
        });
        self.dialog = None;
    }

    fn unshare_session(&mut self) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let client = self.client.clone();
        let sid = session_id.clone();
        tokio::spawn(async move {
            let _ = client.session_unshare(&sid).await;
        });
        self.dialog = None;
    }

    fn compact_session(&mut self) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let client = self.client.clone();
        let sid = session_id.clone();
        tokio::spawn(async move {
            let _ = client.session_compact(&sid).await;
        });
        self.dialog = None;
    }

    fn delete_session(&mut self) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let client = self.client.clone();
        let sid = session_id.clone();
        tokio::spawn(async move {
            let _ = client.session_delete(&sid).await;
        });
        self.dialog = None;
    }

    fn navigate_parent(&mut self) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let parent = self
            .sync
            .session(&session_id)
            .and_then(|s| s.parent_id.clone());
        if let Some(parent) = parent {
            self.navigate_session(&parent);
        }
        self.dialog = None;
    }

    fn navigate_child(&mut self, offset: i32) {
        let Some(session_id) = self.current_session_id() else {
            return;
        };
        let parent_id = self
            .sync
            .session(&session_id)
            .and_then(|s| s.parent_id.clone())
            .unwrap_or_else(|| session_id.clone());
        let mut children: Vec<&crate::types::Session> = self
            .sync
            .sessions
            .iter()
            .filter(|s| s.parent_id.as_deref() == Some(parent_id.as_str()))
            .collect();
        children.sort_by(|a, b| a.id.cmp(&b.id));
        if children.is_empty() {
            return;
        }
        let current_idx = children
            .iter()
            .position(|s| s.id == session_id)
            .unwrap_or(0);
        let next = if offset == 0 {
            0
        } else {
            (current_idx as i32 + offset).rem_euclid(children.len() as i32) as usize
        };
        self.navigate_session(&children[next].id.clone());
        self.dialog = None;
    }

    fn quick_switch(&mut self, slot: usize) {
        let id = self.sync.sessions.iter().nth(slot).map(|s| s.id.clone());
        if let Some(id) = id {
            self.navigate_session(&id);
        }
    }

    fn interrupt_session(&mut self) {
        if let Some(prompt) = self.active_prompt() {
            if prompt.mode == PromptMode::Shell {
                prompt.mode = PromptMode::Normal;
                return;
            }
            prompt.interrupt += 1;
            if prompt.interrupt >= 3 {
                prompt.interrupt = 0;
                if let Some(session_id) = self.current_session_id() {
                    let client = self.client.clone();
                    let sid = session_id.clone();
                    tokio::spawn(async move {
                        let _ = client.session_abort(&sid).await;
                    });
                }
            }
        }
    }

    fn copy_last_assistant_message(&mut self) {
        let Some(session_id) = self.current_session_id() else {
            self.toasts.error("No assistant messages found");
            self.dialog = None;
            return;
        };
        let revert_id = self
            .sync
            .session(&session_id)
            .and_then(|s| s.revert.clone());
        let last = self
            .sync
            .messages_for(&session_id)
            .iter()
            .rev()
            .find(|m| {
                m.role() == "assistant"
                    && (revert_id
                        .as_ref()
                        .is_none_or(|r| m.id() < r.message_id.as_str()))
            })
            .map(|m| m.id().to_string());
        let Some(message_id) = last else {
            self.toasts.error("No assistant messages found");
            self.dialog = None;
            return;
        };
        let text: String = self
            .sync
            .parts_for(&message_id)
            .iter()
            .filter_map(|p| match p {
                crate::types::Part::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if text.is_empty() {
            self.toasts
                .error("No text content found in last assistant message");
        } else {
            self.toasts.info("Message copied to clipboard");
            // TODO(integration): write to the system clipboard.
        }
        self.dialog = None;
    }

    fn clear_prompt(&mut self) {
        if let Some(prompt) = self.active_prompt() {
            prompt.buffer.clear();
            prompt.parts = Vec::new();
            prompt.update_autocomplete();
        }
        self.dialog = None;
    }

    fn set_prompt_mode(&mut self, mode: PromptMode) {
        if let Some(prompt) = self.active_prompt() {
            prompt.mode = mode;
            if mode == PromptMode::Shell {
                prompt.buffer.set_cursor(0);
            }
        }
    }

    fn history_move(&mut self, direction: i32) {
        let current = self.active_prompt().map(|p| p.text()).unwrap_or_default();
        let item = if direction < 0 {
            self.history.move_previous(&current)
        } else {
            self.history.move_next(&current)
        };
        if let Some(item) = item {
            let mode = match item.effective_mode() {
                "shell" => PromptMode::Shell,
                _ => PromptMode::Normal,
            };
            if let Some(prompt) = self.active_prompt() {
                prompt.buffer.set_text(&item.input);
                prompt.parts = item.parts;
                prompt.mode = mode;
                if direction > 0 {
                    prompt.buffer.buffer_end();
                } else {
                    prompt.buffer.buffer_home();
                }
                prompt.update_autocomplete();
            }
        }
    }

    fn stash_prompt(&mut self) {
        let (text, parts, mode) = match self.active_prompt() {
            Some(prompt) => {
                if prompt.buffer.is_empty() {
                    return;
                }
                (
                    prompt.text(),
                    prompt.parts.clone(),
                    Some(prompt.mode.as_str().to_string()),
                )
            }
            None => return,
        };
        self.stash.push(text, parts, mode);
        if let Some(prompt) = self.active_prompt() {
            prompt.buffer.clear();
            prompt.parts = Vec::new();
        }
        self.dialog = None;
    }

    fn stash_pop(&mut self) {
        if let Some(entry) = self.stash.pop() {
            if let Some(prompt) = self.active_prompt() {
                prompt.buffer.set_text(&entry.input);
                prompt.parts = entry.parts;
                prompt.buffer.buffer_end();
            }
        }
        self.dialog = None;
    }

    // ---- prompt submission ------------------------------------------------------

    /// Submit the active prompt. Mirrors `submitInner` in
    /// reference/packages/tui/src/component/prompt/index.tsx.
    pub fn submit_prompt(&mut self) {
        let Some(prompt) = self.active_prompt_ref() else {
            return;
        };
        if prompt.buffer.is_empty() {
            return;
        }
        let text = prompt.text();
        let trimmed = text.trim().to_string();
        if trimmed == "exit" || trimmed == "quit" || trimmed == ":q" {
            self.exiting = true;
            return;
        }

        let Some(agent) = self.local.current_agent(&self.sync).map(|a| a.name.clone()) else {
            self.toasts.warning("No agents configured");
            return;
        };
        let Some(model) = self.local.current_model(&self.sync) else {
            self.toasts.warning("Connect a provider to send prompts");
            return;
        };
        let mode = prompt.mode;
        let parts = prompt.parts.clone();
        let input_text = expand_text_parts(&text, &parts);

        // History append (with draft retention for prompt.clear).
        self.history.append(crate::prompt::history::PromptInfo {
            input: text.clone(),
            mode: Some(mode.as_str().to_string()),
            parts: parts.clone(),
        });

        // Take the prompt state out to reset it.
        if let Some(prompt) = self.active_prompt() {
            prompt.buffer.clear();
            prompt.parts = Vec::new();
            prompt.mode = PromptMode::Normal;
            prompt.update_autocomplete();
        }

        if let Some(session_id) = self.current_session_id() {
            self.send_to_session(&session_id, &input_text, &agent, &model, mode, parts);
        } else {
            // Create a session first, then send.
            let create_agent = agent.clone();
            let create_model = crate::types::ModelRef {
                id: model.model_id.clone(),
                provider_id: model.provider_id.clone(),
                variant: model.variant.clone(),
            };
            let client = self.client.clone();
            let (tx, rx) = mpsc::channel::<Option<String>>(1);
            let directory = self.cwd.to_string_lossy().to_string();
            tokio::spawn(async move {
                let created = client
                    .session_create(crate::client::SessionCreateInput {
                        directory: Some(directory),
                        agent: Some(create_agent),
                        model: Some(create_model),
                        workspace: None,
                        workspace_id: None,
                    })
                    .await
                    .ok();
                let _ = tx.send(created.map(|s| s.id)).await;
            });
            self.pending_create = Some((rx, input_text, agent, model, mode, parts));
        }
    }

    fn send_to_session(
        &mut self,
        session_id: &str,
        input_text: &str,
        agent: &str,
        model: &crate::local::ModelSelection,
        mode: PromptMode,
        parts: Vec<serde_json::Value>,
    ) {
        let client = self.client.clone();
        let sid = session_id.to_string();
        let input_text = input_text.to_string();
        let agent = agent.to_string();
        let model_ref = crate::types::ModelRef {
            id: model.model_id.clone(),
            provider_id: model.provider_id.clone(),
            variant: model.variant.clone(),
        };
        match mode {
            PromptMode::Shell => {
                let shell_variant = model_ref.variant.clone();
                tokio::spawn(async move {
                    let _ = client
                        .session_shell(crate::client::ShellInput {
                            session_id: sid,
                            agent,
                            model: crate::types::ModelRef {
                                id: model_ref.id,
                                provider_id: model_ref.provider_id,
                                variant: shell_variant,
                            },
                            command: input_text,
                        })
                        .await;
                });
            }
            PromptMode::Normal if input_text.starts_with('/') => {
                let command = input_text[1..]
                    .split('\n')
                    .next()
                    .unwrap_or("")
                    .split(' ')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let known = self.sync.commands.iter().any(|c| c.name == command);
                let (command_name, args) = if known {
                    let first_line_end = input_text.find('\n').unwrap_or(input_text.len());
                    let first_line = &input_text[..first_line_end];
                    let rest = if first_line_end < input_text.len() {
                        &input_text[first_line_end + 1..]
                    } else {
                        ""
                    };
                    let mut parts_iter = first_line.splitn(2, ' ');
                    let cmd = parts_iter.next().unwrap_or("").to_string();
                    let first_args = parts_iter.next().unwrap_or("").to_string();
                    let args = if rest.is_empty() {
                        first_args
                    } else if first_args.is_empty() {
                        rest.to_string()
                    } else {
                        format!("{first_args}\n{rest}")
                    };
                    (cmd, args)
                } else {
                    (command, String::new())
                };
                let file_parts: Vec<serde_json::Value> = parts
                    .into_iter()
                    .filter(|p| p.get("type").and_then(serde_json::Value::as_str) == Some("file"))
                    .collect();
                tokio::spawn(async move {
                    let _ = client
                        .session_command(crate::client::CommandInput {
                            session_id: sid,
                            command: command_name,
                            arguments: args,
                            agent,
                            model: format!("{}/{}", model_ref.provider_id, model_ref.id),
                            variant: model_ref.variant.clone(),
                            parts: file_parts,
                        })
                        .await;
                });
            }
            _ => {
                let non_text_parts = strip_prompt_part_ids(&parts);
                let prompt_variant = model_ref.variant.clone();
                tokio::spawn(async move {
                    let _ = client
                        .session_prompt(crate::client::PromptInput {
                            session_id: sid,
                            agent,
                            model: model_ref,
                            variant: prompt_variant,
                            parts: non_text_parts,
                        })
                        .await;
                });
            }
        }
        if let Some(on_submit) = self.on_submit.take() {
            (on_submit)();
        }
    }

    // ---- dialog handling ---------------------------------------------------------

    fn dialog_items(&self) -> Vec<DialogItem> {
        let Some(dialog) = &self.dialog else {
            return Vec::new();
        };
        match &dialog.kind {
            DialogKind::CommandPalette => self.palette_items(),
            DialogKind::ModelList => {
                let current = self.local.current_model(&self.sync);
                let mut items = Vec::new();
                for provider in &self.sync.providers {
                    let mut ids: Vec<&String> = provider.models.keys().collect();
                    ids.sort();
                    for id in ids {
                        let model = &provider.models[id];
                        items.push(
                            DialogItem::new(format!("{} / {}", provider.name, model.name))
                                .with_description(format!(
                                    "{provider_id}",
                                    provider_id = provider.id
                                )),
                        );
                        if let Some(current) = &current {
                            if current.provider_id == provider.id && current.model_id == *id {
                                if let Some(last) = items.last_mut() {
                                    last.selected = true;
                                }
                            }
                        }
                    }
                }
                items
            }
            DialogKind::AgentList => {
                let current = self.local.current_agent(&self.sync).map(|a| a.name.clone());
                let mut items: Vec<DialogItem> = self
                    .local
                    .primary_agents(&self.sync)
                    .into_iter()
                    .map(|a| {
                        let mut item = DialogItem::new(a.name.clone());
                        if let Some(desc) = &a.description {
                            item.description = Some(desc.clone());
                        }
                        if current.as_deref() == Some(a.name.as_str()) {
                            item.selected = true;
                        }
                        item
                    })
                    .collect();
                items.extend(
                    self.local
                        .all_agents(&self.sync)
                        .into_iter()
                        .filter(|a| a.mode == "subagent")
                        .map(|a| DialogItem::new(format!("@{}", a.name))),
                );
                items
            }
            DialogKind::SessionList => {
                let mut items: Vec<DialogItem> = self
                    .sync
                    .sessions
                    .iter()
                    .map(|s| {
                        DialogItem::new(s.title.clone())
                            .with_description(format!("{} · {}", s.id, s.time.updated))
                    })
                    .collect();
                items.sort_by(|a, b| a.description.cmp(&b.description));
                items
            }
            DialogKind::ProviderList => {
                let mut items: Vec<DialogItem> = self
                    .sync
                    .providers
                    .iter()
                    .map(|p| {
                        DialogItem::new(p.name.clone())
                            .with_description(format!("{} models", p.models.len()))
                    })
                    .collect();
                if items.is_empty() {
                    items.push(DialogItem::new("No providers connected"));
                }
                items
            }
            DialogKind::InfoItems { items, .. } => items.clone(),
            DialogKind::StashList => self
                .stash
                .list()
                .iter()
                .enumerate()
                .map(|(idx, entry)| DialogItem::new(format!("{}. {}", idx + 1, entry.input)))
                .collect(),
            DialogKind::Help
            | DialogKind::Rename
            | DialogKind::Confirm { .. }
            | DialogKind::Alert { .. } => Vec::new(),
        }
    }

    fn palette_items(&self) -> Vec<DialogItem> {
        let mut items = Vec::new();
        let add = |items: &mut Vec<DialogItem>, name: &str, title: &str, desc: Option<String>| {
            items.push(DialogItem::new(title).with_description(desc.unwrap_or_default()));
            let _ = name;
        };
        add(
            &mut items,
            "command.palette.show",
            "Show command palette",
            None,
        );
        add(&mut items, "session.list", "Switch session", None);
        add(&mut items, "session.new", "New session", None);
        add(&mut items, "model.list", "Switch model", None);
        add(&mut items, "agent.list", "Switch agent", None);
        add(&mut items, "mcp.list", "Toggle MCPs", None);
        add(&mut items, "provider.connect", "Connect provider", None);
        add(&mut items, "opencode.status", "View status", None);
        add(&mut items, "opencode.debug", "View debug info", None);
        add(&mut items, "theme.switch", "Switch theme", None);
        add(&mut items, "help.show", "Help", None);
        add(&mut items, "session.rename", "Rename session", None);
        add(&mut items, "session.share", "Share session", None);
        add(&mut items, "session.undo", "Undo previous message", None);
        add(&mut items, "session.compact", "Compact session", None);
        add(&mut items, "session.timeline", "Jump to message", None);
        add(&mut items, "session.export", "Export session", None);
        add(
            &mut items,
            "messages.copy",
            "Copy last assistant message",
            None,
        );
        add(&mut items, "app.exit", "Exit the app", None);
        items
    }

    fn dialog_move(&mut self, delta: i32) {
        let total = self.dialog_items().len();
        if let Some(dialog) = &mut self.dialog {
            dialog.selected = dialog::move_selection(dialog.selected, total, delta);
        }
    }

    fn dialog_move_page(&mut self, direction: i32) {
        let total = self.dialog_items().len();
        let page = 10i32;
        if let Some(dialog) = &mut self.dialog {
            dialog.selected = dialog::move_selection(dialog.selected, total, direction * page);
        }
    }

    fn dialog_home(&mut self) {
        if let Some(dialog) = &mut self.dialog {
            dialog.selected = 0;
        }
    }

    fn dialog_end(&mut self) {
        let total = self.dialog_items().len();
        if let Some(dialog) = &mut self.dialog {
            dialog.selected = total.saturating_sub(1);
        }
    }

    fn dialog_submit(&mut self) {
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        match dialog.kind {
            DialogKind::ModelList => {
                let items = self.dialog_items();
                if let Some(item) = items.get(dialog.selected) {
                    if let Some(model) = self.parse_model_item(item) {
                        self.local.set_model(model, true);
                    }
                }
                self.dialog = None;
            }
            DialogKind::AgentList => {
                let items = self.dialog_items();
                if let Some(item) = items.get(dialog.selected) {
                    let name = item.title.trim_start_matches('@').to_string();
                    if self
                        .local
                        .all_agents(&self.sync)
                        .iter()
                        .any(|a| a.name == name)
                    {
                        self.local.set_agent(&name);
                    }
                }
                self.dialog = None;
            }
            DialogKind::SessionList => {
                let items = self.dialog_items();
                if let Some(item) = items.get(dialog.selected) {
                    // The description carries the session id.
                    if let Some(desc) = &item.description {
                        let id = desc.split(" · ").next().unwrap_or("").to_string();
                        if !id.is_empty() {
                            self.navigate_session(&id);
                            return;
                        }
                    }
                    if let Some(session) = self.sync.sessions.get(dialog.selected) {
                        let id = session.id.clone();
                        self.navigate_session(&id);
                    }
                }
                self.dialog = None;
            }
            DialogKind::ProviderList => {
                self.dialog = None;
            }
            DialogKind::CommandPalette => {
                let items = self.dialog_items();
                let command = if let Some(item) = items.get(dialog.selected) {
                    self.palette_command_for_title(&item.title)
                } else {
                    None
                };
                self.dialog = None;
                if let Some(command) = command {
                    self.dispatch(&command);
                }
            }
            DialogKind::InfoItems { title, items } => {
                if title == "Themes" {
                    if let Some(item) = items.get(dialog.selected) {
                        if item.title.contains("light mode") {
                            self.theme = Theme::light();
                        } else if item.title.contains("dark mode") {
                            self.theme = Theme::dark();
                        }
                    }
                }
                self.dialog = None;
            }
            DialogKind::StashList => {
                let items = self.dialog_items();
                if let Some(item) = items.get(dialog.selected) {
                    if let Some(entry) = self.stash.list().get(dialog.selected).cloned() {
                        if let Some(prompt) = self.active_prompt() {
                            prompt.buffer.set_text(&entry.input);
                            prompt.parts = entry.parts;
                            prompt.buffer.buffer_end();
                        }
                    }
                    let _ = item;
                }
                self.dialog = None;
            }
            DialogKind::Help => self.dialog = None,
            DialogKind::Rename => self.dialog = None,
            DialogKind::Confirm { .. } | DialogKind::Alert { .. } => self.dialog = None,
        }
    }

    fn parse_model_item(&self, item: &DialogItem) -> Option<crate::local::ModelSelection> {
        // Item title is "ProviderName / ModelName"; find by matching.
        for provider in &self.sync.providers {
            for (id, model) in &provider.models {
                if format!("{} / {}", provider.name, model.name) == item.title {
                    return Some(crate::local::ModelSelection {
                        provider_id: provider.id.clone(),
                        model_id: id.clone(),
                        variant: None,
                    });
                }
            }
        }
        None
    }

    fn palette_command_for_title(&self, title: &str) -> Option<String> {
        self.palette_items()
            .iter()
            .position(|i| i.title == title)
            .and_then(|idx| {
                let titles = [
                    "Show command palette",
                    "Switch session",
                    "New session",
                    "Switch model",
                    "Switch agent",
                    "Toggle MCPs",
                    "Connect provider",
                    "View status",
                    "View debug info",
                    "Switch theme",
                    "Help",
                    "Rename session",
                    "Share session",
                    "Undo previous message",
                    "Compact session",
                    "Jump to message",
                    "Export session",
                    "Copy last assistant message",
                    "Exit the app",
                ];
                let commands = [
                    "command.palette.show",
                    "session.list",
                    "session.new",
                    "model.list",
                    "agent.list",
                    "mcp.list",
                    "provider.connect",
                    "opencode.status",
                    "opencode.debug",
                    "theme.switch",
                    "help.show",
                    "session.rename",
                    "session.share",
                    "session.undo",
                    "session.compact",
                    "session.timeline",
                    "session.export",
                    "messages.copy",
                    "app.exit",
                ];
                titles.get(idx).and_then(|t| {
                    let _ = t;
                    commands.get(idx).map(|c| c.to_string())
                })
            })
    }
}

// ---- rendering --------------------------------------------------------------

impl App {
    /// Draw a frame.
    pub fn render(&mut self, frame: &mut ratatui::Frame<'_>) {
        self.tick += 1;
        self.toasts.prune();
        self.drain_pending();
        self.drain_pending_create();
        self.rebuild_keymap();

        let size = frame.area();
        self.terminal_size = (size.width, size.height);

        // Background.
        frame.render_widget(
            ratatui::widgets::Block::default()
                .style(ratatui::style::Style::default().bg(self.theme.background)),
            size,
        );

        match &self.route {
            Route::Home => self.render_home(frame),
            Route::Session { id } => self.render_session(frame, id.clone()),
        }

        // Toasts overlay at the top.
        let toast_lines = crate::components::toast::toast_lines(&self.toasts, &self.theme);
        for (idx, line) in toast_lines.iter().enumerate() {
            if idx as u16 >= size.height {
                break;
            }
            let styled_line = to_ratatui(line);
            frame.render_widget(
                ratatui::widgets::Paragraph::new(styled_line)
                    .style(ratatui::style::Style::default().bg(self.theme.background_panel)),
                ratatui::layout::Rect::new(0, idx as u16, size.width.min(80), 1),
            );
        }
    }

    fn drain_pending_create(&mut self) {
        let Some((mut rx, input_text, agent, model, mode, parts)) = self.pending_create.take()
        else {
            return;
        };
        match rx.try_recv() {
            Ok(Some(session_id)) => {
                self.send_to_session(&session_id, &input_text, &agent, &model, mode, parts);
                // Navigate after a short delay so the new session is visible.
                self.navigate_session(&session_id);
            }
            Ok(None) => {
                self.toasts.error("Creating a session failed");
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                self.pending_create = Some((rx, input_text, agent, model, mode, parts));
            }
            Err(_) => {}
        }
    }

    fn render_home(&mut self, frame: &mut ratatui::Frame<'_>) {
        let size = frame.area();
        let theme = &self.theme;
        let width = size.width as usize;

        // Logo.
        let logo = crate::logo::LOGO.lines();
        let logo_height = logo.len() as u16;
        let center_y = size.height.saturating_sub(logo_height + 8) / 2;
        for (idx, line) in logo.iter().enumerate() {
            let spans: Vec<ratatui::text::Span> = line
                .chars()
                .enumerate()
                .map(|(_ci, c)| {
                    let color = match c {
                        '█' => theme.text,
                        '▄' => theme.text_muted,
                        _ => theme.background,
                    };
                    let mut s = String::new();
                    s.push(c);
                    ratatui::text::Span::styled(s, ratatui::style::Style::default().fg(color))
                })
                .collect();
            let x = (width.saturating_sub(line.chars().count() + 30) / 2) as u16;
            frame.render_widget(
                ratatui::widgets::Paragraph::new(ratatui::text::Line::from(spans)),
                ratatui::layout::Rect::new(x, center_y.saturating_add(idx as u16), size.width, 1),
            );
        }

        // Prompt.
        let prompt_width = crate::config::home_prompt_max_width(&self.config, size.width)
            .min(width.saturating_sub(4))
            .max(20) as u16;
        let prompt_x = ((width.saturating_sub(prompt_width as usize)) / 2) as u16;
        let prompt_y = center_y.saturating_add(logo_height).saturating_add(3);
        self.render_prompt_widget(
            frame,
            &self.home_prompt,
            ratatui::layout::Rect::new(
                prompt_x,
                prompt_y,
                prompt_width,
                size.height.saturating_sub(prompt_y).max(3),
            ),
        );
    }

    fn render_session(&mut self, frame: &mut ratatui::Frame<'_>, session_id: String) {
        let size = frame.area();
        let theme = &self.theme;

        // Read view-independent settings first.
        let conceal = self.kv_get_bool("conceal", true);
        let thinking_mode = self.kv_get_str("thinking_mode", "hide");
        let show_timestamps = self.kv_get_bool("timestamps", false);
        let show_details = self.kv_get_bool("tool_details_visibility", true);
        let show_generic = self.kv_get_bool("generic_tool_output_visibility", false);

        // Compute message list from the view + sync. Sidebar defaults to
        // "auto" (visible on wide terminals) until explicitly toggled.
        let wide = size.width > 120;
        let sidebar_visible = {
            let view = self.views.entry(session_id.clone()).or_default();
            if !view.sidebar_init {
                view.sidebar_init = true;
                view.sidebar_visible = wide;
            }
            view.sidebar_visible
        };
        let content_width = (size.width as usize)
            .saturating_sub(if sidebar_visible { 42 } else { 0 })
            .saturating_sub(4);
        let cwd_str = self.cwd.to_string_lossy().to_string();
        let message_lines = {
            let view = self.views.entry(session_id.clone()).or_default();
            let render = crate::components::message::SessionRender {
                width: content_width,
                sync: &self.sync,
                theme,
                conceal,
                thinking_mode: &thinking_mode,
                show_timestamps,
                show_details,
                show_generic_tool_output: show_generic,
                expanded_tools: &view.expanded_tools,
                reasoning_expanded: &view.reasoning_expanded,
                session_id: &session_id,
                cwd: &cwd_str,
            };
            crate::components::message::render_messages(&render)
        };
        self.messages = message_lines;

        // Layout: messages area + prompt area.
        let permissions = self
            .sync
            .permissions
            .get(&session_id)
            .cloned()
            .unwrap_or_default();
        let questions = self
            .sync
            .questions
            .get(&session_id)
            .cloned()
            .unwrap_or_default();
        let subagent = self.route_is_subagent();
        let prompt_height = 6u16;
        let messages_height = size.height.saturating_sub(prompt_height).max(1);

        // Messages scroll view (scoped borrow of the view).
        let messages_rect = ratatui::layout::Rect::new(0, 0, size.width, messages_height);
        let max_scroll = self
            .messages
            .len()
            .saturating_sub(messages_rect.height as usize) as i64;
        let start = {
            let view = self.views.entry(session_id.clone()).or_default();
            if view.sticky_bottom {
                view.scroll = max_scroll.max(0);
            } else {
                view.scroll = view.scroll.clamp(0, max_scroll.max(0));
            }
            view.message_height = messages_rect.height as usize;
            view.scroll.clamp(0, self.messages.len() as i64) as usize
        };

        let mut visible: Vec<crate::components::message::MessageLine> = Vec::new();
        for line in self
            .messages
            .iter()
            .skip(start)
            .take(messages_rect.height as usize)
        {
            visible.push(crate::components::message::MessageLine {
                line: line.line.clone(),
                owner: line.owner.clone(),
            });
        }
        self.draw_message_lines(frame, messages_rect, &visible);

        // Sidebar.
        if sidebar_visible {
            let sidebar_rect =
                ratatui::layout::Rect::new(size.width.saturating_sub(42), 0, 42, size.height);
            self.render_sidebar(frame, sidebar_rect, &session_id);
        }

        // Permission / question / subagent footer / prompt below.
        let prompt_rect = ratatui::layout::Rect::new(
            0,
            messages_height,
            size.width,
            size.height.saturating_sub(messages_height),
        );
        if let Some(request) = permissions.first() {
            self.render_permission_prompt(frame, prompt_rect, request);
        } else if let Some(request) = questions.first() {
            let state = {
                let view = self.views.entry(session_id.clone()).or_default();
                view.question
                    .get_or_insert_with(|| question::QuestionState::new(request.questions.len()))
                    .clone()
            };
            let theme = self.theme.clone();
            let lines = question::render(
                request,
                &state,
                prompt_rect.width as usize,
                prompt_rect.height as usize,
                &theme,
            );
            self.views.entry(session_id.clone()).or_default().question = Some(state);
            self.draw_styled_lines(frame, prompt_rect, &lines);
        } else if subagent {
            self.render_subagent_footer(frame, prompt_rect, &session_id);
        } else {
            if let Some(prompt) = self.session_prompt.as_ref() {
                self.render_prompt_widget(frame, prompt, prompt_rect);
            }
        }
    }

    fn draw_message_lines(
        &self,
        frame: &mut ratatui::Frame<'_>,
        rect: ratatui::layout::Rect,
        lines: &[crate::components::message::MessageLine],
    ) {
        let theme = &self.theme;
        let mut rendered: Vec<ratatui::text::Line<'static>> = Vec::new();
        for line in lines {
            let mut styled: StyledLine = Vec::new();
            for (text, style) in &line.line {
                let mut s = *style;
                if s.bg == Some(ratatui::style::Color::Reset) {
                    s = s.bg(theme.background);
                }
                styled.push((text.clone(), s));
            }
            rendered.push(to_ratatui(&styled));
        }
        let text = ratatui::text::Text::from(rendered);
        frame.render_widget(
            ratatui::widgets::Paragraph::new(text)
                .style(ratatui::style::Style::default().bg(theme.background)),
            rect,
        );
    }

    fn render_prompt_widget(
        &self,
        frame: &mut ratatui::Frame<'_>,
        prompt: &PromptState,
        rect: ratatui::layout::Rect,
    ) {
        let theme = &self.theme;
        let width = rect.width as usize;
        let border_color = if self.leader_active {
            theme.border
        } else if prompt.mode == PromptMode::Shell {
            theme.primary
        } else {
            match self.local.current_agent(&self.sync) {
                Some(agent) => self.theme.agent_color(&agent.name, &self.sync.agents),
                None => theme.border,
            }
        };
        let placeholder = if prompt.mode == PromptMode::Shell {
            "Run a command...".to_string()
        } else {
            "Ask anything...".to_string()
        };
        let (box_lines, cursor) = crate::components::prompt::prompt_lines(
            &prompt.text(),
            width,
            prompt.buffer.cursor(),
            &prompt.parts,
            theme,
            border_color,
            Some(&placeholder),
        );
        let height = box_lines.len().min(rect.height as usize);
        let draw_height = rect.height.min(8);
        let mut displayed: Vec<StyledLine> = Vec::new();
        for line in box_lines.iter().take(draw_height as usize) {
            let mut l = line.clone();
            for (_, s) in l.iter_mut() {
                if s.bg == Some(ratatui::style::Color::Reset) {
                    *s = s.bg(theme.background_element);
                }
            }
            displayed.push(l);
        }
        self.draw_styled_lines(frame, rect, &displayed);

        // Status/hint row below the box.
        let status_y = rect.y.saturating_add(height as u16);
        if status_y < rect.y + rect.height {
            let status = self.prompt_status_row(prompt, width);
            let status_rect = ratatui::layout::Rect::new(rect.x, status_y, width as u16, 1);
            let mut lines: Vec<StyledLine> = Vec::new();
            lines.push(status);
            self.draw_styled_lines(frame, status_rect, &lines);
        }

        // Cursor.
        if let Some((row, col)) = cursor {
            let y = rect.y + row as u16;
            let x = rect.x + col as u16;
            frame.set_cursor_position((x, y));
        }
    }

    fn prompt_status_row(&self, prompt: &PromptState, width: usize) -> StyledLine {
        let theme = &self.theme;
        let mut line: StyledLine = Vec::new();
        let agent = self.local.current_agent(&self.sync);
        let status = self
            .current_session_id()
            .and_then(|id| self.sync.session_status.get(id.as_str()).cloned());
        let busy = status.as_ref().is_some_and(|s| s.kind() != "idle");

        if busy {
            let spinner = crate::components::spinner::frame(self.tick);
            line.push((format!("  {spinner} "), Style::default().fg(theme.text)));
            if let Some(SessionStatus::Retry(retry)) = &status {
                let message = locale::truncate(&retry.message, 80);
                line.push((
                    format!(" [retrying attempt #{}] {message}", retry.attempt),
                    Style::default().fg(theme.error),
                ));
            }
            line.push((
                format!(
                    "  esc {}",
                    if prompt.interrupt > 0 {
                        "again to interrupt"
                    } else {
                        "interrupt"
                    }
                ),
                Style::default().fg(if prompt.interrupt > 0 {
                    theme.primary
                } else {
                    theme.text
                }),
            ));
        } else {
            match &self.route {
                Route::Session { .. } => {
                    if let Some(id) = self.current_session_id() {
                        if let Some(session) = self.sync.session(&id) {
                            line.push((
                                format!("  {}", session.directory),
                                Style::default().fg(theme.text_muted),
                            ));
                        }
                    }
                }
                Route::Home => {}
            }
        }

        let agent_name = match agent {
            Some(agent) => {
                if prompt.mode == PromptMode::Shell {
                    "Shell".to_string()
                } else {
                    locale::titlecase(&agent.name)
                }
            }
            None => String::new(),
        };
        if !agent_name.is_empty() {
            line.push((format!("  {agent_name}"), Style::default().fg(theme.text)));
        }
        let model = self.local.current_model(&self.sync);
        if prompt.mode == PromptMode::Normal {
            if let Some(model) = &model {
                let provider = self
                    .sync
                    .providers
                    .iter()
                    .find(|p| p.id == model.provider_id);
                line.push((" · ".to_string(), Style::default().fg(theme.text_muted)));
                line.push((model.model_id.clone(), Style::default().fg(theme.text)));
                if let Some(provider) = provider {
                    line.push((" ".to_string(), Style::default().fg(theme.text_muted)));
                    line.push((provider.name.clone(), Style::default().fg(theme.text_muted)));
                }
            }
        }
        // Right-aligned hints.
        let agent_shortcut = self
            .config
            .get("agent_cycle")
            .and_then(|b| b.sequences.first())
            .map(|s| {
                s.strokes
                    .first()
                    .map(|st| match st {
                        crate::keymap::Stroke::Leader => crate::keymap::leader_key_name(),
                        crate::keymap::Stroke::Key(k) => k.display(),
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_else(|| "tab".to_string());
        let palette_shortcut = self
            .config
            .get("command_list")
            .and_then(|b| b.sequences.first())
            .map(|s| {
                s.strokes
                    .first()
                    .map(|st| match st {
                        crate::keymap::Stroke::Leader => crate::keymap::leader_key_name(),
                        crate::keymap::Stroke::Key(k) => k.display(),
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_else(|| "ctrl+p".to_string());
        let hint = if prompt.mode == PromptMode::Shell {
            format!("  esc exit shell mode")
        } else {
            format!("  {agent_shortcut} agents  {palette_shortcut} commands")
        };
        let hint_len = hint.chars().count();
        let used: usize = line.iter().map(|(s, _)| s.chars().count()).sum();
        if used.saturating_add(hint_len) < width {
            line.push((
                " ".repeat(width.saturating_sub(used + hint_len)),
                Style::default(),
            ));
        }
        line.push((hint, Style::default().fg(theme.text)));
        line.push(("  ".to_string(), Style::default()));
        line
    }

    fn render_permission_prompt(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        rect: ratatui::layout::Rect,
        request: &crate::types::PermissionRequest,
    ) {
        let id = request.session_id.clone();
        let cwd = self.cwd.to_string_lossy().to_string();
        let theme = self.theme.clone();
        let lines = {
            let view = self.views.entry(id.clone()).or_default();
            let state = view.permission.get_or_insert_with(PermissionState::default);
            permission::render(
                request,
                state,
                &cwd,
                rect.width as usize,
                rect.height as usize,
                &theme,
            )
        };
        self.draw_styled_lines(frame, rect, &lines);
    }

    fn render_subagent_footer(
        &mut self,
        frame: &mut ratatui::Frame<'_>,
        rect: ratatui::layout::Rect,
        session_id: &str,
    ) {
        let theme = &self.theme;
        let session = self.sync.session(session_id);
        let label = match session.map(|s| s.title.clone()) {
            Some(title) => {
                let mut agent = "Subagent".to_string();
                if let Some(idx) = title.find(" subagent") {
                    let name = &title[..idx];
                    agent = locale::titlecase(name.trim_start_matches('@'));
                }
                agent
            }
            None => "Subagent".to_string(),
        };
        let mut line: StyledLine = vec![
            ("  ".to_string(), Style::default()),
            (label, Style::default().fg(theme.text)),
        ];
        line.push((
            "  Parent ↑  Prev ←  Next →".to_string(),
            Style::default().fg(theme.text_muted),
        ));
        let mut lines = Vec::new();
        lines.push(line);
        self.draw_styled_lines(frame, rect, &lines);
    }

    fn draw_styled_lines(
        &self,
        frame: &mut ratatui::Frame<'_>,
        rect: ratatui::layout::Rect,
        lines: &[StyledLine],
    ) {
        let theme = &self.theme;
        let rendered: Vec<ratatui::text::Line<'static>> = lines
            .iter()
            .map(|l| {
                let mut styled: StyledLine = Vec::new();
                for (text, style) in l {
                    let mut s = *style;
                    if s.bg == Some(ratatui::style::Color::Reset) {
                        s = s.bg(theme.background_element);
                    }
                    styled.push((text.clone(), s));
                }
                to_ratatui(&styled)
            })
            .collect();
        frame.render_widget(
            ratatui::widgets::Paragraph::new(ratatui::text::Text::from(rendered))
                .style(ratatui::style::Style::default().bg(theme.background_element)),
            rect,
        );
    }
}

/// Paste text into a prompt, summarizing large pastes as a tracked part.
/// From reference/packages/tui/src/component/prompt/index.tsx (`pasteInputText`)
pub fn paste_into_prompt(prompt: &mut PromptState, text: &str, summary_enabled: bool) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let pasted = normalized.trim();
    let line_count = normalized.matches('\n').count() + 1;
    if summary_enabled && (line_count >= 3 || pasted.chars().count() > 150) {
        let part = serde_json::json!({
            "type": "text",
            "text": pasted,
            "source": { "text": { "value": "", "start": 0, "end": 0 } }
        });
        let marker = format!("[Pasted ~{line_count} lines]");
        prompt.insert_part(&marker, part);
    } else {
        prompt.buffer.insert_str(&normalized);
    }
}

// ---- bootstrap + run ---------------------------------------------------------

/// Bootstrap data in parallel, mirroring `sync.tsx` `bootstrap`.
async fn bootstrap(client: Arc<dyn SdkClient>) -> BootstrapData {
    let providers_fut = client.config_providers();
    let agent_fut = client.app_agents();
    let config_fut = client.config_get();
    let command_fut = client.command_list();
    let sessions_fut = client.session_list();
    let caps_fut = client.experimental_capabilities();
    let console_fut = client.experimental_console();
    let status_fut = client.session_status();

    let (providers, agents, config, commands, sessions, caps, console, status) = tokio::join!(
        providers_fut,
        agent_fut,
        config_fut,
        command_fut,
        sessions_fut,
        caps_fut,
        console_fut,
        status_fut,
    );
    BootstrapData {
        providers: providers.unwrap_or_default().providers,
        agents: agents.unwrap_or_default(),
        commands: commands.unwrap_or_default(),
        config: config.unwrap_or_default(),
        sessions: sessions.unwrap_or_default(),
        capabilities: caps.unwrap_or_default(),
        console_state: console.unwrap_or_default(),
        session_status: status.unwrap_or_default(),
    }
}

/// Run the TUI against a server. Blocks until exit.
pub async fn run_async(input: TuiInput) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_with_terminal(&mut terminal, input).await;

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn run_with_terminal<W: std::io::Write>(
    terminal: &mut Terminal<CrosstermBackend<W>>,
    input: TuiInput,
) -> anyhow::Result<()> {
    let directory = input.directory.clone();
    let url = input.url.clone();
    let workspace = input.workspace.clone();
    let client: Arc<dyn SdkClient> = Arc::new(crate::client::HttpSdkClient::new(
        crate::client::ClientConfig {
            url: url.clone(),
            directory: directory.clone(),
            workspace: workspace.clone(),
        },
    )?);

    let (tx, mut rx) = mpsc::channel::<ClientMessage>(256);

    // Event stream task.
    {
        let client = client.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let stream = match client.subscribe_events() {
                Ok(stream) => stream,
                Err(error) => {
                    tracing::warn!(%error, "failed to subscribe to events");
                    return;
                }
            };
            let mut stream = stream;
            while let Some(event) = stream.next().await {
                if tx.send(ClientMessage::Event(event)).await.is_err() {
                    break;
                }
            }
        });
    }

    // Bootstrap task.
    {
        let client = client.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let data = bootstrap(client).await;
            let _ = tx.send(ClientMessage::Bootstrap(data)).await;
        });
    }

    let mut app = App::new(input, client);

    // Auto-navigate to --session / --continue.
    if let Some(session_id) = app.initial_session_id.clone() {
        app.navigate_session(&session_id);
    }
    if let Some(agent) = app.initial_agent.clone() {
        app.local.set_agent(&agent);
    }
    if let Some(model) = app.initial_model.clone() {
        if let Some((provider, model_id)) = parse_model_arg(&model) {
            app.local.set_model(
                crate::local::ModelSelection {
                    provider_id: provider,
                    model_id,
                    variant: None,
                },
                true,
            );
        }
    }

    // Enable mouse capture.
    if app.config.mouse {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
    }
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste);

    let mut last_draw = Instant::now();
    let mut running = true;
    while running {
        // Drain client messages.
        while let Ok(msg) = rx.try_recv() {
            app.handle_client_message(msg);
            app.dirty = true;
        }

        // Auto-submit --prompt once the server is ready.
        if app.prompt_ready && !app.autosubmitted {
            if let Some(prompt_text) = app.initial_prompt.clone() {
                if app.route == Route::Home && app.home_prompt.text().is_empty() {
                    app.home_prompt.buffer.set_text(&prompt_text);
                    app.home_prompt.buffer.buffer_end();
                    app.autosubmitted = true;
                    app.submit_prompt();
                }
            }
        }

        // Poll terminal input (blocking up to 16ms).
        if crossterm::event::poll(Duration::from_millis(16))? {
            loop {
                match crossterm::event::read() {
                    Ok(event) => {
                        let redraw = app.handle_input(event);
                        if redraw {
                            app.dirty = true;
                        }
                    }
                    Err(_) => break,
                }
                if !crossterm::event::poll(Duration::from_millis(0))? {
                    break;
                }
            }
        }

        // Redraw on state changes or at a regular tick (spinner animation).
        if app.dirty || last_draw.elapsed() > Duration::from_millis(80) {
            terminal.draw(|frame| app.render(frame))?;
            app.dirty = false;
            last_draw = Instant::now();
        }

        if app.exiting {
            running = false;
        }
    }

    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    Ok(())
}

fn parse_model_arg(input: &str) -> Option<(String, String)> {
    let mut parts = input.splitn(2, '/');
    let provider = parts.next()?;
    let model = parts.next()?;
    if provider.is_empty() || model.is_empty() {
        None
    } else {
        Some((provider.to_string(), model.to_string()))
    }
}

// ---- sidebar -----------------------------------------------------------------

impl App {
    /// Render the session sidebar (title + todo list).
    /// From reference/packages/tui/src/routes/session/sidebar.tsx
    fn render_sidebar(
        &self,
        frame: &mut ratatui::Frame<'_>,
        rect: ratatui::layout::Rect,
        session_id: &str,
    ) {
        let theme = &self.theme;
        let mut lines: Vec<StyledLine> = Vec::new();
        if let Some(session) = self.sync.session(session_id) {
            let title = locale::truncate(&session.title, 38);
            lines.push(vec![(
                title,
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            )]);
            if let Some(workspace_id) = &session.workspace_id {
                lines.push(vec![(
                    format!("  {workspace_id}"),
                    Style::default().fg(theme.text_muted),
                )]);
            }
        }
        lines.push(vec![("".to_string(), Style::default())]);
        let todos = self.sync.todos.get(session_id).cloned().unwrap_or_default();
        if todos.is_empty() {
            lines.push(vec![(
                "  No todos".to_string(),
                Style::default().fg(theme.text_muted),
            )]);
        }
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
            let mut line: StyledLine = vec![
                ("  ".to_string(), Style::default()),
                (format!("[{mark}] "), Style::default().fg(fg)),
            ];
            let content = locale::truncate(&todo.content, 34);
            line.push((content, Style::default().fg(fg)));
            lines.push(line);
        }
        lines.push(vec![("".to_string(), Style::default())]);
        lines.push(vec![
            ("  • Open".to_string(), Style::default().fg(theme.success)),
            ("Code ".to_string(), Style::default().fg(theme.text)),
            (
                crate::version().to_string(),
                Style::default().fg(theme.text_muted),
            ),
        ]);
        let mut rendered: Vec<ratatui::text::Line<'static>> = Vec::new();
        for line in lines {
            rendered.push(to_ratatui(&pad_line(
                line,
                rect.width as usize,
                theme.background_panel,
            )));
        }
        frame.render_widget(
            ratatui::widgets::Paragraph::new(ratatui::text::Text::from(rendered))
                .style(ratatui::style::Style::default().bg(theme.background_panel)),
            rect,
        );
    }
}

fn pad_line(mut line: StyledLine, width: usize, bg: Color) -> StyledLine {
    let used: usize = line.iter().map(|(s, _)| s.chars().count()).sum();
    if used < width {
        line.push((" ".repeat(width - used), Style::default().bg(bg)));
    }
    line
}
