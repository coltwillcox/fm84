//! Quick search, selecting entries, and the sizes of directories counted
//! in the background.

use super::*;

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
    pub(super) cancel: Arc<AtomicBool>,
    updates: Receiver<SizeUpdate>,
}

enum SizeUpdate {
    Found(u64),
    /// None when cancelled.
    Finished(Result<Option<u64>, String>),
}

impl AppState {
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
        if self.is_left_active { (&self.children_left, &mut self.state_left) } else { (&self.children_right, &mut self.state_right) }
    }

    /// + and - in a panel: ask for a pattern, starting from the last one.
    pub fn open_select_prompt(&mut self, select: bool) {
        let mut input = TextInput::new();
        input.set(self.select_pattern.clone());
        self.dialog = Some(Dialog::Prompt(if select { PromptKind::Select } else { PromptKind::Deselect }, input));
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
        for item in children.iter().filter(|item| !item.is_parent() && glob.matches(&item.name_full, item.is_dir)) {
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
        for item in children.iter().filter(|item| (with_dirs || !item.is_dir) && !item.is_parent()) {
            if !selected.remove(&item.name_os) {
                selected.insert(item.name_os.clone());
            }
        }
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
                if index < children.len() && !children[index].is_parent() {
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

    /// Work out a directory's size - next, ahead of any waiting, since it was
    /// asked for on its own - unless that is already under way.
    pub fn start_dir_size(&mut self, path: PathBuf) {
        if self.dir_sizing.iter().any(|sizing| sizing.path == path) {
            return;
        }
        self.dir_size_queue.retain(|queued| *queued != path);
        self.dir_size_queue.push_front(path);
        self.start_queued_sizes();
    }

    /// Alt+Shift+Enter, as in Total Commander: the size of every directory in
    /// the active panel, counted again if it was before. Links to directories
    /// are left out: one to / or to a slow mount would walk all of it. They
    /// go in turn, top to bottom, a few at once, and each shows in the Size
    /// column as it is counted.
    pub fn size_all_dirs(&mut self) {
        let (children, dir) = if self.is_left_active { (&self.children_left, &self.dir_left) } else { (&self.children_right, &self.dir_right) };
        let paths: Vec<PathBuf> = children.iter().filter(|item| item.is_dir && !item.is_symlink && !item.is_parent()).map(|item| item.path_in(dir)).collect();
        for path in paths {
            if !self.dir_sizing.iter().any(|sizing| sizing.path == path) && !self.dir_size_queue.contains(&path) {
                self.dir_size_queue.push_back(path);
            }
        }
        self.start_queued_sizes();
    }

    /// Start what is waiting, as far as there are workers free.
    fn start_queued_sizes(&mut self) {
        while self.dir_sizing.len() < crate::constants::SIZE_WORKERS
            && let Some(path) = self.dir_size_queue.pop_front()
        {
            self.spawn_dir_size(path);
        }
    }

    /// Work out a directory's size on a thread of its own.
    fn spawn_dir_size(&mut self, path: PathBuf) {
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
        self.dir_sizing.retain_mut(|sizing| {
            loop {
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
        // Workers freed, for what is waiting.
        self.start_queued_sizes();
    }

    /// Stop every size still being worked out. Each thread notices between
    /// entries; one stuck in a read on a dead mount is simply forgotten, and
    /// whatever it finds is never asked for.
    pub fn cancel_dir_sizes(&mut self) {
        self.dir_size_queue.clear();
        for sizing in self.dir_sizing.drain(..) {
            sizing.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// A panel has left `dir`: stop sizing what is in it, running or waiting,
    /// which nothing shows any more - unless the other panel does. Left to
    /// run, Alt+Shift+Enter in / and a step elsewhere walked the whole disk
    /// out of sight, with Esc in a panel the only way to stop it.
    pub(super) fn forget_sizes_in(&mut self, dir: &Path) {
        if self.dir_left == dir || self.dir_right == dir {
            return;
        }
        let inside = |path: &Path| path.parent() == Some(dir);
        self.dir_size_queue.retain(|path| !inside(path));
        self.dir_sizing.retain(|sizing| {
            let stop = inside(&sizing.path);
            if stop {
                sizing.cancel.store(true, Ordering::Relaxed);
            }
            !stop
        });
        self.start_queued_sizes();
    }

    /// The running total for a directory still being sized - nothing yet,
    /// for one waiting its turn.
    pub fn dir_size_so_far(&self, path: &Path) -> Option<u64> {
        let waiting = || self.dir_size_queue.iter().any(|queued| queued == path).then_some(0);
        self.dir_sizing.iter().find(|sizing| sizing.path == path).map(|sizing| sizing.found).or_else(waiting)
    }

    pub fn clear_active_selections(&mut self) {
        if self.is_left_active {
            self.selected_left.clear();
        } else {
            self.selected_right.clear();
        }
    }
}
