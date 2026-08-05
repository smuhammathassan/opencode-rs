//! Port of `reference/packages/opencode/src/tool/shell/prompt.ts` and
//! `reference/packages/opencode/src/tool/shell/id.ts`.

use crate::prompts;
use crate::schema::{opt_prop, prop, Schema};

pub const TOOL_ID: &str = "bash";

/// `ShellID.toKind` from `reference/packages/opencode/src/tool/shell/id.ts:10`.
pub fn to_kind(value: &str) -> &'static str {
    match value {
        "pwsh" | "powershell" | "cmd" => match value {
            "pwsh" => "pwsh",
            "powershell" => "powershell",
            _ => "cmd",
        },
        _ => "bash",
    }
}

/// `Shell.name` — base name of the shell executable.
pub fn shell_name(shell: &str) -> String {
    std::path::Path::new(shell)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| shell.to_string())
}

pub fn is_powershell(name: &str) -> bool {
    name == "powershell" || name == "pwsh"
}

pub fn is_cmd(name: &str) -> bool {
    name == "cmd"
}

/// `parameterSchema` from `reference/packages/opencode/src/tool/shell/prompt.ts:15`.
pub fn parameter_schema() -> Schema {
    Schema::struct_(
        vec![
            prop(
                "command",
                Schema::string("The command to execute"),
            ),
            opt_prop(
                "timeout",
                Schema::positive_int().with_description("Optional timeout in milliseconds"),
            ),
            opt_prop(
                "workdir",
                Schema::string("The working directory to run the command in. Defaults to the current directory. Use this instead of 'cd' commands."),
            ),
        ],
        "shell",
    )
}

pub struct Limits {
    pub max_lines: usize,
    pub max_bytes: usize,
}

/// `renderPrompt` from `reference/packages/opencode/src/tool/shell/prompt.ts:28`.
pub fn render_prompt(template: &str, values: &std::collections::HashMap<String, String>) -> String {
    let re = regex::Regex::new(r"\$\{(\w+)\}").unwrap();
    re.replace_all(template, |captures: &regex::Captures| {
        let key = &captures[1];
        match values.get(key) {
            Some(value) => value.clone(),
            None => panic!("Missing shell prompt value: {key}"),
        }
    })
    .to_string()
}

/// `shellDisplayName` from `reference/packages/opencode/src/tool/shell/prompt.ts:36`.
fn shell_display_name(name: &str) -> String {
    match name {
        "pwsh" => "PowerShell (7+)".to_string(),
        "powershell" => "Windows PowerShell (5.1)".to_string(),
        "cmd" => "cmd.exe".to_string(),
        other => other.to_string(),
    }
}

/// `powershellNotes` from `reference/packages/opencode/src/tool/shell/prompt.ts:43`.
fn powershell_notes(name: &str) -> String {
    if name == "pwsh" {
        return "# PowerShell (7+) shell notes
- This cross-platform shell supports pipeline chain operators (`&&` and `||`).
- Use double quotes for interpolated strings (`\"Hello $name\"`), single quotes for verbatim strings.
- Prefer full cmdlet names like `Get-ChildItem`, `Set-Content`, `Remove-Item`, and `New-Item` over aliases.
- Use `$(...)` for subexpressions. Use `@(...)` for array expressions.
- To call a native executable whose path contains spaces, use the call operator: `& \"path/to/exe\" args`.
- Escape special characters with the PowerShell backtick character."
            .to_string();
    }
    if name == "powershell" {
        return "# Windows PowerShell (5.1) shell notes
- Use `cmd1; if ($?) { cmd2 }` to chain dependent commands.
- Use double quotes for interpolated strings (`\"Hello $name\"`), single quotes for verbatim strings.
- Prefer full cmdlet names like `Get-ChildItem`, `Set-Content`, `Remove-Item`, and `New-Item` over aliases.
- Use `$(...)` for subexpressions. Use `@(...)` for array expressions.
- To call a native executable whose path contains spaces, use the call operator: `& \"path/to/exe\" args`.
- Escape special characters with the PowerShell backtick character."
            .to_string();
    }
    String::new()
}

/// `chainGuidance` from `reference/packages/opencode/src/tool/shell/prompt.ts:65`.
fn chain_guidance(name: &str) -> String {
    if name == "powershell" {
        return "If the commands depend on each other and must run sequentially, avoid '&&' in this shell because Windows PowerShell (5.1) does not support it. Use PowerShell conditionals such as `cmd1; if ($?) { cmd2 }` when later commands must depend on earlier success.".to_string();
    }
    if is_powershell(name) {
        return "If the commands depend on each other and must run sequentially, use a single bash tool call with '&&' to chain them together (e.g., `git add . && git commit -m \"message\" && git push`). For instance, if one operation must complete before another starts (like New-Item before Copy-Item, Write before bash for git operations, or git add before git commit), run these operations sequentially instead.".to_string();
    }
    if is_cmd(name) {
        return "If the commands depend on each other and must run sequentially, use a single bash tool call with `&&` to chain them together (e.g., `mkdir out && dir out`). For instance, if one operation must complete before another starts, run these operations sequentially instead.".to_string();
    }
    "If the commands depend on each other and must run sequentially, use a single Bash call with '&&' to chain them together (e.g., `git add . && git commit -m \"message\" && git push`). For instance, if one operation must complete before another starts (like mkdir before cp, Write before Bash for git operations, or git add before git commit), run these operations sequentially instead.".to_string()
}

/// `bashCommandSection` from `reference/packages/opencode/src/tool/shell/prompt.ts:78`.
fn bash_command_section(chain: &str, limits: &Limits, default_timeout_ms: usize) -> String {
    format!(
        "Before executing the command, please follow these steps:

1. Directory Verification:
   - If the command will create new directories or files, first use `ls` to verify the parent directory exists and is the correct location
   - For example, before running \"mkdir foo/bar\", first use `ls foo` to check that \"foo\" exists and is the intended parent directory

2. Command Execution:
   - Always quote file paths that contain spaces with double quotes (e.g., rm \"path with spaces/file.txt\")
   - Examples of proper quoting:
     - mkdir \"/Users/name/My Documents\" (correct)
     - mkdir /Users/name/My Documents (incorrect - will fail)
     - python \"/path/with spaces/script.py\" (correct)
     - python /path/with spaces/script.py (incorrect - will fail)
   - After ensuring proper quoting, execute the command.
   - Capture the output of the command.

Usage notes:
  - The command argument is required.
  - You can specify an optional timeout in milliseconds. If not specified, commands will time out after {default_timeout_ms}ms.
  - If the output exceeds {max_lines} lines or {max_bytes} bytes, it will be truncated and the full output will be written to a file. You can use Read with offset/limit to read specific sections or Grep to search the full content. Do NOT use `head`, `tail`, or other truncation commands to limit output; the full output will already be captured to a file for more precise searching.

  - Avoid using Bash with the `find`, `grep`, `cat`, `head`, `tail`, `sed`, `awk`, or `echo` commands, unless explicitly instructed or when these commands are truly necessary for the task. Instead, always prefer using the dedicated tools for these commands:
    - File search: Use Glob (NOT find or ls)
    - Content search: Use Grep (NOT grep or rg)
    - Read files: Use Read (NOT cat/head/tail)
    - Edit files: Use Edit (NOT sed/awk)
    - Write files: Use Write (NOT echo >/cat <<EOF)
    - Communication: Output text directly (NOT echo/printf)
  - When issuing multiple commands:
    - If the commands are independent and can run in parallel, make multiple bash tool calls in a single message. For example, if you need to run \"git status\" and \"git diff\", send a single message with two bash tool calls in parallel.
    - {chain}
    - Use ';' only when you need to run commands sequentially but don't care if earlier commands fail
    - DO NOT use newlines to separate commands (newlines are ok in quoted strings)
  - AVOID using `cd <directory> && <command>`. Use the `workdir` parameter to change directories instead.
    <good-example>
    Use workdir=\"/foo/bar\" with command: pytest tests
    </good-example>
    <bad-example>
    cd /foo/bar && pytest tests
    </bad-example>",
        default_timeout_ms = default_timeout_ms,
        max_lines = limits.max_lines,
        max_bytes = limits.max_bytes,
        chain = chain,
    )
}

/// `powershellCommandSection` from `reference/packages/opencode/src/tool/shell/prompt.ts:121`.
fn powershell_command_section(
    name: &str,
    chain: &str,
    path_sep: &str,
    limits: &Limits,
    default_timeout_ms: usize,
) -> String {
    format!(
        "{notes}

Before executing the command, please follow these steps:

1. Directory Verification:
   - If the command will create new directories or files, first use `Test-Path -LiteralPath <parent>` to verify the parent directory exists and is the correct location
   - For example, before creating `foo{path_sep}bar`, first use `Test-Path -LiteralPath \"foo\"` to check that `foo` exists and is the intended parent directory

2. Command Execution:
   - Always quote file paths that contain spaces with double quotes (e.g., Remove-Item -LiteralPath \"path with spaces{path_sep}file.txt\")
   - Examples of proper quoting:
     - New-Item -ItemType Directory -Path \"My Documents\" (correct)
     - New-Item -ItemType Directory -Path My Documents (incorrect - path is split)
     - & \"path with spaces{path_sep}script.ps1\" (correct)
     - path with spaces{path_sep}script.ps1 (incorrect - path is split and not invoked)
   - After ensuring proper quoting, execute the command.
   - Capture the output of the command.

Usage notes:
  - The command argument is required.
  - You can specify an optional timeout in milliseconds. If not specified, commands will time out after {default_timeout_ms}ms.
  - If the output exceeds {max_lines} lines or {max_bytes} bytes, it will be truncated and the full output will be written to a file. You can use Read with offset/limit to read specific sections or Grep to search the full content. Do NOT use `Select-Object -First`, `Select-Object -Last`, or other truncation commands to limit output; the full output will already be captured to a file for more precise searching.

  - Avoid using Shell with PowerShell file/content cmdlets unless explicitly instructed or when these cmdlets are truly necessary for the task. Instead, always prefer using the dedicated tools for these commands:
    - File search: Use Glob (NOT Get-ChildItem)
    - Content search: Use Grep (NOT Select-String)
    - Read files: Use Read (NOT Get-Content)
    - Edit files: Use Edit (NOT Set-Content)
    - Write files: Use Write (NOT Set-Content/Out-File or here-strings)
    - Communication: Output text directly (NOT Write-Output/Write-Host)
  - When issuing multiple commands:
    - If the commands are independent and can run in parallel, make multiple bash tool calls in a single message. For example, if you need to run \"git status\" and \"git diff\", send a single message with two bash tool calls in parallel.
    - {chain}
    - Use `;` only when you need to run commands sequentially but don't care if earlier commands fail
    - DO NOT use newlines to separate commands (newlines are ok in quoted strings)
  - AVOID changing directories inside the command. Use the `workdir` parameter to change directories instead.
    <good-example>
    Use workdir=\"project{path_sep}subdir\" with command: pytest tests
    </good-example>
    <bad-example>
    {bad_example}
    </bad-example>",
        notes = powershell_notes(name),
        path_sep = path_sep,
        default_timeout_ms = default_timeout_ms,
        max_lines = limits.max_lines,
        max_bytes = limits.max_bytes,
        chain = chain,
        bad_example = if name == "powershell" {
            format!("Set-Location -LiteralPath \"project{path_sep}subdir\"; if ($?) {{ pytest tests }}")
        } else {
            format!("Set-Location -LiteralPath \"project{path_sep}subdir\" && pytest tests")
        },
    )
}

/// `cmdCommandSection` from `reference/packages/opencode/src/tool/shell/prompt.ts:172`.
fn cmd_command_section(chain: &str, limits: &Limits, default_timeout_ms: usize) -> String {
    format!(
        "# cmd.exe shell notes
- Use double quotes for paths with spaces.
- Use %VAR% for environment variables.
- Use `if exist` for existence checks.
- Use `call` when invoking batch files from another batch-style command.

Before executing the command, please follow these steps:

1. Directory Verification:
   - If the command will create new directories or files, first use `if exist` to verify the parent directory exists and is the correct location
   - For example, before creating `foo\\bar`, first use `if exist \"foo\\\" dir \"foo\"` to check that `foo` exists and is the intended parent directory

2. Command Execution:
   - Always quote file paths that contain spaces with double quotes (e.g., del \"path with spaces\\file.txt\")
   - Examples of proper quoting:
     - mkdir \"My Documents\" (correct)
     - mkdir My Documents (incorrect - path is split)
     - call \"path with spaces\\script.bat\" (correct)
     - path with spaces\\script.bat (incorrect - path is split and not invoked correctly)
   - After ensuring proper quoting, execute the command.
   - Capture the output of the command.

Usage notes:
  - The command argument is required.
  - You can specify an optional timeout in milliseconds. If not specified, commands will time out after {default_timeout_ms}ms.
  - If the output exceeds {max_lines} lines or {max_bytes} bytes, it will be truncated and the full output will be written to a file. You can use Read with offset/limit to read specific sections or Grep to search the full content. Do NOT use `more` or other pagination commands to limit output; the full output will already be captured to a file for more precise searching.

  - Avoid using Shell with cmd.exe file/content commands unless explicitly instructed or when these commands are truly necessary for the task. Instead, always prefer using the dedicated tools for these commands:
    - File search: Use Glob (NOT dir /s)
    - Content search: Use Grep (NOT findstr)
    - Read files: Use Read (NOT type)
    - Edit files: Use Edit (NOT copy)
    - Write files: Use Write (NOT echo > file)
    - Communication: Output text directly (NOT echo)
  - When issuing multiple commands:
    - If the commands are independent and can run in parallel, make multiple bash tool calls in a single message. For example, if you need to run \"dir\" and \"where cmd\", send a single message with two bash tool calls in parallel.
    - {chain}
    - Use `&` only when you need to run commands sequentially but don't care if earlier commands fail
    - DO NOT use newlines to separate commands (newlines are ok in quoted strings)
  - AVOID changing directories inside the command. Use the `workdir` parameter to change directories instead.
    <good-example>
    Use workdir=\"project\\subdir\" with command: dir
    </good-example>
    <bad-example>
    cd /d \"project\\subdir\" && dir
    </bad-example>",
        default_timeout_ms = default_timeout_ms,
        max_lines = limits.max_lines,
        max_bytes = limits.max_bytes,
        chain = chain,
    )
}

struct Profile {
    intro: String,
    workdir_section: String,
    command_section: String,
    git_commands: String,
    git_command_restriction: String,
    create_pr_instruction: String,
    create_pr_example: String,
}

/// `profile` from `reference/packages/opencode/src/tool/shell/prompt.ts:221`.
fn profile(name: &str, platform: &str, limits: &Limits, default_timeout_ms: usize) -> Profile {
    let chain = chain_guidance(name);
    if is_cmd(name) {
        return Profile {
            intro: format!("Executes a given {} command with optional timeout, ensuring proper handling and security measures.", shell_display_name(name)),
            workdir_section: "All commands run in the current working directory by default. Use the `workdir` parameter if you need to run a command in a different directory. AVOID changing directories inside the command - use `workdir` instead.".to_string(),
            command_section: cmd_command_section(&chain, limits, default_timeout_ms),
            git_commands: "git commands".to_string(),
            git_command_restriction: "git commands".to_string(),
            create_pr_instruction: "Create PR using a temporary body file so cmd.exe quoting stays simple.".to_string(),
            create_pr_example: "(\n  echo ## Summary\n  echo - ^<1-3 bullet points^>\n) > pr-body.txt\ngh pr create --title \"the pr title\" --body-file pr-body.txt".to_string(),
        };
    }
    if is_powershell(name) {
        let path_sep = if platform == "win32" { "\\" } else { "/" };
        return Profile {
            intro: format!("Executes a given {} command with optional timeout, ensuring proper handling and security measures.", shell_display_name(name)),
            workdir_section: "All commands run in the current working directory by default. Use the `workdir` parameter if you need to run a command in a different directory. AVOID changing directories inside the command - use `workdir` instead.".to_string(),
            command_section: powershell_command_section(name, &chain, path_sep, limits, default_timeout_ms),
            git_commands: "git commands".to_string(),
            git_command_restriction: "git commands".to_string(),
            create_pr_instruction: "Create PR using gh pr create with a PowerShell here-string to pass the body correctly.".to_string(),
            create_pr_example: "gh pr create --title \"the pr title\" --body @'\n## Summary\n- <1-3 bullet points>\n'@".to_string(),
        };
    }
    Profile {
        intro: "Executes a given bash command in a persistent shell session with optional timeout, ensuring proper handling and security measures.".to_string(),
        workdir_section: "All commands run in the current working directory by default. Use the `workdir` parameter if you need to run a command in a different directory. AVOID using `cd <directory> && <command>` patterns - use `workdir` instead.".to_string(),
        command_section: bash_command_section(&chain, limits, default_timeout_ms),
        git_commands: "bash commands".to_string(),
        git_command_restriction: "git bash commands".to_string(),
        create_pr_instruction: "Create PR using gh pr create with the format below. Use a HEREDOC to pass the body to ensure correct formatting.".to_string(),
        create_pr_example: "gh pr create --title \"the pr title\" --body \"$(cat <<'EOF'\n## Summary\n<1-3 bullet points>".to_string(),
    }
}

/// `render` from `reference/packages/opencode/src/tool/shell/prompt.ts:273`.
pub fn render(
    name: &str,
    platform: &str,
    limits: &Limits,
    default_timeout_ms: usize,
) -> (String, Schema) {
    let selected = profile(name, platform, limits, default_timeout_ms);
    let tmp = std::env::temp_dir().join("opencode");
    let tmp = tmp.to_string_lossy().to_string();
    let mut values = std::collections::HashMap::new();
    values.insert("intro".to_string(), selected.intro);
    values.insert("os".to_string(), platform.to_string());
    values.insert("shell".to_string(), name.to_string());
    values.insert("tmp".to_string(), tmp);
    values.insert("workdirSection".to_string(), selected.workdir_section);
    values.insert("commandSection".to_string(), selected.command_section);
    values.insert("gitCommands".to_string(), selected.git_commands);
    values.insert("toolName".to_string(), TOOL_ID.to_string());
    values.insert(
        "gitCommandRestriction".to_string(),
        selected.git_command_restriction,
    );
    values.insert(
        "createPrInstruction".to_string(),
        selected.create_pr_instruction,
    );
    values.insert("createPrExample".to_string(), selected.create_pr_example);
    let description = render_prompt(prompts::SHELL, &values);
    (description, parameter_schema())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_bash_description() {
        let (description, parameters) = render(
            "bash",
            "linux",
            &Limits {
                max_lines: 2000,
                max_bytes: 50 * 1024,
            },
            120_000,
        );
        assert!(
            description.starts_with("Executes a given bash command in a persistent shell session"),
            "{description}"
        );
        assert!(description.contains("commands will time out after 120000ms."));
        assert!(description.contains("for temporary work outside the workspace"));
        assert!(description.contains("If the output exceeds 2000 lines or 51200 bytes"));
        let schema = crate::jsonschema::from_schema(&parameters);
        assert_eq!(
            schema.pointer("/required"),
            Some(&serde_json::json!(["command"]))
        );
    }
}
