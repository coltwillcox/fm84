//! Selecting, by hand, by pattern and by comparing, and sizing directories.

use super::*;

/// Space sizes a directory on a thread, so the panel goes on answering
/// while it counts, and the size lands once the walk is done.
#[test]
fn space_sizes_a_directory_off_the_ui_thread() {
    let dir = std::env::temp_dir().join(format!("fm84-sizing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("data.bin"), vec![0u8; 4096]).unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(true, dir.clone(), Some("sub".as_ref()));
    app_state.toggle_selection();
    assert_eq!(app_state.dir_sizing.len(), 1);

    let started = std::time::Instant::now();
    while !app_state.dir_sizing.is_empty() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "sizing never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_dir_sizes();
    }
    assert_eq!(app_state.dir_sizes.get(&dir.join("sub")), Some(&4096));

    // Esc stops one under way, and nothing it finds is kept.
    app_state.dir_sizes.clear();
    app_state.start_dir_size(dir.join("sub"));
    app_state.cancel_dir_sizes();
    assert!(app_state.dir_sizing.is_empty());
    app_state.poll_dir_sizes();
    assert!(app_state.dir_sizes.is_empty());

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Shift+F2: each side selects what only it has and the newer of what
/// both have; the size settles it only when the times agree.
#[test]
fn compare_selects_what_differs() {
    let at = |seconds: u64| Some(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds));
    let file = |name: &str, seconds: u64, size: u64| Item {
        modified_at: at(seconds),
        size_bytes: size,
        ..row(name)
    };
    let dir = |name: &str| Item { is_dir: true, ..row(name) };
    let left = vec![
        row(".."),
        file("same.txt", 1000, 5),
        file("newer-left.txt", 2000, 5),
        file("newer-right.txt", 1000, 5),
        file("resized.txt", 1000, 5),
        file("fat-rounded.txt", 1001, 5),
        file("only-left.txt", 1000, 5),
        dir("both-dirs"),
        dir("only-left-dir"),
        file("file-or-dir", 1000, 5),
        row("no-time"),
    ];
    let right = vec![
        row(".."),
        file("same.txt", 1000, 5),
        file("newer-left.txt", 1000, 5),
        file("newer-right.txt", 2000, 5),
        file("resized.txt", 1000, 6),
        file("fat-rounded.txt", 1000, 5),
        file("only-right.txt", 1000, 5),
        dir("both-dirs"),
        dir("file-or-dir"),
        Item { size_bytes: 1, ..row("no-time") },
    ];
    let sorted = |names: HashSet<std::ffi::OsString>| {
        let mut names: Vec<String> = names.into_iter().map(|name| name.to_string_lossy().into_owned()).collect();
        names.sort();
        names
    };
    let (on_left, on_right) = crate::app::compare::differences(&left, &right);
    assert_eq!(sorted(on_left), ["file-or-dir", "newer-left.txt", "no-time", "only-left-dir", "only-left.txt", "resized.txt"]);
    assert_eq!(sorted(on_right), ["file-or-dir", "newer-right.txt", "no-time", "only-right.txt", "resized.txt"]);

    // Through the panels: what was selected goes, and the count is said.
    let mut app_state = AppState::new();
    app_state.dir_left = "/left".into();
    app_state.dir_right = "/right".into();
    app_state.children_left = left;
    app_state.children_right = right;
    app_state.selected_left.insert("same.txt".into());
    app_state.compare_dirs();
    assert!(!app_state.selected_left.contains(std::ffi::OsStr::new("same.txt")));
    assert_eq!((app_state.selected_left.len(), app_state.selected_right.len()), (6, 5));
    assert_eq!(app_state.find_note.as_deref(), Some("Differ: 6 on the left, 5 on the right"));

    app_state.children_right = app_state.children_left.clone();
    app_state.compare_dirs();
    assert!(app_state.selected_left.is_empty() && app_state.selected_right.is_empty());
    assert_eq!(app_state.find_note.as_deref(), Some("The directories hold the same files"));

    app_state.dir_right = "/left".into();
    app_state.compare_dirs();
    assert_eq!(app_state.find_note.as_deref(), Some("Both panels show the same directory"));
}

#[test]
fn plus_minus_and_star_select_by_pattern() {
    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    let directory = Item { is_dir: true, ..row("photos") };
    app_state.children_left = vec![row(".."), directory, row("a.jpg"), row("B.JPG"), row("c.png"), row("notes.txt")];
    let selected = |app_state: &AppState| {
        let mut names: Vec<String> = app_state.selected_left.iter().map(|name| name.to_string_lossy().into_owned()).collect();
        names.sort();
        names
    };

    // Through the prompt, as + then typing then Enter does.
    app_state.open_select_prompt(true);
    app_state.dialog_input().unwrap().set("*.jpg;*.png".to_string());
    app_state.confirm_prompt();
    assert_eq!(selected(&app_state), ["B.JPG", "a.jpg", "c.png"]);
    assert_eq!(app_state.select_pattern, "*.jpg;*.png");

    app_state.open_select_prompt(false);
    app_state.dialog_input().unwrap().set("?.png".to_string());
    app_state.confirm_prompt();
    assert_eq!(selected(&app_state), ["B.JPG", "a.jpg"]);

    // Files are inverted; the directory and .. are left alone.
    app_state.invert_selection(false);
    assert_eq!(selected(&app_state), ["c.png", "notes.txt"]);
    // Unless directories are asked for too.
    app_state.invert_selection(true);
    assert_eq!(selected(&app_state), ["B.JPG", "a.jpg", "photos"]);
    app_state.invert_selection(true);
    assert_eq!(selected(&app_state), ["c.png", "notes.txt"]);

    // * takes files only, */ the directories.
    app_state.select_by(&crate::glob::Glob::new("*").unwrap(), true);
    assert_eq!(selected(&app_state), ["B.JPG", "a.jpg", "c.png", "notes.txt"]);
    app_state.select_by(&crate::glob::Glob::new("*/").unwrap(), true);
    assert_eq!(selected(&app_state), ["B.JPG", "a.jpg", "c.png", "notes.txt", "photos"]);

    // An empty pattern does nothing and is not kept.
    app_state.open_select_prompt(false);
    app_state.dialog_input().unwrap().set(String::new());
    app_state.confirm_prompt();
    assert_eq!(selected(&app_state).len(), 5);
    assert_eq!(app_state.select_pattern, "?.png");
}

#[test]
fn a_selection_outlives_a_re_sort_but_not_its_file() {
    let mut selected: HashSet<std::ffi::OsString> = ["one.txt", "two.txt"].iter().map(|name| name.into()).collect();

    // Re-sorted, renumbered: both files are still there, so both stay
    // selected. This is what keying by name rather than row is for.
    prune_selection(&mut selected, &[row("zzz.txt"), row("two.txt"), row("one.txt")]);
    assert_eq!(selected.len(), 2);

    // one.txt is gone, so its selection goes with it.
    prune_selection(&mut selected, &[row("renamed.txt"), row("two.txt")]);
    assert_eq!(selected.iter().collect::<Vec<_>>(), ["two.txt"]);

    // And it does not come back when something else takes the name, which
    // would otherwise put a file the user never picked into the next delete.
    prune_selection(&mut selected, &[row("one.txt"), row("two.txt")]);
    assert_eq!(selected.iter().collect::<Vec<_>>(), ["two.txt"]);

    // An empty directory clears the lot.
    prune_selection(&mut selected, &[]);
    assert!(selected.is_empty());
}

/// Two names that are not valid UTF-8 can read the same once made
/// printable. Selecting one must not select the other, or the next delete
/// takes a file nobody picked.
#[cfg(unix)]
#[test]
fn names_that_read_alike_are_selected_apart() {
    use std::os::unix::ffi::OsStrExt;

    let dir = std::env::temp_dir().join(format!("fm84-alike-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for raw in [&b"a\xfe"[..], &b"a\xff"[..]] {
        std::fs::write(dir.join(std::ffi::OsStr::from_bytes(raw)), "").unwrap();
    }

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(true, dir.clone(), None);
    let rows: Vec<Item> = app_state.children_left.iter().filter(|item| item.name != "..").cloned().collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name_full, rows[1].name_full);

    let index = app_state.children_left.iter().position(|row| row.name_os == rows[0].name_os).unwrap();
    app_state.state_left.select(Some(index));
    app_state.toggle_selection_no_size();
    assert_eq!(app_state.selected_left.iter().collect::<Vec<_>>(), [&rows[0].name_os]);

    std::fs::remove_dir_all(&dir).unwrap();
}
