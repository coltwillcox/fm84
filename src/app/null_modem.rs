//! Alt+N and Alt+Shift+N: the null modem. Alt+Shift+N listens - the active
//! panel's directory takes files other fm84s send, each asked about first
//! unless F11 says not to. Alt+N looks for those listening, and Enter on one
//! sends it what is chosen, as a job: the bar, and a file in the way asked
//! about here as a copy asks. The talking is crate::null_modem's; here is
//! the popups' state, and the job.

use super::*;
use crate::null_modem::{How, Link, Listening, Peer, Scanner, There, Update};

/// A peer not heard from in this long is taken to have stopped listening.
const GONE_AFTER: Duration = Duration::from_secs(4);

/// Alt+Shift+N's popup: the directory taking files, and how it is going.
pub struct ListenView {
    pub listening: Listening,
    /// As this machine is known to others.
    pub name: String,
    /// A sender waiting on whether it may.
    pub asking: Option<Asking>,
    /// Who is sending now.
    pub sender: Option<String>,
    /// The file coming in now: its name, bytes so far and size.
    pub coming: Option<(String, u64, u64)>,
    /// How many have come whole, and their bytes.
    pub received: usize,
    pub received_bytes: u64,
    /// The last that did not come, and why - said until another comes.
    pub lost: Option<(String, String)>,
    /// The last sender gone: who, and whether it said goodbye.
    pub ended: Option<(String, bool)>,
    /// The server could not go on.
    pub failed: Option<String>,
    /// What was not there before, for Ctrl+Z to take back once the popup is
    /// put away.
    created: Vec<PathBuf>,
}

/// A sender asking to send, and where the answer goes.
pub struct Asking {
    pub from: String,
    pub files: u64,
    pub bytes: u64,
    answer: Sender<bool>,
    since: Instant,
}

/// Alt+N's popup: the fm84s found listening, to pick one to send to.
pub struct PickView {
    _scanner: Scanner,
    found: Receiver<Peer>,
    /// Each with when it was last heard from.
    pub peers: Vec<(Peer, Instant)>,
    pub index: usize,
    /// What goes, once one is picked.
    pub items: Vec<(PathBuf, bool)>,
}

impl AppState {
    /// Alt+Shift+N: the active panel's directory, listening for fm84s to
    /// send to it. Not inside an archive, a git commit or the trash.
    pub fn open_null_modem_listen(&mut self) {
        let side = self.active_side;
        if self.refuse_in_archive(side) {
            return;
        }
        let dir = self.panel_dir(side);
        let name = self.user_at_host.0.clone();
        match crate::null_modem::listen(dir, name.clone(), self.options.null_modem_ask, crate::null_modem::PORT) {
            Ok(listening) => {
                self.dialog = Some(Dialog::NullModemListen(Box::new(ListenView {
                    listening,
                    name,
                    asking: None,
                    sender: None,
                    coming: None,
                    received: 0,
                    received_bytes: 0,
                    lost: None,
                    ended: None,
                    failed: None,
                    created: Vec::new(),
                })))
            }
            Err(e) => self.display_error(t!("Cannot listen: {e}", e)),
        }
    }

    /// Y or N to the sender asking.
    pub fn answer_null_modem(&mut self, yes: bool) {
        if let Some(Dialog::NullModemListen(view)) = &mut self.dialog
            && let Some(asking) = view.asking.take()
        {
            let _ = asking.answer.send(yes);
        }
    }

    /// Esc: the listening stops, and what came is one step for Ctrl+Z to
    /// take back - each only as it came, unchanged since.
    pub fn close_null_modem_listen(&mut self) {
        if let Some(Dialog::NullModemListen(view)) = self.dialog.take() {
            let mut view = *view;
            if let Some(asking) = view.asking.take() {
                let _ = asking.answer.send(false);
            }
            let created = std::mem::take(&mut view.created);
            drop(view);
            let done: Vec<Done> = created.into_iter().filter_map(|to| Mark::of(&to, true).map(|mark| Done::Copied { to, mark })).collect();
            if !done.is_empty() {
                self.journal(UndoKind::Copy, done);
            }
        }
    }

    /// Alt+N: look for fm84s listening, to send `items` to one of them.
    pub fn open_null_modem_send(&mut self, items: Vec<(PathBuf, bool)>) {
        if items.is_empty() || self.refuse_in_archive(self.active_side) {
            return;
        }
        match crate::null_modem::scan(crate::null_modem::PORT) {
            Ok(mut scanner) => {
                // The scanner's channel, taken out so the view can read it
                // while still owning what keeps the scanning going.
                let found = std::mem::replace(&mut scanner.found, mpsc::channel().1);
                self.dialog = Some(Dialog::NullModemPick(Box::new(PickView {
                    _scanner: scanner,
                    found,
                    peers: Vec::new(),
                    index: 0,
                    items,
                })));
            }
            Err(e) => self.display_error(t!("Cannot look for machines: {e}", e)),
        }
    }

    pub fn move_null_modem_cursor(&mut self, delta: isize) {
        if let Some(Dialog::NullModemPick(view)) = &mut self.dialog
            && !view.peers.is_empty()
        {
            view.index = view.index.saturating_add_signed(delta).min(view.peers.len() - 1);
        }
    }

    /// Enter: what is chosen, sent to the one picked.
    pub fn send_null_modem(&mut self) {
        let Some(Dialog::NullModemPick(view)) = &self.dialog else {
            return;
        };
        let Some((peer, _)) = view.peers.get(view.index) else {
            return;
        };
        let (peer, items) = (peer.clone(), view.items.clone());
        self.dialog = None;
        self.start_null_modem_send(peer, items);
    }

    /// `items` sent to `peer`, as a job.
    pub(super) fn start_null_modem_send(&mut self, peer: Peer, items: Vec<(PathBuf, bool)>) {
        let from = self.user_at_host.0.clone();
        let clashes = match self.options.on_existing {
            crate::options::OnExisting::Overwrite => Clashes::ReplaceAll,
            crate::options::OnExisting::KeepBoth => Clashes::KeepBothAll,
            // Nothing over there can be looked at first, to refuse before
            // anything is sent; each is asked about as it comes up instead.
            crate::options::OnExisting::Ask | crate::options::OnExisting::Refuse => Clashes::Ask,
        };
        self.start_job(TransferKind::Send, clashes, move |progress| run_send(peer, from, items, progress));
    }

    /// Once a loop: what the listener and the scanner have heard.
    pub fn poll_null_modem(&mut self) {
        match &mut self.dialog {
            Some(Dialog::NullModemListen(view)) => {
                let mut landed = false;
                while let Ok(update) = view.listening.updates.try_recv() {
                    match update {
                        Update::Asking { from, files, bytes, answer } => {
                            // One at a time: an earlier one still asking is
                            // turned away.
                            if let Some(earlier) = view.asking.replace(Asking {
                                from,
                                files,
                                bytes,
                                answer,
                                since: Instant::now(),
                            }) {
                                let _ = earlier.answer.send(false);
                            }
                        }
                        Update::Connected { from } => {
                            view.sender = Some(from);
                            view.ended = None;
                        }
                        Update::Started { name, size } => view.coming = Some((name, 0, size)),
                        Update::Got(got) => {
                            if let Some(coming) = &mut view.coming {
                                coming.1 = got;
                            }
                        }
                        Update::Saved { size, .. } => {
                            view.coming = None;
                            view.received += 1;
                            view.received_bytes += size;
                            view.lost = None;
                            landed = true;
                        }
                        Update::Lost { name, reason } => {
                            view.coming = None;
                            view.lost = Some((name, reason));
                        }
                        Update::Created(path) => {
                            view.created.push(path);
                            landed = true;
                        }
                        Update::Ended { from, complete } => {
                            view.sender = None;
                            view.coming = None;
                            view.ended = Some((from, complete));
                        }
                        Update::Failed(message) => view.failed = Some(message),
                    }
                }
                // A sender that has given up waiting is not asked about any
                // more.
                if view.asking.as_ref().is_some_and(|asking| asking.since.elapsed() >= crate::null_modem::ASK_FOR) {
                    view.asking = None;
                }
                if landed {
                    let dir = view.listening.dir.clone();
                    for side in Side::BOTH {
                        if self.panel_dir(side).starts_with(&dir) {
                            self.reload_panel(side, None);
                        }
                    }
                }
            }
            Some(Dialog::NullModemPick(view)) => {
                while let Ok(peer) = view.found.try_recv() {
                    let now = Instant::now();
                    // The same fm84 heard by two ways - this machine's own,
                    // by its loopback and by its address - kept once, by the
                    // address another machine would reach it at.
                    match view.peers.iter_mut().find(|(known, _)| known.address.port() == peer.address.port() && known.name == peer.name) {
                        Some((known, seen)) => {
                            if crate::null_modem::is_loopback(&known.address) && !crate::null_modem::is_loopback(&peer.address) {
                                known.address = peer.address;
                            }
                            known.dir = peer.dir;
                            *seen = now;
                        }
                        None => view.peers.push((peer, now)),
                    }
                }
                view.peers.retain(|(_, seen)| seen.elapsed() < GONE_AFTER);
                view.index = view.index.min(view.peers.len().saturating_sub(1));
            }
            _ => {}
        }
    }
}

/// What is sent, in the order it goes: each directory before what is in it.
enum Piece {
    Dir { source: PathBuf, path: PathBuf },
    File { source: PathBuf, path: PathBuf, size: u64 },
}

/// `items` walked into pieces, each with its path under the directory it
/// goes into over there, and the bytes of them all. Links are not followed,
/// as a link to somewhere on this machine means nothing on another; they are
/// left out, as is anything else that is neither file nor directory, and how
/// many were is the third. None when the job ended while walking.
fn plan(items: &[(PathBuf, bool)], progress: &mut JobProgress<'_>) -> Option<(Vec<Piece>, u64, u64)> {
    let mut pieces = Vec::new();
    let (mut bytes, mut left_out) = (0u64, 0u64);
    let mut stack: Vec<(PathBuf, PathBuf)> = items.iter().rev().filter_map(|(source, _)| source.file_name().map(|name| (source.clone(), PathBuf::from(name)))).collect();
    while let Some((source, path)) = stack.pop() {
        if progress.cancel.load(Ordering::Relaxed) {
            return None;
        }
        let metadata = loop {
            match std::fs::symlink_metadata(&source) {
                Ok(metadata) => break Some(metadata),
                Err(error) => match progress.failed(&source, &error) {
                    Reply::Retry => {}
                    Reply::Skip | Reply::Delete => break None,
                    Reply::Abort => return None,
                },
            }
        };
        let Some(metadata) = metadata else { continue };
        if metadata.is_file() {
            bytes += metadata.len();
            pieces.push(Piece::File { source, path, size: metadata.len() });
        } else if metadata.is_dir() {
            let entries = loop {
                match std::fs::read_dir(&source).and_then(|entries| entries.collect::<Result<Vec<_>, _>>()) {
                    Ok(entries) => break Some(entries),
                    Err(error) => match progress.failed(&source, &error) {
                        Reply::Retry => {}
                        Reply::Skip | Reply::Delete => break None,
                        Reply::Abort => return None,
                    },
                }
            };
            let Some(mut entries) = entries else { continue };
            entries.sort_by_key(|entry| std::cmp::Reverse(entry.file_name()));
            pieces.push(Piece::Dir { source: source.clone(), path: path.clone() });
            stack.extend(entries.into_iter().map(|entry| (entry.path(), path.join(entry.file_name()))));
        } else {
            left_out += 1;
        }
    }
    Some((pieces, bytes, left_out))
}

/// The worker behind Alt+N's Enter: connect to `peer`, ask to send as
/// `from`, and send each piece of `items` - asking here about what is in the
/// way there, and about what cannot be read, as a copy does.
fn run_send(peer: Peer, from: String, items: Vec<(PathBuf, bool)>, mut progress: JobProgress<'_>) {
    let Some((pieces, bytes, left_out)) = plan(&items, &mut progress) else {
        return progress.finish(Ok(Transfer::Cancelled));
    };
    for _ in 0..left_out {
        let _ = progress.updates.send(JobUpdate::Skipped);
    }
    let files = pieces.iter().filter(|piece| matches!(piece, Piece::File { .. })).count() as u64;
    let _ = progress.updates.send(JobUpdate::Total(bytes));
    let _ = progress.updates.send(JobUpdate::Starting(PathBuf::from(t!("Waiting for {name} to accept", name = peer.name))));
    let mut link = match Link::dial(peer.address, &from, files, bytes, progress.cancel) {
        Ok(link) => link,
        Err(_) if progress.cancel.load(Ordering::Relaxed) => return progress.finish(Ok(Transfer::Cancelled)),
        Err(error) => return progress.finish(Err(std::io::Error::new(error.kind(), format!("{}: {error}", peer.name)))),
    };
    // Where each would land over there, as the question about one in the
    // way shows it.
    let over_there = PathBuf::from(format!("{}:{}", peer.name, link_dir(&peer, &link)));
    let there = |path: &Path| over_there.join(path);
    for piece in pieces {
        match piece {
            Piece::Dir { source, path } => {
                if !progress.step(Step::Starting(&source)) {
                    return progress.finish(Ok(Transfer::Cancelled));
                }
                loop {
                    match link.make_dir(&path) {
                        Ok(Ok(())) => break,
                        Ok(Err(reason)) => match progress.failed(&there(&path), &std::io::Error::other(reason)) {
                            Reply::Retry => {}
                            Reply::Skip | Reply::Delete => break,
                            Reply::Abort => return progress.finish(Ok(Transfer::Cancelled)),
                        },
                        Err(error) => return progress.finish(Err(error)),
                    }
                }
            }
            Piece::File { source, path, size } => {
                if !progress.step(Step::Starting(&source)) {
                    return progress.finish(Ok(Transfer::Cancelled));
                }
                match send_file(&mut link, &source, &path, size, &there(&path), &mut progress) {
                    Ok(true) => {}
                    Ok(false) => return progress.finish(Ok(Transfer::Cancelled)),
                    Err(error) => return progress.finish(Err(error)),
                }
            }
        }
    }
    match link.end() {
        Ok(()) => progress.finish(Ok(Transfer::Done)),
        Err(error) => progress.finish(Err(error)),
    }
}

/// The directory the files go into, as the other end named it on letting
/// us in - or as it named it on being found.
fn link_dir<'a>(peer: &'a Peer, link: &'a Link) -> &'a str {
    if link.dir.is_empty() { &peer.dir } else { &link.dir }
}

/// One file sent: opened, offered, asked about if something is in its way
/// there, and its bytes sent. False when the job is to stop; an error when
/// the link is gone.
fn send_file(link: &mut Link, source: &Path, path: &Path, size: u64, there: &Path, progress: &mut JobProgress<'_>) -> std::io::Result<bool> {
    loop {
        let file = match std::fs::File::open(source) {
            Ok(file) => file,
            Err(error) => match progress.failed(source, &error) {
                Reply::Retry => continue,
                Reply::Skip | Reply::Delete => {
                    let _ = progress.updates.send(JobUpdate::Advanced(size));
                    return Ok(true);
                }
                Reply::Abort => return Ok(false),
            },
        };
        let metadata = file.metadata().ok();
        let modified = metadata.as_ref().and_then(|metadata| metadata.modified().ok());
        #[cfg(unix)]
        let mode = metadata.as_ref().map(|metadata| {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o777
        });
        #[cfg(not(unix))]
        let mode = None;
        let how = match link.offer(path, size, modified, mode)? {
            Ok(There::Nothing) => How::New,
            Ok(There::Something { size: dest_size, modified: dest_modified }) => {
                let question = JobClash {
                    source: source.to_path_buf(),
                    dest: there.to_path_buf(),
                    source_size: Some(size),
                    source_modified: modified,
                    dest_size: Some(dest_size),
                    dest_modified,
                };
                match progress.decide(question) {
                    Clash::Replace => How::Replace,
                    Clash::KeepBoth => How::KeepBoth,
                    Clash::Keep => {
                        link.skip()?;
                        let _ = progress.updates.send(JobUpdate::Advanced(size));
                        return Ok(true);
                    }
                    Clash::Abort => {
                        link.skip()?;
                        return Ok(false);
                    }
                }
            }
            Err(reason) => match progress.failed(there, &std::io::Error::other(reason)) {
                Reply::Retry => continue,
                Reply::Skip | Reply::Delete => {
                    let _ = progress.updates.send(JobUpdate::Advanced(size));
                    return Ok(true);
                }
                Reply::Abort => return Ok(false),
            },
        };
        let mut reader = std::io::BufReader::new(file);
        let mut step = |amount: u64| progress.step(Step::Advanced(amount));
        match link.send(how, &mut reader, size, &mut step)? {
            Ok(Some(_)) => return Ok(true),
            Ok(None) => return Ok(false),
            // Sent whole, and not put there: asked about as a copy that
            // failed is, and sent again on Retry.
            Err(reason) => match progress.failed(there, &std::io::Error::other(reason)) {
                Reply::Retry => continue,
                Reply::Skip | Reply::Delete => return Ok(true),
                Reply::Abort => return Ok(false),
            },
        }
    }
}
