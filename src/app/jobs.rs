//! Copies, moves and deletes, run off the interface thread, and what they ask
//! about as they go.

use super::*;

/// A copy, move or delete running on a worker thread, and what it has told us
/// so far.
/// The work is off the UI thread because a single write to a stalled network
/// mount can block for seconds, and that is exactly when a progress bar and a
/// way out of it are wanted.
pub struct TransferJob {
    pub kind: TransferKind,
    /// None until the counting pass has something to report. Counted in bytes
    /// for a copy or move, in entries removed for a delete, and in items for a
    /// move to the trash.
    pub total: Option<u64>,
    pub done: u64,
    pub current: PathBuf,
    pub started: Instant,
    /// Set by Esc, read by the worker between entries.
    pub(super) cancel: Arc<AtomicBool>,
    updates: Receiver<JobUpdate>,
    /// An entry the worker could not deal with, waiting on Retry, Skip, Skip
    /// all or Abort - or Delete, for what the trash would not take. The worker
    /// sits still until it hears which.
    pub problem: Option<JobProblem>,
    /// A file in the way of one being copied or moved, waiting on whether to
    /// write over it. The worker sits still until it hears.
    pub clash: Option<JobClash>,
    /// A filesystem without the room for what is about to go to it, waiting
    /// on whether to look again, go on anyway or stop. Asked before anything
    /// is written.
    pub room: Option<Shortfall>,
    /// A file larger than where it goes takes in one, waiting on whether to
    /// split it into pieces, skip it or stop. Asked before anything is
    /// written.
    pub too_big: Option<TooBig>,
    answers: Sender<Answer>,
    /// Entries left where they were, by Skip or Skip all, to say so at the end.
    pub skipped: u64,
    /// What to do with what was copied out, once it all is.
    after: Option<AfterJob>,
    /// What a checksum job found, shown once it is done.
    checksums: Option<Box<crate::checksum::Report>>,
    /// What it did, for Ctrl+Z to take back.
    did: Vec<Done>,
    /// What a search for duplicates found, fed to the panel once it is done.
    duplicates: Option<Duplicates>,
    /// What Alt+R found, shown once it is done.
    dossier: Option<Box<Dossier>>,
    /// What a ghost hunt found, fed to the panel once it is done.
    ghosts: Option<Ghosts>,
    /// What `*` read of a tree, taken in however the job ends.
    tree: Option<TreeFound>,
}

/// A checksum report on screen, and how far down it is scrolled.
#[derive(Debug)]
pub struct ChecksumView {
    pub report: crate::checksum::Report,
    pub scroll: usize,
    /// Rows the popup showed last, for a page up or down.
    pub room: usize,
}

/// An entry of an archive copied out to be looked at, and what then: shown in
/// the viewer, handed to the program the system opens it with, or compared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AfterJob {
    View(PathBuf),
    Open(PathBuf),
    /// Opened with the program picked from Alt+O's chooser.
    OpenWith(PathBuf, Box<crate::opener::Opener>),
    /// Compare the two files, once out, shown under the paths they have in
    /// the panels. `then` is a second archive to copy out of first, when the
    /// two are in different ones.
    Compare {
        files: (PathBuf, PathBuf),
        shown: (PathBuf, PathBuf),
        then: Option<(PathBuf, Vec<(PathBuf, PathBuf)>)>,
    },
    /// Not from an archive: an archive packed, or what was made in one,
    /// landed on in whichever panel shows where it went.
    Land(PathBuf),
}

impl AfterJob {
    /// Copied out only to be looked at, which changes nothing in the panels.
    fn to_scratch(&self) -> bool {
        !matches!(self, AfterJob::Land(_))
    }
}

/// Which of the long jobs is running. They share a popup, a worker and a
/// way out, and differ only in what they count and what they are called.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransferKind {
    Copy,
    Move,
    Delete,
    Trash,
    /// Copying out of an archive.
    Extract,
    /// Making one.
    Pack,
    /// Writing into one: copied or moved in, deleted from it, made in it.
    Update,
    /// Working out checksums, or checking them.
    Checksum,
    /// Alt+S's plan: copies either way, and deletes.
    Sync,
    /// Ctrl+Z, taking back the last of the others.
    Undo,
    /// F5 on the first of a file's pieces: all of them joined back into it.
    Join,
    /// Alt+T's F6: what is in the trash put back where it came from.
    Restore,
    /// Ctrl+E's plan: deletes, renames and copies.
    DirEdit,
    /// Alt+D, looking for files the same as another.
    Duplicates,
    /// Alt+R, summing a directory up.
    Dossier,
    /// Alt+Shift+G, looking for files nobody has touched in years.
    Ghosts,
    /// `*` in Alt+F10's tree, reading everything under a directory.
    Tree,
}

impl TransferKind {
    /// The word the popup and the cancelled message use.
    pub fn title(self) -> &'static str {
        match self {
            TransferKind::Copy => tc!("job", "Copy"),
            TransferKind::Move => tc!("job", "Move"),
            TransferKind::Delete => tc!("job", "Delete"),
            TransferKind::Trash => tc!("job", "Move to trash"),
            TransferKind::Extract => tc!("job", "Extract"),
            TransferKind::Pack => tc!("job", "Pack"),
            TransferKind::Update => tc!("job", "Update archive"),
            TransferKind::Checksum => tc!("job", "Checksum"),
            TransferKind::Sync => tc!("job", "Synchronize"),
            TransferKind::Undo => tc!("job", "Undo"),
            TransferKind::Join => tc!("job", "Join"),
            TransferKind::Restore => tc!("job", "Put back"),
            TransferKind::DirEdit => tc!("job", "Edit as text"),
            TransferKind::Duplicates => tc!("job", "Find duplicates"),
            TransferKind::Dossier => tc!("job", "Dossier"),
            TransferKind::Ghosts => tc!("job", "Ghost hunt"),
            TransferKind::Tree => tc!("job", "Open the tree"),
        }
    }
}

impl TransferJob {
    /// How far along, once there is a total to measure against. None while the
    /// counting pass is still running, or when there is nothing to count.
    pub fn fraction(&self) -> Option<f64> {
        match self.total {
            Some(total) if total > 0 => Some((self.done as f64 / total as f64).min(1.0)),
            _ => None,
        }
    }

    pub fn is_cancelling(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Answer the question on screen, letting the worker go on.
    pub fn answer(&mut self, answer: Answer) {
        if self.problem.take().is_some() || self.clash.take().is_some() || self.room.take().is_some() || self.too_big.take().is_some() {
            let _ = self.answers.send(answer);
        }
    }
}

/// What the popup shows while the worker waits on whether to write over a
/// file: both of them, so the sizes and dates can be weighed against each
/// other.
pub struct JobClash {
    pub source: PathBuf,
    pub dest: PathBuf,
    pub source_size: Option<u64>,
    pub source_modified: Option<SystemTime>,
    pub dest_size: Option<u64>,
    pub dest_modified: Option<SystemTime>,
}

/// How a copy or move deals with files already there. Starts as F11's "When
/// destination exists" says, and the answers to Ask can change it partway.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clashes {
    /// Stop and ask about each.
    Ask,
    ReplaceAll,
    KeepAll,
    /// Keep every file in the way, and write each beside it under a name of
    /// its own.
    KeepBothAll,
    /// Write over a file only when it is older than the one replacing it.
    ReplaceOlder,
}

/// A file larger than the filesystem it is going to takes in one - past
/// 4 GiB, onto FAT - and how large a file there may be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TooBig {
    pub path: PathBuf,
    pub size: u64,
    /// The directory it goes into, standing for its filesystem.
    pub dir: PathBuf,
    pub limit: u64,
}

/// What the popup shows while the worker waits for an answer.
pub struct JobProblem {
    pub path: PathBuf,
    pub message: String,
}

/// The answer to a JobProblem or a JobClash, sent back to the worker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Retry,
    /// Leave this one where it is: the entry that failed, or the file that
    /// would have been written over, along with the one that would have.
    Skip,
    /// Skip this one and every other like it after it without asking.
    SkipAll,
    Abort,
    /// Delete it for good, when the trash would not take it.
    Delete,
    /// Write over the file in the way.
    Overwrite,
    /// Write over this one and every other in the way without asking.
    OverwriteAll,
    /// Write over this one and every other in the way when it is older than
    /// the one replacing it, and leave the rest - without asking again.
    OverwriteOlder,
    /// Keep the file in the way, and write this one beside it as
    /// `name (2).ext`.
    KeepBoth,
    /// Keep both, for this one and every other in the way.
    KeepBothAll,
    /// Go on although there seems not to be the room.
    GoOn,
    /// Split a file too big for where it goes into pieces that fit.
    Split,
}

/// What the worker sends back. Bytes are per chunk rather than a running total,
/// so a message that arrives late still counts once.
pub(super) enum JobUpdate {
    Total(u64),
    Starting(PathBuf),
    Advanced(u64),
    /// Stuck on this entry until an Answer comes back.
    Problem(JobProblem),
    /// Stuck on a file in the way until an Answer comes back.
    Clash(JobClash),
    /// Stuck on a filesystem without the room until an Answer comes back.
    Room(Shortfall),
    /// Stuck on a file too big for where it goes until an Answer comes back.
    TooBig(TooBig),
    /// One more entry left where it was.
    Skipped,
    /// What a checksum job found.
    Checksums(Box<crate::checksum::Report>),
    /// One more thing done, for the journal.
    Did(Done),
    /// What a search for duplicates found.
    Duplicates(Duplicates),
    /// What Alt+R found.
    Dossier(Box<Dossier>),
    /// What a ghost hunt found.
    Ghosts(Ghosts),
    /// What `*` read of a tree.
    Tree(TreeFound),
    Finished(Result<Transfer, String>),
}

/// The worker's side of a job: passes progress to the UI, and when an entry
/// fails, asks there what to do and waits for the answer.
pub(super) struct JobProgress<'a> {
    pub(super) updates: &'a Sender<JobUpdate>,
    pub(super) cancel: &'a AtomicBool,
    pub(super) answers: &'a Receiver<Answer>,
    skip_all: bool,
    pub(super) clashes: Clashes,
    /// Set once the answer was Abort, so the error it ends the job with is
    /// reported as the cancel it was rather than as a failure all over again.
    aborted: bool,
    /// F11's Verify copies, as it was when the job started.
    verify: bool,
}

impl<'a> JobProgress<'a> {
    fn new(updates: &'a Sender<JobUpdate>, cancel: &'a AtomicBool, answers: &'a Receiver<Answer>, clashes: Clashes, verify: bool) -> Self {
        JobProgress {
            updates,
            cancel,
            answers,
            skip_all: false,
            clashes,
            aborted: false,
            verify,
        }
    }

    /// What the bar counts for `bytes` copied: each byte again as it is read
    /// back, when copies are verified.
    fn to_count(&self, bytes: u64) -> u64 {
        if self.verify { bytes.saturating_mul(2) } else { bytes }
    }

    /// Before anything is written: when a filesystem has not the room for
    /// `writes`, ask whether to look again - once some has been made - to go
    /// on anyway, or to stop. The count can be more than is needed: a file
    /// written over gives its room back, and a filesystem may compress. True
    /// to go on; false when the job has ended here, the reason said.
    fn room_for(&mut self, writes: &[(PathBuf, u64)]) -> bool {
        loop {
            let Some(short) = crate::fs_ops::short_of_room(writes) else {
                return true;
            };
            let answer = if self.cancel.load(Ordering::Relaxed) || self.updates.send(JobUpdate::Room(short.clone())).is_err() {
                Answer::Abort
            } else {
                self.answers.recv().unwrap_or(Answer::Abort)
            };
            match answer {
                Answer::Retry => {}
                Answer::GoOn => return true,
                _ => {
                    let _ = self.updates.send(JobUpdate::Finished(Err(short_of_room_message(&short))));
                    return false;
                }
            }
        }
    }

    /// Before anything is written: for each file larger than the filesystem
    /// it goes to takes in one, ask whether to split it into pieces that
    /// fit, to skip it, or to stop. By the items, the size of piece each is
    /// to be cut into - or None to copy it as it is, or to leave it, as
    /// `skipped` says. None for the lot when the job has stopped here.
    fn splits(&mut self, items: &[(PathBuf, PathBuf, bool)], sizes: &[u64]) -> Option<(Vec<Option<u64>>, Vec<bool>)> {
        let mut splits = vec![None; items.len()];
        let mut skipped = vec![false; items.len()];
        for (index, ((source, dest, is_dir), &size)) in items.iter().zip(sizes).enumerate() {
            let regular = std::fs::symlink_metadata(source).is_ok_and(|metadata| metadata.is_file());
            let Some((dir, limit)) = (!*is_dir && regular)
                .then(|| crate::fs_ops::nearest_existing_dir(dest))
                .flatten()
                .and_then(|dir| crate::fs_ops::max_file_size(&dir).map(|limit| (dir, limit)))
            else {
                continue;
            };
            if size <= limit {
                continue;
            }
            let question = TooBig { path: source.clone(), size, dir, limit };
            let answer = if self.cancel.load(Ordering::Relaxed) || self.updates.send(JobUpdate::TooBig(question)).is_err() {
                Answer::Abort
            } else {
                self.answers.recv().unwrap_or(Answer::Abort)
            };
            match answer {
                Answer::Split => splits[index] = Some(crate::fs_ops::piece_size(limit)),
                Answer::Skip => {
                    skipped[index] = true;
                    let _ = self.updates.send(JobUpdate::Skipped);
                    let _ = self.updates.send(JobUpdate::Advanced(self.to_count(size)));
                }
                _ => {
                    let _ = self.updates.send(JobUpdate::Finished(Ok(Transfer::Cancelled)));
                    return None;
                }
            }
        }
        Some((splits, skipped))
    }

    /// Tell the journal of something done.
    pub(super) fn did(&self, done: Done) {
        let _ = self.updates.send(JobUpdate::Did(done));
    }

    /// The message that ends the job, given how its last item came out.
    pub(super) fn finish(&self, outcome: Result<Transfer, std::io::Error>) {
        let outcome = match outcome {
            Err(_) if self.aborted => Ok(Transfer::Cancelled),
            outcome => outcome.map_err(|e| e.to_string()),
        };
        let _ = self.updates.send(JobUpdate::Finished(outcome));
    }
}

impl Progress for JobProgress<'_> {
    fn step(&mut self, step: Step<'_>) -> bool {
        let update = match step {
            Step::Starting(path) => JobUpdate::Starting(path.to_path_buf()),
            Step::Advanced(amount) => JobUpdate::Advanced(amount),
        };
        // A closed channel means the UI has gone, which is its own reason to stop.
        self.updates.send(update).is_ok() && !self.cancel.load(Ordering::Relaxed)
    }

    fn failed(&mut self, path: &Path, error: &std::io::Error) -> Reply {
        // Esc pressed before this came up already said to stop.
        if self.cancel.load(Ordering::Relaxed) {
            self.aborted = true;
            return Reply::Abort;
        }
        let answer = if self.skip_all {
            Answer::Skip
        } else {
            let problem = JobProblem {
                path: path.to_path_buf(),
                message: error.to_string(),
            };
            // Nobody left to ask, or nobody answering, is an Abort.
            if self.updates.send(JobUpdate::Problem(problem)).is_err() { Answer::Abort } else { self.answers.recv().unwrap_or(Answer::Abort) }
        };
        match answer {
            Answer::Retry => Reply::Retry,
            Answer::Skip | Answer::SkipAll => {
                self.skip_all |= answer == Answer::SkipAll;
                let _ = self.updates.send(JobUpdate::Skipped);
                Reply::Skip
            }
            Answer::Abort => {
                self.aborted = true;
                Reply::Abort
            }
            Answer::Delete => Reply::Delete,
            // Answers to other questions, which a failure is never asked.
            Answer::Overwrite | Answer::OverwriteAll | Answer::OverwriteOlder | Answer::KeepBoth | Answer::KeepBothAll | Answer::GoOn | Answer::Split => Reply::Skip,
        }
    }

    fn verify(&self) -> bool {
        self.verify
    }

    fn exists(&mut self, source: &Path, dest: &Path) -> Clash {
        if self.cancel.load(Ordering::Relaxed) {
            return Clash::Abort;
        }
        // An entry of an archive is not on the disk to be asked; its archive
        // says the same of it.
        let described = |path: &Path| {
            std::fs::metadata(path).or_else(|_| path.symlink_metadata()).ok().map(|metadata| (metadata.len(), metadata.modified().ok())).or_else(|| {
                crate::archive::entry_at(path).map(|(index, at)| {
                    let entry = index.entry(at);
                    (entry.size, entry.modified)
                })
            })
        };
        let modified = |path: &Path| described(path).and_then(|(_, modified)| modified);
        let clash = match self.clashes {
            Clashes::Ask => {
                let size = |path: &Path| described(path).map(|(size, _)| size);
                let question = JobClash {
                    source: source.to_path_buf(),
                    dest: dest.to_path_buf(),
                    source_size: size(source),
                    source_modified: modified(source),
                    dest_size: size(dest),
                    dest_modified: modified(dest),
                };
                // Nobody left to ask, or nobody answering, is an Abort.
                let answer = if self.updates.send(JobUpdate::Clash(question)).is_err() { Answer::Abort } else { self.answers.recv().unwrap_or(Answer::Abort) };
                match answer {
                    Answer::Overwrite => Clash::Replace,
                    Answer::Skip => Clash::Keep,
                    Answer::OverwriteAll => {
                        self.clashes = Clashes::ReplaceAll;
                        Clash::Replace
                    }
                    Answer::SkipAll => {
                        self.clashes = Clashes::KeepAll;
                        Clash::Keep
                    }
                    Answer::KeepBoth => Clash::KeepBoth,
                    Answer::KeepBothAll => {
                        self.clashes = Clashes::KeepBothAll;
                        Clash::KeepBoth
                    }
                    Answer::OverwriteOlder => {
                        self.clashes = Clashes::ReplaceOlder;
                        replace_if_older(modified(source), modified(dest))
                    }
                    Answer::Abort | Answer::Retry | Answer::Delete | Answer::GoOn | Answer::Split => Clash::Abort,
                }
            }
            Clashes::ReplaceAll => Clash::Replace,
            Clashes::KeepAll => Clash::Keep,
            Clashes::KeepBothAll => Clash::KeepBoth,
            Clashes::ReplaceOlder => replace_if_older(modified(source), modified(dest)),
        };
        if clash == Clash::Keep {
            let _ = self.updates.send(JobUpdate::Skipped);
        }
        clash
    }
}

/// Why a job did not start for want of room, as the popup says it.
pub fn room_needed(short: &Shortfall) -> String {
    t!(
        "Needs {needed}, {free} free on {path}",
        needed = crate::utils::format_size(short.needed),
        free = crate::utils::format_size(short.free),
        path = short.dir.display()
    )
}

fn short_of_room_message(short: &Shortfall) -> String {
    format!("{}: {}", t!("Not enough room"), room_needed(short))
}

/// Overwrite older: write over what is there only when it is older than what
/// would replace it. When either time cannot be read, it is left alone - it
/// cannot be shown to be older, and what is left can still be copied again.
/// Older by more than a filesystem rounds a time off: FAT to two seconds,
/// NTFS to a tenth of a microsecond. A file copied there and back is the
/// same age, not older, and is not copied again.
fn replace_if_older(source: Option<SystemTime>, dest: Option<SystemTime>) -> Clash {
    match (source, dest) {
        (Some(source), Some(dest)) if source.duration_since(dest).is_ok_and(|gap| gap > crate::sync::SAME_TIME) => Clash::Replace,
        _ => Clash::Keep,
    }
}

/// The worker thread behind a delete. The count comes first so the bar has a
/// denominator; walking the tree twice costs a second pass of readdir, which is
/// cheap beside the removals themselves.
fn run_delete(items: Vec<(PathBuf, bool)>, mut progress: JobProgress<'_>) {
    let _ = progress.updates.send(JobUpdate::Total(count_entries(&items)));

    for (path, is_dir) in items {
        match delete_path(path, is_dir, &mut progress) {
            Ok(Transfer::Done) => {}
            outcome => return progress.finish(outcome),
        }
    }
    progress.finish(Ok(Transfer::Done));
}

/// The worker thread behind a move to the trash. Counted in items rather than
/// entries, since each goes in one piece.
fn run_trash(items: Vec<(PathBuf, bool)>, mut progress: JobProgress<'_>) {
    let _ = progress.updates.send(JobUpdate::Total(items.len() as u64));

    for (path, is_dir) in items {
        let since = super::undo::now_seconds();
        let outcome = crate::fs_ops::trash_path(path.clone(), is_dir, &mut progress);
        // Skipped, it is still where it was.
        if matches!(outcome, Ok(Transfer::Done)) && !path_exists(&path) {
            progress.did(Done::Trashed { from: path, since });
        }
        match outcome {
            Ok(Transfer::Done) => {}
            outcome => return progress.finish(outcome),
        }
    }
    progress.finish(Ok(Transfer::Done));
}

/// The worker thread behind copying out of an archive: each item an entry's
/// path inside it, and where it goes.
fn run_extract(archive: PathBuf, items: Vec<(PathBuf, PathBuf)>, mut progress: JobProgress<'_>) {
    let inners: Vec<PathBuf> = items.iter().map(|(inner, _)| inner.clone()).collect();
    let _ = progress.updates.send(JobUpdate::Total(crate::fs_ops::measure_extract(&archive, &inners)));
    let outcome = crate::fs_ops::extract(&archive, &items, &mut progress);
    progress.finish(outcome);
}

/// The worker thread behind working out checksums. What cannot be read is
/// listed as such rather than asked about: a report is what is wanted.
/// Every algorithm's at once, so the popup goes between them without
/// reading anything again; SHA-256 shown first.
fn run_compute(sources: Vec<(PathBuf, bool)>, base: PathBuf, mut progress: JobProgress<'_>) {
    let (targets, unreadable) = crate::checksum::targets(&sources, &base);
    let _ = progress.updates.send(JobUpdate::Total(targets.iter().map(|target| target.size).sum()));
    let Some(mut sums) = crate::checksum::compute(&targets, &mut progress) else {
        return progress.finish(Ok(Transfer::Cancelled));
    };
    sums.extend(unreadable.into_iter().map(|(name, error)| (name, Err(error))));
    let _ = progress.updates.send(JobUpdate::Checksums(Box::new(crate::checksum::Report::Computed {
        algorithm: crate::checksum::Algorithm::Sha256,
        base,
        sums,
    })));
    progress.finish(Ok(Transfer::Done));
}

/// The worker thread behind checking a checksum file.
fn run_check(file: PathBuf, mut progress: JobProgress<'_>) {
    let text = match std::fs::read(&file) {
        Ok(text) => text,
        Err(error) => return progress.finish(Err(error)),
    };
    let (expected, not_understood) = crate::checksum::parse_sums(&text);
    let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let total = expected.iter().filter_map(|line| std::fs::metadata(base.join(&line.name)).ok()).map(|metadata| metadata.len()).sum();
    let _ = progress.updates.send(JobUpdate::Total(total));
    let Some(results) = crate::checksum::check(&expected, &base, &mut progress) else {
        return progress.finish(Ok(Transfer::Cancelled));
    };
    let _ = progress.updates.send(JobUpdate::Checksums(Box::new(crate::checksum::Report::Checked { file, results, not_understood })));
    progress.finish(Ok(Transfer::Done));
}

/// One archive Alt+P is to make: what goes in, where it goes, and whether
/// it may write over what is there.
pub struct PackArchive {
    pub sources: Vec<(PathBuf, bool)>,
    pub dest: PathBuf,
    pub replacing: bool,
}

/// How Alt+P packs: into what, how hard, what is left out, with what
/// password, and whether what went in is then taken off the disk.
#[derive(Clone, Debug)]
pub struct PackHow {
    pub format: crate::fs_ops::PackFormat,
    pub level: crate::fs_ops::PackLevel,
    pub move_after: bool,
    pub leave_out: Option<String>,
    pub password: Option<String>,
}

/// The worker thread behind packing: what goes into every archive is found
/// first, which gives the bar its total - and keeps one archive made from
/// being found and packed into the next - and then each is written, and
/// what went into it removed when it is a move.
fn run_pack(archives: Vec<PackArchive>, how: PackHow, mut progress: JobProgress<'_>) {
    let mut plans = Vec::with_capacity(archives.len());
    let leave_out = how.leave_out.as_deref().and_then(crate::glob::Glob::exclude);
    for archive in &archives {
        match crate::fs_ops::plan_pack(&archive.sources, &archive.dest, how.format, leave_out.as_ref(), &mut progress) {
            Ok(Some(plan)) => plans.push(plan),
            Ok(None) => return progress.finish(Ok(Transfer::Cancelled)),
            Err(error) => return progress.finish(Err(error)),
        }
    }
    let _ = progress.updates.send(JobUpdate::Total(plans.iter().map(|plan| plan.bytes).sum()));
    for (archive, plan) in archives.iter().zip(&plans) {
        let mut outcome = crate::fs_ops::pack(plan, &archive.dest, how.format, how.level, how.password.as_deref(), archive.replacing, &mut progress);
        if how.move_after && matches!(outcome, Ok(Transfer::Done)) {
            outcome = crate::fs_ops::remove_packed(plan, &mut progress);
        }
        match outcome {
            Ok(Transfer::Done) => {}
            outcome => return progress.finish(outcome),
        }
    }
    progress.finish(Ok(Transfer::Done));
}

/// The worker thread behind writing into an archive: what changes is
/// worked out first - what comes in walked, names already taken asked about
/// - which gives the bar its total, and then the archive is written again.
fn run_repack(archive: PathBuf, edits: crate::fs_ops::Repack, move_after: bool, mut progress: JobProgress<'_>) {
    let outcome = match crate::fs_ops::plan_repack(&archive, &edits, &mut progress) {
        Ok(Some(plan)) => {
            let _ = progress.updates.send(JobUpdate::Total(plan.bytes));
            crate::fs_ops::repack(&plan, move_after, &mut progress)
        }
        Ok(None) => Ok(Transfer::Cancelled),
        Err(error) => Err(error),
    };
    progress.finish(outcome);
}

/// The worker thread behind a copy or move.
///
/// Renames come first and on their own. A move within one filesystem is a
/// rename, which takes no time and moves no bytes - measuring a large tree
/// before doing it would hold up a transfer that was about to be instant.
fn run_transfer(items: Vec<(PathBuf, PathBuf, bool)>, is_copy: bool, mut progress: JobProgress<'_>) {
    let mut remaining = Vec::new();
    for (source, dest, is_dir) in items {
        // rename() replaces a file, or an empty directory, without a word, so
        // only a name nothing has taken goes this way. One that is taken goes
        // through move_path, which asks about a file and merges a directory.
        // Nor one too big for where it goes, which is asked about below.
        let too_big = || {
            let limit = crate::fs_ops::nearest_existing_dir(&dest).and_then(|dir| crate::fs_ops::max_file_size(&dir));
            limit.is_some_and(|limit| std::fs::symlink_metadata(&source).is_ok_and(|metadata| metadata.is_file() && metadata.len() > limit))
        };
        if !is_copy && !path_exists(&dest) && !too_big() && rename_in_place(&source, &dest) {
            if let Some(mark) = Mark::of(&dest, false) {
                progress.did(Done::Moved { from: source, to: dest, mark });
            }
            continue;
        }
        remaining.push((source, dest, is_dir));
    }

    // The first of a file's pieces, copied to a name that is not one: all
    // of them joined, and counted as all of them.
    let joins: Vec<Option<Vec<PathBuf>>> = remaining.iter().map(|(source, dest, is_dir)| if is_copy && !is_dir { crate::fs_ops::joining(source, dest) } else { None }).collect();

    // Only what is actually going to be copied needs counting, and the
    // room for it is looked for before any of it goes.
    let sizes: Vec<u64> = remaining.iter().zip(&joins).map(|(item, join)| join.as_deref().map_or_else(|| measure(std::slice::from_ref(item)), crate::fs_ops::joined_size)).collect();
    // A join is checked against its checksum file, not read back: counted
    // once on the bar however copies are verified.
    let total = sizes.iter().zip(&joins).map(|(&size, join)| if join.is_some() { size } else { progress.to_count(size) }).sum();
    let _ = progress.updates.send(JobUpdate::Total(total));
    if !progress.room_for(&crate::fs_ops::writes(&remaining, &sizes, is_copy)) {
        return;
    }
    // Too big for where it goes, a file is split, or left, as answered.
    let Some((splits, skipped)) = progress.splits(&remaining, &sizes) else {
        return;
    };

    for (((source, dest, is_dir), join), (split, skip)) in remaining.into_iter().zip(joins).zip(splits.into_iter().zip(skipped)) {
        if skip {
            continue;
        }
        if let Some(piece) = split {
            // Moved, the file goes once all its pieces are there. Neither
            // way is it taken back by Ctrl+Z: undoing a move split would
            // delete the only copy left.
            let result = match crate::fs_ops::split_file(&source, &dest, piece, &mut progress) {
                Ok((Transfer::Done, written)) => {
                    if is_copy {
                        for path in written {
                            if let Some(mark) = Mark::of(&path, false) {
                                progress.did(Done::Copied { to: path, mark });
                            }
                        }
                    } else if !written.is_empty() {
                        let _ = std::fs::remove_file(&source);
                    }
                    Ok(Transfer::Done)
                }
                Ok((transfer, _)) => Ok(transfer),
                Err(error) => Err(error),
            };
            match result {
                Ok(Transfer::Done) => continue,
                outcome => return progress.finish(outcome),
            }
        }
        // Only what went where nothing was can be undone: taking back what
        // was written over, or merged into, would take what was there too.
        let new = !path_exists(&dest);
        let result = if let Some(pieces) = join {
            crate::fs_ops::join_pieces(&pieces, &dest, &mut progress)
        } else if is_copy {
            copy_path(source.clone(), dest.clone(), is_dir, &mut progress)
        } else {
            move_path(source.clone(), dest.clone(), &mut progress)
        };
        // However it ended: a copy cancelled partway is undone by deleting
        // what it got as far as. A move partway is in two places, and not.
        if new
            && is_copy
            && let Some(mark) = Mark::of(&dest, true)
        {
            progress.did(Done::Copied { to: dest, mark });
        } else if new
            && !is_copy
            && !path_exists(&source)
            && let Some(mark) = Mark::of(&dest, false)
        {
            progress.did(Done::Moved { from: source, to: dest, mark });
        }
        match result {
            Ok(Transfer::Done) => {}
            outcome => return progress.finish(outcome),
        }
    }
    progress.finish(Ok(Transfer::Done));
}

/// The worker thread behind Alt+S: each copy and delete of the plan in its
/// order, a copy writing over what is in the way - the plan said which way
/// it goes. Counted in the bytes the copies move.
fn run_sync_steps(steps: Vec<crate::app::SyncStep>, to_trash: bool, mut progress: JobProgress<'_>) {
    use crate::app::SyncStep;
    let copies: Vec<(PathBuf, PathBuf, bool)> = steps
        .iter()
        .filter_map(|step| match step {
            SyncStep::Copy(from, to, is_dir) => Some((from.clone(), to.clone(), *is_dir)),
            SyncStep::Delete(..) => None,
        })
        .collect();
    let sizes: Vec<u64> = copies.iter().map(|item| measure(std::slice::from_ref(item))).collect();
    let _ = progress.updates.send(JobUpdate::Total(progress.to_count(sizes.iter().sum())));
    if !progress.room_for(&crate::fs_ops::writes(&copies, &sizes, true)) {
        return;
    }
    for step in steps {
        let outcome = match step {
            SyncStep::Copy(from, to, is_dir) => copy_path(from, to, is_dir, &mut progress),
            SyncStep::Delete(path, is_dir) if to_trash => crate::fs_ops::trash_path(path, is_dir, &mut progress),
            SyncStep::Delete(path, is_dir) => delete_path(path, is_dir, &mut progress),
        };
        match outcome {
            Ok(Transfer::Done) => {}
            outcome => return progress.finish(outcome),
        }
    }
    progress.finish(Ok(Transfer::Done));
}

impl AppState {
    /// A job that has ended, counted onto F1's high scores: what it got
    /// done, cancelled or not, and how long it took, when it finished.
    fn count_scores(&mut self, job: &TransferJob, result: &Result<Transfer, String>) {
        let verified = self.options.verify_copies;
        let scores = &mut self.scores;
        if matches!(result, Ok(Transfer::Done)) {
            scores.jobs += 1;
            scores.longest_job = scores.longest_job.max(job.started.elapsed().as_secs());
            if job.kind == TransferKind::Undo {
                scores.undos += 1;
            }
        }
        match job.kind {
            // Each byte counted twice on the bar, read back, when verified.
            TransferKind::Copy => scores.bytes_copied += if verified { job.done / 2 } else { job.done },
            TransferKind::Join => scores.bytes_copied += job.done,
            TransferKind::Move => scores.bytes_moved += job.done,
            TransferKind::Delete | TransferKind::Trash => scores.deleted += job.done,
            TransferKind::DirEdit => {
                scores.files_renamed += job.did.iter().filter(|done| matches!(done, Done::Moved { .. })).count() as u64;
                scores.deleted += job.did.iter().filter(|done| matches!(done, Done::Trashed { .. })).count() as u64;
            }
            _ => {}
        }
        if let Some(found) = &job.duplicates {
            for group in &found.groups {
                let extra = group.len().saturating_sub(1) as u64;
                let size = group.first().and_then(|path| path.metadata().ok()).map_or(0, |metadata| metadata.len());
                scores.copies_found += extra;
                scores.copy_bytes_found += size * extra;
            }
        }
        scores.save();
    }

    /// Alt+S's plan, run.
    pub fn start_sync(&mut self, steps: Vec<crate::app::SyncStep>, to_trash: bool) {
        self.start_job(TransferKind::Sync, Clashes::ReplaceAll, move |progress| run_sync_steps(steps, to_trash, progress));
    }

    /// Hand a long job to a worker thread and start following it.
    pub(super) fn start_job<F>(&mut self, kind: TransferKind, clashes: Clashes, work: F)
    where
        F: FnOnce(JobProgress<'_>) + Send + 'static,
    {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, updates) = mpsc::channel();
        let (answers, worker_answers) = mpsc::channel();
        let worker_cancel = Arc::clone(&cancel);
        let verify = self.options.verify_copies;
        std::thread::spawn(move || work(JobProgress::new(&sender, &worker_cancel, &worker_answers, clashes, verify)));

        self.workers.job = Some(TransferJob {
            kind,
            total: None,
            done: 0,
            current: PathBuf::new(),
            started: Instant::now(),
            cancel,
            updates,
            problem: None,
            clash: None,
            room: None,
            too_big: None,
            answers,
            skipped: 0,
            after: None,
            checksums: None,
            did: Vec::new(),
            duplicates: None,
            ghosts: None,
            tree: None,
            dossier: None,
        });
    }

    /// Work out the checksums of `sources`, each named from `base`, and show
    /// them once they are all in.
    pub fn start_checksums(&mut self, sources: Vec<(PathBuf, bool)>, base: PathBuf) {
        self.start_job(TransferKind::Checksum, Clashes::Ask, move |progress| run_compute(sources, base, progress));
    }

    /// Check the files a checksum file names against it.
    pub fn start_check(&mut self, file: PathBuf) {
        self.start_job(TransferKind::Checksum, Clashes::Ask, move |progress| run_check(file, progress));
    }

    /// Make each of `archives` as `how` says, and land on the first once
    /// it is there.
    pub fn start_pack(&mut self, archives: Vec<PackArchive>, how: PackHow) {
        let land = archives.first().map(|archive| archive.dest.clone());
        self.start_job(TransferKind::Pack, Clashes::Ask, move |progress| run_pack(archives, how, progress));
        if let Some(job) = &mut self.workers.job {
            job.after = land.map(AfterJob::Land);
        }
    }

    /// Write `edits` into `archive` - moving in, when `move_after`, what is
    /// put in - names it already has going by `clashes`; then land on
    /// `land`, when there is one.
    pub fn start_repack(&mut self, archive: PathBuf, edits: crate::fs_ops::Repack, move_after: bool, clashes: Clashes, land: Option<PathBuf>) {
        // An encrypted archive is read to be written again, and what goes
        // into it is encrypted with the same password.
        if crate::archive::password(&archive).is_none() && crate::archive::is_encrypted(&archive) {
            let again = archive.clone();
            return self.ask_password(archive, false, Box::new(move |app: &mut AppState| app.start_repack(again, edits, move_after, clashes, land)));
        }
        self.start_job(TransferKind::Update, clashes, move |progress| run_repack(archive, edits, move_after, progress));
        if let Some(job) = &mut self.workers.job {
            job.after = land.map(AfterJob::Land);
        }
    }

    /// Copy entries out of `archive`, each its path inside and where it
    /// goes, then do `after` with what came out.
    pub fn start_extract(&mut self, archive: PathBuf, items: Vec<(PathBuf, PathBuf)>, clashes: Clashes, after: Option<AfterJob>) {
        // Encrypted, and no password yet: asked for first.
        let inners: Vec<PathBuf> = items.iter().map(|(inner, _)| inner.clone()).collect();
        if crate::archive::needs_password(&archive, &inners) {
            let again = archive.clone();
            return self.ask_password(archive, false, Box::new(move |app: &mut AppState| app.start_extract(again, items, clashes, after)));
        }
        self.start_job(TransferKind::Extract, clashes, move |progress| run_extract(archive, items, progress));
        if let Some(job) = &mut self.workers.job {
            job.after = after;
        }
    }

    /// `clashes` says what happens to a file already where one is going:
    /// asked about, written over, or left. A directory onto a directory is
    /// merged whichever it is.
    pub fn start_transfer(&mut self, items: Vec<(PathBuf, PathBuf, bool)>, is_copy: bool, clashes: Clashes) {
        let kind = match &items[..] {
            [(source, dest, false)] if is_copy && crate::fs_ops::joining(source, dest).is_some() => TransferKind::Join,
            _ if is_copy => TransferKind::Copy,
            _ => TransferKind::Move,
        };
        self.start_job(kind, clashes, move |progress| run_transfer(items, is_copy, progress));
    }

    pub fn start_delete(&mut self, items: Vec<(PathBuf, bool)>) {
        self.start_job(TransferKind::Delete, Clashes::Ask, move |progress| run_delete(items, progress));
    }

    pub fn start_trash(&mut self, items: Vec<(PathBuf, bool)>) {
        self.start_job(TransferKind::Trash, Clashes::Ask, move |progress| run_trash(items, progress));
    }

    /// Ask a running transfer to stop. It ends at the next chunk or file, so
    /// the job stays up for a moment afterwards rather than vanishing at once.
    /// One waiting on a question is answered Abort, which is the same thing.
    pub fn cancel_transfer(&mut self) {
        if let Some(job) = &mut self.workers.job {
            job.cancel.store(true, Ordering::Relaxed);
            job.answer(Answer::Abort);
        }
    }

    /// Take whatever the worker has sent since the last frame. Called once per
    /// frame, so the bar advances at the rate the loop already runs at.
    pub fn poll_transfer(&mut self) {
        let Some(job) = &mut self.workers.job else {
            return;
        };

        let mut finished = None;
        loop {
            match job.updates.try_recv() {
                Ok(JobUpdate::Total(amount)) => job.total = Some(amount),
                Ok(JobUpdate::Starting(path)) => job.current = path,
                Ok(JobUpdate::Advanced(amount)) => job.done += amount,
                // Asked after Esc, which answered nothing then: there was no
                // question on screen yet. Esc is the answer, so it is not shown.
                Ok(JobUpdate::Problem(_) | JobUpdate::Clash(_) | JobUpdate::Room(_) | JobUpdate::TooBig(_)) if job.is_cancelling() => {
                    let _ = job.answers.send(Answer::Abort);
                }
                Ok(JobUpdate::Problem(problem)) => job.problem = Some(problem),
                Ok(JobUpdate::Clash(clash)) => job.clash = Some(clash),
                Ok(JobUpdate::Room(short)) => job.room = Some(short),
                Ok(JobUpdate::TooBig(too_big)) => job.too_big = Some(too_big),
                Ok(JobUpdate::Skipped) => job.skipped += 1,
                Ok(JobUpdate::Checksums(report)) => job.checksums = Some(report),
                Ok(JobUpdate::Did(done)) => job.did.push(done),
                Ok(JobUpdate::Duplicates(found)) => job.duplicates = Some(found),
                Ok(JobUpdate::Ghosts(found)) => job.ghosts = Some(found),
                Ok(JobUpdate::Tree(found)) => job.tree = Some(found),
                Ok(JobUpdate::Dossier(found)) => job.dossier = Some(found),
                Ok(JobUpdate::Finished(result)) => {
                    finished = Some(result);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                // Gone without a word, which only happens if it panicked.
                Err(TryRecvError::Disconnected) => {
                    finished = Some(Err(t!("Transfer stopped unexpectedly").to_string()));
                    break;
                }
            }
        }

        let Some(result) = finished else {
            return;
        };
        let mut job = self.workers.job.take().expect("checked at the top");
        self.count_scores(&job, &result);
        // Whatever was done, the job finished or not.
        let undo_kind = match job.kind {
            TransferKind::Copy | TransferKind::Join => Some(UndoKind::Copy),
            TransferKind::Move => Some(UndoKind::Move),
            TransferKind::Trash => Some(UndoKind::Trash),
            TransferKind::DirEdit => Some(UndoKind::Edit),
            _ => None,
        };
        if matches!(job.kind, TransferKind::Copy | TransferKind::Join | TransferKind::Move) {
            self.flash_done(&job.did);
        }
        if let Some(kind) = undo_kind {
            self.journal(kind, std::mem::take(&mut job.did));
        }

        // An entry copied out to be looked at changed nothing anyone can
        // see: the panels, their selections and sizes stay as they are.
        if !matches!(job.kind, TransferKind::Checksum | TransferKind::Duplicates | TransferKind::Dossier | TransferKind::Ghosts | TransferKind::Tree) && job.after.as_ref().is_none_or(|after| !after.to_scratch()) {
            // Both panels: a move empties one and fills the other, and a copy
            // into the same directory shows up on the side it came from.
            self.reload_panel(Side::Left, None);
            self.reload_panel(Side::Right, None);
            self.clear_active_selections();
            // Any directory size worked out before may now be wrong - the ones
            // copied into, moved out of or deleted from, and every directory
            // above them. Space works them out again. One still being counted
            // may have walked through them halfway, so it goes too.
            self.dir_sizes.clear();
            self.cancel_dir_sizes();
        }

        // What `*` read is kept, stopped or not.
        if let Some(found) = job.tree.take() {
            self.take_tree(found);
        }

        // The offer to quit covered this job, and this job is over.
        self.quit_armed = false;

        if let (Ok(Transfer::Done), Some(report)) = (&result, job.checksums) {
            self.dialog = Some(Dialog::Checksums(Box::new(ChecksumView { report: *report, scroll: 0, room: 1 })));
            return;
        }
        if let (Ok(Transfer::Done), Some(dossier)) = (&result, job.dossier.take()) {
            self.dialog = Some(Dialog::Dossier(Box::new(DossierView { dossier, scroll: 0, room: 1 })));
            return;
        }
        // Fed to the panel; anything that could not be read is said over it.
        if let (Ok(Transfer::Done), Some(found)) = (&result, job.duplicates.take()) {
            self.feed_duplicates(found);
            if job.skipped == 0 {
                return;
            }
        }
        if let (Ok(Transfer::Done), Some(found)) = (&result, job.ghosts.take()) {
            self.feed_ghosts(found);
            if job.skipped == 0 {
                return;
            }
        }
        match (&result, job.after) {
            (Ok(Transfer::Done), Some(AfterJob::View(path))) if job.skipped == 0 => return self.request_open(path, false),
            (Ok(Transfer::Done), Some(AfterJob::Open(path))) if job.skipped == 0 => return self.open_outside(&path, false),
            (Ok(Transfer::Done), Some(AfterJob::OpenWith(path, opener))) if job.skipped == 0 => return self.launch(&opener, &path),
            (Ok(Transfer::Done), Some(AfterJob::Land(archive))) => {
                for side in Side::BOTH {
                    if archive.parent() == Some(self.panel_dir(side).as_path()) {
                        self.reload_panel(side, archive.file_name());
                    }
                }
            }
            (Ok(Transfer::Done), Some(AfterJob::Compare { files, shown, then })) if job.skipped == 0 => {
                match then {
                    Some((archive, items)) => self.start_extract(archive, items, Clashes::ReplaceAll, Some(AfterJob::Compare { files, shown, then: None })),
                    None => self.compare_shown(files, shown),
                }
                return;
            }
            _ => {}
        }
        match result {
            Ok(Transfer::Done) if job.skipped > 0 => {
                let (job_title, count) = (job.kind.title(), job.skipped);
                self.display_error(tn!(count, "{job_title} finished, {count} entry skipped", "{job_title} finished, {count} entries skipped", job_title, count))
            }
            Ok(Transfer::Done) => {}
            Ok(Transfer::Cancelled) => {
                let (job_title, done) = (job.kind.title(), job.done);
                self.display_error(match job.kind {
                    TransferKind::Delete => tn!(done, "{job_title} cancelled after {done} entry", "{job_title} cancelled after {done} entries", job_title, done),
                    TransferKind::Trash | TransferKind::Undo | TransferKind::DirEdit | TransferKind::Restore | TransferKind::Tree => tn!(done, "{job_title} cancelled after {done} item", "{job_title} cancelled after {done} items", job_title, done),
                    _ => t!("{job_title} cancelled after {size}", job_title, size = crate::utils::format_size(done)),
                })
            }
            Err(e) => self.display_error(e),
        }
    }
}

#[cfg(test)]
mod job_tests {
    use super::AppState;
    use super::{Clash, replace_if_older};
    use std::time::{Duration, SystemTime};

    /// "Overwrite all older" takes times within two seconds for the same, as
    /// Shift+F2 and Alt+S do: FAT keeps them to two seconds and NTFS to a
    /// tenth of a microsecond, so a file copied there and back would always
    /// look older, and be copied again for nothing.
    #[test]
    fn older_is_older_by_more_than_a_filesystem_rounds_off() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(replace_if_older(Some(at + Duration::from_millis(1_900)), Some(at)), Clash::Keep, "FAT's rounding");
        assert_eq!(replace_if_older(Some(at + Duration::from_nanos(99)), Some(at)), Clash::Keep, "NTFS's");
        assert_eq!(replace_if_older(Some(at + Duration::from_secs(3)), Some(at)), Clash::Replace, "older indeed");
        assert_eq!(replace_if_older(Some(at), Some(at + Duration::from_secs(3))), Clash::Keep, "newer");
        assert_eq!(replace_if_older(None, Some(at)), Clash::Keep, "not known");
    }

    #[test]
    fn a_finished_job_forgets_directory_sizes() {
        let dir = std::env::temp_dir().join(format!("fm84-sizes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("doomed"), vec![0u8; 100]).unwrap();

        let mut app_state = AppState::new();
        app_state.dir_sizes.insert(dir.clone(), 100);
        app_state.start_delete(vec![(dir.join("doomed"), false)]);
        let started = std::time::Instant::now();
        while app_state.workers.job.is_some() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "delete never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_state.poll_transfer();
        }
        assert!(app_state.dir_sizes.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
