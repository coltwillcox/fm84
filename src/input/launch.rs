//! Programs started from the panels: the terminal F9 opens, and whatever the
//! system opens a file with.

use super::*;

#[cfg(target_os = "macos")]
pub(super) fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    Command::new("open").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub(super) fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    Command::new("cmd").args(["/C", "start", ""]).arg(path).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(super) fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    Command::new("xdg-open").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()?;
    Ok(())
}

pub(super) fn open_terminal(app_state: &mut AppState) {
    let dir = if app_state.is_left_active { &app_state.dir_left } else { &app_state.dir_right };
    let command = app_state.options.terminal.trim();
    let result = if command.is_empty() { spawn_detached_terminal(dir) } else { spawn_configured_terminal(command, dir) };
    if let Err(e) = result {
        app_state.display_error(format!("Cannot open terminal: {}", e));
    }
}

/// The terminal named in F11. Split on whitespace, with `{}` standing for the
/// directory in case the terminal wants it as an argument; it is started in
/// the directory either way.
fn spawn_configured_terminal(command: &str, dir: &std::path::Path) -> std::io::Result<()> {
    let mut parts = command.split_whitespace().map(|part| crate::utils::substitute(part, dir.as_os_str()));
    let program = parts.next().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Empty terminal command"))?;
    let mut process = Command::new(program);
    process.args(parts).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut process);
    process.spawn()?;
    Ok(())
}

/// Keep a started terminal alive after fm84 exits, and out of its signals.
#[cfg(unix)]
fn detach(process: &mut Command) {
    use std::os::unix::process::CommandExt;
    process.process_group(0);
}

#[cfg(windows)]
fn detach(process: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    process.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn detach(_process: &mut Command) {}

#[cfg(target_os = "macos")]
fn spawn_detached_terminal(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    Command::new("open").arg("-a").arg("Terminal").arg(dir).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()?;
    Ok(())
}

#[cfg(windows)]
fn spawn_detached_terminal(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    Command::new("cmd")
        .args(["/C", "start", "cmd"])
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn spawn_detached_terminal(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    if let Ok(term) = std::env::var("TERMINAL") {
        Command::new(&term).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()?;
        return Ok(());
    }
    let emulators = ["xdg-terminal-emulator", "alacritty", "kitty", "foot", "gnome-terminal", "konsole", "xfce4-terminal", "xterm"];
    for emu in &emulators {
        if Command::new(emu).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn().is_ok() {
            return Ok(());
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::NotFound, "No terminal emulator found. Set $TERMINAL."))
}
