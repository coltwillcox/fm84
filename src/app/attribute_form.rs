//! Alt+A: permissions, owner, group and modified time.

use super::*;

/// Alt+A's form. Each field opens on what every entry has in common, or
/// empty where they differ, and one left as it opened - or emptied - is left
/// alone on every entry.
pub struct AttributesForm {
    pub items: Vec<(PathBuf, bool)>,
    /// Each entry's mode as it opened, for what a symbolic mode makes of it.
    pub modes: Vec<u32>,
    pub fields: Vec<AttributeField>,
    pub focus: usize,
}

pub struct AttributeField {
    pub kind: AttributeKind,
    pub input: TextInput,
    pub opened: String,
}

impl AttributeField {
    /// What was typed, when it is something to change to.
    fn changed(&self) -> Option<&str> {
        let text = self.input.text.trim();
        (!text.is_empty() && text != self.opened.trim()).then_some(text)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttributeKind {
    Mode,
    Owner,
    Group,
    Modified,
}

impl AttributeKind {
    pub fn label(self) -> &'static str {
        match self {
            AttributeKind::Mode => "Mode",
            AttributeKind::Owner => "Owner",
            AttributeKind::Group => "Group",
            AttributeKind::Modified => "Modified",
        }
    }
}

impl AppState {
    /// Alt+A on what is chosen. Only the modified time where there are no
    /// Unix permissions and owners to change.
    pub fn open_attributes(&mut self, items: Vec<(PathBuf, bool)>) {
        let mut metadata = Vec::with_capacity(items.len());
        for (path, _) in &items {
            match std::fs::metadata(path) {
                Ok(found) => metadata.push(found),
                Err(e) => {
                    self.display_error(format!("Cannot read {}: {}", path.display(), e));
                    return;
                }
            }
        }
        // What they all have, or nothing where they differ.
        let common = |value: &dyn Fn(&std::fs::Metadata) -> String| {
            let first = metadata.first().map(value).unwrap_or_default();
            if metadata.iter().all(|found| value(found) == first) { first } else { String::new() }
        };
        let modified = |found: &std::fs::Metadata| found.modified().map(crate::attributes::format_time).unwrap_or_default();

        let mut values = Vec::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            values.push((AttributeKind::Mode, common(&|found| format!("{:o}", found.permissions().mode() & 0o7777))));
            values.push((AttributeKind::Owner, common(&|found| crate::fs_ops::user_name(found.uid()))));
            values.push((AttributeKind::Group, common(&|found| crate::fs_ops::group_name(found.gid()))));
        }
        values.push((AttributeKind::Modified, common(&modified)));

        #[cfg(unix)]
        let modes = {
            use std::os::unix::fs::PermissionsExt;
            metadata.iter().map(|found| found.permissions().mode()).collect()
        };
        #[cfg(not(unix))]
        let modes = Vec::new();

        let fields = values
            .into_iter()
            .map(|(kind, opened)| {
                let mut input = TextInput::new();
                input.set(opened.clone());
                AttributeField { kind, input, opened }
            })
            .collect();
        self.dialog = Some(Dialog::Attributes(Box::new(AttributesForm { items, modes, fields, focus: 0 })));
    }

    /// Tab and Shift+Tab, Down and Up: the next field, or the one before,
    /// round from the end.
    pub fn attributes_switch_field(&mut self, forward: bool) {
        if let Some(Dialog::Attributes(form)) = &mut self.dialog {
            let count = form.fields.len();
            form.focus = if forward { (form.focus + 1) % count } else { (form.focus + count - 1) % count };
        }
    }

    /// A click on one of the fields: the keys go to it, and when what it
    /// holds fits, the cursor goes where the click was.
    pub fn attributes_click(&mut self, column: u16, row: u16) {
        let Some(index) = self.attribute_rows.iter().position(|(cell, _)| cell.contains(Position::new(column, row))) else {
            return;
        };
        let (cell, text_x) = self.attribute_rows[index];
        if let Some(Dialog::Attributes(form)) = &mut self.dialog
            && let Some(field) = form.fields.get_mut(index)
        {
            form.focus = index;
            let room = cell.right().saturating_sub(text_x) as usize;
            if crate::utils::display_width(&field.input.text) < room {
                field.input.cursor_to_column(column.saturating_sub(text_x) as usize);
            }
        }
    }

    /// Enter: make the changes asked for. Anything typed that cannot be read
    /// is said so before a thing is touched, and the form stays to fix it.
    /// An entry that refuses is passed over, and the rest still change.
    pub fn apply_attributes(&mut self) {
        let Some(Dialog::Attributes(form)) = &self.dialog else {
            return;
        };
        let mut change = crate::attributes::Change::default();
        for field in &form.fields {
            let Some(text) = field.changed() else {
                continue;
            };
            let read = match field.kind {
                AttributeKind::Mode => crate::attributes::ModeChange::parse(text).map(|mode| change.mode = Some(mode)),
                #[cfg(unix)]
                AttributeKind::Owner => crate::fs_ops::user_id(text).map(|uid| change.owner = Some(uid)).ok_or_else(|| format!("No such user: {text}")),
                #[cfg(unix)]
                AttributeKind::Group => crate::fs_ops::group_id(text).map(|gid| change.group = Some(gid)).ok_or_else(|| format!("No such group: {text}")),
                #[cfg(not(unix))]
                AttributeKind::Owner | AttributeKind::Group => Ok(()),
                AttributeKind::Modified => crate::attributes::parse_time(text).map(|time| change.modified = Some(time)),
            };
            if let Err(e) = read {
                self.display_error(e);
                return;
            }
        }
        let Some(Dialog::Attributes(form)) = self.dialog.take() else {
            return;
        };
        if change.is_empty() {
            return;
        }

        let mut failed = Vec::new();
        for (path, is_dir) in &form.items {
            if let Err(e) = change.apply(path, *is_dir) {
                failed.push(format!("{}: {}", path.display(), e));
            }
        }
        self.reload_panel(true, None);
        self.reload_panel(false, None);
        match failed.len() {
            0 => {}
            1 if form.items.len() == 1 => self.display_error(format!("Cannot change {}", failed[0])),
            count => self.display_error(format!("Could not change {count} of {}. {}", form.items.len(), failed[0])),
        }
    }
}
