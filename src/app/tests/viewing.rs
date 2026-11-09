//! Where a picture sits in the viewer's pane as it is zoomed.

use super::*;

#[test]
fn a_drawing_smaller_than_the_pane_has_no_offset() {
    // The renderer centres it, so scrolling would only push it off-screen.
    // Zooming back in from below 100% used to land here at the far end.
    assert_eq!(recentre(0, 33, 7, 15), 0);
    assert_eq!(recentre(0, 33, 15, 22), 0);
    assert_eq!(recentre(0, 33, 30, 33), 0);
    // Nothing drawn yet.
    assert_eq!(recentre(0, 33, 0, 45), 0);
}

#[test]
fn growing_past_the_pane_centres_what_was_centred() {
    // A 30-row drawing in a 33-row pane was centred, so 45 rows should be
    // centred too: (45 - 33) / 2.
    assert_eq!(recentre(0, 33, 30, 45), 6);
    assert_eq!(recentre(0, 33, 30, 60), 14);
}

#[test]
fn an_overflowing_drawing_keeps_the_middle_of_the_pane() {
    // Centred at 45 rows, still centred at 60.
    assert_eq!(recentre(6, 33, 45, 60), 14);
    // It is the middle of the pane that is held, not the edges: the pane
    // covers less of the bigger drawing, so a view at the top or the bottom
    // moves inwards rather than staying pinned there.
    assert_eq!(recentre(0, 33, 45, 60), 6);
    assert_eq!(recentre(12, 33, 45, 60), 22);
}

#[test]
fn the_offset_never_runs_past_the_end() {
    for before in [1usize, 7, 30, 45, 200] {
        for after in [1usize, 7, 30, 45, 200] {
            for offset in [0, 5, 100] {
                let landed = recentre(offset, 33, before, after);
                assert!(landed <= after.saturating_sub(33), "{before}->{after} from {offset}: {landed}");
            }
        }
    }
}

/// F on a log: what is written to it afterwards comes onto the end, the
/// last line carried on and a character cut in half waited for, and the
/// view stays at the end. Cut short, it is read again; looked back up, it
/// is no longer followed.
#[test]
fn a_followed_file_grows_in_the_viewer() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!("fm84-follow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("app.log");
    std::fs::write(&log, "one\ntw").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.drawn.viewer_viewport_height = 2;
    app_state.open_viewer(log.clone()).unwrap();
    app_state.viewer_toggle_follow();
    let poll = |app_state: &mut AppState| {
        app_state.viewer_mut().unwrap().follow.as_mut().unwrap().checked = None;
        app_state.poll_follow();
    };
    let lines = |app_state: &AppState| app_state.viewer().unwrap().content_lines.clone();

    // "é" is two bytes; the first comes alone.
    let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    file.write_all(b"o\r\nthree\ncaf\xc3").unwrap();
    poll(&mut app_state);
    assert_eq!(lines(&app_state), ["one", "two", "three", "caf"]);
    file.write_all(b"\xa9\n").unwrap();
    poll(&mut app_state);
    assert_eq!(lines(&app_state), ["one", "two", "three", "café", ""]);
    let state = app_state.viewer().unwrap();
    assert_eq!((state.total_lines, state.scroll_offset), (5, 3), "at the end");

    // Turned over, as a log is: read from the start.
    std::fs::write(&log, "fresh\n").unwrap();
    poll(&mut app_state);
    assert_eq!(lines(&app_state), ["fresh", ""]);
    assert!(app_state.viewer().unwrap().follow.is_some());

    app_state.viewer_scroll_up();
    assert!(app_state.viewer().unwrap().follow.is_none());
    std::fs::write(&log, "fresh\nmore\n").unwrap();
    app_state.poll_follow();
    assert_eq!(lines(&app_state), ["fresh", ""], "no longer followed");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A log turned over - moved away, and a new one written in its place -
/// is followed from its start, however long it is by then.
#[cfg(unix)]
#[test]
fn a_followed_log_turned_over_is_read_from_its_start() {
    let dir = std::env::temp_dir().join(format!("fm84-follow-turn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("app.log");
    std::fs::write(&log, "old\n").unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_viewer(log.clone()).unwrap();
    app_state.viewer_toggle_follow();
    std::fs::rename(&log, dir.join("app.log.1")).unwrap();
    std::fs::write(&log, "a new log, longer than the old one\n").unwrap();
    app_state.poll_follow();
    assert_eq!(app_state.viewer().unwrap().content_lines, ["a new log, longer than the old one", ""]);
    std::fs::remove_dir_all(&dir).unwrap();
}
