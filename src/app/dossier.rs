//! Alt+R: a directory summed up in one popup - what kinds of file take its
//! room, the largest and the newest, the languages its code is in by lines,
//! how busy git has been in it the last weeks, and how much of it is a copy
//! of something else in it.

use super::*;

/// Weeks of git that the dossier looks back over.
pub const DOSSIER_WEEKS: usize = 8;

/// How many of the largest and of the newest files are listed.
const LISTED: usize = 5;

/// The largest file read for its lines: past it, a file of code is not one
/// anybody wrote, and reading it would only hold the dossier up.
const LINES_UP_TO: u64 = 8 * 1024 * 1024;

/// Directories not looked into: git's own, whose objects would count as
/// files of no extension and as copies of each other.
const PASSED_BY: [&str; 1] = [".git"];

/// What Alt+R found.
#[derive(Debug, Default)]
pub struct Dossier {
    pub dir: PathBuf,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// Each extension, lower case and empty for none: its bytes and its
    /// files, the most bytes first.
    pub kinds: Vec<(String, u64, u64)>,
    /// The largest files, by their paths from `dir`.
    pub largest: Vec<(PathBuf, u64)>,
    /// The ones changed last.
    pub newest: Vec<(PathBuf, SystemTime)>,
    /// Each language: its lines and its files, the most lines first.
    pub languages: Vec<(&'static str, u64, u64)>,
    /// Commits a week, the oldest week first and this one last; None outside
    /// a repository.
    pub weeks: Option<[u32; DOSSIER_WEEKS]>,
    /// Files with the same content as one already counted, and their bytes.
    pub copies: u64,
    pub wasted: u64,
}

/// The dossier on screen, and how far down it is scrolled.
#[derive(Debug)]
pub struct DossierView {
    pub dossier: Box<Dossier>,
    pub scroll: usize,
    /// Rows the popup showed last, for a page up or down.
    pub room: usize,
}

/// The language a file is written in, told by its extension.
fn language(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "rs" => "Rust",
        "py" | "pyw" => "Python",
        "js" | "mjs" | "cjs" | "jsx" => "JavaScript",
        "ts" | "mts" | "cts" | "tsx" => "TypeScript",
        "go" => "Go",
        "c" => "C",
        "h" => "C/C++ header",
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => "C++",
        "cs" => "C#",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "scala" => "Scala",
        "swift" => "Swift",
        "m" | "mm" => "Objective-C",
        "rb" => "Ruby",
        "php" => "PHP",
        "pl" | "pm" => "Perl",
        "lua" => "Lua",
        "r" => "R",
        "dart" => "Dart",
        "ex" | "exs" => "Elixir",
        "erl" | "hrl" => "Erlang",
        "hs" => "Haskell",
        "ml" | "mli" => "OCaml",
        "clj" | "cljs" => "Clojure",
        "zig" => "Zig",
        "nim" => "Nim",
        "jl" => "Julia",
        "sh" | "bash" | "zsh" | "fish" => "Shell",
        "ps1" => "PowerShell",
        "sql" => "SQL",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" | "less" => "CSS",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "md" | "markdown" => "Markdown",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "json" => "JSON",
        "xml" => "XML",
        _ => return None,
    })
}

impl AppState {
    /// Alt+R: the active panel's directory, summed up as a job.
    pub fn open_dossier(&mut self) {
        let side = self.active_side;
        if self.refuse_in_archive(side) {
            return;
        }
        let dir = self.panel_dir(side);
        self.start_job(TransferKind::Dossier, Clashes::Ask, move |progress| run_dossier(dir, progress));
    }
}

/// The worker thread behind Alt+R. The walk is counted in nothing; then
/// the bar counts the bytes read, for the copies and for the lines.
fn run_dossier(dir: PathBuf, mut progress: JobProgress<'_>) {
    let mut dossier = Dossier { dir: dir.clone(), ..Default::default() };
    let mut kinds: HashMap<String, (u64, u64)> = HashMap::new();
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut files: Vec<(PathBuf, u64, Option<SystemTime>)> = Vec::new();
    let mut code: Vec<(PathBuf, u64, &'static str)> = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![dir.clone()];
    while let Some(path) = stack.pop() {
        let Ok(metadata) = path.symlink_metadata() else { continue };
        if metadata.is_dir() {
            if !progress.step(Step::Starting(&path)) {
                return progress.finish(Ok(Transfer::Cancelled));
            }
            if path != dir {
                dossier.dirs += 1;
            }
            match std::fs::read_dir(&path) {
                Ok(entries) => stack.extend(entries.flatten().filter(|entry| !PASSED_BY.iter().any(|name| entry.file_name() == *name)).map(|entry| entry.path())),
                Err(_) => {
                    let _ = progress.updates.send(JobUpdate::Skipped);
                }
            }
            continue;
        }
        // A file with several names is counted once; a link is not a file.
        if !metadata.is_file() || !super::duplicates::first_name(&metadata, &mut seen) {
            continue;
        }
        let size = metadata.len();
        dossier.files += 1;
        dossier.bytes += size;
        let extension = path.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
        if let Some(language) = language(&extension).filter(|_| size <= LINES_UP_TO) {
            code.push((path.clone(), size, language));
        }
        let kind = kinds.entry(extension).or_default();
        (kind.0, kind.1) = (kind.0 + size, kind.1 + 1);
        if size > 0 {
            by_size.entry(size).or_default().push(path.clone());
        }
        files.push((path, size, metadata.modified().ok()));
    }

    let sizes = super::duplicates::shared_sizes(by_size);
    let code_bytes: u64 = code.iter().map(|(_, size, _)| size).sum();
    let _ = progress.updates.send(JobUpdate::Total(super::duplicates::bytes_to_read(&sizes).saturating_add(code_bytes)));
    let Some(groups) = super::duplicates::same_content(sizes, &mut progress) else {
        return progress.finish(Ok(Transfer::Cancelled));
    };
    for group in &groups {
        let size = group.first().and_then(|path| path.metadata().ok()).map_or(0, |metadata| metadata.len());
        let extra = group.len() as u64 - 1;
        dossier.copies += extra;
        dossier.wasted += size * extra;
    }

    let mut languages: HashMap<&'static str, (u64, u64)> = HashMap::new();
    for (path, _, name) in code {
        if !progress.step(Step::Starting(&path)) {
            return progress.finish(Ok(Transfer::Cancelled));
        }
        let Some(lines) = count_lines(&path, &mut progress) else {
            return progress.finish(Ok(Transfer::Cancelled));
        };
        let language = languages.entry(name).or_default();
        (language.0, language.1) = (language.0 + lines, language.1 + 1);
    }

    dossier.weeks = crate::git::commit_times(&dir, DOSSIER_WEEKS).ok().map(|times| by_week(&times, SystemTime::now()));

    let relative = |path: PathBuf| path.strip_prefix(&dir).map(Path::to_path_buf).unwrap_or(path);
    let mut kinds: Vec<(String, u64, u64)> = kinds.into_iter().map(|(extension, (bytes, count))| (extension, bytes, count)).collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    dossier.kinds = kinds;
    let mut languages: Vec<(&'static str, u64, u64)> = languages.into_iter().map(|(name, (lines, count))| (name, lines, count)).collect();
    languages.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    dossier.languages = languages;
    files.sort_by_key(|file| std::cmp::Reverse(file.1));
    dossier.largest = files.iter().take(LISTED).map(|(path, size, _)| (relative(path.clone()), *size)).collect();
    files.sort_by_key(|file| std::cmp::Reverse(file.2));
    dossier.newest = files.into_iter().filter_map(|(path, _, modified)| Some((relative(path), modified?))).take(LISTED).collect();

    let _ = progress.updates.send(JobUpdate::Dossier(Box::new(dossier)));
    progress.finish(Ok(Transfer::Done));
}

/// The lines of a file, as wc -l counts them, with one more for a last line
/// that has no newline. Counted onto the bar as read; one that cannot be
/// read has none. None when the job was stopped.
fn count_lines(path: &Path, progress: &mut JobProgress<'_>) -> Option<u64> {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return Some(0);
    };
    let mut buffer = vec![0u8; 256 * 1024];
    let (mut lines, mut last) = (0, b'\n');
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        lines += buffer[..read].iter().filter(|&&byte| byte == b'\n').count() as u64;
        last = buffer[read - 1];
        if !progress.step(Step::Advanced(read as u64)) {
            return None;
        }
    }
    Some(lines + u64::from(last != b'\n'))
}

/// Commits counted into the week each was made in, back from `now`: the
/// last of them this week, the first the week `DOSSIER_WEEKS - 1` before.
fn by_week(times: &[i64], now: SystemTime) -> [u32; DOSSIER_WEEKS] {
    const WEEK: i64 = 7 * 24 * 60 * 60;
    let now = now.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64);
    let mut weeks = [0; DOSSIER_WEEKS];
    for &time in times {
        let ago = ((now - time).max(0) / WEEK) as usize;
        if ago < DOSSIER_WEEKS {
            weeks[DOSSIER_WEEKS - 1 - ago] += 1;
        }
    }
    weeks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commits_fall_in_the_week_they_were_made() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100 * 7 * 86_400);
        let at = |weeks_ago: f64| 100 * 7 * 86_400 - (weeks_ago * 7.0 * 86_400.0) as i64;
        let weeks = by_week(&[at(0.1), at(0.5), at(1.5), at(7.9), at(8.5)], now);
        assert_eq!(weeks, [1, 0, 0, 0, 0, 0, 1, 2]);
    }
}
