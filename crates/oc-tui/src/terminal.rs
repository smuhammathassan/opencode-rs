//! Terminal mode transitions owned by the interactive TUI.
//!
//! The reference TUI temporarily leaves its alternate screen before handing
//! terminal control back to the shell. Unix job control provides the same
//! behavior through SIGTSTP: once the process is continued, raw mode and the
//! alternate screen are restored before the next redraw.

use std::io::{self, Write};

use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// Suspend the current TUI process and restore its terminal state on resume.
///
/// This is deliberately Unix-only behavior. Other platforms do not have a
/// portable foreground-job-control equivalent, so the command is ignored
/// with a warning there rather than attempting to emulate it with a shell
/// command or an unsafe terminal reset.
pub(crate) fn suspend<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<W>>,
    mouse_enabled: bool,
) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        leave_interactive_mode(terminal)?;

        // SIGTSTP has the shell-visible stop/resume semantics expected from
        // Ctrl-Z. After `fg`, execution continues here and restores the TUI.
        let raised = unsafe { libc::raise(libc::SIGTSTP) };
        if raised != 0 {
            let error = io::Error::last_os_error();
            let _ = restore_interactive_mode(terminal, mouse_enabled);
            return Err(error.into());
        }

        restore_interactive_mode(terminal, mouse_enabled)?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = (terminal, mouse_enabled);
        tracing::warn!("terminal suspend is unavailable on this platform");
        Ok(())
    }
}

fn leave_interactive_mode<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<W>>,
) -> anyhow::Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableBracketedPaste,
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    terminal.backend_mut().flush()?;
    Ok(())
}

fn restore_interactive_mode<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<W>>,
    mouse_enabled: bool,
) -> anyhow::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableBracketedPaste
    )?;
    if mouse_enabled {
        crossterm::execute!(terminal.backend_mut(), crossterm::event::EnableMouseCapture)?;
    }
    terminal.clear()?;
    terminal.backend_mut().flush()?;
    Ok(())
}
