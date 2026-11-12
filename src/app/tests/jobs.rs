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
    wait(&mut app_state, &|app_state| app_state.workers.job.as_ref().is_some_and(|job| job.problem.is_some()));
    assert_eq!(app_state.workers.job.as_ref().unwrap().problem.as_ref().unwrap().path, tree.join("pipe"));

    app_state.workers.job.as_mut().unwrap().answer(crate::app::Answer::Skip);
    wait(&mut app_state, &|app_state| app_state.workers.job.is_none());
    assert_eq!(app_state.error.as_deref(), Some("Copy finished, 1 entry skipped"));
    assert!(dir.join("copy").join("a.txt").exists());
    assert!(dir.join("copy").join("z.txt").exists());

    // Esc on the question stops the job there, and says it was cancelled
    // rather than reporting the failure it was asked about.
    app_state.start_transfer(vec![(tree.clone(), dir.join("again"), true)], true, crate::app::Clashes::Ask);
    wait(&mut app_state, &|app_state| app_state.workers.job.as_ref().is_some_and(|job| job.problem.is_some()));
    app_state.cancel_transfer();
    wait(&mut app_state, &|app_state| app_state.workers.job.is_none());
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
    while app_state.workers.job.is_some() {
        assert!(app_state.workers.job.as_ref().is_some_and(|job| job.clash.is_none()), "the question came up after Esc");
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
        while let Some(job) = app_state.workers.job.as_mut() {
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

    // Keep both leaves what was there and puts the new one beside it, and
    // is nothing skipped; for all, it is asked once - and F11's Keep both
    // asks nothing.
    let beside = |onto: &std::path::Path| ["a (2)", "b (2)", "c (2)"].map(|file| std::fs::read_to_string(onto.join(file)).ok());
    let (from, onto) = fresh("keep-one");
    let (asked, held, said) = run(Clashes::Ask, &|name| if name == "b" { Answer::KeepBoth } else { Answer::Skip }, &from, &onto);
    assert_eq!((asked, held), (3, vec!["old".into(), "old".into(), "old".into(), "new".into()]));
    assert_eq!(beside(&onto), [None, Some("new".to_string()), None]);
    assert_eq!(said.as_deref(), Some("Copy finished, 2 entries skipped"));
    let (from, onto) = fresh("keep-all");
    let (asked, held, said) = run(Clashes::Ask, &|_| Answer::KeepBothAll, &from, &onto);
    assert_eq!((asked, held, said), (1, vec!["old".into(), "old".into(), "old".into(), "new".into()], None));
    assert_eq!(beside(&onto), [Some("new".to_string()), Some("new".to_string()), Some("new".to_string())]);
    let (from, onto) = fresh("keep-f11");
    let (asked, _, _) = run(Clashes::KeepBothAll, &|_| Answer::Abort, &from, &onto);
    assert_eq!((asked, beside(&onto).iter().flatten().count()), (0, 3));

    std::fs::remove_dir_all(&dir).unwrap();
}

/// A copy bigger than the room where it goes asks before it writes a byte:
/// R looks again and asks again while there is still not the room, and Esc
/// stops it there with the reason. A sparse file stands for a big one - it
/// says it is far larger than it takes.
#[cfg(unix)]
#[test]
fn a_copy_without_the_room_asks_before_it_starts() {
    let dir = std::env::temp_dir().join(format!("fm84-no-room-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("onto")).unwrap();
    let Some(free) = crate::fs_ops::free_space(&dir) else {
        return;
    };
    let file = std::fs::File::create(dir.join("huge")).unwrap();
    // A filesystem that will not hold a file that size has nothing to show.
    if file.set_len(free.saturating_add(1 << 30)).is_err() {
        std::fs::remove_dir_all(&dir).unwrap();
        return;
    }
    drop(file);

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.start_transfer(vec![(dir.join("huge"), dir.join("onto").join("huge"), false)], true, crate::app::Clashes::Ask);

    let started = std::time::Instant::now();
    let wait = |app_state: &mut AppState, done: &dyn Fn(&AppState) -> bool| {
        while !done(app_state) {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never got there");
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_state.poll_transfer();
        }
    };
    let asked = |app_state: &AppState| app_state.workers.job.as_ref().is_some_and(|job| job.room.is_some());
    wait(&mut app_state, &asked);
    let short = app_state.workers.job.as_ref().unwrap().room.clone().unwrap();
    assert_eq!(short.dir, dir.join("onto"));
    assert!(short.needed > short.free);

    // Looked at again, it is still short, so it asks again.
    app_state.workers.job.as_mut().unwrap().answer(crate::app::Answer::Retry);
    wait(&mut app_state, &asked);

    app_state.cancel_transfer();
    wait(&mut app_state, &|app_state| app_state.workers.job.is_none());
    assert!(app_state.error.as_deref().unwrap_or_default().starts_with("Not enough room: Needs "), "{:?}", app_state.error);
    assert!(!dir.join("onto").join("huge").exists(), "something was written");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// With F11's Verify copies on, a copy job reads each file back: the bar's
/// total counts every byte twice, and reaches it.
#[test]
fn verify_copies_reads_back_what_a_job_copies() {
    let dir = std::env::temp_dir().join(format!("fm84-verify-job-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tree")).unwrap();
    std::fs::write(dir.join("tree").join("a"), vec![7u8; 5000]).unwrap();
    std::fs::write(dir.join("tree").join("b"), vec![9u8; 3000]).unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options { verify_copies: true, ..Options::default() };
    app_state.start_transfer(vec![(dir.join("tree"), dir.join("copy"), true)], true, crate::app::Clashes::Ask);
    let started = std::time::Instant::now();
    let (mut total, mut done) = (None, 0);
    while let Some(job) = &app_state.workers.job {
        (total, done) = (job.total, job.done);
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never ended");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
    assert_eq!(app_state.error, None);
    assert_eq!(total, Some(16000));
    assert!(done <= 16000);
    assert_eq!(std::fs::read(dir.join("copy").join("a")).unwrap(), vec![7u8; 5000]);
    assert_eq!(std::fs::read(dir.join("copy").join("b")).unwrap(), vec![9u8; 3000]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file larger than where it goes takes in one is asked about before
/// anything is written. Split, it lands as pieces that fit with a
/// .sha256 beside them that sha256sum reads; F5 on the first piece joins
/// them back, checked; and a piece gone bad fails the join, leaving
/// nothing.
#[test]
fn a_file_too_big_for_where_it_goes_is_split_and_joined_back() {
    let dir = std::env::temp_dir().join(format!("fm84-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stick = dir.join("stick");
    std::fs::create_dir_all(&stick).unwrap();
    std::fs::create_dir_all(dir.join("home")).unwrap();
    crate::fs_ops::test_limits::set(&stick, 10);
    let content: Vec<u8> = (0..25u8).collect();
    let source = dir.join("big.iso");
    std::fs::write(&source, &content).unwrap();
    std::fs::write(dir.join("small.txt"), "fits").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    let started = std::time::Instant::now();
    let wait = |app_state: &mut AppState, done: &dyn Fn(&AppState) -> bool| {
        while !done(app_state) {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never got there");
            std::thread::sleep(std::time::Duration::from_millis(5));
            app_state.poll_transfer();
        }
    };
    let asked = |app_state: &AppState| app_state.workers.job.as_ref().is_some_and(|job| job.too_big.is_some());

    // Asked before anything goes - the small file that fits too.
    let items = vec![(source.clone(), stick.join("big.iso"), false), (dir.join("small.txt"), stick.join("small.txt"), false)];
    app_state.start_transfer(items.clone(), true, crate::app::Clashes::Ask);
    wait(&mut app_state, &asked);
    let too_big = app_state.workers.job.as_ref().unwrap().too_big.clone().unwrap();
    assert_eq!((too_big.size, too_big.limit), (25, 10));
    assert!(!stick.join("small.txt").exists());

    // Skipped, the rest goes on.
    app_state.workers.job.as_mut().unwrap().answer(crate::app::Answer::Skip);
    wait(&mut app_state, &|app_state| app_state.workers.job.is_none());
    assert_eq!(app_state.error.take().as_deref(), Some("Copy finished, 1 entry skipped"));
    assert!(stick.join("small.txt").exists() && !crate::fs_ops::piece_path(&stick.join("big.iso"), 1).exists());

    // Split: three pieces of ten bytes at most, and their checksums.
    app_state.start_transfer(items[..1].to_vec(), true, crate::app::Clashes::Ask);
    wait(&mut app_state, &asked);
    app_state.workers.job.as_mut().unwrap().answer(crate::app::Answer::Split);
    wait(&mut app_state, &|app_state| app_state.workers.job.is_none());
    assert_eq!(app_state.error, None);
    let whole = stick.join("big.iso");
    let pieces: Vec<Vec<u8>> = (1..=3).map(|number| std::fs::read(crate::fs_ops::piece_path(&whole, number)).unwrap()).collect();
    assert_eq!(pieces.iter().map(Vec::len).collect::<Vec<_>>(), [10, 10, 5]);
    assert_eq!(pieces.concat(), content);
    assert!(!whole.exists() && !crate::fs_ops::piece_path(&whole, 4).exists());
    let (sums, not_understood) = crate::checksum::parse_sums(&std::fs::read(crate::fs_ops::sums_path(&whole)).unwrap());
    assert_eq!(not_understood, 0);
    assert_eq!(sums.iter().map(|line| line.name.clone()).collect::<Vec<_>>(), ["big.iso.001", "big.iso.002", "big.iso.003"].map(std::path::PathBuf::from));
    // Ctrl+Z takes the pieces back, as a copy's.
    assert!(matches!(app_state.undo.last(), Some(undo) if undo.kind == crate::app::UndoKind::Copy));

    // F5 on the first piece joins them all, back into the whole file.
    let first = crate::fs_ops::piece_path(&whole, 1);
    assert_eq!(crate::fs_ops::pieces_of(&first).map(|pieces| pieces.len()), Some(3));
    let back = dir.join("home").join("big.iso");
    assert!(crate::fs_ops::joining(&first, &back).is_some());
    assert!(crate::fs_ops::joining(&first, &dir.join("home").join("big.iso.001")).is_none(), "to a first piece's name, a copy of it");
    app_state.start_transfer(vec![(first.clone(), back.clone(), false)], true, crate::app::Clashes::Ask);
    assert_eq!(app_state.workers.job.as_ref().unwrap().kind, crate::app::TransferKind::Join);
    wait(&mut app_state, &|app_state| app_state.workers.job.is_none());
    assert_eq!(app_state.error, None);
    assert_eq!(std::fs::read(&back).unwrap(), content);

    // A piece gone bad: the join fails on it, asked about, and leaves
    // nothing behind.
    let mut bad = pieces[1].clone();
    bad[0] ^= 1;
    std::fs::write(crate::fs_ops::piece_path(&whole, 2), bad).unwrap();
    let again = dir.join("home").join("again.iso");
    app_state.start_transfer(vec![(first.clone(), again.clone(), false)], true, crate::app::Clashes::Ask);
    wait(&mut app_state, &|app_state| app_state.workers.job.as_ref().is_some_and(|job| job.problem.is_some()));
    assert_eq!(app_state.workers.job.as_ref().unwrap().problem.as_ref().unwrap().message, "big.iso.002 does not match its checksum");
    app_state.workers.job.as_mut().unwrap().answer(crate::app::Answer::Skip);
    wait(&mut app_state, &|app_state| app_state.workers.job.is_none());
    assert!(!again.exists());
    assert!(std::fs::read_dir(dir.join("home")).unwrap().count() == 1, "no temporary left");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A move split goes once its pieces are there; and a split onto pieces
/// already there writes over them when told to, taking away any left from
/// a split into more.
#[test]
fn a_move_split_takes_the_file_away_and_old_pieces_with_it() {
    let dir = std::env::temp_dir().join(format!("fm84-split-move-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stick = dir.join("stick");
    std::fs::create_dir_all(&stick).unwrap();
    crate::fs_ops::test_limits::set(&stick, 10);
    let source = dir.join("big.bin");
    std::fs::write(&source, vec![7u8; 15]).unwrap();
    let whole = stick.join("big.bin");
    for number in 1..=4 {
        std::fs::write(crate::fs_ops::piece_path(&whole, number), "old").unwrap();
    }

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.start_transfer(vec![(source.clone(), whole.clone(), false)], false, crate::app::Clashes::ReplaceAll);
    let started = std::time::Instant::now();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never finished");
        if let Some(job) = app_state.workers.job.as_mut().filter(|job| job.too_big.is_some()) {
            job.answer(crate::app::Answer::Split);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
    assert_eq!(app_state.error, None);
    assert!(!source.exists(), "moved");
    assert_eq!(std::fs::read(crate::fs_ops::piece_path(&whole, 2)).unwrap(), vec![7u8; 5]);
    assert!(!crate::fs_ops::piece_path(&whole, 3).exists(), "old pieces gone");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A split that writes over nothing leaves alone what comes after its last
/// piece: a `.003` beside two new pieces is not one of an earlier split it
/// was told to write over, and may be anything.
#[test]
fn a_split_onto_nothing_leaves_the_names_after_its_pieces_alone() {
    let dir = std::env::temp_dir().join(format!("fm84-split-alone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stick = dir.join("stick");
    std::fs::create_dir_all(&stick).unwrap();
    crate::fs_ops::test_limits::set(&stick, 10);
    let source = dir.join("big.bin");
    std::fs::write(&source, vec![7u8; 15]).unwrap();
    let whole = stick.join("big.bin");
    let unrelated = crate::fs_ops::piece_path(&whole, 3);
    std::fs::write(&unrelated, "mine").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.start_transfer(vec![(source.clone(), whole.clone(), false)], true, crate::app::Clashes::Ask);
    let started = std::time::Instant::now();
    while app_state.workers.job.is_some() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the job never finished");
        if let Some(job) = app_state.workers.job.as_mut().filter(|job| job.too_big.is_some()) {
            job.answer(crate::app::Answer::Split);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_transfer();
    }
    assert_eq!(app_state.error, None);
    assert!(crate::fs_ops::piece_path(&whole, 2).exists(), "split in two");
    assert_eq!(std::fs::read(&unrelated).unwrap(), b"mine", "nothing was written over: the .003 is left as it was");
    std::fs::remove_dir_all(&dir).unwrap();
}
