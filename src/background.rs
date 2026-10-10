//! Work that touches the disk, kept off the thread that draws and reads keys.
//!
//! A stat, a statvfs or a readdir on a network mount that has stopped
//! answering does not fail, it waits - for minutes, or for good on a hard NFS
//! mount. Done on the UI thread, any of them froze the whole program, Esc and
//! F10 included. Here each is done on a thread of its own, and the UI waits
//! for it only a moment: on a disk that answers, the answer is in long before
//! the frame is drawn, so nothing looks any different; on one that does not,
//! the UI goes on without it and picks it up when it comes.

use crate::fs_ops::Mount;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

/// One thread answering one question at a time, where only the latest
/// question matters: what the cursor is on. Asking again while it is busy
/// replaces the question waiting rather than queueing behind it, so holding an
/// arrow key down over a dead mount costs one stuck thread, not one a key.
pub struct Latest<Q, A> {
    slot: Arc<(Mutex<Slot<Q>>, Condvar)>,
    answers: Receiver<(Q, A)>,
}

struct Slot<Q> {
    question: Option<Q>,
    closed: bool,
}

impl<Q: Send + 'static, A: Send + 'static> Latest<Q, A> {
    pub fn new(work: impl Fn(&Q) -> A + Send + 'static) -> Self {
        let slot = Arc::new((Mutex::new(Slot { question: None, closed: false }), Condvar::new()));
        let (sender, answers) = mpsc::channel();
        let worker_slot = Arc::clone(&slot);
        std::thread::spawn(move || {
            loop {
                let question = {
                    let (lock, wake) = &*worker_slot;
                    let mut slot = lock.lock().unwrap_or_else(PoisonError::into_inner);
                    loop {
                        if slot.closed {
                            return;
                        }
                        if let Some(question) = slot.question.take() {
                            break question;
                        }
                        slot = wake.wait(slot).unwrap_or_else(PoisonError::into_inner);
                    }
                };
                let answer = work(&question);
                if sender.send((question, answer)).is_err() {
                    return;
                }
            }
        });
        Latest { slot, answers }
    }

    /// Put a question to the thread, in place of any it has not started on.
    pub fn ask(&self, question: Q) {
        let (lock, wake) = &*self.slot;
        lock.lock().unwrap_or_else(PoisonError::into_inner).question = Some(question);
        wake.notify_one();
    }

    /// The newest answer to have come in, waiting up to `wait` for one when
    /// none has. Older answers still queued are passed over.
    pub fn answer(&self, wait: Duration) -> Option<(Q, A)> {
        let mut newest = match self.answers.recv_timeout(wait) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
        };
        while let Ok(answer) = self.answers.try_recv() {
            newest = answer;
        }
        Some(newest)
    }
}

impl<Q, A> Drop for Latest<Q, A> {
    fn drop(&mut self) {
        let (lock, wake) = &*self.slot;
        lock.lock().unwrap_or_else(PoisonError::into_inner).closed = true;
        wake.notify_one();
    }
}

/// What the watchers report. Only changes are sent, so a panel left alone
/// costs a message only when something about it moves.
pub enum WatchEvent {
    /// The directory a panel shows changed on disk since it was read.
    Changed { is_left: bool, dir: PathBuf },
    /// Used and total bytes on that directory's filesystem.
    Disk { is_left: bool, dir: PathBuf, usage: Option<(u64, u64)> },
    /// The mounts offered in the drive strips.
    Mounts(Vec<Mount>),
}

/// A watcher thread, stopped when this is dropped. One stuck in a stat on a
/// dead mount is simply left behind; it sends nothing anyone is waiting for.
pub struct Watch {
    pub dir: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Watch the directory a panel shows: its modified time, which moves when an
/// entry is added, removed or renamed, and the space left on its filesystem,
/// which moves with any write anywhere on it. `stamp` and `usage` are what
/// the listing it is watching saw, so a change made while it was being read
/// is still reported.
pub fn watch_dir(
    is_left: bool,
    dir: PathBuf,
    mut stamp: Option<SystemTime>,
    mut usage: Option<(u64, u64)>,
    interval: Duration,
    events: Sender<WatchEvent>,
) -> Watch {
    let stop = Arc::new(AtomicBool::new(false));
    let (watched, worker_stop) = (dir.clone(), Arc::clone(&stop));
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(interval);
            if worker_stop.load(Ordering::Relaxed) {
                return;
            }
            let now = std::fs::metadata(&watched).and_then(|metadata| metadata.modified()).ok();
            if now != stamp {
                stamp = now;
                if events.send(WatchEvent::Changed { is_left, dir: watched.clone() }).is_err() {
                    return;
                }
            }
            let now = crate::fs_ops::disk_usage(&watched);
            if now != usage {
                usage = now;
                if events.send(WatchEvent::Disk { is_left, dir: watched.clone(), usage }).is_err() {
                    return;
                }
            }
        }
    });
    Watch { dir, stop }
}

/// Watch the mounts, so a drive plugged in or pulled out shows in the strip.
/// The first list is sent at once.
pub fn watch_mounts(interval: Duration, events: Sender<WatchEvent>) -> Watch {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = Arc::clone(&stop);
    std::thread::spawn(move || {
        let mut known: Option<Vec<Mount>> = None;
        loop {
            let mounts = crate::fs_ops::list_mounts();
            if known.as_ref() != Some(&mounts) {
                known = Some(mounts.clone());
                if events.send(WatchEvent::Mounts(mounts)).is_err() {
                    return;
                }
            }
            std::thread::sleep(interval);
            if worker_stop.load(Ordering::Relaxed) {
                return;
            }
        }
    });
    Watch { dir: PathBuf::new(), stop }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_latest_question_is_answered_when_it_falls_behind() {
        // A worker held up on its first question, as one in a stat on a dead
        // mount would be, while three more are asked.
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let worker_gate = Arc::clone(&gate);
        let latest = Latest::new(move |question: &u32| {
            drop(worker_gate.lock().unwrap());
            question * 10
        });
        latest.ask(1);
        std::thread::sleep(Duration::from_millis(20));
        latest.ask(2);
        latest.ask(3);
        assert_eq!(latest.answer(Duration::ZERO).map(|(question, _)| question), None, "nothing is in yet");
        drop(held);

        // The one it was on, and then only the last of those waiting.
        let mut answered = Vec::new();
        while let Some(answer) = latest.answer(Duration::from_millis(200)) {
            answered.push(answer);
        }
        assert_eq!(answered.last(), Some(&(3, 30)));
        assert!(!answered.iter().any(|&(question, _)| question == 2), "{answered:?}");
    }
}
