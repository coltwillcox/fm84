//! Deletes, for good and to the trash.

use super::*;

/// The trash is found through XDG_DATA_HOME, so pointed at a scratch
/// directory the test trashes nothing of anyone's. Set for a child run of
/// this same test rather than with set_var: the variable belongs to the
/// whole process, where every other test is running at the same time.
#[cfg(target_os = "linux")]
#[test]
fn trashing_moves_it_away_and_what_will_not_go_can_be_deleted() {
    const ROOT: &str = "FM84_TRASH_TEST_ROOT";
    let Some(root) = std::env::var_os(ROOT).map(PathBuf::from) else {
        let root = std::env::temp_dir().join(format!("fm84-trash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        // This test's name as the harness knows it, from where it sits, so a
        // move to another module does not leave the child running nothing.
        let module = module_path!().split_once("::").map_or("", |(_, module)| module);
        let name = format!("{module}::trashing_moves_it_away_and_what_will_not_go_can_be_deleted");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture"])
            .env(ROOT, &root)
            .env("XDG_DATA_HOME", root.join("data"))
            .output()
            .unwrap();
        let _ = fs::remove_dir_all(&root);
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success() && said.contains("1 passed"), "the child run failed:\n{said}");
        return;
    };
    let data = root.join("data");

    // A file and a directory with something in it go, and are in the trash.
    let file = root.join("note.txt");
    fs::write(&file, "kept").unwrap();
    let dir = root.join("folder");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("inside.txt"), "").unwrap();
    let mut answering = Answering::new(Reply::Abort);
    assert_eq!(trash_path(file.clone(), false, &mut answering).unwrap(), Transfer::Done);
    assert_eq!(trash_path(dir.clone(), true, &mut answering).unwrap(), Transfer::Done);
    assert!(!file.exists() && !dir.exists());
    assert_eq!(fs::read_to_string(data.join("Trash/files/note.txt")).unwrap(), "kept");
    assert!(data.join("Trash/files/folder/inside.txt").exists());
    assert!(data.join("Trash/info/note.txt.trashinfo").exists());
    assert!(answering.asked.is_empty());

    // A trash that cannot be made takes nothing. Its directory is a file
    // now, where it was one to make before.
    fs::remove_dir_all(&data).unwrap();
    fs::write(&data, "").unwrap();
    let other = root.join("other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("inside.txt"), "").unwrap();

    // Skipped, it stays; asked to delete instead, it goes for good.
    let mut skipping = Answering::new(Reply::Skip);
    assert_eq!(trash_path(other.clone(), true, &mut skipping).unwrap(), Transfer::Done);
    assert!(other.exists());
    assert_eq!(skipping.asked, std::slice::from_ref(&other));
    let mut deleting = Answering::new(Reply::Delete);
    assert_eq!(trash_path(other.clone(), true, &mut deleting).unwrap(), Transfer::Done);
    assert!(!other.exists());
}

/// The delete inside a trash job counts nothing of its own: the item it
/// stands for is counted once, by the trash.
#[test]
fn a_delete_inside_another_step_counts_nothing() {
    let dir = std::env::temp_dir().join(format!("fm84-uncounted-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("a"), "").unwrap();

    let mut counted = 0;
    let mut started = 0;
    let mut report = |step: Step<'_>| {
        match step {
            Step::Advanced(amount) => counted += amount,
            Step::Starting(_) => started += 1,
        }
        true
    };
    let mut uncounted = Uncounted(&mut report);
    assert_eq!(delete_path(dir.clone(), true, &mut uncounted).unwrap(), Transfer::Done);
    assert!(!dir.exists());
    assert_eq!(counted, 0);
    // Still named as it goes, so the popup shows what is being deleted.
    assert_eq!(started, 3);
}

#[cfg(unix)]
#[test]
fn a_skipped_delete_keeps_only_what_holds_it() {
    let dir = scratch("skip-delete");
    let tree = dir.join("tree");
    fs::create_dir_all(tree.join("locked")).unwrap();
    fs::create_dir_all(tree.join("open")).unwrap();
    fs::write(tree.join("locked").join("stuck.txt"), "").unwrap();
    fs::write(tree.join("open").join("free.txt"), "").unwrap();
    fs::write(tree.join("top.txt"), "").unwrap();
    if locked(&tree.join("locked")).is_none() {
        return fs::remove_dir_all(&dir).unwrap();
    }

    let mut progress = Answering::new(Reply::Skip);
    assert_eq!(delete_path(tree.clone(), true, &mut progress).unwrap(), Transfer::Done);
    // Asked about the one file, and not again about the directories that
    // cannot go because it is still in them.
    assert_eq!(progress.asked, [tree.join("locked").join("stuck.txt")]);
    assert!(path_exists(&tree.join("locked").join("stuck.txt")));
    assert!(!path_exists(&tree.join("open")));
    assert!(!path_exists(&tree.join("top.txt")));

    unlock(&tree.join("locked"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_delete_counts_and_removes_every_entry() {
    let dir = scratch("delete").canonicalize().unwrap();
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join("a.bin"), b"a").unwrap();
    fs::create_dir(tree.join("inner")).unwrap();
    fs::write(tree.join("inner").join("b.bin"), b"b").unwrap();

    // tree, a.bin, inner, b.bin. Counted before the delete, since counting
    // walks the live filesystem and there is nothing left to walk after.
    let total = count_entries(&[(tree.clone(), true)]);
    assert_eq!(total, 4);

    let mut removed = 0u64;
    let mut named = Vec::new();
    {
        let mut report = |step: Step<'_>| {
            match step {
                Step::Starting(path) => named.push(path.file_name().unwrap().to_string_lossy().into_owned()),
                Step::Advanced(n) => removed += n,
            }
            true
        };
        assert_eq!(delete_path(tree.clone(), true, &mut report).unwrap(), Transfer::Done);
    }

    assert!(!tree.exists());
    // What the bar counts has to reach what the counting pass promised.
    assert_eq!(removed, total);
    named.sort();
    assert_eq!(named, ["a.bin", "b.bin", "inner", "tree"]);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cancelling_a_delete_leaves_the_rest_alone() {
    let dir = scratch("delete-cancel").canonicalize().unwrap();
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    for index in 0..8 {
        fs::write(tree.join(format!("f{index}.bin")), b"x").unwrap();
    }

    // Stop after the first entry, the way Esc does partway through.
    let mut removed = 0;
    {
        let mut report = |step: Step<'_>| {
            if let Step::Advanced(_) = step {
                removed += 1;
            }
            removed < 1
        };
        assert_eq!(delete_path(tree.clone(), true, &mut report).unwrap(), Transfer::Cancelled);
    }

    // The tree is still there, minus what had already gone.
    assert!(tree.exists());
    assert_eq!(fs::read_dir(&tree).unwrap().count(), 7);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[cfg(unix)]
fn deleting_a_link_to_a_directory_spares_what_it_points_at() {
    let dir = scratch("delete-link").canonicalize().unwrap();
    let target = dir.join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep.bin"), b"keep").unwrap();

    // The panel reports a link to a directory as a directory, so the delete
    // arrives with is_dir set and must still only unlink it.
    let link = dir.join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_eq!(count_entries(&[(link.clone(), true)]), 1);

    let mut report = |_: Step<'_>| true;
    assert_eq!(delete_path(link.clone(), true, &mut report).unwrap(), Transfer::Done);

    assert!(!link.exists());
    assert!(target.join("keep.bin").exists(), "the link's target was followed");

    fs::remove_dir_all(&dir).unwrap();
}
