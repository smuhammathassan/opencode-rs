//! The full `opencode` CLI surface, mirroring `reference/packages/opencode/src/index.ts`
//! and every `src/cli/cmd/*.ts` builder.

use clap::{Args, Parser, Subcommand};

use super::network::NetworkArgs;

/// Global options defined once at the top level and inherited by every
/// subcommand. Mirrors the `--print-logs` / `--log-level` / `--pure` options in
/// index.ts.
#[derive(Args, Clone, Debug, Default)]
pub struct GlobalArgs {
    /// show version number
    #[arg(short = 'v', long = "version", global = true)]
    pub version: bool,
    /// print logs to stderr
    #[arg(long, global = true)]
    pub print_logs: bool,
    /// log level
    #[arg(long, global = true, value_parser = ["DEBUG", "INFO", "WARN", "ERROR"])]
    pub log_level: Option<String>,
    /// run without external plugins
    #[arg(long, global = true)]
    pub pure: bool,
}

// Options of the default (TUI) command `$0 [project]`.
// From reference/packages/opencode/src/cli/cmd/tui.ts.
#[derive(Args, Clone, Debug, Default)]
pub struct TuiArgs {
    #[command(flatten)]
    pub network: NetworkArgs,

    /// path to start opencode in
    #[arg(value_name = "project")]
    pub project: Option<String>,

    /// model to use in the format of provider/model
    #[arg(short = 'm', long)]
    pub model: Option<String>,
    /// continue the last session
    #[arg(short = 'c', long)]
    pub continue_: bool,
    /// session id to continue
    #[arg(short = 's', long)]
    pub session: Option<String>,
    /// fork the session when continuing (use with --continue or --session)
    #[arg(long)]
    pub fork: bool,
    /// prompt to use
    #[arg(long)]
    pub prompt: Option<String>,
    /// agent to use
    #[arg(long)]
    pub agent: Option<String>,
    /// auto-approve permissions that are not explicitly denied (dangerous!)
    #[arg(long, default_value_t = false)]
    pub auto: bool,
    #[arg(long, hide = true, default_value_t = false)]
    pub yolo: bool,
    #[arg(
        long = "dangerously-skip-permissions",
        hide = true,
        default_value_t = false
    )]
    pub dangerously_skip_permissions: bool,
    /// start the minimal interactive interface
    #[arg(long, default_value_t = false)]
    pub mini: bool,
    #[arg(long, hide = true, num_args = 0..=1, default_missing_value = "true")]
    pub replay: Option<bool>,
    /// disable mini session history replay on resume and after resize
    #[arg(long)]
    pub no_replay: bool,
    /// cap visible mini replay to the newest N messages
    #[arg(long)]
    pub replay_limit: Option<u64>,
    #[arg(long, hide = true)]
    pub demo: Option<bool>,
}

#[derive(Parser, Clone, Debug)]
#[command(
    name = "opencode",
    bin_name = "opencode",
    version = crate::VERSION,
    term_width = 100,
    disable_help_subcommand = true,
    disable_version_flag = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(flatten)]
    pub tui: TuiArgs,
    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    /// Apply the global middleware env vars, mirroring index.ts's middleware:
    /// `--print-logs`, `--log-level` and `--pure` set `OPENCODE_*` env vars.
    pub fn apply_env(&self) {
        if self.global.print_logs {
            std::env::set_var("OPENCODE_PRINT_LOGS", "1");
        }
        if let Some(level) = &self.global.log_level {
            std::env::set_var("OPENCODE_LOG_LEVEL", level);
        }
        if self.global.pure {
            std::env::set_var("OPENCODE_PURE", "1");
        }
    }
}

#[derive(Subcommand, Clone, Debug)]
pub enum Command {
    /// generate shell completion script
    Completion,
    /// start ACP (Agent Client Protocol) server
    Acp(AcpArgs),
    /// manage MCP (Model Context Protocol) servers
    Mcp(McpArgs),
    /// attach to a running opencode server
    Attach(AttachArgs),
    /// run opencode with a message
    Run(RunArgs),
    /// debugging and troubleshooting tools
    Debug(DebugArgs),
    /// manage AI providers and credentials
    #[command(visible_alias = "auth")]
    Providers(ProvidersArgs),
    /// manage agents
    Agent(AgentArgs),
    /// upgrade opencode to the latest or a specific version
    Upgrade(UpgradeArgs),
    /// uninstall opencode and remove all related files
    Uninstall(UninstallArgs),
    /// starts a headless opencode server
    Serve(ServeArgs),
    /// start opencode server and open web interface
    Web(WebArgs),
    /// list all available models
    Models(ModelsArgs),
    /// show token usage and cost statistics
    Stats(StatsArgs),
    /// export session data as JSON
    Export(ExportArgs),
    /// import session data from JSON file or URL
    Import(ImportArgs),
    /// manage GitHub agent
    Github(GithubArgs),
    /// fetch and checkout a GitHub PR branch, then run opencode
    Pr(PrArgs),
    /// manage sessions
    Session(SessionArgs),
    /// install plugin and update config
    #[command(visible_alias = "plug")]
    Plugin(PluginArgs),
    /// database tools
    Db(DbArgs),
    #[command(hide = true)]
    Generate(GenerateArgs),
    #[command(hide = true)]
    Console(ConsoleArgs),
}

#[derive(Args, Clone, Debug, Default)]
pub struct ServeArgs {
    #[command(flatten)]
    pub network: NetworkArgs,
}

#[derive(Args, Clone, Debug, Default)]
pub struct WebArgs {
    #[command(flatten)]
    pub network: NetworkArgs,
}

#[derive(Args, Clone, Debug, Default)]
pub struct AcpArgs {
    #[command(flatten)]
    pub network: NetworkArgs,
    /// working directory
    #[arg(long)]
    pub cwd: Option<String>,
}

/// `opencode run [message..]`
/// From reference/packages/opencode/src/cli/cmd/run.ts.
#[derive(Args, Clone, Debug, Default)]
pub struct RunArgs {
    /// message to send
    #[arg(value_name = "message", num_args = 0..)]
    pub message: Vec<String>,
    /// the command to run, use message for args
    #[arg(long)]
    pub command: Option<String>,
    /// continue the last session
    #[arg(short = 'c', long)]
    pub continue_: bool,
    /// session id to continue
    #[arg(short = 's', long)]
    pub session: Option<String>,
    /// fork the session before continuing (requires --continue or --session)
    #[arg(long)]
    pub fork: bool,
    /// share the session
    #[arg(long)]
    pub share: bool,
    /// model to use in the format of provider/model
    #[arg(short = 'm', long)]
    pub model: Option<String>,
    /// agent to use
    #[arg(long)]
    pub agent: Option<String>,
    /// format: default (formatted) or json (raw JSON events)
    #[arg(long, value_parser = ["default", "json"], default_value = "default")]
    pub format: String,
    /// file(s) to attach to message
    #[arg(short = 'f', long)]
    pub file: Vec<String>,
    /// title for the session (uses truncated prompt if no value provided)
    #[arg(long)]
    pub title: Option<String>,
    /// attach to a running opencode server (e.g., http://localhost:4096)
    #[arg(long)]
    pub attach: Option<String>,
    /// basic auth password (defaults to OPENCODE_SERVER_PASSWORD)
    #[arg(short = 'p', long)]
    pub password: Option<String>,
    /// basic auth username (defaults to OPENCODE_SERVER_USERNAME or 'opencode')
    #[arg(short = 'u', long)]
    pub username: Option<String>,
    /// directory to run in, path on remote server if attaching
    #[arg(long)]
    pub dir: Option<String>,
    /// port for the local server (defaults to random port if no value provided)
    #[arg(long)]
    pub port: Option<u16>,
    /// model variant (provider-specific reasoning effort, e.g., high, max, minimal)
    #[arg(long)]
    pub variant: Option<String>,
    /// show thinking blocks
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub thinking: Option<bool>,
    /// run in direct interactive split-footer mode
    #[arg(short = 'i', long, default_value_t = false)]
    pub interactive: bool,
    /// auto-approve permissions that are not explicitly denied (dangerous!)
    #[arg(long, default_value_t = false)]
    pub auto: bool,
    #[arg(long, hide = true, default_value_t = false)]
    pub mini: bool,
    #[arg(long, hide = true, num_args = 0..=1, default_missing_value = "true")]
    pub replay: Option<bool>,
    #[arg(long = "replay-limit", hide = true)]
    pub replay_limit: Option<u64>,
    #[arg(long, hide = true, default_value_t = false)]
    pub yolo: bool,
    #[arg(
        long = "dangerously-skip-permissions",
        hide = true,
        default_value_t = false
    )]
    pub dangerously_skip_permissions: bool,
    #[arg(long, hide = true, default_value_t = false)]
    pub demo: bool,
    /// arguments after `--` (merged into the message, mirroring populate--).
    #[arg(last = true, num_args = 0.., hide = true)]
    pub dashes: Vec<String>,
}

#[derive(Args, Clone, Debug, Default)]
pub struct ModelsArgs {
    /// provider ID to filter models by
    #[arg(value_name = "provider")]
    pub provider: Option<String>,
    /// use more verbose model output (includes metadata like costs)
    #[arg(long)]
    pub verbose: bool,
    /// refresh the models cache from models.dev
    #[arg(long)]
    pub refresh: bool,
}

#[derive(Args, Clone, Debug)]
pub struct ProvidersArgs {
    #[command(subcommand)]
    pub command: ProvidersCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum ProvidersCommand {
    /// list providers and credentials
    #[command(visible_alias = "ls")]
    List,
    /// log in to a provider
    Login(ProvidersLoginArgs),
    /// log out from a configured provider
    Logout(ProvidersLogoutArgs),
}

#[derive(Args, Clone, Debug, Default)]
pub struct ProvidersLoginArgs {
    /// opencode auth provider
    pub url: Option<String>,
    /// provider id or name to log in to (skips provider selection)
    #[arg(short = 'p', long)]
    pub provider: Option<String>,
    /// login method label (skips method selection)
    #[arg(short = 'm', long)]
    pub method: Option<String>,
}

#[derive(Args, Clone, Debug, Default)]
pub struct ProvidersLogoutArgs {
    /// provider id or name to log out from
    pub provider: Option<String>,
}

#[derive(Args, Clone, Debug, Default)]
pub struct UpgradeArgs {
    /// version to upgrade to, for ex '0.1.48' or 'v0.1.48'
    pub target: Option<String>,
    /// installation method to use
    #[arg(short = 'm', long, value_parser = ["curl", "npm", "pnpm", "bun", "brew", "choco", "scoop"])]
    pub method: Option<String>,
    /// show the upgrade plan without changing the installation
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
}

#[derive(Args, Clone, Debug, Default)]
pub struct UninstallArgs {
    /// keep configuration files
    #[arg(short = 'c', long, default_value_t = false)]
    pub keep_config: bool,
    /// keep session data and snapshots
    #[arg(short = 'd', long, default_value_t = false)]
    pub keep_data: bool,
    /// show what would be removed without removing
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
    /// skip confirmation prompts
    #[arg(short = 'f', long, default_value_t = false)]
    pub force: bool,
}

#[derive(Args, Clone, Debug, Default)]
pub struct StatsArgs {
    /// show stats for the last N days (default: all time)
    #[arg(long)]
    pub days: Option<u32>,
    /// number of tools to show (default: all)
    #[arg(long)]
    pub tools: Option<u32>,
    /// show model statistics (default: hidden). Pass a number to show top N, otherwise shows all
    #[arg(long, num_args = 0..=1, default_missing_value = "all")]
    pub models: Option<String>,
    /// filter by project (default: all projects, empty string: current project)
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Args, Clone, Debug, Default)]
pub struct ExportArgs {
    /// session id to export
    #[arg(value_name = "sessionID")]
    pub session_id: Option<String>,
    /// redact sensitive transcript and file data
    #[arg(long)]
    pub sanitize: bool,
}

#[derive(Args, Clone, Debug, Default)]
pub struct ImportArgs {
    /// path to JSON file or share URL
    pub file: String,
}

#[derive(Args, Clone, Debug, Default)]
pub struct PrArgs {
    /// PR number to checkout
    pub number: u64,
}

#[derive(Args, Clone, Debug, Default)]
pub struct AttachArgs {
    /// http://localhost:4096
    pub url: String,
    /// directory to run in
    #[arg(long)]
    pub dir: Option<String>,
    /// continue the last session
    #[arg(short = 'c', long)]
    pub continue_: bool,
    /// session id to continue
    #[arg(short = 's', long)]
    pub session: Option<String>,
    /// fork the session when continuing (use with --continue or --session)
    #[arg(long)]
    pub fork: bool,
    /// basic auth password (defaults to OPENCODE_SERVER_PASSWORD)
    #[arg(short = 'p', long)]
    pub password: Option<String>,
    /// basic auth username (defaults to OPENCODE_SERVER_USERNAME or 'opencode')
    #[arg(short = 'u', long)]
    pub username: Option<String>,
    /// start the minimal interactive interface
    #[arg(long, default_value_t = false)]
    pub mini: bool,
    #[arg(long, hide = true, num_args = 0..=1, default_missing_value = "true")]
    pub replay: Option<bool>,
    /// disable mini session history replay on resume and after resize
    #[arg(long)]
    pub no_replay: bool,
    /// cap visible mini replay to the newest N messages
    #[arg(long)]
    pub replay_limit: Option<u64>,
}

#[derive(Args, Clone, Debug)]
pub struct McpArgs {
    #[command(subcommand)]
    pub command: McpCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum McpCommand {
    /// add an MCP server
    Add(McpAddArgs),
    /// list MCP servers and their status
    #[command(visible_alias = "ls")]
    List,
    /// authenticate with an OAuth-enabled MCP server
    Auth(McpAuthArgs),
    /// remove OAuth credentials for an MCP server
    Logout(McpLogoutArgs),
    /// debug OAuth connection for an MCP server
    Debug(McpDebugArgs),
}

#[derive(Args, Clone, Debug, Default)]
pub struct McpAddArgs {
    /// name of the MCP server
    pub name: Option<String>,
    /// URL for a remote MCP server
    #[arg(long)]
    pub url: Option<String>,
    /// environment variable for a local MCP server (KEY=VALUE)
    #[arg(long)]
    pub env: Vec<String>,
    /// HTTP header for a remote MCP server (KEY=VALUE)
    #[arg(long)]
    pub header: Vec<String>,
    /// command to run for a local MCP server (after --)
    #[arg(last = true, num_args = 0.., hide = true)]
    pub command: Vec<String>,
}

#[derive(Args, Clone, Debug)]
pub struct McpAuthArgs {
    /// name of the MCP server
    pub name: Option<String>,
    #[command(subcommand)]
    pub command: Option<McpAuthCommand>,
}

#[derive(Subcommand, Clone, Debug)]
pub enum McpAuthCommand {
    /// list OAuth-capable MCP servers and their auth status
    #[command(visible_alias = "ls")]
    List,
}

#[derive(Args, Clone, Debug, Default)]
pub struct McpLogoutArgs {
    /// name of the MCP server
    pub name: Option<String>,
}

#[derive(Args, Clone, Debug, Default)]
pub struct McpDebugArgs {
    /// name of the MCP server
    pub name: String,
}

#[derive(Args, Clone, Debug)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub command: AgentCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum AgentCommand {
    /// create a new agent
    Create(AgentCreateArgs),
    /// list all available agents
    List,
}

#[derive(Args, Clone, Debug, Default)]
pub struct AgentCreateArgs {
    /// directory path to generate the agent file
    #[arg(long)]
    pub path: Option<String>,
    /// what the agent should do
    #[arg(long)]
    pub description: Option<String>,
    /// agent mode
    #[arg(long, value_parser = ["all", "primary", "subagent"])]
    pub mode: Option<String>,
    /// comma-separated list of permissions to allow (default: all). Available: "bash, read, edit, glob, grep, webfetch, task, todowrite, websearch, lsp, skill"
    #[arg(long, visible_alias = "tools")]
    pub permissions: Option<String>,
    /// model to use in the format of provider/model
    #[arg(short = 'm', long)]
    pub model: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct DebugArgs {
    #[command(subcommand)]
    pub command: Option<DebugCommand>,
}

#[derive(Subcommand, Clone, Debug)]
pub enum DebugCommand {
    /// show resolved configuration
    Config,
    /// LSP debugging utilities
    Lsp(DebugLspArgs),
    /// ripgrep debugging utilities
    Rg(DebugRgArgs),
    /// file system debugging utilities
    File(DebugFileArgs),
    /// list all known projects
    Scrap,
    /// list all available skills
    Skill,
    /// snapshot debugging utilities
    Snapshot(DebugSnapshotArgs),
    /// print startup timing
    Startup,
    /// show agent configuration details
    Agent(DebugAgentArgs),
    /// debug v2 catalog and built-in plugins
    V2,
    /// show debug information
    Info,
    /// show global paths (data, config, cache, state)
    Paths,
    /// wait indefinitely (for debugging)
    Wait,
}

#[derive(Args, Clone, Debug)]
pub struct DebugLspArgs {
    #[command(subcommand)]
    pub command: DebugLspCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum DebugLspCommand {
    /// get diagnostics for a file
    Diagnostics {
        #[arg(value_name = "file")]
        file: String,
    },
    /// search workspace symbols
    Symbols {
        #[arg(value_name = "query")]
        query: String,
    },
    /// get symbols from a document
    DocumentSymbols {
        #[arg(value_name = "uri")]
        uri: String,
    },
}

#[derive(Args, Clone, Debug)]
pub struct DebugRgArgs {
    #[command(subcommand)]
    pub command: DebugRgCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum DebugRgCommand {
    /// list files using ripgrep
    Files {
        /// Filter files by query
        #[arg(long)]
        query: Option<String>,
        /// Glob pattern to match files
        #[arg(long)]
        glob: Option<String>,
        /// Limit number of results
        #[arg(long)]
        limit: Option<u64>,
    },
    /// search file contents using ripgrep
    Search {
        /// Search pattern
        pattern: String,
        /// File glob patterns
        #[arg(long)]
        glob: Vec<String>,
        /// Limit number of results
        #[arg(long)]
        limit: Option<u64>,
    },
}

#[derive(Args, Clone, Debug)]
pub struct DebugFileArgs {
    #[command(subcommand)]
    pub command: DebugFileCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum DebugFileCommand {
    /// search files by query
    Search {
        /// Search query
        query: String,
    },
    /// read file contents as JSON
    Read {
        /// File path to read
        path: String,
    },
    /// list files in a directory
    List {
        /// File path to list
        path: String,
    },
}

#[derive(Args, Clone, Debug)]
pub struct DebugSnapshotArgs {
    #[command(subcommand)]
    pub command: DebugSnapshotCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum DebugSnapshotCommand {
    /// track current snapshot state
    Track,
    /// show patch for a snapshot hash
    Patch {
        /// hash
        hash: String,
    },
    /// show diff for a snapshot hash
    Diff {
        /// hash
        hash: String,
    },
}

#[derive(Args, Clone, Debug, Default)]
pub struct DebugAgentArgs {
    /// Agent name
    pub name: String,
    /// Tool id to execute
    #[arg(long)]
    pub tool: Option<String>,
    /// Tool params as JSON or a JS object literal
    #[arg(long)]
    pub params: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum SessionCommand {
    /// list sessions
    List {
        /// limit to N most recent sessions
        #[arg(short = 'n', long)]
        max_count: Option<u32>,
        /// output format
        #[arg(long, value_parser = ["table", "json"], default_value = "table")]
        format: String,
    },
    /// delete a session
    Delete {
        /// session ID to delete
        #[arg(value_name = "sessionID")]
        session_id: String,
    },
}

#[derive(Args, Clone, Debug)]
pub struct DbArgs {
    /// SQL query to execute
    pub query: Option<String>,
    /// Output format
    #[arg(long, value_parser = ["json", "tsv"], default_value = "tsv")]
    pub format: String,
    #[command(subcommand)]
    pub command: Option<DbCommand>,
}

#[derive(Subcommand, Clone, Debug)]
pub enum DbCommand {
    /// print the database path
    Path,
}

#[derive(Args, Clone, Debug)]
pub struct GithubArgs {
    #[command(subcommand)]
    pub command: GithubCommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum GithubCommand {
    /// install the GitHub agent
    Install,
    /// run the GitHub agent
    Run {
        /// GitHub mock event to run the agent for
        #[arg(long)]
        event: Option<String>,
        /// GitHub personal access token (github_pat_********)
        #[arg(long)]
        token: Option<String>,
    },
}

#[derive(Args, Clone, Debug, Default)]
pub struct PluginArgs {
    /// npm module name (optional: read from stdin when not a TTY)
    pub module: Option<String>,
    /// install in global config
    #[arg(short = 'g', long, default_value_t = false)]
    pub global: bool,
    /// replace existing plugin version
    #[arg(short = 'f', long, default_value_t = false)]
    pub force: bool,
}

#[derive(Args, Clone, Debug, Default)]
pub struct GenerateArgs {}

#[derive(Args, Clone, Debug)]
pub struct ConsoleArgs {
    #[command(subcommand)]
    pub command: Option<ConsoleCommand>,
}

#[derive(Subcommand, Clone, Debug)]
pub enum ConsoleCommand {
    /// log in to console
    Login { url: Option<String> },
    /// log out from console
    Logout { email: Option<String> },
    /// switch active org
    Switch,
    /// list orgs
    Orgs,
    /// open active console account
    Open,
}
