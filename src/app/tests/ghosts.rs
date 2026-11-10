//! Alt+Shift+G: the files nobody has touched in years, fed to the panel
//! faded, oldest first, nothing chosen.

use super::*;
use std::time::{Duration, Instant, SystemTime};

/// A file last read `read` ago and last written `written` ago.
fn aged(path: &std::path::Path, read: Duration, written: Duration) {
    std::fs::write(path, "boo").unwrap();
    let times = std::fs::FileTimes::new().set_accessed(SystemTime::now() - read).set_modified(SystemTime::now() - written);
    std::fs::File::options().write(true).open(path).unwrap().set_times(times).unwrap();
}

fn hunt(app_state: &mut AppState) {
    app_state.hunt_ghosts();
    let started = Instant::now();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < Duration::from_secs(10), "the hunt never finished");
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_transfer();
    }
}

#[test]
fn files_untouched_for_years_are_fed_as_ghosts() {
    let dir = std::env::temp_dir().join(format!("fm84-ghosts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("old/attic")).unwrap();
    let year = Duration::from_secs(365 * 24 * 60 * 60);
    aged(&dir.join("old/attic/letter.txt"), year * 9, year * 10);
    aged(&dir.join("old/tax-2019.pdf"), year * 3, year * 5);
    // Read last month, if written long ago: not forgotten.
    aged(&dir.join("old/read.txt"), year / 12, year * 6);
    // Written last month, whatever the access time says.
    aged(&dir.join("written.txt"), year * 6, year / 12);
    aged(&dir.join("new.txt"), Duration::ZERO, Duration::ZERO);

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.active_side = Side::Left;
    hunt(&mut app_state);
    assert_eq!(app_state.error, None);

    let flat = app_state.left.flat.as_ref().expect("fed to the panel");
    assert_eq!(flat.kind, crate::app::FeedKind::Ghosts);
    let feed: Vec<std::path::PathBuf> = flat.feed.as_ref().unwrap().iter().map(|(path, _)| path.clone()).collect();
    // Oldest first.
    assert_eq!(feed, [std::path::Path::new("old/attic/letter.txt"), std::path::Path::new("old/tax-2019.pdf")]);
    assert!(app_state.left.selected.is_empty(), "nothing chosen for F8");

    // None to find: said, and the panel left as it was.
    std::fs::remove_dir_all(dir.join("old")).unwrap();
    app_state.left.flat = None;
    app_state.reload_panel(Side::Left, None);
    hunt(&mut app_state);
    assert_eq!(app_state.find_note.as_deref(), Some("No ghosts here"));
    assert!(app_state.left.flat.is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}
