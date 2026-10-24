use super::*;

mod copy;
mod delete;
mod moving;
mod paths;
mod save;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fm84-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A named pipe with no writer: anything that opens it to read blocks, so a
/// test that reaches one hangs rather than fails.
#[cfg(unix)]
fn fifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}

/// Answers every file in the way the same way, and remembers which.
struct Clashing {
    answer: Clash,
    asked: Vec<(PathBuf, PathBuf)>,
}

impl Clashing {
    fn new(answer: Clash) -> Self {
        Clashing { answer, asked: Vec::new() }
    }
}

impl Progress for Clashing {
    fn step(&mut self, _step: Step<'_>) -> bool {
        true
    }

    fn exists(&mut self, source: &Path, dest: &Path) -> Clash {
        self.asked.push((source.to_path_buf(), dest.to_path_buf()));
        self.answer
    }
}

/// Answers every failure the same way, and remembers what it was asked about.
struct Answering {
    reply: Reply,
    asked: Vec<PathBuf>,
}

impl Answering {
    fn new(reply: Reply) -> Self {
        Answering { reply, asked: Vec::new() }
    }
}

impl Progress for Answering {
    fn step(&mut self, _step: Step<'_>) -> bool {
        true
    }

    fn failed(&mut self, path: &Path, _error: &Error) -> Reply {
        self.asked.push(path.to_path_buf());
        self.reply
    }
}

/// A directory nothing can be added to or removed from. None when running
/// as root, which the permissions do not stop.
#[cfg(unix)]
fn locked(dir: &Path) -> Option<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).unwrap();
    if File::create(dir.join(".probe")).is_ok() {
        let _ = remove_file(dir.join(".probe"));
        unlock(dir);
        return None;
    }
    Some(())
}

#[cfg(unix)]
fn unlock(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
}
