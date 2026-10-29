use super::{handle_key, request_quit};
use crate::app::{AppState, Dialog, FindFiles, PromptKind, Screen};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

mod archives;
mod dialogs;
mod editor;
mod file_ops;
mod mouse;
mod panel;

fn press(app_state: &mut AppState, code: KeyCode) -> bool {
    handle_key(app_state, KeyEvent::new(code, KeyModifiers::NONE))
}
