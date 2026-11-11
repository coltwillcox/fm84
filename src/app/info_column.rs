//! The Info column: what the commands in the `columns` file say of each
//! entry - a picture's size, a song's length, a PDF's pages. Run on a thread
//! once the listing is in, as git's marks are, the values coming in as they
//! are worked out and kept until the file changes.

use super::*;
use std::process::Stdio;

/// How long one command has to answer.
const TIMEOUT: Duration = Duration::from_secs(2);

/// Entries a listing runs commands for, at most: a directory of a hundred
/// thousand pictures is no place to start a hundred thousand programs.
const LIMIT: usize = 5000;

/// The most of a value kept, in characters.
const LONGEST: usize = 40;

/// Values kept before the cache starts over.
const CACHED: usize = 50_000;

/// A file as it was when its command ran, and the command: either changing
/// is a reason to run it again.
type InfoKey = (PathBuf, u64, Option<SystemTime>, String);

/// One panel's values, and those on their way.
#[derive(Default)]
pub struct PanelInfo {
    /// By the name the panel lists the entry under.
    pub values: HashMap<OsString, String>,
    /// The thread working the rest out, stopped when this is dropped.
    running: Option<(Receiver<InfoUpdate>, StopOnDrop)>,
}

enum InfoUpdate {
    Value(OsString, InfoKey, String),
    Done,
}

impl AppState {
    /// After a listing: the values there are already, from the cache, and a
    /// thread for the rest - only with the column on, outside an archive,
    /// and for a file a line of the `columns` file is for.
    pub(super) fn request_info(&mut self, side: Side, in_archive: bool) {
        let columns = if self.options.column_info && !in_archive { crate::options::load_columns() } else { Vec::new() };
        self.fill_info(side, &columns);
    }

    /// The values for what `columns` - the `columns` file's lines - says.
    pub(super) fn fill_info(&mut self, side: Side, columns: &[crate::options::Association]) {
        self.panel_mut(side).info.running = None;
        let panel = self.panel(side);
        let mut values = HashMap::new();
        let mut todo = Vec::new();
        for item in panel.children.iter().filter(|item| !item.is_dir).take(LIMIT) {
            let path = item.path_in(&panel.dir);
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let Some(line) = columns.iter().find(|line| crate::opener::applies(line, &name)) else { continue };
            let key = (path.clone(), item.size_bytes, item.modified_at, line.command.clone());
            match self.info_cache.get(&key) {
                Some(value) => {
                    values.insert(item.name_os.clone(), value.clone());
                }
                None => todo.push((item.name_os.clone(), key)),
            }
        }
        self.panel_mut(side).info.values = values;
        if todo.is_empty() {
            return self.sort_by_info(side);
        }
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        std::thread::spawn(move || {
            for (name, key) in todo {
                if stopped.load(Ordering::Relaxed) {
                    return;
                }
                let value = run_command(&key.3, &key.0);
                if sender.send(InfoUpdate::Value(name, key, value)).is_err() {
                    return;
                }
            }
            let _ = sender.send(InfoUpdate::Done);
        });
        self.panel_mut(side).info.running = Some((receiver, StopOnDrop(stop)));
    }

    /// Take in whichever values have come since the last frame. Sorted by
    /// them, the panel is sorted again once they all have.
    pub fn poll_info(&mut self) {
        for side in Side::BOTH {
            let mut done = false;
            let mut came = Vec::new();
            if let Some((receiver, _)) = &self.panel(side).info.running {
                loop {
                    match receiver.try_recv() {
                        Ok(InfoUpdate::Value(name, key, value)) => came.push((name, key, value)),
                        Ok(InfoUpdate::Done) | Err(TryRecvError::Disconnected) => {
                            done = true;
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                    }
                }
            }
            if self.info_cache.len() + came.len() > CACHED {
                self.info_cache.clear();
            }
            for (name, key, value) in came {
                self.info_cache.insert(key, value.clone());
                self.panel_mut(side).info.values.insert(name, value);
            }
            if done {
                self.panel_mut(side).info.running = None;
                self.sort_by_info(side);
            }
        }
    }

    /// The panel in the order its values put it, when that is the order
    /// asked for: directories first if F11 says so, numbers in a value as
    /// numbers, and what has none after all that have one. The listing has
    /// sorted by name already, and the sort keeps that order among equals.
    /// The cursor stays on the entry it was on.
    fn sort_by_info(&mut self, side: Side) {
        if self.options.sort_key != SortKey::Info {
            return;
        }
        let (dirs_first, descending) = (self.options.dirs_first, self.options.sort_descending);
        let panel = self.panel_mut(side);
        let cursor = panel.state.selected().and_then(|index| panel.children.get(index)).map(|item| item.name_os.clone());
        let start = usize::from(panel.children.first().is_some_and(Item::is_parent));
        let values = &panel.info.values;
        panel.children[start..].sort_by(|a, b| {
            use std::cmp::Ordering;
            if dirs_first && a.is_dir != b.is_dir {
                return if a.is_dir { Ordering::Less } else { Ordering::Greater };
            }
            match (values.get(&a.name_os), values.get(&b.name_os)) {
                (Some(a), Some(b)) if descending => natural(a, b).reverse(),
                (Some(a), Some(b)) => natural(a, b),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        });
        if let Some(index) = cursor.and_then(|name| panel.children.iter().position(|item| item.name_os == name)) {
            panel.state.select(Some(index));
        }
    }

    /// E on F11's Info row: the `columns` file, in whichever editor F4 uses.
    pub fn edit_columns(&mut self) {
        self.dialog = None;
        let Some(file) = crate::options::columns_file() else {
            self.display_error(t!("No config directory to keep the commands in").to_string());
            return;
        };
        self.edit_config(file);
    }

    /// E on F11's Theme row: the `colors` file, in whichever editor F4 uses.
    /// What it says shows within a second of saving it.
    pub fn edit_colors(&mut self) {
        self.dialog = None;
        match crate::options::colors_file() {
            Some(file) => self.edit_config(file),
            None => self.display_error(t!("No config directory").to_string()),
        }
    }

    fn edit_config(&mut self, file: PathBuf) {
        if self.options.editor.trim().is_empty() {
            self.request_open(file, true);
        } else {
            self.external_edit = Some(file);
        }
    }
}

/// Two values in the order a person would put them: the numbers in them as
/// numbers, so 800x600 comes before 1920x1080 and 9:58 before 10:02, and
/// the rest as text, whatever its case.
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let number = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut digits = String::new();
                    while let Some(&digit) = chars.peek().filter(|c| c.is_ascii_digit()) {
                        digits.push(digit);
                        chars.next();
                    }
                    digits.trim_start_matches('0').to_string()
                };
                let (x, y) = (number(&mut a), number(&mut b));
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(&y));
                if order.is_ne() {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order.is_ne() {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// What `command` prints first for the file at `path`: its first line that
/// is not empty, made fit to show. Nothing, when it fails to start, or
/// takes longer than it has - then it is stopped.
fn run_command(command: &str, path: &Path) -> String {
    use std::io::Read;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let values = MenuValues {
        file: Some(path.to_path_buf()),
        files: vec![path.to_path_buf()],
        dir,
        other: PathBuf::new(),
    };
    let command = if uses_entries(command).0 { command.to_string() } else { format!("{command} {{file}}") };
    let mut process = shell_command(&command, &values);
    process.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    // A group of its own, so what the shell starts - a pipeline, something
    // sent to the background - can be stopped along with it.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut process, 0);
    let Ok(mut child) = process.spawn() else {
        return String::new();
    };
    // Read on a thread of its own, so a command that writes a lot cannot
    // fill the pipe and stand still; and given up on, should something it
    // started keep the pipe open past its end - which is then stopped, so
    // the pipe closes and the thread ends rather than wait on it for good.
    let (sender, read) = mpsc::channel();
    if let Some(mut stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = (&mut stdout).take(64 * 1024).read_to_end(&mut out);
            let _ = sender.send(out);
        });
    }
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                stop_group(&child);
                let _ = child.kill();
                let _ = child.wait();
                return String::new();
            }
        }
    }
    let out = match read.recv_timeout(Duration::from_millis(200)) {
        Ok(out) => out,
        // Still held open: what is left is stopped, and what was written
        // before that comes in as the pipe closes.
        Err(_) => {
            stop_group(&child);
            read.recv_timeout(Duration::from_millis(200)).unwrap_or_default()
        }
    };
    let text = String::from_utf8_lossy(&out);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    line.chars().filter(|c| !c.is_control()).take(LONGEST).collect()
}

/// Everything left of the group `child` leads. Its id stays taken while
/// anything is left in it, so after the shell is gone it still names them.
#[cfg(unix)]
fn stop_group(child: &std::process::Child) {
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        // SAFETY: kill only sends a signal; a group gone already is ESRCH.
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
}

#[cfg(not(unix))]
fn stop_group(_child: &std::process::Child) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_sort_with_their_numbers_as_numbers() {
        let mut values = vec!["1920x1080", "800x600", "10:02", "9:58", "Page", "page 2", "page 10", "007"];
        values.sort_by(|a, b| natural(a, b));
        assert_eq!(values, ["007", "9:58", "10:02", "800x600", "1920x1080", "Page", "page 2", "page 10"]);
    }

    /// Something a command leaves running with its output open is stopped
    /// once the value is in, not left holding the pipe and the reader.
    #[cfg(unix)]
    #[test]
    fn what_a_command_leaves_running_is_stopped() {
        let dir = std::env::temp_dir().join(format!("fm84-info-left-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("file");
        std::fs::write(&file, "").unwrap();
        let mark = dir.join("mark");
        let command = format!("(sleep 1; touch '{}') & echo now {{file}}", mark.display());
        let value = run_command(&command, &file);
        assert!(value.starts_with("now"), "{value:?}");
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!mark.exists(), "the background part was left running");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
