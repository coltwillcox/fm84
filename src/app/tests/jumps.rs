//! Alt+J: a directory gone to before, by a few letters of it.

use super::*;
use crate::app::Dialog;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Go to `dir` and wait for the panel to land there.
fn go(app_state: &mut AppState, dir: &std::path::Path) {
    app_state.open_dir(Side::Left, dir.to_path_buf(), None);
    let started = Instant::now();
    while app_state.panel_busy(Side::Left) {
        assert!(started.elapsed() < Duration::from_secs(10), "the listing never came");
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_listings();
    }
}

fn shown(app_state: &AppState) -> Vec<PathBuf> {
    match &app_state.dialog {
        Some(Dialog::History(list)) => list.shown.clone(),
        _ => panic!("no jump list"),
    }
}

#[test]
fn alt_j_ranks_where_the_panels_go_and_jumps_by_a_few_letters() {
    let root = std::env::temp_dir().join(format!("fm84-jumps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (music, programs, fm84) = (root.join("music"), root.join("programs"), root.join("programs").join("fm84"));
    std::fs::create_dir_all(&music).unwrap();
    std::fs::create_dir_all(&fm84).unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    go(&mut app_state, &root);
    // fm84 the most gone to; music once, and last.
    for _ in 0..3 {
        go(&mut app_state, &fm84);
        go(&mut app_state, &programs);
    }
    go(&mut app_state, &fm84);
    go(&mut app_state, &music);

    // Where the panel is now is no jump.
    app_state.open_jumps();
    let all = shown(&app_state);
    assert_eq!(all[..2], [fm84.clone(), programs], "{all:?}");
    assert!(!all.contains(&music));
    assert!(all.contains(&root));

    // A few letters, in order, the last in the last name.
    let input = app_state.dialog_input().unwrap();
    "pro fm".chars().for_each(|c| input.insert(c));
    app_state.history_typed();
    assert_eq!(shown(&app_state), std::slice::from_ref(&fm84));
    app_state.go_to_history();
    let started = Instant::now();
    while app_state.panel_busy(Side::Left) {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_listings();
    }
    assert_eq!(app_state.left.dir, fm84);

    // One gone since is said to be, and forgotten.
    std::fs::remove_dir_all(&music).unwrap();
    app_state.open_jumps();
    let input = app_state.dialog_input().unwrap();
    "music".chars().for_each(|c| input.insert(c));
    app_state.history_typed();
    assert_eq!(shown(&app_state), std::slice::from_ref(&music));
    app_state.go_to_history();
    let started = Instant::now();
    while app_state.panel_busy(Side::Left) {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_listings();
    }
    assert!(app_state.error.take().is_some());
    assert_eq!(app_state.left.dir, fm84, "the panel stays put");
    assert!(!app_state.jumps.ranked(crate::options::now_seconds()).contains(&music));
    std::fs::remove_dir_all(&root).unwrap();
}
