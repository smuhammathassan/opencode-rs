//! TUI keybinding definitions.
//! From reference/packages/tui/src/config/keybind.ts (Definitions + CommandMap)

/// Default leader key.
/// From reference/packages/tui/src/config/keybind.ts (`LeaderDefault`)
pub const LEADER_DEFAULT: &str = "ctrl+x";
pub const LEADER_TIMEOUT_DEFAULT: u64 = 2000;

#[derive(Debug, Clone)]
pub struct KeybindDef {
    pub name: &'static str,
    pub default: &'static str,
    pub desc: &'static str,
    pub command: &'static str,
}

macro_rules! keybind {
    ($name:literal, $default:expr, $desc:literal, $cmd:literal) => {
        KeybindDef {
            name: $name,
            default: $default,
            desc: $desc,
            command: $cmd,
        }
    };
}

pub const DEFINITIONS: &[KeybindDef] = &[
    keybind!(
        "leader",
        LEADER_DEFAULT,
        "Leader key for keybind combinations",
        ""
    ),
    keybind!(
        "app_exit",
        "ctrl+c,ctrl+d,<leader>q",
        "Exit the application",
        "app.exit"
    ),
    keybind!("app_debug", "none", "Toggle debug panel", "app.debug"),
    keybind!("app_console", "none", "Toggle console", "app.console"),
    keybind!(
        "app_heap_snapshot",
        "none",
        "Write heap snapshot",
        "app.heap_snapshot"
    ),
    keybind!(
        "app_toggle_animations",
        "none",
        "Toggle animations",
        "app.toggle.animations"
    ),
    keybind!(
        "app_toggle_file_context",
        "none",
        "Toggle file context",
        "app.toggle.file_context"
    ),
    keybind!(
        "app_toggle_diffwrap",
        "none",
        "Toggle diff wrapping",
        "app.toggle.diffwrap"
    ),
    keybind!(
        "app_toggle_paste_summary",
        "none",
        "Toggle paste summary",
        "app.toggle.paste_summary"
    ),
    keybind!(
        "app_toggle_session_directory_filter",
        "none",
        "Toggle session directory filtering",
        "app.toggle.session_directory_filter"
    ),
    keybind!(
        "command_list",
        "ctrl+p",
        "List available commands",
        "command.palette.show"
    ),
    keybind!("help_show", "none", "Open help dialog", "help.show"),
    keybind!("docs_open", "none", "Open documentation", "docs.open"),
    keybind!("diff_open", "none", "Open diff viewer", "diff.open"),
    keybind!("diff_close", "escape,q", "Close diff viewer", "diff.close"),
    keybind!(
        "diff_toggle",
        "enter,space",
        "Toggle diff viewer item",
        "diff.toggle"
    ),
    keybind!(
        "diff_expand",
        "right",
        "Expand diff viewer item",
        "diff.expand"
    ),
    keybind!(
        "diff_expand_all",
        "E",
        "Expand all diff viewer folders",
        "diff.expand_all"
    ),
    keybind!(
        "diff_collapse",
        "left",
        "Collapse diff viewer item",
        "diff.collapse"
    ),
    keybind!(
        "diff_switch_focus",
        "tab",
        "Switch diff viewer focus",
        "diff.switch_focus"
    ),
    keybind!(
        "diff_next_hunk",
        "]",
        "Jump to next diff hunk",
        "diff.next_hunk"
    ),
    keybind!(
        "diff_previous_hunk",
        "[",
        "Jump to previous diff hunk",
        "diff.previous_hunk"
    ),
    keybind!(
        "diff_next_file",
        "n",
        "Jump to next diff file",
        "diff.next_file"
    ),
    keybind!(
        "diff_previous_file",
        "p",
        "Jump to previous diff file",
        "diff.previous_file"
    ),
    keybind!(
        "diff_toggle_file_tree",
        "b",
        "Toggle diff viewer file tree",
        "diff.toggle_file_tree"
    ),
    keybind!(
        "diff_single_patch",
        "s",
        "Toggle single patch view",
        "diff.single_patch"
    ),
    keybind!(
        "diff_switch_source",
        "d",
        "Switch diff viewer source",
        "diff.switch_source"
    ),
    keybind!(
        "diff_toggle_view",
        "v",
        "Toggle diff viewer split or unified view",
        "diff.toggle_view"
    ),
    keybind!(
        "diff_help",
        "?",
        "Show more diff viewer shortcuts",
        "diff.help"
    ),
    keybind!(
        "editor_open",
        "<leader>e",
        "Open external editor",
        "prompt.editor"
    ),
    keybind!(
        "theme_list",
        "<leader>t",
        "List available themes",
        "theme.switch"
    ),
    keybind!(
        "theme_switch_mode",
        "none",
        "Switch between light and dark theme mode",
        "theme.switch_mode"
    ),
    keybind!(
        "theme_mode_lock",
        "none",
        "Lock or unlock theme mode",
        "theme.mode.lock"
    ),
    keybind!(
        "sidebar_toggle",
        "<leader>b",
        "Toggle sidebar",
        "session.sidebar.toggle"
    ),
    keybind!(
        "scrollbar_toggle",
        "none",
        "Toggle session scrollbar",
        "session.toggle.scrollbar"
    ),
    keybind!("status_view", "<leader>s", "View status", "opencode.status"),
    keybind!("debug_view", "none", "View debug info", "opencode.debug"),
    keybind!(
        "session_export",
        "<leader>x",
        "Export session to editor",
        "session.export"
    ),
    keybind!(
        "session_copy",
        "none",
        "Copy session transcript",
        "session.copy"
    ),
    keybind!("session_move", "none", "Move session", "session.move"),
    keybind!(
        "session_new",
        "<leader>n",
        "Create a new session",
        "session.new"
    ),
    keybind!(
        "session_list",
        "<leader>l",
        "List all sessions",
        "session.list"
    ),
    keybind!(
        "session_timeline",
        "<leader>g",
        "Show session timeline",
        "session.timeline"
    ),
    keybind!(
        "session_fork",
        "none",
        "Fork session from message",
        "session.fork"
    ),
    keybind!(
        "session_rename",
        "ctrl+r",
        "Rename session",
        "session.rename"
    ),
    keybind!(
        "session_delete",
        "ctrl+d",
        "Delete session",
        "session.delete"
    ),
    keybind!(
        "session_share",
        "none",
        "Share current session",
        "session.share"
    ),
    keybind!(
        "session_unshare",
        "none",
        "Unshare current session",
        "session.unshare"
    ),
    keybind!(
        "session_interrupt",
        "escape",
        "Interrupt current session",
        "session.interrupt"
    ),
    keybind!(
        "session_background",
        "ctrl+b",
        "Background synchronous subagents",
        "session.background"
    ),
    keybind!(
        "session_compact",
        "<leader>c",
        "Compact the session",
        "session.compact"
    ),
    keybind!(
        "session_toggle_timestamps",
        "none",
        "Toggle message timestamps",
        "session.toggle.timestamps"
    ),
    keybind!(
        "session_toggle_generic_tool_output",
        "none",
        "Toggle generic tool output",
        "session.toggle.generic_tool_output"
    ),
    keybind!(
        "session_queued_prompts",
        "<leader>q",
        "Manage queued prompts",
        "session.queued_prompts"
    ),
    keybind!(
        "session_child_first",
        "<leader>down",
        "Go to first child session",
        "session.child.first"
    ),
    keybind!(
        "session_child_cycle",
        "right",
        "Go to next child session",
        "session.child.next"
    ),
    keybind!(
        "session_child_cycle_reverse",
        "left",
        "Go to previous child session",
        "session.child.previous"
    ),
    keybind!(
        "session_parent",
        "up",
        "Go to parent session",
        "session.parent"
    ),
    keybind!(
        "session_pin_toggle",
        "ctrl+f",
        "Pin or unpin session in the session list",
        "session.pin.toggle"
    ),
    keybind!(
        "session_quick_switch_1",
        "<leader>1",
        "Switch to session in quick slot 1",
        "session.quick_switch.1"
    ),
    keybind!(
        "session_quick_switch_2",
        "<leader>2",
        "Switch to session in quick slot 2",
        "session.quick_switch.2"
    ),
    keybind!(
        "session_quick_switch_3",
        "<leader>3",
        "Switch to session in quick slot 3",
        "session.quick_switch.3"
    ),
    keybind!(
        "session_quick_switch_4",
        "<leader>4",
        "Switch to session in quick slot 4",
        "session.quick_switch.4"
    ),
    keybind!(
        "session_quick_switch_5",
        "<leader>5",
        "Switch to session in quick slot 5",
        "session.quick_switch.5"
    ),
    keybind!(
        "session_quick_switch_6",
        "<leader>6",
        "Switch to session in quick slot 6",
        "session.quick_switch.6"
    ),
    keybind!(
        "session_quick_switch_7",
        "<leader>7",
        "Switch to session in quick slot 7",
        "session.quick_switch.7"
    ),
    keybind!(
        "session_quick_switch_8",
        "<leader>8",
        "Switch to session in quick slot 8",
        "session.quick_switch.8"
    ),
    keybind!(
        "session_quick_switch_9",
        "<leader>9",
        "Switch to session in quick slot 9",
        "session.quick_switch.9"
    ),
    keybind!(
        "stash_delete",
        "ctrl+d",
        "Delete stash entry",
        "stash.delete"
    ),
    keybind!(
        "model_provider_list",
        "ctrl+a",
        "Open provider list from model dialog",
        "model.dialog.provider"
    ),
    keybind!(
        "model_favorite_toggle",
        "ctrl+f",
        "Toggle model favorite status",
        "model.dialog.favorite"
    ),
    keybind!(
        "model_list",
        "<leader>m",
        "List available models",
        "model.list"
    ),
    keybind!(
        "model_cycle_recent",
        "f2",
        "Next recently used model",
        "model.cycle_recent"
    ),
    keybind!(
        "model_cycle_recent_reverse",
        "shift+f2",
        "Previous recently used model",
        "model.cycle_recent_reverse"
    ),
    keybind!(
        "model_cycle_favorite",
        "none",
        "Next favorite model",
        "model.cycle_favorite"
    ),
    keybind!(
        "model_cycle_favorite_reverse",
        "none",
        "Previous favorite model",
        "model.cycle_favorite_reverse"
    ),
    keybind!("mcp_list", "none", "List MCP servers", "mcp.list"),
    keybind!(
        "provider_connect",
        "none",
        "Connect provider",
        "provider.connect"
    ),
    keybind!(
        "console_org_switch",
        "none",
        "Switch console organization",
        "console.org.switch"
    ),
    keybind!("agent_list", "<leader>a", "List agents", "agent.list"),
    keybind!("agent_cycle", "tab", "Next agent", "agent.cycle"),
    keybind!(
        "agent_cycle_reverse",
        "shift+tab",
        "Previous agent",
        "agent.cycle.reverse"
    ),
    keybind!(
        "variant_cycle",
        "ctrl+t",
        "Cycle model variants",
        "variant.cycle"
    ),
    keybind!(
        "variant_list",
        "none",
        "List model variants",
        "variant.list"
    ),
    keybind!(
        "messages_page_up",
        "pageup,ctrl+alt+b",
        "Scroll messages up by one page",
        "session.page.up"
    ),
    keybind!(
        "messages_page_down",
        "pagedown,ctrl+alt+f",
        "Scroll messages down by one page",
        "session.page.down"
    ),
    keybind!(
        "messages_line_up",
        "ctrl+alt+y",
        "Scroll messages up by one line",
        "session.line.up"
    ),
    keybind!(
        "messages_line_down",
        "ctrl+alt+e",
        "Scroll messages down by one line",
        "session.line.down"
    ),
    keybind!(
        "messages_half_page_up",
        "ctrl+alt+u",
        "Scroll messages up by half page",
        "session.half.page.up"
    ),
    keybind!(
        "messages_half_page_down",
        "ctrl+alt+d",
        "Scroll messages down by half page",
        "session.half.page.down"
    ),
    keybind!(
        "messages_first",
        "ctrl+g,home",
        "Navigate to first message",
        "session.first"
    ),
    keybind!(
        "messages_last",
        "ctrl+alt+g,end",
        "Navigate to last message",
        "session.last"
    ),
    keybind!(
        "messages_next",
        "none",
        "Navigate to next message",
        "session.message.next"
    ),
    keybind!(
        "messages_previous",
        "none",
        "Navigate to previous message",
        "session.message.previous"
    ),
    keybind!(
        "messages_last_user",
        "none",
        "Navigate to last user message",
        "session.messages_last_user"
    ),
    keybind!(
        "messages_copy",
        "<leader>y",
        "Copy message",
        "messages.copy"
    ),
    keybind!("messages_undo", "<leader>u", "Undo message", "session.undo"),
    keybind!("messages_redo", "<leader>r", "Redo message", "session.redo"),
    keybind!(
        "messages_toggle_conceal",
        "<leader>h",
        "Toggle code block concealment in messages",
        "session.toggle.conceal"
    ),
    keybind!(
        "tool_details",
        "none",
        "Toggle tool details visibility",
        "session.toggle.actions"
    ),
    keybind!(
        "display_thinking",
        "none",
        "Toggle thinking blocks visibility",
        "session.toggle.thinking"
    ),
    keybind!("prompt_submit", "none", "Submit prompt", "prompt.submit"),
    keybind!(
        "prompt_editor_context_clear",
        "none",
        "Clear editor context",
        "prompt.editor_context.clear"
    ),
    keybind!(
        "prompt_skills",
        "none",
        "Open skill selector",
        "prompt.skills"
    ),
    keybind!("prompt_stash", "none", "Stash prompt", "prompt.stash"),
    keybind!(
        "prompt_stash_pop",
        "none",
        "Pop stashed prompt",
        "prompt.stash.pop"
    ),
    keybind!(
        "prompt_stash_list",
        "none",
        "List stashed prompts",
        "prompt.stash.list"
    ),
    keybind!("workspace_set", "none", "Set workspace", "workspace.set"),
    keybind!("input_clear", "ctrl+c", "Clear input field", "prompt.clear"),
    keybind!(
        "input_paste",
        "{key:ctrl+v}",
        "Paste from clipboard",
        "prompt.paste"
    ),
    keybind!("input_submit", "return", "Submit input", "input.submit"),
    keybind!(
        "input_newline",
        "shift+return,ctrl+return,alt+return,ctrl+j",
        "Insert newline in input",
        "input.newline"
    ),
    keybind!(
        "input_move_left",
        "left,ctrl+b",
        "Move cursor left in input",
        "input.move.left"
    ),
    keybind!(
        "input_move_right",
        "right,ctrl+f",
        "Move cursor right in input",
        "input.move.right"
    ),
    keybind!(
        "input_move_up",
        "up",
        "Move cursor up in input",
        "input.move.up"
    ),
    keybind!(
        "input_move_down",
        "down",
        "Move cursor down in input",
        "input.move.down"
    ),
    keybind!(
        "input_select_left",
        "shift+left",
        "Select left in input",
        "input.select.left"
    ),
    keybind!(
        "input_select_right",
        "shift+right",
        "Select right in input",
        "input.select.right"
    ),
    keybind!(
        "input_select_up",
        "shift+up",
        "Select up in input",
        "input.select.up"
    ),
    keybind!(
        "input_select_down",
        "shift+down",
        "Select down in input",
        "input.select.down"
    ),
    keybind!(
        "input_line_home",
        "ctrl+a",
        "Move to start of line in input",
        "input.line.home"
    ),
    keybind!(
        "input_line_end",
        "ctrl+e",
        "Move to end of line in input",
        "input.line.end"
    ),
    keybind!(
        "input_select_line_home",
        "ctrl+shift+a",
        "Select to start of line in input",
        "input.select.line.home"
    ),
    keybind!(
        "input_select_line_end",
        "ctrl+shift+e",
        "Select to end of line in input",
        "input.select.line.end"
    ),
    keybind!(
        "input_visual_line_home",
        "alt+a",
        "Move to start of visual line in input",
        "input.visual.line.home"
    ),
    keybind!(
        "input_visual_line_end",
        "alt+e",
        "Move to end of visual line in input",
        "input.visual.line.end"
    ),
    keybind!(
        "input_select_visual_line_home",
        "alt+shift+a",
        "Select to start of visual line in input",
        "input.select.visual.line.home"
    ),
    keybind!(
        "input_select_visual_line_end",
        "alt+shift+e",
        "Select to end of visual line in input",
        "input.select.visual.line.end"
    ),
    keybind!(
        "input_buffer_home",
        "home",
        "Move to start of buffer in input",
        "input.buffer.home"
    ),
    keybind!(
        "input_buffer_end",
        "end",
        "Move to end of buffer in input",
        "input.buffer.end"
    ),
    keybind!(
        "input_select_buffer_home",
        "shift+home",
        "Select to start of buffer in input",
        "input.select.buffer.home"
    ),
    keybind!(
        "input_select_buffer_end",
        "shift+end",
        "Select to end of buffer in input",
        "input.select.buffer.end"
    ),
    keybind!(
        "input_delete_line",
        "ctrl+shift+d",
        "Delete line in input",
        "input.delete.line"
    ),
    keybind!(
        "input_delete_to_line_end",
        "ctrl+k",
        "Delete to end of line in input",
        "input.delete.to.line.end"
    ),
    keybind!(
        "input_delete_to_line_start",
        "ctrl+u",
        "Delete to start of line in input",
        "input.delete.to.line.start"
    ),
    keybind!(
        "input_backspace",
        "backspace,shift+backspace",
        "Backspace in input",
        "input.backspace"
    ),
    keybind!(
        "input_delete",
        "ctrl+d,delete,shift+delete",
        "Delete character in input",
        "input.delete"
    ),
    keybind!(
        "input_undo",
        "ctrl+-,super+z",
        "Undo in input",
        "input.undo"
    ),
    keybind!(
        "input_redo",
        "ctrl+.,super+shift+z",
        "Redo in input",
        "input.redo"
    ),
    keybind!(
        "input_word_forward",
        "alt+f,alt+right,ctrl+right",
        "Move word forward in input",
        "input.word.forward"
    ),
    keybind!(
        "input_word_backward",
        "alt+b,alt+left,ctrl+left",
        "Move word backward in input",
        "input.word.backward"
    ),
    keybind!(
        "input_select_word_forward",
        "alt+shift+f,alt+shift+right",
        "Select word forward in input",
        "input.select.word.forward"
    ),
    keybind!(
        "input_select_word_backward",
        "alt+shift+b,alt+shift+left",
        "Select word backward in input",
        "input.select.word.backward"
    ),
    keybind!(
        "input_delete_word_forward",
        "alt+d,alt+delete,ctrl+delete",
        "Delete word forward in input",
        "input.delete.word.forward"
    ),
    keybind!(
        "input_delete_word_backward",
        "ctrl+w,ctrl+backspace,alt+backspace",
        "Delete word backward in input",
        "input.delete.word.backward"
    ),
    keybind!(
        "input_select_all",
        "super+a",
        "Select all in input",
        "input.select.all"
    ),
    keybind!(
        "history_previous",
        "up",
        "Previous history item",
        "prompt.history.previous"
    ),
    keybind!(
        "history_next",
        "down",
        "Next history item",
        "prompt.history.next"
    ),
    keybind!(
        "dialog.select.prev",
        "up,ctrl+p",
        "Move to previous dialog item",
        "dialog.select.prev"
    ),
    keybind!(
        "dialog.select.next",
        "down,ctrl+n",
        "Move to next dialog item",
        "dialog.select.next"
    ),
    keybind!(
        "dialog.select.page_up",
        "pageup",
        "Move up one page in dialog",
        "dialog.select.page_up"
    ),
    keybind!(
        "dialog.select.page_down",
        "pagedown",
        "Move down one page in dialog",
        "dialog.select.page_down"
    ),
    keybind!(
        "dialog.select.home",
        "home",
        "Move to first dialog item",
        "dialog.select.home"
    ),
    keybind!(
        "dialog.select.end",
        "end",
        "Move to last dialog item",
        "dialog.select.end"
    ),
    keybind!(
        "dialog.select.submit",
        "return",
        "Submit selected dialog item",
        "dialog.select.submit"
    ),
    keybind!(
        "dialog.prompt.submit",
        "return",
        "Submit dialog prompt",
        "dialog.prompt.submit"
    ),
    keybind!(
        "dialog.mcp.toggle",
        "space",
        "Toggle MCP in MCP dialog",
        "dialog.mcp.toggle"
    ),
    keybind!(
        "dialog.move_session.new",
        "ctrl+m",
        "New project copy",
        "dialog.move_session.new"
    ),
    keybind!(
        "dialog.move_session.delete",
        "ctrl+d",
        "Delete project copy",
        "dialog.move_session.delete"
    ),
    keybind!(
        "dialog.move_session.refresh",
        "ctrl+r",
        "Refresh project copies",
        "dialog.move_session.refresh"
    ),
    keybind!(
        "prompt.autocomplete.prev",
        "up,ctrl+p",
        "Move to previous autocomplete item",
        "prompt.autocomplete.prev"
    ),
    keybind!(
        "prompt.autocomplete.next",
        "down,ctrl+n",
        "Move to next autocomplete item",
        "prompt.autocomplete.next"
    ),
    keybind!(
        "prompt.autocomplete.hide",
        "escape",
        "Hide autocomplete",
        "prompt.autocomplete.hide"
    ),
    keybind!(
        "prompt.autocomplete.select",
        "return",
        "Select autocomplete item",
        "prompt.autocomplete.select"
    ),
    keybind!(
        "prompt.autocomplete.complete",
        "tab",
        "Complete autocomplete item",
        "prompt.autocomplete.complete"
    ),
    keybind!(
        "permission.prompt.fullscreen",
        "ctrl+f",
        "Toggle permission prompt fullscreen",
        "permission.prompt.fullscreen"
    ),
    keybind!("plugins.toggle", "space", "Toggle plugin", "plugins.toggle"),
    keybind!(
        "dialog.plugins.install",
        "shift+i",
        "Install plugin from plugin dialog",
        "dialog.plugins.install"
    ),
    keybind!(
        "terminal_suspend",
        "ctrl+z",
        "Suspend terminal",
        "terminal.suspend"
    ),
    keybind!(
        "terminal_title_toggle",
        "none",
        "Toggle terminal title",
        "terminal.title.toggle"
    ),
    keybind!(
        "tips_toggle",
        "<leader>h",
        "Toggle tips on home screen",
        "tips.toggle"
    ),
    keybind!(
        "plugin_manager",
        "none",
        "Open plugin manager dialog",
        "plugins.list"
    ),
    keybind!(
        "plugin_install",
        "none",
        "Install plugin",
        "plugins.install"
    ),
    keybind!(
        "which_key_toggle",
        "ctrl+alt+k",
        "Toggle which-key panel",
        "which-key.toggle"
    ),
    keybind!(
        "which_key_layout_toggle",
        "ctrl+alt+shift+k",
        "Switch which-key layout",
        "which-key.layout.toggle"
    ),
    keybind!(
        "which_key_pending_toggle",
        "ctrl+alt+shift+p",
        "Toggle which-key pending preview",
        "which-key.pending.toggle"
    ),
    keybind!(
        "which_key_group_previous",
        "ctrl+alt+left,ctrl+alt+[",
        "Previous which-key group",
        "which-key.group.previous"
    ),
    keybind!(
        "which_key_group_next",
        "ctrl+alt+right,ctrl+alt+]",
        "Next which-key group",
        "which-key.group.next"
    ),
    keybind!(
        "which_key_scroll_up",
        "ctrl+alt+up,ctrl+alt+p",
        "Scroll which-key up",
        "which-key.scroll.up"
    ),
    keybind!(
        "which_key_scroll_down",
        "ctrl+alt+down,ctrl+alt+n",
        "Scroll which-key down",
        "which-key.scroll.down"
    ),
    keybind!(
        "which_key_page_up",
        "ctrl+alt+pageup",
        "Page which-key up",
        "which-key.page.up"
    ),
    keybind!(
        "which_key_page_down",
        "ctrl+alt+pagedown",
        "Page which-key down",
        "which-key.page.down"
    ),
    keybind!(
        "which_key_home",
        "ctrl+alt+home",
        "Jump to first which-key binding",
        "which-key.home"
    ),
    keybind!(
        "which_key_end",
        "ctrl+alt+end",
        "Jump to last which-key binding",
        "which-key.end"
    ),
];

pub fn definitions() -> &'static [KeybindDef] {
    DEFINITIONS
}

pub fn command_for_name(name: &str) -> Option<&'static str> {
    DEFINITIONS
        .iter()
        .find(|d| d.name == name)
        .filter(|d| !d.command.is_empty())
        .map(|d| d.command)
}

/// Reverse mapping: the keybind name that dispatches `command`.
/// `rebuild_keymap` groups bindings by command and looks the keybind up by its
/// underscore name, so a command must be resolved to its name first.
pub fn name_for_command(command: &str) -> Option<&'static str> {
    DEFINITIONS
        .iter()
        .filter(|d| !d.command.is_empty())
        .find(|d| d.command == command)
        .map(|d| d.name)
}

pub fn desc_for_command(command: &str) -> Option<&'static str> {
    DEFINITIONS
        .iter()
        .find(|d| d.command == command)
        .map(|d| d.desc)
}

/// Known keybind names for validating config overrides.
/// From reference/packages/tui/src/config/keybind.ts (`unknownKeys`)
pub fn unknown_keys(input: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    input
        .keys()
        .filter(|k| !DEFINITIONS.iter().any(|d| d.name == *k))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_have_unique_names_and_commands() {
        let mut names = std::collections::HashSet::new();
        let mut commands = std::collections::HashSet::new();
        for d in DEFINITIONS {
            assert!(names.insert(d.name), "duplicate name {}", d.name);
            if !d.command.is_empty() {
                assert!(
                    commands.insert(d.command),
                    "duplicate command {}",
                    d.command
                );
            }
        }
    }

    #[test]
    fn key_defaults_present() {
        assert_eq!(
            DEFINITIONS
                .iter()
                .find(|d| d.name == "leader")
                .unwrap()
                .default,
            LEADER_DEFAULT
        );
        assert_eq!(
            DEFINITIONS
                .iter()
                .find(|d| d.name == "command_list")
                .unwrap()
                .default,
            "ctrl+p"
        );
        assert_eq!(
            DEFINITIONS
                .iter()
                .find(|d| d.name == "session_interrupt")
                .unwrap()
                .default,
            "escape"
        );
    }

    #[test]
    fn command_map_roundtrips() {
        assert_eq!(command_for_name("model_list"), Some("model.list"));
        assert_eq!(
            command_for_name("history_previous"),
            Some("prompt.history.previous")
        );
        assert_eq!(command_for_name("leader"), None);
    }
}
