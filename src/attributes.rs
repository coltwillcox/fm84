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
                _ => Err(format!("Not a mode: {text}")),
            };
        }
        let not_a_mode = || format!("Not a mode: {text} - 755, or u+x,go-w");
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
                    // `=` clears what the classes named had, setuid and
                    // setgid with them, before it sets.
                    _ => mode = (mode & !(clause.who & 0o6777)) | bits,
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
        .map_err(|_| format!("Not a time: {text} - 2026-10-23 14:02:11"))?;
    // A time a clock change skips is not one; one it repeats is the first.
    Local.from_local_datetime(&naive).earliest().map(SystemTime::from).ok_or_else(|| format!("Not a time here: {text}"))
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
            // Setting a time needs only to own the file, not to write it, so
            // opening it to read is enough - a directory too, on Unix.
            fs::File::open(path)?.set_modified(modified)?;
        }
        Ok(())
    }
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
