//! F1's high scores: counted as jobs finish, and drawn as a cabinet would.

use super::*;
use std::time::{Duration, Instant};

fn finish(app_state: &mut AppState) {
    let started = Instant::now();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < Duration::from_secs(10), "the job never finished");
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_transfer();
    }
}

/// F1 as drawn, a row a line.
fn f1(app_state: &mut AppState) -> String {
    app_state.dialog = Some(crate::app::Dialog::Help);
    app_state.help_scroll = usize::MAX;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 200)).unwrap();
    crate::ui::render_ui(&mut terminal, app_state);
    let buffer = terminal.backend().buffer().clone();
    (0..200).map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect::<String>() + "\n").collect()
}

#[test]
fn jobs_count_onto_the_high_scores() {
    let dir = std::env::temp_dir().join(format!("fm84-scores-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), vec![b'x'; 1500]).unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    // In F1's lines rather than one frame of it: it blinks.
    assert!(crate::ui::help_lines(&app_state.scores).iter().any(|(text, kind)| text == "INSERT COIN" && *kind == crate::ui::HelpLine::Coin));

    app_state.start_transfer(vec![(dir.join("a.txt"), dir.join("b.txt"), false)], true, crate::app::Clashes::Ask);
    finish(&mut app_state);
    crate::fs_ops::rename_path(dir.join("b.txt"), dir.join("c.txt")).unwrap();
    app_state.journal_renames(&[(dir.join("b.txt"), dir.join("c.txt"))]);
    app_state.undo_last();
    finish(&mut app_state);
    let scores = &app_state.scores;
    assert_eq!((scores.bytes_copied, scores.files_renamed, scores.undos, scores.jobs), (1500, 1, 1, 2));

    let drawn = f1(&mut app_state);
    assert!(!drawn.contains("INSERT COIN"));
    for text in ["High scores", "BYTES COPIED", "FILES RENAMED", "★ Player one", "★ Second thoughts", "☆ Moved a terabyte"] {
        assert!(drawn.contains(text), "{text} not drawn:\n{drawn}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_scores_file_reads_back_what_was_written() {
    let scores = crate::options::Scores {
        bytes_copied: 7,
        longest_job: 3600,
        revisions: 2,
        ..Default::default()
    };
    let text = "bytes_copied = 7\nlongest_job = 3600\nrevisions = 2\nnot_a_score = 9\n";
    assert_eq!(crate::options::Scores::parse(text), scores);
    let had: Vec<bool> = scores.achievements().iter().map(|achievement| achievement.unlocked).collect();
    // Marathon and Time traveller; no job finished, so no Player one.
    assert_eq!(had, [false, false, false, true, false, false, false, true]);
}
