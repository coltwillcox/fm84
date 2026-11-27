//! Alt+V: fm84's clipboard shared with a phone, the address drawn as a QR
//! code to scan. The serving is crate::phone_clipboard's; here is the
//! popup's state, and how what the phone sends reaches the clipboard - and
//! the terminal's, as Ctrl+C's paths go - or the panels, as a note saved.

use super::*;
use crate::phone_clipboard::{Sharing, Update};

/// The popup: the clipboard served, the addresses it can be reached at, the
/// code of the one shown, and what last happened.
pub struct ClipboardView {
    pub sharing: Sharing,
    /// The machine's addresses, likeliest for a phone first; Tab goes on.
    pub addresses: Vec<std::net::IpAddr>,
    pub chosen: usize,
    /// The code's width in modules, and whether each is dark, row by row.
    pub code: (usize, Vec<bool>),
    pub status: ClipboardStatus,
    /// Notes saved, for Ctrl+Z to take back once the popup is put away.
    done: Vec<Done>,
}

/// What last happened, for the popup to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardStatus {
    Waiting,
    /// Text from the phone, now on the clipboard: its first line.
    Received(String),
    /// Text from the phone, kept in a file of this name.
    Saved(String),
    /// The phone took fm84's clipboard.
    Taken,
    Failed(String),
}

impl ClipboardView {
    /// What a phone opens, at the address shown.
    pub fn url(&self) -> String {
        self.sharing.url(self.addresses[self.chosen])
    }

    /// Tab or Shift+Tab: the next address, or the one before, and its code.
    pub fn step_address(&mut self, forward: bool) {
        let count = self.addresses.len();
        if count < 2 {
            return;
        }
        self.chosen = if forward { (self.chosen + 1) % count } else { (self.chosen + count - 1) % count };
        if let Some(code) = crate::share::qr(&self.url()) {
            self.code = code;
        }
    }
}

impl AppState {
    /// Alt+V: the clipboard, shared with a phone; a note it saves goes into
    /// the active panel's directory. Not inside an archive, a git commit or
    /// the trash, which are not written to so.
    pub fn open_phone_clipboard(&mut self) {
        let side = self.active_side;
        if self.refuse_in_archive(side) {
            return;
        }
        let addresses = crate::share::lan_addresses();
        let Some(&first) = addresses.first() else {
            return self.display_error(t!("No network to share on").to_string());
        };
        let sharing = match crate::phone_clipboard::share(self.panel_dir(side), self.clipboard.clone(), std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)) {
            Ok(sharing) => sharing,
            Err(e) => return self.display_error(t!("Cannot share: {e}", e)),
        };
        let Some(code) = crate::share::qr(&sharing.url(first)) else {
            return self.display_error(t!("Cannot share: {e}", e = t!("No network to share on")));
        };
        self.dialog = Some(Dialog::PhoneClipboard(Box::new(ClipboardView {
            sharing,
            addresses,
            chosen: 0,
            code,
            status: ClipboardStatus::Waiting,
            done: Vec::new(),
        })));
    }

    /// Once a loop: what the phone sent or took, and fm84's clipboard as it
    /// is now, for the phone to be given.
    pub fn poll_phone_clipboard(&mut self) {
        let Some(Dialog::PhoneClipboard(view)) = &mut self.dialog else {
            return;
        };
        let (mut text, mut landed) = (None, false);
        while let Ok(update) = view.sharing.updates.try_recv() {
            match update {
                Update::Text(received) => {
                    let first = received.lines().find(|line| !line.trim().is_empty()).unwrap_or_default();
                    let more = received.trim_end().contains('\n');
                    view.status = ClipboardStatus::Received(if more { format!("{first}…") } else { first.to_string() });
                    text = Some(received);
                }
                Update::Saved(path) => {
                    view.status = ClipboardStatus::Saved(path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default());
                    if let Some(mark) = Mark::of(&path, false) {
                        view.done.push(Done::Copied { to: path, mark });
                    }
                    landed = true;
                }
                Update::Taken => view.status = ClipboardStatus::Taken,
                Update::Failed(message) => view.status = ClipboardStatus::Failed(message),
            }
        }
        if let Some(text) = text {
            crate::utils::set_system_clipboard(&text);
            self.clipboard = text;
        }
        view.sharing.set_clipboard(&self.clipboard);
        if landed {
            let dir = view.sharing.dir.clone();
            for side in Side::BOTH {
                if self.panel_dir(side) == dir {
                    self.reload_panel(side, None);
                }
            }
        }
    }

    /// Esc: the sharing stops, and the notes saved are one step for Ctrl+Z
    /// to take back - each only as it came, unchanged since.
    pub fn close_phone_clipboard(&mut self) {
        if let Some(Dialog::PhoneClipboard(view)) = self.dialog.take() {
            let mut view = *view;
            let done = std::mem::take(&mut view.done);
            drop(view);
            if !done.is_empty() {
                self.journal(UndoKind::Copy, done);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A request as the page makes it; what comes back after the head.
    fn ask(url: &str, method: &str, path: &str, body: &str) -> String {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, base) = rest.split_at(rest.find('/').unwrap());
        let mut stream = std::net::TcpStream::connect(host).unwrap();
        stream.write_all(format!("{method} {base}{path} HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        let mut all = String::new();
        stream.read_to_string(&mut all).unwrap();
        all.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default()
    }

    /// Polled until `done` says the popup has heard.
    fn until(app_state: &mut AppState, done: impl Fn(&ClipboardStatus) -> bool) {
        let started = Instant::now();
        loop {
            app_state.poll_phone_clipboard();
            if let Some(Dialog::PhoneClipboard(view)) = &app_state.dialog
                && done(&view.status)
            {
                return;
            }
            assert!(started.elapsed() < Duration::from_secs(5), "never heard");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The phone's text on fm84's clipboard, fm84's clipboard as it is now
    /// given to the phone, and a note saved into the panel - for Ctrl+Z to
    /// take back once the popup is put away.
    #[test]
    fn the_clipboard_goes_both_ways_and_a_note_can_be_undone() {
        let dir = std::env::temp_dir().join(format!("fm84-phone-clipboard-app-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app_state = AppState::new();
        let localhost = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        let sharing = crate::phone_clipboard::share(dir.clone(), String::new(), localhost).unwrap();
        let code = crate::share::qr(&sharing.url(localhost)).unwrap();
        app_state.dialog = Some(Dialog::PhoneClipboard(Box::new(ClipboardView {
            sharing,
            addresses: vec![localhost],
            chosen: 0,
            code,
            status: ClipboardStatus::Waiting,
            done: Vec::new(),
        })));
        let Some(Dialog::PhoneClipboard(view)) = &app_state.dialog else { unreachable!() };
        let url = view.url();

        ask(&url, "POST", "/clipboard", "\nfirst line\nsecond");
        until(&mut app_state, |status| matches!(status, ClipboardStatus::Received(_)));
        assert_eq!(app_state.clipboard, "\nfirst line\nsecond");
        let Some(Dialog::PhoneClipboard(view)) = &app_state.dialog else { unreachable!() };
        assert_eq!(view.status, ClipboardStatus::Received("first line…".to_string()));

        app_state.clipboard = "/home/me/copied".to_string();
        app_state.poll_phone_clipboard();
        assert_eq!(ask(&url, "GET", "/clipboard", ""), "/home/me/copied");
        until(&mut app_state, |status| *status == ClipboardStatus::Taken);

        assert_eq!(ask(&url, "POST", "/file", "kept"), "note.txt");
        until(&mut app_state, |status| matches!(status, ClipboardStatus::Saved(_)));
        app_state.close_phone_clipboard();
        assert!(app_state.dialog.is_none());
        let last = app_state.undo.last().expect("nothing to undo");
        assert!(matches!(&last.steps[..], [Done::Copied { to, .. }] if *to == dir.join("note.txt")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
