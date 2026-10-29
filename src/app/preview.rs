//! F12 and the detail lines: what is under the cursor, read in the background.

use super::*;

/// What the preview shows of an entry, read on the preview thread: the head
/// of a file, or how many entries a directory holds.
pub(super) fn read_preview(path: &Path, is_dir: bool) -> Preview {
    if path.symlink_metadata().is_err()
        && let Some(preview) = read_archive_preview(path)
    {
        return preview;
    }
    if !is_dir {
        return crate::viewer::load_preview(path, crate::constants::PREVIEW_MAX_BYTES, crate::constants::PREVIEW_MAX_LINES);
    }
    Preview::Lines(vec![match std::fs::read_dir(path) {
        Ok(entries) => format!("{} items", entries.count()),
        Err(e) => e.to_string(),
    }])
}

/// The preview of an entry inside an archive: the head of a file, read out
/// of it, or what a directory holds. None for a path not in one.
fn read_archive_preview(path: &Path) -> Option<Preview> {
    use crate::archive::{EntryKind, Next};
    let (index, at) = crate::archive::entry_at(path)?;
    let entry = index.entry(at);
    if entry.kind == EntryKind::Dir {
        return Some(Preview::Lines(vec![format!("{} items", index.children(&entry.path).count())]));
    }
    let (EntryKind::File, Some(position)) = (entry.kind, entry.position) else {
        return Some(Preview::Lines(vec!["Not a regular file".to_string()]));
    };
    let located = crate::archive::locate(path)?;
    let mut head = None;
    let _ = crate::archive::read_entries(&located.archive, index.kind, &[position], &mut |_, contents| {
        let mut buffer = Vec::new();
        if let Ok(reader) = contents
            && std::io::Read::read_to_end(&mut std::io::Read::take(reader, crate::constants::PREVIEW_MAX_BYTES), &mut buffer).is_ok()
        {
            head = Some(buffer);
        }
        Next::Go
    });
    Some(match head {
        Some(head) => crate::viewer::preview_of(head, crate::constants::PREVIEW_MAX_LINES),
        None => crate::viewer::unreadable_preview(),
    })
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
    /// Asked for and not yet in: only the name is known so far.
    pub(super) awaiting: bool,
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
    /// Asked for and not yet in: only the name is shown so far.
    pub(super) awaiting: bool,
}

impl AppState {
    /// Full path of the entry under the cursor, unless that is the parent entry.
    pub(super) fn cursor_target(&self) -> Option<(PathBuf, String, bool)> {
        let (children, state, dir) = if self.is_left_active {
            (&self.children_left, &self.state_left, &self.dir_left)
        } else {
            (&self.children_right, &self.state_right, &self.dir_right)
        };
        let item = children.get(state.selected()?)?;
        if item.is_parent() {
            return None;
        }
        Some((item.path_in(dir), item.name_full.clone(), item.is_dir))
    }

    /// Gather what the detail lines say, when the cursor has moved to something
    /// else. Asked of a thread, since it stats the file, reads a link and looks
    /// the owner up, and any of those can wait for good on a dead mount. The
    /// name is known already, and shows at once; the rest follows, at once on
    /// a disk that answers.
    pub fn refresh_cursor_detail(&mut self) {
        let target = self.cursor_target();
        let path = target.as_ref().map(|(path, _, _)| path.clone());
        let mut wait = Duration::ZERO;
        if self.cursor_detail.as_ref().is_none_or(|detail| detail.path != path) {
            let (name, awaiting) = match target {
                Some((path, name, is_dir)) => {
                    self.detail_worker.ask((path, is_dir));
                    wait = self.detail_wait;
                    (name, true)
                }
                None => (String::new(), false),
            };
            self.cursor_detail = Some(CursorDetail {
                path,
                name,
                size: String::new(),
                modified: String::new(),
                owner: String::new(),
                attributes: String::new(),
                link: None,
                awaiting,
            });
        }

        if !self.cursor_detail.as_ref().is_some_and(|detail| detail.awaiting) {
            return;
        }
        let Some(((path, _), described)) = self.detail_worker.answer(wait) else {
            return;
        };
        let date_format = self.options.date_format;
        let Some(detail) = self.cursor_detail.as_mut().filter(|detail| detail.path.as_ref() == Some(&path)) else {
            return;
        };
        detail.awaiting = false;
        // The exact count, since the column rounds it to something like
        // "9 MiB" and the difference is the point of showing it again.
        detail.size = described.as_ref().and_then(|described| described.size_bytes).map(|bytes| format!("{} bytes", crate::utils::grouped(bytes))).unwrap_or_default();
        // To the second, which the column has no room for either.
        detail.modified = described.as_ref().and_then(|described| described.modified).map(|at| crate::utils::format_exact(at, date_format)).unwrap_or_default();
        detail.owner = described.as_ref().map(|described| described.owner.clone()).unwrap_or_default();
        detail.attributes = described.as_ref().map(|described| described.attributes.clone()).unwrap_or_default();
        detail.link = described.and_then(|described| described.link);
    }

    /// Keep the preview pointed at whatever the cursor is on. Reads only when
    /// the target actually changed, so holding an arrow key stays cheap - and
    /// on a thread, like the detail lines, for the same reason.
    pub fn refresh_preview(&mut self) {
        if !self.show_preview {
            self.preview = None;
            return;
        }

        let target = self.cursor_target();
        let path = target.as_ref().map(|(path, _, _)| path.clone());
        let mut wait = Duration::ZERO;
        if self.preview.as_ref().is_none_or(|preview| preview.path != path) {
            let (label, awaiting) = match target {
                Some((path, name, is_dir)) => {
                    self.preview_worker.ask((path, is_dir));
                    wait = self.detail_wait;
                    (name, true)
                }
                None => (String::new(), false),
            };
            self.preview = Some(PreviewState {
                path,
                label,
                lines: Vec::new(),
                bytes: Vec::new(),
                awaiting,
            });
        }

        if !self.preview.as_ref().is_some_and(|preview| preview.awaiting) {
            return;
        }
        let Some(((path, _), content)) = self.preview_worker.answer(wait) else {
            return;
        };
        let Some(preview) = self.preview.as_mut().filter(|preview| preview.path.as_ref() == Some(&path)) else {
            return;
        };
        preview.awaiting = false;
        match content {
            Preview::Lines(lines) => preview.lines = lines,
            Preview::Bytes(bytes) => preview.bytes = bytes,
        }
    }
}
