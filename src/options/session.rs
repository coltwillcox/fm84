//! The files kept beside the config: the panels' directories and tabs, the
//! bookmarks, and Alt+M's commands.

use super::*;

/// One panel as the session file keeps it: the directory of each of its
/// tabs, and which one it was showing.
#[derive(Debug, PartialEq, Eq)]
pub struct PanelSession {
    pub tabs: Vec<PathBuf>,
    pub active: usize,
}

impl PanelSession {
    /// The directory the panel was showing.
    pub fn dir(&self) -> &std::path::Path {
        &self.tabs[self.active]
    }
}

/// Where the panels were when fm84 last quit, for Remember directories. Kept
/// apart from the options: it changes on every run, and the options only when
/// asked to.
pub fn load_session() -> Option<(PanelSession, PanelSession)> {
    let text = fs::read_to_string(config_path("session")?).ok()?;
    parse_session(&text)
}

/// `left` and `right` are the directories shown, as they always were, so a
/// session reads the same in a version from before tabs. Each tab follows as
/// a `left_tab` or `right_tab` line, in order, with which one was showing.
pub fn save_session(left: &PanelSession, right: &PanelSession) -> io::Result<()> {
    let mut text = format!("left = {}\nright = {}\n", escape_path(left.dir()), escape_path(right.dir()));
    for (side, panel) in [("left", left), ("right", right)] {
        for tab in &panel.tabs {
            text.push_str(&format!("{side}_tab = {}\n", escape_path(tab)));
        }
        text.push_str(&format!("{side}_tab_active = {}\n", panel.active));
    }
    write_config_file("session", &text)
}

/// The directories kept with Ctrl+D, in the order they were added or put
/// in. One `bookmark` line each, in a file of its own beside the session, so
/// a hand-edited list survives a session being written.
pub fn load_bookmarks() -> Vec<PathBuf> {
    config_path("bookmarks").and_then(|path| fs::read_to_string(path).ok()).map(|text| parse_bookmarks(&text)).unwrap_or_default()
}

pub fn save_bookmarks(bookmarks: &[PathBuf]) -> io::Result<()> {
    let text: String = bookmarks.iter().map(|path| format!("bookmark = {}\n", escape_path(path))).collect();
    write_config_file("bookmarks", &text)
}

pub(super) fn parse_bookmarks(text: &str) -> Vec<PathBuf> {
    key_values(text).filter(|&(key, value)| key == "bookmark" && !value.is_empty()).map(|(_, value)| unescape_path(value)).collect()
}

/// One of Alt+M's commands: what the menu calls it, and what runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub label: String,
    pub command: String,
}

/// What a new menu file says, for whoever opens it to add the first.
const MENU_TEMPLATE: &str = "\
# fm84's user menu, opened with Alt+M. A command a line, as
#   label = command
# Each runs in the shell, in the active panel's directory, with fm84 out of
# the way until Enter is pressed after it. In the command:
#   {file}   the entry under the cursor     {name}   its name alone
#   {files}  what is chosen, the selection or else the entry under the cursor
#   {dir}    the panel's directory          {other}  the other panel's
# Each is one word to the shell, spaces and all, quoted or not - and
# {files} one word for each.
#
# Disk use = du -sh {files}
# Git log = git log --oneline --graph -20
# Pack as tar.gz = tar czf {name}.tar.gz {files}
# Diff with the other panel = diff -r {dir} {other}
";

/// Alt+M's commands, from the `menu` file beside the config, in order. A
/// line without a label or a command is passed over.
pub fn load_menu() -> Vec<MenuEntry> {
    config_path("menu").and_then(|path| fs::read_to_string(path).ok()).map(|text| parse_menu(&text)).unwrap_or_default()
}

pub(super) fn parse_menu(text: &str) -> Vec<MenuEntry> {
    key_values(text)
        .filter(|(label, command)| !label.is_empty() && !command.is_empty())
        .map(|(label, command)| MenuEntry {
            label: label.to_string(),
            command: command.to_string(),
        })
        .collect()
}

/// The menu file, written with a few examples to start from if there is
/// none yet. None without a config directory.
pub fn menu_file() -> Option<PathBuf> {
    let path = config_path("menu")?;
    if !path.exists() {
        let _ = write_config_file("menu", MENU_TEMPLATE);
    }
    Some(path)
}

/// A line of the `open` file: the extensions it is for, and the program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Association {
    /// Without their dots, as written; `*` is every file.
    pub extensions: Vec<String>,
    pub command: String,
    /// Run in this terminal, written with a `!` in front.
    pub terminal: bool,
}

/// What a new `open` file says.
const OPEN_TEMPLATE: &str = "\
# The programs fm84 opens files with, by extension, as
#   extensions = command
# The first line for a file is what Enter and Shift+Enter open it with, in
# place of the system's choice; Alt+O lists them all, then what the system
# offers. Extensions are separated by spaces, any case, without the dot -
# tar.gz works too - and * is every file. The command runs in the shell, in
# the file's directory, with {file} where the file goes, or on the end if it
# is not there; {name} {dir} {other} as in the user menu, and unquoted.
# A ! in front runs it in this terminal, fm84 waiting until it is done.
#
# pdf epub = zathura
# jpg jpeg png gif webp = gimp
# md txt = !vim
# * = !less
";

/// The `open` file's lines, in order. One with no extension or no command
/// is passed over.
pub fn load_associations() -> Vec<Association> {
    config_path("open").and_then(|path| fs::read_to_string(path).ok()).map(|text| parse_associations(&text)).unwrap_or_default()
}

pub(super) fn parse_associations(text: &str) -> Vec<Association> {
    key_values(text)
        .filter_map(|(extensions, command)| {
            let extensions: Vec<String> = extensions.split([' ', '\t', ',']).map(|extension| extension.trim_start_matches('.')).filter(|extension| !extension.is_empty()).map(String::from).collect();
            let (command, terminal) = match command.strip_prefix('!') {
                Some(command) => (command.trim(), true),
                None => (command, false),
            };
            (!extensions.is_empty() && !command.is_empty()).then(|| Association {
                extensions,
                command: command.to_string(),
                terminal,
            })
        })
        .collect()
}

/// The `open` file, written with examples if there is none yet.
pub fn associations_file() -> Option<PathBuf> {
    let path = config_path("open")?;
    if !path.exists() {
        let _ = write_config_file("open", OPEN_TEMPLATE);
    }
    Some(path)
}

/// What a new `columns` file says.
const COLUMNS_TEMPLATE: &str = "\
# What fm84's Info column shows, by extension, as
#   extensions = command
# F11 turns the column on. The first line for a file is the one used; the
# first line the command prints is what the column shows. Extensions are
# separated by spaces, any case, without the dot, and * is every file. The
# command runs in the shell, in the file's directory, with {file} where the
# file goes, or on the end if it is not there; it has two seconds. What it
# gives is kept until the file changes.
#
# jpg jpeg png gif webp = identify -format '%wx%h' {file}
# mp3 flac ogg opus m4a = ffprobe -v error -show_entries format=duration -sexagesimal -of csv=p=0 {file} | cut -d. -f1
# pdf = pdfinfo {file} | awk '/^Pages:/ {print $2 \" pages\"}'
# rs py c h go js ts md txt = wc -l < {file}
# * = git log -1 --format=%ar -- {file}
";

/// The `columns` file's lines, in order, read afresh: it is small, and an
/// edit to it shows at the next listing.
pub fn load_columns() -> Vec<Association> {
    config_path("columns").and_then(|path| fs::read_to_string(path).ok()).map(|text| parse_associations(&text)).unwrap_or_default()
}

/// The `columns` file, written with examples if there is none yet.
pub fn columns_file() -> Option<PathBuf> {
    let path = config_path("columns")?;
    if !path.exists() {
        let _ = write_config_file("columns", COLUMNS_TEMPLATE);
    }
    Some(path)
}

/// A path as a session line holds it. `display()` would lose any name that is
/// not valid UTF-8, and the reader trims each value, so spaces at either end
/// would go too. Those bytes, control characters and `%` itself are written
/// as `%XX`; everything else stays as it reads, so the file is still fine to
/// look at and edit by hand.
pub(super) fn escape_path(path: &std::path::Path) -> String {
    let escaped = |bytes: &[u8]| bytes.iter().map(|byte| format!("%{byte:02X}")).collect::<String>();

    let mut out = String::new();
    for chunk in path.as_os_str().as_encoded_bytes().utf8_chunks() {
        for ch in chunk.valid().chars() {
            if ch == '%' || ch.is_control() {
                out.push_str(&escaped(ch.encode_utf8(&mut [0; 4]).as_bytes()));
            } else {
                out.push(ch);
            }
        }
        out.push_str(&escaped(chunk.invalid()));
    }

    let start = out.len() - out.trim_start().len();
    let end = start + out.trim().len();
    format!("{}{}{}", escaped(&out.as_bytes()[..start]), &out[start..end], escaped(&out.as_bytes()[end..]))
}

/// The path escape_path wrote. A `%` not followed by two hex digits is kept
/// as it is, so a session written before paths were escaped still reads.
pub(super) fn unescape_path(text: &str) -> PathBuf {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        // Two hex digits and nothing else: from_str_radix would also take
        // a sign, reading "%+1" as a byte.
        let hex = bytes.get(index + 1..index + 3).filter(|pair| pair.iter().all(u8::is_ascii_hexdigit)).and_then(|pair| std::str::from_utf8(pair).ok());
        match (bytes[index], hex.and_then(|pair| u8::from_str_radix(pair, 16).ok())) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    path_from_bytes(out)
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

/// Elsewhere a path is not arbitrary bytes, and a hand-edited file could hold
/// anything, so it is read as text.
#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

pub(super) fn parse_session(text: &str) -> Option<(PanelSession, PanelSession)> {
    let (mut left, mut right) = (None, None);
    let (mut left_tabs, mut right_tabs) = (Vec::new(), Vec::new());
    let (mut left_active, mut right_active) = (0, 0);
    for (key, value) in key_values(text) {
        match key {
            "left" if !value.is_empty() => left = Some(unescape_path(value)),
            "right" if !value.is_empty() => right = Some(unescape_path(value)),
            "left_tab" if !value.is_empty() => left_tabs.push(unescape_path(value)),
            "right_tab" if !value.is_empty() => right_tabs.push(unescape_path(value)),
            "left_tab_active" => left_active = value.parse().unwrap_or(0),
            "right_tab_active" => right_active = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    // Without tab lines - written before there were tabs, or by hand - a
    // panel is the one directory it names.
    let panel = |dir: PathBuf, tabs: Vec<PathBuf>, active: usize| {
        if tabs.is_empty() {
            PanelSession { tabs: vec![dir], active: 0 }
        } else {
            let active = active.min(tabs.len() - 1);
            PanelSession { tabs, active }
        }
    };
    Some((panel(left?, left_tabs, left_active), panel(right?, right_tabs, right_active)))
}
