//! What fm84 holds and does, apart from drawing it and reading keys: the
//! panels, the dialogs, the viewer and editor, and the jobs running behind them.

use crate::background::{Latest, Watch, WatchEvent};
use crate::fs_ops::{Clash, Description, Mount, Progress, Reply, Step, Transfer, copy_path, count_entries, delete_path, disk_usage, get_current_dir, load_directory_rows, measure, move_path, nearest_existing_dir, path_exists, rename_in_place};
use crate::options::{OPTION_ROWS, OptionRow, Options, SortKey};
use crate::strip::{Strip, StripHit};
use crate::viewer::Preview;
use crate::viewer::{ViewMode, ViewerState};
use image::DynamicImage;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::TableState;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

mod archives;
mod attribute_form;
mod bookmarks;
mod branches;
mod compare;
mod editing;
mod filter;
mod find;
mod find_files;
mod flat;
mod git_marks;
mod history;
mod jobs;
mod listing;
mod multi_rename;
mod open_with;
mod preview;
mod selection;
mod settings;
mod sync;
mod tabs;
#[cfg(test)]
mod tests;
mod text_input;
mod user_menu;
mod viewing;

pub use self::{
    archives::*, attribute_form::*, branches::*, editing::*, filter::*, find_files::*, flat::*, git_marks::*, history::*, jobs::*, listing::*, multi_rename::*, open_with::*, preview::*, selection::*, sync::*, tabs::*, text_input::*, user_menu::*,
    viewing::*,
};

/// What the prompt in the status bar is asking for: in the viewer or the
/// editor, something to find or a line to go to; in a panel, a pattern to
/// select or deselect by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromptKind {
    Find,
    GoToLine,
    Select,
    Deselect,
    /// What to replace, in the editor.
    Replace,
    /// What to replace it with.
    ReplaceWith,
    /// Ctrl+L: a path for the panel to go to, typed over the panel's own in
    /// the line above it rather than at the foot of the screen.
    GoTo,
    /// Ctrl+S: a pattern to narrow the active panel to, as it is typed.
    Filter,
}

/// What fills the space between the top bar and the bottom one: the two
/// panels, or a file open in the viewer or the editor. One at a time, by
/// construction - the file being looked at lives here and nowhere else.
pub enum Screen {
    Panels,
    Viewer(ViewerState),
    Editor(EditorState),
    /// Shift+F3: two files side by side.
    Diff(crate::diff::DiffState),
}

/// The one dialog open over the screen, if any, carrying what it is asking
/// about. Only one can be open at a time, and each answers its own keys, so
/// nothing behind it can act on a key meant for it.
pub enum Dialog {
    /// F1.
    Help,
    /// F11. `editing` holds a text option while it is being typed into.
    Options { editing: Option<TextInput> },
    /// F2, typed into in place of the name in the panel.
    Rename(TextInput),
    /// F7 makes a directory, Shift+F4 an empty file.
    Create { is_dir: bool, input: TextInput },
    /// F8: what to delete, as paths - the name alone could not be joined
    /// back to the file it came from when it is not valid UTF-8 - and whether
    /// it goes to the trash or for good.
    Delete { items: Vec<(PathBuf, bool)>, to_trash: bool },
    /// F5 or F6, waiting to be confirmed, with where to put what is chosen
    /// typed into `input`. It opens on the other panel's directory - with the
    /// file's name on the end, when it is a single file - `default` as the
    /// filesystem holds it, and `opened_with` as it was written into
    /// the field: a path that is not valid UTF-8 can only be shown near
    /// enough, so while the field is as it opened, `default` is what is used.
    Transfer {
        sources: Vec<(PathBuf, bool)>,
        is_copy: bool,
        input: TextInput,
        opened_with: String,
        default: PathBuf,
    },
    /// Ctrl+Shift+F5, waiting to be confirmed: a symbolic link to each of
    /// `sources`, made where `input` says - or a hard link, which Tab
    /// switches to. Opened and read as Transfer is, except that a lone
    /// directory is named as a file is - the link is the new entry, not
    /// something to go inside it.
    Symlink {
        sources: Vec<(PathBuf, bool)>,
        input: TextInput,
        opened_with: String,
        default: PathBuf,
        hard: bool,
    },
    /// Alt+F9: where to extract the whole of an archive to.
    Extract { archive: PathBuf, input: TextInput },
    /// Alt+P: the archive to pack what is chosen into, its format by the
    /// ending of its name.
    Pack { sources: Vec<(PathBuf, bool)>, input: TextInput },
    /// Alt+H: checksums worked out, or a checksum file checked.
    Checksums(Box<ChecksumView>),
    /// A file big enough to be worth asking about before it is opened.
    LargeFile(LargeFile),
    /// Unsaved edits, on the way out of the editor. `then_quit` when it was
    /// F10 that asked, so that answering Save or Discard also leaves.
    SaveChanges { then_quit: bool },
    /// Something else has changed the file in the editor since it was opened,
    /// and saving would write over that: asked about before it does. `then` is
    /// what the save was on its way to, done once it is answered with a save.
    DiskChanged { then: AfterSave, opened: Option<DiskStamp>, now: Option<DiskStamp> },
    /// Alt+F1 or Alt+F2: which panel is choosing, and the mount highlighted.
    DrivePicker { is_left: bool, index: usize },
    /// The line at the foot of the screen, asking for something to find, a
    /// line to go to or a pattern to select by - or, for Ctrl+L, the panel's
    /// path, edited where it is shown.
    Prompt(PromptKind, TextInput),
    /// Alt+F7: what to look for, and then what was found.
    FindFiles(Box<FindFiles>),
    /// Ctrl+D: the directories kept to go back to, and the one highlighted.
    Bookmarks { list: Vec<PathBuf>, index: usize },
    /// Alt+Down: the directories visited, narrowed by what is typed.
    History(Box<HistoryList>),
    /// Alt+M: the user's own commands.
    UserMenu(Box<UserMenu>),
    /// F2 with a selection: every name selected changed by one pattern.
    MultiRename(Box<MultiRename>),
    /// Alt+S: the panels' directories compared all the way down, and the
    /// plan of what to copy which way and what to delete.
    Sync(Box<SyncPlan>),
    /// Alt+O: the programs the entry under the cursor can be opened with.
    OpenWith(Box<OpenWith>),
    /// A click on a panel's git branch: the repository's branches, to change
    /// to another.
    Branches(Box<BranchList>),
    /// Alt+A: the permissions, owner, group and modified time of what is
    /// chosen, to change.
    Attributes(Box<AttributesForm>),
    /// Replacing in the editor, a match at a time: the one selected is the
    /// one asked about. `stop_at` is where it started, which it stops at once
    /// it has gone round the end of the file and back; `wrapped` says it has.
    Replacing { stop_at: (usize, usize), wrapped: bool, replaced: usize },
}

impl Dialog {
    /// True for the dialogs drawn as a popup over the screen, which nothing
    /// behind may answer for - not even the mouse. The rename field sits in
    /// the panel, where a click cancels it; the prompt at the foot and the
    /// drive strip leave the panels to be clicked.
    fn is_popup(&self) -> bool {
        !matches!(self, Dialog::Rename(_) | Dialog::Prompt(..) | Dialog::DrivePicker { .. })
    }
}

pub struct AppState {
    pub screen: Screen,
    pub dialog: Option<Dialog>,
    /// An error to show, over the screen and over any dialog.
    pub error: Option<String>,
    /// What F11 sets, as loaded from the config file at startup.
    pub options: Options,
    /// The row the options popup has highlighted.
    pub options_cursor: usize,
    /// How far down F1's list it is scrolled.
    pub help_scroll: usize,
    /// A file F4 is handing to an external editor. The main loop runs it,
    /// since only that can give the terminal up and take it back.
    pub external_edit: Option<PathBuf>,
    /// A command from Alt+M's menu, for the main loop to hand the terminal to.
    pub user_command: Option<UserCommand>,
    /// F12: the opposite panel shows what the cursor is on.
    pub show_preview: bool,
    pub preview: Option<PreviewState>,
    /// Editor clipboard. Internal, so it works in a bare TTY too.
    pub clipboard: String,
    /// Mounts offered in the drive strips, kept current by a watcher thread.
    pub mounts: Vec<Mount>,
    pub is_left_active: bool,
    /// What each panel is showing, everything that goes with it to the other
    /// side when Ctrl+U swaps them.
    pub left: Panel,
    pub right: Panel,
    pub drawn: Drawn,
    /// The filter there was when Ctrl+S's prompt opened, for Esc to put back.
    pub filter_before: String,
    pub search_input: String,
    pub cached_separator_height: u16,
    pub cached_separator: String,
    pub viewer_viewport_height: usize,
    pub viewer_viewport_width: usize,
    pub editor_viewport_height: usize,
    /// Rows the diff shows, and the width of each side's text.
    pub diff_viewport_height: usize,
    pub diff_viewport_width: usize,
    /// What + or - last selected by, offered again the next time.
    pub select_pattern: String,
    /// What Find files last looked for - the name pattern and the text -
    /// offered again the next time.
    pub find_files_last: FindForm,
    /// How many results the Find files list showed when it was last drawn,
    /// which is how far PageUp and PageDown go.
    pub find_files_page: usize,
    /// What Ctrl+R last replaced with, offered again the next time.
    pub replace_with: String,
    /// Whether W last left the viewer wrapping lines, which the next file
    /// opened takes up.
    pub viewer_wrap: bool,
    /// What Ctrl+F last looked for. Kept when the prompt closes, for F3 and
    /// Shift+F3, and offered again the next time it opens.
    pub find_term: String,
    /// Alt+R in the find prompt: the term is a regular expression.
    pub find_regex: bool,
    /// Whether the matches of find_term are marked on screen. On from the
    /// first search until the viewer or editor closes.
    pub find_shown: bool,
    /// What the last search came to - "Not found", or that it went round the
    /// end - or what Ctrl+C copied in a panel, shown in the status bar until
    /// the next key.
    pub find_note: Option<String>,
    pub dir_sizes: HashMap<PathBuf, u64>,
    /// Every directory either panel has been in this run, newest first, for
    /// Alt+Down.
    pub visited: Vec<PathBuf>,
    pub last_click_time: Option<Instant>,
    pub last_click_pos: (u16, u16),
    // Where the last frame actually drew things. Mouse handling reads these
    // instead of recomputing the layout from hardcoded row numbers.
    pub table_area_left: Rect,
    pub table_area_right: Rect,
    /// The row of the panels' paths, a click on either of which edits it as
    /// Ctrl+L does.
    pub path_bar_area: Rect,
    /// Alt+A's fields as last drawn: each one's row, and the column its text
    /// starts at, for a click to land in.
    pub attribute_rows: Vec<(Rect, u16)>,
    /// The F-key labels along the bottom, as last drawn, and their keys.
    pub fkey_hits: Vec<(Rect, u8)>,
    /// Each panel's git branch on the path bar, left then right, as last
    /// drawn - a click there lists the repository's branches.
    pub branch_hits: [Option<Rect>; 2],
    /// The rows of the branch list as last drawn, and the branch on each.
    pub branch_rows: Vec<(Rect, usize)>,
    /// A branch git is still switching to.
    pub branch_switch: Option<PendingSwitch>,
    /// The column headings a click sorts by, both panels', as last drawn.
    pub sort_headers: Vec<(Rect, SortKey)>,
    pub editor_content_area: Rect,
    pub viewer_content_area: Rect,
    /// The viewer's or the editor's scroll bars, as last drawn.
    pub scroll_bars: Vec<crate::scroll_bar::ScrollBar>,
    /// The bar whose thumb the left button took hold of, as it was then, and
    /// how many cells from the thumb's start it was taken - followed until
    /// the button comes up.
    pub bar_drag: Option<(crate::scroll_bar::ScrollBar, usize)>,
    // The drive strips as drawn, and which of the drives each one shows.
    pub drive_strip_left: Strip,
    pub drive_strip_right: Strip,
    /// How long the UI waits on a listing before going on without it.
    /// Without limit under test, which wants each answer in hand before it
    /// looks; the same goes for `detail_wait`.
    pub listing_wait: Duration,
    /// How long the UI waits on the detail lines and the preview.
    pub detail_wait: Duration,
    // Watching the mounts for drives coming and going. Dropping it stops it.
    _watch_mounts: Watch,
    /// Handed to each watcher as it starts; what they report arrives on
    /// `watch_events`.
    watch_sender: Sender<WatchEvent>,
    watch_events: Receiver<WatchEvent>,
    /// How often a watcher looks. Shortened under test, so a change is seen
    /// inside it.
    pub watch_interval: Duration,
    /// Where the detail lines are gathered.
    detail_worker: Latest<(PathBuf, bool), Option<Description>>,
    /// Where the preview is gathered.
    preview_worker: Latest<(PathBuf, bool), Preview>,
    /// `user@host`, for the top border, and whether that is root.
    pub user_at_host: (String, bool),
    /// Set while a copy, move or delete is running on its own thread.
    pub job: Option<TransferJob>,
    /// Directories Space is working out the size of, each on its own thread.
    pub dir_sizing: Vec<DirSizing>,
    /// Directories waiting for a sizing worker to come free, next first.
    pub dir_size_queue: std::collections::VecDeque<PathBuf>,
    /// Pictures read around the one on screen, while F11 leaves it on.
    pub image_cache: ImageCache,
    /// The entry under the cursor, spelled out under the panels.
    pub cursor_detail: Option<CursorDetail>,
    /// F10 was pressed during a job. The job carries on; a second press is
    /// what leaves. Cleared when the job ends, so it only ever covers one.
    pub quit_armed: bool,
    /// The quit was Shift+F10: leave without telling `cd84` where, so the
    /// shell stays where it was. Set by every F10, so the last one decides.
    pub stay_put: bool,
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
    /// A symbolic link, listed as what it points to - a link to a directory
    /// is a directory here, and is_dir says so.
    pub is_symlink: bool,
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

    /// The last part of the name as shown: the name itself, or under Ctrl+B,
    /// the file's own without the directories it is in.
    pub fn leaf(&self) -> &str {
        self.name_full.rsplit(std::path::is_separator).next().unwrap_or(&self.name_full)
    }

    /// The `..` row the listing puts first. Told by the name the filesystem
    /// holds, never by `name`: that is a file's stem, and the stem of a file
    /// called `...` is `..`.
    pub fn is_parent(&self) -> bool {
        self.name_os == ".."
    }
}

/// One panel: the directory it is in and what it shows of it.
pub struct Panel {
    pub dir: PathBuf,
    pub state: TableState,
    pub children: Vec<Item>,
    /// Ctrl+S: what the panel is narrowed to, if anything.
    pub filter: Option<PanelFilter>,
    /// Ctrl+B: the directory the panel lists every file under, if any.
    pub flat: Option<Flat>,
    /// What git says of the panel's entries.
    pub git: PanelGit,
    // Keyed by file name, not row index: a reload can re-sort the rows, and an
    // index would then point at a different file than the one the user picked.
    // The name the filesystem holds, not the one shown: two names that are not
    // valid UTF-8 can read the same, and picking one would pick both.
    pub selected: HashSet<OsString>,
    pub viewport_start: usize,
    // The panel's tabs, and its tab strip as drawn.
    pub tabs: Tabs,
    // The panel's directory still being read, past the moment the UI waits for
    // it. The panel goes on showing what it did until it lands.
    pub listing: Option<PendingListing>,
    // Watching the directory for changes and its filesystem for free space.
    // Dropping it stops it.
    watch: Option<Watch>,
    // Changed on disk since it was read, to be read again once nothing is
    // open over it.
    stale: bool,
    // (used, total) bytes for the panel's filesystem.
    pub disk: Option<(u64, u64)>,
    /// The git branch the panel's directory is on, shown after its path.
    pub branch: Option<String>,
    /// The archive the panel's directory runs through, when it is inside
    /// one, as its listing found it. Read-only there: everything that would
    /// write asks this first, and never the disk.
    pub archive: Option<PathBuf>,
}

/// What the last frame drew, where, and how much room it had: written while
/// drawing, read by the keys and the mouse, which would otherwise have to
/// work the layout out again.
#[derive(Default)]
pub struct Drawn {
    /// How many lines F1's list had room for - what the keys scroll it by,
    /// and up to.
    pub help_room: usize,
    /// Rows a panel had room for - how far PageUp and PageDown go.
    pub page_size: u16,
    /// The clock in the top border, as last drawn.
    pub cached_clock: String,
}

impl AppState {
    /// The left panel, or the right.
    pub fn panel(&self, is_left: bool) -> &Panel {
        if is_left { &self.left } else { &self.right }
    }

    pub fn panel_mut(&mut self, is_left: bool) -> &mut Panel {
        if is_left { &mut self.left } else { &mut self.right }
    }

    pub fn new() -> Self {
        let mut state_left = TableState::default();
        state_left.select(Some(1));
        let mut state_right = TableState::default();
        state_right.select(Some(1));

        let (watch_sender, watch_events) = mpsc::channel();
        let (error, dir_root) = match get_current_dir() {
            Ok(root) => (None, root),
            Err(e) => (Some(e.to_string()), PathBuf::new()),
        };

        Self {
            screen: Screen::Panels,
            dialog: None,
            error,
            // A test starts from the defaults, never from whatever the config
            // of the machine running it says.
            options: if cfg!(test) { Options::default() } else { Options::load() },
            options_cursor: 0,
            help_scroll: 0,
            external_edit: None,
            user_command: None,
            show_preview: false,
            preview: None,
            clipboard: String::new(),
            mounts: Vec::new(),
            is_left_active: true,
            left: Panel {
                dir: dir_root.clone(),
                state: state_left,
                children: Vec::new(),
                filter: None,
                flat: None,
                git: PanelGit::default(),
                selected: HashSet::new(),
                viewport_start: 0,
                tabs: Tabs::new(vec![dir_root.clone()], 0),
                listing: None,
                watch: None,
                stale: false,
                disk: None,
                branch: None,
                archive: None,
            },
            right: Panel {
                dir: dir_root.clone(),
                state: state_right,
                children: Vec::new(),
                filter: None,
                flat: None,
                git: PanelGit::default(),
                selected: HashSet::new(),
                viewport_start: 0,
                tabs: Tabs::new(vec![dir_root], 0),
                listing: None,
                watch: None,
                stale: false,
                disk: None,
                branch: None,
                archive: None,
            },
            drawn: Drawn::default(),
            filter_before: String::new(),
            search_input: String::new(),
            cached_separator_height: 0,
            cached_separator: String::new(),
            viewer_viewport_height: 0,
            viewer_viewport_width: 0,
            editor_viewport_height: 0,
            diff_viewport_height: 0,
            diff_viewport_width: 0,
            select_pattern: "*".to_string(),
            find_files_last: FindForm::default(),
            find_files_page: 10,
            find_term: String::new(),
            find_regex: false,
            replace_with: String::new(),
            viewer_wrap: false,
            find_shown: false,
            find_note: None,
            dir_sizes: HashMap::new(),
            visited: Vec::new(),
            last_click_time: None,
            last_click_pos: (0, 0),
            table_area_left: Rect::default(),
            table_area_right: Rect::default(),
            path_bar_area: Rect::default(),
            attribute_rows: Vec::new(),
            fkey_hits: Vec::new(),
            branch_hits: [None, None],
            branch_rows: Vec::new(),
            branch_switch: None,
            sort_headers: Vec::new(),
            editor_content_area: Rect::default(),
            viewer_content_area: Rect::default(),
            scroll_bars: Vec::new(),
            bar_drag: None,
            listing_wait: if cfg!(test) { Duration::MAX } else { crate::constants::LISTING_WAIT },
            detail_wait: if cfg!(test) { Duration::MAX } else { crate::constants::DETAIL_WAIT },
            _watch_mounts: crate::background::watch_mounts(crate::constants::REFRESH_INTERVAL, watch_sender.clone()),
            watch_sender,
            watch_events,
            watch_interval: crate::constants::REFRESH_INTERVAL,
            detail_worker: Latest::new(|(path, is_dir): &(PathBuf, bool)| crate::fs_ops::describe(path, *is_dir)),
            preview_worker: Latest::new(|(path, is_dir): &(PathBuf, bool)| read_preview(path, *is_dir)),
            user_at_host: crate::fs_ops::user_at_host(),
            job: None,
            dir_sizing: Vec::new(),
            dir_size_queue: std::collections::VecDeque::new(),
            image_cache: ImageCache::default(),
            cursor_detail: None,
            quit_armed: false,
            stay_put: false,
            drive_strip_left: Strip::default(),
            drive_strip_right: Strip::default(),
        }
    }

    pub fn display_error(&mut self, message: String) {
        self.error = Some(message);
    }

    pub fn reset_error(&mut self) {
        self.error = None;
    }

    /// The text field of the dialog that is open, when it has one being
    /// typed into: a name, a pattern, something to find, a text option.
    pub fn dialog_input(&mut self) -> Option<&mut TextInput> {
        match self.dialog.as_mut()? {
            Dialog::Rename(input) | Dialog::Create { input, .. } | Dialog::Transfer { input, .. } | Dialog::Symlink { input, .. } | Dialog::Extract { input, .. } | Dialog::Pack { input, .. } | Dialog::Prompt(_, input) => Some(input),
            Dialog::Options { editing } => editing.as_mut(),
            Dialog::History(list) => Some(&mut list.input),
            Dialog::Attributes(form) => form.fields.get_mut(form.focus).map(|field| &mut field.input),
            Dialog::MultiRename(rename) => rename.fields.get_mut(rename.focus),
            Dialog::FindFiles(find) => match find.as_mut() {
                FindFiles::Asking(form) => form.fields.get_mut(form.focus),
                FindFiles::Found(_) => None,
            },
            _ => None,
        }
    }

    /// True while a popup is covering the screen. What is behind one must sit
    /// still: it cannot be seen, and the popup is answering for it.
    pub fn popup_is_open(&self) -> bool {
        self.error.is_some() || self.job.is_some() || self.dialog.as_ref().is_some_and(Dialog::is_popup)
    }

    /// True while a dialog, viewer or editor owns the screen. Added to the
    /// popups: the viewer and the editor, whole-screen modes that take their
    /// own mouse input, and the rename field, which sits in the panel, where a
    /// click cancels it rather than being swallowed.
    pub fn is_modal_open(&self) -> bool {
        self.popup_is_open() || !matches!(self.screen, Screen::Panels) || matches!(self.dialog, Some(Dialog::Rename(_)))
    }
}
