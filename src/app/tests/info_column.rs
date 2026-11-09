//! The Info column: values from commands, cached, and sorted by.

use super::*;
use crate::options::{Association, SortKey};
use std::ffi::OsString;
use std::time::{Duration, Instant};

fn wait(app_state: &mut AppState) {
    let started = Instant::now();
    loop {
        app_state.poll_info();
        if app_state.left.info.values.len() >= 3 {
            return;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "values never came: {:?}", app_state.left.info.values);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(unix)]
#[test]
fn a_command_fills_the_column_and_the_panel_sorts_by_it() {
    let dir = std::env::temp_dir().join(format!("fm84-info-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Their line counts, by name, run against what the files hold.
    std::fs::write(dir.join("a.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n").unwrap();
    std::fs::write(dir.join("b.txt"), "1\n").unwrap();
    std::fs::write(dir.join("c.txt"), "1\n2\n").unwrap();
    std::fs::write(dir.join("d.bin"), "not for the command").unwrap();
    let count = vec![Association {
        extensions: vec!["txt".into()],
        command: "wc -l < {file} | tr -d ' '".into(),
        terminal: false,
    }];

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.options.column_info = true;
    app_state.options.sort_key = SortKey::Info;
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.fill_info(Side::Left, &count);
    wait(&mut app_state);
    let value = |app_state: &AppState, name: &str| app_state.left.info.values.get(&OsString::from(name)).cloned();
    assert_eq!(value(&app_state, "a.txt").as_deref(), Some("10"));
    assert_eq!(value(&app_state, "d.bin"), None);

    // 1, 2, 10 - as numbers - and the one with no value last.
    let started = Instant::now();
    let order = |app_state: &AppState| app_state.left.children.iter().filter(|item| !item.is_parent()).map(|item| item.name_full.clone()).collect::<Vec<_>>();
    while order(&app_state)[0] != "b.txt" {
        assert!(started.elapsed() < Duration::from_secs(10), "never sorted: {:?}", order(&app_state));
        std::thread::sleep(Duration::from_millis(5));
        app_state.poll_info();
    }
    assert_eq!(order(&app_state), ["b.txt", "c.txt", "a.txt", "d.bin"]);

    // Asked again, nothing runs: what was worked out is kept.
    app_state.fill_info(Side::Left, &count);
    assert_eq!(app_state.left.info.values.len(), 3);
    // A file changed is asked again.
    std::fs::write(dir.join("b.txt"), "1\n2\n3\n4\n").unwrap();
    app_state.reload_panel(Side::Left, None);
    app_state.fill_info(Side::Left, &count);
    wait(&mut app_state);
    assert_eq!(value(&app_state, "b.txt").as_deref(), Some("4"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A command that takes too long is stopped, and gives nothing.
#[cfg(unix)]
#[test]
fn a_slow_command_is_stopped() {
    let dir = std::env::temp_dir().join(format!("fm84-info-slow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "").unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.options.column_info = true;
    app_state.open_dir(Side::Left, dir.clone(), None);
    let started = Instant::now();
    app_state.fill_info(
        Side::Left,
        &[Association {
            extensions: vec!["*".into()],
            command: "sleep 30; echo late".into(),
            terminal: false,
        }],
    );
    while app_state.left.info.values.is_empty() {
        assert!(started.elapsed() < Duration::from_secs(10), "never stopped");
        std::thread::sleep(Duration::from_millis(20));
        app_state.poll_info();
    }
    assert_eq!(app_state.left.info.values.get(&OsString::from("a.txt")).map(String::as_str), Some(""));
    std::fs::remove_dir_all(&dir).unwrap();
}
