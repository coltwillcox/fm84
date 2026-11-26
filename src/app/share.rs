//! Alt+Q: what is chosen, shared with a phone on the same network, the
//! address drawn as a QR code to scan. The serving is crate::share's; here
//! is the popup's state, and how what the server says reaches it.

use super::*;
use crate::share::{Serving, Update, What};

/// How the share is going, as the popup says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareStatus {
    Waiting,
    Sending { to: std::net::IpAddr, sent: u64 },
    Done(std::net::IpAddr),
    Failed(String),
}

/// The popup: the share being served, the addresses it can be reached at,
/// the code of the one shown, and how it is going.
pub struct ShareView {
    pub serving: Serving,
    /// The machine's addresses, likeliest for a phone first; Tab goes on to
    /// the next, for a machine on more than one network.
    pub addresses: Vec<std::net::IpAddr>,
    pub chosen: usize,
    /// The code's width in modules, and whether each is dark, row by row.
    pub code: (usize, Vec<bool>),
    pub status: ShareStatus,
}

impl ShareView {
    /// A view of `serving`, showing the first of `addresses`; None when no
    /// code can be made of it.
    pub fn new(serving: Serving, addresses: Vec<std::net::IpAddr>) -> Option<ShareView> {
        let code = crate::share::qr(&serving.url(*addresses.first()?))?;
        Some(ShareView {
            serving,
            addresses,
            chosen: 0,
            code,
            status: ShareStatus::Waiting,
        })
    }

    /// What a phone opens, at the address shown.
    pub fn url(&self) -> String {
        self.serving.url(self.addresses[self.chosen])
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
    /// Alt+Q on what is chosen: served on the network the machine is on,
    /// one file as it is, anything more - or a directory - as one zip.
    pub fn open_share(&mut self, items: Vec<(PathBuf, bool)>) {
        let side = self.active_side;
        if items.is_empty() || self.refuse_in_archive(side) {
            return;
        }
        let paths: Vec<PathBuf> = items.into_iter().map(|(path, _)| path).collect();
        let what = match paths.as_slice() {
            [one] if one.is_file() => What::File(one.clone()),
            _ => What::Zip(paths),
        };
        let dir_name = self.panel_dir(side).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let name = crate::share::download_name(&what, &dir_name);
        let addresses = crate::share::lan_addresses();
        if addresses.is_empty() {
            return self.display_error(t!("No network to share on").to_string());
        }
        // At every address, so whichever one the phone can reach - Tab
        // between them - answers.
        let serving = match crate::share::serve(what, name, std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)) {
            Ok(serving) => serving,
            Err(e) => return self.display_error(t!("Cannot share: {e}", e)),
        };
        match ShareView::new(serving, addresses) {
            Some(view) => self.dialog = Some(Dialog::Share(Box::new(view))),
            None => self.display_error(t!("Cannot share: {e}", e = t!("No network to share on"))),
        }
    }

    /// Once a loop: what the server has said since, onto the popup.
    pub fn poll_share(&mut self) {
        let Some(Dialog::Share(view)) = &mut self.dialog else {
            return;
        };
        while let Ok(update) = view.serving.updates.try_recv() {
            view.status = match (update, &view.status) {
                (Update::Started(to), _) => ShareStatus::Sending { to, sent: 0 },
                (Update::Sent(sent), ShareStatus::Sending { to, .. }) => ShareStatus::Sending { to: *to, sent },
                (Update::Sent(_), status) => status.clone(),
                (Update::Broken, ShareStatus::Done(to)) => ShareStatus::Done(*to),
                (Update::Broken, _) => ShareStatus::Waiting,
                (Update::Done(to), _) => ShareStatus::Done(to),
                (Update::Failed(message), _) => ShareStatus::Failed(message),
            };
        }
    }
}
