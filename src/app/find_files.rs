//! Alt+F7: find files by name, content, size and age - and the results fed
//! to a panel, to work on together.

use super::*;

/// Find files, from the form to the results.
pub enum FindFiles {
    /// What to look for.
    Asking(FindForm),
    /// The search, running or over, and what it has found.
    Found(Search),
}

/// The form's fields' labels, in the order they are shown and Tab goes
/// through them.
pub fn find_fields() -> [&'static str; 4] {
    [t!("Name"), t!("Containing"), t!("Size"), t!("Modified")]
}
/// The row after the fields, where Space turns regular expressions on.
pub const FIND_REGEX_ROW: usize = 4;

/// What to look for: a name pattern, text the file has to hold, its size
/// and when it was modified - and whether the text is a regular expression.
#[derive(Clone)]
pub struct FindForm {
    pub fields: [TextInput; 4],
    /// Which row the keys go to: a field, or the regular expression switch.
    pub focus: usize,
    pub regex: bool,
}

impl FindForm {
    fn text(&self, field: usize) -> &str {
        &self.fields[field].text
    }
}

impl Default for FindForm {
    fn default() -> Self {
        let mut name = TextInput::new();
        name.set("*".to_string());
        FindForm {
            fields: [name, TextInput::new(), TextInput::new(), TextInput::new()],
            focus: 0,
            regex: false,
        }
    }
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
        if self.panel_archive(self.active_side).is_some() {
            self.display_error(t!("Find files cannot search inside an archive yet").to_string());
            return;
        }
        let mut form = self.recall.find_files_last.clone();
        form.focus = 0;
        self.dialog = Some(Dialog::FindFiles(Box::new(FindFiles::Asking(form))));
    }

    /// Tab and Down in the Find files form: the next row, or with `forward`
    /// false the one before, the regular expression switch among them.
    pub fn find_files_switch_field(&mut self, forward: bool) {
        if let Some(Dialog::FindFiles(find)) = &mut self.dialog
            && let FindFiles::Asking(form) = find.as_mut()
        {
            let rows = FIND_REGEX_ROW + 1;
            form.focus = if forward { (form.focus + 1) % rows } else { (form.focus + rows - 1) % rows };
        }
    }

    /// Space on the regular expression row.
    pub fn find_files_toggle_regex(&mut self) {
        if let Some(Dialog::FindFiles(find)) = &mut self.dialog
            && let FindFiles::Asking(form) = find.as_mut()
        {
            form.regex = !form.regex;
        }
    }

    /// Enter on the Find files form: start the walk, down from the active
    /// panel's directory, on a thread of its own. An empty name looks at
    /// every file, and an empty field asks for nothing. One that cannot be
    /// read - a size, a time, an expression - is said so, and the form
    /// stays to put it right.
    pub fn start_find_files(&mut self) {
        let Some(Dialog::FindFiles(find)) = &self.dialog else {
            return;
        };
        let FindFiles::Asking(form) = find.as_ref() else {
            return;
        };
        let form = form.clone();
        self.recall.find_files_last = form.clone();
        let (name, containing, size, modified) = (form.text(0), form.text(1), form.text(2), form.text(3));
        let parsed = (|| -> Result<_, String> { Ok((crate::search::Text::new(containing, form.regex)?, crate::search::SizeRange::parse(size)?, crate::search::TimeRange::parse(modified, chrono::Local::now())?)) })();
        let (text, size_range, time_range) = match parsed {
            Ok(parsed) => parsed,
            Err(message) => {
                self.display_error(message);
                return;
            }
        };

        let pattern = if name.trim().is_empty() { "*" } else { name };
        let mut label = pattern.to_string();
        if !containing.is_empty() {
            label.push_str(&if form.regex { format!(" matching /{containing}/") } else { format!(" containing \"{containing}\"") });
        }
        if !size.trim().is_empty() {
            label.push_str(&format!(", size {}", size.trim()));
        }
        if !modified.trim().is_empty() {
            label.push_str(&format!(", modified {}", modified.trim()));
        }
        let start = self.panel_dir(self.active_side);
        let query = crate::search::Query {
            start: start.clone(),
            name: crate::glob::Glob::new(pattern).unwrap_or_else(|| crate::glob::Glob::new("*").expect("* is a pattern")),
            containing: text,
            size: size_range,
            modified: time_range,
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
        let page = self.drawn.find_files_page.max(1);
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

    /// F on the results: what has been found so far, in the active panel as
    /// one list under the directory the search started in - each by its
    /// path from there, as Ctrl+B lists - to be copied, moved, deleted or
    /// renamed together. A search still going is stopped. The list is the
    /// panel's until it goes elsewhere, or Ctrl+B goes back; a reread drops
    /// what is gone since.
    pub fn feed_find_files(&mut self) {
        let Some(Dialog::FindFiles(find)) = &self.dialog else {
            return;
        };
        let FindFiles::Found(search) = find.as_ref() else {
            return;
        };
        let start = search.start.clone();
        let feed: Vec<(PathBuf, bool)> = search.results.iter().filter_map(|(path, is_dir)| path.strip_prefix(&start).ok().map(|relative| (relative.to_path_buf(), *is_dir))).collect();
        if feed.is_empty() {
            self.display_error(t!("Nothing found to feed to the panel").to_string());
            return;
        }
        let first = feed[0].0.clone().into_os_string();
        self.dialog = None;
        let side = self.active_side;
        self.panel_mut(side).flat = Some(Flat {
            dir: start.clone(),
            feed: Some(Arc::new(feed)),
            basket: false,
        });
        if self.panel_dir(side) == start {
            self.reload_panel(side, Some(&first));
        } else {
            self.open_dir(side, start, Some(&first));
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
        self.open_dir(self.active_side, dir, Some(&name));
    }
}
