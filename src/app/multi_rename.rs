//! F2 with a selection: every name chosen renamed at once, by a pattern -
//! `[N]` the name, `[E]` the extension, `[C]` a counter - then a find and
//! replace and a change of case, with the old names and the new side by
//! side before anything happens.

use super::*;

/// What the case of each new name is changed to, last of all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseChange {
    Unchanged,
    Lower,
    Upper,
    /// The first letter a capital, the rest small.
    First,
    /// Each word's first letter a capital, the rest small.
    Title,
}

impl CaseChange {
    const ALL: [CaseChange; 5] = [CaseChange::Unchanged, CaseChange::Lower, CaseChange::Upper, CaseChange::First, CaseChange::Title];

    pub fn label(self) -> &'static str {
        match self {
            CaseChange::Unchanged => t!("Unchanged"),
            CaseChange::Lower => t!("lower case"),
            CaseChange::Upper => t!("UPPER CASE"),
            CaseChange::First => t!("First letter"),
            CaseChange::Title => t!("Title Case"),
        }
    }

    fn apply(self, name: &str) -> String {
        match self {
            CaseChange::Unchanged => name.to_string(),
            CaseChange::Lower => name.to_lowercase(),
            CaseChange::Upper => name.to_uppercase(),
            CaseChange::First => {
                let mut chars = name.chars();
                chars.next().map(|first| first.to_uppercase().chain(chars.as_str().to_lowercase().chars()).collect()).unwrap_or_default()
            }
            CaseChange::Title => {
                let mut out = String::new();
                let mut start = true;
                for c in name.chars() {
                    if start {
                        out.extend(c.to_uppercase());
                    } else {
                        out.extend(c.to_lowercase());
                    }
                    start = !c.is_alphanumeric() && c != '\'';
                }
                out
            }
        }
    }
}

/// One of the entries being renamed.
pub struct RenameEntry {
    /// Where it is now.
    pub path: PathBuf,
    /// Its name as it is shown, and as the pattern reads it.
    pub old: String,
    /// False for a name that is not valid UTF-8, which `old` only stands for:
    /// a name made from it would lose what could not be read.
    pub readable: bool,
    /// What the file says of itself, read the first time a pattern asks -
    /// and kept, as the table is made again at every key.
    pub meta: std::cell::OnceCell<crate::metadata::Fields>,
}

/// Why a new name cannot be had.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenameProblem {
    /// Empty, `.` or `..`, or with a separator in it.
    NotPlain,
    /// Two of the new names are the same.
    Twice,
    /// Something else in the directory already has it.
    Taken,
    /// The old name is not valid UTF-8, so the new one cannot be made of it.
    Unreadable,
    /// The pattern asks for what the file does not say of itself - a
    /// picture with no EXIF, a song with no tags.
    NoMetadata,
}

impl RenameProblem {
    pub fn label(self) -> &'static str {
        match self {
            RenameProblem::NotPlain => t!("not a plain name"),
            RenameProblem::Twice => t!("given twice"),
            RenameProblem::Taken => t!("already exists"),
            RenameProblem::Unreadable => t!("name not readable as text"),
            RenameProblem::NoMetadata => t!("not in the file"),
        }
    }
}

/// The dialog, as it is filled in.
pub struct MultiRename {
    pub entries: Vec<RenameEntry>,
    /// The pattern, the text to find, and what replaces it.
    pub fields: [TextInput; 3],
    /// Which of the fields is being typed into; one past them is the case.
    pub focus: usize,
    pub case: CaseChange,
    /// The first row of the table shown, and how many it showed last.
    pub scroll: usize,
    pub room: usize,
}

/// The row of the dialog that changes the case rather than being typed into.
pub const CASE_ROW: usize = 3;

impl MultiRename {
    pub fn new(entries: Vec<RenameEntry>) -> Self {
        let mut pattern = TextInput::new();
        pattern.set("[N].[E]".to_string());
        MultiRename {
            entries,
            fields: [pattern, TextInput::new(), TextInput::new()],
            focus: 0,
            case: CaseChange::Unchanged,
            scroll: 0,
            room: 1,
        }
    }

    /// Each entry's new name, and what stands in its way, if anything.
    pub fn preview(&self) -> Vec<(String, Option<RenameProblem>)> {
        // Each new name, and whether the pattern asked the file for what it
        // does not say.
        let (names, missing): (Vec<String>, Vec<bool>) = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let missing = std::cell::Cell::new(false);
                let lookup = |key: &str| {
                    let value = entry.meta.get_or_init(|| crate::metadata::read(&entry.path)).get(key).cloned();
                    missing.set(missing.get() || value.is_none());
                    value
                };
                let name = new_name_with(&entry.old, index, &self.fields[0].text, &self.fields[1].text, &self.fields[2].text, self.case, &lookup);
                (name, missing.get())
            })
            .unzip();
        let changing: Vec<bool> = self.entries.iter().zip(&names).map(|(entry, name)| *name != entry.old).collect();
        // One left as it is stays at its own path - not one made again from
        // its name as text, which for a name that is not UTF-8 is another.
        let targets: Vec<PathBuf> = self
            .entries
            .iter()
            .zip(&names)
            .zip(&changing)
            .map(|((entry, name), &changing)| if changing { entry.path.with_file_name(name) } else { entry.path.clone() })
            .collect();
        // What is moving away frees its name for another to take - told
        // apart as the filesystem tells names apart, as the rest are.
        let leaving: HashSet<OsString> = self.entries.iter().zip(&changing).filter(|(_, changing)| **changing).map(|(entry, _)| fold_for_fs(&entry.path)).collect();
        let mut seen: HashMap<OsString, usize> = HashMap::new();
        for target in &targets {
            *seen.entry(fold_for_fs(target)).or_default() += 1;
        }
        self.entries
            .iter()
            .zip(names)
            .zip(targets.iter().zip(changing))
            .zip(missing)
            .map(|(((entry, name), (target, changing)), lacking)| {
                let problem = if !changing {
                    // Left as it is, it is only in the way of another.
                    (seen[&fold_for_fs(target)] > 1).then_some(RenameProblem::Twice)
                } else if !entry.readable {
                    Some(RenameProblem::Unreadable)
                } else if lacking {
                    Some(RenameProblem::NoMetadata)
                } else if !crate::fs_ops::is_plain_name(&name) {
                    Some(RenameProblem::NotPlain)
                } else if seen[&fold_for_fs(target)] > 1 {
                    Some(RenameProblem::Twice)
                } else if !leaving.contains(&fold_for_fs(target)) && path_exists(target) && !crate::fs_ops::is_same_entry(&entry.path, target) {
                    Some(RenameProblem::Taken)
                } else {
                    None
                };
                (name, problem)
            })
            .collect()
    }
}

/// A path as the filesystem tells names apart: on Windows and macOS, in
/// either case alike.
pub(super) fn fold_for_fs(path: &Path) -> OsString {
    // There, names are text. Elsewhere they are bytes, compared as they are,
    // so two that only read alike as text stay apart.
    if cfg!(any(windows, target_os = "macos")) {
        path.to_string_lossy().to_lowercase().into()
    } else {
        path.as_os_str().to_os_string()
    }
}

/// `old`'s name and extension: the extension after the last dot, and none
/// for a name whose only dot starts it, as `.bashrc`. One ending in a dot,
/// as `file.`, has an extension with nothing in it - which is not none: the
/// dot is part of the name, and `[N].[E]` keeps it.
pub fn split_name(old: &str) -> (&str, Option<&str>) {
    match old.rsplit_once('.') {
        Some((name, extension)) if !name.is_empty() => (name, Some(extension)),
        _ => (old, None),
    }
}

/// The new name for `old`, the `index`th of those chosen. In `pattern`:
///
/// - `[N]` the name without its extension, `[E]` the extension, and either
///   with a range of letters: `[N2-5]`, `[N3-]` from the third on, `[N4]`
///   the fourth alone. `.[E]` with no extension leaves no dot behind.
/// - `[C]` a counter from 1, or `[C10]` from 10, `[C10+5]` by fives, and
///   `[C:3]` or `[C10+5:3]` at least three digits, from the front.
///
/// - `[Exif:date]`, `[Tag:artist]`, `[Pdf:title]` and the rest of
///   metadata::KNOWN, what the file says of itself, as `metadata` gives it.
///
/// Anything else in brackets is kept as it is written. Then every `find` is
/// replaced, and last the case changed.
#[cfg(test)]
pub fn new_name(old: &str, index: usize, pattern: &str, find: &str, replace: &str, case: CaseChange) -> String {
    new_name_with(old, index, pattern, find, replace, case, &|_| None)
}

/// new_name, with what the file says of itself to hand.
pub fn new_name_with(old: &str, index: usize, pattern: &str, find: &str, replace: &str, case: CaseChange, metadata: &dyn Fn(&str) -> Option<String>) -> String {
    let (name, extension) = split_name(old);
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let token = &after[..close];
        match expand(token, name, extension.unwrap_or_default(), index, metadata) {
            Some(text) => {
                if extension.is_none() && token == "E" && out.ends_with('.') {
                    out.pop();
                }
                out.push_str(&text);
            }
            None => {
                out.push('[');
                out.push_str(token);
                out.push(']');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    if !find.is_empty() {
        out = out.replace(find, replace);
    }
    case.apply(&out)
}

/// What a bracketed token stands for; None for one that is not a token.
fn expand(token: &str, name: &str, extension: &str, index: usize, metadata: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if token.contains(':') && !token.starts_with('C') {
        let key = crate::metadata::KNOWN.iter().find(|key| key.eq_ignore_ascii_case(token))?;
        return Some(metadata(key).map(|value| fit_for_name(&value)).unwrap_or_default());
    }
    let mut chars = token.chars();
    let kind = chars.next()?;
    let spec = chars.as_str();
    match kind {
        'N' => letters(name, spec),
        'E' => letters(extension, spec),
        'C' => counter(spec, index),
        _ => None,
    }
}

/// A value as a name can hold it: what no filesystem takes in a name - a
/// separator, Windows' reserved signs, a control character - made a dash.
fn fit_for_name(value: &str) -> String {
    let fit: String = value.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '-' } else { c }).collect();
    fit.trim().trim_end_matches('.').to_string()
}

/// `text`, or the letters of it a range picks: `2-5`, `3-`, `4`. Counted
/// from 1, and cut short where the text is.
fn letters(text: &str, range: &str) -> Option<String> {
    if range.is_empty() {
        return Some(text.to_string());
    }
    let number = |part: &str| part.parse::<usize>().ok().filter(|&number| number > 0);
    let (from, to) = match range.split_once('-') {
        Some((from, "")) => (number(from)?, usize::MAX),
        Some((from, to)) => (number(from)?, number(to)?),
        None => {
            let at = number(range)?;
            (at, at)
        }
    };
    Some(text.chars().skip(from - 1).take(to.saturating_sub(from - 1)).collect())
}

/// The longest name most filesystems allow, in bytes.
const MAX_NAME: usize = 255;

/// The counter for the `index`th entry: `[C]`, `[C10]`, `[C10+5]`, and any of
/// them with `:3` for at least three digits.
fn counter(spec: &str, index: usize) -> Option<String> {
    let (numbers, width) = match spec.split_once(':') {
        // No wider than a name can be: each keystroke and each frame makes
        // one for every entry, and [C:1000000000] would be gigabytes.
        Some((numbers, width)) => (numbers, width.parse::<usize>().ok().filter(|&width| width <= MAX_NAME)?),
        None => (spec, 0),
    };
    let (start, step): (u64, u64) = match numbers.split_once('+') {
        Some((start, step)) => (if start.is_empty() { 1 } else { start.parse().ok()? }, step.parse().ok()?),
        None => (if numbers.is_empty() { 1 } else { numbers.parse::<u64>().ok()? }, 1),
    };
    let value = start.saturating_add(step.saturating_mul(index as u64));
    Some(format!("{value:0width$}"))
}

impl AppState {
    /// F2 with a selection: the dialog, on what is selected.
    pub fn open_multi_rename(&mut self, items: Vec<PathBuf>) {
        let entries = items
            .into_iter()
            .filter_map(|path| {
                let name = path.file_name()?;
                Some(RenameEntry {
                    old: name.to_string_lossy().into_owned(),
                    readable: name.to_str().is_some(),
                    path,
                    meta: Default::default(),
                })
            })
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            self.dialog = Some(Dialog::MultiRename(Box::new(MultiRename::new(entries))));
        }
    }

    /// Tab and Shift+Tab, or Down and Up: the next field, or the one before,
    /// the case among them.
    pub fn multi_rename_switch_field(&mut self, forward: bool) {
        if let Some(Dialog::MultiRename(rename)) = &mut self.dialog {
            let count = CASE_ROW + 1;
            rename.focus = if forward { (rename.focus + 1) % count } else { (rename.focus + count - 1) % count };
        }
    }

    /// Left and Right, or Space, on the case row.
    pub fn multi_rename_case(&mut self, forward: bool) {
        if let Some(Dialog::MultiRename(rename)) = &mut self.dialog {
            let all = CaseChange::ALL;
            let at = all.iter().position(|&case| case == rename.case).unwrap_or(0);
            rename.case = all[if forward { (at + 1) % all.len() } else { (at + all.len() - 1) % all.len() }];
        }
    }

    /// Page Up and Page Down through the table.
    pub fn multi_rename_scroll(&mut self, step: isize) {
        if let Some(Dialog::MultiRename(rename)) = &mut self.dialog {
            let last = rename.entries.len().saturating_sub(rename.room);
            rename.scroll = rename.scroll.saturating_add_signed(step).min(last);
        }
    }

    /// Enter: every name changed, unless any of them cannot be, which keeps
    /// the dialog open on the table that says why. Names that go to where
    /// another one is leaving - `a` to `b` while `b` goes to `c`, or two
    /// swapped - are moved to a name of their own first and then on, so
    /// that none is written over on the way.
    pub fn apply_multi_rename(&mut self) {
        let Some(Dialog::MultiRename(rename)) = &self.dialog else {
            return;
        };
        let preview = rename.preview();
        if let Some(problem) = preview.iter().find_map(|(_, problem)| *problem) {
            let count = preview.iter().filter(|(_, problem)| problem.is_some()).count();
            self.display_error(tn!(
                count,
                "{count} of the new names cannot be had: {why} - see the table",
                "{count} of the new names cannot be had: {why} - see the table",
                count,
                why = problem.label()
            ));
            return;
        }
        let Some(Dialog::MultiRename(rename)) = self.dialog.take() else {
            return;
        };
        let moves: Vec<(PathBuf, PathBuf)> = rename
            .entries
            .iter()
            .zip(&preview)
            .filter(|(entry, (name, _))| *name != entry.old)
            .map(|(entry, (name, _))| (entry.path.clone(), entry.path.with_file_name(name)))
            .collect();
        let result = rename_all(&moves);

        let renamed = match &result {
            Ok(()) => moves.len(),
            Err((done, _)) => *done,
        };
        self.journal_renames(&moves[..renamed]);
        for (from, to) in moves.iter().take(renamed) {
            if let Some(size) = self.dir_sizes.remove(from) {
                self.dir_sizes.insert(to.clone(), size);
            }
        }
        self.feed_follow(&moves[..renamed]);
        // The cursor stays on the entry it was on, under its new name.
        let side = self.active_side;
        let panel = self.panel(side);
        let (children, state, dir) = (&panel.children, &panel.state, &panel.dir);
        let landing = state.selected().and_then(|index| children.get(index)).map(|item| {
            let at = item.path_in(dir);
            let now = moves.iter().take(renamed).find(|(from, _)| *from == at).map(|(_, to)| to.clone()).unwrap_or(at);
            now.strip_prefix(dir).map(|relative| relative.as_os_str().to_owned()).unwrap_or_else(|_| item.name_os.clone())
        });
        if result.is_ok() {
            self.clear_active_selections();
        }
        self.reload_panel(side, landing.as_deref());
        if let Err((done, e)) = result {
            self.display_error(t!("Renamed {done} of {total}, then: {e}", done, total = moves.len(), e));
        }
    }
}

impl AppState {
    /// Renames done, kept for Ctrl+Z to put back.
    pub(crate) fn journal_renames(&mut self, moves: &[(PathBuf, PathBuf)]) {
        let steps: Vec<Done> = moves
            .iter()
            .filter_map(|(from, to)| {
                let mark = Mark::of(to, false)?;
                Some(Done::Moved { from: from.clone(), to: to.clone(), mark })
            })
            .collect();
        self.scores.files_renamed += steps.len() as u64;
        self.scores.save();
        self.journal(UndoKind::Rename, steps);
    }
}

/// Every one of `moves` renamed, through scratch names when one goes to
/// where another is leaving. The error comes with how many were done.
pub(super) fn rename_all(moves: &[(PathBuf, PathBuf)]) -> Result<(), (usize, std::io::Error)> {
    // On a filesystem that does not tell case apart, `b` is in the way
    // of `a` going to `B` until it has gone itself.
    let sources: HashSet<OsString> = moves.iter().map(|(from, _)| fold_for_fs(from)).collect();
    let chained = moves.iter().any(|(from, to)| to != from && sources.contains(&fold_for_fs(to)));
    if chained { rename_through_scratch(moves) } else { rename_each(moves) }
}

/// `from` to `to`, refused when something else is there already. The preview
/// has said nothing is, but it tells names apart as the system usually does,
/// and a drive may not: a FAT stick on Linux takes `ax` and `Ax` for one
/// name, and a rename would write over the one that got there first.
fn rename_unless_taken(from: &Path, to: &Path) -> std::io::Result<()> {
    if path_exists(to) && !crate::fs_ops::is_same_entry(from, to) {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, t!("{path} already exists", path = to.display())));
    }
    crate::fs_ops::rename_path(from.to_path_buf(), to.to_path_buf())
}

/// Each straight to its new name, stopping at the first that fails. The
/// error comes with how many were done.
fn rename_each(moves: &[(PathBuf, PathBuf)]) -> Result<(), (usize, std::io::Error)> {
    for (done, (from, to)) in moves.iter().enumerate() {
        rename_unless_taken(from, to).map_err(|e| (done, e))?;
    }
    Ok(())
}

/// Each to a name nothing has first, then all of them on to their new
/// ones. One that fails on the way out puts back those already moved. One
/// that fails on the way in puts the rest back only where their old names
/// are still free - rename would write over whatever has moved in - and
/// leaves any other under its scratch name, to be seen and seen to.
fn rename_through_scratch(moves: &[(PathBuf, PathBuf)]) -> Result<(), (usize, std::io::Error)> {
    let mut parked: Vec<(PathBuf, &PathBuf, &PathBuf)> = Vec::new();
    for (index, (from, to)) in moves.iter().enumerate() {
        let mut scratch = from.with_file_name(format!(".fm84-rename-{}-{index}", std::process::id()));
        let mut tries = 0;
        while path_exists(&scratch) {
            tries += 1;
            scratch = from.with_file_name(format!(".fm84-rename-{}-{index}-{tries}", std::process::id()));
        }
        if let Err(e) = crate::fs_ops::rename_path(from.clone(), scratch.clone()) {
            for (scratch, from, _) in parked.iter().rev() {
                let _ = crate::fs_ops::rename_path(scratch.clone(), (*from).clone());
            }
            return Err((0, e));
        }
        parked.push((scratch, from, to));
    }
    for (done, (scratch, _, to)) in parked.iter().enumerate() {
        if let Err(e) = rename_unless_taken(scratch, to) {
            for (scratch, from, _) in &parked[done..] {
                if !path_exists(from) {
                    let _ = crate::fs_ops::rename_path(scratch.clone(), (*from).clone());
                }
            }
            return Err((done, e));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(old: &str, index: usize, pattern: &str) -> String {
        new_name(old, index, pattern, "", "", CaseChange::Unchanged)
    }

    #[test]
    fn patterns_take_the_name_the_extension_and_a_counter() {
        assert_eq!(named("photo.JPG", 0, "[N].[E]"), "photo.JPG");
        assert_eq!(named("archive.tar.gz", 0, "[N]-old.[E]"), "archive.tar-old.gz");
        // No extension, and no dot left behind for it.
        assert_eq!(named("Makefile", 0, "[N].[E]"), "Makefile");
        assert_eq!(named(".bashrc", 0, "[N].[E]"), ".bashrc");
        // An extension with nothing in it keeps its dot.
        assert_eq!(named("file.", 0, "[N].[E]"), "file.");
        assert_eq!(named("file.", 0, "[N]_old.[E]"), "file_old.");
        assert_eq!(named("x.txt", 0, "[E]_[N]"), "txt_x");
        // Letters by their place, counted from 1, cut short at the end.
        assert_eq!(named("abcdefg.txt", 0, "[N2-4]"), "bcd");
        assert_eq!(named("abcdefg.txt", 0, "[N5-]"), "efg");
        assert_eq!(named("abcdefg.txt", 0, "[N3]"), "c");
        assert_eq!(named("ab.txt", 0, "[N2-9]"), "b");
        assert_eq!(named("žvaka.txt", 0, "[N1-2]"), "žv");
        // Counters.
        assert_eq!(named("a.txt", 0, "[C].[E]"), "1.txt");
        assert_eq!(named("a.txt", 2, "[C].[E]"), "3.txt");
        assert_eq!(named("a.txt", 2, "[C10+5]"), "20");
        assert_eq!(named("a.txt", 4, "[C:3]_[N]"), "005_a");
        assert_eq!(named("a.txt", 1, "[C0+2:2]"), "02");
        // Wider than a name can be is not a counter.
        assert_eq!(named("a.txt", 0, "[C:255]").len(), 255);
        assert_eq!(named("a.txt", 0, "[C:256]"), "[C:256]");
        assert_eq!(named("a.txt", 0, "[C:99999999999]"), "[C:99999999999]");
        // What is not a token is kept as written.
        assert_eq!(named("a.txt", 0, "[N] [x] [N0] [C+] [unclosed"), "a [x] [N0] [C+] [unclosed");
    }

    /// What the file says of itself goes where its token is, made fit for
    /// a name; a token for nothing there is left as written.
    #[test]
    fn metadata_tokens_fill_from_the_file() {
        let lookup = |key: &str| match key {
            "exif:date" => Some("2026-03-14 18.02.11".to_string()),
            "tag:artist" => Some("AC/DC: Live?".to_string()),
            _ => None,
        };
        let named = |pattern: &str| new_name_with("IMG_0001.JPG", 0, pattern, "", "", CaseChange::Unchanged, &lookup);
        assert_eq!(named("[Exif:date].[E]"), "2026-03-14 18.02.11.JPG");
        assert_eq!(named("[tag:ARTIST] - [N]"), "AC-DC- Live- - IMG_0001");
        assert_eq!(named("[Exif:lens]"), "[Exif:lens]");
        assert_eq!(named("[Pdf:title]x"), "x");
    }

    /// A file that does not say what the pattern asks of it is refused, not
    /// renamed to what is left - `.jpg`, for a picture with no EXIF.
    #[test]
    fn a_file_without_the_metadata_asked_for_is_refused() {
        let dir = std::env::temp_dir().join(format!("fm84-rename-meta-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plain.jpg"), "not a photograph").unwrap();
        let entry = RenameEntry {
            path: dir.join("plain.jpg"),
            old: "plain.jpg".into(),
            readable: true,
            meta: Default::default(),
        };
        let mut rename = MultiRename::new(vec![entry]);
        rename.fields[0].set("[Exif:date].[E]".to_string());
        assert_eq!(rename.preview()[0].1, Some(RenameProblem::NoMetadata));
        rename.fields[0].set("[N]-1.[E]".to_string());
        assert_eq!(rename.preview()[0], ("plain-1.jpg".to_string(), None));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn find_replace_and_case_come_after_the_pattern() {
        assert_eq!(new_name("my_holiday_photo.JPG", 0, "[N].[E]", "_", " ", CaseChange::Title), "My Holiday Photo.Jpg");
        assert_eq!(new_name("Report.TXT", 0, "[N].[E]", "", "x", CaseChange::Lower), "report.txt");
        assert_eq!(new_name("report.txt", 0, "[N].[E]", "rep", "", CaseChange::Upper), "ORT.TXT");
        assert_eq!(new_name("hELLO wORLD.md", 0, "[N]", "", "", CaseChange::First), "Hello world");
        assert_eq!(new_name("it's done", 0, "[N]", "", "", CaseChange::Title), "It's Done");
    }
    /// A rename never writes over what is at its new name: where the preview
    /// could not see two clash - on a drive that does not tell case apart,
    /// or names alike but for how their accents are made up - the second is
    /// refused, and what the first put there stays.
    #[test]
    fn a_rename_never_writes_over_another() {
        let dir = std::env::temp_dir().join(format!("fm84-rename-no-clobber-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        type Renamer = fn(&[(PathBuf, PathBuf)]) -> Result<(), (usize, std::io::Error)>;
        for (how, rename) in [("each", rename_each as Renamer), ("through scratch", rename_through_scratch as Renamer)] {
            for name in ["one", "two"] {
                std::fs::write(dir.join(name), name).unwrap();
            }
            let moves = [(dir.join("one"), dir.join("same")), (dir.join("two"), dir.join("same"))];
            let result = rename(&moves);
            assert!(matches!(result, Err((1, ref e)) if e.kind() == std::io::ErrorKind::AlreadyExists), "{how}: {result:?}");
            assert_eq!(std::fs::read_to_string(dir.join("same")).unwrap(), "one", "{how}");
            assert_eq!(std::fs::read_to_string(dir.join("two")).unwrap(), "two", "{how}: put back");
            std::fs::remove_file(dir.join("same")).unwrap();
            std::fs::remove_file(dir.join("two")).unwrap();
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
    /// Two names that are not valid UTF-8 read alike as text, but are not the
    /// same name: left as they are, they are in nobody's way, and the rest
    /// can still be renamed.
    #[cfg(unix)]
    #[test]
    fn names_alike_only_as_text_do_not_clash() {
        use std::os::unix::ffi::OsStrExt;
        let dir = Path::new("/nowhere");
        let unreadable = |bytes: &[u8]| {
            let name = std::ffi::OsStr::from_bytes(bytes);
            RenameEntry {
                path: dir.join(name),
                old: name.to_string_lossy().into_owned(),
                readable: false,
                meta: Default::default(),
            }
        };
        let entries = vec![
            unreadable(b"a\xff"),
            unreadable(b"a\xfe"),
            RenameEntry {
                path: dir.join("b.txt"),
                old: "b.txt".into(),
                readable: true,
                meta: Default::default(),
            },
        ];
        let mut rename = MultiRename::new(entries);
        rename.fields[1].set("b".to_string());
        rename.fields[2].set("c".to_string());
        let preview = rename.preview();
        assert_eq!(preview[2].0, "c.txt");
        assert!(preview.iter().all(|(_, problem)| problem.is_none()), "{:?}", preview.iter().map(|(_, problem)| *problem).collect::<Vec<_>>());
    }
}
