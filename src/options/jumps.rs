//! Alt+J's jumps: every directory the panels have gone to, kept in the
//! `jumps` file beside the config with how often and how lately, so a few
//! letters of one bring it back - most used and most recent first, as
//! zoxide and autojump rank them.

use super::*;
use std::path::Path;

/// One directory: how much it has been gone to, which fades with age, and
/// when last, in seconds since 1970.
#[derive(Clone, Debug, PartialEq)]
pub struct Jump {
    pub path: PathBuf,
    pub rank: f64,
    pub last: u64,
}

/// Every directory gone to, and what this run did to them - to be put onto
/// the file as it is when saved, so two fm84s at once both count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Jumps {
    pub entries: Vec<Jump>,
    visits: Vec<(PathBuf, u64)>,
    forgotten: Vec<PathBuf>,
}

/// Past this many visits in all, every rank is scaled down to fit and the
/// ones left under a visit dropped: the file stays small, and a directory
/// once busy and long left gives way to the ones in use now.
const MOST: f64 = 10_000.0;

/// What a visit counts for, by how long ago `last` is from `now`: four times
/// within the hour, twice within the day, half within the week, a quarter
/// after that.
pub fn frecency(rank: f64, last: u64, now: u64) -> f64 {
    let age = now.saturating_sub(last);
    rank * match age {
        0..3_600 => 4.0,
        3_600..86_400 => 2.0,
        86_400..604_800 => 0.5,
        _ => 0.25,
    }
}

/// The seconds since 1970 now.
pub fn now_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

impl Jumps {
    /// What the `jumps` file holds; none for a file not there.
    pub fn load() -> Self {
        let text = config_path("jumps").and_then(|path| fs::read_to_string(path).ok()).unwrap_or_default();
        Self::parse(&text)
    }

    /// `jump = rank last path` lines; any that is not one passed over.
    pub fn parse(text: &str) -> Self {
        let entries = key_values(text)
            .filter(|(key, _)| *key == "jump")
            .filter_map(|(_, value)| {
                let mut parts = value.splitn(3, ' ');
                let rank: f64 = parts.next()?.parse().ok().filter(|rank: &f64| rank.is_finite() && *rank > 0.0)?;
                let last = parts.next()?.parse().ok()?;
                let path = unescape_path(parts.next()?.trim());
                (!path.as_os_str().is_empty()).then_some(Jump { path, rank, last })
            })
            .collect();
        Jumps { entries, ..Jumps::default() }
    }

    pub fn serialize(&self) -> String {
        let mut text = String::from("# fm84's jumps, for Alt+J: each directory gone to, how much and when last.\n");
        for jump in &self.entries {
            text.push_str(&format!("jump = {} {} {}\n", jump.rank, jump.last, escape_path(&jump.path)));
        }
        text
    }

    /// A panel has gone to `path`.
    pub fn visit(&mut self, path: &Path, now: u64) {
        self.visits.push((path.to_path_buf(), now));
        self.add(path, now);
    }

    fn add(&mut self, path: &Path, now: u64) {
        match self.entries.iter_mut().find(|jump| jump.path == path) {
            Some(jump) => {
                jump.rank += 1.0;
                jump.last = jump.last.max(now);
            }
            None => self.entries.push(Jump { path: path.to_path_buf(), rank: 1.0, last: now }),
        }
        let total: f64 = self.entries.iter().map(|jump| jump.rank).sum();
        if total > MOST {
            let scale = 0.9 * MOST / total;
            self.entries.iter_mut().for_each(|jump| jump.rank *= scale);
            self.entries.retain(|jump| jump.rank >= 1.0);
        }
    }

    /// Take `path` off: it is not there any more.
    pub fn forget(&mut self, path: &Path) {
        if self.entries.iter().any(|jump| jump.path == path) {
            self.entries.retain(|jump| jump.path != path);
            self.forgotten.push(path.to_path_buf());
        }
    }

    /// The directories, the one to jump to first.
    pub fn ranked(&self, now: u64) -> Vec<PathBuf> {
        let mut scored: Vec<(f64, &Jump)> = self.entries.iter().map(|jump| (frecency(jump.rank, jump.last, now), jump)).collect();
        scored.sort_by(|(a, first), (b, second)| b.total_cmp(a).then(second.last.cmp(&first.last)).then_with(|| first.path.cmp(&second.path)));
        scored.into_iter().map(|(_, jump)| jump.path.clone()).collect()
    }

    /// Written onto the file as another fm84 may have left it since this one
    /// read it: this run's visits counted again there, what it forgot taken
    /// off.
    pub fn save(&self) -> io::Result<()> {
        if cfg!(test) || (self.visits.is_empty() && self.forgotten.is_empty()) {
            return Ok(());
        }
        let mut fresh = Self::load();
        for path in &self.forgotten {
            fresh.entries.retain(|jump| jump.path != *path);
        }
        for (path, at) in &self.visits {
            fresh.add(path, *at);
        }
        write_config_file("jumps", &fresh.serialize())
    }
}

/// Whether `path` is one the words typed ask for, as zoxide matches: each
/// word in the path, in the order typed, and the last of them in its last
/// name - `pro fm` is `~/programs/fm84`, but not `~/fm84/programs`. Smart
/// case: words all in lower case match either case.
pub fn jump_matches(path: &Path, words: &[&str]) -> bool {
    let Some(&last) = words.last() else {
        return true;
    };
    let fold = !words.iter().any(|word| word.chars().any(char::is_uppercase));
    let folded = |text: &str| -> String { if fold { text.chars().map(crate::find::fold_char).collect() } else { text.to_string() } };
    let text = folded(&path.to_string_lossy());
    let mut from = 0;
    for word in &words[..words.len() - 1] {
        let word = folded(word);
        match text[from..].find(&word) {
            Some(at) => from += at + word.len(),
            None => return false,
        }
    }
    // The last word within the last name, after whatever matched before it.
    let name_start = text.rfind(std::path::is_separator).map_or(0, |at| at + 1);
    text[from.max(name_start)..].contains(&folded(last))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3_600;
    const DAY: u64 = 86_400;

    #[test]
    fn the_most_used_lately_comes_first() {
        let now = 100 * DAY;
        let mut jumps = Jumps::default();
        // Gone to often, but a month ago; once, just now; twice, yesterday.
        for _ in 0..10 {
            jumps.visit(Path::new("/old/busy"), now - 30 * DAY);
        }
        jumps.visit(Path::new("/new"), now - 60);
        jumps.visit(Path::new("/yesterday"), now - DAY - HOUR);
        jumps.visit(Path::new("/yesterday"), now - DAY - HOUR);
        // 10 x 0.25 = 2.5, 1 x 4 = 4, 2 x 0.5 = 1.
        assert_eq!(jumps.ranked(now), [PathBuf::from("/new"), PathBuf::from("/old/busy"), PathBuf::from("/yesterday")]);
        assert_eq!(jumps.entries.iter().find(|jump| jump.path == Path::new("/old/busy")).unwrap().rank, 10.0);

        jumps.forget(Path::new("/new"));
        assert_eq!(jumps.ranked(now)[0], PathBuf::from("/old/busy"));
    }

    #[test]
    fn too_many_visits_fade_and_the_least_go() {
        let mut jumps = Jumps::default();
        jumps.visit(Path::new("/once"), 0);
        for _ in 0..MOST as usize {
            jumps.visit(Path::new("/always"), 0);
        }
        let total: f64 = jumps.entries.iter().map(|jump| jump.rank).sum();
        assert!(total <= MOST, "{total}");
        assert_eq!(jumps.entries.len(), 1, "the one visited once is let go");
    }

    #[test]
    fn the_file_reads_back_what_it_wrote() {
        let mut jumps = Jumps::default();
        jumps.visit(Path::new("/home/me/My Music"), 1_700_000_000);
        jumps.visit(Path::new(" spaced "), 1_700_000_001);
        jumps.visit(Path::new(" spaced "), 1_700_000_002);
        let read = Jumps::parse(&jumps.serialize());
        assert_eq!(read.entries, jumps.entries);
        // A line that is not one is passed over.
        assert!(Jumps::parse("jump = x 1 /a\njump = 1 /b\njump = -1 2 /c\njump = 1 2 \n").entries.is_empty());
    }

    #[test]
    fn words_match_in_order_and_the_last_in_the_last_name() {
        let matches = |path: &str, typed: &str| jump_matches(Path::new(path), &typed.split_whitespace().collect::<Vec<_>>());
        assert!(matches("/home/me/programs/fm84", ""));
        assert!(matches("/home/me/programs/fm84", "fm"));
        assert!(matches("/home/me/programs/fm84", "pro fm"));
        assert!(!matches("/home/me/fm84/programs", "pro fm"), "out of order");
        assert!(!matches("/home/me/programs/fm84", "pro"), "the last word not in the last name");
        assert!(matches("/home/me/Programs/fm84", "programs 84"), "lower case matches either");
        assert!(!matches("/home/me/programs/fm84", "Programs 84"), "a capital only itself");
        assert!(matches("/home/me/programs/fm84", "fm 84"), "two words in the last name");
        assert!(!matches("/home/me/programs/fm84", "84 fm"));
    }
}
