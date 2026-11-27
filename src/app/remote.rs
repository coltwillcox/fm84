//! Alt+W: a phone as fm84's remote. The serving is crate::remote's; here is
//! what fm84 shows it - the active panel's listing around the cursor, or the
//! file in the viewer - and how what it sends reaches fm84: a tap moves the
//! cursor, and a key goes through as a key typed would. The remote goes on
//! with its code put away, once a phone has found it, until S in its popup.

use super::*;
use crate::remote::{Key, Remote, Update};

/// Entries the phone is given at a time, around the cursor. A directory of
/// thousands would be a lot to send twice a second; scrolling the cursor
/// brings the rest.
const WINDOW: usize = 200;

/// The remote: the server, the addresses it can be reached at, the code of
/// the one shown, and the phone, once one is heard from.
pub struct RemoteView {
    pub remote: Remote,
    /// The machine's addresses, likeliest for a phone first; Tab goes on.
    pub addresses: Vec<std::net::IpAddr>,
    pub chosen: usize,
    /// The code's width in modules, and whether each is dark, row by row.
    pub code: (usize, Vec<bool>),
    pub phone: Option<std::net::IpAddr>,
    pub failed: Option<String>,
}

impl RemoteView {
    /// What a phone opens, at the address shown.
    pub fn url(&self) -> String {
        self.remote.url(self.addresses[self.chosen])
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

impl Key {
    /// The key it stands for, as the terminal would send it.
    pub fn event(self) -> crossterm::event::KeyEvent {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let code = match self {
            Key::Up => KeyCode::Up,
            Key::Down => KeyCode::Down,
            Key::Left => KeyCode::Left,
            Key::Right => KeyCode::Right,
            Key::PageUp => KeyCode::PageUp,
            Key::PageDown => KeyCode::PageDown,
            Key::Home => KeyCode::Home,
            Key::End => KeyCode::End,
            Key::Enter => KeyCode::Enter,
            Key::Backspace => KeyCode::Backspace,
            Key::Tab => KeyCode::Tab,
            Key::Esc => KeyCode::Esc,
            Key::View => KeyCode::F(3),
        };
        KeyEvent::new(code, KeyModifiers::NONE)
    }
}

impl AppState {
    /// Alt+W: the remote's code shown - the remote started first, if it is
    /// not running.
    pub fn open_remote(&mut self) {
        if self.remote.is_none() {
            let addresses = crate::share::lan_addresses();
            let Some(&first) = addresses.first() else {
                return self.display_error(t!("No network to share on").to_string());
            };
            let remote = match crate::remote::serve(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)) {
                Ok(remote) => remote,
                Err(e) => return self.display_error(t!("Cannot share: {e}", e)),
            };
            let Some(code) = crate::share::qr(&remote.url(first)) else {
                return self.display_error(t!("Cannot share: {e}", e = t!("No network to share on")));
            };
            self.remote = Some(Box::new(RemoteView {
                remote,
                addresses,
                chosen: 0,
                code,
                phone: None,
                failed: None,
            }));
        }
        self.dialog = Some(Dialog::Remote);
    }

    /// S in the popup: the remote stops, and the phone's page with it.
    pub fn stop_remote(&mut self) {
        self.remote = None;
        if matches!(self.dialog, Some(Dialog::Remote)) {
            self.dialog = None;
        }
    }

    /// Once a loop: taps applied, and what fm84 shows given to the phone.
    /// The keys pressed come back, for the caller to put through as keys -
    /// they mean whatever they mean where fm84 is.
    pub fn poll_remote(&mut self) -> Vec<crossterm::event::KeyEvent> {
        let Some(view) = &mut self.remote else {
            return Vec::new();
        };
        let (mut keys, mut picks, mut found) = (Vec::new(), Vec::new(), false);
        while let Ok(update) = view.remote.updates.try_recv() {
            match update {
                Update::Heard(phone) => {
                    found |= view.phone.is_none();
                    view.phone = Some(phone);
                }
                Update::Key(key) => keys.push(key.event()),
                Update::Pick(row) => picks.push(row),
                Update::Failed(message) => view.failed = Some(message),
            }
        }
        // Found, the code has done its work; the screen is the phone's to
        // drive.
        if found && matches!(self.dialog, Some(Dialog::Remote)) {
            self.dialog = None;
        }
        for row in picks {
            self.pick_row(row);
        }
        let shown = self.remote_shown();
        if let Some(view) = &self.remote {
            view.remote.show(shown);
        }
        keys
    }

    /// A tap on an entry: the cursor to its row - in a panel with nothing
    /// over it, where a click would do the same.
    fn pick_row(&mut self, row: usize) {
        if self.dialog.is_some() || self.error.is_some() || self.workers.job.is_some() || !matches!(self.screen, Screen::Panels) {
            return;
        }
        let side = self.active_side;
        if row < self.panel(side).children.len() {
            self.panel_mut(side).state.select(Some(row));
        }
    }

    /// What the phone shows, as JSON: where fm84 is, the entries around the
    /// cursor in the active panel, and what is in the way of the keys.
    fn remote_shown(&self) -> String {
        use crate::remote::json_string;
        let name = |path: &Path| path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let note = if let Some(error) = &self.error {
            Some(error.clone())
        } else if self.workers.job.is_some() {
            Some(t!("A job is running").to_string())
        } else if self.dialog.as_ref().is_some_and(|dialog| !matches!(dialog, Dialog::Remote)) {
            Some(t!("A question is open on the screen").to_string())
        } else {
            None
        };
        let note = note.map_or_else(|| "null".to_string(), |note| json_string(&note));
        let title = match &self.screen {
            Screen::Viewer(state) => t!("Viewing {name}", name = name(&state.file_path)),
            Screen::Editor(state) => t!("Editing {name}", name = name(&state.file_path)),
            Screen::Diff(state) => format!("{} ↔ {}", name(&state.left_path), name(&state.right_path)),
            Screen::Treemap | Screen::Panels => self.panel_dir(self.active_side).display().to_string(),
        };
        if !matches!(self.screen, Screen::Panels) {
            return format!(r#"{{"title":{},"note":{note},"start":0,"cursor":-1,"items":[]}}"#, json_string(&title));
        }
        let panel = self.panel(self.active_side);
        let total = panel.children.len();
        let cursor = panel.state.selected().unwrap_or(0).min(total.saturating_sub(1));
        let start = cursor.saturating_sub(WINDOW / 2).min(total.saturating_sub(WINDOW));
        let items: Vec<String> = panel.children[start..(start + WINDOW).min(total)]
            .iter()
            .map(|item| {
                let size = if item.is_dir { String::new() } else { item.size.trim().to_string() };
                format!(r#"{{"n":{},"d":{},"s":{}}}"#, json_string(&item.name_full), u8::from(item.is_dir), json_string(&size))
            })
            .collect();
        format!(r#"{{"title":{},"note":{note},"start":{start},"cursor":{cursor},"items":[{}]}}"#, json_string(&title), items.join(","))
    }
}
