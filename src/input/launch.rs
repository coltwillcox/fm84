//! Programs started from the panels: the terminal F9 opens, and whatever the
//! system opens a file with.

use super::*;

#[cfg(target_os = "macos")]
pub(crate) fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    Command::new("open").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    Ok(())
}

/// Asked of the shell directly. Through `cmd /C start` the path was read by
/// cmd, which quotes nothing without a space in it, so a file named `x&calc`
/// ran calc - and `%VAR%` in a name was expanded.
#[cfg(target_os = "windows")]
pub(crate) fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    use std::ffi::{OsStr, c_void};
    use std::os::windows::ffi::OsStrExt;

    // Declared here rather than pulling in windows-sys, as for the drives.
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(hwnd: *mut c_void, operation: *const u16, file: *const u16, parameters: *const u16, directory: *const u16, show: i32) -> *mut c_void;
    }
    const SW_SHOWNORMAL: i32 = 1;

    let wide = |text: &OsStr| text.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let operation = wide(OsStr::new("open"));
    let file = wide(path.as_os_str());
    // SAFETY: both strings are NUL-terminated and outlive the call; the
    // other pointers may be null.
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), operation.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    // Above 32 is success; anything else is an error, also left in GetLastError.
    if result as usize > 32 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub(crate) fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    Command::new("xdg-open").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()?;
    Ok(())
}

pub(super) fn open_terminal(app_state: &mut AppState) {
    // Inside an archive, in the directory the archive is in.
    let dir = &app_state.panel_real_dir(app_state.is_left_active);
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
