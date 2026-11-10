//! Alt+B and Alt+Shift+B: mixtapes gathered from several directories, listed
//! by name and shown in a panel with all of it chosen.

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
    assert!(matches!(&app_state.dialog, Some(crate::app::Dialog::Mixtapes(_))), "the list, with nothing on it");
    app_state.dialog = None;

    let (a, b, kept) = (dir.join("one/a.txt"), dir.join("two/deeper/b.txt"), dir.join("two/deeper/kept"));
    app_state.open_dir(Side::Left, dir.join("one"), None);
    app_state.basket_toggle(vec![(a.clone(), false)]);
    app_state.open_dir(Side::Left, dir.join("two/deeper"), None);
    // Twice is once.
    app_state.basket_toggle(vec![(b.clone(), false), (kept.clone(), true), (b.clone(), false)]);
    // With no tape yet, Alt+B starts one.
    assert_eq!(app_state.find_note.as_deref(), Some("Mixtape: 3"));

    app_state.open_basket();
    app_state.show_mixtape();
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
    app_state.show_mixtape();
    assert_eq!(app_state.tapes.tapes[0].items, vec![(renamed.clone(), false)]);
    assert_eq!(shown(&app_state).0, vec![renamed]);
    assert_eq!(app_state.left.dir, dir.join("one"));

    // Again, and the panel is a directory as it was.
    app_state.open_basket();
    assert!(!app_state.showing_basket());
    assert!(app_state.left.selected.is_empty());
    assert!(app_state.left.children.iter().any(|item| item.name_os == "c.txt"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The list of tapes: a new one named plays, and Alt+B adds to that one
/// only; a name another has is refused; Space plays one without showing
/// it; and one removed takes no file with it.
#[test]
fn tapes_are_named_played_and_removed() {
    let dir = std::env::temp_dir().join(format!("fm84-mixtapes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), None);
    let name = |app_state: &mut AppState, renaming: bool, text: &str| {
        app_state.name_mixtape(renaming);
        if let Some(crate::app::Dialog::Mixtapes(list)) = &mut app_state.dialog {
            list.naming.as_mut().unwrap().input.set(text.to_string());
        }
        app_state.commit_mixtape_name();
    };
    let names = |app_state: &AppState| app_state.tapes.tapes.iter().map(|tape| tape.name.clone()).collect::<Vec<_>>();

    app_state.open_basket();
    name(&mut app_state, false, " Release files ");
    name(&mut app_state, false, "Tax 2026");
    assert_eq!(names(&app_state), ["Release files", "Tax 2026"]);
    assert_eq!(app_state.tapes.playing, 1, "a new tape plays");
    app_state.dialog = None;
    app_state.basket_toggle(vec![(a.clone(), false)]);
    assert_eq!(app_state.find_note.as_deref(), Some("Tax 2026: 1"));

    // Space plays the first; Alt+B adds to it, and the other keeps its own.
    app_state.open_basket();
    app_state.move_mixtape_cursor(-1);
    app_state.play_mixtape();
    app_state.dialog = None;
    app_state.basket_toggle(vec![(b.clone(), false)]);
    assert_eq!(app_state.tapes.tapes[0].items, vec![(b.clone(), false)]);
    assert_eq!(app_state.tapes.tapes[1].items, vec![(a.clone(), false)]);

    // A name that is taken, refused, and the field left up to change it.
    app_state.open_basket();
    name(&mut app_state, true, "Tax 2026");
    assert!(app_state.error.take().is_some());
    assert!(matches!(&app_state.dialog, Some(crate::app::Dialog::Mixtapes(list)) if list.naming.is_some()));
    app_state.cancel_mixtape_name();
    name(&mut app_state, true, "Out");
    assert_eq!(names(&app_state), ["Out", "Tax 2026"]);

    // Removed, the tape goes and its files stay; the one playing still
    // plays, wherever it now is.
    app_state.move_mixtape_cursor(1);
    app_state.play_mixtape();
    app_state.move_mixtape_cursor(-1);
    app_state.remove_mixtape();
    assert_eq!(names(&app_state), ["Tax 2026"]);
    assert_eq!(app_state.tapes.playing, 0);
    assert!(b.exists());

    // Enter shows it, and plays it.
    app_state.show_mixtape();
    assert!(app_state.showing_basket());
    assert_eq!(shown(&app_state).1, vec![a.clone()]);
    std::fs::remove_dir_all(&dir).unwrap();
}
