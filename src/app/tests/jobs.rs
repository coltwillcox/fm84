//! Jobs asking about what fails and what is in the way.

use super::*;

/// A copy that meets an entry it cannot copy stops and asks. Skipped, the
/// rest goes on, and the end says how many were left.
#[cfg(unix)]
#[test]
fn a_job_asks_about_a_failure_and_goes_on_when_told_to_skip() {
    let dir = std::env::temp_dir().join(format!("fm84-ask-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tree = dir.join("tree");
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::write(tree.join("a.txt"), "a").unwrap();
    let pipe = std::ffi::CString::new(tree.join("pipe").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
    std::fs::write(tree.join("z.txt"), "z").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.start_transfer(vec![(tree.clone(), dir.join("copy"), true)], true, crate::app::Clashes::Ask);

    let started = std::time::Instant::now();
    let wait = |app_state: &mut AppState, done: &dyn Fn(&AppState) -> bool| {
        while !done(app_state) {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never got there");
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_state.poll_transfer();
        }
    };
    wait(&mut app_state, &|app_state| app_state.job.as_ref().is_some_and(|job| job.problem.is_some()));
    assert_eq!(app_state.job.as_ref().unwrap().problem.as_ref().unwrap().path, tree.join("pipe"));

    app_state.job.as_mut().unwrap().answer(crate::app::Answer::Skip);
    wait(&mut app_state, &|app_state| app_state.job.is_none());
    assert_eq!(app_state.error.as_deref(), Some("Copy finished, 1 entry skipped"));
    assert!(dir.join("copy").join("a.txt").exists());
    assert!(dir.join("copy").join("z.txt").exists());

    // Esc on the question stops the job there, and says it was cancelled
    // rather than reporting the failure it was asked about.
    app_state.start_transfer(vec![(tree.clone(), dir.join("again"), true)], true, crate::app::Clashes::Ask);
    wait(&mut app_state, &|app_state| app_state.job.as_ref().is_some_and(|job| job.problem.is_some()));
    app_state.cancel_transfer();
    wait(&mut app_state, &|app_state| app_state.job.is_none());
    assert!(app_state.error.as_deref().unwrap_or_default().starts_with("Copy cancelled"), "{:?}", app_state.error);

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Esc pressed after the worker asked, but before the question reached the
/// screen, found nothing to answer. The question then came up anyway, and
/// the job waited on a second answer.
#[test]
fn esc_answers_a_question_still_on_its_way() {
    let dir = std::env::temp_dir().join(format!("fm84-esc-race-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("onto")).unwrap();
    std::fs::write(dir.join("a.txt"), "new").unwrap();
    std::fs::write(dir.join("onto").join("a.txt"), "old").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.start_transfer(vec![(dir.join("a.txt"), dir.join("onto").join("a.txt"), false)], true, crate::app::Clashes::Ask);
    // Time for the worker to ask, with nothing polled to take the question in.
    std::thread::sleep(std::time::Duration::from_millis(200));
    app_state.cancel_transfer();

    let started = std::time::Instant::now();
    while app_state.job.is_some() {
        assert!(app_state.job.as_ref().is_some_and(|job| job.clash.is_none()), "the question came up after Esc");
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never ended");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
    assert_eq!(std::fs::read_to_string(dir.join("onto").join("a.txt")).unwrap(), "old");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Each file already there is asked about as the job reaches it, and the
/// answers do what they say: one file, every file, or every older file.
#[test]
fn files_in_the_way_are_asked_about_one_at_a_time() {
    use crate::app::{Answer, Clashes};
    let dir = std::env::temp_dir().join(format!("fm84-clashes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let then = |seconds: u64| std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds);
    let write = |path: &std::path::Path, text: &str, at: u64| {
        std::fs::write(path, text).unwrap();
        std::fs::File::options().write(true).open(path).unwrap().set_modified(then(at)).unwrap();
    };
    // A tree to copy, onto one holding the same names: some older than
    // what would replace them, one newer.
    let fresh = |name: &str| {
        let (from, onto) = (dir.join(name).join("from"), dir.join(name).join("onto"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&onto).unwrap();
        for file in ["a", "b", "c"] {
            write(&from.join(file), "new", 2_000_000_000);
            write(&onto.join(file), "old", if file == "c" { 2_100_000_000 } else { 1_000_000_000 });
        }
        write(&from.join("only-new"), "new", 2_000_000_000);
        (from, onto)
    };
    // Run a job to the end, answering by the name asked about. How many
    // questions there were, and what each file holds afterwards.
    let run = |clashes: Clashes, answer: &dyn Fn(&str) -> Answer, from: &std::path::Path, onto: &std::path::Path| {
        let mut app_state = AppState::new();
        app_state.start_transfer(vec![(from.to_path_buf(), onto.to_path_buf(), true)], true, clashes);
        let (mut asked, started) = (0, std::time::Instant::now());
        while let Some(job) = app_state.job.as_mut() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never finished");
            if let Some(clash) = &job.clash {
                let name = clash.dest.file_name().unwrap().to_string_lossy().into_owned();
                asked += 1;
                job.answer(answer(&name));
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
            app_state.poll_transfer();
        }
        let held: Vec<String> = ["a", "b", "c", "only-new"].iter().map(|file| std::fs::read_to_string(onto.join(file)).unwrap()).collect();
        (asked, held, app_state.error)
    };

    // One at a time: overwrite a, skip b, overwrite c. A file with nothing
    // in its way is never asked about.
    let (from, onto) = fresh("each");
    let (asked, held, said) = run(Clashes::Ask, &|name| if name == "b" { Answer::Skip } else { Answer::Overwrite }, &from, &onto);
    assert_eq!(asked, 3);
    assert_eq!(held, ["new", "old", "new", "new"]);
    assert_eq!(said.as_deref(), Some("Copy finished, 1 entry skipped"));

    // Overwrite all and skip all are asked once, and hold for the rest.
    let (from, onto) = fresh("all");
    let (asked, held, _) = run(Clashes::Ask, &|_| Answer::OverwriteAll, &from, &onto);
    assert_eq!((asked, held), (1, vec!["new".to_string(); 4]));
    let (from, onto) = fresh("none");
    let (asked, held, said) = run(Clashes::Ask, &|_| Answer::SkipAll, &from, &onto);
    assert_eq!((asked, held), (1, vec!["old".into(), "old".into(), "old".into(), "new".into()]));
    assert_eq!(said.as_deref(), Some("Copy finished, 3 entries skipped"));

    // Overwrite older: c is newer than what would replace it, and stays.
    let (from, onto) = fresh("older");
    let (asked, held, _) = run(Clashes::Ask, &|_| Answer::OverwriteOlder, &from, &onto);
    assert_eq!((asked, held), (1, vec!["new".into(), "new".into(), "old".into(), "new".into()]));

    // F11's Overwrite asks nothing at all.
    let (from, onto) = fresh("overwrite");
    let (asked, held, _) = run(Clashes::ReplaceAll, &|_| Answer::Abort, &from, &onto);
    assert_eq!((asked, held), (0, vec!["new".to_string(); 4]));

    std::fs::remove_dir_all(&dir).unwrap();
}
