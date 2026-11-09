//! Alt+R: a directory summed up.

use super::*;
use crate::app::{DOSSIER_WEEKS, Dialog};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn a_directory_is_summed_up() {
    let dir = std::env::temp_dir().join(format!("fm84-dossier-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {\n    run();\n}\n").unwrap();
    // A last line with no newline still counts.
    std::fs::write(dir.join("src/lib.rs"), "pub fn run() {}").unwrap();
    std::fs::write(dir.join("tool.py"), "print(1)\nprint(2)\n").unwrap();
    std::fs::write(dir.join("photo.jpg"), vec![7u8; 4000]).unwrap();
    std::fs::write(dir.join("src/photo copy.jpg"), vec![7u8; 4000]).unwrap();
    std::fs::write(dir.join("README"), "no extension").unwrap();
    let git = |arguments: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
            .args(arguments)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    let in_git = git(&["init", "--quiet"]) && git(&["add", "."]) && git(&["commit", "--quiet", "-m", "first"]);

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.active_side = Side::Left;
    app_state.open_dossier();
    let started = Instant::now();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < Duration::from_secs(10), "never finished");
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_transfer();
    }
    let Some(Dialog::Dossier(view)) = &app_state.dialog else { panic!("no dossier: {:?}", app_state.error) };
    let dossier = &view.dossier;

    // Git's own directory is not counted.
    assert_eq!((dossier.files, dossier.dirs), (6, 1));
    assert_eq!(dossier.kinds[0], ("jpg".to_string(), 8000, 2));
    assert!(dossier.kinds.iter().any(|kind| kind.0.is_empty()), "{:?}", dossier.kinds);
    assert_eq!(dossier.largest[0].1, 4000);
    assert_eq!(dossier.languages, [("Rust", 4, 2), ("Python", 2, 1)]);
    assert_eq!((dossier.copies, dossier.wasted), (1, 4000));
    if in_git {
        assert_eq!(dossier.weeks.map(|weeks| weeks[DOSSIER_WEEKS - 1]), Some(1));
    }
    let newest: Vec<PathBuf> = dossier.newest.iter().map(|(path, _)| path.clone()).collect();
    assert!(newest.iter().all(|path| path.is_relative()), "{newest:?}");

    // Drawn, every section of it.
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 60)).unwrap();
    crate::ui::render_ui(&mut terminal, &mut app_state);
    let buffer = terminal.backend().buffer().clone();
    let screen: String = (0..60).map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect::<String>() + "\n").collect();
    for text in ["Kinds of file", ".jpg", "Largest", "Newest", "Languages, by lines", "Rust", "Duplicates", "Copies of another: 1"] {
        assert!(screen.contains(text), "{text} not drawn:\n{screen}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
