mod app;
mod constants;
mod display;
mod fs_ops;
mod input;
mod options;
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
use std::io::{self, IsTerminal, Stdout, stdout};
use ui::render_ui;

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<()> {
    color_eyre::install()?;

    // Launched from a .desktop entry with no terminal, raw mode fails with a
    // bare ENXIO that nobody sees. Say what is wrong instead.
    if !stdout().is_terminal() {
        eprintln!("fm84 needs a terminal.");
        eprintln!();
        eprintln!("Run it from one, or launch it through one, for example:");
        eprintln!("    kitty -e fm84");
        eprintln!();
        eprintln!("In a .desktop file, set Terminal=true or make Exec start a terminal.");
        std::process::exit(1);
    }

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
    take_terminal()?;
    Terminal::new(CrosstermBackend::new(stdout()))
}

/// Raw mode, the alternate screen and the mouse: everything fm84 needs from
/// the terminal, and everything restore_terminal gives back.
fn take_terminal() -> io::Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)
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
    display::apply(&app_state.options);

    // A directory gone since then is handled by the reload, which climbs to
    // the nearest one still there.
    if app_state.options.remember_dirs
        && let Some((left, right)) = options::load_session()
    {
        app_state.dir_left = left;
        app_state.dir_right = right;
    }
    app_state.is_f12_displayed = app_state.options.preview_on_start;

    app_state.mounts = fs_ops::list_mounts();
    app_state.reload_panel(true, None);
    app_state.reload_panel(false, None);

    loop {
        // Both before the draw, so the frame shows the newest it could: a cursor
        // move shows its preview, and a transfer its latest count, rather than
        // whatever they were an iteration ago.
        app_state.refresh_cursor_detail();
        app_state.refresh_preview();
        app_state.poll_transfer();
        app_state.image_cache.collect();
        render_ui(terminal, &mut app_state);
        // An image fits the width the viewer was just drawn at, so a new width
        // means drawing again now rather than on the next event.
        if app_state.fit_viewer_image() {
            render_ui(terminal, &mut app_state);
        }
        app_state.refresh_stale_panels();
        if !handle_input(&mut app_state)? {
            break;
        }
        if let Some(path) = app_state.external_edit.take() {
            run_external_editor(terminal, &mut app_state, &path)?;
        }
    }

    // Nowhere to report a failure by now, and nothing lost by it but a
    // starting point.
    if app_state.options.remember_dirs {
        let _ = options::save_session(&app_state.dir_left, &app_state.dir_right);
    }

    Ok(())
}

/// Give the terminal to the editor F11 names until it exits, then take it
/// back. The command is split on whitespace; `{}` stands for the file, which
/// otherwise goes on the end.
fn run_external_editor(terminal: &mut Tui, app_state: &mut AppState, path: &std::path::Path) -> io::Result<()> {
    let command = app_state.options.editor.clone();
    let mut parts: Vec<std::ffi::OsString> =
        command.split_whitespace().map(|part| utils::substitute(part, path.as_os_str())).collect();
    if !command.contains("{}") {
        parts.push(path.as_os_str().to_owned());
    }
    let program = parts.remove(0);

    restore_terminal()?;
    let result = std::process::Command::new(&program)
        .args(&parts)
        .current_dir(path.parent().unwrap_or(std::path::Path::new(".")))
        .status();
    take_terminal()?;
    // The editor drew over everything; forget what ratatui thinks is there.
    terminal.clear()?;

    match result {
        Ok(status) if !status.success() => app_state.display_error(format!("{} exited with {}", program.display(), status)),
        Ok(_) => {}
        Err(e) => app_state.display_error(format!("Cannot start {}: {}", program.display(), e)),
    }
    // The file has likely changed size, and the editor may have left a backup
    // or swap file beside it.
    app_state.reload_panel(true, None);
    app_state.reload_panel(false, None);
    Ok(())
}
