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
