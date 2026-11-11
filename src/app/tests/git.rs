//! Alt+G: git from the panel.

use super::*;
use crate::app::{Dialog, GitAction, Screen};

#[test]
fn alt_g_asks_before_discarding_and_compares_with_head() {
    let dir = std::env::temp_dir().join(format!("fm84-alt-g-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |arguments: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
            .args(arguments)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    if !git(&["init", "--quiet"]) {
        return;
    }
    let file = dir.join("notes.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    assert!(git(&["add", "notes.txt"]) && git(&["commit", "--quiet", "-m", "first"]));
    std::fs::write(&file, "one\nthree\n").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_git(vec![(file.clone(), false)]);
    app_state.git_compare_head();
    let Screen::Diff(state) = &app_state.screen else { panic!("no Compare view: {:?}", app_state.error) };
    assert_eq!(state.left_path, dir.join("notes.txt @ HEAD"));
    assert_eq!(state.right_path, file);
    app_state.close_diff();

    // D asks; only the answer throws the change away.
    app_state.open_git(vec![(file.clone(), false)]);
    app_state.git_act(GitAction::Discard);
    assert!(matches!(app_state.dialog, Some(Dialog::Git { discarding: true, .. })));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nthree\n");
    app_state.git_act(GitAction::Discard);
    assert!(app_state.dialog.is_none());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
    assert_eq!(app_state.find_note.as_deref(), Some("Changes discarded"));

    // A directory has no one file to compare.
    app_state.open_git(vec![(dir.clone(), true)]);
    app_state.git_compare_head();
    assert_eq!(app_state.error.as_deref(), Some("Only a file can be compared with HEAD"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// R on Alt+G: an older commit as a folder, gone into from where the panel
/// was; its files listed, read and copied out as they were then.
#[test]
fn a_revision_opens_as_a_read_only_folder() {
    let dir = std::env::temp_dir().join(format!("fm84-revision-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let git = |arguments: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
            .args(arguments)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    if !git(&["init", "--quiet", "--initial-branch=main"]) {
        return;
    }
    std::fs::write(dir.join("a.txt"), "then\n").unwrap();
    std::fs::write(dir.join("sub").join("b.txt"), "inside\n").unwrap();
    assert!(git(&["add", "."]) && git(&["commit", "--quiet", "-m", "first"]));
    std::fs::write(dir.join("a.txt"), "now\n").unwrap();
    std::fs::remove_file(dir.join("sub").join("b.txt")).unwrap();
    std::fs::write(dir.join("sub").join("c.txt"), "new\n").unwrap();
    assert!(git(&["add", "-A"]) && git(&["commit", "--quiet", "-m", "second"]) && git(&["branch", "feature/old", "HEAD~1"]));
    assert!(git(&["tag", "v1", "HEAD~1"]));
    std::fs::write(dir.join("a.txt"), "half done\n").unwrap();
    assert!(git(&["stash", "--quiet"]));

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.join("sub"), None);
    app_state.ask_revision();
    // Everything there is to open, each kind together.
    let Some(Dialog::Revision(list)) = &app_state.dialog else { panic!("no list: {:?}", app_state.error) };
    use crate::git::RevisionKind;
    let listed: Vec<(RevisionKind, &str, bool)> = list.all.iter().map(|revision| (revision.kind, revision.name.as_str(), revision.current)).collect();
    assert_eq!(
        listed[..4],
        [
            (RevisionKind::Branch, "feature/old", false),
            (RevisionKind::Branch, "main", true),
            (RevisionKind::Tag, "v1", false),
            (RevisionKind::Stash, "stash@{0}", false)
        ]
    );
    assert_eq!(listed[4..].iter().filter(|(kind, _, _)| *kind == RevisionKind::Commit).count(), 2);
    assert_eq!(list.all.iter().find(|revision| revision.kind == RevisionKind::Commit).map(|revision| revision.subject.as_str()), Some("second"));
    // Drawn with each kind's name on its first row, the branch checked out
    // marked, and every row's subject.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let buffer = terminal.backend().buffer().clone();
    let screen: String = (0..40).map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect::<String>() + "\n").collect();
    for text in ["Branches", "• main", "Tags", "v1", "Stashes", "stash@{0}", "Commits", "second", "Enter - Open"] {
        assert!(screen.contains(text), "{text} not drawn:\n{screen}");
    }
    // Typed, it narrows; the highlight is what Enter opens.
    if let Some(Dialog::Revision(list)) = &mut app_state.dialog {
        list.input.set("old".to_string());
    }
    app_state.revisions_typed();
    let Some(Dialog::Revision(list)) = &app_state.dialog else { unreachable!() };
    assert_eq!(list.shown.iter().map(|&at| list.all[at].name.as_str()).collect::<Vec<_>>(), ["feature/old"]);
    app_state.open_revision();
    let started = std::time::Instant::now();
    while app_state.left.listing.is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "never listed");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_listings();
    }
    assert_eq!(app_state.error, None);
    let folder = dir.join("@git:feature%2Fold");
    assert_eq!(app_state.left.dir, folder.join("sub"));
    assert_eq!(app_state.panel_archive(Side::Left), Some(&folder));
    let names: Vec<String> = app_state.left.children.iter().filter(|item| !item.is_parent()).map(|item| item.name_full.clone()).collect();
    assert_eq!(names, ["b.txt"]);

    // Read as it was, and copied out as it was.
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let items = vec![(std::path::PathBuf::from("a.txt"), out.join("a.txt")), (std::path::PathBuf::from("sub"), out.join("sub"))];
    let done = crate::fs_ops::extract(&folder, &items, &mut |_: crate::fs_ops::Step<'_>| true).unwrap();
    assert_eq!(done, crate::fs_ops::Transfer::Done);
    assert_eq!(std::fs::read_to_string(out.join("a.txt")).unwrap(), "then\n");
    assert_eq!(std::fs::read_to_string(out.join("sub").join("b.txt")).unwrap(), "inside\n");

    // Writing into it is refused, as into any archive.
    assert!(app_state.refuse_in_archive(Side::Left));
    app_state.reset_error();

    // A revision that is not one says so, and the panel stays.
    app_state.ask_revision();
    if let Some(Dialog::Revision(list)) = &mut app_state.dialog {
        list.input.set("nowhere".to_string());
    }
    app_state.revisions_typed();
    app_state.open_revision();
    assert_eq!(app_state.error.as_deref(), Some("No such revision: nowhere"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Alt+Left and Alt+Right in the viewer: the file as each commit that
/// changed it had it, back to the first and on to the file as it is now,
/// the view and the matches kept as it goes. A version from the past is
/// not followed, and its hexdump is of it, not of the file on the disk.
#[test]
fn the_viewer_steps_through_a_files_history() {
    let dir = std::env::temp_dir().join(format!("fm84-time-machine-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |arguments: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
            .args(arguments)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    if !git(&["init", "--quiet"]) {
        return;
    }
    let file = dir.join("notes.txt");
    for (text, subject) in [("a\nb\nfirst\n", "first"), ("a\nb\nsecond\n", "second")] {
        std::fs::write(&file, text).unwrap();
        assert!(git(&["add", "notes.txt"]) && git(&["commit", "--quiet", "-m", subject]));
    }
    std::fs::write(&file, "a\nb\nnow\n").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.drawn.viewer_viewport_height = 10;
    app_state.open_viewer(file.clone()).unwrap();
    app_state.viewer_mut().unwrap().scroll_offset = 1;
    app_state.find_shown = true;
    let shown = |app_state: &AppState| app_state.viewer().unwrap().content_lines[2].clone();
    let at = |app_state: &AppState| app_state.viewer().unwrap().history.as_ref().and_then(|history| history.at);

    // Nothing newer than the file as it is.
    app_state.viewer_step_history(false);
    assert_eq!(shown(&app_state), "now");
    app_state.viewer_step_history(true);
    assert_eq!((shown(&app_state), at(&app_state)), ("second".to_string(), Some(0)));
    let state = app_state.viewer().unwrap();
    assert_eq!(state.scroll_offset, 1, "the view stays where it was");
    assert_eq!(state.history.as_ref().unwrap().commits[0].subject, "second");
    assert!(app_state.find_shown, "the matches stay marked");
    app_state.viewer_step_history(true);
    assert_eq!(shown(&app_state), "first");
    // Nothing older than the first.
    app_state.viewer_step_history(true);
    assert_eq!((shown(&app_state), at(&app_state)), ("first".to_string(), Some(1)));

    app_state.viewer_toggle_follow();
    assert!(app_state.viewer().unwrap().follow.is_none(), "a commit's version does not grow");
    app_state.viewer_next_mode();
    assert_eq!(app_state.viewer().unwrap().bytes, b"a\nb\nfirst\n");
    app_state.viewer_step_history(false);
    let state = app_state.viewer().unwrap();
    assert_eq!((state.mode, state.bytes.as_slice()), (crate::viewer::ViewMode::Hex, b"a\nb\nsecond\n".as_slice()), "a hexdump stays one");
    app_state.viewer_next_mode();

    app_state.viewer_step_history(false);
    assert_eq!((shown(&app_state), at(&app_state)), ("now".to_string(), None));
    assert!(app_state.error.is_none(), "{:?}", app_state.error);

    // A file git has never had has no history.
    let new = dir.join("new.txt");
    std::fs::write(&new, "new\n").unwrap();
    app_state.open_viewer(new).unwrap();
    app_state.viewer_step_history(true);
    assert_eq!(app_state.error.as_deref(), Some("No commits of new.txt to step through"));
    app_state.error = None;

    // A picture a commit had is drawn as one, when it is small enough to
    // open without asking - and one that is not, as the binary it is,
    // never decoded.
    let picture = dir.join("logo.bmp");
    let mut small = Vec::new();
    image::DynamicImage::new_rgba8(4, 4).write_to(&mut std::io::Cursor::new(&mut small), image::ImageFormat::Bmp).unwrap();
    let mut huge = small.clone();
    // The same header, saying 12000 by 12000.
    huge[18..22].copy_from_slice(&12000i32.to_le_bytes());
    huge[22..26].copy_from_slice(&12000i32.to_le_bytes());
    for bytes in [&huge, &small] {
        std::fs::write(&picture, bytes).unwrap();
        assert!(git(&["add", "logo.bmp"]) && git(&["commit", "--quiet", "-m", "logo"]));
    }
    app_state.open_viewer(picture).unwrap();
    // Off the disk as it is, and as the newest commit had it: small.
    app_state.viewer_step_history(true);
    let state = app_state.viewer().unwrap();
    assert_eq!((state.mode, state.image.is_some()), (crate::viewer::ViewMode::Image, true), "{:?}", app_state.error);
    app_state.viewer_step_history(true);
    let state = app_state.viewer().unwrap();
    assert_eq!((state.mode, state.image.is_some(), state.bytes.len()), (crate::viewer::ViewMode::Hex, false, huge.len()));

    // A version past F11's limit is not read at all, as a file that size
    // would be asked about before it opened; the view stays as it was.
    app_state.options.large_file_mib = 0;
    app_state.open_viewer(file.clone()).unwrap();
    app_state.viewer_step_history(true);
    assert!(app_state.error.as_deref().is_some_and(|error| error.starts_with("Too large to view as that commit had it")), "{:?}", app_state.error);
    assert_eq!((shown(&app_state), at(&app_state)), ("now".to_string(), None));
    std::fs::remove_dir_all(&dir).unwrap();
}
