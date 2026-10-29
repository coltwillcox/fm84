//! Copies: progress, overwrites, cancels, and what is refused or skipped.

use super::*;

#[test]
fn a_copy_reports_every_file_and_every_byte() {
    let dir = scratch("report");
    let source = dir.join("src");
    fs::create_dir(&source).unwrap();
    // One file longer than a chunk, so the inner loop reports several times.
    fs::write(source.join("big.bin"), vec![7u8; COPY_CHUNK as usize * 2 + 13]).unwrap();
    fs::write(source.join("small.bin"), vec![7u8; 10]).unwrap();
    let dest = dir.join("dst");

    let mut started = Vec::new();
    let mut bytes = 0u64;
    {
        let mut report = |step: Step<'_>| {
            match step {
                Step::Starting(path) => started.push(path.file_name().unwrap().to_string_lossy().into_owned()),
                Step::Advanced(n) => bytes += n,
            }
            true
        };
        assert_eq!(copy_path(source.clone(), dest.clone(), true, &mut report).unwrap(), Transfer::Done);
    }

    started.sort();
    assert_eq!(started, ["big.bin", "small.bin"]);
    // What the bar counts has to reach what the counting pass promised.
    assert_eq!(bytes, measure(&[(source, dest, true)]));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_cancelled_overwrite_leaves_the_old_file_alone() {
    let dir = scratch("overwrite-cancel").canonicalize().unwrap();
    let source = dir.join("new.bin");
    let dest = dir.join("old.bin");
    fs::write(&source, vec![7u8; COPY_CHUNK as usize * 3]).unwrap();
    fs::write(&dest, b"the file that was already here").unwrap();

    // Stop after the first chunk, the way Esc does partway through.
    let mut chunks = 0;
    {
        let mut report = |step: Step<'_>| {
            if let Step::Advanced(_) = step {
                chunks += 1;
            }
            chunks < 1
        };
        assert_eq!(copy_path(source, dest.clone(), false, &mut report).unwrap(), Transfer::Cancelled);
    }

    // Untouched: not truncated, not removed, still the original bytes.
    assert_eq!(fs::read(&dest).unwrap(), b"the file that was already here");
    // And nothing of ours left lying beside it.
    let strays: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).filter(|n| n.to_string_lossy().contains("fm84-")).collect();
    assert!(strays.is_empty(), "left behind {strays:?}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_overwrite_that_finishes_replaces_the_file() {
    let dir = scratch("overwrite-done").canonicalize().unwrap();
    let source = dir.join("new.bin");
    let dest = dir.join("old.bin");
    fs::write(&source, b"replacement").unwrap();
    fs::write(&dest, b"the file that was already here").unwrap();

    let mut report = |_: Step<'_>| true;
    assert_eq!(copy_path(source, dest.clone(), false, &mut report).unwrap(), Transfer::Done);
    assert_eq!(fs::read(&dest).unwrap(), b"replacement");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2, "a temporary was left behind");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cancelling_clears_the_half_written_file() {
    let dir = scratch("cancel");
    let source = dir.join("big.bin");
    let dest = dir.join("copy.bin");
    fs::write(&source, vec![7u8; COPY_CHUNK as usize * 3]).unwrap();

    // Stop after the first chunk, the way Esc does partway through.
    let mut chunks = 0;
    {
        let mut report = |step: Step<'_>| {
            if let Step::Advanced(_) = step {
                chunks += 1;
            }
            chunks < 1
        };
        assert_eq!(copy_path(source, dest.clone(), false, &mut report).unwrap(), Transfer::Cancelled);
    }
    // A fragment left behind would sit there looking like a finished copy.
    assert!(!dest.exists());
    // Nor is the temporary it was written into.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "a temporary was left behind");
    fs::remove_dir_all(&dir).unwrap();
}

/// Until the last byte is in, a new copy is not under its own name: fm84
/// quit or killed partway would otherwise leave half a file passing for
/// a whole one.
#[test]
fn a_copy_under_way_is_not_under_its_own_name() {
    let dir = scratch("copy-temp");
    let source = dir.join("big.bin");
    let dest = dir.join("copy.bin");
    fs::write(&source, vec![7u8; COPY_CHUNK as usize * 2]).unwrap();

    let mut seen_early = None;
    {
        let mut report = |step: Step<'_>| {
            if let Step::Advanced(_) = step {
                seen_early.get_or_insert(dest.exists());
            }
            true
        };
        assert_eq!(copy_path(source.clone(), dest.clone(), false, &mut report).unwrap(), Transfer::Done);
    }
    assert_eq!(seen_early, Some(false));
    assert_eq!(fs::read(&dest).unwrap(), fs::read(&source).unwrap());
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2, "a temporary was left behind");
    fs::remove_dir_all(&dir).unwrap();
}

/// A copy keeps the times of what it came from, files and directories
/// alike. A move across filesystems copies through the same code.
#[test]
fn a_copy_keeps_its_times() {
    let dir = scratch("copy-times");
    let source = dir.join("src");
    fs::create_dir_all(source.join("sub")).unwrap();
    fs::write(source.join("sub").join("old.txt"), "old").unwrap();
    let then = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    let times = fs::FileTimes::new().set_modified(then).set_accessed(then);
    File::options().write(true).open(source.join("sub").join("old.txt")).unwrap().set_times(times).unwrap();
    for directory in [source.join("sub"), source.clone()] {
        if let Ok(handle) = File::open(&directory) {
            let _ = handle.set_times(times);
        }
    }

    let mut report = |_: Step<'_>| true;
    let copied = dir.join("copied");
    assert_eq!(copy_path(source.clone(), copied.clone(), true, &mut report).unwrap(), Transfer::Done);
    let modified = |path: &Path| fs::metadata(path).unwrap().modified().unwrap();
    assert_eq!(modified(&copied.join("sub").join("old.txt")), then);
    // Directories only where the platform lets them be opened to set it.
    if modified(&source.join("sub")) == then {
        assert_eq!(modified(&copied.join("sub")), then);
        assert_eq!(modified(&copied), then);
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_skipped_pipe_leaves_the_rest_of_the_copy_whole() {
    let dir = scratch("skip-pipe");
    let tree = dir.join("tree");
    fs::create_dir_all(tree.join("sub")).unwrap();
    fs::write(tree.join("a.txt"), "a").unwrap();
    fifo(&tree.join("pipe"));
    fs::write(tree.join("sub").join("b.txt"), "b").unwrap();

    let mut progress = Answering::new(Reply::Skip);
    let dest = dir.join("copy");
    assert_eq!(copy_path(tree.clone(), dest.clone(), true, &mut progress).unwrap(), Transfer::Done);
    assert_eq!(progress.asked, [tree.join("pipe")]);
    assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "a");
    assert_eq!(fs::read_to_string(dest.join("sub").join("b.txt")).unwrap(), "b");
    assert!(!path_exists(&dest.join("pipe")));

    // Abort ends it with the error, as a failure always did before.
    let mut progress = Answering::new(Reply::Abort);
    assert!(copy_path(tree, dir.join("aborted"), true, &mut progress).is_err());
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_retry_tries_the_same_entry_again() {
    struct UnlockThenRetry(PathBuf, u32);
    impl Progress for UnlockThenRetry {
        fn step(&mut self, _step: Step<'_>) -> bool {
            true
        }
        fn failed(&mut self, _path: &Path, _error: &Error) -> Reply {
            // What the user would do between seeing the error and pressing R.
            unlock(&self.0);
            self.1 += 1;
            Reply::Retry
        }
    }

    let dir = scratch("retry");
    fs::create_dir(dir.join("locked")).unwrap();
    fs::write(dir.join("locked").join("file"), "").unwrap();
    if locked(&dir.join("locked")).is_none() {
        return fs::remove_dir_all(&dir).unwrap();
    }

    let mut progress = UnlockThenRetry(dir.join("locked"), 0);
    assert_eq!(delete_path(dir.join("locked"), true, &mut progress).unwrap(), Transfer::Done);
    assert_eq!(progress.1, 1);
    assert!(!path_exists(&dir.join("locked")));
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn special_files_are_never_opened() {
    let dir = scratch("fifo");
    let pipe = dir.join("pipe");
    fifo(&pipe);

    assert!(!is_regular_file(&pipe));
    assert!(!is_regular_file(Path::new("/dev/zero")));
    assert!(!is_regular_file(&dir));
    fs::write(dir.join("plain"), "x").unwrap();
    assert!(is_regular_file(&dir.join("plain")));

    assert!(matches!(crate::viewer::load_preview(&pipe, 1024, 10), crate::viewer::Preview::Lines(lines) if lines == ["Not a regular file"]));
    assert!(crate::viewer::load_file_content(&pipe, None).is_err());
    assert!(crate::viewer::load_file_content(Path::new("/dev/zero"), None).is_err());

    // Alone, and inside a directory being copied.
    let mut report = |_: Step<'_>| true;
    assert!(copy_path(pipe.clone(), dir.join("pipe-copy"), false, &mut report).is_err());
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    fifo(&tree.join("pipe"));
    assert!(copy_path(tree, dir.join("tree-copy"), true, &mut report).is_err());

    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn a_failed_copy_leaves_no_fragment() {
    // A regular file that opens, then fails the first read: offset 0 of a
    // process's memory is never mapped.
    let source = Path::new("/proc/self/mem");
    let dir = scratch("failed");
    let dest = dir.join("mem-copy");
    let mut report = |_: Step<'_>| true;
    assert!(copy_path(source.to_path_buf(), dest.clone(), false, &mut report).is_err());
    assert!(!path_exists(&dest));
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_copy_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
    let set = |path: &Path, mode: u32| fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();

    let dir = scratch("modes");
    let source = dir.join("src");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("script"), "#!/bin/sh\n").unwrap();
    set(&source.join("script"), 0o755);
    fs::write(source.join("secret"), "x").unwrap();
    set(&source.join("secret"), 0o600);
    // Read-only, so its files have to be copied in before it gets its mode.
    set(&source, 0o555);

    let dest = dir.join("dst");
    let mut report = |_: Step<'_>| true;
    assert_eq!(copy_path(source.clone(), dest.clone(), true, &mut report).unwrap(), Transfer::Done);
    assert_eq!(mode(&dest.join("script")), 0o755);
    assert_eq!(mode(&dest.join("secret")), 0o600);
    assert_eq!(mode(&dest), 0o555);

    set(&source, 0o755);
    set(&dest, 0o755);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn refusing_the_first_file_copies_nothing() {
    let dir = scratch("refuse");
    let source = dir.join("src");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("a.bin"), vec![1u8; 10]).unwrap();
    let dest = dir.join("dst");

    let mut report = |_: Step<'_>| false;
    assert_eq!(copy_path(source, dest.clone(), true, &mut report).unwrap(), Transfer::Cancelled);
    assert!(!dest.join("a.bin").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_directory_will_not_be_copied_into_itself() {
    let dir = scratch("into-itself").canonicalize().unwrap();
    let source = dir.join("sub");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("f.bin"), vec![1u8; 32]).unwrap();

    // Straight into itself, and deeper inside itself.
    assert!(copies_into_itself(&source, &source.join("sub")));
    assert!(copies_into_itself(&source, &source.join("a").join("b")));

    // Ordinary copies must still go through.
    assert!(!copies_into_itself(&source, &dir.join("elsewhere")));
    assert!(!copies_into_itself(&source, &dir.join("sub2")));
    // Sharing a name prefix is not being inside: "/x/subfoo" is not in "/x/sub".
    assert!(!copies_into_itself(&source, &dir.join("subfoo").join("sub")));
    // A file has nothing to recurse into.
    assert!(!copies_into_itself(&source.join("f.bin"), &source.join("f.bin").join("x")));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[cfg(unix)]
fn a_link_into_the_source_is_caught_and_a_link_source_is_not() {
    let dir = scratch("into-itself-links").canonicalize().unwrap();
    let source = dir.join("sub");
    fs::create_dir(&source).unwrap();

    // A destination that reaches the source through a link is still inside.
    let link = dir.join("link");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    assert!(copies_into_itself(&source, &link.join("copy")));

    // A link is recreated rather than walked, so it cannot recurse and is
    // not refused - even pointing at the directory it is copied into.
    assert!(!copies_into_itself(&link, &source.join("copy")));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_runaway_copy_is_refused_before_anything_is_written() {
    let dir = scratch("runaway").canonicalize().unwrap();
    let source = dir.join("sub");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("f.bin"), vec![9u8; 64]).unwrap();

    let mut report = |_: Step<'_>| true;
    let error = copy_path(source.clone(), source.join("sub"), true, &mut report).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert!(error.to_string().contains("into itself"), "{error}");
    // Nothing started, so nothing was left behind.
    assert!(!source.join("sub").exists());
    assert_eq!(fs::read_dir(&source).unwrap().count(), 1);

    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn overwriting_replaces_a_link_rather_than_writing_through_it() {
    let dir = scratch("through");
    fs::write(dir.join("precious"), "keep me").unwrap();
    fs::write(dir.join("new"), "new").unwrap();
    std::os::unix::fs::symlink(dir.join("precious"), dir.join("link")).unwrap();

    let mut report = |_: Step<'_>| true;
    copy_path(dir.join("new"), dir.join("link"), false, &mut report).unwrap();
    assert_eq!(fs::read_to_string(dir.join("precious")).unwrap(), "keep me");
    assert!(!dir.join("link").symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(dir.join("link")).unwrap(), "new");
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn overwriting_a_read_only_file_replaces_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("readonly");
    fs::write(dir.join("new"), "new").unwrap();
    fs::write(dir.join("locked"), "old").unwrap();
    fs::set_permissions(dir.join("locked"), fs::Permissions::from_mode(0o444)).unwrap();

    let mut report = |_: Step<'_>| true;
    copy_path(dir.join("new"), dir.join("locked"), false, &mut report).unwrap();
    assert_eq!(fs::read_to_string(dir.join("locked")).unwrap(), "new");
    fs::remove_dir_all(&dir).unwrap();
}

/// A name of emoji near the filesystem's limit of 255 bytes still copies.
/// The temporary beside it was cut to 64 characters, which in four-byte
/// emoji is 256 bytes, and with its prefix and suffix it was too long a name
/// to make.
#[test]
fn a_long_name_of_wide_characters_still_copies() {
    let dir = scratch("emoji-name");
    let name = "🦀".repeat(63);
    fs::write(dir.join("src.txt"), "crab").unwrap();
    let dest = dir.join(&name);
    assert_eq!(copy_path(dir.join("src.txt"), dest.clone(), false, &mut |_: Step<'_>| true).unwrap(), Transfer::Done);
    assert_eq!(fs::read_to_string(&dest).unwrap(), "crab");
    fs::remove_dir_all(&dir).unwrap();
}

/// The name a copy kept beside the file in its way is given: numbered
/// before the ending - all of an archive's double one - and on past the
/// numbers already taken.
#[test]
fn a_free_name_is_numbered_before_its_ending() {
    let dir = scratch("free-name");
    let free = |name: &str| free_name(&dir.join(name)).file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(free("notes.txt"), "notes (2).txt");
    assert_eq!(free("src.tar.gz"), "src (2).tar.gz");
    assert_eq!(free("SRC.TAR.XZ"), "SRC (2).TAR.XZ");
    assert_eq!(free(".bashrc"), ".bashrc (2)");
    assert_eq!(free("Makefile"), "Makefile (2)");
    fs::write(dir.join("notes (2).txt"), "").unwrap();
    fs::write(dir.join("notes (3).txt"), "").unwrap();
    assert_eq!(free("notes.txt"), "notes (4).txt");
    fs::remove_dir_all(&dir).unwrap();
}

/// Keep both writes the new one beside what is in the way, which is left
/// as it was: a file, a link, files merged into a directory, and a move -
/// whose source then goes.
#[test]
fn keep_both_writes_beside_what_is_in_the_way() {
    let dir = scratch("keep-both");
    let (from, onto) = (dir.join("from"), dir.join("onto"));
    fs::create_dir_all(from.join("sub")).unwrap();
    fs::create_dir_all(onto.join("sub")).unwrap();
    fs::write(from.join("a.txt"), "new").unwrap();
    fs::write(onto.join("a.txt"), "old").unwrap();
    fs::write(from.join("sub/b.txt"), "new b").unwrap();
    fs::write(onto.join("sub/b.txt"), "old b").unwrap();

    let mut keeping = Clashing::new(Clash::KeepBoth);
    copy_path(from.join("a.txt"), onto.join("a.txt"), false, &mut keeping).unwrap();
    assert_eq!((fs::read_to_string(onto.join("a.txt")).unwrap(), fs::read_to_string(onto.join("a (2).txt")).unwrap()), ("old".into(), "new".into()));

    copy_path(from.join("sub"), onto.join("sub"), true, &mut keeping).unwrap();
    assert_eq!((fs::read_to_string(onto.join("sub/b.txt")).unwrap(), fs::read_to_string(onto.join("sub/b (2).txt")).unwrap()), ("old b".into(), "new b".into()));

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("a.txt", from.join("link")).unwrap();
        fs::write(onto.join("link"), "a file").unwrap();
        copy_path(from.join("link"), onto.join("link"), false, &mut keeping).unwrap();
        assert_eq!(fs::read_link(onto.join("link (2)")).unwrap(), Path::new("a.txt"));
        assert_eq!(fs::read_to_string(onto.join("link")).unwrap(), "a file");
    }

    // A third time: on past the number taken.
    move_path(from.join("a.txt"), onto.join("a.txt"), &mut keeping).unwrap();
    assert_eq!(fs::read_to_string(onto.join("a (3).txt")).unwrap(), "new");
    assert!(!from.join("a.txt").exists());
    assert_eq!(fs::read_to_string(onto.join("a.txt")).unwrap(), "old");
    fs::remove_dir_all(&dir).unwrap();
}
