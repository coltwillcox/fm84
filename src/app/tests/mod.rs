use super::{AppState, Item, prune_selection, recentre, slot_at};
use crate::options::Options;
use std::collections::HashSet;

mod jobs;
mod panels;
mod selection;
mod settings;
mod viewing;

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
