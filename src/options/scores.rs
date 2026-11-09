//! F1's high-score table: what has been done with fm84, all told, and the
//! achievements it has come to. Counted as the jobs finish, and kept in the
//! `scores` file beside the config.

use super::*;

/// Everything counted, from the first run on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scores {
    pub bytes_copied: u64,
    pub bytes_moved: u64,
    pub files_renamed: u64,
    /// Entries deleted for good, and items moved to the trash.
    pub deleted: u64,
    /// Files Alt+D found to be copies of another, and their bytes.
    pub copies_found: u64,
    pub copy_bytes_found: u64,
    /// The longest a job ran, in seconds - finished, not cancelled.
    pub longest_job: u64,
    pub jobs: u64,
    pub undos: u64,
    /// Git revisions opened as a folder.
    pub revisions: u64,
}

/// A terabyte, as a drive's box counts one.
const TERABYTE: u64 = 1_000_000_000_000;
const GIGABYTE: u64 = 1_000_000_000;

/// One achievement: what it is called and how it is had, and whether
/// `scores` have it.
pub struct Achievement {
    pub name: &'static str,
    pub unlocked: bool,
}

impl Scores {
    /// Nothing counted yet: the table says INSERT COIN.
    pub fn is_empty(&self) -> bool {
        *self == Scores::default()
    }

    /// Every achievement, had or not, in the order they are listed.
    pub fn achievements(&self) -> Vec<Achievement> {
        let had = |name, unlocked| Achievement { name, unlocked };
        vec![
            had(t!("Player one - finish a job"), self.jobs > 0),
            had(t!("Second thoughts - undo something"), self.undos > 0),
            had(t!("Moved a terabyte - copy or move 1 TB"), self.bytes_copied.saturating_add(self.bytes_moved) >= TERABYTE),
            had(t!("Marathon - sit through a job of an hour"), self.longest_job >= 3600),
            had(t!("Spring cleaning - delete 1000 things"), self.deleted >= 1000),
            had(t!("Name dropper - rename 1000 files"), self.files_renamed >= 1000),
            had(t!("Déjà vu - find a gigabyte of copies"), self.copy_bytes_found >= GIGABYTE),
            had(t!("Time traveller - open a git revision"), self.revisions > 0),
        ]
    }

    /// What the `scores` file holds; nothing yet for a file not there.
    pub fn load() -> Self {
        let text = config_path("scores").and_then(|path| fs::read_to_string(path).ok()).unwrap_or_default();
        Self::parse(&text)
    }

    /// The `scores` file's figures; any it does not know are passed over.
    pub fn parse(text: &str) -> Self {
        let mut scores = Scores::default();
        for (key, value) in key_values(text) {
            let Ok(value) = value.parse::<u64>() else { continue };
            match key {
                "bytes_copied" => scores.bytes_copied = value,
                "bytes_moved" => scores.bytes_moved = value,
                "files_renamed" => scores.files_renamed = value,
                "deleted" => scores.deleted = value,
                "copies_found" => scores.copies_found = value,
                "copy_bytes_found" => scores.copy_bytes_found = value,
                "longest_job" => scores.longest_job = value,
                "jobs" => scores.jobs = value,
                "undos" => scores.undos = value,
                "revisions" => scores.revisions = value,
                _ => {}
            }
        }
        scores
    }

    /// Kept for next time. A failed write costs only the counts since.
    pub fn save(&self) {
        let _ = write_config_file(
            "scores",
            &format!(
                "# fm84's high scores, counted as the jobs finish.\n\
                 bytes_copied = {}\nbytes_moved = {}\nfiles_renamed = {}\ndeleted = {}\n\
                 copies_found = {}\ncopy_bytes_found = {}\nlongest_job = {}\njobs = {}\n\
                 undos = {}\nrevisions = {}\n",
                self.bytes_copied, self.bytes_moved, self.files_renamed, self.deleted, self.copies_found, self.copy_bytes_found, self.longest_job, self.jobs, self.undos, self.revisions
            ),
        );
    }
}
