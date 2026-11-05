//! Find files (Alt+F7): a walk down from a directory for names matching a
//! pattern, and optionally holding some text or a regular expression, of a
//! size or modified when asked, run on a thread of its own so a large tree
//! or a slow disk holds up nothing.

use crate::find::Needle;
use crate::glob::Glob;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// What to look for, and where.
pub struct Query {
    pub start: PathBuf,
    /// Matched against each name as `+` matches: files, or directories with
    /// a `/` on the end.
    pub name: Glob,
    /// What the file has to hold. Directories never hold any, so with this
    /// set only files are found.
    pub containing: Option<Text>,
    /// How big a file has to be. Directories have no size of their own, so
    /// with this set only files are found.
    pub size: Option<SizeRange>,
    /// When it was last modified, a file or a directory.
    pub modified: Option<TimeRange>,
    /// Hidden entries are passed over unless the panels show them, since the
    /// panel could not put its cursor on one found there.
    pub show_hidden: bool,
    /// Names are put in order as the panels put them: ignoring case, unless
    /// F11 says otherwise.
    pub case_sensitive: bool,
}

/// Text to look for in a file.
pub enum Text {
    /// As typed, smart case as Ctrl+F is.
    Plain(Needle),
    /// A regular expression, smart case too.
    Regex(regex::bytes::Regex),
}

impl Text {
    /// What is typed into Containing, read as text or as a regular
    /// expression. None for nothing typed; an error for an expression that
    /// is not one, in its own words.
    pub fn new(typed: &str, regex: bool) -> Result<Option<Self>, String> {
        if typed.is_empty() {
            return Ok(None);
        }
        if !regex {
            return Ok(Needle::new(typed).map(Text::Plain));
        }
        regex::bytes::RegexBuilder::new(typed)
            .case_insensitive(!crate::find::has_capital(typed, true))
            .multi_line(true)
            .size_limit(1 << 24)
            .build()
            .map(|regex| Some(Text::Regex(regex)))
            .map_err(|e| match e {
                regex::Error::Syntax(said) => format!("Not a regular expression: {}", said.lines().last().unwrap_or(&said).trim_start_matches("error: ")),
                other => format!("Not a regular expression: {other}"),
            })
    }
}

/// Sizes in bytes a file may have, from and to, both ends included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeRange {
    pub min: u64,
    pub max: u64,
}

impl SizeRange {
    /// `>10M`, `<1K`, `>=`, `<=`, or `1M..10M` - both ends included. Units
    /// K, M, G and T, in powers of 1024, with or without a B, and a bare
    /// number is bytes; `1.5G` too. None for nothing typed.
    pub fn parse(text: &str) -> Result<Option<Self>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let wrong = || format!("Size as >10M, <1K or 1M..10M, not {text}");
        let range = if let Some((from, to)) = text.split_once("..") {
            let (from, to) = (bytes(from).ok_or_else(wrong)?, bytes(to).ok_or_else(wrong)?);
            SizeRange { min: from.min(to), max: from.max(to) }
        } else if let Some(at) = text.strip_prefix(">=") {
            SizeRange {
                min: bytes(at).ok_or_else(wrong)?,
                max: u64::MAX,
            }
        } else if let Some(at) = text.strip_prefix("<=") {
            SizeRange { min: 0, max: bytes(at).ok_or_else(wrong)? }
        } else if let Some(at) = text.strip_prefix('>') {
            SizeRange {
                min: bytes(at).ok_or_else(wrong)?.saturating_add(1),
                max: u64::MAX,
            }
        } else if let Some(at) = text.strip_prefix('<') {
            let at = bytes(at).ok_or_else(wrong)?;
            if at == 0 {
                return Err("Nothing is smaller than 0 bytes".to_string());
            }
            SizeRange { min: 0, max: at - 1 }
        } else {
            return Err(wrong());
        };
        Ok(Some(range))
    }

    fn holds(&self, size: u64) -> bool {
        (self.min..=self.max).contains(&size)
    }
}

/// `10M`, `1.5 GiB`, `200` as bytes.
fn bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let digits = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let (number, unit) = text.split_at(digits);
    let number: f64 = number.parse().ok()?;
    let unit = unit.trim().to_ascii_lowercase();
    let power = match unit.trim_end_matches('b').trim_end_matches('i') {
        "" => 0,
        "k" => 1,
        "m" => 2,
        "g" => 3,
        "t" => 4,
        _ => return None,
    };
    // "b" alone is bytes; "ib" alone is nothing.
    if unit == "ib" {
        return None;
    }
    let value = number * 1024f64.powi(power);
    (value.is_finite() && value < u64::MAX as f64).then_some(value.round() as u64)
}

/// When a file may have been modified: from, included, and until, not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    pub after: Option<SystemTime>,
    pub before: Option<SystemTime>,
}

impl TimeRange {
    /// An age - `<7d` in the last seven days, `>1y` longer ago than a year,
    /// `1d..7d` between - in `min`, `h`, `d`, `w`, `mo` and `y`; or a date,
    /// `2026-10-01` that day, `>2026-10-01` after it, `<` before it, `>=`
    /// and `<=` it included, and `2026-10-01..2026-10-31` from the one to
    /// the other, both days whole. None for nothing typed.
    pub fn parse(text: &str, now: chrono::DateTime<chrono::Local>) -> Result<Option<Self>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let wrong = || format!("Modified as <7d, >1y, 1d..7d, 2026-10-01 or >2026-10-01, not {text}");
        let at = |age: Duration| SystemTime::from(now) - age;
        let range = |after, before| Ok(Some(TimeRange { after, before }));
        // A day's start, and the next day's.
        let day = |text: &str| -> Option<(SystemTime, SystemTime)> {
            let date = chrono::NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()?;
            let start = |date: chrono::NaiveDate| date.and_hms_opt(0, 0, 0)?.and_local_timezone(chrono::Local).earliest().map(SystemTime::from);
            Some((start(date)?, start(date.succ_opt()?)?))
        };
        if let Some((from, to)) = text.split_once("..") {
            if let (Some(from), Some(to)) = (day(from), day(to)) {
                let (first, last) = if from.0 <= to.0 { (from, to) } else { (to, from) };
                return range(Some(first.0), Some(last.1));
            }
            let (from, to) = (age(from).ok_or_else(wrong)?, age(to).ok_or_else(wrong)?);
            return range(Some(at(from.max(to))), Some(at(from.min(to))));
        }
        // The two-character ones first, or >= would read as > and "=1d".
        for operator in [">=", "<=", ">", "<"] {
            let Some(rest) = text.strip_prefix(operator) else {
                continue;
            };
            if let Some((start, end)) = day(rest) {
                return match operator {
                    ">" => range(Some(end), None),
                    ">=" => range(Some(start), None),
                    "<" => range(None, Some(start)),
                    _ => range(None, Some(end)),
                };
            }
            let age = age(rest).ok_or_else(wrong)?;
            // Less than a week old is newer than a week ago.
            return if operator.starts_with('<') { range(Some(at(age)), None) } else { range(None, Some(at(age))) };
        }
        match day(text) {
            Some((start, end)) => range(Some(start), Some(end)),
            None => Err(wrong()),
        }
    }

    fn holds(&self, time: SystemTime) -> bool {
        self.after.is_none_or(|after| time >= after) && self.before.is_none_or(|before| time < before)
    }
}

/// `7d`, `12h`, `2 w`, `30min`, `6mo`, `1y` as a span of time.
fn age(text: &str) -> Option<Duration> {
    let text = text.trim();
    let digits = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let (number, unit) = text.split_at(digits);
    let number: f64 = number.parse().ok()?;
    let seconds = match unit.trim().to_ascii_lowercase().as_str() {
        "min" | "mins" | "minute" | "minutes" => 60.0,
        "h" | "hour" | "hours" => 3600.0,
        "d" | "day" | "days" => 86400.0,
        "w" | "week" | "weeks" => 7.0 * 86400.0,
        "mo" | "month" | "months" => 30.0 * 86400.0,
        "y" | "year" | "years" => 365.0 * 86400.0,
        _ => return None,
    };
    Duration::try_from_secs_f64(number * seconds).ok()
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
            && fits(query, &entry, &path, is_dir)
            && match &query.containing {
                None => true,
                Some(text) => !is_dir && holds(&path, text, cancel),
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

/// Whether an entry is of the size and the time asked for. Of a link, what
/// it points at, as the panels show it; one that points at nothing, the
/// link itself. Only looked up when asked for, which a walk of a large
/// tree would otherwise pay for at every entry.
fn fits(query: &Query, entry: &fs::DirEntry, path: &Path, is_dir: bool) -> bool {
    if query.size.is_none() && query.modified.is_none() {
        return true;
    }
    if query.size.is_some() && is_dir {
        return false;
    }
    let Ok(metadata) = fs::metadata(path).or_else(|_| entry.metadata()) else {
        return false;
    };
    // A link to a directory has no size of its own either.
    if query.size.is_some() && metadata.is_dir() {
        return false;
    }
    query.size.is_none_or(|size| size.holds(metadata.len())) && query.modified.is_none_or(|modified| metadata.modified().is_ok_and(|time| modified.holds(time)))
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

/// Bytes of the last chunk a regular expression looks at again with the
/// next, for a match across the two. One longer than this, split between
/// them, is not found.
const REGEX_OVERLAP: usize = 4096;

/// Whether the file at `path` holds what `text` looks for. Read a chunk at
/// a time, each carrying on from the tail of the last so that a match split
/// across two chunks is still found. A pipe or a device is never opened: one
/// would wait for good, or never end. Anything that cannot be read holds
/// nothing.
fn holds(path: &Path, text: &Text, cancel: &AtomicBool) -> bool {
    if !crate::fs_ops::is_regular_file(path) {
        return false;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    // Enough of the last chunk kept to hold the longest match, however many
    // bytes its characters take.
    let overlap = match text {
        Text::Plain(needle) => needle.len() * 4,
        Text::Regex(_) => REGEX_OVERLAP,
    };
    let mut carried: Vec<u8> = Vec::new();
    let mut chunk = vec![0; CHUNK];
    // A match that ran to the end of what was read, waiting on what comes
    // next: `$` and `\b` match at the end of the bytes searched, which is
    // the end of the file only once nothing more can be read.
    let mut waiting = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let read = match file.read(&mut chunk) {
            Ok(0) => return waiting,
            Err(_) => return false,
            Ok(read) => read,
        };
        carried.extend_from_slice(&chunk[..read]);
        let found = match text {
            Text::Plain(needle) => !needle.find_all(&String::from_utf8_lossy(&carried)).is_empty(),
            Text::Regex(regex) => {
                waiting = false;
                regex.find_iter(&carried).any(|found| {
                    waiting = found.end() == carried.len();
                    !waiting
                })
            }
        };
        if found {
            return true;
        }
        let mut keep = carried.len().saturating_sub(overlap);
        // From the start of a line, where one can be found, so that ^ never
        // matches partway along one.
        if matches!(text, Text::Regex(_))
            && let Some(newline) = carried[keep..].iter().position(|&byte| byte == b'\n')
        {
            keep += newline + 1;
        }
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
            containing: Text::new(containing, false).unwrap(),
            size: None,
            modified: None,
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
                size: None,
                modified: None,
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
    fn a_regular_expression_is_looked_for_smart_case() {
        let dir = tree("regex");
        let regex = |typed: &str| Query {
            containing: Text::new(typed, true).unwrap(),
            ..query(&dir, "*", "")
        };
        assert_eq!(run(&regex(r"fn \w+\(\)")).0, ["src/main.rs"]);
        // Lower case is either case; \W is a class, not a capital.
        assert_eq!(run(&regex(r"^// run\W")).0, ["src/deep/walk.rs"]);
        assert!(run(&regex(r"^// RUN")).0.is_empty());
        // ^ is the start of a line, wherever the file was cut into chunks.
        assert_eq!(run(&regex(r"^ +run\(")).0, ["src/main.rs"]);
        assert!(run(&regex(r"^needle")).0.is_empty());
        assert_eq!(run(&regex(r"x{3}needle$")).0, ["src/big.txt"]);
        // Nor is the end of a chunk the end of a line, or of a word.
        let cut = dir.join("cut.txt");
        fs::write(&cut, format!("{}foobar\nend", "x".repeat(CHUNK - 3))).unwrap();
        let held = |typed: &str| holds(&cut, &Text::new(typed, true).unwrap().unwrap(), &AtomicBool::new(false));
        assert!(!held(r"foo$") && !held(r"foo\b"), "matched where the chunk was cut");
        assert!(held(r"foobar$") && held(r"^end$") && held(r"d\b"));
        // One that is not an expression says why.
        let wrong = Text::new("fn (", true).err().unwrap();
        assert!(wrong.starts_with("Not a regular expression"), "{wrong}");
        assert!(Text::new("", true).unwrap().is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sizes_read_with_their_units_and_both_ends_included() {
        let parse = |text: &str| SizeRange::parse(text).unwrap().map(|range| (range.min, range.max));
        assert_eq!(parse(""), None);
        assert_eq!(parse(">10M"), Some((10 * 1024 * 1024 + 1, u64::MAX)));
        assert_eq!(parse(">= 10 MiB"), Some((10 * 1024 * 1024, u64::MAX)));
        assert_eq!(parse("<1k"), Some((0, 1023)));
        assert_eq!(parse("<=1KB"), Some((0, 1024)));
        assert_eq!(parse("1.5G..1M"), Some((1024 * 1024, 1536 * 1024 * 1024)));
        assert_eq!(parse(">200"), Some((201, u64::MAX)));
        for wrong in ["10M", ">ten", ">10Q", "<0", ">5iB"] {
            assert!(SizeRange::parse(wrong).is_err(), "{wrong}");
        }

        let dir = tree("size");
        // Directories have no size of their own, and are left out.
        let sized = |text: &str| Query {
            size: SizeRange::parse(text).unwrap(),
            ..query(&dir, "*", "")
        };
        assert_eq!(run(&sized(">60K")).0, ["src/big.txt"]);
        assert_eq!(run(&sized("<1K")).0, ["Cargo.toml", "src/deep/walk.rs", "src/main.rs"]);
        // Nor has a link to one, which is listed as what it points at.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("src"), dir.join("to-src")).unwrap();
            assert_eq!(run(&sized("<1M")).0, ["Cargo.toml", "src/big.txt", "src/deep/walk.rs", "src/main.rs"]);
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn times_read_as_ages_or_as_days() {
        use chrono::TimeZone;
        let now = chrono::Local.with_ymd_and_hms(2026, 11, 5, 12, 0, 0).unwrap();
        let at = |y, m, d, h| SystemTime::from(chrono::Local.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap());
        let parse = |text: &str| TimeRange::parse(text, now).unwrap().unwrap();
        let week = Duration::from_secs(7 * 86400);
        assert_eq!(
            parse("<7d"),
            TimeRange {
                after: Some(SystemTime::from(now) - week),
                before: None
            }
        );
        assert_eq!(
            parse(">1w"),
            TimeRange {
                after: None,
                before: Some(SystemTime::from(now) - week)
            }
        );
        assert_eq!(
            parse("7d..1d"),
            TimeRange {
                after: Some(SystemTime::from(now) - week),
                before: Some(SystemTime::from(now) - Duration::from_secs(86400))
            }
        );
        // A day is the whole of it.
        assert_eq!(
            parse("2026-10-01"),
            TimeRange {
                after: Some(at(2026, 10, 1, 0)),
                before: Some(at(2026, 10, 2, 0))
            }
        );
        assert_eq!(parse(">2026-10-01"), TimeRange { after: Some(at(2026, 10, 2, 0)), before: None });
        assert_eq!(parse(">=2026-10-01"), TimeRange { after: Some(at(2026, 10, 1, 0)), before: None });
        assert_eq!(parse("<2026-10-01"), TimeRange { after: None, before: Some(at(2026, 10, 1, 0)) });
        assert_eq!(parse("<=2026-10-01"), TimeRange { after: None, before: Some(at(2026, 10, 2, 0)) });
        assert_eq!(
            parse("2026-10-31..2026-10-01"),
            TimeRange {
                after: Some(at(2026, 10, 1, 0)),
                before: Some(at(2026, 11, 1, 0))
            }
        );
        assert!(parse("<2026-10-01").holds(at(2026, 9, 30, 23)) && !parse("<2026-10-01").holds(at(2026, 10, 1, 0)));
        assert_eq!(TimeRange::parse(" ", now).unwrap(), None);
        // An age alone does not say newer or older.
        for wrong in ["7", "12h", "<7x", "2026-13-01", ">", "yesterday"] {
            assert!(TimeRange::parse(wrong, now).is_err(), "{wrong}");
        }

        let dir = tree("modified");
        let old = fs::File::options().write(true).open(dir.join("Cargo.toml")).unwrap();
        old.set_modified(SystemTime::now() - Duration::from_secs(30 * 86400)).unwrap();
        let aged = |text: &str| Query {
            modified: TimeRange::parse(text, chrono::Local::now()).unwrap(),
            ..query(&dir, "*", "")
        };
        assert_eq!(run(&aged(">7d")).0, ["Cargo.toml"]);
        assert!(!run(&aged("<7d")).0.contains(&"Cargo.toml".to_string()));
        // Directories have times of their own, and are found by them.
        let dirs = Query { name: Glob::new("*/").unwrap(), ..aged("<7d") };
        assert!(run(&dirs).0.contains(&"src/".to_string()));
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
