//! Whether there is the room for a copy, looked for before it starts.

use super::*;

/// More than any disk holds is short; a byte is not. What is short is said
/// with the directory written into, and what goes to one filesystem is
/// added up, wherever on it each goes.
#[test]
fn a_write_larger_than_the_disk_is_short_of_room() {
    let dir = scratch("room");
    let Some(free) = free_space(&dir) else {
        return; // A platform that will not say has nothing to check.
    };
    assert_eq!(short_of_room(&[(dir.join("small"), 1)]), None);

    // Well past it: other tests come and go on the same disk meanwhile.
    let too_much = free.saturating_add(1 << 30);
    let short = short_of_room(&[(dir.join("big"), too_much)]).expect("short");
    assert_eq!(short.dir, dir);
    assert_eq!(short.needed, too_much);

    // Two halves on one filesystem are the whole, and somewhere not yet
    // made is on the filesystem of the directory it would go into.
    fs::create_dir(dir.join("sub")).unwrap();
    let half = too_much / 2 + 1;
    let short = short_of_room(&[(dir.join("a"), half), (dir.join("sub").join("not").join("yet"), half)]).expect("short, added up");
    assert_eq!(short.needed, half * 2);

    // Nothing to write needs no room, however little there is.
    assert_eq!(short_of_room(&[(dir.join("empty"), 0)]), None);
    fs::remove_dir_all(&dir).unwrap();
}

/// A move within one filesystem renames, and writes nothing; a copy writes
/// all of it.
#[test]
fn a_move_on_one_filesystem_needs_no_room() {
    let dir = scratch("room-move");
    fs::write(dir.join("a"), "abc").unwrap();
    let items = [(dir.join("a"), dir.join("b"), false)];
    assert_eq!(writes(&items, &[3], false), [(dir.join("b"), 0)]);
    assert_eq!(writes(&items, &[3], true), [(dir.join("b"), 3)]);
    fs::remove_dir_all(&dir).unwrap();
}
