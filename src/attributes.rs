//! Alt+A: change what is selected - its permissions, owner and group, and
//! when it was last modified. Read from what is typed into the dialog, and
//! applied one entry at a time.

use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// How the permission bits change: to a mode written in octal, or by
/// clauses as chmod takes them, which suit several entries whose modes differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModeChange {
    Absolute(u32),
    Symbolic(Vec<Clause>),
}

/// One clause of a symbolic mode: who, and what is done for them - `go-w`,
/// `u+x`, `a=rX`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause {
    /// The bits of the classes named, `0o700` for u and so on.
    who: u32,
    /// Each `+`, `-` or `=` and the letters after it.
    actions: Vec<(char, String)>,
}

impl ModeChange {
    /// `755`, `0644` or `4755`; or `u+x,go-w`, `a=rX`, `+t`. With no class
    /// named, a clause is for everyone - the umask has no say, as it does
    /// for chmod.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if !text.is_empty() && text.chars().all(|c| c.is_digit(8)) {
            return match u32::from_str_radix(text, 8) {
                Ok(mode) if mode <= 0o7777 => Ok(ModeChange::Absolute(mode)),
                _ => Err(t!("Not a mode: {text}", text)),
            };
        }
        let not_a_mode = || t!("Not a mode: {text} - 755, or u+x,go-w", text);
        let mut clauses = Vec::new();
        for part in text.split(',') {
            let mut chars = part.trim().chars().peekable();
            let mut who = 0;
            while let Some(&c) = chars.peek() {
                who |= match c {
                    'u' => 0o4700,
                    'g' => 0o2070,
                    'o' => 0o1007,
                    'a' => 0o7777,
                    _ => break,
                };
                chars.next();
            }
            if who == 0 {
                who = 0o7777;
            }
            let mut actions = Vec::new();
            while let Some(op) = chars.next() {
                if !matches!(op, '+' | '-' | '=') {
                    return Err(not_a_mode());
                }
                let mut letters = String::new();
                while let Some(&c) = chars.peek() {
                    if !matches!(c, 'r' | 'w' | 'x' | 'X' | 's' | 't') {
                        break;
                    }
                    letters.push(c);
                    chars.next();
                }
                actions.push((op, letters));
            }
            if actions.is_empty() {
                return Err(not_a_mode());
            }
            clauses.push(Clause { who, actions });
        }
        Ok(ModeChange::Symbolic(clauses))
    }

    /// The mode an entry with `mode` ends up with. `X` is execute for a
    /// directory, or for a file someone may already execute.
    pub fn apply(&self, mode: u32, is_dir: bool) -> u32 {
        let clauses = match self {
            ModeChange::Absolute(mode) => return *mode,
            ModeChange::Symbolic(clauses) => clauses,
        };
        let mut mode = mode & 0o7777;
        for clause in clauses {
            for (op, letters) in &clause.actions {
                let mut bits = 0;
                for letter in letters.chars() {
                    bits |= match letter {
                        'r' => 0o444,
                        'w' => 0o222,
                        'x' => 0o111,
                        'X' if is_dir || mode & 0o111 != 0 => 0o111,
                        's' => 0o6000,
                        't' => 0o1000,
                        _ => 0,
                    };
                }
                let bits = bits & clause.who;
                match op {
                    '+' => mode |= bits,
                    '-' => mode &= !bits,
                    // `=` clears what the classes named had before it sets,
                    // as GNU chmod does: setuid and setgid with them, and the
                    // sticky bit with the others'. A directory keeps its
                    // setuid and setgid unless they are named.
                    _ => mode = (mode & !(clause.who & if is_dir { 0o1777 } else { 0o7777 })) | bits,
                }
            }
        }
        mode
    }
}

/// A modified time as the dialog writes it, in local time: `2026-10-23
/// 14:02:11`, the seconds optional.
pub fn parse_time(text: &str) -> Result<SystemTime, String> {
    use chrono::{Local, NaiveDateTime, TimeZone};
    let text = text.trim();
    let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M"))
        .map_err(|_| t!("Not a time: {text} - 2026-10-23 14:02:11", text))?;
    // A time a clock change skips is not one; one it repeats is the first.
    Local.from_local_datetime(&naive).earliest().map(SystemTime::from).ok_or_else(|| t!("Not a time here: {text}", text))
}

pub fn format_time(time: SystemTime) -> String {
    let local: chrono::DateTime<chrono::Local> = time.into();
    local.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// What the dialog changes. None leaves that alone.
#[derive(Default)]
pub struct Change {
    pub mode: Option<ModeChange>,
    pub owner: Option<u32>,
    pub group: Option<u32>,
    pub modified: Option<SystemTime>,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.mode.is_none() && self.owner.is_none() && self.group.is_none() && self.modified.is_none()
    }

    /// Make the change to one entry, following a link as chmod and chown
    /// do. Owner and group go first: a chown clears setuid and setgid, which
    /// the mode can then put back.
    pub fn apply(&self, path: &Path, is_dir: bool) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if self.owner.is_some() || self.group.is_some() {
                std::os::unix::fs::chown(path, self.owner, self.group)?;
            }
            if let Some(change) = &self.mode {
                let mode = fs::metadata(path)?.permissions().mode();
                fs::set_permissions(path, fs::Permissions::from_mode(change.apply(mode, is_dir)))?;
            }
        }
        #[cfg(not(unix))]
        let _ = is_dir;
        if let Some(modified) = self.modified {
            set_modified(path, modified)?;
        }
        Ok(())
    }
}

/// Set `path`'s modified time, following a link, its access time left as it
/// is. Setting a time needs only to own the entry. On Unix it is set by name,
/// never opened - opening one its owner may not read is refused, and opening
/// a named pipe waits for a writer, for ever if none comes.
#[cfg(unix)]
fn set_modified(path: &Path, modified: SystemTime) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // Before 1970 the seconds go below none, and the nanoseconds stay above.
    let (seconds, nanoseconds) = match modified.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => (since.as_secs() as i64, since.subsec_nanos() as i64),
        Err(before) => {
            let before = before.duration();
            let (seconds, nanoseconds) = (before.as_secs() as i64, before.subsec_nanos() as i64);
            if nanoseconds == 0 { (-seconds, 0) } else { (-seconds - 1, 1_000_000_000 - nanoseconds) }
        }
    };
    let times = [
        libc::timespec { tv_sec: 0, tv_nsec: libc::UTIME_OMIT },
        libc::timespec {
            tv_sec: seconds as libc::time_t,
            tv_nsec: nanoseconds as _,
        },
    ];
    // SAFETY: a name ended by its nul, and the two times utimensat reads.
    if unsafe { libc::utimensat(libc::AT_FDCWD, name.as_ptr(), times.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// On Windows, opened asking only to write its attributes, which is what a
/// time is - opened to read, the system refuses the time - and with backup
/// semantics, without which a directory cannot be opened at all.
#[cfg(windows)]
fn set_modified(path: &Path, modified: SystemTime) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    fs::OpenOptions::new().access_mode(FILE_WRITE_ATTRIBUTES).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(path)?.set_modified(modified)
}

#[cfg(not(any(unix, windows)))]
fn set_modified(path: &Path, modified: SystemTime) -> io::Result<()> {
    fs::File::open(path)?.set_modified(modified)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, mode: u32, is_dir: bool) -> u32 {
        ModeChange::parse(text).unwrap().apply(mode, is_dir)
    }

    #[test]
    fn octal_modes() {
        assert_eq!(apply("755", 0o600, false), 0o755);
        assert_eq!(apply("0644", 0o777, false), 0o644);
        assert_eq!(apply("4755", 0o644, false), 0o4755);
        assert!(ModeChange::parse("17777").is_err());
        assert!(ModeChange::parse("789").is_err());
    }

    #[test]
    fn symbolic_modes_as_chmod_reads_them() {
        assert_eq!(apply("u+x", 0o644, false), 0o744);
        assert_eq!(apply("go-w", 0o666, false), 0o644);
        assert_eq!(apply("+x", 0o644, false), 0o755);
        assert_eq!(apply("a=r", 0o4755, false), 0o444);
        assert_eq!(apply("u=rw,go=r", 0o777, false), 0o644);
        assert_eq!(apply("o-rwx,g+w", 0o755, false), 0o770);
        assert_eq!(apply("u+s", 0o755, false), 0o4755);
        assert_eq!(apply("+t", 0o777, true), 0o1777);
        assert_eq!(apply("u-x+w", 0o544, false), 0o644);
    }

    /// `=` as GNU chmod has it, each checked against it from mode 7755: the
    /// sticky bit goes with the others' class, so `a=` and `o=` clear it;
    /// and a directory keeps its setuid and setgid bits unless they are
    /// named, as they decide who owns what is made in it.
    #[test]
    fn equals_clears_as_chmod_does() {
        for (text, file, dir) in [
            ("a=rx", 0o555, 0o6555),
            ("o=r", 0o6754, 0o6754),
            ("o=rx", 0o6755, 0o6755),
            ("u=rw", 0o3655, 0o7655),
            ("g=", 0o5705, 0o7705),
            ("=r", 0o444, 0o6444),
            ("a=rwxt", 0o1777, 0o7777),
            ("u=rwxs", 0o7755, 0o7755),
        ] {
            assert_eq!(apply(text, 0o7755, false), file, "{text} on a file: {:o}", apply(text, 0o7755, false));
            assert_eq!(apply(text, 0o7755, true), dir, "{text} on a directory: {:o}", apply(text, 0o7755, true));
        }
    }

    #[test]
    fn capital_x_is_for_directories_and_files_already_executable() {
        assert_eq!(apply("a+X", 0o644, true), 0o755);
        assert_eq!(apply("a+X", 0o644, false), 0o644);
        assert_eq!(apply("a+X", 0o744, false), 0o755);
    }

    #[test]
    fn what_is_not_a_mode() {
        for text in ["", "u", "u+q", "x+r", "755x", "u+x,"] {
            assert!(ModeChange::parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn times_read_back_as_they_were_written() {
        let time = parse_time("2026-10-23 14:02:11").unwrap();
        assert_eq!(format_time(time), "2026-10-23 14:02:11");
        assert_eq!(format_time(parse_time(" 2026-01-02 03:04 ").unwrap()), "2026-01-02 03:04:00");
        assert!(parse_time("23/10/26").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_change_reaches_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("fm84-attributes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        fs::write(&file, "").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();

        let then = parse_time("2001-02-03 04:05:06").unwrap();
        let change = Change {
            mode: Some(ModeChange::parse("u+x,o-r").unwrap()),
            modified: Some(then),
            ..Change::default()
        };
        change.apply(&file, false).unwrap();
        let metadata = fs::metadata(&file).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o7777, 0o740);
        assert_eq!(metadata.modified().unwrap(), then);

        // Read-only, it still takes a time.
        fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
        Change {
            modified: Some(SystemTime::UNIX_EPOCH),
            ..Change::default()
        }
        .apply(&file, false)
        .unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(all(test, unix))]
mod time_tests {
    use super::*;
    use std::time::Duration;

    /// A modified time is set on what can be neither read nor opened without
    /// waiting: a file its owner may not read, as `touch` sets it, and a named
    /// pipe, which opening to read would wait on for a writer for ever.
    #[test]
    fn a_time_is_set_without_opening_what_it_is_set_on() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("fm84-attr-time-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
        let change = Change { modified: Some(when), ..Change::default() };

        let locked = dir.join("locked");
        fs::write(&locked, "x").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o200)).unwrap();
        let result = change.apply(&locked, false);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o600)).unwrap();
        result.expect("set on a file its owner may not read");
        assert_eq!(fs::metadata(&locked).unwrap().modified().unwrap(), when);
        // Before 1970, part of a second in.
        let early = SystemTime::UNIX_EPOCH - Duration::from_millis(1_500);
        Change { modified: Some(early), ..Change::default() }.apply(&locked, false).unwrap();
        assert_eq!(fs::metadata(&locked).unwrap().modified().unwrap(), early);

        let pipe = dir.join("pipe");
        let name = std::ffi::CString::new(pipe.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (sent, got) = std::sync::mpsc::channel();
        let path = pipe.clone();
        std::thread::spawn(move || {
            let change = Change { modified: Some(when), ..Change::default() };
            let _ = sent.send(change.apply(&path, false).map_err(|e| e.to_string()));
        });
        let result = got.recv_timeout(Duration::from_secs(3));
        // Let a thread still waiting on the pipe go, before anything fails.
        if result.is_err() {
            let _ = fs::OpenOptions::new().write(true).open(&pipe);
        }
        result.expect("not waiting on the pipe").expect("set on a named pipe");
        assert_eq!(fs::metadata(&pipe).unwrap().modified().unwrap(), when);
        fs::remove_dir_all(&dir).unwrap();
    }
}
