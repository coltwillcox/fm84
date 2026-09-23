mod app;
mod constants;
mod fs_ops;
mod input;
mod ui;
mod utils;
mod viewer;

use app::AppState;
use color_eyre::Result;
use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use input::handle_input;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout, stdout};
use ui::render_ui;

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<()> {
    color_eyre::install()?;
    install_panic_hook();

    let mut terminal = init_terminal()?;
    let run_result = run(&mut terminal);
    let restore_result = restore_terminal();

    // Report what went wrong in the app before any trouble tearing the terminal down.
    run_result?;
    restore_result?;
    Ok(())
}

fn init_terminal() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

/// Undo everything init_terminal did. Runs on normal exit, on error, and from the
/// panic hook, so it can't rely on the Terminal still being alive - a build with
/// panic = "abort" never drops it. Showing the cursor here covers that case.
fn restore_terminal() -> io::Result<()> {
    execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture, DisableBracketedPaste, Show)?;
    disable_raw_mode()
}

/// Put the terminal back before the panic report prints. Without this the report
/// is written in raw mode onto the alternate screen, which the terminal discards
/// on exit - the user sees a mangled shell and no message at all.
fn install_panic_hook() {
    // color_eyre::install() has already set its hook; chain onto it.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        hook(panic_info);
    }));
}

fn run(terminal: &mut Tui) -> io::Result<()> {
    let mut app_state = AppState::new();

    app_state.reload_panel(true, None);
    app_state.reload_panel(false, None);

    loop {
        // Before the draw, so a cursor move shows its preview in the same frame.
        app_state.refresh_preview();
        render_ui(terminal, &mut app_state);
        app_state.refresh_stale_panels();
        if !handle_input(&mut app_state)? {
            break;
        }
    }

    Ok(())
}
