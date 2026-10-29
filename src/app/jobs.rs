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
    answers: Sender<Answer>,
    /// Entries left where they were, by Skip or Skip all, to say so at the end.
    pub skipped: u64,
    /// What to do with what was copied out, once it all is.
    after: Option<AfterJob>,
    /// What a checksum job found, shown once it is done.
    checksums: Option<Box<crate::checksum::Report>>,
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
    /// Compare the two files, once out, shown under the paths they have in
    /// the panels. `then` is a second archive to copy out of first, when the
    /// two are in different ones.
    Compare {
        files: (PathBuf, PathBuf),
        shown: (PathBuf, PathBuf),
        then: Option<(PathBuf, Vec<(PathBuf, PathBuf)>)>,
    },
    /// Not from an archive: an archive packed, landed on in whichever panel
    /// shows where it went.
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
    /// Working out checksums, or checking them.
    Checksum,
}

impl TransferKind {
    /// The word the popup and the cancelled message use.
    pub fn title(self) -> &'static str {
        match self {
            TransferKind::Copy => "Copy",
            TransferKind::Move => "Move",
            TransferKind::Delete => "Delete",
            TransferKind::Trash => "Move to trash",
            TransferKind::Extract => "Extract",
            TransferKind::Pack => "Pack",
            TransferKind::Checksum => "Checksum",
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
        if self.problem.take().is_some() || self.clash.take().is_some() {
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
}

/// What the worker sends back. Bytes are per chunk rather than a running total,
/// so a message that arrives late still counts once.
enum JobUpdate {
    Total(u64),
    Starting(PathBuf),
    Advanced(u64),
    /// Stuck on this entry until an Answer comes back.
    Problem(JobProblem),
    /// Stuck on a file in the way until an Answer comes back.
    Clash(JobClash),
    /// One more entry left where it was.
    Skipped,
    /// What a checksum job found.
    Checksums(Box<crate::checksum::Report>),
    Finished(Result<Transfer, String>),
}

/// The worker's side of a job: passes progress to the UI, and when an entry
/// fails, asks there what to do and waits for the answer.
struct JobProgress<'a> {
    pub(super) updates: &'a Sender<JobUpdate>,
    pub(super) cancel: &'a AtomicBool,
    answers: &'a Receiver<Answer>,
    skip_all: bool,
    pub(super) clashes: Clashes,
    /// Set once the answer was Abort, so the error it ends the job with is
    /// reported as the cancel it was rather than as a failure all over again.
    aborted: bool,
}

impl<'a> JobProgress<'a> {
    fn new(updates: &'a Sender<JobUpdate>, cancel: &'a AtomicBool, answers: &'a Receiver<Answer>, clashes: Clashes) -> Self {
        JobProgress {
            updates,
            cancel,
            answers,
            skip_all: false,
            clashes,
            aborted: false,
        }
    }

    /// The message that ends the job, given how its last item came out.
    fn finish(&self, outcome: Result<Transfer, std::io::Error>) {
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
            // Answers to a clash, which a failure is never asked.
            Answer::Overwrite | Answer::OverwriteAll | Answer::OverwriteOlder | Answer::KeepBoth | Answer::KeepBothAll => Reply::Skip,
        }
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
                    Answer::Abort | Answer::Retry | Answer::Delete => Clash::Abort,
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

/// Overwrite older: write over what is there only when it is older than what
/// would replace it. When either time cannot be read, it is left alone - it
/// cannot be shown to be older, and what is left can still be copied again.
fn replace_if_older(source: Option<SystemTime>, dest: Option<SystemTime>) -> Clash {
    match (source, dest) {
        (Some(source), Some(dest)) if dest < source => Clash::Replace,
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
        match crate::fs_ops::trash_path(path, is_dir, &mut progress) {
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
fn run_compute(sources: Vec<(PathBuf, bool)>, base: PathBuf, algorithm: crate::checksum::Algorithm, mut progress: JobProgress<'_>) {
    let (targets, unreadable) = crate::checksum::targets(&sources, &base);
    let _ = progress.updates.send(JobUpdate::Total(targets.iter().map(|target| target.size).sum()));
    let Some(mut sums) = crate::checksum::compute(&targets, algorithm, &mut progress) else {
        return progress.finish(Ok(Transfer::Cancelled));
    };
    sums.extend(unreadable.into_iter().map(|(name, error)| (name, Err(error))));
    let _ = progress.updates.send(JobUpdate::Checksums(Box::new(crate::checksum::Report::Computed { algorithm, base, sources, sums })));
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

/// The worker thread behind packing: what goes in is found first, which
/// gives the bar its total, and then written.
fn run_pack(sources: Vec<(PathBuf, bool)>, dest: PathBuf, format: crate::fs_ops::PackFormat, replacing: bool, mut progress: JobProgress<'_>) {
    let outcome = match crate::fs_ops::plan_pack(&sources, &dest, format, &mut progress) {
        Ok(Some(plan)) => {
            let _ = progress.updates.send(JobUpdate::Total(plan.bytes));
            crate::fs_ops::pack(&plan, &dest, format, replacing, &mut progress)
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
        if !is_copy && !path_exists(&dest) && rename_in_place(&source, &dest) {
            continue;
        }
        remaining.push((source, dest, is_dir));
    }

    // Only what is actually going to be copied needs counting.
    let _ = progress.updates.send(JobUpdate::Total(measure(&remaining)));

    for (source, dest, is_dir) in remaining {
        let result = if is_copy { copy_path(source, dest, is_dir, &mut progress) } else { move_path(source, dest, &mut progress) };
        match result {
            Ok(Transfer::Done) => {}
            outcome => return progress.finish(outcome),
        }
    }
    progress.finish(Ok(Transfer::Done));
}

impl AppState {
    /// Hand a long job to a worker thread and start following it.
    fn start_job<F>(&mut self, kind: TransferKind, clashes: Clashes, work: F)
    where
        F: FnOnce(JobProgress<'_>) + Send + 'static,
    {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, updates) = mpsc::channel();
        let (answers, worker_answers) = mpsc::channel();
        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || work(JobProgress::new(&sender, &worker_cancel, &worker_answers, clashes)));

        self.job = Some(TransferJob {
            kind,
            total: None,
            done: 0,
            current: PathBuf::new(),
            started: Instant::now(),
            cancel,
            updates,
            problem: None,
            clash: None,
            answers,
            skipped: 0,
            after: None,
            checksums: None,
        });
    }

    /// Work out the checksums of `sources`, each named from `base`, and show
    /// them once they are all in.
    pub fn start_checksums(&mut self, sources: Vec<(PathBuf, bool)>, base: PathBuf, algorithm: crate::checksum::Algorithm) {
        self.start_job(TransferKind::Checksum, Clashes::Ask, move |progress| run_compute(sources, base, algorithm, progress));
    }

    /// Check the files a checksum file names against it.
    pub fn start_check(&mut self, file: PathBuf) {
        self.start_job(TransferKind::Checksum, Clashes::Ask, move |progress| run_check(file, progress));
    }

    /// Pack `sources` into a new archive at `dest`, written over only when
    /// `replacing`, and land on it once it is there.
    pub fn start_pack(&mut self, sources: Vec<(PathBuf, bool)>, dest: PathBuf, format: crate::fs_ops::PackFormat, replacing: bool) {
        let land = dest.clone();
        self.start_job(TransferKind::Pack, Clashes::Ask, move |progress| run_pack(sources, dest, format, replacing, progress));
        if let Some(job) = &mut self.job {
            job.after = Some(AfterJob::Land(land));
        }
    }

    /// Copy entries out of `archive`, each its path inside and where it
    /// goes, then do `after` with what came out.
    pub fn start_extract(&mut self, archive: PathBuf, items: Vec<(PathBuf, PathBuf)>, clashes: Clashes, after: Option<AfterJob>) {
        self.start_job(TransferKind::Extract, clashes, move |progress| run_extract(archive, items, progress));
        if let Some(job) = &mut self.job {
            job.after = after;
        }
    }

    /// `clashes` says what happens to a file already where one is going:
    /// asked about, written over, or left. A directory onto a directory is
    /// merged whichever it is.
    pub fn start_transfer(&mut self, items: Vec<(PathBuf, PathBuf, bool)>, is_copy: bool, clashes: Clashes) {
        let kind = if is_copy { TransferKind::Copy } else { TransferKind::Move };
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
        if let Some(job) = &mut self.job {
            job.cancel.store(true, Ordering::Relaxed);
            job.answer(Answer::Abort);
        }
    }

    /// Take whatever the worker has sent since the last frame. Called once per
    /// frame, so the bar advances at the rate the loop already runs at.
    pub fn poll_transfer(&mut self) {
        let Some(job) = &mut self.job else {
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
                Ok(JobUpdate::Problem(_) | JobUpdate::Clash(_)) if job.is_cancelling() => {
                    let _ = job.answers.send(Answer::Abort);
                }
                Ok(JobUpdate::Problem(problem)) => job.problem = Some(problem),
                Ok(JobUpdate::Clash(clash)) => job.clash = Some(clash),
                Ok(JobUpdate::Skipped) => job.skipped += 1,
                Ok(JobUpdate::Checksums(report)) => job.checksums = Some(report),
                Ok(JobUpdate::Finished(result)) => {
                    finished = Some(result);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                // Gone without a word, which only happens if it panicked.
                Err(TryRecvError::Disconnected) => {
                    finished = Some(Err("Transfer stopped unexpectedly".to_string()));
                    break;
                }
            }
        }

        let Some(result) = finished else {
            return;
        };
        let job = self.job.take().expect("checked at the top");

        // An entry copied out to be looked at changed nothing anyone can
        // see: the panels, their selections and sizes stay as they are.
        if job.kind != TransferKind::Checksum && job.after.as_ref().is_none_or(|after| !after.to_scratch()) {
            // Both panels: a move empties one and fills the other, and a copy
            // into the same directory shows up on the side it came from.
            self.reload_panel(true, None);
            self.reload_panel(false, None);
            self.clear_active_selections();
            // Any directory size worked out before may now be wrong - the ones
            // copied into, moved out of or deleted from, and every directory
            // above them. Space works them out again. One still being counted
            // may have walked through them halfway, so it goes too.
            self.dir_sizes.clear();
            self.cancel_dir_sizes();
        }

        // The offer to quit covered this job, and this job is over.
        self.quit_armed = false;

        if let (Ok(Transfer::Done), Some(report)) = (&result, job.checksums) {
            self.dialog = Some(Dialog::Checksums(Box::new(ChecksumView { report: *report, scroll: 0, room: 1 })));
            return;
        }
        match (&result, job.after) {
            (Ok(Transfer::Done), Some(AfterJob::View(path))) if job.skipped == 0 => return self.request_open(path, false),
            (Ok(Transfer::Done), Some(AfterJob::Open(path))) if job.skipped == 0 => {
                if let Err(e) = crate::input::open_with_default(&path) {
                    self.display_error(format!("Cannot open file: {e}"));
                }
                return;
            }
            (Ok(Transfer::Done), Some(AfterJob::Land(archive))) => {
                for is_left in [true, false] {
                    if archive.parent() == Some(self.panel_dir(is_left).as_path()) {
                        self.reload_panel(is_left, archive.file_name());
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
                let entries = if job.skipped == 1 { "entry" } else { "entries" };
                self.display_error(format!("{} finished, {} {entries} skipped", job.kind.title(), job.skipped))
            }
            Ok(Transfer::Done) => {}
            Ok(Transfer::Cancelled) => {
                let far = match job.kind {
                    TransferKind::Delete => format!("{} entries", job.done),
                    TransferKind::Trash => format!("{} items", job.done),
                    _ => crate::utils::format_size(job.done),
                };
                self.display_error(format!("{} cancelled after {far}", job.kind.title()))
            }
            Err(e) => self.display_error(e),
        }
    }
}

#[cfg(test)]
mod job_tests {
    use super::AppState;

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
        while app_state.job.is_some() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "delete never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_state.poll_transfer();
        }
        assert!(app_state.dir_sizes.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
