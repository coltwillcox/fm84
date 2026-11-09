//! The programs a file can be opened with: the user's own, by its extension,
//! from the `open` file beside the config, and on Linux and the BSDs the
//! applications the desktop says take its type - read from the same files
//! `xdg-open` and every file manager read, rather than by running anything.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::options::Association;

/// One way of opening a file, as Alt+O's chooser lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opener {
    /// What the chooser calls it.
    pub label: String,
    /// What it runs, written out beside the label.
    pub detail: String,
    pub launch: Launch,
    /// Whether it runs in this terminal, which fm84 hands over to it, rather
    /// than in a window of its own.
    pub terminal: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// A command of the user's own, run in the shell, with the placeholders
    /// Alt+M's commands have.
    Shell(String),
    /// An application's `Exec` line, already split into its words, with
    /// the desktop entry's field codes still in them.
    Desktop { words: Vec<String>, name: String, icon: Option<String>, entry: PathBuf },
    /// Whatever the system opens it with, as Shift+Enter does.
    Default,
    /// Windows' own "Open with" dialog, for a program that is not listed.
    Choose,
}

/// Whether a line of the `open` file takes this file: its name ends in one
/// of the extensions, in either case - `tar.gz` as well as `gz` - or the
/// line is for every file.
pub fn applies(association: &Association, name: &str) -> bool {
    let name = name.to_lowercase();
    association.extensions.iter().any(|extension| extension == "*" || name.ends_with(&format!(".{}", extension.to_lowercase())))
}

/// The user's own programs for `path`, in the order the file has them.
pub fn configured(path: &Path, associations: &[Association]) -> Vec<Opener> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    associations
        .iter()
        .filter(|association| applies(association, &name))
        .map(|association| Opener {
            label: program_of(&association.command),
            detail: association.command.clone(),
            launch: Launch::Shell(association.command.clone()),
            terminal: association.terminal,
        })
        .collect()
}

/// The first word of a command, without its directory, to name it by.
fn program_of(command: &str) -> String {
    let first = command.split_whitespace().next().unwrap_or(command);
    Path::new(first).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| first.to_string())
}

/// Everything Alt+O offers for `path`: the user's own first, then what the
/// desktop knows of, then the system's default - and on Windows its own
/// chooser, for the programs fm84 cannot list there.
pub fn openers(path: &Path, is_dir: bool, associations: &[Association]) -> Vec<Opener> {
    let mut openers = if is_dir { Vec::new() } else { configured(path, associations) };
    if cfg!(all(unix, not(target_os = "macos"))) {
        let (data, config) = xdg_dirs();
        let mimes = mime_types(path, is_dir, &data);
        openers.extend(desktop_openers(&mimes, &data, &config));
    }
    openers.push(Opener {
        label: t!("System default").to_string(),
        detail: String::new(),
        launch: Launch::Default,
        terminal: false,
    });
    if cfg!(windows) && !is_dir {
        openers.push(Opener {
            label: t!("Choose another program").to_string(),
            detail: String::new(),
            launch: Launch::Choose,
            terminal: false,
        });
    }
    openers
}

/// Where the desktop keeps its data and its settings, most personal first.
fn xdg_dirs() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let home = var("HOME").map(PathBuf::from);
    let list = |name: &str, fallback: &str| -> Vec<PathBuf> {
        let value = var(name).unwrap_or_else(|| OsString::from(fallback));
        std::env::split_paths(&value).filter(|dir| dir.is_absolute()).collect()
    };
    let mut data: Vec<PathBuf> = var("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|home| home.join(".local/share"))).into_iter().collect();
    data.extend(list("XDG_DATA_DIRS", "/usr/local/share:/usr/share"));
    let mut config: Vec<PathBuf> = var("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|home| home.join(".config"))).into_iter().collect();
    config.extend(list("XDG_CONFIG_DIRS", "/etc/xdg"));
    (data, config)
}

/// The file's type as the shared MIME database names it, then the types it
/// is a kind of - a Rust source is plain text too, and most editors say
/// only that they take plain text. Found by name; one the database has no
/// name for is looked into, and taken as text when it reads as text.
pub fn mime_types(path: &Path, is_dir: bool, data: &[PathBuf]) -> Vec<String> {
    let mut types = Vec::new();
    if is_dir {
        types.push("inode/directory".to_string());
        return types;
    }
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    match mime_by_name(&name, data) {
        Some(mime) => types.push(mime),
        None if reads_as_text(path) => types.push("text/plain".to_string()),
        None => return types,
    }
    let parents = subclasses(data);
    let mut index = 0;
    while index < types.len() && types.len() < 16 {
        for parent in parents.get(&types[index]).into_iter().flatten() {
            if !types.contains(parent) {
                types.push(parent.clone());
            }
        }
        index += 1;
    }
    if types[0].starts_with("text/") && !types.iter().any(|mime| mime == "text/plain") {
        types.push("text/plain".to_string());
    }
    types
}

/// The type `globs2` gives a name: of the patterns that match, the heaviest,
/// and of those the longest - `*.tar.gz` over `*.gz`.
fn mime_by_name(name: &str, data: &[PathBuf]) -> Option<String> {
    let lower = name.to_lowercase();
    let mut best: Option<(u32, usize, String)> = None;
    for dir in data {
        let Ok(text) = std::fs::read_to_string(dir.join("mime/globs2")) else {
            continue;
        };
        for line in text.lines().filter(|line| !line.starts_with('#')) {
            let mut fields = line.split(':');
            let (Some(weight), Some(mime), Some(pattern)) = (fields.next(), fields.next(), fields.next()) else {
                continue;
            };
            let exact = fields.any(|flag| flag.split(',').any(|flag| flag == "cs"));
            let (subject, pattern) = if exact { (name, pattern.to_string()) } else { (lower.as_str(), pattern.to_lowercase()) };
            let matched = match pattern.strip_prefix('*') {
                Some(suffix) if !suffix.contains(['*', '?', '[']) => subject.ends_with(suffix),
                _ if !pattern.contains(['*', '?', '[']) => subject == pattern,
                _ => crate::glob::Glob::new(&pattern).is_some_and(|glob| glob.matches(subject, false)),
            };
            let weight = weight.parse().unwrap_or(50);
            if matched && best.as_ref().is_none_or(|(best_weight, length, _)| (weight, pattern.len()) > (*best_weight, *length)) {
                best = Some((weight, pattern.len(), mime.to_string()));
            }
        }
        // The first directory with a database is the one that counts: the
        // others are older copies of much the same.
        if best.is_some() {
            break;
        }
    }
    best.map(|(_, _, mime)| mime)
}

/// The start of the file holds no NUL and is UTF-8, short of a character
/// cut off at the end of what was read. Only a regular file is looked into:
/// opening a named pipe waits for something to write to it, and reading a
/// terminal waits for a key, either of them for ever.
fn reads_as_text(path: &Path) -> bool {
    use std::io::Read;
    let mut start = Vec::new();
    if !crate::fs_ops::is_regular_file(path) {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    if file.take(1024).read_to_end(&mut start).is_err() {
        return false;
    }
    !start.contains(&0)
        && match std::str::from_utf8(&start) {
            Ok(_) => true,
            Err(e) => e.error_len().is_none(),
        }
}

/// Each type with the types it is a kind of, from `mime/subclasses`.
fn subclasses(data: &[PathBuf]) -> HashMap<String, Vec<String>> {
    let mut parents: HashMap<String, Vec<String>> = HashMap::new();
    for dir in data {
        let Ok(text) = std::fs::read_to_string(dir.join("mime/subclasses")) else {
            continue;
        };
        for (mime, parent) in text.lines().filter_map(|line| line.split_once(' ')) {
            let list = parents.entry(mime.to_string()).or_default();
            if !list.iter().any(|known| known == parent) {
                list.push(parent.to_string());
            }
        }
    }
    parents
}

/// The applications for `mimes`, the first type's before its parents': the
/// defaults and additions of every `mimeapps.list`, then what each
/// installed application says it takes, as `mimeinfo.cache` gathers it,
/// less the ones a `mimeapps.list` takes away. Each once, in that order.
pub fn desktop_openers(mimes: &[String], data: &[PathBuf], config: &[PathBuf]) -> Vec<Opener> {
    if mimes.is_empty() {
        return Vec::new();
    }
    let app_dirs: Vec<PathBuf> = data.iter().map(|dir| dir.join("applications")).collect();
    let lists: Vec<PathBuf> = config.iter().map(|dir| dir.join("mimeapps.list")).chain(app_dirs.iter().map(|dir| dir.join("mimeapps.list"))).collect();
    let lists: Vec<Ini> = lists.iter().filter_map(|path| std::fs::read_to_string(path).ok()).map(|text| parse_ini(&text)).collect();
    let caches: Vec<Ini> = app_dirs.iter().filter_map(|dir| std::fs::read_to_string(dir.join("mimeinfo.cache")).ok()).map(|text| parse_ini(&text)).collect();

    let ids_in = |ini: &Ini, section: &str, mime: &str| -> Vec<String> {
        ini.get(section)
            .and_then(|keys| keys.get(mime))
            .map(|value| value.split(';').map(str::trim).filter(|id| !id.is_empty()).map(String::from).collect())
            .unwrap_or_default()
    };
    let removed: HashSet<String> = mimes.iter().flat_map(|mime| lists.iter().flat_map(|ini| ids_in(ini, "Removed Associations", mime))).collect();
    let mut ids: Vec<String> = Vec::new();
    for mime in mimes {
        let listed = lists.iter().flat_map(|ini| ids_in(ini, "Default Applications", mime).into_iter().chain(ids_in(ini, "Added Associations", mime)));
        let installed = caches.iter().flat_map(|ini| ids_in(ini, "MIME Cache", mime));
        for id in listed.chain(installed) {
            if !removed.contains(&id) && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids.iter().filter_map(|id| desktop_entry(id, &app_dirs)).collect()
}

/// An ini file's sections, each with its keys.
type Ini = HashMap<String, HashMap<String, String>>;

fn parse_ini(text: &str) -> Ini {
    let mut ini = Ini::new();
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
            section = name.to_string();
        } else if let Some((key, value)) = line.split_once('=') {
            // The first of a key in a section counts, as in the specs.
            ini.entry(section.clone()).or_default().entry(key.trim().to_string()).or_insert_with(|| value.trim().to_string());
        }
    }
    ini
}

/// The application a desktop file id names, from the first directory that
/// has it - `kde4-okular.desktop` may be `kde4/okular.desktop`. None for one
/// that is hidden, has nothing to run, or is not an application.
fn desktop_entry(id: &str, app_dirs: &[PathBuf]) -> Option<Opener> {
    let candidates = [id.to_string(), id.replacen('-', "/", 1)];
    let (path, text) = app_dirs
        .iter()
        .flat_map(|dir| candidates.iter().map(move |candidate| dir.join(candidate)))
        .find_map(|path| std::fs::read_to_string(&path).ok().map(|text| (path, text)))?;
    let ini = parse_ini(&text);
    let entry = ini.get("Desktop Entry")?;
    let yes = |key: &str| entry.get(key).is_some_and(|value| value == "true");
    if yes("Hidden") || entry.get("Type").is_some_and(|kind| kind != "Application") {
        return None;
    }
    let exec = unescape(entry.get("Exec")?);
    let words = split_exec(&exec);
    if words.is_empty() {
        return None;
    }
    let name = entry.get("Name").map(|name| unescape(name)).unwrap_or_else(|| id.trim_end_matches(".desktop").to_string());
    Some(Opener {
        label: name.clone(),
        detail: exec,
        launch: Launch::Desktop {
            words,
            name,
            icon: entry.get("Icon").filter(|icon| !icon.is_empty()).cloned(),
            entry: path,
        },
        terminal: yes("Terminal"),
    })
}

/// A desktop file's value with its escapes read: `\s` a space, `\n`, `\t`,
/// `\r`, and `\\` one backslash. This comes before the quoting of `Exec`
/// is read, which has escapes of its own, so a backslash in a quoted word
/// is written four times over and a `$` as `\\$`.
fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                // Not an escape here: left for the quoting of `Exec` to read.
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `Exec` in its words: split at spaces, except inside double quotes, where
/// a backslash makes the `"`, `` ` ``, `$` or `\` after it plain.
fn split_exec(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut started) = (false, false);
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            ' ' | '\t' if !quoted => {
                if started || !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                started = false;
            }
            _ => word.push(c),
        }
    }
    if started || !word.is_empty() {
        words.push(word);
    }
    words
}

/// The words of an `Exec` line with its field codes filled in for `path`:
/// `%f` and the others that take files as the path, whole however it is
/// spelt; `%i` the icon, `%c` the name, `%k` the desktop file, `%%` a `%`,
/// and the codes no longer used dropped. A line that takes no file gets it
/// on the end - the way file managers have it, though the spec would not.
pub fn exec_args(words: &[String], path: &Path, name: &str, icon: Option<&str>, entry: &Path) -> Vec<OsString> {
    let mut args = Vec::new();
    let mut took_file = false;
    for word in words {
        match word.as_str() {
            "%f" | "%F" | "%u" | "%U" => {
                args.push(path.as_os_str().to_owned());
                took_file = true;
                continue;
            }
            "%i" => {
                if let Some(icon) = icon {
                    args.push("--icon".into());
                    args.push(icon.into());
                }
                continue;
            }
            _ => {}
        }
        // Put together as the system's own string: a path in it - one not
        // UTF-8 too - goes in as it is.
        let mut out = OsString::new();
        let mut chars = word.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c.encode_utf8(&mut [0; 4]));
                continue;
            }
            match chars.next() {
                Some('%') => out.push("%"),
                Some('f' | 'F' | 'u' | 'U') => {
                    out.push(path.as_os_str());
                    took_file = true;
                }
                Some('c') => out.push(name),
                Some('k') => out.push(entry.as_os_str()),
                _ => {}
            }
        }
        // A word of nothing but codes no longer used - `%m`, `%d` - goes
        // with them, rather than reach the program as an empty word it
        // would take for a file. One quoted empty on purpose was empty to
        // begin with, and stays.
        if out.is_empty() && !word.is_empty() {
            continue;
        }
        args.push(out);
    }
    if !took_file {
        args.push(path.as_os_str().to_owned());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(extensions: &[&str]) -> Association {
        Association {
            extensions: extensions.iter().map(|extension| extension.to_string()).collect(),
            command: "/usr/bin/zathura --fork".to_string(),
            terminal: false,
        }
    }

    #[test]
    fn extensions_match_the_end_of_the_name_in_either_case() {
        assert!(applies(&line(&["pdf"]), "Book.PDF"));
        assert!(applies(&line(&["tar.gz"]), "src.tar.gz"));
        assert!(applies(&line(&["*"]), "Makefile"));
        assert!(!applies(&line(&["pdf"]), "pdf"));
        assert!(!applies(&line(&["gz"]), "src.tgz"));
        let openers = configured(Path::new("/a/b.pdf"), &[line(&["txt"]), line(&["epub", "pdf"])]);
        assert_eq!(openers.len(), 1);
        assert_eq!(openers[0].label, "zathura");
    }

    /// A desktop's data and config directories, built in a scratch one.
    fn desktop(name: &str) -> (PathBuf, Vec<PathBuf>, Vec<PathBuf>) {
        let root = std::env::temp_dir().join(format!("fm84-opener-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (data, config) = (root.join("data"), root.join("config"));
        std::fs::create_dir_all(data.join("mime")).unwrap();
        std::fs::create_dir_all(data.join("applications/kde4")).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        (root, vec![data], vec![config])
    }

    #[test]
    fn types_come_from_the_heaviest_longest_glob_and_bring_their_parents() {
        let (root, data, _) = desktop("mime");
        std::fs::write(
            data[0].join("mime/globs2"),
            "# comment\n50:application/gzip:*.gz\n50:application/x-compressed-tar:*.tar.gz\n50:text/rust:*.rs\n50:text/x-c:*.C:cs\n20:text/x-readme:readme*\n",
        )
        .unwrap();
        std::fs::write(data[0].join("mime/subclasses"), "text/rust text/x-source\ntext/x-source text/plain\n").unwrap();
        let mime = |name: &str| mime_types(&root.join(name), false, &data);
        assert_eq!(mime("a.tar.gz"), ["application/x-compressed-tar"]);
        assert_eq!(mime("A.GZ"), ["application/gzip"]);
        assert_eq!(mime("main.rs"), ["text/rust", "text/x-source", "text/plain"]);
        assert_eq!(mime("x.C"), ["text/x-c", "text/plain"]);
        assert_eq!(mime("README.md"), ["text/x-readme", "text/plain"]);
        assert_eq!(mime_types(&root, true, &data), ["inode/directory"]);
        // Found by what it holds when the name says nothing.
        std::fs::write(root.join("notes"), "plain words\n").unwrap();
        std::fs::write(root.join("blob"), [0u8, 1, 2]).unwrap();
        assert_eq!(mime("notes"), ["text/plain"]);
        assert!(mime("blob").is_empty());
        // A pipe is not opened, which would wait for a writer for ever.
        #[cfg(unix)]
        {
            let pipe = std::ffi::CString::new(root.join("pipe").into_os_string().into_encoded_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(pipe.as_ptr(), 0o600) }, 0);
            assert!(mime("pipe").is_empty());
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn applications_come_defaults_first_without_the_removed_or_hidden() {
        let (root, data, config) = desktop("apps");
        let apps = data[0].join("applications");
        let entry = |name: &str, exec: &str, extra: &str| format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}[Desktop Action new]\nName=Other\n");
        std::fs::write(apps.join("editor.desktop"), entry("Editor", "editor %F", "")).unwrap();
        std::fs::write(apps.join("viewer.desktop"), entry("Viewer", "viewer --open %u", "Terminal=true\n")).unwrap();
        std::fs::write(apps.join("kde4/kate.desktop"), entry("Kate", "kate", "")).unwrap();
        std::fs::write(apps.join("gone.desktop"), entry("Gone", "gone", "")).unwrap();
        std::fs::write(apps.join("hidden.desktop"), entry("Hidden", "hidden", "Hidden=true\n")).unwrap();
        std::fs::write(
            apps.join("mimeinfo.cache"),
            "[MIME Cache]\ntext/rust=editor.desktop;gone.desktop;\ntext/plain=hidden.desktop;kde4-kate.desktop;editor.desktop;missing.desktop;\n",
        )
        .unwrap();
        std::fs::write(config[0].join("mimeapps.list"), "[Default Applications]\ntext/plain=viewer.desktop\n[Removed Associations]\ntext/rust=gone.desktop;\n").unwrap();

        let openers = desktop_openers(&["text/rust".to_string(), "text/plain".to_string()], &data, &config);
        let names: Vec<(&str, bool)> = openers.iter().map(|opener| (opener.label.as_str(), opener.terminal)).collect();
        assert_eq!(names, [("Editor", false), ("Viewer", true), ("Kate", false)]);
        assert_eq!(openers[1].detail, "viewer --open %u");
        assert!(desktop_openers(&[], &data, &config).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A name that is not UTF-8 reaches the program as it is, inside a
    /// word too - `--file=%f` - not as a lossy reading of it that names no
    /// file at all.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_reaches_the_program_whole() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let file = Path::new(std::ffi::OsStr::from_bytes(b"/home/me/caf\xe9.txt"));
        let words = ["app".to_string(), "--file=%f".to_string(), "%f".to_string()];
        let args = exec_args(&words, file, "App", None, Path::new("/apps/app.desktop"));
        assert_eq!(args[1].clone().into_vec(), b"--file=/home/me/caf\xe9.txt");
        assert_eq!(args[2].as_bytes(), file.as_os_str().as_bytes());
    }

    #[test]
    fn exec_lines_split_as_the_spec_quotes_them() {
        // As a file has them: each backslash the quoting wants doubled again.
        let exec = unescape(r#"env "A=b c" app\s--x "say \"hi\" \\\\ \\$HOME" a\\b %U"#);
        let words = split_exec(&exec);
        assert_eq!(words, ["env", "A=b c", "app", "--x", r#"say "hi" \ $HOME"#, r"a\b", "%U"]);
        // What Wine writes for a Windows path.
        let wine = split_exec(&unescape(r#"wine "C:\\\\Program Files\\\\app.exe""#));
        assert_eq!(wine, ["wine", r"C:\Program Files\app.exe"]);
        let entry = Path::new("/apps/app.desktop");
        let file = Path::new("/home/me/a b.txt");
        let args = |words: &[&str], icon| exec_args(&words.iter().map(|word| word.to_string()).collect::<Vec<_>>(), file, "App", icon, entry);
        assert_eq!(
            args(&["app", "%i", "--title=%c", "%f", "100%%", "%d"], Some("app-icon")),
            ["app", "--icon", "app-icon", "--title=App", "/home/me/a b.txt", "100%"].map(OsString::from)
        );
        // A code no longer used is taken out of the line, not left as an
        // empty word the program would take for a file; a word quoted empty
        // on purpose stays.
        assert_eq!(args(&["app", "%m", "%v%D", "", "%f"], None), ["app", "", "/home/me/a b.txt"].map(OsString::from));
        // Nothing to take the file: it goes on the end. %k is the entry.
        assert_eq!(args(&["app", "%i", "%k"], None), ["app", "/apps/app.desktop", "/home/me/a b.txt"].map(OsString::from));
    }
}
