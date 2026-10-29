//! Alt+F7: find files by name and content.

use super::*;

/// Find files, from the form to the results.
pub enum FindFiles {
    /// What to look for: a name pattern, and text the file has to hold.
    /// `on_containing` says which field the keys go to.
    Asking { name: TextInput, containing: TextInput, on_containing: bool },
    /// The search, running or over, and what it has found.
    Found(Search),
}

/// A Find files walk on a thread of its own, and what it has sent so far.
/// Dropped - the dialog closed - it is told to stop.
pub struct Search {
    /// Where it started, which the results are shown from.
    pub start: PathBuf,
    /// What was looked for, for the title.
    pub label: String,
    /// Each match, and whether it is a directory, in the order found.
    pub results: Vec<(PathBuf, bool)>,
    pub cursor: usize,
    /// The first result showing, kept so the list scrolls only when the
    /// cursor reaches an edge of it.
    pub scroll: usize,
    /// The directory being looked through, while the walk goes on.
    pub looking_in: Option<PathBuf>,
    pub done: bool,
    /// Directories that could not be read, counted rather than stopping it.
    pub unreadable: usize,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) updates: Receiver<crate::search::Update>,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl AppState {
    /// Alt+F7: ask what to look for, offering the last search again.
    pub fn open_find_files(&mut self) {
        if self.panel_archive(self.is_left_active).is_some() {
            self.display_error("Find files cannot search inside an archive yet".to_string());
            return;
        }
        let (mut name, mut containing) = (TextInput::new(), TextInput::new());
        name.set(self.find_files_last.0.clone());
        containing.set(self.find_files_last.1.clone());
        self.dialog = Some(Dialog::FindFiles(Box::new(FindFiles::Asking { name, containing, on_containing: false })));
    }

    /// Tab in the Find files form: the other field.
    pub fn find_files_switch_field(&mut self) {
        if let Some(Dialog::FindFiles(find)) = &mut self.dialog
            && let FindFiles::Asking { on_containing, .. } = find.as_mut()
        {
            *on_containing = !*on_containing;
        }
    }

    /// Enter on the Find files form: start the walk, down from the active
    /// panel's directory, on a thread of its own. An empty name looks at
    /// every file, and empty text asks for none.
    pub fn start_find_files(&mut self) {
        let Some(Dialog::FindFiles(find)) = &self.dialog else {
            return;
        };
        let FindFiles::Asking { name, containing, .. } = find.as_ref() else {
            return;
        };
        let (name, containing) = (name.text.clone(), containing.text.clone());
        self.find_files_last = (name.clone(), containing.clone());

        let pattern = if name.trim().is_empty() { "*" } else { name.as_str() };
        let label = match containing.is_empty() {
            true => pattern.to_string(),
            false => format!("{pattern} containing \"{containing}\""),
        };
        let start = self.panel_dir(self.is_left_active);
        let query = crate::search::Query {
            start: start.clone(),
            name: crate::glob::Glob::new(pattern).unwrap_or_else(|| crate::glob::Glob::new("*").expect("* is a pattern")),
            containing: crate::find::Needle::new(&containing),
            show_hidden: self.options.show_hidden,
            case_sensitive: self.options.case_sensitive,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, updates) = mpsc::channel();
        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            crate::search::search(&query, &worker_cancel, &mut |update| sender.send(update).is_ok());
        });
        self.dialog = Some(Dialog::FindFiles(Box::new(FindFiles::Found(Search {
            looking_in: Some(start.clone()),
            start,
            label,
            results: Vec::new(),
            cursor: 0,
            scroll: 0,
            done: false,
            unreadable: 0,
            cancel,
            updates,
        }))));
    }

    /// Take in what the Find files walk has sent since the last frame.
    pub fn poll_find_files(&mut self) {
        let Some(Dialog::FindFiles(find)) = &mut self.dialog else {
            return;
        };
        let FindFiles::Found(search) = find.as_mut() else {
            return;
        };
        loop {
            match search.updates.try_recv() {
                Ok(crate::search::Update::Looking(dir)) => search.looking_in = Some(dir),
                Ok(crate::search::Update::Found(path, is_dir)) => search.results.push((path, is_dir)),
                Ok(crate::search::Update::Finished { unreadable }) => {
                    search.unreadable = unreadable;
                    search.looking_in = None;
                    search.done = true;
                }
                Err(TryRecvError::Empty) => break,
                // Gone without a word, which only happens if it panicked.
                Err(TryRecvError::Disconnected) => {
                    search.looking_in = None;
                    search.done = true;
                    break;
                }
            }
        }
    }

    /// Move the Find files cursor `by` results, stopping at either end, and
    /// scroll the list only as far as it takes to keep it showing.
    pub fn find_files_move(&mut self, by: isize) {
        let page = self.find_files_page.max(1);
        if let Some(Dialog::FindFiles(find)) = &mut self.dialog
            && let FindFiles::Found(search) = find.as_mut()
        {
            let last = search.results.len().saturating_sub(1);
            search.cursor = search.cursor.saturating_add_signed(by).min(last);
            if search.cursor < search.scroll {
                search.scroll = search.cursor;
            } else if search.cursor >= search.scroll + page {
                search.scroll = search.cursor + 1 - page;
            }
        }
    }

    /// Enter on a result: the active panel goes to the directory it is in,
    /// with the cursor on it, and the search is put away.
    pub fn find_files_go(&mut self) {
        let Some(Dialog::FindFiles(find)) = &self.dialog else {
            return;
        };
        let FindFiles::Found(search) = find.as_ref() else {
            return;
        };
        let Some((path, _)) = search.results.get(search.cursor) else {
            return;
        };
        let (Some(dir), Some(name)) = (path.parent().map(Path::to_path_buf), path.file_name().map(OsString::from)) else {
            return;
        };
        self.dialog = None;
        self.open_dir(self.is_left_active, dir, Some(&name));
    }
}
