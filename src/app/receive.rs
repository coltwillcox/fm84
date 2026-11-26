//! Alt+Shift+Q: files sent from a phone into the directory the active panel
//! is in, the address drawn as a QR code to scan. The receiving is
//! crate::receive's; here is the popup's state, and how what the server says
//! reaches it - the panel read again as each file lands, and the lot taken
//! back by Ctrl+Z once the popup is put away, as a copy's are.

use super::*;
use crate::receive::{Receiving, Update};

/// The popup: the directory taking files, the addresses it can be reached
/// at, the code of the one shown, and what has come.
pub struct ReceiveView {
    pub receiving: Receiving,
    /// The machine's addresses, likeliest for a phone first; Tab goes on.
    pub addresses: Vec<std::net::IpAddr>,
    pub chosen: usize,
    /// The code's width in modules, and whether each is dark, row by row.
    pub code: (usize, Vec<bool>),
    /// Files coming in now: each one's number, name, bytes so far and size.
    pub coming: Vec<(u64, String, u64, u64)>,
    /// How many have come whole, and their bytes.
    pub received: usize,
    pub received_bytes: u64,
    /// The last that did not come, and why - said until another comes.
    pub lost: Option<(String, String)>,
    /// The server could not go on.
    pub failed: Option<String>,
    /// What came, for Ctrl+Z to take back once the popup is put away.
    done: Vec<Done>,
}

impl ReceiveView {
    /// A view of `receiving`, showing the first of `addresses`; None when no
    /// code can be made of it.
    pub fn new(receiving: Receiving, addresses: Vec<std::net::IpAddr>) -> Option<ReceiveView> {
        let code = crate::share::qr(&receiving.url(*addresses.first()?))?;
        Some(ReceiveView {
            receiving,
            addresses,
            chosen: 0,
            code,
            coming: Vec::new(),
            received: 0,
            received_bytes: 0,
            lost: None,
            failed: None,
            done: Vec::new(),
        })
    }

    /// What a phone opens, at the address shown.
    pub fn url(&self) -> String {
        self.receiving.url(self.addresses[self.chosen])
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
    /// Alt+Shift+Q: the active panel's directory, taking files a phone sends.
    /// Not inside an archive, a git commit or the trash, which are not
    /// written to so.
    pub fn open_receive(&mut self) {
        let side = self.active_side;
        if self.refuse_in_archive(side) {
            return;
        }
        let dir = self.panel_dir(side);
        let addresses = crate::share::lan_addresses();
        if addresses.is_empty() {
            return self.display_error(t!("No network to share on").to_string());
        }
        // At every address, so whichever one the phone can reach answers.
        let receiving = match crate::receive::receive(dir, std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)) {
            Ok(receiving) => receiving,
            Err(e) => return self.display_error(t!("Cannot receive: {e}", e)),
        };
        match ReceiveView::new(receiving, addresses) {
            Some(view) => self.dialog = Some(Dialog::Receive(Box::new(view))),
            None => self.display_error(t!("Cannot receive: {e}", e = t!("No network to share on"))),
        }
    }

    /// Once a loop: what has come since, onto the popup - and each file that
    /// landed into the panels showing where it went.
    pub fn poll_receive(&mut self) {
        let Some(Dialog::Receive(view)) = &mut self.dialog else {
            return;
        };
        let mut landed = false;
        while let Ok(update) = view.receiving.updates.try_recv() {
            match update {
                Update::Started { id, name, size } => view.coming.push((id, name, 0, size)),
                Update::Got { id, got } => {
                    if let Some(coming) = view.coming.iter_mut().find(|coming| coming.0 == id) {
                        coming.2 = got;
                    }
                }
                Update::Saved { id, path, size } => {
                    view.coming.retain(|coming| coming.0 != id);
                    view.received += 1;
                    view.received_bytes += size;
                    view.lost = None;
                    if let Some(mark) = Mark::of(&path, false) {
                        view.done.push(Done::Copied { to: path, mark });
                    }
                    landed = true;
                }
                Update::Lost { id, name, reason } => {
                    view.coming.retain(|coming| coming.0 != id);
                    view.lost = Some((name, reason));
                }
                Update::Failed(message) => view.failed = Some(message),
            }
        }
        if landed {
            let dir = view.receiving.dir.clone();
            for side in Side::BOTH {
                if self.panel_dir(side) == dir {
                    self.reload_panel(side, None);
                }
            }
        }
    }

    /// Esc: the receiving stops, and what came is one step for Ctrl+Z to
    /// take back - each file only as it came, unchanged since.
    pub fn close_receive(&mut self) {
        if let Some(Dialog::Receive(view)) = self.dialog.take() {
            let mut view = *view;
            let done = std::mem::take(&mut view.done);
            drop(view);
            if !done.is_empty() {
                self.journal(UndoKind::Copy, done);
            }
        }
    }
}
