//! Alt+B and Alt+Shift+B: a basket gathered from several directories, shown
//! in a panel with all of it chosen.

use super::*;
use std::path::PathBuf;

/// What the panel shows of it, by the paths each entry stands for, and
/// which are chosen.
fn shown(app_state: &AppState) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let panel = &app_state.left;
    let mut all: Vec<PathBuf> = panel.children.iter().filter(|item| !item.is_parent()).map(|item| item.path_in(&panel.dir)).collect();
    let mut chosen: Vec<PathBuf> = panel.children.iter().filter(|item| panel.selected.contains(&item.name_os)).map(|item| item.path_in(&panel.dir)).collect();
    all.sort();
    chosen.sort();
    (all, chosen)
}

#[test]
fn a_basket_from_several_directories_is_shown_all_chosen() {
    let dir = std::env::temp_dir().join(format!("fm84-basket-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("one")).unwrap();
    std::fs::create_dir_all(dir.join("two/deeper/kept")).unwrap();
    std::fs::write(dir.join("one/a.txt"), "a").unwrap();
    std::fs::write(dir.join("two/deeper/b.txt"), "b").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_basket();
    assert_eq!(app_state.find_note.as_deref(), Some("The basket is empty"));

    let (a, b, kept) = (dir.join("one/a.txt"), dir.join("two/deeper/b.txt"), dir.join("two/deeper/kept"));
    app_state.open_dir(Side::Left, dir.join("one"), None);
    app_state.basket_toggle(vec![(a.clone(), false)]);
    app_state.open_dir(Side::Left, dir.join("two/deeper"), None);
    // Twice is once.
    app_state.basket_toggle(vec![(b.clone(), false), (kept.clone(), true), (b.clone(), false)]);
    assert_eq!(app_state.find_note.as_deref(), Some("Basket: 3"));

    app_state.open_basket();
    assert!(app_state.showing_basket());
    assert_eq!(app_state.left.dir, dir);
    let everything = vec![a.clone(), b.clone(), kept.clone()];
    let mut sorted = everything.clone();
    sorted.sort();
    assert_eq!(shown(&app_state), (sorted.clone(), sorted));

    // Alt+B there takes out of the basket - and never deletes.
    app_state.basket_toggle(vec![(kept.clone(), true)]);
    assert!(kept.is_dir());
    let mut left = vec![a.clone(), b.clone()];
    left.sort();
    assert_eq!(shown(&app_state), (left.clone(), left));
    assert_eq!(app_state.left.selected.len(), 2);

    // A file renamed is followed, and one gone is let go of.
    let renamed = dir.join("one/c.txt");
    std::fs::rename(&a, &renamed).unwrap();
    app_state.feed_follow(&[(a.clone(), renamed.clone())]);
    std::fs::remove_file(&b).unwrap();
    app_state.open_basket();
    app_state.open_basket();
    assert_eq!(app_state.basket, vec![(renamed.clone(), false)]);
    assert_eq!(shown(&app_state).0, vec![renamed]);
    assert_eq!(app_state.left.dir, dir.join("one"));

    // Again, and the panel is a directory as it was.
    app_state.open_basket();
    assert!(!app_state.showing_basket());
    assert!(app_state.left.selected.is_empty());
    assert!(app_state.left.children.iter().any(|item| item.name_os == "c.txt"));
    std::fs::remove_dir_all(&dir).unwrap();
}
