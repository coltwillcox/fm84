//! The viewer's time machine: Alt+Left and Alt+Right step the file back and
//! on through the commits that changed it, a commit at a time, and back to
//! the file as it is now.

use super::*;
use crate::viewer::History;

impl AppState {
    /// Alt+Left (`back`) or Alt+Right in the viewer: the file as the commit
    /// before, or after, the one shown had it - the newest first, from the
    /// file as it is, and the file as it is again after the newest. Where
    /// the view is stays where it was, as far as the other version goes, and
    /// the matches found stay marked.
    pub fn viewer_step_history(&mut self, back: bool) {
        let Some(state) = self.viewer() else { return };
        if state.from_edit {
            return;
        }
        let path = state.file_path.clone();
        let commits = match &state.history {
            Some(_) => None,
            None => match crate::git::file_history(&path) {
                Ok(commits) if !commits.is_empty() => Some(commits),
                Ok(_) => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    return self.display_error(t!("No commits of {name} to step through", name));
                }
                Err(e) => return self.display_error(e),
            },
        };
        let Some(state) = self.viewer_mut() else { return };
        let history = state.history.get_or_insert_with(|| Box::new(History { commits: commits.unwrap_or_default(), at: None }));
        let count = history.commits.len();
        let going_to = match (history.at, back) {
            (None, true) => Some(0),
            (Some(at), true) if at + 1 < count => Some(at + 1),
            (Some(at), false) if at > 0 => Some(at - 1),
            (Some(_), false) => None,
            // Nothing older than the first, nothing newer than the file.
            _ => return,
        };
        let dir = path.parent().unwrap_or(Path::new("."));
        // Asked about before it is read, as the file is when it is opened: a
        // version from long ago may be far bigger than the one opened.
        if let Some(index) = going_to {
            let limit = self.options.large_file_bytes();
            let Some(state) = self.viewer() else { return };
            let commit = &state.history.as_ref().expect("made above").commits[index];
            match crate::git::file_size_at(dir, commit) {
                Ok(size) if size > limit => return self.display_error(t!("Too large to view as that commit had it: {size}; F11 sets the limit", size = crate::utils::format_size(size))),
                Ok(_) => {}
                Err(e) => return self.display_error(e),
            }
        }
        let Some(state) = self.viewer() else { return };
        let history = state.history.as_ref().expect("made above");
        let bytes = match going_to {
            Some(index) => crate::git::file_at(dir, &history.commits[index]),
            None => std::fs::read(&path).map_err(|e| e.to_string()),
        };
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(e) => return self.display_error(e),
        };
        let Some(state) = self.viewer_mut() else { return };
        // A hexdump stays one, of text as of anything else.
        let raw = (state.mode == ViewMode::Hex).then(|| bytes.clone());
        // A picture is decoded only to be drawn, and only within what
        // opening one is allowed without asking: an old version can unpack
        // to far more than the one opened did. Past that it is a hexdump.
        let pictures = raw.is_none() && crate::viewer::image_cost_of(&bytes).is_none_or(|cost| cost <= crate::constants::IMAGE_MAX_DECODED);
        let mut fresh = crate::viewer::content_as(&path, bytes, None, pictures);
        if let Some(raw) = raw
            && fresh.mode == ViewMode::Text
        {
            (fresh.bytes, fresh.mode) = (raw, ViewMode::Hex);
            fresh.total_lines = fresh.line_count();
        }
        fresh.history = state.history.take().map(|history| Box::new(History { at: going_to, ..*history }));
        (fresh.wrap, fresh.image_fill, fresh.image_zoom) = (state.wrap, state.image_fill, state.image_zoom);
        if fresh.mode == state.mode {
            // The line at the top, which wrapping breaks onto rows again.
            fresh.scroll_offset = state.row(state.scroll_offset).0.min(fresh.line_count().saturating_sub(1));
            fresh.horizontal_offset = state.horizontal_offset;
        }
        *state = fresh;
    }

    /// The bytes of the file the viewer shows: as the commit shown had it,
    /// or as it is on the disk.
    pub(super) fn viewer_bytes(state: &ViewerState) -> Result<Vec<u8>, String> {
        match state.history.as_deref() {
            Some(History { commits, at: Some(at) }) => crate::git::file_at(state.file_path.parent().unwrap_or(Path::new(".")), &commits[*at]),
            _ => std::fs::read(&state.file_path).map_err(|e| e.to_string()),
        }
    }
}

/// Whether the viewer shows the file as a commit had it, rather than as it is.
pub fn in_the_past(state: &ViewerState) -> bool {
    state.history.as_ref().is_some_and(|history| history.at.is_some())
}
