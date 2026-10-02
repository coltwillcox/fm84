use crate::fs_ops::{
    Mount, Progress, Reply, Step, Transfer, copy_path, count_entries, delete_path, disk_usage, get_current_dir, list_mounts,
    load_directory_rows, measure, move_path, nearest_existing_dir, path_exists, rename_in_place,
};
use crate::options::{OPTION_ROWS, OptionRow, Options};
use image::DynamicImage;
use crate::viewer::{ViewMode, ViewerState};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::TableState;
use std::ffi::OsString;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Instant, SystemTime};

/// Reusable single-line text input with cursor.
pub struct TextInput {
    pub text: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn new() -> Self {
        Self { text: String::new(), cursor: 0 }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn set(&mut self, value: String) {
        self.cursor = value.chars().count();
        self.text = value;
    }

    fn byte_index(&self) -> usize {
        self.text.char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        let len = self.text.chars().count();
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.chars().count();
    }

    pub fn insert(&mut self, c: char) {
        let idx = self.byte_index();
        self.text.insert(idx, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let byte_start = self.text.char_indices()
                .nth(self.cursor - 1)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let byte_end = self.byte_index();
            self.text.replace_range(byte_start..byte_end, "");
            self.cursor -= 1;
        }
    }

    pub fn delete_forward(&mut self) {
        let len = self.text.chars().count();
        if self.cursor < len {
            let byte_start = self.byte_index();
            let byte_end = self.text.char_indices()
                .nth(self.cursor + 1)
                .map(|(i, _)| i)
                .unwrap_or(self.text.len());
            self.text.replace_range(byte_start..byte_end, "");
        }
    }

    /// Styled spans with a block cursor, windowed to `width` columns so the
    /// cursor stays in view. A name longer than the field it is typed into
    /// would otherwise run past the edge and be clipped there, taking the
    /// cursor with it: you would be typing somewhere you could not see.
    pub fn cursor_spans_within(&self, width: usize, text_style: Style, cursor_style: Style) -> Vec<Span<'static>> {
        if width == 0 {
            return Vec::new();
        }

        let cell = |character: char| crate::utils::display_width(&character.to_string()).max(1);
        let characters: Vec<char> = self.text.chars().collect();
        let cursor = self.cursor.min(characters.len());
        let at_cursor = characters.get(cursor).copied().unwrap_or(' ');

        // A cursor past the end still needs a cell of its own to sit in.
        let whole: usize = characters.iter().map(|&c| cell(c)).sum::<usize>() + 1;

        // Everything fits: start at the beginning. Otherwise scroll so the
        // cursor is at the right-hand edge, which is where typing keeps it.
        let mut first = 0;
        if whole > width {
            let mut used = cell(at_cursor);
            first = cursor;
            while first > 0 && used + cell(characters[first - 1]) <= width {
                first -= 1;
                used += cell(characters[first]);
            }
        }

        let before: String = characters[first..cursor].iter().collect();
        let mut used = crate::utils::display_width(&before) + cell(at_cursor);
        let mut after = String::new();
        for &character in characters.iter().skip(cursor + 1) {
            let this = cell(character);
            if used + this > width {
                break;
            }
            used += this;
            after.push(character);
        }

        // A rename field is opened on the name that is there, and a name can
        // hold anything; the width is unchanged, a control character standing
        // in one column as the '.' that replaces it does.
        vec![
            Span::styled(crate::utils::printable_name(&before), text_style),
            Span::styled(crate::utils::printable_name(&at_cursor.to_string()), cursor_style),
            Span::styled(crate::utils::printable_name(&after), text_style),
        ]
    }

    /// Styled spans with a block cursor at the cursor position, for a field
    /// wide enough to hold whatever is typed into it.
    pub fn cursor_spans(&self, text_style: Style, cursor_style: Style) -> Vec<Span<'static>> {
        let byte_idx = self.byte_index();
        let before = self.text[..byte_idx].to_string();
        let rest = &self.text[byte_idx..];
        let mut chars = rest.chars();
        let cursor_char = chars.next().unwrap_or(' ');
        let after: String = chars.collect();

        vec![
            Span::styled(crate::utils::printable_name(&before), text_style),
            Span::styled(crate::utils::printable_name(&cursor_char.to_string()), cursor_style),
            Span::styled(crate::utils::printable_name(&after), text_style),
        ]
    }
}

/// What the prompt in the status bar is asking for: in the viewer or the
/// editor, something to find or a line to go to; in a panel, a pattern to
/// select or deselect by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromptKind {
    Find,
    GoToLine,
    Select,
    Deselect,
}

pub struct AppState {
    pub is_error_displayed: bool,
    pub is_f1_displayed: bool,
    pub is_f11_displayed: bool,
    /// What F11 sets, as loaded from the config file at startup.
    pub options: Options,
    /// The row the options popup has highlighted.
    pub options_cursor: usize,
    /// Set while a text option is being typed into; `options_input` holds it.
    pub options_editing: bool,
    pub options_input: TextInput,
    /// A file F4 is handing to an external editor. The main loop runs it,
    /// since only that can give the terminal up and take it back.
    pub external_edit: Option<PathBuf>,
    pub is_f12_displayed: bool,
    pub preview: Option<PreviewState>,
    /// Editor clipboard. Internal, so it works in a bare TTY too.
    pub clipboard: String,
    /// Mounts offered in the top strip, refreshed on the same tick as disk usage.
    pub mounts: Vec<Mount>,
    /// Which panel is choosing a drive, and which mount it has highlighted.
    pub drive_picker: Option<(bool, usize)>,
    pub is_f2_displayed: bool,
    pub is_f7_displayed: bool,
    /// The create dialog makes a directory (F7) or an empty file (Shift+F4).
    pub create_is_dir: bool,
    pub is_left_active: bool,
    pub dir_left: PathBuf,
    pub dir_right: PathBuf,
    pub page_size: u16,
    pub state_left: TableState,
    pub state_right: TableState,
    pub children_left: Vec<Item>,
    pub children_right: Vec<Item>,
    pub error_message: String,
    pub rename_input: TextInput,
    pub create_input: TextInput,
    pub is_f8_displayed: bool,
    /// What F8 is asking about, as paths: the name alone could not be joined
    /// back to the file it came from when it is not valid UTF-8.
    pub delete_items: Vec<(PathBuf, bool)>,
    pub search_input: String,
    pub cached_clock: String,
    pub cached_separator_height: u16,
    pub cached_separator: String,
    pub is_f3_displayed: bool,
    pub viewer_state: Option<ViewerState>,
    pub viewer_viewport_height: usize,
    pub viewer_viewport_width: usize,
    pub is_f4_displayed: bool,
    pub editor_state: Option<EditorState>,
    pub editor_viewport_height: usize,
    /// The line at the foot of the screen, while it is asking for something
    /// to find, a line to go to or a pattern to select by.
    pub prompt: Option<(PromptKind, TextInput)>,
    /// What + or - last selected by, offered again the next time.
    pub select_pattern: String,
    /// What Ctrl+F last looked for. Kept when the prompt closes, for F3 and
    /// Shift+F3, and offered again the next time it opens.
    pub find_term: String,
    /// Whether the matches of find_term are marked on screen. On from the
    /// first search until the viewer or editor closes.
    pub find_shown: bool,
    /// What the last search came to - "Not found", or that it went round the
    /// end - shown in the status bar until the next key.
    pub find_note: Option<String>,
    pub is_f5_displayed: bool,
    pub copy_items: Vec<(PathBuf, PathBuf, bool)>,
    pub is_f6_displayed: bool,
    pub move_items: Vec<(PathBuf, PathBuf, bool)>,
    // Keyed by file name, not row index: a reload can re-sort the rows, and an
    // index would then point at a different file than the one the user picked.
    // The name the filesystem holds, not the one shown: two names that are not
    // valid UTF-8 can read the same, and picking one would pick both.
    pub selected_left: HashSet<OsString>,
    pub selected_right: HashSet<OsString>,
    pub dir_sizes: HashMap<PathBuf, u64>,
    pub last_click_time: Option<Instant>,
    pub last_click_pos: (u16, u16),
    pub is_editor_save_prompt: bool,
    // Where the last frame actually drew things. Mouse handling reads these
    // instead of recomputing the layout from hardcoded row numbers.
    pub table_area_left: Rect,
    pub table_area_right: Rect,
    pub viewport_start_left: usize,
    pub viewport_start_right: usize,
    pub editor_content_area: Rect,
    pub viewer_content_area: Rect,
    // The drive strips as drawn, and where each icon sits along one. Both
    // strips lay their icons out the same way, so one set of slots serves both.
    pub drive_strip_left: Rect,
    pub drive_strip_right: Rect,
    pub drive_slots: Vec<(u16, u16)>,
    // Directory mtimes as of the last load, so an external change can be spotted
    // without stat-ing every entry.
    pub dir_stamp_left: Option<SystemTime>,
    pub dir_stamp_right: Option<SystemTime>,
    pub last_refresh_check: Instant,
    // (used, total) bytes for each panel's filesystem.
    pub disk_left: Option<(u64, u64)>,
    pub disk_right: Option<(u64, u64)>,
    /// Set while the prompt for an expensive file is up.
    pub large_file: Option<LargeFile>,
    /// Set while asking whether a copy or move may write over what is there.
    pub overwrite_prompt: Option<OverwritePrompt>,
    /// Set while a copy, move or delete is running on its own thread.
    pub job: Option<TransferJob>,
    /// Directories Space is working out the size of, each on its own thread.
    pub dir_sizing: Vec<DirSizing>,
    /// Pictures read around the one on screen, while F11 leaves it on.
    pub image_cache: ImageCache,
    /// The entry under the cursor, spelled out under the panels.
    pub cursor_detail: Option<CursorDetail>,
    /// F10 was pressed during a job. The job carries on; a second press is
    /// what leaves. Cleared when the job ends, so it only ever covers one.
    pub quit_armed: bool,
}

/// A copy or move running on a worker thread, and what it has told us so far.
/// The work is off the UI thread because a single write to a stalled network
/// mount can block for seconds, and that is exactly when a progress bar and a
/// way out of it are wanted.
pub struct TransferJob {
    pub kind: TransferKind,
    /// None until the counting pass has something to report. Counted in bytes
    /// for a copy or move, and in entries removed for a delete.
    pub total: Option<u64>,
    pub done: u64,
    pub current: PathBuf,
    pub started: Instant,
    /// Set by Esc, read by the worker between entries.
    cancel: Arc<AtomicBool>,
    updates: Receiver<JobUpdate>,
    /// An entry the worker could not deal with, waiting on Retry, Skip, Skip
    /// all or Abort. The worker sits still until it hears which.
    pub problem: Option<JobProblem>,
    answers: Sender<Answer>,
    /// Entries left where they were, by Skip or Skip all, to say so at the end.
    pub skipped: u64,
}

/// Which of the three long jobs is running. They share a popup, a worker and a
/// way out, and differ only in what they count and what they are called.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransferKind {
    Copy,
    Move,
    Delete,
}

impl TransferKind {
    /// The word the popup and the cancelled message use.
    pub fn title(self) -> &'static str {
        match self {
            TransferKind::Copy => "Copy",
            TransferKind::Move => "Move",
            TransferKind::Delete => "Delete",
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

    /// Answer the problem on screen, letting the worker go on.
    pub fn answer(&mut self, answer: Answer) {
        if self.problem.take().is_some() {
            let _ = self.answers.send(answer);
        }
    }
}

/// What the popup shows while the worker waits for an answer.
pub struct JobProblem {
    pub path: PathBuf,
    pub message: String,
}

/// The answer to a JobProblem, sent back to the worker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Retry,
    Skip,
    /// Skip this one and every failure after it without asking.
    SkipAll,
    Abort,
}

/// What the worker sends back. Bytes are per chunk rather than a running total,
/// so a message that arrives late still counts once.
enum JobUpdate {
    Total(u64),
    Starting(PathBuf),
    Advanced(u64),
    /// Stuck on this entry until an Answer comes back.
    Problem(JobProblem),
    /// One more entry left where it was.
    Skipped,
    Finished(Result<Transfer, String>),
}

/// The worker's side of a job: passes progress to the UI, and when an entry
/// fails, asks there what to do and waits for the answer.
struct JobProgress<'a> {
    updates: &'a Sender<JobUpdate>,
    cancel: &'a AtomicBool,
    answers: &'a Receiver<Answer>,
    skip_all: bool,
    /// Set once the answer was Abort, so the error it ends the job with is
    /// reported as the cancel it was rather than as a failure all over again.
    aborted: bool,
}

impl<'a> JobProgress<'a> {
    fn new(updates: &'a Sender<JobUpdate>, cancel: &'a AtomicBool, answers: &'a Receiver<Answer>) -> Self {
        JobProgress { updates, cancel, answers, skip_all: false, aborted: false }
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
            let problem = JobProblem { path: path.to_path_buf(), message: error.to_string() };
            // Nobody left to ask, or nobody answering, is an Abort.
            if self.updates.send(JobUpdate::Problem(problem)).is_err() {
                Answer::Abort
            } else {
                self.answers.recv().unwrap_or(Answer::Abort)
            }
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
        }
    }
}

/// A copy or move held back because some of its names are taken, waiting for
/// the answer to whether it may write over them.
pub struct OverwritePrompt {
    pub items: Vec<(PathBuf, PathBuf, bool)>,
    pub is_copy: bool,
    /// The destinations already taken, to name in the question.
    pub taken: Vec<PathBuf>,
}

/// A directory whose size Space asked for, being walked on its own thread.
/// Not a TransferJob: nothing waits on it, so it has no popup and holds up
/// nothing. The panels go on working while it counts, and the Size column
/// shows how far it has got.
///
/// Off the UI thread because walking / or a large tree takes seconds, and
/// on an unresponsive mount a single read_dir can take forever.
pub struct DirSizing {
    pub path: PathBuf,
    /// Bytes found so far, for the Size column to show while it counts.
    pub found: u64,
    cancel: Arc<AtomicBool>,
    updates: Receiver<SizeUpdate>,
}

enum SizeUpdate {
    Found(u64),
    /// None when cancelled.
    Finished(Result<Option<u64>, String>),
}

/// What a read-ahead delivers: the picture and the label the status bar shows
/// for it, or nothing if the file turned out not to be one after all.
type Decoded = Option<(DynamicImage, String)>;

/// Pictures decoded around the one being looked at, so stepping through a
/// folder does not stop to decode each one.
///
/// Only the decoded picture is held, never the file's bytes: decoding is the
/// slow part, and a decoded picture is capped at IMAGE_MAX_SIDE, so two of
/// them come to a few megabytes however large the files were. The one just
/// left costs nothing to keep - it is already decoded - and the one ahead is
/// read on a thread, so a slow disk never shows up as a pause.
#[derive(Default)]
pub struct ImageCache {
    ready: Vec<(PathBuf, DynamicImage, String)>,
    /// The read that is running, and where it will arrive.
    pending: Option<(PathBuf, Receiver<Decoded>)>,
    /// Which way the last step went, so the right side is read ahead.
    forward: bool,
}

impl ImageCache {
    /// Both neighbours at most: one behind, one ahead.
    const KEEP: usize = 2;

    fn take(&mut self, path: &Path) -> Option<(DynamicImage, String)> {
        let at = self.ready.iter().position(|(held, _, _)| held == path)?;
        let (_, image, label) = self.ready.remove(at);
        Some((image, label))
    }

    fn keep(&mut self, path: PathBuf, image: DynamicImage, label: String) {
        self.ready.retain(|(held, _, _)| *held != path);
        self.ready.push((path, image, label));
        while self.ready.len() > Self::KEEP {
            self.ready.remove(0);
        }
    }

    fn holds(&self, path: &Path) -> bool {
        self.ready.iter().any(|(held, _, _)| held == path)
            || self.pending.as_ref().is_some_and(|(wanted, _)| wanted == path)
    }

    /// Start reading one, unless it is already here or on its way.
    fn request(&mut self, path: PathBuf) {
        if self.holds(&path) {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let wanted = path.clone();
        std::thread::spawn(move || {
            let _ = sender.send(crate::viewer::decode_image(&wanted));
        });
        self.pending = Some((path, receiver));
    }

    /// Take delivery of anything that has finished. Called once a frame.
    pub fn collect(&mut self) {
        let Some((path, receiver)) = &self.pending else {
            return;
        };
        match receiver.try_recv() {
            Ok(Some((image, label))) => {
                let path = path.clone();
                self.pending = None;
                self.keep(path, image, label);
            }
            // Not a picture after all, or the thread is gone: stop waiting.
            Ok(None) | Err(TryRecvError::Disconnected) => self.pending = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    fn clear(&mut self) {
        self.ready.clear();
        self.pending = None;
    }
}

/// What the two lines under the panels say about the entry the cursor is on.
/// Held rather than worked out while drawing, because gathering it stats the
/// file, and a stat on an unresponsive mount would take the whole frame with it.
pub struct CursorDetail {
    /// What it describes, so it is only gathered again when the cursor moves.
    pub path: Option<PathBuf>,
    pub name: String,
    pub size: String,
    pub modified: String,
    pub owner: String,
    pub attributes: String,
    pub link: Option<String>,
}

/// A file big enough to be worth asking about before it is opened.
pub struct LargeFile {
    pub path: PathBuf,
    pub is_edit: bool,
    /// Memory it will take once open.
    pub size: u64,
    /// Set for a picture, whose `size` is what it unpacks to rather than what it
    /// occupies on disk - a small file can hold an enormous number of pixels.
    pub dimensions: Option<(u32, u32)>,
}

#[derive(Clone)]
pub struct EditorState {
    pub file_path: PathBuf,
    pub lines: Vec<String>,
    pub highlighted_lines: Vec<Vec<Span<'static>>>,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub scroll_offset: usize,
    pub horizontal_offset: usize,
    pub modified: bool,
    pub auto_scroll: bool,
    /// Where a selection began. The selection runs from here to the cursor.
    pub selection_anchor: Option<(usize, usize)>,
    /// Edits that can be undone, oldest first.
    pub undo_stack: Vec<EditStep>,
    /// Undone edits, ready to be put back. Cleared by any fresh edit.
    pub redo_stack: Vec<EditStep>,
    /// Parser state entering each line, so an edit re-parses from that line
    /// instead of the whole file. Empty when highlighting is off.
    pub line_states: Vec<crate::viewer::LineState>,
    /// The terminator this file was written with, so saving doesn't rewrite
    /// every line of a CRLF file just because one character changed.
    pub line_ending: &'static str,
}

/// One undoable edit: the lines it replaced, and where the cursor was. The
/// number of lines that took their place is worked out at undo time from how
/// the buffer's length changed, so nothing has to be recorded afterwards.
#[derive(Clone)]
pub struct EditStep {
    first_line: usize,
    before: Vec<String>,
    total_lines_before: usize,
    cursor: (usize, usize),
}

/// What the opposite panel is showing while preview mode is on.
pub struct PreviewState {
    /// None for the parent entry, which has nothing to show but still holds
    /// the pane so it doesn't flicker back to a table as the cursor passes.
    pub path: Option<PathBuf>,
    pub label: String,
    pub lines: Vec<String>,
    /// The head of a binary file, laid out as a hexdump by whatever draws it.
    /// Empty for everything else, which comes as lines.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Item {
    /// The name the filesystem holds. On Unix that is bytes, which need not be
    /// valid UTF-8 - a file from an old archive or a foreign disk often is not.
    /// Every path an operation works on is built from this, never from the
    /// strings beside it: those come from `to_string_lossy`, which puts a
    /// replacement character where it could not read one, and a path joined
    /// from that names a file that does not exist.
    pub name_os: OsString,
    /// The whole name as it is shown and sorted, searched and selected by.
    pub name_full: String,
    pub name: String,
    pub extension: String,
    pub is_dir: bool,
    pub size: String,
    pub size_bytes: u64,
    pub modified_at: Option<SystemTime>,
    pub attributes: String,
}

impl Item {
    /// Where this entry is, inside the directory it was listed from.
    pub fn path_in(&self, dir: &Path) -> PathBuf {
        dir.join(&self.name_os)
    }
}

impl AppState {
    pub fn new() -> Self {
        let mut state_left = TableState::default();
        state_left.select(Some(1));
        let mut state_right = TableState::default();
        state_right.select(Some(1));

        let (is_error_displayed, error_message, dir_root) = match get_current_dir() {
            Ok(root) => (false, String::new(), root),
            Err(e) => (true, e.to_string(), PathBuf::new()),
        };

        Self {
            is_error_displayed,
            is_f1_displayed: false,
            is_f11_displayed: false,
            options: Options::load(),
            options_cursor: 0,
            options_editing: false,
            options_input: TextInput::new(),
            external_edit: None,
            is_f12_displayed: false,
            preview: None,
            clipboard: String::new(),
            mounts: Vec::new(),
            drive_picker: None,
            is_f2_displayed: false,
            is_f7_displayed: false,
            create_is_dir: true,
            is_left_active: true,
            dir_left: dir_root.clone(),
            dir_right: dir_root,
            page_size: 0,
            state_left,
            state_right,
            children_left: Vec::new(),
            children_right: Vec::new(),
            error_message,
            rename_input: TextInput::new(),
            create_input: TextInput::new(),
            is_f8_displayed: false,
            delete_items: Vec::new(),
            search_input: String::new(),
            cached_clock: String::new(),
            cached_separator_height: 0,
            cached_separator: String::new(),
            is_f3_displayed: false,
            viewer_state: None,
            viewer_viewport_height: 0,
            viewer_viewport_width: 0,
            is_f4_displayed: false,
            editor_state: None,
            editor_viewport_height: 0,
            prompt: None,
            select_pattern: "*".to_string(),
            find_term: String::new(),
            find_shown: false,
            find_note: None,
            is_f5_displayed: false,
            copy_items: Vec::new(),
            is_f6_displayed: false,
            move_items: Vec::new(),
            selected_left: HashSet::new(),
            selected_right: HashSet::new(),
            dir_sizes: HashMap::new(),
            last_click_time: None,
            last_click_pos: (0, 0),
            is_editor_save_prompt: false,
            table_area_left: Rect::default(),
            table_area_right: Rect::default(),
            viewport_start_left: 0,
            viewport_start_right: 0,
            editor_content_area: Rect::default(),
            viewer_content_area: Rect::default(),
            dir_stamp_left: None,
            dir_stamp_right: None,
            last_refresh_check: Instant::now(),
            disk_left: None,
            disk_right: None,
            large_file: None,
            overwrite_prompt: None,
            job: None,
            dir_sizing: Vec::new(),
            image_cache: ImageCache::default(),
            cursor_detail: None,
            quit_armed: false,
            drive_strip_left: Rect::default(),
            drive_strip_right: Rect::default(),
            drive_slots: Vec::new(),
        }
    }

    pub fn reset_rename(&mut self) {
        self.rename_input.clear();
        self.is_f2_displayed = false;
    }

    pub fn display_error(&mut self, message: String) {
        self.is_error_displayed = true;
        self.error_message = message;
    }

    pub fn reset_error(&mut self) {
        self.is_error_displayed = false;
        self.error_message.clear();
    }

    pub fn close_options(&mut self) {
        self.is_f11_displayed = false;
        self.options_editing = false;
        self.options_input.clear();
    }

    fn options_row(&self) -> OptionRow {
        OPTION_ROWS[self.options_cursor.min(OPTION_ROWS.len() - 1)]
    }

    pub fn options_move(&mut self, down: bool) {
        let count = OPTION_ROWS.len();
        self.options_cursor = if down { (self.options_cursor + 1) % count } else { (self.options_cursor + count - 1) % count };
    }

    /// Step the highlighted option, or start typing into it if it is text.
    pub fn options_change(&mut self, forward: bool) {
        let row = self.options_row();
        if row.is_text() {
            self.options_input.set(self.options.text(row).to_string());
            self.options_editing = true;
            return;
        }
        self.options.cycle(row, forward);
        self.options_changed(row);
    }

    pub fn options_commit_edit(&mut self) {
        let row = self.options_row();
        self.options.set_text(row, self.options_input.text.trim().to_string());
        self.options_editing = false;
        self.options_input.clear();
        self.options_changed(row);
    }

    pub fn options_cancel_edit(&mut self) {
        self.options_editing = false;
        self.options_input.clear();
    }

    /// Keep a change: write it out, and reread the panels if it changes what
    /// they list. A failed write still leaves the change in place for this run.
    fn options_changed(&mut self, row: OptionRow) {
        crate::display::apply(&self.options);
        if row.affects_listing() {
            self.reload_panel(true, None);
            self.reload_panel(false, None);
        }
        if let Err(e) = self.options.save() {
            self.display_error(format!("Cannot save options: {}", e));
        }
    }

    pub fn reset_create(&mut self) {
        self.is_f7_displayed = false;
        self.create_input.clear();
    }

    // Quick search methods
    pub fn search_add_char(&mut self, c: char) {
        self.search_input.push(c);
        self.jump_to_first_match();
    }

    pub fn search_backspace(&mut self) {
        self.search_input.pop();
        if !self.search_input.is_empty() {
            self.jump_to_first_match();
        }
    }

    pub fn search_clear(&mut self) {
        self.search_input.clear();
    }

    pub fn jump_to_first_match(&mut self) {
        if self.search_input.is_empty() {
            return;
        }
        let search_lower = self.search_input.to_lowercase();
        let (children, state) = self.active_panel_mut();
        if let Some(index) = children.iter().position(|item| item.name_full.to_lowercase().starts_with(&search_lower)) {
            state.select(Some(index));
        }
    }

    pub fn jump_to_next_match(&mut self) {
        if self.search_input.is_empty() {
            return;
        }
        let search_lower = self.search_input.to_lowercase();
        let (children, state) = self.active_panel_mut();
        let current = state.selected().unwrap_or(0);
        let len = children.len();

        // Search forward from current+1, wrapping around
        for offset in 1..=len {
            let i = (current + offset) % len;
            if children[i].name_full.to_lowercase().starts_with(&search_lower) {
                state.select(Some(i));
                return;
            }
        }
    }

    pub fn jump_to_prev_match(&mut self) {
        if self.search_input.is_empty() {
            return;
        }
        let search_lower = self.search_input.to_lowercase();
        let (children, state) = self.active_panel_mut();
        let current = state.selected().unwrap_or(0);
        let len = children.len();

        // Search backward from current-1, wrapping around
        for offset in 1..=len {
            let i = (current + len - offset) % len;
            if children[i].name_full.to_lowercase().starts_with(&search_lower) {
                state.select(Some(i));
                return;
            }
        }
    }

    /// Returns (&children, &mut state) for the active panel.
    fn active_panel_mut(&mut self) -> (&[Item], &mut TableState) {
        if self.is_left_active {
            (&self.children_left, &mut self.state_left)
        } else {
            (&self.children_right, &mut self.state_right)
        }
    }

    pub fn reset_delete(&mut self) {
        self.is_f8_displayed = false;
        self.delete_items.clear();
    }

    /// Open a file for viewing or editing, asking first when it is large enough
    /// that loading it will stall for a noticeable while.
    pub fn request_open(&mut self, file_path: PathBuf, is_edit: bool) {
        let size = match std::fs::metadata(&file_path) {
            // A pipe, socket or device: opening one to read can block for good,
            // and /dev/zero would pass the size check below and never end.
            Ok(metadata) if !metadata.is_file() => {
                self.display_error(format!("Not a regular file: {}", file_path.display()));
                return;
            }
            Ok(metadata) => metadata.len(),
            Err(e) => {
                self.display_error(e.to_string());
                return;
            }
        };

        if size > self.options.large_file_bytes() {
            self.large_file = Some(LargeFile { path: file_path, is_edit, size, dimensions: None });
            return;
        }

        // What a picture costs is its pixel count, and the two part company
        // completely: a few hundred KB of PNG can unpack to hundreds of MB, and
        // the size checked above never sees it coming. Reading the header to
        // find out costs a fraction of a millisecond. The editor refuses
        // binaries outright, so nothing is decoded on that path.
        if !is_edit
            && let Some((dimensions, decoded)) = crate::viewer::image_cost(&file_path)
            && decoded > crate::constants::IMAGE_MAX_DECODED
        {
            self.large_file = Some(LargeFile { path: file_path, is_edit, size: decoded, dimensions: Some(dimensions) });
            return;
        }

        self.open_file(file_path, is_edit);
    }

    /// Answer to the large-file prompt: load it after all.
    pub fn confirm_large_file(&mut self) {
        if let Some(large) = self.large_file.take() {
            self.open_file(large.path, large.is_edit);
        }
    }

    pub fn reset_large_file(&mut self) {
        self.large_file = None;
    }

    fn open_file(&mut self, file_path: PathBuf, is_edit: bool) {
        let result = if is_edit { self.open_editor(file_path) } else { self.open_viewer(file_path) };
        if let Err(e) = result {
            self.display_error(e);
        }
    }

    pub fn open_viewer(&mut self, file_path: PathBuf) -> Result<(), String> {
        let decoded = if self.options.image_prefetch { self.image_cache.take(&file_path) } else { None };
        let mut state = crate::viewer::load_file_content(&file_path, decoded).map_err(|e| e.to_string())?;
        state.image_fill = self.options.image_fill;

        // Only once the new one is in hand: a load that failed used to leave
        // what was open alone, and should go on doing so. The picture being
        // replaced is already decoded, so keeping it costs nothing and makes
        // stepping back as quick as stepping on.
        if self.options.image_prefetch
            && let Some(previous) = self.viewer_state.take()
            && let Some(image) = previous.image
        {
            self.image_cache.keep(previous.file_path, image, previous.syntax_name);
        }

        self.viewer_state = Some(state);
        self.is_f3_displayed = true;
        self.read_ahead();
        Ok(())
    }

    /// Redraw an image as ASCII whenever the viewer size or the fit/fill choice
    /// asks for a different width than it was drawn at. Returns true if it did,
    /// so the caller can draw again.
    pub fn fit_viewer_image(&mut self) -> bool {
        let (width, height) = (self.viewer_viewport_width, self.viewer_viewport_height);
        let backgrounds = self.options.image_backgrounds;
        let Some(state) = &mut self.viewer_state else {
            return false;
        };
        let Some(image) = &state.image else {
            return false;
        };
        if state.mode != ViewMode::Image || width == 0 || height == 0 {
            return false;
        }
        let (columns, rows) = crate::viewer::image_size_for(image, width, height, state.image_fill, state.image_zoom);
        // Rows as well as columns: a picture squashed to fit is one column wide
        // whatever the viewer's height, so only the row count shows the change.
        // The backgrounds too, since turning them on or off redraws the same
        // size - whether they are filled is the record of which way it was made.
        let drawn_with_backgrounds = !state.image_backgrounds.is_empty();
        if columns == state.image_columns && rows == state.image_lines.len() && backgrounds == drawn_with_backgrounds {
            return false;
        }

        let (was_columns, was_rows) = (state.image_columns, state.image_lines.len());
        (state.image_lines, state.image_colors, state.image_backgrounds) =
            crate::viewer::image_to_ascii(image, columns, rows, backgrounds);
        state.total_lines = state.line_count();
        state.image_columns = columns;
        // Hold whatever was in the middle of the pane in the middle of it. A
        // zoom step that threw the view back to the top-left would be no use
        // for looking closely at a detail, which is what zooming in is for.
        state.horizontal_offset = recentre(state.horizontal_offset, width, was_columns, columns);
        state.scroll_offset = recentre(state.scroll_offset, height, was_rows, rows);
        // Positions into the old drawing mean nothing in the new one.
        state.selection = None;
        state.pan_from = None;
        true
    }

    /// Switch an image between fitting inside the viewer and filling it. The
    /// redraw itself happens in fit_viewer_image before the next frame.
    pub fn viewer_toggle_fill(&mut self) {
        if let Some(state) = &mut self.viewer_state
            && state.mode == ViewMode::Image
        {
            state.image_fill = !state.image_fill;
            // F is also the way back to an unzoomed picture from any zoom.
            state.image_zoom = crate::constants::IMAGE_ZOOM_NORMAL;
            state.scroll_offset = 0;
        }
    }

    /// Step an image up or down the zoom ladder. The zoom scales whichever of
    /// Fit and Fill is showing, so F still decides what 100% means. The drawing
    /// itself is rebuilt by fit_viewer_image before the next frame.
    pub fn viewer_zoom(&mut self, closer: bool) {
        if let Some(state) = &mut self.viewer_state
            && state.mode == ViewMode::Image
        {
            let steps = crate::constants::IMAGE_ZOOM_STEPS;
            // Where the current zoom sits on the ladder, whichever rung it is on.
            let at = steps.iter().position(|&percent| percent >= state.image_zoom).unwrap_or(steps.len() - 1);
            let next = if closer { (at + 1).min(steps.len() - 1) } else { at.saturating_sub(1) };
            state.image_zoom = steps[next];
        }
    }

    pub fn close_viewer(&mut self) {
        self.is_f3_displayed = false;
        self.viewer_state = None;
        self.close_find();
        // Nothing is going to be stepped to now, and these are the only thing
        // in the app holding decoded pictures.
        self.image_cache.clear();
    }

    pub fn viewer_scroll_down(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            let max = state.total_lines.saturating_sub(self.viewer_viewport_height);
            state.scroll_offset = (state.scroll_offset + 1).min(max);
        }
    }

    pub fn viewer_scroll_up(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.scroll_offset = state.scroll_offset.saturating_sub(1);
        }
    }

    pub fn viewer_page_down(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            let max = state.total_lines.saturating_sub(self.viewer_viewport_height);
            state.scroll_offset = (state.scroll_offset + self.viewer_viewport_height).min(max);
        }
    }

    pub fn viewer_page_up(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.scroll_offset = state.scroll_offset.saturating_sub(self.viewer_viewport_height);
        }
    }

    pub fn viewer_home(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.scroll_offset = 0;
            state.horizontal_offset = 0;
        }
    }

    pub fn viewer_scroll_left(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.horizontal_offset = state.horizontal_offset.saturating_sub(1);
        }
    }

    pub fn viewer_copy(&mut self) {
        if let Some(text) = self.viewer_state.as_ref().and_then(|state| state.selected_text()) {
            crate::utils::set_system_clipboard(&text);
            self.clipboard = text;
        }
    }

    /// Step the viewer to its next mode: text and a hexdump, and the picture for
    /// an image. Reads the raw bytes the first time they are needed, so a text
    /// file only pays for them on demand.
    pub fn viewer_next_mode(&mut self) {
        // Nothing to toggle on the refusal notice F4 puts up.
        if self.viewer_state.as_ref().is_some_and(|state| state.from_edit) {
            return;
        }

        let mut error = None;

        if let Some(state) = &mut self.viewer_state {
            let next = state.next_mode();
            if next == ViewMode::Hex && state.bytes.is_empty() && state.file_size > 0 {
                match std::fs::read(&state.file_path) {
                    Ok(bytes) => state.bytes = bytes,
                    Err(e) => error = Some(e.to_string()),
                }
            }

            if error.is_none() {
                // Text of a file that was never decoded - a binary or an image:
                // build it lossily rather than leave the pane blank.
                if next == ViewMode::Text && state.content_lines.is_empty() {
                    state.content_lines = String::from_utf8_lossy(&state.bytes).lines().map(str::to_string).collect();
                    if state.content_lines.is_empty() {
                        state.content_lines.push(String::new());
                    }
                    state.max_line_width = state
                        .content_lines
                        .iter()
                        .map(|line| crate::utils::line_display_width(line))
                        .max()
                        .unwrap_or(0);
                }

                state.mode = next;
                state.total_lines = state.line_count();
                // Positions in one mode's lines mean nothing in another's.
                state.selection = None;
                state.scroll_offset = state.scroll_offset.min(state.total_lines.saturating_sub(1));
                state.horizontal_offset = 0;
            }
        }

        if let Some(e) = error {
            self.display_error(e);
        }
    }

    /// Step to the next picture in the panel this one was opened from, or to
    /// the one before it. Anything that is not a picture is stepped over and
    /// the walk wraps round, so a folder of photographs can be gone through
    /// without leaving the viewer.
    pub fn viewer_step_image(&mut self, forward: bool) {
        let Some(showing) = self.viewer_state.as_ref().map(|state| state.file_path.clone()) else {
            return;
        };
        let Some(going_to) = self.image_neighbour(&showing, forward) else {
            return;
        };

        // Carry across how it is being looked at, so a folder can be stepped
        // through at one zoom rather than starting over on every picture.
        let carried = self.viewer_state.as_ref().map(|state| (state.image_fill, state.image_zoom));
        self.image_cache.forward = forward;
        self.request_open(going_to, false);
        if let Some(state) = &mut self.viewer_state
            && let Some((fill, zoom)) = carried
        {
            state.image_fill = fill;
            state.image_zoom = zoom;
        }
    }

    /// The picture before or after `showing` in the panel it came from, wrapping
    /// round. Which files are pictures is settled by reading each header, not by
    /// the name: that is how the viewer decides everywhere else, and it costs a
    /// fraction of a millisecond a file.
    fn image_neighbour(&self, showing: &Path, forward: bool) -> Option<PathBuf> {
        let (children, dir) =
            if self.is_left_active { (&self.children_left, &self.dir_left) } else { (&self.children_right, &self.dir_right) };
        let files: Vec<PathBuf> = children.iter().filter(|item| !item.is_dir).map(|item| item.path_in(dir)).collect();
        let at = files.iter().position(|path| path == showing)?;

        let total = files.len();
        (1..=total)
            .find_map(|step| {
                let index = if forward { (at + step) % total } else { (at + total - step) % total };
                crate::viewer::image_cost(&files[index]).map(|_| index)
            })
            .map(|index| files[index].clone())
            .filter(|path| path != showing)
    }

    /// Start reading the picture on the far side of this one, in whichever
    /// direction the last step went. Nothing happens if F11 has it off, or if
    /// what is open is not a picture.
    fn read_ahead(&mut self) {
        if !self.options.image_prefetch || !self.viewer_shows_image() {
            return;
        }
        let Some(showing) = self.viewer_state.as_ref().map(|state| state.file_path.clone()) else {
            return;
        };
        if let Some(next) = self.image_neighbour(&showing, self.image_cache.forward) {
            self.image_cache.request(next);
        }
    }

    /// True while the viewer is showing a picture, where dragging moves the
    /// picture rather than selecting the characters it is drawn from.
    pub fn viewer_shows_image(&self) -> bool {
        self.viewer_state.as_ref().is_some_and(|state| state.mode == ViewMode::Image && !state.from_edit)
    }

    /// Take hold of a picture at the pointer, ready to drag it about.
    pub fn viewer_pan_start(&mut self, column: u16, row: u16) {
        let content = self.viewer_content_area;
        if let Some(state) = &mut self.viewer_state
            && content.contains(Position::new(column, row))
        {
            state.pan_from = Some(((column, row), (state.horizontal_offset, state.scroll_offset)));
        }
    }

    /// Move the picture taken hold of above. It follows the pointer the way a
    /// sheet of paper follows a finger on it: moving right brings what was off
    /// to the left into view, so the offset goes the other way. Both offsets
    /// move at once, which is the whole point of doing this with the mouse.
    pub fn viewer_pan_to(&mut self, column: u16, row: u16) {
        let (width, height) = (self.viewer_viewport_width, self.viewer_viewport_height);
        if let Some(state) = &mut self.viewer_state
            && let Some(((from_column, from_row), (from_horizontal, from_vertical))) = state.pan_from
        {
            let moved_x = i64::from(column) - i64::from(from_column);
            let moved_y = i64::from(row) - i64::from(from_row);
            let furthest_x = state.image_columns.saturating_sub(width) as i64;
            let furthest_y = state.total_lines.saturating_sub(height) as i64;
            state.horizontal_offset = (from_horizontal as i64 - moved_x).clamp(0, furthest_x) as usize;
            state.scroll_offset = (from_vertical as i64 - moved_y).clamp(0, furthest_y) as usize;
        }
    }

    pub fn viewer_scroll_right(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            // Stop once the longest line's end reaches the right edge.
            let longest = match state.mode {
                ViewMode::Hex => crate::constants::HEX_LINE_WIDTH,
                ViewMode::Image => state.image_columns,
                ViewMode::Text => state.max_line_width,
            };
            let max = longest.saturating_sub(self.viewer_viewport_width);
            state.horizontal_offset = (state.horizontal_offset + 1).min(max);
        }
    }

    pub fn viewer_end(&mut self) {
        if let Some(state) = &mut self.viewer_state {
            state.scroll_offset = state.total_lines.saturating_sub(self.viewer_viewport_height);
        }
    }

    pub fn open_editor(&mut self, file_path: PathBuf) -> Result<(), String> {
        use crate::viewer::{highlight_all, is_binary_file};

        let file_size = std::fs::metadata(&file_path).map_err(|e| e.to_string())?.len();

        if is_binary_file(&file_path).unwrap_or(false) {
            self.open_viewer(file_path)?;
            if let Some(state) = &mut self.viewer_state {
                // Editing is refused outright; F3 is where a binary gets read.
                state.from_edit = true;
                state.mode = ViewMode::Text;
                state.bytes = Vec::new();
                state.image = None;
                state.image_lines = Vec::new();
                state.image_colors = Vec::new();
                state.content_lines = Vec::new();
                state.total_lines = 1;
            }
            return Ok(());
        }

        let content = std::fs::read_to_string(&file_path).map_err(|e| e.to_string())?;
        let line_ending = detect_line_ending(&content);
        let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        if content.ends_with('\n') {
            lines.push(String::new());
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        // The initial parse measures about 0.9s per megabyte, which is what the
        // limit in F11 caps; editing stays fast afterwards regardless of length.
        let limit = self.options.highlight_limit_bytes();
        let (highlighted_lines, line_states) = if limit > 0 && file_size <= limit {
            highlight_all(&lines, extension)
        } else {
            // Rendering already falls back to plain text when these are empty.
            (Vec::new(), Vec::new())
        };

        self.editor_state = Some(EditorState {
            file_path,
            lines,
            highlighted_lines,
            cursor_line: 0,
            cursor_col: 0,
            scroll_offset: 0,
            horizontal_offset: 0,
            modified: false,
            auto_scroll: true,
            selection_anchor: None,
            line_ending,
            line_states,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        });
        self.is_f4_displayed = true;
        Ok(())
    }

    pub fn close_editor(&mut self) {
        self.is_f4_displayed = false;
        self.editor_state = None;
        self.close_find();
    }

    /// The prompt and the marks go with the file they were for. The term is
    /// kept, to be offered again.
    fn close_find(&mut self) {
        self.prompt = None;
        self.find_shown = false;
        self.find_note = None;
    }

    /// Ask for something to find, or a line to go to. Find opens on the last
    /// term, so Enter alone looks for it again.
    pub fn open_prompt(&mut self, kind: PromptKind) {
        if !self.can_find() {
            return;
        }
        let mut input = TextInput::new();
        if kind == PromptKind::Find {
            input.set(self.find_term.clone());
        }
        self.prompt = Some((kind, input));
    }

    /// Only text can be searched or numbered: not a picture, and not the notice
    /// F4 raises on a binary file.
    fn can_find(&self) -> bool {
        if self.is_f4_displayed {
            return self.editor_state.is_some();
        }
        self.viewer_state.as_ref().is_some_and(|state| !state.from_edit && state.mode != ViewMode::Image)
    }

    /// Enter on the prompt: look for what was typed, or go to the line.
    pub fn confirm_prompt(&mut self) {
        let Some((kind, input)) = self.prompt.take() else {
            return;
        };
        match kind {
            PromptKind::Find => {
                if input.text.is_empty() {
                    // Nothing to look for, so nothing to mark either.
                    self.find_shown = false;
                    return;
                }
                self.find_term = input.text;
                self.find(true, true);
            }
            PromptKind::GoToLine => match input.text.trim().parse::<usize>() {
                Ok(number) => self.go_to_line(number),
                Err(_) if input.text.trim().is_empty() => {}
                Err(_) => self.find_note = Some(format!("Not a line number: {}", input.text.trim())),
            },
            PromptKind::Select | PromptKind::Deselect => {
                let Some(glob) = crate::glob::Glob::new(&input.text) else {
                    return;
                };
                self.select_pattern = input.text;
                self.select_by(&glob, kind == PromptKind::Select);
            }
        }
    }

    /// + and - in a panel: ask for a pattern, starting from the last one.
    pub fn open_select_prompt(&mut self, select: bool) {
        let mut input = TextInput::new();
        input.set(self.select_pattern.clone());
        self.prompt = Some((if select { PromptKind::Select } else { PromptKind::Deselect }, input));
    }

    /// Select, or deselect, every entry in the active panel the pattern
    /// matches. No directory is sized: a pattern can take dozens at once, and
    /// Space is there for the one that is wanted.
    pub fn select_by(&mut self, glob: &crate::glob::Glob, select: bool) {
        let (children, selected) = if self.is_left_active {
            (&self.children_left, &mut self.selected_left)
        } else {
            (&self.children_right, &mut self.selected_right)
        };
        for item in children.iter().filter(|item| item.name != ".." && glob.matches(&item.name_full, item.is_dir)) {
            if select {
                selected.insert(item.name_os.clone());
            } else {
                selected.remove(&item.name_os);
            }
        }
    }

    /// The * key in a panel: every file selected is deselected and every other
    /// one selected. Directories stay as they are, as + leaves them unless
    /// asked: inverting a few picked files should not sweep every tree beside
    /// them into the next delete. Alt+* asks, and inverts them too.
    pub fn invert_selection(&mut self, with_dirs: bool) {
        let (children, selected) = if self.is_left_active {
            (&self.children_left, &mut self.selected_left)
        } else {
            (&self.children_right, &mut self.selected_right)
        };
        for item in children.iter().filter(|item| (with_dirs || !item.is_dir) && item.name != "..") {
            if !selected.remove(&item.name_os) {
                selected.insert(item.name_os.clone());
            }
        }
    }

    /// Move to the next match of find_term, or the previous one, selecting it.
    /// `fresh` is a new search from the prompt, which may find the match the
    /// cursor is already on; F3 and Shift+F3 go on past it.
    pub fn find(&mut self, forward: bool, fresh: bool) {
        let Some(needle) = crate::find::Needle::new(&self.find_term) else {
            // F3 with nothing looked for yet asks what to look for.
            self.open_prompt(PromptKind::Find);
            return;
        };
        if !self.can_find() {
            return;
        }
        self.find_shown = true;

        let found = if self.is_f4_displayed {
            self.editor_find(&needle, forward, fresh)
        } else {
            self.viewer_find(&needle, forward, fresh)
        };
        self.find_note = match found {
            None => Some(format!("Not found: {}", self.find_term)),
            Some(true) => Some(if forward { "Wrapped to the top" } else { "Wrapped to the bottom" }.to_string()),
            Some(false) => None,
        };
    }

    /// Some(wrapped) when a match was found and selected.
    fn editor_find(&mut self, needle: &crate::find::Needle, forward: bool, fresh: bool) -> Option<bool> {
        let height = self.editor_viewport_height.max(1);
        let state = self.editor_state.as_mut()?;
        let cursor = (state.cursor_line, state.cursor_col);
        let start = state.selection().map_or(cursor, |(start, _)| start);
        // A match is selected from its start to the cursor at its end, so on
        // from the cursor is past it and back from its start is before it.
        let from = if forward && !fresh { cursor } else { start };

        let lines = &state.lines;
        let found = crate::find::search(needle, lines.len(), |index| lines[index].clone(), from, forward)?;
        state.selection_anchor = Some((found.line, found.start));
        state.cursor_line = found.line;
        state.cursor_col = found.end;
        state.auto_scroll = true;
        if found.line < state.scroll_offset || found.line >= state.scroll_offset + height {
            state.scroll_offset = found.line.saturating_sub(height / 3);
        }
        Some(found.wrapped)
    }

    fn viewer_find(&mut self, needle: &crate::find::Needle, forward: bool, fresh: bool) -> Option<bool> {
        let (height, width) = (self.viewer_viewport_height.max(1), self.viewer_viewport_width.max(1));
        let state = self.viewer_state.as_mut()?;
        // From the match on screen when there is one, and otherwise from the
        // top of what is showing.
        let from = match state.selected_range() {
            Some((_, end)) if forward && !fresh => end,
            Some((start, _)) => start,
            None => (state.scroll_offset, 0),
        };

        let found = crate::find::search(needle, state.line_count(), |index| state.line_text(index), from, forward)?;
        state.selection = Some(((found.line, found.start), (found.line, found.end)));
        if found.line < state.scroll_offset || found.line >= state.scroll_offset + height {
            let max = state.total_lines.saturating_sub(height);
            state.scroll_offset = found.line.saturating_sub(height / 3).min(max);
        }
        if found.start < state.horizontal_offset || found.end > state.horizontal_offset + width {
            state.horizontal_offset = found.start.saturating_sub(width / 4);
        }
        Some(found.wrapped)
    }

    /// Ctrl+G: put the cursor on the start of a line, counted from 1. A number
    /// past the end goes to the last line. In the viewer the line goes to the
    /// top, as far as the end of the file allows.
    pub fn go_to_line(&mut self, number: usize) {
        if self.is_f4_displayed {
            let height = self.editor_viewport_height.max(1);
            if let Some(state) = &mut self.editor_state {
                let line = number.clamp(1, state.lines.len().max(1)) - 1;
                state.selection_anchor = None;
                state.cursor_line = line;
                state.cursor_col = 0;
                state.auto_scroll = true;
                if line < state.scroll_offset || line >= state.scroll_offset + height {
                    state.scroll_offset = line.saturating_sub(height / 3);
                }
            }
        } else if let Some(state) = &mut self.viewer_state {
            let line = number.clamp(1, state.total_lines.max(1)) - 1;
            state.scroll_offset = line.min(state.total_lines.saturating_sub(self.viewer_viewport_height));
        }
    }

    /// Re-highlight the file from `from` downward. Cheap: the cached state lets
    /// it resume mid-file and stop again as soon as the parse converges.
    pub fn editor_rehighlight_from(&mut self, from: usize) {
        if let Some(state) = &mut self.editor_state {
            if state.line_states.is_empty() {
                return; // highlighting disabled for this file
            }
            let extension = state.file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            crate::viewer::highlight_from(
                &state.lines,
                extension,
                from,
                &mut state.highlighted_lines,
                &mut state.line_states,
            );
        }
    }

    pub fn editor_scroll_up(&mut self) {
        if let Some(state) = &mut self.editor_state
            && state.scroll_offset > 0
        {
            state.scroll_offset -= 1;
            state.auto_scroll = false;
        }
    }

    pub fn editor_scroll_down(&mut self) {
        if let Some(state) = &mut self.editor_state {
            let max = state.lines.len().saturating_sub(self.editor_viewport_height);
            if state.scroll_offset < max {
                state.scroll_offset += 1;
                state.auto_scroll = false;
            }
        }
    }

    pub fn editor_scroll_left(&mut self) {
        if let Some(state) = &mut self.editor_state {
            state.horizontal_offset = state.horizontal_offset.saturating_sub(1);
            state.auto_scroll = false;
        }
    }

    pub fn editor_scroll_right(&mut self) {
        if let Some(state) = &mut self.editor_state {
            state.horizontal_offset += 1;
            state.auto_scroll = false;
        }
    }

    /// Remember the lines `range` covers before they are replaced. Called once
    /// per user action, with a range spanning everything that action touches.
    fn push_undo(&mut self, range: std::ops::RangeInclusive<usize>) {
        if let Some(state) = &mut self.editor_state {
            let last = (*range.end()).min(state.lines.len().saturating_sub(1));
            let first = (*range.start()).min(last);

            state.undo_stack.push(EditStep {
                first_line: first,
                before: state.lines[first..=last].to_vec(),
                total_lines_before: state.lines.len(),
                cursor: (state.cursor_line, state.cursor_col),
            });
        }
    }

    /// Settle the step push_undo recorded, once the action has run. One that
    /// changed nothing - Backspace at the very start, Delete at the very end,
    /// Ctrl+X with nothing selected - is dropped, so it neither costs an undo
    /// that does nothing nor throws away the redo history. One that did change
    /// something forks the history: editing after undoing loses the old branch.
    fn finish_edit(&mut self) {
        if let Some(state) = &mut self.editor_state {
            let Some(step) = state.undo_stack.last() else {
                return;
            };
            let unchanged = state.lines.len() == step.total_lines_before
                && state.lines[step.first_line..step.first_line + step.before.len()] == step.before[..];
            if unchanged {
                state.undo_stack.pop();
                return;
            }
            state.redo_stack.clear();
            if state.undo_stack.len() > crate::constants::UNDO_LIMIT {
                state.undo_stack.remove(0);
            }
        }
    }

    /// The range a user action is about to touch: the selection when there is
    /// one, otherwise the given fallback.
    fn edit_range(&self, fallback: std::ops::RangeInclusive<usize>) -> std::ops::RangeInclusive<usize> {
        match self.editor_state.as_ref().and_then(|state| state.selection()) {
            Some(((first, _), (last, _))) => first..=last,
            None => fallback,
        }
    }

    pub fn editor_undo(&mut self) {
        self.step_history(true);
    }

    pub fn editor_redo(&mut self) {
        self.step_history(false);
    }

    /// Move one step through the edit history. Undo and redo are the same
    /// operation in opposite directions: apply a step, and push the step that
    /// would put things back onto the other stack.
    fn step_history(&mut self, undoing: bool) {
        let mut from = None;

        if let Some(state) = &mut self.editor_state {
            let stack = if undoing { &mut state.undo_stack } else { &mut state.redo_stack };
            if let Some(step) = stack.pop() {
                // However many lines replaced the originals, the buffer's change
                // in length tells us how many to take back out.
                let removed = step.total_lines_before - step.before.len();
                let replaced = state.lines.len().saturating_sub(removed);
                let end = (step.first_line + replaced).min(state.lines.len());

                let inverse = EditStep {
                    first_line: step.first_line,
                    before: state.lines[step.first_line..end].to_vec(),
                    total_lines_before: state.lines.len(),
                    cursor: (state.cursor_line, state.cursor_col),
                };

                state.lines.splice(step.first_line..end, step.before);
                state.cursor_line = step.cursor.0.min(state.lines.len().saturating_sub(1));
                state.cursor_col = step.cursor.1.min(state.lines[state.cursor_line].chars().count());
                state.selection_anchor = None;
                state.modified = true;

                if state.cursor_line < state.scroll_offset {
                    state.scroll_offset = state.cursor_line;
                }

                let other = if undoing { &mut state.redo_stack } else { &mut state.undo_stack };
                other.push(inverse);
                if other.len() > crate::constants::UNDO_LIMIT {
                    other.remove(0);
                }

                from = Some(step.first_line);
            }
        }

        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
    }

    /// Remove the selected range, leaving the cursor where the selection began.
    /// Returns the line to re-highlight from, or None if nothing was selected.
    /// Shared by cut, paste, typing, Backspace and Delete.
    fn delete_selection(&mut self) -> Option<usize> {
        let state = self.editor_state.as_mut()?;
        let ((first_line, first_col), (last_line, last_col)) = state.selection()?;

        let head = char_slice(&state.lines[first_line], 0, first_col);
        let last = &state.lines[last_line];
        let tail = char_slice(last, last_col, last.chars().count());

        state.lines.splice(first_line..=last_line, [format!("{head}{tail}")]);
        state.cursor_line = first_line;
        state.cursor_col = first_col;
        state.selection_anchor = None;
        state.modified = true;
        Some(first_line)
    }

    pub fn editor_select_all(&mut self) {
        let height = self.editor_viewport_height;
        if let Some(state) = &mut self.editor_state {
            let last_line = state.lines.len().saturating_sub(1);
            state.selection_anchor = Some((0, 0));
            state.cursor_line = last_line;
            state.cursor_col = state.lines[last_line].chars().count();
            state.scroll_offset = last_line.saturating_sub(height.saturating_sub(1));
        }
    }

    pub fn editor_copy(&mut self) {
        if let Some(text) = self.editor_state.as_ref().and_then(|state| state.selected_text()) {
            // Offer it to the terminal as well, so it reaches the system
            // clipboard where OSC 52 is supported.
            crate::utils::set_system_clipboard(&text);
            self.clipboard = text;
        }
    }

    pub fn editor_cut(&mut self) {
        let line = self.editor_state.as_ref().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        self.editor_copy();
        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
        }
        self.finish_edit();
    }

    pub fn editor_paste(&mut self) {
        let text = self.clipboard.clone();
        self.editor_insert_text(&text);
    }

    /// Insert text at the cursor, replacing any selection. Serves both the
    /// internal clipboard and a bracketed paste arriving from the terminal.
    pub fn editor_insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let line = self.editor_state.as_ref().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));

        // A pasted Windows clipboard arrives with CRLF; the buffer holds lines.
        let text = text.replace("\r\n", "\n");

        // A selection is replaced by what is pasted over it.
        let mut rehighlight = self.delete_selection();

        if let Some(state) = &mut self.editor_state {
            let line = state.lines[state.cursor_line].clone();
            let head = char_slice(&line, 0, state.cursor_col);
            let tail = char_slice(&line, state.cursor_col, line.chars().count());
            let pieces: Vec<&str> = text.split('\n').collect();
            let start_line = state.cursor_line;

            if let [only] = pieces[..] {
                state.lines[start_line] = format!("{head}{only}{tail}");
                state.cursor_col += only.chars().count();
            } else {
                let last = pieces.len() - 1;
                let mut replacement = Vec::with_capacity(pieces.len());
                replacement.push(format!("{head}{}", pieces[0]));
                replacement.extend(pieces[1..last].iter().map(|piece| (*piece).to_string()));
                replacement.push(format!("{}{tail}", pieces[last]));

                state.lines.splice(start_line..=start_line, replacement);
                state.cursor_line = start_line + last;
                state.cursor_col = pieces[last].chars().count();
            }

            state.modified = true;
            state.selection_anchor = None;
            rehighlight = Some(rehighlight.unwrap_or(start_line).min(start_line));
        }

        if let Some(from) = rehighlight {
            self.editor_rehighlight_from(from);
        }
        self.finish_edit();
    }

    /// Called before a cursor move: Shift extends the selection from where the
    /// cursor was, anything else drops it.
    pub fn editor_prepare_move(&mut self, extend: bool) {
        if let Some(state) = &mut self.editor_state {
            if extend {
                if state.selection_anchor.is_none() {
                    state.selection_anchor = Some((state.cursor_line, state.cursor_col));
                }
            } else {
                state.selection_anchor = None;
            }
        }
    }

    pub fn editor_cursor_up(&mut self) {
        if let Some(state) = &mut self.editor_state
            && state.cursor_line > 0
        {
            state.cursor_line -= 1;
            state.clamp_col();
            if state.cursor_line < state.scroll_offset {
                state.scroll_offset = state.cursor_line;
            }
        }
    }

    pub fn editor_cursor_down(&mut self) {
        if let Some(state) = &mut self.editor_state
            && state.cursor_line < state.lines.len().saturating_sub(1)
        {
            state.cursor_line += 1;
            state.clamp_col();
            if state.cursor_line >= state.scroll_offset + self.editor_viewport_height {
                state.scroll_offset = state.cursor_line - self.editor_viewport_height + 1;
            }
        }
    }

    pub fn editor_cursor_left(&mut self) {
        if let Some(state) = &mut self.editor_state {
            if state.cursor_col > 0 {
                state.cursor_col -= 1;
            } else if state.cursor_line > 0 {
                state.cursor_line -= 1;
                state.cursor_col = state.lines[state.cursor_line].chars().count();
                if state.cursor_line < state.scroll_offset {
                    state.scroll_offset = state.cursor_line;
                }
            }
        }
    }

    pub fn editor_cursor_right(&mut self) {
        if let Some(state) = &mut self.editor_state {
            let line_len = state.lines[state.cursor_line].chars().count();
            if state.cursor_col < line_len {
                state.cursor_col += 1;
            } else if state.cursor_line < state.lines.len().saturating_sub(1) {
                state.cursor_line += 1;
                state.cursor_col = 0;
                if state.cursor_line >= state.scroll_offset + self.editor_viewport_height {
                    state.scroll_offset = state.cursor_line - self.editor_viewport_height + 1;
                }
            }
        }
    }

    pub fn editor_home(&mut self) {
        if let Some(state) = &mut self.editor_state {
            state.cursor_col = 0;
        }
    }

    pub fn editor_end(&mut self) {
        if let Some(state) = &mut self.editor_state {
            state.cursor_col = state.lines[state.cursor_line].chars().count();
        }
    }

    pub fn editor_page_up(&mut self) {
        if let Some(state) = &mut self.editor_state {
            let page = self.editor_viewport_height.saturating_sub(1);
            state.cursor_line = state.cursor_line.saturating_sub(page);
            state.scroll_offset = state.scroll_offset.saturating_sub(page);
            state.clamp_col();
        }
    }

    pub fn editor_page_down(&mut self) {
        if let Some(state) = &mut self.editor_state {
            let page = self.editor_viewport_height.saturating_sub(1);
            let max_line = state.lines.len().saturating_sub(1);
            state.cursor_line = (state.cursor_line + page).min(max_line);
            let max_scroll = state.lines.len().saturating_sub(self.editor_viewport_height);
            state.scroll_offset = (state.scroll_offset + page).min(max_scroll);
            state.clamp_col();
        }
    }

    pub fn editor_insert_char(&mut self, c: char) {
        let line = self.editor_state.as_ref().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        // Typing over a selection replaces it.
        self.delete_selection();
        let mut from = None;
        if let Some(state) = &mut self.editor_state {
            let line = &mut state.lines[state.cursor_line];
            let byte_idx = char_to_byte(line, state.cursor_col);
            line.insert(byte_idx, c);
            state.cursor_col += 1;
            state.modified = true;
            from = Some(state.cursor_line);
        }
        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
        self.finish_edit();
    }

    pub fn editor_backspace(&mut self) {
        let fallback = match self.editor_state.as_ref() {
            // Joining with the line above puts that line in range too.
            Some(state) if state.cursor_col == 0 => state.cursor_line.saturating_sub(1)..=state.cursor_line,
            Some(state) => state.cursor_line..=state.cursor_line,
            None => return,
        };
        self.push_undo(self.edit_range(fallback));

        // With a selection, Backspace removes that rather than a character.
        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
            self.finish_edit();
            return;
        }
        let mut from = None;
        if let Some(state) = &mut self.editor_state {
            if state.cursor_col > 0 {
                let line = &mut state.lines[state.cursor_line];
                let byte_start = char_to_byte(line, state.cursor_col - 1);
                let byte_end = char_to_byte(line, state.cursor_col);
                line.replace_range(byte_start..byte_end, "");
                state.cursor_col -= 1;
                state.modified = true;
                from = Some(state.cursor_line);
            } else if state.cursor_line > 0 {
                let current_line = state.lines.remove(state.cursor_line);
                state.cursor_line -= 1;
                state.cursor_col = state.lines[state.cursor_line].chars().count();
                state.lines[state.cursor_line].push_str(&current_line);
                state.modified = true;
                from = Some(state.cursor_line);
                if state.cursor_line < state.scroll_offset {
                    state.scroll_offset = state.cursor_line;
                }
            }
        }
        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
        self.finish_edit();
    }

    pub fn editor_delete(&mut self) {
        let fallback = match self.editor_state.as_ref() {
            // At end of line the next line is pulled up, so include it.
            Some(state) if state.cursor_col >= state.lines[state.cursor_line].chars().count() => {
                state.cursor_line..=state.cursor_line + 1
            }
            Some(state) => state.cursor_line..=state.cursor_line,
            None => return,
        };
        self.push_undo(self.edit_range(fallback));

        if let Some(from) = self.delete_selection() {
            self.editor_rehighlight_from(from);
            self.finish_edit();
            return;
        }
        let mut from = None;
        if let Some(state) = &mut self.editor_state {
            let line_len = state.lines[state.cursor_line].chars().count();
            if state.cursor_col < line_len {
                let line = &mut state.lines[state.cursor_line];
                let byte_start = char_to_byte(line, state.cursor_col);
                let byte_end = char_to_byte(line, state.cursor_col + 1);
                line.replace_range(byte_start..byte_end, "");
                state.modified = true;
                from = Some(state.cursor_line);
            } else if state.cursor_line < state.lines.len().saturating_sub(1) {
                let next_line = state.lines.remove(state.cursor_line + 1);
                state.lines[state.cursor_line].push_str(&next_line);
                state.modified = true;
                from = Some(state.cursor_line);
            }
        }
        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
        self.finish_edit();
    }

    pub fn editor_enter(&mut self) {
        let line = self.editor_state.as_ref().map_or(0, |state| state.cursor_line);
        self.push_undo(self.edit_range(line..=line));
        self.delete_selection();
        let mut from = None;
        if let Some(state) = &mut self.editor_state {
            from = Some(state.cursor_line);
            let line = &mut state.lines[state.cursor_line];
            let byte_idx = char_to_byte(line, state.cursor_col);
            let new_line = line[byte_idx..].to_string();
            line.truncate(byte_idx);
            state.cursor_line += 1;
            state.lines.insert(state.cursor_line, new_line);
            state.cursor_col = 0;
            state.modified = true;
            if state.cursor_line >= state.scroll_offset + self.editor_viewport_height {
                state.scroll_offset = state.cursor_line - self.editor_viewport_height + 1;
            }
        }
        if let Some(from) = from {
            self.editor_rehighlight_from(from);
        }
        self.finish_edit();
    }

    pub fn editor_save(&mut self) -> Result<(), String> {
        if let Some(state) = &mut self.editor_state {
            let content = state.lines.join(state.line_ending);
            crate::fs_ops::save_file(&state.file_path, content.as_bytes()).map_err(|e| e.to_string())?;
            state.modified = false;
            // A save written in place leaves its directory's mtime alone - only
            // adding, removing or renaming entries moves that - so the refresh
            // that watches it never notices, and the panels would go on showing
            // the size from before the edit. Either of them may be showing the
            // file, and a link to it may be in a directory neither is watching.
            self.reload_panel(true, None);
            self.reload_panel(false, None);
        }
        Ok(())
    }

    pub fn editor_is_modified(&self) -> bool {
        self.editor_state.as_ref().is_some_and(|s| s.modified)
    }

    pub fn reset_copy(&mut self) {
        self.is_f5_displayed = false;
        self.copy_items.clear();
    }

    pub fn reset_move(&mut self) {
        self.is_f6_displayed = false;
        self.move_items.clear();
    }

    pub fn toggle_selection(&mut self) {
        self.toggle_selection_inner(true);
    }

    pub fn toggle_selection_no_size(&mut self) {
        self.toggle_selection_inner(false);
    }

    fn toggle_selection_inner(&mut self, calculate_size: bool) {
        let mut to_size: Option<PathBuf> = None;

        {
            let (state, children, selected_set, current_dir) = if self.is_left_active {
                (&mut self.state_left, &self.children_left, &mut self.selected_left, &self.dir_left)
            } else {
                (&mut self.state_right, &self.children_right, &mut self.selected_right, &self.dir_right)
            };

            if let Some(index) = state.selected() {
                if index < children.len() && children[index].name != ".." {
                    let item = &children[index];

                    if !selected_set.remove(&item.name_os) {
                        selected_set.insert(item.name_os.clone());

                        if calculate_size && item.is_dir {
                            to_size = Some(item.path_in(current_dir));
                        }
                    }
                }

                // Move to next item
                let len = children.len();
                if len > 0 {
                    state.select(Some((index + 1).min(len - 1)));
                }
            }
        }

        if let Some(path) = to_size {
            self.start_dir_size(path);
        }
    }

    /// Work out a directory's size on a thread of its own, unless that is
    /// already under way.
    pub fn start_dir_size(&mut self, path: PathBuf) {
        if self.dir_sizing.iter().any(|sizing| sizing.path == path) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, updates) = mpsc::channel();
        let (worker_path, worker_cancel) = (path.clone(), Arc::clone(&cancel));
        std::thread::spawn(move || {
            let (mut found, mut reported) = (0, Instant::now());
            let result = crate::fs_ops::walk_dir_size(&worker_path, &worker_cancel, &mut |bytes| {
                found += bytes;
                if reported.elapsed() >= crate::constants::SIZE_PROGRESS_INTERVAL {
                    reported = Instant::now();
                    let _ = sender.send(SizeUpdate::Found(found));
                }
            });
            let _ = sender.send(SizeUpdate::Finished(result.map_err(|e| e.to_string())));
        });
        self.dir_sizing.push(DirSizing { path, found: 0, cancel, updates });
    }

    /// Take what the sizing threads have sent since the last frame, keeping
    /// each size that is finished.
    pub fn poll_dir_sizes(&mut self) {
        let mut finished = Vec::new();
        self.dir_sizing.retain_mut(|sizing| loop {
            match sizing.updates.try_recv() {
                Ok(SizeUpdate::Found(found)) => sizing.found = found,
                Ok(SizeUpdate::Finished(result)) => {
                    finished.push((sizing.path.clone(), result));
                    break false;
                }
                Err(TryRecvError::Empty) => break true,
                // Gone without a word, which only happens if it panicked.
                Err(TryRecvError::Disconnected) => break false,
            }
        });

        for (path, result) in finished {
            match result {
                Ok(Some(size)) => {
                    self.dir_sizes.insert(path, size);
                }
                Ok(None) => {}
                Err(e) => self.display_error(format!("Cannot calculate size: {}", e)),
            }
        }
    }

    /// Stop every size still being worked out. Each thread notices between
    /// entries; one stuck in a read on a dead mount is simply forgotten, and
    /// whatever it finds is never asked for.
    pub fn cancel_dir_sizes(&mut self) {
        for sizing in self.dir_sizing.drain(..) {
            sizing.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The running total for a directory still being sized.
    pub fn dir_size_so_far(&self, path: &Path) -> Option<u64> {
        self.dir_sizing.iter().find(|sizing| sizing.path == path).map(|sizing| sizing.found)
    }

    /// True while a popup is covering the screen. What is behind one must sit
    /// still: it cannot be seen, and the popup is answering for it.
    pub fn popup_is_open(&self) -> bool {
        self.is_error_displayed
            || self.is_f1_displayed
            || self.is_f11_displayed
            || self.is_f5_displayed
            || self.is_f6_displayed
            || self.is_f7_displayed
            || self.is_f8_displayed
            || self.is_editor_save_prompt
            || self.large_file.is_some()
            || self.overwrite_prompt.is_some()
            || self.job.is_some()
    }

    /// True while a dialog, prompt, viewer or editor owns the screen. The three
    /// added here are not popups: the viewer and the editor are whole-screen
    /// modes that take their own mouse input, and the rename prompt sits in the
    /// panel, where a click cancels it rather than being swallowed.
    pub fn is_modal_open(&self) -> bool {
        self.popup_is_open() || self.is_f2_displayed || self.is_f3_displayed || self.is_f4_displayed
    }

    /// Hand a long job to a worker thread and start following it.
    fn start_job<F>(&mut self, kind: TransferKind, work: F)
    where
        F: FnOnce(JobProgress<'_>) + Send + 'static,
    {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, updates) = mpsc::channel();
        let (answers, worker_answers) = mpsc::channel();
        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || work(JobProgress::new(&sender, &worker_cancel, &worker_answers)));

        self.job = Some(TransferJob {
            kind,
            total: None,
            done: 0,
            current: PathBuf::new(),
            started: Instant::now(),
            cancel,
            updates,
            problem: None,
            answers,
            skipped: 0,
        });
    }

    /// `overwrite` lets it write over names already taken: files replaced,
    /// directories merged. Without it, one taken since the check is refused.
    pub fn start_transfer(&mut self, items: Vec<(PathBuf, PathBuf, bool)>, is_copy: bool, overwrite: bool) {
        let kind = if is_copy { TransferKind::Copy } else { TransferKind::Move };
        self.start_job(kind, move |progress| run_transfer(items, is_copy, overwrite, progress));
    }

    pub fn start_delete(&mut self, items: Vec<(PathBuf, bool)>) {
        self.start_job(TransferKind::Delete, move |progress| run_delete(items, progress));
    }

    /// Ask a running transfer to stop. It ends at the next chunk or file, so
    /// the job stays up for a moment afterwards rather than vanishing at once.
    /// One waiting on a problem is answered Abort, which is the same thing.
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
                Ok(JobUpdate::Problem(problem)) => job.problem = Some(problem),
                Ok(JobUpdate::Skipped) => job.skipped += 1,
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

        // Both panels: a move empties one and fills the other, and a copy into
        // the same directory shows up on the side it came from.
        self.reload_panel(true, None);
        self.reload_panel(false, None);
        self.clear_active_selections();
        // Any directory size worked out before may now be wrong - the ones
        // copied into, moved out of or deleted from, and every directory above
        // them. Space works them out again. One still being counted may
        // have walked through them halfway, so it goes too.
        self.dir_sizes.clear();
        self.cancel_dir_sizes();

        // The offer to quit covered this job, and this job is over.
        self.quit_armed = false;

        match result {
            Ok(Transfer::Done) if job.skipped > 0 => {
                let entries = if job.skipped == 1 { "entry" } else { "entries" };
                self.display_error(format!("{} finished, {} {entries} skipped", job.kind.title(), job.skipped))
            }
            Ok(Transfer::Done) => {}
            Ok(Transfer::Cancelled) => {
                let far = match job.kind {
                    TransferKind::Delete => format!("{} entries", job.done),
                    _ => crate::utils::format_size(job.done),
                };
                self.display_error(format!("{} cancelled after {far}", job.kind.title()))
            }
            Err(e) => self.display_error(e),
        }
    }

    /// Remember a directory's mtime so a later change to it stands out.
    pub fn record_dir_stamp(&mut self, is_left: bool) {
        let dir = if is_left { &self.dir_left } else { &self.dir_right };
        let stamp = std::fs::metadata(dir).and_then(|metadata| metadata.modified()).ok();
        if is_left {
            self.dir_stamp_left = stamp;
        } else {
            self.dir_stamp_right = stamp;
        }
        self.record_disk_usage(is_left);
    }

    /// Read the panel filesystem's used/total. Off the render path: statvfs is
    /// a syscall and blocks outright on an unresponsive network mount.
    pub fn record_disk_usage(&mut self, is_left: bool) {
        let dir = if is_left { &self.dir_left } else { &self.dir_right };
        let usage = disk_usage(dir);
        if is_left {
            self.disk_left = usage;
        } else {
            self.disk_right = usage;
        }
    }

    /// Point a panel at `dir` and read it. The single way a panel's directory
    /// changes - navigation, and anything else that jumps somewhere. `select`
    /// names the entry to land on, otherwise the cursor goes to the top.
    pub fn open_dir(&mut self, is_left: bool, dir: PathBuf, select: Option<&std::ffi::OsStr>) {
        // A directory that is there but cannot be read - /root, to anyone
        // else - is refused before the panel moves. Moved first, the panel
        // went on listing the directory it left under the name of one it
        // could not show, and every operation built its paths in the wrong
        // place. One that is not there at all goes ahead, for the reload to
        // climb from.
        if dir.exists()
            && let Err(e) = std::fs::read_dir(&dir)
        {
            self.display_error(format!("Cannot open {}: {}", dir.display(), e));
            return;
        }
        if is_left {
            self.dir_left = dir;
            self.selected_left.clear();
            self.state_left.select(Some(0));
        } else {
            self.dir_right = dir;
            self.selected_right.clear();
            self.state_right.select(Some(0));
        }
        self.search_clear();
        // Handles a vanished target too, by climbing to the nearest parent.
        self.reload_panel(is_left, select);
    }

    /// Ctrl+Left and Ctrl+Right, as in Total Commander: the arrow names the
    /// panel, and it is sent to the directory under the cursor in the active
    /// one. On `..` that is the parent, landing on the directory left; on a
    /// file it is the directory the file is in, landing on the file. The focus
    /// stays where it is. Pointed at the active panel itself, it simply goes in.
    pub fn open_in_panel(&mut self, is_left: bool) {
        let (children, state, dir) = if self.is_left_active {
            (&self.children_left, &self.state_left, &self.dir_left)
        } else {
            (&self.children_right, &self.state_right, &self.dir_right)
        };
        let item = state.selected().and_then(|index| children.get(index));
        let (target, select) = match item {
            Some(item) if item.name == ".." => match dir.parent() {
                Some(parent) => (parent.to_path_buf(), dir.file_name().map(std::ffi::OsStr::to_os_string)),
                None => return,
            },
            Some(item) if item.is_dir => (item.path_in(dir), None),
            Some(item) => (dir.clone(), Some(item.name_os.clone())),
            None => (dir.clone(), None),
        };
        self.open_dir(is_left, target, select.as_deref());
    }

    /// Reread one panel from disk. `prefer` names the entry to land on - the
    /// file just renamed or created. Otherwise the cursor keeps the *file* it
    /// was on rather than the row, since entries appearing or vanishing above
    /// shift every index below them; if that file is gone, the row is kept.
    pub fn reload_panel(&mut self, is_left: bool, prefer: Option<&std::ffi::OsStr>) {
        let dir = if is_left { self.dir_left.clone() } else { self.dir_right.clone() };

        // The directory may have been removed underneath us; climb to the
        // nearest ancestor that still exists rather than sitting on an error.
        let (dir, relocated) = match nearest_existing_dir(&dir) {
            Some(found) if found == dir => (dir, false),
            Some(found) => (found, true),
            None => {
                self.display_error(format!("No such directory: {}", dir.display()));
                return;
            }
        };

        if relocated {
            // Cursor, search and selection all referred to entries that are gone.
            if is_left {
                self.dir_left = dir.clone();
                self.selected_left.clear();
                self.state_left.select(Some(0));
            } else {
                self.dir_right = dir.clone();
                self.selected_right.clear();
                self.state_right.select(Some(0));
            }
            self.search_clear();
        }

        let (children, state) = if is_left {
            (&self.children_left, &self.state_left)
        } else {
            (&self.children_right, &self.state_right)
        };
        let previous_index = state.selected().unwrap_or(0);
        let wanted = if relocated {
            None
        } else {
            // By the name the filesystem holds: two that are not valid UTF-8
            // can read the same, and the cursor would land on the wrong one.
            prefer.map(std::ffi::OsStr::to_os_string).or_else(|| {
                children.get(previous_index).map(|item| item.name_os.clone())
            })
        };

        match load_directory_rows(&dir, &self.options) {
            Ok(items) => {
                let selected = if is_left { &mut self.selected_left } else { &mut self.selected_right };
                prune_selection(selected, &items);

                let index = wanted
                    .and_then(|name| items.iter().position(|item| item.name_os == name))
                    .unwrap_or(previous_index)
                    .min(items.len().saturating_sub(1));

                if is_left {
                    self.children_left = items;
                    self.state_left.select(Some(index));
                } else {
                    self.children_right = items;
                    self.state_right.select(Some(index));
                }
                self.record_dir_stamp(is_left);
            }
            Err(e) => {
                self.display_error(e.to_string());
                // Taken as read all the same, so the refresh only tries again
                // once the directory changes. Otherwise it found the stamp
                // out of date every second and put the same error back up
                // each time it was closed.
                self.record_dir_stamp(is_left);
            }
        }

        // Both of these keep what they gathered until the cursor moves to some
        // other entry, which is what makes them cheap to hold. A reread is the
        // other way the entry under the cursor changes - it was saved in the
        // editor, written over by a copy, or changed by something else
        // entirely - and neither would notice that on its own. Dropping them
        // here has them gathered again before the next frame, since the loop
        // refreshes both on its way to drawing one.
        self.preview = None;
        self.cursor_detail = None;
    }

    /// The panel and the mount under a click, when it landed on a drive icon.
    /// The strip runs the width of its half, so most of it is not an icon: a
    /// click on the label beside them, or past the last one, is not a drive.
    pub fn drive_at(&self, column: u16, row: u16) -> Option<(bool, usize)> {
        let position = Position::new(column, row);
        let is_left = if self.drive_strip_left.contains(position) {
            true
        } else if self.drive_strip_right.contains(position) {
            false
        } else {
            return None;
        };

        let strip = if is_left { self.drive_strip_left } else { self.drive_strip_right };
        let offset = column - strip.x;
        let index = slot_at(&self.drive_slots, offset)?;
        (index < self.mounts.len()).then_some((is_left, index))
    }

    /// The mount a panel is sitting on: the longest one its directory is under.
    pub fn current_mount(&self, is_left: bool) -> Option<usize> {
        let dir = if is_left { &self.dir_left } else { &self.dir_right };
        self.mounts
            .iter()
            .enumerate()
            .filter(|(_, mount)| dir.starts_with(&mount.path))
            .max_by_key(|(_, mount)| mount.path.as_os_str().len())
            .map(|(index, _)| index)
    }

    /// Start choosing a drive for a panel, highlighting the one it is on.
    pub fn open_drive_picker(&mut self, is_left: bool) {
        // Re-read so a stick plugged in a moment ago shows up.
        self.mounts = list_mounts();
        if self.mounts.is_empty() {
            return;
        }
        let current = self.current_mount(is_left).unwrap_or(0);
        self.drive_picker = Some((is_left, current));
    }

    pub fn move_drive_picker(&mut self, forward: bool) {
        if let Some((is_left, index)) = self.drive_picker {
            let count = self.mounts.len();
            let next = if forward { (index + 1) % count } else { (index + count - 1) % count };
            self.drive_picker = Some((is_left, next));
        }
    }

    /// Take the highlighted drive; the panel jumps to that mount point.
    pub fn confirm_drive_picker(&mut self) {
        if let Some((is_left, index)) = self.drive_picker.take()
            && let Some(mount) = self.mounts.get(index)
        {
            let path = mount.path.clone();
            self.open_dir(is_left, path, None);
        }
    }

    /// Reread any panel whose directory changed underneath us. Called once per
    /// frame; the interval keeps it to a couple of stat calls a second.
    pub fn refresh_stale_panels(&mut self) {
        // Reloading under an open dialog would move things out from under the user.
        if self.is_modal_open() || self.last_refresh_check.elapsed() < crate::constants::REFRESH_INTERVAL {
            return;
        }
        self.last_refresh_check = Instant::now();

        // A drive appearing or going away should show up in the strip.
        self.mounts = list_mounts();

        for is_left in [true, false] {
            // Free space moves without the directory changing - a copy anywhere
            // else on the same filesystem shifts it - so this updates every tick.
            self.record_disk_usage(is_left);

            let dir = if is_left { &self.dir_left } else { &self.dir_right };
            let stamp = std::fs::metadata(dir).and_then(|metadata| metadata.modified()).ok();
            let known = if is_left { self.dir_stamp_left } else { self.dir_stamp_right };
            if stamp != known {
                self.reload_panel(is_left, None);
            }
        }
    }

    /// Full path of the entry under the cursor, unless that is the parent entry.
    fn cursor_target(&self) -> Option<(PathBuf, String, bool)> {
        let (children, state, dir) = if self.is_left_active {
            (&self.children_left, &self.state_left, &self.dir_left)
        } else {
            (&self.children_right, &self.state_right, &self.dir_right)
        };
        let item = children.get(state.selected()?)?;
        if item.name == ".." {
            return None;
        }
        Some((item.path_in(dir), item.name_full.clone(), item.is_dir))
    }

    /// Gather what the detail lines say, when the cursor has moved to something
    /// else. Same shape as the preview below it, and for the same reason.
    pub fn refresh_cursor_detail(&mut self) {
        let target = self.cursor_target();
        let path = target.as_ref().map(|(path, _, _)| path.clone());
        if self.cursor_detail.as_ref().is_some_and(|detail| detail.path == path) {
            return;
        }

        let Some((path, name, is_dir)) = target else {
            self.cursor_detail = Some(CursorDetail {
                path: None,
                name: String::new(),
                size: String::new(),
                modified: String::new(),
                owner: String::new(),
                attributes: String::new(),
                link: None,
            });
            return;
        };

        let described = crate::fs_ops::describe(&path, is_dir);
        self.cursor_detail = Some(CursorDetail {
            name,
            // The exact count, since the column rounds it to something like
            // "9 MiB" and the difference is the point of showing it again.
            size: described
                .as_ref()
                .and_then(|described| described.size_bytes)
                .map(|bytes| format!("{} bytes", crate::utils::grouped(bytes)))
                .unwrap_or_default(),
            // To the second, which the column has no room for either.
            modified: described
                .as_ref()
                .and_then(|described| described.modified)
                .map(|at| {
                    let at: chrono::DateTime<chrono::Local> = at.into();
                    at.format("%d/%m/%y %H:%M:%S").to_string()
                })
                .unwrap_or_default(),
            owner: described.as_ref().map(|described| described.owner.clone()).unwrap_or_default(),
            attributes: described.as_ref().map(|described| described.attributes.clone()).unwrap_or_default(),
            link: described.and_then(|described| described.link),
            path: Some(path),
        });
    }

    /// Keep the preview pointed at whatever the cursor is on. Reads only when
    /// the target actually changed, so holding an arrow key stays cheap.
    pub fn refresh_preview(&mut self) {
        if !self.is_f12_displayed {
            self.preview = None;
            return;
        }

        let target = self.cursor_target();
        let path = target.as_ref().map(|(path, _, _)| path.clone());
        if self.preview.as_ref().is_some_and(|preview| preview.path == path) {
            return;
        }

        let (label, content) = match &target {
            None => (String::new(), crate::viewer::Preview::Lines(Vec::new())),
            Some((path, name, true)) => {
                let lines = match std::fs::read_dir(path) {
                    Ok(entries) => vec![format!("{} items", entries.count())],
                    Err(e) => vec![e.to_string()],
                };
                (name.clone(), crate::viewer::Preview::Lines(lines))
            }
            Some((path, name, false)) => (
                name.clone(),
                crate::viewer::load_preview(path, crate::constants::PREVIEW_MAX_BYTES, crate::constants::PREVIEW_MAX_LINES),
            ),
        };
        let (lines, bytes) = match content {
            crate::viewer::Preview::Lines(lines) => (lines, Vec::new()),
            crate::viewer::Preview::Bytes(bytes) => (Vec::new(), bytes),
        };

        self.preview = Some(PreviewState { path, label, lines, bytes });
    }

    pub fn clear_all_selections(&mut self) {
        self.selected_left.clear();
        self.selected_right.clear();
    }

    pub fn clear_active_selections(&mut self) {
        if self.is_left_active {
            self.selected_left.clear();
        } else {
            self.selected_right.clear();
        }
    }
}

impl EditorState {
    /// The selection as ((line, col), (line, col)) in document order, or None
    /// when nothing is selected.
    pub fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let anchor = self.selection_anchor?;
        let cursor = (self.cursor_line, self.cursor_col);
        if anchor == cursor {
            return None;
        }
        Some(if anchor < cursor { (anchor, cursor) } else { (cursor, anchor) })
    }

    /// The selected text, with '\n' between lines whatever the file uses.
    pub fn selected_text(&self) -> Option<String> {
        let ((first_line, first_col), (last_line, last_col)) = self.selection()?;

        if first_line == last_line {
            return Some(char_slice(&self.lines[first_line], first_col, last_col));
        }

        let first = &self.lines[first_line];
        let mut text = char_slice(first, first_col, first.chars().count());
        for line in &self.lines[first_line + 1..last_line] {
            text.push('\n');
            text.push_str(line);
        }
        text.push('\n');
        text.push_str(&char_slice(&self.lines[last_line], 0, last_col));
        Some(text)
    }

    /// Settle the vertical scroll before a frame is drawn. It never runs past
    /// the last screenful - an edit that shortens the file, such as deleting a
    /// selection made with Ctrl+A, would otherwise leave it pointing past the
    /// end - and after a key, which sets auto_scroll, it brings the cursor into
    /// view: a paste, an undo or a deleted selection can each land it off
    /// screen. The mouse wheel clears auto_scroll, so reading elsewhere in the
    /// file does not snap back to the cursor.
    pub fn keep_in_view(&mut self, height: usize) {
        let height = height.max(1);
        if self.auto_scroll {
            if self.cursor_line < self.scroll_offset {
                self.scroll_offset = self.cursor_line;
            } else if self.cursor_line >= self.scroll_offset + height {
                self.scroll_offset = self.cursor_line + 1 - height;
            }
        }
        self.scroll_offset = self.scroll_offset.min(self.lines.len().saturating_sub(height));
    }

    /// Clamp cursor_col to current line length.
    fn clamp_col(&mut self) {
        let line_len = self.lines[self.cursor_line].chars().count();
        self.cursor_col = self.cursor_col.min(line_len);
    }
}

/// Pick the terminator to save a file with. A mixed file normalises to whichever
/// style already dominates; a file with no newline at all gets "\n".
fn detect_line_ending(content: &str) -> &'static str {
    let crlf = content.matches("\r\n").count();
    let lf = content.matches('\n').count() - crlf;
    if crlf > lf { "\r\n" } else { "\n" }
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

/// Drop selections whose file is no longer there.
///
/// A selection is keyed by name so it can follow its file across a re-sort, but
/// that leaves the name behind when the file itself goes. Nothing shows it: the
/// count and the operations all walk the files and ask whether each is
/// selected, so a name with no file behind it is silently inert - until
/// something takes that name again, at which point a file the user never picked
/// is selected, and joins the next copy, move or delete. Worse, those read the
/// selection *instead of* the cursor whenever it is not empty, so one such name
/// quietly redirects the whole operation.
fn prune_selection(selected: &mut HashSet<OsString>, items: &[Item]) {
    let names: HashSet<&OsString> = items.iter().map(|item| &item.name_os).collect();
    selected.retain(|name| names.contains(name));
}

/// Which of a strip's slots a column falls in, measured from the strip's left
/// edge. None for the gaps around them: the space the strip opens with, and
/// everything past the last icon, where the mount's name is written.
fn slot_at(slots: &[(u16, u16)], offset: u16) -> Option<usize> {
    slots.iter().position(|&(start, width)| offset >= start && offset < start + width)
}

/// The worker thread behind a copy or move.
///
/// Renames come first and on their own. A move within one filesystem is a
/// rename, which takes no time and moves no bytes - measuring a large tree
/// before doing it would hold up a transfer that was about to be instant.
fn run_transfer(items: Vec<(PathBuf, PathBuf, bool)>, is_copy: bool, overwrite: bool, mut progress: JobProgress<'_>) {
    let mut remaining = Vec::new();
    for (source, dest, is_dir) in items {
        // rename() replaces a file, or an empty directory, without a word, so
        // it only goes over a taken name when that was asked for. A directory
        // with anything in it refuses, and is merged by move_path instead.
        if !is_copy && (overwrite || !path_exists(&dest)) && rename_in_place(&source, &dest) {
            continue;
        }
        remaining.push((source, dest, is_dir));
    }

    // Only what is actually going to be copied needs counting.
    let _ = progress.updates.send(JobUpdate::Total(measure(&remaining)));

    for (source, dest, is_dir) in remaining {
        let result = if is_copy {
            copy_path(source, dest, is_dir, &mut progress)
        } else {
            move_path(source, dest, overwrite, &mut progress)
        };
        match result {
            Ok(Transfer::Done) => {}
            outcome => return progress.finish(outcome),
        }
    }
    progress.finish(Ok(Transfer::Done));
}

/// Where a scroll offset belongs after the drawing changed size, so that the
/// middle of the pane goes on showing the same part of the picture. `extent` is
/// the pane's size along the axis being moved.
///
/// A drawing smaller than the pane is centred by the renderer rather than
/// scrolled, so it carries no offset of its own - which is both where such a
/// drawing starts from and where it lands.
fn recentre(offset: usize, extent: usize, before: usize, after: usize) -> usize {
    if before == 0 || after <= extent {
        return 0;
    }
    // Where the middle of the pane falls on the old drawing, as a fraction of
    // it. One that fitted was centred, so its middle was the picture's own.
    let middle = if before <= extent { 0.5 } else { (offset as f64 + extent as f64 / 2.0) / before as f64 };
    let centre = middle * after as f64 - extent as f64 / 2.0;
    (centre.round().max(0.0) as usize).min(after - extent)
}

/// A character range of a line, as a new String.
fn char_slice(line: &str, from: usize, to: usize) -> String {
    line[char_to_byte(line, from)..char_to_byte(line, to)].to_string()
}

/// Convert a char index to a byte index in a string.
fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod editor_tests {
    use super::AppState;

    fn editor_with(name: &str, text: &str) -> (AppState, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("fm84-{name}-{}.txt", std::process::id()));
        std::fs::write(&path, text).unwrap();
        let mut app_state = AppState::new();
        app_state.options = crate::options::Options::default();
        app_state.open_editor(path.clone()).unwrap();
        (app_state, path)
    }

    /// Type into the prompt as a user would, and press Enter.
    fn answer_prompt(app_state: &mut AppState, kind: super::PromptKind, text: &str) {
        app_state.open_prompt(kind);
        let input = &mut app_state.prompt.as_mut().unwrap().1;
        input.clear();
        text.chars().for_each(|c| input.insert(c));
        app_state.confirm_prompt();
    }

    fn selected(app_state: &AppState) -> Option<((usize, usize), (usize, usize))> {
        app_state.editor_state.as_ref().unwrap().selection()
    }

    #[test]
    fn find_selects_each_match_in_turn_and_wraps() {
        let (mut app_state, path) = editor_with("find", "alpha beta\nBETA\ngamma beta\n");
        app_state.editor_viewport_height = 10;

        // F3 before anything was looked for asks what to look for.
        app_state.find(true, false);
        assert_eq!(app_state.prompt.as_ref().map(|(kind, _)| *kind), Some(super::PromptKind::Find));
        app_state.prompt = None;

        answer_prompt(&mut app_state, super::PromptKind::Find, "beta");
        assert_eq!(selected(&app_state), Some(((0, 6), (0, 10))));
        assert!(app_state.find_shown);
        assert_eq!(app_state.find_note, None);

        app_state.find(true, false);
        assert_eq!(selected(&app_state), Some(((1, 0), (1, 4))), "lower case finds any case");
        app_state.find(true, false);
        assert_eq!(selected(&app_state), Some(((2, 6), (2, 10))));
        app_state.find(true, false);
        assert_eq!(selected(&app_state), Some(((0, 6), (0, 10))));
        assert_eq!(app_state.find_note.as_deref(), Some("Wrapped to the top"));

        app_state.find(false, false);
        assert_eq!(selected(&app_state), Some(((2, 6), (2, 10))));
        assert_eq!(app_state.find_note.as_deref(), Some("Wrapped to the bottom"));

        // The prompt offers the last term again, and a miss moves nothing.
        app_state.open_prompt(super::PromptKind::Find);
        assert_eq!(app_state.prompt.as_ref().unwrap().1.text, "beta");
        app_state.prompt = None;
        answer_prompt(&mut app_state, super::PromptKind::Find, "BETAMAX");
        assert_eq!(selected(&app_state), Some(((2, 6), (2, 10))));
        assert_eq!(app_state.find_note.as_deref(), Some("Not found: BETAMAX"));

        // Closing the editor takes the marks with it, and keeps the term.
        app_state.close_editor();
        assert!(!app_state.find_shown);
        assert_eq!(app_state.find_term, "BETAMAX");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn go_to_line_puts_the_cursor_on_it_and_keeps_inside_the_file() {
        let text: String = (1..=100).map(|n| format!("line {n}\n")).collect();
        let (mut app_state, path) = editor_with("goto", &text);
        app_state.editor_viewport_height = 10;

        answer_prompt(&mut app_state, super::PromptKind::GoToLine, "42");
        let state = app_state.editor_state.as_ref().unwrap();
        assert_eq!((state.cursor_line, state.cursor_col), (41, 0));
        assert!(state.scroll_offset <= 41 && 41 < state.scroll_offset + 10, "the line is on screen");

        answer_prompt(&mut app_state, super::PromptKind::GoToLine, "100000");
        assert_eq!(app_state.editor_state.as_ref().unwrap().cursor_line, 100, "the last line");
        answer_prompt(&mut app_state, super::PromptKind::GoToLine, "0");
        assert_eq!(app_state.editor_state.as_ref().unwrap().cursor_line, 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn the_viewer_finds_and_goes_to_lines_too() {
        let path = std::env::temp_dir().join(format!("fm84-view-find-{}.txt", std::process::id()));
        let text: String = (1..=50).map(|n| if n == 30 { "\tthe needle\n".to_string() } else { format!("hay {n}\n") }).collect();
        std::fs::write(&path, text).unwrap();
        let mut app_state = AppState::new();
        app_state.options = crate::options::Options::default();
        app_state.open_viewer(path.clone()).unwrap();
        app_state.viewer_viewport_height = 10;
        app_state.viewer_viewport_width = 40;

        answer_prompt(&mut app_state, super::PromptKind::Find, "needle");
        let state = app_state.viewer_state.as_ref().unwrap();
        // Columns as drawn, with the tab opened out, so the selection and the
        // copy line up with the screen.
        let start = crate::display::tab_width() + 4;
        assert_eq!(state.selected_range(), Some(((29, start), (29, start + 6))));
        assert_eq!(state.selected_text().as_deref(), Some("needle"));
        assert!(state.scroll_offset <= 29 && 29 < state.scroll_offset + 10);

        answer_prompt(&mut app_state, super::PromptKind::GoToLine, "5");
        assert_eq!(app_state.viewer_state.as_ref().unwrap().scroll_offset, 4);
        answer_prompt(&mut app_state, super::PromptKind::GoToLine, "50");
        // As far down as End goes, which leaves the last screenful showing.
        app_state.viewer_end();
        let end = app_state.viewer_state.as_ref().unwrap().scroll_offset;
        app_state.viewer_home();
        answer_prompt(&mut app_state, super::PromptKind::GoToLine, "50");
        assert_eq!(app_state.viewer_state.as_ref().unwrap().scroll_offset, end, "no further than the end allows");
        std::fs::remove_file(path).unwrap();
    }

    fn stacks(app_state: &AppState) -> (usize, usize) {
        let state = app_state.editor_state.as_ref().unwrap();
        (state.undo_stack.len(), state.redo_stack.len())
    }

    #[test]
    fn saving_updates_the_size_the_panels_show() {
        let dir = std::env::temp_dir().join(format!("fm84-save-size-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.txt");
        std::fs::write(&path, "ab").unwrap();

        let mut app_state = AppState::new();
        app_state.options = crate::options::Options::default();
        app_state.dir_left = dir.clone();
        app_state.dir_right = dir.clone();
        app_state.reload_panel(true, None);
        app_state.reload_panel(false, None);
        let size = |children: &[super::Item]| children.iter().find(|item| item.name_full == "notes.txt").unwrap().size_bytes;
        assert_eq!(size(&app_state.children_left), 2);

        app_state.open_editor(path.clone()).unwrap();
        app_state.editor_insert_text("more text");
        app_state.editor_save().unwrap();
        assert_eq!(size(&app_state.children_left), 11);
        assert_eq!(size(&app_state.children_right), 11);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_edit_that_changes_nothing_keeps_the_redo_history() {
        let (mut app_state, path) = editor_with("noop", "ab\ncd");
        // One real edit, undone, so there is something to redo.
        app_state.editor_end();
        app_state.editor_insert_char('x');
        app_state.editor_undo();
        assert_eq!(stacks(&app_state), (0, 1));

        // Backspace at the very start, Delete at the very end, and Ctrl+X with
        // nothing selected: none of them change the text.
        app_state.editor_state.as_mut().unwrap().cursor_col = 0;
        app_state.editor_backspace();
        app_state.editor_cut();
        {
            let state = app_state.editor_state.as_mut().unwrap();
            state.cursor_line = 1;
            state.cursor_col = 2;
        }
        app_state.editor_delete();
        assert_eq!(stacks(&app_state), (0, 1));
        app_state.editor_redo();
        assert_eq!(app_state.editor_state.as_ref().unwrap().lines, ["abx", "cd"]);

        // A real edit still forks the history.
        app_state.editor_undo();
        app_state.editor_insert_char('y');
        assert_eq!(stacks(&app_state), (1, 0));
        std::fs::remove_file(path).unwrap();
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

#[cfg(test)]
mod tests {
    use super::{AppState, Item, prune_selection, recentre, slot_at};
    use crate::options::Options;
    use std::collections::HashSet;

    /// A file name is bytes, and need not be valid UTF-8. The strings the
    /// interface shows come from to_string_lossy, so joining one back together
    /// names a file that is not there - every path has to come from the name
    /// the filesystem gave.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_still_names_its_file() {
        use std::os::unix::ffi::OsStrExt;

        let dir = std::env::temp_dir().join(format!("fm84-latin1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // "café.txt" as Latin-1 writes it, which is not valid UTF-8.
        let raw = std::ffi::OsStr::from_bytes(b"caf\xe9.txt");
        std::fs::write(dir.join(raw), "contents").unwrap();

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.open_dir(true, dir.clone(), None);
        let item = app_state.children_left.iter().find(|item| item.name_full != "..").unwrap().clone();
        // Blank until the stem was read lossily too, which left the column empty.
        assert!(!item.name.is_empty());

        let index = app_state.children_left.iter().position(|row| row.name_full == item.name_full).unwrap();
        app_state.state_left.select(Some(index));
        let (path, _, _) = app_state.cursor_target().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "contents");
        // What the name alone would have named, and what every operation used
        // to be handed: a path with a replacement character in it, and nothing
        // of that name on disk.
        assert!(!dir.join(&item.name_full).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The preview and the detail lines hold what they gathered until the
    /// cursor moves, so a file that changes under a still cursor has to be
    /// picked up by the reread instead.
    #[test]
    fn a_reread_gathers_the_preview_and_the_detail_again() {
        let dir = std::env::temp_dir().join(format!("fm84-reread-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "hello").unwrap();

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.is_f12_displayed = true;
        app_state.open_dir(true, dir.clone(), Some("notes.txt".as_ref()));
        app_state.refresh_cursor_detail();
        app_state.refresh_preview();
        assert_eq!(app_state.cursor_detail.as_ref().unwrap().size, "5 bytes");
        assert_eq!(app_state.preview.as_ref().unwrap().lines, ["hello"]);

        // The file grows where it stands, and the panel rereads - which is
        // what saving in the editor, or a copy landing on it, comes to.
        std::fs::write(dir.join("notes.txt"), "hello, a longer file now").unwrap();
        app_state.reload_panel(true, None);
        app_state.refresh_cursor_detail();
        app_state.refresh_preview();
        assert_eq!(app_state.cursor_detail.as_ref().unwrap().size, "24 bytes");
        assert_eq!(app_state.preview.as_ref().unwrap().lines, ["hello, a longer file now"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Space sizes a directory on a thread, so the panel goes on answering
    /// while it counts, and the size lands once the walk is done.
    #[test]
    fn space_sizes_a_directory_off_the_ui_thread() {
        let dir = std::env::temp_dir().join(format!("fm84-sizing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("data.bin"), vec![0u8; 4096]).unwrap();

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.open_dir(true, dir.clone(), Some("sub".as_ref()));
        app_state.toggle_selection();
        assert_eq!(app_state.dir_sizing.len(), 1);

        let started = std::time::Instant::now();
        while !app_state.dir_sizing.is_empty() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "sizing never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_state.poll_dir_sizes();
        }
        assert_eq!(app_state.dir_sizes.get(&dir.join("sub")), Some(&4096));

        // Esc stops one under way, and nothing it finds is kept.
        app_state.dir_sizes.clear();
        app_state.start_dir_size(dir.join("sub"));
        app_state.cancel_dir_sizes();
        assert!(app_state.dir_sizing.is_empty());
        app_state.poll_dir_sizes();
        assert!(app_state.dir_sizes.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A copy that meets an entry it cannot copy stops and asks. Skipped, the
    /// rest goes on, and the end says how many were left.
    #[cfg(unix)]
    #[test]
    fn a_job_asks_about_a_failure_and_goes_on_when_told_to_skip() {
        let dir = std::env::temp_dir().join(format!("fm84-ask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let tree = dir.join("tree");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join("a.txt"), "a").unwrap();
        let pipe = std::ffi::CString::new(tree.join("pipe").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        std::fs::write(tree.join("z.txt"), "z").unwrap();

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.start_transfer(vec![(tree.clone(), dir.join("copy"), true)], true, false);

        let started = std::time::Instant::now();
        let wait = |app_state: &mut AppState, done: &dyn Fn(&AppState) -> bool| {
            while !done(app_state) {
                assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never got there");
                std::thread::sleep(std::time::Duration::from_millis(5));
                app_state.poll_transfer();
            }
        };
        wait(&mut app_state, &|app_state| app_state.job.as_ref().is_some_and(|job| job.problem.is_some()));
        assert_eq!(app_state.job.as_ref().unwrap().problem.as_ref().unwrap().path, tree.join("pipe"));

        app_state.job.as_mut().unwrap().answer(super::Answer::Skip);
        wait(&mut app_state, &|app_state| app_state.job.is_none());
        assert_eq!(app_state.error_message, "Copy finished, 1 entry skipped");
        assert!(dir.join("copy").join("a.txt").exists());
        assert!(dir.join("copy").join("z.txt").exists());

        // Esc on the question stops the job there, and says it was cancelled
        // rather than reporting the failure it was asked about.
        app_state.start_transfer(vec![(tree.clone(), dir.join("again"), true)], true, false);
        wait(&mut app_state, &|app_state| app_state.job.as_ref().is_some_and(|job| job.problem.is_some()));
        app_state.cancel_transfer();
        wait(&mut app_state, &|app_state| app_state.job.is_none());
        assert!(app_state.error_message.starts_with("Copy cancelled"), "{}", app_state.error_message);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A directory that cannot be read is refused, and the panel stays where
    /// it was rather than moving there with nothing to show. The error is said
    /// once: the refresh does not raise it again after it is closed.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_directory_is_refused_once() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("fm84-unreadable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let locked = dir.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(dir.join("here.txt"), "").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root reads anything, so there is nothing to refuse.
        if std::fs::read_dir(&locked).is_ok() {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
            return std::fs::remove_dir_all(&dir).unwrap();
        }

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.open_dir(true, dir.clone(), None);
        let listed = app_state.children_left.len();

        app_state.open_dir(true, locked.clone(), None);
        assert!(app_state.is_error_displayed);
        assert_eq!(app_state.dir_left, dir);
        assert_eq!(app_state.children_left.len(), listed);

        // Closed, and a refresh comes round: it stays closed.
        app_state.reset_error();
        app_state.last_refresh_check -= crate::constants::REFRESH_INTERVAL;
        app_state.refresh_stale_panels();
        assert!(!app_state.is_error_displayed);

        // Gone unreadable while the panel is in it: said once, not every tick.
        app_state.open_dir(true, locked.parent().unwrap().to_path_buf(), None);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        app_state.open_dir(true, locked.clone(), None);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        app_state.reload_panel(true, None);
        assert!(app_state.is_error_displayed);
        app_state.reset_error();
        app_state.last_refresh_check -= crate::constants::REFRESH_INTERVAL;
        app_state.refresh_stale_panels();
        assert!(!app_state.is_error_displayed);

        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ctrl_arrows_send_the_entry_under_the_cursor_to_a_panel() {
        let dir = std::env::temp_dir().join(format!("fm84-open-in-panel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        std::fs::write(dir.join("note.txt"), "").unwrap();

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.open_dir(true, dir.clone(), None);
        app_state.open_dir(false, std::env::temp_dir(), None);
        app_state.is_left_active = true;
        let at = |app_state: &AppState, name: &str| app_state.children_left.iter().position(|item| item.name_full == name);
        let cursor_right = |app_state: &AppState| {
            app_state.state_right.selected().and_then(|index| app_state.children_right.get(index)).map(|item| item.name_full.clone())
        };

        // A directory: the other panel goes into it, and the focus stays.
        app_state.state_left.select(at(&app_state, "inner"));
        app_state.open_in_panel(false);
        assert_eq!(app_state.dir_right, dir.join("inner"));
        assert!(app_state.is_left_active);
        assert_eq!(app_state.dir_left, dir);

        // A file: the directory it is in, on the file.
        app_state.state_left.select(at(&app_state, "note.txt"));
        app_state.open_in_panel(false);
        assert_eq!(app_state.dir_right, dir);
        assert_eq!(cursor_right(&app_state).as_deref(), Some("note.txt"));

        // The parent entry: the parent, on the directory left.
        app_state.state_left.select(at(&app_state, ".."));
        app_state.open_in_panel(false);
        assert_eq!(app_state.dir_right, std::env::temp_dir());
        assert_eq!(cursor_right(&app_state), dir.file_name().map(|name| name.to_string_lossy().into_owned()));

        // Pointed at the active panel, it goes in there.
        app_state.state_left.select(at(&app_state, "inner"));
        app_state.open_in_panel(true);
        assert_eq!(app_state.dir_left, dir.join("inner"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn row(name: &str) -> Item {
        Item {
            name_os: name.into(),
            name_full: name.to_string(),
            name: name.to_string(),
            extension: String::new(),
            is_dir: false,
            size: String::new(),
            size_bytes: 0,
            modified_at: None,
            attributes: String::new(),
        }
    }

    #[test]
    fn plus_minus_and_star_select_by_pattern() {
        let mut app_state = AppState::new();
        app_state.is_left_active = true;
        let directory = Item { is_dir: true, ..row("photos") };
        app_state.children_left = vec![row(".."), directory, row("a.jpg"), row("B.JPG"), row("c.png"), row("notes.txt")];
        let selected = |app_state: &AppState| {
            let mut names: Vec<String> = app_state.selected_left.iter().map(|name| name.to_string_lossy().into_owned()).collect();
            names.sort();
            names
        };

        // Through the prompt, as + then typing then Enter does.
        app_state.open_select_prompt(true);
        app_state.prompt.as_mut().unwrap().1.set("*.jpg;*.png".to_string());
        app_state.confirm_prompt();
        assert_eq!(selected(&app_state), ["B.JPG", "a.jpg", "c.png"]);
        assert_eq!(app_state.select_pattern, "*.jpg;*.png");

        app_state.open_select_prompt(false);
        app_state.prompt.as_mut().unwrap().1.set("?.png".to_string());
        app_state.confirm_prompt();
        assert_eq!(selected(&app_state), ["B.JPG", "a.jpg"]);

        // Files are inverted; the directory and .. are left alone.
        app_state.invert_selection(false);
        assert_eq!(selected(&app_state), ["c.png", "notes.txt"]);
        // Unless directories are asked for too.
        app_state.invert_selection(true);
        assert_eq!(selected(&app_state), ["B.JPG", "a.jpg", "photos"]);
        app_state.invert_selection(true);
        assert_eq!(selected(&app_state), ["c.png", "notes.txt"]);

        // * takes files only, */ the directories.
        app_state.select_by(&crate::glob::Glob::new("*").unwrap(), true);
        assert_eq!(selected(&app_state), ["B.JPG", "a.jpg", "c.png", "notes.txt"]);
        app_state.select_by(&crate::glob::Glob::new("*/").unwrap(), true);
        assert_eq!(selected(&app_state), ["B.JPG", "a.jpg", "c.png", "notes.txt", "photos"]);

        // An empty pattern does nothing and is not kept.
        app_state.open_select_prompt(false);
        app_state.prompt.as_mut().unwrap().1.set(String::new());
        app_state.confirm_prompt();
        assert_eq!(selected(&app_state).len(), 5);
        assert_eq!(app_state.select_pattern, "?.png");
    }

    #[test]
    fn a_selection_outlives_a_re_sort_but_not_its_file() {
        let mut selected: HashSet<std::ffi::OsString> =
            ["one.txt", "two.txt"].iter().map(|name| name.into()).collect();

        // Re-sorted, renumbered: both files are still there, so both stay
        // selected. This is what keying by name rather than row is for.
        prune_selection(&mut selected, &[row("zzz.txt"), row("two.txt"), row("one.txt")]);
        assert_eq!(selected.len(), 2);

        // one.txt is gone, so its selection goes with it.
        prune_selection(&mut selected, &[row("renamed.txt"), row("two.txt")]);
        assert_eq!(selected.iter().collect::<Vec<_>>(), ["two.txt"]);

        // And it does not come back when something else takes the name, which
        // would otherwise put a file the user never picked into the next delete.
        prune_selection(&mut selected, &[row("one.txt"), row("two.txt")]);
        assert_eq!(selected.iter().collect::<Vec<_>>(), ["two.txt"]);

        // An empty directory clears the lot.
        prune_selection(&mut selected, &[]);
        assert!(selected.is_empty());
    }

    /// Two names that are not valid UTF-8 can read the same once made
    /// printable. Selecting one must not select the other, or the next delete
    /// takes a file nobody picked.
    #[cfg(unix)]
    #[test]
    fn names_that_read_alike_are_selected_apart() {
        use std::os::unix::ffi::OsStrExt;

        let dir = std::env::temp_dir().join(format!("fm84-alike-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for raw in [&b"a\xfe"[..], &b"a\xff"[..]] {
            std::fs::write(dir.join(std::ffi::OsStr::from_bytes(raw)), "").unwrap();
        }

        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.open_dir(true, dir.clone(), None);
        let rows: Vec<Item> = app_state.children_left.iter().filter(|item| item.name != "..").cloned().collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name_full, rows[1].name_full);

        let index = app_state.children_left.iter().position(|row| row.name_os == rows[0].name_os).unwrap();
        app_state.state_left.select(Some(index));
        app_state.toggle_selection_no_size();
        assert_eq!(app_state.selected_left.iter().collect::<Vec<_>>(), [&rows[0].name_os]);

        std::fs::remove_dir_all(&dir).unwrap();
    }


    #[test]
    fn a_click_lands_on_the_icon_it_looks_like() {
        // Seven icons, each a glyph and the space after it, past the space the
        // strip opens with - the layout drive_strip actually produces.
        let slots: Vec<(u16, u16)> = (0..7).map(|i| (1 + i * 2, 2)).collect();

        assert_eq!(slot_at(&slots, 0), None); // the opening space
        assert_eq!(slot_at(&slots, 1), Some(0)); // first glyph
        assert_eq!(slot_at(&slots, 2), Some(0)); // the space that goes with it
        assert_eq!(slot_at(&slots, 3), Some(1));
        assert_eq!(slot_at(&slots, 13), Some(6)); // last icon
        assert_eq!(slot_at(&slots, 14), Some(6));
        assert_eq!(slot_at(&slots, 15), None); // past the end, where the label goes
        assert_eq!(slot_at(&slots, 200), None);

        // Every column of every slot maps back to that slot, and they never
        // overlap - a gap or an overlap here is a click landing one icon over.
        for (index, &(start, width)) in slots.iter().enumerate() {
            for offset in start..start + width {
                assert_eq!(slot_at(&slots, offset), Some(index), "offset {offset}");
            }
        }

        assert_eq!(slot_at(&[], 1), None);
    }

    #[test]
    fn a_drawing_smaller_than_the_pane_has_no_offset() {
        // The renderer centres it, so scrolling would only push it off-screen.
        // Zooming back in from below 100% used to land here at the far end.
        assert_eq!(recentre(0, 33, 7, 15), 0);
        assert_eq!(recentre(0, 33, 15, 22), 0);
        assert_eq!(recentre(0, 33, 30, 33), 0);
        // Nothing drawn yet.
        assert_eq!(recentre(0, 33, 0, 45), 0);
    }

    #[test]
    fn growing_past_the_pane_centres_what_was_centred() {
        // A 30-row drawing in a 33-row pane was centred, so 45 rows should be
        // centred too: (45 - 33) / 2.
        assert_eq!(recentre(0, 33, 30, 45), 6);
        assert_eq!(recentre(0, 33, 30, 60), 14);
    }

    #[test]
    fn an_overflowing_drawing_keeps_the_middle_of_the_pane() {
        // Centred at 45 rows, still centred at 60.
        assert_eq!(recentre(6, 33, 45, 60), 14);
        // It is the middle of the pane that is held, not the edges: the pane
        // covers less of the bigger drawing, so a view at the top or the bottom
        // moves inwards rather than staying pinned there.
        assert_eq!(recentre(0, 33, 45, 60), 6);
        assert_eq!(recentre(12, 33, 45, 60), 22);
    }

    #[test]
    fn the_offset_never_runs_past_the_end() {
        for before in [1usize, 7, 30, 45, 200] {
            for after in [1usize, 7, 30, 45, 200] {
                for offset in [0, 5, 100] {
                    let landed = recentre(offset, 33, before, after);
                    assert!(landed <= after.saturating_sub(33), "{before}->{after} from {offset}: {landed}");
                }
            }
        }
    }
}
