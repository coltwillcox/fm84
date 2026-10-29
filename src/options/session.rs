//! The files kept beside the config: the panels' directories and tabs, and the
//! bookmarks.

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
