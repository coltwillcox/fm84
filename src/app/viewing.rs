//! F3: the viewer, and the pictures decoded ahead of it.

use super::*;

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
    pub(super) pending: Option<(PathBuf, Receiver<Decoded>)>,
    /// Which way the last step went, so the right side is read ahead.
    pub(super) forward: bool,
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
        self.ready.iter().any(|(held, _, _)| held == path) || self.pending.as_ref().is_some_and(|(wanted, _)| wanted == path)
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

/// Where a scroll offset belongs after the drawing changed size, so that the
/// middle of the pane goes on showing the same part of the picture. `extent` is
/// the pane's size along the axis being moved.
///
/// A drawing smaller than the pane is centred by the renderer rather than
/// scrolled, so it carries no offset of its own - which is both where such a
/// drawing starts from and where it lands.
pub(super) fn recentre(offset: usize, extent: usize, before: usize, after: usize) -> usize {
    if before == 0 || after <= extent {
        return 0;
    }
    // Where the middle of the pane falls on the old drawing, as a fraction of
    // it. One that fitted was centred, so its middle was the picture's own.
    let middle = if before <= extent { 0.5 } else { (offset as f64 + extent as f64 / 2.0) / before as f64 };
    let centre = middle * after as f64 - extent as f64 / 2.0;
    (centre.round().max(0.0) as usize).min(after - extent)
}

impl AppState {
    /// The file open in the viewer, if that is what is on screen.
    pub fn viewer(&self) -> Option<&ViewerState> {
        match &self.screen {
            Screen::Viewer(state) => Some(state),
            _ => None,
        }
    }

    pub fn viewer_mut(&mut self) -> Option<&mut ViewerState> {
        match &mut self.screen {
            Screen::Viewer(state) => Some(state),
            _ => None,
        }
    }

    pub fn is_viewing(&self) -> bool {
        matches!(self.screen, Screen::Viewer(_))
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
            self.dialog = Some(Dialog::LargeFile(LargeFile {
                path: file_path,
                is_edit,
                size,
                dimensions: None,
            }));
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
            self.dialog = Some(Dialog::LargeFile(LargeFile {
                path: file_path,
                is_edit,
                size: decoded,
                dimensions: Some(dimensions),
            }));
            return;
        }

        self.open_file(file_path, is_edit);
    }

    /// Answer to the large-file prompt: load it after all.
    pub fn confirm_large_file(&mut self) {
        if let Some(Dialog::LargeFile(large)) = self.dialog.take() {
            self.open_file(large.path, large.is_edit);
        }
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
        state.wrap = self.viewer_wrap;

        // Only once the new one is in hand: a load that failed used to leave
        // what was open alone, and should go on doing so. The picture being
        // replaced is already decoded, so keeping it costs nothing and makes
        // stepping back as quick as stepping on.
        if self.options.image_prefetch
            && let Screen::Viewer(previous) = std::mem::replace(&mut self.screen, Screen::Panels)
            && let Some(image) = previous.image
        {
            self.image_cache.keep(previous.file_path, image, previous.syntax_name);
        }

        self.screen = Screen::Viewer(state);
        self.read_ahead();
        Ok(())
    }

    /// Redraw an image as ASCII whenever the viewer size or the fit/fill choice
    /// asks for a different width than it was drawn at. Returns true if it did,
    /// so the caller can draw again.
    pub fn fit_viewer_image(&mut self) -> bool {
        let (width, height) = (self.viewer_viewport_width, self.viewer_viewport_height);
        let backgrounds = self.options.image_backgrounds;
        let Screen::Viewer(state) = &mut self.screen else {
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
        (state.image_lines, state.image_colors, state.image_backgrounds) = crate::viewer::image_to_ascii(image, columns, rows, backgrounds);
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

    /// Break the viewer's lines onto rows whenever it is wrapping and has not
    /// yet been, or the width has changed since; go back to a row a line when
    /// it has stopped - X to a hexdump, or W. True if it did either, so the
    /// caller can draw again.
    pub fn fit_viewer_wrap(&mut self) -> bool {
        let width = self.viewer_viewport_width;
        let Screen::Viewer(state) = &mut self.screen else {
            return false;
        };
        if state.wrapping() {
            if width > 0 && state.rows_width != width {
                state.rewrap(width);
                return true;
            }
        } else if !state.rows.is_empty() {
            state.unwrap_rows();
            return true;
        }
        false
    }

    /// W in the viewer: wrap long lines, or stop. Only for text; the next
    /// file opened wraps or not as this one was left.
    pub fn viewer_toggle_wrap(&mut self) {
        let width = self.viewer_viewport_width;
        let Screen::Viewer(state) = &mut self.screen else {
            return;
        };
        if state.mode != ViewMode::Text || state.from_edit {
            return;
        }
        state.wrap = !state.wrap;
        self.viewer_wrap = state.wrap;
        if state.wrap && width > 0 {
            state.rewrap(width);
        } else {
            state.unwrap_rows();
        }
    }

    /// Switch an image between fitting inside the viewer and filling it. The
    /// redraw itself happens in fit_viewer_image before the next frame.
    pub fn viewer_toggle_fill(&mut self) {
        if let Screen::Viewer(state) = &mut self.screen
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
        if let Screen::Viewer(state) = &mut self.screen
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
        self.screen = Screen::Panels;
        self.close_find();
        // Nothing is going to be stepped to now, and these are the only thing
        // in the app holding decoded pictures.
        self.image_cache.clear();
    }

    pub fn viewer_scroll_down(&mut self) {
        if let Screen::Viewer(state) = &mut self.screen {
            let max = state.total_lines.saturating_sub(self.viewer_viewport_height);
            state.scroll_offset = (state.scroll_offset + 1).min(max);
        }
    }

    pub fn viewer_scroll_up(&mut self) {
        if let Screen::Viewer(state) = &mut self.screen {
            state.scroll_offset = state.scroll_offset.saturating_sub(1);
        }
    }

    pub fn viewer_page_down(&mut self) {
        if let Screen::Viewer(state) = &mut self.screen {
            let max = state.total_lines.saturating_sub(self.viewer_viewport_height);
            state.scroll_offset = (state.scroll_offset + self.viewer_viewport_height).min(max);
        }
    }

    pub fn viewer_page_up(&mut self) {
        if let Screen::Viewer(state) = &mut self.screen {
            state.scroll_offset = state.scroll_offset.saturating_sub(self.viewer_viewport_height);
        }
    }

    pub fn viewer_home(&mut self) {
        if let Screen::Viewer(state) = &mut self.screen {
            state.scroll_offset = 0;
            state.horizontal_offset = 0;
        }
    }

    pub fn viewer_scroll_left(&mut self) {
        if let Screen::Viewer(state) = &mut self.screen {
            state.horizontal_offset = state.horizontal_offset.saturating_sub(1);
        }
    }

    pub fn viewer_copy(&mut self) {
        if let Some(text) = self.viewer().and_then(|state| state.selected_text()) {
            crate::utils::set_system_clipboard(&text);
            self.clipboard = text;
        }
    }

    /// Step the viewer to its next mode: text and a hexdump, and the picture for
    /// an image. Reads the raw bytes the first time they are needed, so a text
    /// file only pays for them on demand.
    pub fn viewer_next_mode(&mut self) {
        // Nothing to toggle on the refusal notice F4 puts up.
        if self.viewer().is_some_and(|state| state.from_edit) {
            return;
        }

        let mut error = None;

        if let Screen::Viewer(state) = &mut self.screen {
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
                    state.max_line_width = state.content_lines.iter().map(|line| crate::utils::line_display_width(line)).max().unwrap_or(0);
                }

                // Rows broken from the lines of one mode mean nothing in
                // another's; the next frame breaks them again if it wraps.
                state.unwrap_rows();
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
        let Some(showing) = self.viewer().map(|state| state.file_path.clone()) else {
            return;
        };
        let Some(going_to) = self.image_neighbour(&showing, forward) else {
            return;
        };

        // Carry across how it is being looked at, so a folder can be stepped
        // through at one zoom rather than starting over on every picture.
        let carried = self.viewer().map(|state| (state.image_fill, state.image_zoom));
        self.image_cache.forward = forward;
        self.request_open(going_to, false);
        if let Screen::Viewer(state) = &mut self.screen
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
        let (children, dir) = if self.is_left_active { (&self.children_left, &self.dir_left) } else { (&self.children_right, &self.dir_right) };
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
        let Some(showing) = self.viewer().map(|state| state.file_path.clone()) else {
            return;
        };
        if let Some(next) = self.image_neighbour(&showing, self.image_cache.forward) {
            self.image_cache.request(next);
        }
    }

    /// True while the viewer is showing a picture, where dragging moves the
    /// picture rather than selecting the characters it is drawn from.
    pub fn viewer_shows_image(&self) -> bool {
        self.viewer().is_some_and(|state| state.mode == ViewMode::Image && !state.from_edit)
    }

    /// Take hold of a picture at the pointer, ready to drag it about.
    pub fn viewer_pan_start(&mut self, column: u16, row: u16) {
        let content = self.viewer_content_area;
        if let Screen::Viewer(state) = &mut self.screen
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
        if let Screen::Viewer(state) = &mut self.screen
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
        if let Screen::Viewer(state) = &mut self.screen
            && !state.wrapping()
        {
            // Stop once the longest line's end reaches the right edge.
            let max = state.content_width().saturating_sub(self.viewer_viewport_width);
            state.horizontal_offset = (state.horizontal_offset + 1).min(max);
        }
    }

    /// Start the view at `offset`, from a scroll bar: a line down, or a
    /// column along. No further than the keys go.
    pub fn viewer_scroll_to(&mut self, vertical: bool, offset: usize) {
        let (height, width) = (self.viewer_viewport_height, self.viewer_viewport_width);
        if let Screen::Viewer(state) = &mut self.screen {
            if vertical {
                state.scroll_offset = offset.min(state.total_lines.saturating_sub(height));
            } else if !state.wrapping() {
                state.horizontal_offset = offset.min(state.content_width().saturating_sub(width));
            }
        }
    }

    pub fn viewer_end(&mut self) {
        if let Screen::Viewer(state) = &mut self.screen {
            state.scroll_offset = state.total_lines.saturating_sub(self.viewer_viewport_height);
        }
    }
}
