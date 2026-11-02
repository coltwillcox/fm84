//! Alt+M: commands of the user's own, from a file beside the config, run in
//! the shell on what the panels hold - as F2 is in Midnight Commander.

use super::*;
use crate::options::MenuEntry;

/// The menu on screen, and the one highlighted.
pub struct UserMenu {
    pub entries: Vec<MenuEntry>,
    pub index: usize,
    /// Where they are kept, to say so and to open for editing.
    pub file: Option<PathBuf>,
}

/// What a command is run on.
pub struct MenuValues {
    /// The entry under the cursor, if there is one that is not `..`.
    pub file: Option<PathBuf>,
    /// What is chosen: the selection, or else the entry under the cursor.
    pub files: Vec<PathBuf>,
    pub dir: PathBuf,
    pub other: PathBuf,
}

/// A command ready to run with the terminal handed over to it, and how to
/// name it on the way.
pub struct UserCommand {
    pub label: String,
    pub process: std::process::Command,
    /// Wait for Enter after it, whatever it did; otherwise only after one
    /// that failed, to read why.
    pub pause: bool,
}

impl AppState {
    /// Alt+M: the commands, read afresh so an edit shows at once. The file
    /// is made, with examples in it, the first time.
    pub fn open_user_menu(&mut self) {
        let file = crate::options::menu_file();
        let entries = crate::options::load_menu();
        self.dialog = Some(Dialog::UserMenu(Box::new(UserMenu { entries, index: 0, file })));
    }

    /// Move the highlight by `step`, stopping at either end.
    pub fn move_user_menu_cursor(&mut self, step: isize) {
        if let Some(Dialog::UserMenu(menu)) = &mut self.dialog {
            menu.index = menu.index.saturating_add_signed(step).min(menu.entries.len().saturating_sub(1));
        }
    }

    /// E: the menu file, in whichever editor F4 uses.
    pub fn edit_user_menu(&mut self) {
        let Some(Dialog::UserMenu(menu)) = self.dialog.take() else {
            return;
        };
        let Some(file) = menu.file else {
            self.display_error("No config directory to keep a menu in".to_string());
            return;
        };
        if self.options.editor.trim().is_empty() {
            self.request_open(file, true);
        } else {
            self.external_edit = Some(file);
        }
    }
}

/// Which placeholders a command uses, to know what it needs before it runs.
pub fn uses_entries(command: &str) -> (bool, bool) {
    let file = ["{}", "{file}", "{name}"].iter().any(|placeholder| command.contains(placeholder));
    (file, command.contains("{files}"))
}

/// `command` as the shell is to run it, in `values.dir`. The placeholders
/// are never pasted in as text, where a name with a space or a quote in it
/// would come apart or run as something else: each becomes a variable the
/// shell expands as one word - `{files}` one word a file - and the values go
/// in through the environment and the arguments, exactly as the names are.
pub fn shell_command(command: &str, values: &MenuValues) -> std::process::Command {
    let name = values.file.as_ref().and_then(|file| file.file_name()).map(OsString::from).unwrap_or_default();
    let file = values.file.clone().unwrap_or_default();
    #[cfg(windows)]
    let script = {
        // Nothing in a Windows name needs escaping inside quotes, and cmd
        // does not read again what a variable expands to.
        let files: Vec<String> = (1..=values.files.len()).map(|n| format!("\"%FM84_FILE_{n}%\"")).collect();
        command
            .replace("{files}", &files.join(" "))
            .replace("{file}", "\"%FM84_FILE%\"")
            .replace("{}", "\"%FM84_FILE%\"")
            .replace("{name}", "\"%FM84_NAME%\"")
            .replace("{dir}", "\"%FM84_DIR%\"")
            .replace("{other}", "\"%FM84_OTHER%\"")
    };
    #[cfg(not(windows))]
    let script = command
        .replace("{files}", "\"$@\"")
        .replace("{file}", "\"$FM84_FILE\"")
        .replace("{}", "\"$FM84_FILE\"")
        .replace("{name}", "\"$FM84_NAME\"")
        .replace("{dir}", "\"$FM84_DIR\"")
        .replace("{other}", "\"$FM84_OTHER\"");

    #[cfg(windows)]
    let mut process = {
        use std::os::windows::process::CommandExt;
        let mut process = std::process::Command::new("cmd");
        // As typed: the usual quoting of arguments is not cmd's.
        process.raw_arg(format!("/S /C \"{script}\""));
        for (n, path) in values.files.iter().enumerate() {
            process.env(format!("FM84_FILE_{}", n + 1), path);
        }
        process
    };
    #[cfg(not(windows))]
    let mut process = {
        let mut process = std::process::Command::new("sh");
        // $0, then the files as $1 onwards for "$@".
        process.arg("-c").arg(script).arg("fm84").args(&values.files);
        process
    };
    process.env("FM84_FILE", file).env("FM84_NAME", name).env("FM84_DIR", &values.dir).env("FM84_OTHER", &values.other).current_dir(&values.dir);
    process
}
