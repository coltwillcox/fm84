//! Find files (Alt+F7): a walk down from a directory for names matching a
//! pattern, and optionally holding some text, run on a thread of its own so
//! a large tree or a slow disk holds up nothing.

use crate::find::Needle;
use crate::glob::Glob;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// What to look for, and where.
pub struct Query {
    pub start: PathBuf,
    /// Matched against each name as `+` matches: files, or directories with
    /// a `/` on the end.
    pub name: Glob,
    /// Text the file has to hold, smart case as Ctrl+F is. Directories never
    /// hold any, so with this set only files are found.
    pub containing: Option<Needle>,
    /// Hidden entries are passed over unless the panels show them, since the
    /// panel could not put its cursor on one found there.
    pub show_hidden: bool,
    /// Names are put in order as the panels put them: ignoring case, unless
    /// F11 says otherwise.
    pub case_sensitive: bool,
}

/// What the walk reports as it goes.
pub enum Update {
    /// Looking through this directory now. Sent only now and then: a walk
    /// goes through directories far faster than anyone could read them.
    Looking(PathBuf),
    /// A match, and whether it is a directory.
    Found(PathBuf, bool),
    /// The walk is over: how many directories could not be read, which are
    /// counted rather than stopping it, as du carries on past them.
    Finished { unreadable: usize },
}

/// Bytes of a file read at a time when looking for text in it, so a large
/// file is never held whole.
const CHUNK: usize = 64 * 1024;

/// Walk down from `query.start`, in name order, reporting each match as it
/// is found - each directory's contents straight after it, as `find` lists a
/// tree. Links to directories are not followed: one pointing back up the tree
/// would never end. Nor are the system's own trees gone into, unless the
/// search started in one - see is_system_tree. Stops when `cancel` is set, or when `send` returns false
/// because nobody is listening any more.
pub fn search(query: &Query, cancel: &AtomicBool, send: &mut dyn FnMut(Update) -> bool) {
    let mut unreadable = 0;
    let mut reported = Instant::now();
    // The entries still to come in each directory on the way down, the
    // deepest last. Kept here rather than in the call stack, so a tree
    // thousands deep cannot run it out.
    let mut levels: Vec<std::vec::IntoIter<fs::DirEntry>> = Vec::new();
    let mut entering = Some(query.start.clone());
    loop {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if let Some(dir) = entering.take() {
            if reported.elapsed() >= crate::constants::SIZE_PROGRESS_INTERVAL {
                reported = Instant::now();
                if !send(Update::Looking(dir.clone())) {
                    return;
                }
            }
            match fs::read_dir(&dir) {
                Ok(entries) => {
                    let mut entries: Vec<fs::DirEntry> = entries.flatten().collect();
                    // As the panels order them, so a result is where the
                    // panel would have it - not byte by byte, which put every
                    // capital first. The name itself settles a tie.
                    let case_sensitive = query.case_sensitive;
                    entries.sort_by_cached_key(|entry| {
                        let name = entry.file_name();
                        let shown = name.to_string_lossy().into_owned();
                        (if case_sensitive { shown } else { shown.to_lowercase() }, name)
                    });
                    levels.push(entries.into_iter());
                }
                Err(_) => unreadable += 1,
            }
        }
        let Some(level) = levels.last_mut() else {
            break;
        };
        let Some(entry) = level.next() else {
            levels.pop();
            continue;
        };

        let name = entry.file_name().to_string_lossy().into_owned();
        let hidden_by = if cfg!(windows) { entry.metadata().ok() } else { None };
        if !query.show_hidden && crate::fs_ops::is_hidden(&name, hidden_by.as_ref()) {
            continue;
        }
        // The entry itself: a link to a directory is a link here.
        let is_dir = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
        let path = entry.path();
        let found = query.name.matches(&name, is_dir)
            && match &query.containing {
                None => true,
                Some(needle) => !is_dir && holds(&path, needle, cancel),
            };
        if found && !send(Update::Found(path.clone(), is_dir)) {
            return;
        }
        if is_dir && (!is_system_tree(&path) || is_system_tree(&query.start)) {
            entering = Some(path);
        }
    }
    send(Update::Finished { unreadable });
}

/// The trees the system keeps for its own machinery rather than for files:
/// the kernel's views of processes and devices under /proc, /sys and /dev, and
/// runtime state under /run - credentials, portals and gvfs. Their entries
/// pass for files and are not: reading some never returns - a USB-C port's
/// state under /sys was found waiting for good - and the rest say nothing a
/// search for files is after. /run/media is the exception, where removable
/// media are mounted. The drive strip leaves out the same trees.
fn is_system_tree(path: &Path) -> bool {
    const MACHINERY: [&str; 4] = ["/proc", "/sys", "/dev", "/run"];
    MACHINERY.iter().any(|root| path.starts_with(root)) && !path.starts_with("/run/media")
}

/// Whether the file at `path` holds what `needle` looks for. Read a chunk at
/// a time, each carrying on from the tail of the last so that a match split
/// across two chunks is still found. A pipe or a device is never opened: one
/// would wait for good, or never end. Anything that cannot be read holds
/// nothing.
fn holds(path: &Path, needle: &Needle, cancel: &AtomicBool) -> bool {
    if !crate::fs_ops::is_regular_file(path) {
        return false;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    // Enough of the last chunk kept to hold the longest match, however many
    // bytes its characters take.
    let overlap = needle.len() * 4;
    let mut carried: Vec<u8> = Vec::new();
    let mut chunk = vec![0; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let read = match file.read(&mut chunk) {
            Ok(0) | Err(_) => return false,
            Ok(read) => read,
        };
        carried.extend_from_slice(&chunk[..read]);
        if !needle.find_all(&String::from_utf8_lossy(&carried)).is_empty() {
            return true;
        }
        let keep = carried.len().saturating_sub(overlap);
        carried.drain(..keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(query: &Query) -> (Vec<String>, usize) {
        let mut found = Vec::new();
        let mut unreadable = None;
        search(query, &AtomicBool::new(false), &mut |update| {
            match update {
                Update::Found(path, is_dir) => {
                    let shown = path.strip_prefix(&query.start).unwrap().to_string_lossy().into_owned();
                    found.push(if is_dir { format!("{shown}/") } else { shown });
                }
                Update::Finished { unreadable: count } => unreadable = Some(count),
                Update::Looking(_) => {}
            }
            true
        });
        (found, unreadable.expect("the walk never said it was over"))
    }

    /// A small tree to search, under a name of the test's own: the tests run
    /// at once, and one's pipe must not turn up in another's results.
    fn tree(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fm84-search-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src").join("deep")).unwrap();
        fs::create_dir_all(dir.join(".hidden")).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"fm84\"\n").unwrap();
        fs::write(dir.join("src").join("main.rs"), "fn main() {\n    run();\n}\n").unwrap();
        fs::write(dir.join("src").join("deep").join("walk.rs"), "// Run it.\n").unwrap();
        fs::write(dir.join(".hidden").join("secret.rs"), "fn run() {}\n").unwrap();
        // A match written across the boundary between two chunks.
        let mut big = vec![b'x'; CHUNK - 3];
        big.extend_from_slice(b"needle");
        fs::write(dir.join("src").join("big.txt"), big).unwrap();
        dir
    }

    fn query(dir: &Path, name: &str, containing: &str) -> Query {
        Query {
            start: dir.to_path_buf(),
            name: Glob::new(name).unwrap(),
            containing: Needle::new(containing),
            show_hidden: false,
            case_sensitive: false,
        }
    }

    #[test]
    fn names_are_found_in_tree_order_and_hidden_ones_left_out() {
        let dir = tree("names");
        let (found, unreadable) = run(&query(&dir, "*.rs", ""));
        assert_eq!(found, ["src/deep/walk.rs", "src/main.rs"]);
        assert_eq!(unreadable, 0);

        // Directories with a / on the end, and hidden ones when shown.
        let (found, _) = run(&Query { show_hidden: true, ..query(&dir, "*/", "") });
        assert_eq!(found, [".hidden/", "src/", "src/deep/"]);
        let (found, _) = run(&Query { show_hidden: true, ..query(&dir, "*.rs", "") });
        assert_eq!(found, [".hidden/secret.rs", "src/deep/walk.rs", "src/main.rs"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn text_is_found_in_any_case_unless_a_capital_says_otherwise() {
        let dir = tree("text");
        // "run" matches "run();" and "Run it.": lower case is either case.
        assert_eq!(run(&query(&dir, "*", "run")).0, ["src/deep/walk.rs", "src/main.rs"]);
        assert_eq!(run(&query(&dir, "*", "Run")).0, ["src/deep/walk.rs"]);
        // Across the boundary between two chunks of a large file.
        assert_eq!(run(&query(&dir, "*", "needle")).0, ["src/big.txt"]);
        // With text to find, a directory is never found.
        assert!(run(&query(&dir, "*/", "fn")).0.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn what_cannot_be_read_is_counted_and_pipes_are_never_opened() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tree("unreadable");
        let pipe = std::ffi::CString::new(dir.join("pipe.rs").to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
        fs::create_dir(dir.join("locked")).unwrap();
        fs::set_permissions(dir.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();

        // Would hang on the pipe, were it opened.
        let (found, unreadable) = run(&query(&dir, "*", "fn"));
        assert_eq!(found, ["src/main.rs"]);
        // Root reads anything, so there is nothing to count.
        if fs::read_dir(dir.join("locked")).is_err() {
            assert_eq!(unreadable, 1);
        }
        fs::set_permissions(dir.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_systems_own_trees_are_left_out_unless_started_in() {
        for path in ["/proc", "/sys/class/net", "/dev/sda", "/run/user/1000/gvfs"] {
            assert!(is_system_tree(Path::new(path)), "{path}");
        }
        // Component by component, so names merely starting the same are not.
        for path in ["/", "/home/sys", "/process", "/devices", "/run/media/me/stick", "/running"] {
            assert!(!is_system_tree(Path::new(path)), "{path}");
        }
        // Started in one, it is searched: here /proc/self, where the walk
        // has to go down into task/ to find a status file inside.
        #[cfg(target_os = "linux")]
        {
            let query = Query {
                start: "/proc/self".into(),
                name: Glob::new("status").unwrap(),
                containing: None,
                show_hidden: true,
                case_sensitive: false,
            };
            let mut found = Vec::new();
            search(&query, &AtomicBool::new(false), &mut |update| {
                if let Update::Found(path, _) = update {
                    found.push(path);
                }
                true
            });
            assert!(found.contains(&PathBuf::from("/proc/self/status")));
            assert!(found.iter().any(|path| path.starts_with("/proc/self/task")), "{found:?}");
        }
    }

    #[test]
    fn results_come_in_the_order_the_panels_list_them() {
        let dir = std::env::temp_dir().join(format!("fm84-search-order-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for name in ["README", "app.rs", "Zed", "build.rs"] {
            fs::write(dir.join(name), "").unwrap();
        }
        // Either case together, as the panels have it by default.
        assert_eq!(run(&query(&dir, "*", "")).0, ["app.rs", "build.rs", "README", "Zed"]);
        // And capitals first when F11 sorts by case.
        assert_eq!(run(&Query { case_sensitive: true, ..query(&dir, "*", "") }).0, ["README", "Zed", "app.rs", "build.rs"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_cancelled_walk_stops() {
        let dir = tree("cancel");
        let mut updates = 0;
        search(&query(&dir, "*", ""), &AtomicBool::new(true), &mut |_| {
            updates += 1;
            true
        });
        assert_eq!(updates, 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
