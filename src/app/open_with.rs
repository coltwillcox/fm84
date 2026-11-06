//! Alt+O: the programs a file can be opened with, to pick one - and the
//! user's own choice for each extension, which Enter and Shift+Enter take
//! before the system's.

use super::*;
use crate::opener::{Launch, Opener};

/// The chooser on screen.
pub struct OpenWith {
    /// What is to be opened, as the panel has it - inside an archive, the
    /// path through it.
    pub path: PathBuf,
    pub openers: Vec<Opener>,
    pub index: usize,
}

impl AppState {
    /// Alt+O on the entry under the cursor. A directory inside an archive
    /// has nowhere on the disk to be opened from.
    pub fn open_with_menu(&mut self, path: PathBuf, is_dir: bool) {
        if is_dir && self.panel_archive(self.active_side).is_some() {
            return;
        }
        let associations = crate::options::load_associations();
        let openers = crate::opener::openers(&path, is_dir, &associations);
        self.dialog = Some(Dialog::OpenWith(Box::new(OpenWith { path, openers, index: 0 })));
    }

    pub fn move_open_with_cursor(&mut self, step: isize) {
        if let Some(Dialog::OpenWith(menu)) = &mut self.dialog {
            menu.index = menu.index.saturating_add_signed(step).min(menu.openers.len().saturating_sub(1));
        }
    }

    /// E: the `open` file, in whichever editor F4 uses.
    pub fn edit_associations(&mut self) {
        self.dialog = None;
        let Some(file) = crate::options::associations_file() else {
            self.display_error(t!("No config directory to keep associations in").to_string());
            return;
        };
        if self.options.editor.trim().is_empty() {
            self.request_open(file, true);
        } else {
            self.external_edit = Some(file);
        }
    }

    /// Enter on the chooser: the file opened with the program highlighted.
    /// One inside an archive is copied out first.
    pub fn open_with_chosen(&mut self) {
        let Some(Dialog::OpenWith(menu)) = self.dialog.take() else {
            return;
        };
        let Some(opener) = menu.openers.get(menu.index).cloned() else {
            return;
        };
        if self.panel_archive(self.active_side).is_some() {
            self.extract_then(&menu.path, |dest| AfterJob::OpenWith(dest, Box::new(opener)));
            return;
        }
        self.launch(&opener, &menu.path);
    }

    /// Enter or Shift+Enter on a file on the disk: with the first program
    /// the `open` file has for it, or else as the system opens it. A
    /// directory goes to the system whatever the file says.
    pub fn open_outside(&mut self, path: &Path, is_dir: bool) {
        let associations = if is_dir { Vec::new() } else { crate::options::load_associations() };
        match crate::opener::configured(path, &associations).into_iter().next() {
            Some(opener) => self.launch(&opener, path),
            None => {
                if let Err(e) = crate::input::open_with_default(path) {
                    self.display_error(t!("Cannot open file: {e}", e));
                }
            }
        }
    }

    /// `path` opened with `opener`: in a window of its own, left running
    /// whatever fm84 does next, or in this terminal, which the main loop
    /// hands over to it.
    pub fn launch(&mut self, opener: &Opener, path: &Path) {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut process = match &opener.launch {
            Launch::Default => {
                if let Err(e) = crate::input::open_with_default(path) {
                    self.display_error(t!("Cannot open file: {e}", e));
                }
                return;
            }
            Launch::Choose => choose_program(path),
            Launch::Shell(command) => {
                // The file on the end when the command does not say where.
                let (file, files) = uses_entries(command);
                let command = if file || files { command.clone() } else { format!("{command} {{file}}") };
                let values = MenuValues {
                    file: Some(path.to_path_buf()),
                    files: vec![path.to_path_buf()],
                    dir: dir.clone(),
                    other: self.panel_real_dir(!self.active_side),
                };
                shell_command(&command, &values)
            }
            Launch::Desktop { words, name, icon, entry } => {
                let mut args = crate::opener::exec_args(words, path, name, icon.as_deref(), entry).into_iter();
                let mut process = std::process::Command::new(args.next().unwrap_or_default());
                process.args(args).current_dir(&dir);
                process
            }
        };
        if opener.terminal {
            self.user_command = Some(UserCommand {
                label: opener.label.clone(),
                process,
                pause: false,
            });
            return;
        }
        process.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        crate::input::detach(&mut process);
        // Waited for on a thread of its own, so that it does not linger as a
        // zombie until fm84 quits.
        match process.spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(e) => self.display_error(t!("Cannot start {program}: {e}", program = opener.label, e)),
        }
    }
}

/// Windows' own "Open with" dialog for `path`.
fn choose_program(path: &Path) -> std::process::Command {
    let mut process = std::process::Command::new("rundll32.exe");
    process.arg("shell32.dll,OpenAs_RunDLL").arg(path);
    process
}
