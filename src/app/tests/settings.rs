//! F11 moving through its rows.

use super::*;

#[test]
fn the_options_highlight_stops_at_either_end() {
    let mut app_state = AppState::new();
    app_state.options_move(false);
    assert_eq!(app_state.options_cursor, 0, "Up on the first row stays there");
    let last = crate::options::OPTION_ROWS.len() - 1;
    for _ in 0..=last {
        app_state.options_move(true);
    }
    assert_eq!(app_state.options_cursor, last, "Down on the last row stays there");
}

/// PageUp and PageDown move F11's highlight by what it had room for, Home
/// and End to either end, none of them past the first or the last.
#[test]
fn the_options_page_and_go_to_either_end() {
    let mut app_state = AppState::new();
    let last = crate::options::OPTION_ROWS.len() - 1;
    app_state.drawn.options_room = 10;
    app_state.options_page(true);
    assert_eq!(app_state.options_cursor, 9, "a page less the row kept in sight");
    app_state.options_page(true);
    assert_eq!(app_state.options_cursor, 18);
    app_state.options_page(false);
    assert_eq!(app_state.options_cursor, 9);
    app_state.options_page(false);
    app_state.options_page(false);
    assert_eq!(app_state.options_cursor, 0, "no further than the first");
    for _ in 0..=last {
        app_state.options_page(true);
    }
    assert_eq!(app_state.options_cursor, last, "nor the last");
    app_state.options_move_by(isize::MIN);
    assert_eq!(app_state.options_cursor, 0);
    app_state.options_move_by(isize::MAX);
    assert_eq!(app_state.options_cursor, last);
    // Not drawn yet, nothing known of the room: a row at a time.
    app_state.drawn.options_room = 0;
    app_state.options_page(false);
    assert_eq!(app_state.options_cursor, last - 1);
}
