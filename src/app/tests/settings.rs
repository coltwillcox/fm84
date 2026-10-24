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
