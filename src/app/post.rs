//! F11's POST screen: fm84 starting as a PC of '84 did - its BIOS named,
//! the memory counted up to what the machine really has, and "Detecting
//! drives..." listing the real mounts one by one - then the panels. Any key
//! or click skips it; Del goes to F11, as SETUP.

use super::*;
use crate::fs_ops::{Mount, MountKind};

/// How long the memory takes to count up.
const COUNT: Duration = Duration::from_millis(900);
/// When "Detecting drives..." comes up.
const DRIVES_FROM: Duration = Duration::from_millis(1200);
/// How long each drive takes to be found.
const EACH_DRIVE: Duration = Duration::from_millis(220);
/// How long the screen stays once the last drive is found.
const HOLD: Duration = Duration::from_millis(900);

/// The POST screen showing, and what it reports.
pub struct Post {
    pub started: Instant,
    /// What the machine has, in KiB; None where it will not say.
    memory_kib: Option<u64>,
    /// Each drive's path and, where asking is safe, its size.
    drives: Vec<(String, Option<u64>)>,
}

/// A line of the POST screen, and what it is for how it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostLine {
    /// The BIOS's name.
    Head(String),
    Body(String),
}

impl Post {
    /// The screen for `memory_kib` and `mounts`. Home is no drive, and a
    /// network mount is not asked its size: a dead one would hold the start
    /// up until it answers.
    pub fn new(memory_kib: Option<u64>, mounts: &[Mount]) -> Self {
        let drives = mounts
            .iter()
            .filter(|mount| mount.kind != MountKind::Home)
            .map(|mount| {
                let size = (mount.kind != MountKind::Network).then(|| crate::fs_ops::disk_usage(&mount.path).map(|(_, total)| total)).flatten();
                (crate::utils::printable_name(&mount.path.to_string_lossy()), size)
            })
            .collect();
        Post { started: Instant::now(), memory_kib, drives }
    }

    /// How long the whole of it takes, the hold at the end included.
    pub fn length(&self) -> Duration {
        DRIVES_FROM + EACH_DRIVE * self.drives.len() as u32 + HOLD
    }

    /// What is on the screen `elapsed` after it started: the memory counted
    /// so far, and the drives found so far.
    pub fn lines(&self, elapsed: Duration) -> Vec<PostLine> {
        let mut lines = vec![
            PostLine::Head(format!("FM84 BIOS v{}", crate::constants::VERSION)),
            PostLine::Body("Copyright (c) 2025-2026 Colt Willcox".to_string()),
            PostLine::Body(String::new()),
        ];
        let counted = elapsed >= COUNT;
        let memory = match self.memory_kib {
            Some(total) => {
                // In steps of 64K, as the count went.
                let so_far = if counted { total } else { (total as u128 * elapsed.as_millis() / COUNT.as_millis()) as u64 / 64 * 64 };
                format!("{} {so_far}K", t!("Memory Test:"))
            }
            None => t!("Memory Test:").to_string(),
        };
        lines.push(PostLine::Body(if counted { format!("{memory} OK") } else { memory }));
        if elapsed < DRIVES_FROM {
            return lines;
        }
        lines.push(PostLine::Body(String::new()));
        lines.push(PostLine::Body(t!("Detecting drives...").to_string()));
        let found = ((elapsed - DRIVES_FROM).as_millis() / EACH_DRIVE.as_millis()) as usize;
        let width = self.drives.iter().map(|(path, _)| crate::utils::display_width(path)).max().unwrap_or(0);
        for (number, (path, size)) in self.drives.iter().take(found).enumerate() {
            let pad = " ".repeat(width - crate::utils::display_width(path));
            let size = size.map(crate::utils::format_size).unwrap_or_default();
            lines.push(PostLine::Body(format!("  [{}] {path}{pad}  {size}", number + 1).trim_end().to_string()));
        }
        lines
    }
}

/// The memory the machine has, in KiB.
#[cfg(unix)]
fn physical_memory_kib() -> Option<u64> {
    // SAFETY: sysconf only reads the system's settings.
    let (pages, page) = unsafe { (libc::sysconf(libc::_SC_PHYS_PAGES), libc::sysconf(libc::_SC_PAGESIZE)) };
    (pages > 0 && page > 0).then(|| pages as u64 * page as u64 / 1024)
}

#[cfg(not(unix))]
fn physical_memory_kib() -> Option<u64> {
    None
}

impl AppState {
    /// Start the POST screen, as fm84 starts with F11 asking for it.
    pub fn start_post(&mut self) {
        self.post = Some(Box::new(Post::new(physical_memory_kib(), &crate::fs_ops::list_mounts())));
    }

    /// Over once it has shown everything and held it a moment.
    pub fn poll_post(&mut self) {
        if self.post.as_ref().is_some_and(|post| post.started.elapsed() >= post.length()) {
            self.post = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(path: &str, kind: MountKind) -> Mount {
        Mount {
            path: PathBuf::from(path),
            label: String::new(),
            kind,
        }
    }

    fn text(lines: &[PostLine]) -> Vec<String> {
        lines
            .iter()
            .map(|line| match line {
                PostLine::Head(text) | PostLine::Body(text) => text.clone(),
            })
            .collect()
    }

    /// The memory counts up in steps of 64K to all of it, then says OK;
    /// the drives come one at a time, home left out; and it is over once
    /// the last has been on the screen a moment.
    #[test]
    fn the_post_counts_the_memory_then_finds_the_drives() {
        let mounts = [mount("/", MountKind::Disk), mount("/home/me", MountKind::Home), mount("/mnt/nas", MountKind::Network), mount("/media/stick", MountKind::Removable)];
        let post = Post::new(Some(16_384), &mounts);
        let at = |ms: u64| text(&post.lines(Duration::from_millis(ms)));

        let start = at(0);
        assert!(start[0].starts_with("FM84 BIOS v"));
        assert_eq!(start[3], "Memory Test: 0K");
        let half = at(450);
        assert_eq!(half[3], "Memory Test: 8192K");
        assert_eq!(half.len(), 4, "no drives yet");
        assert_eq!(at(900)[3], "Memory Test: 16384K OK");

        assert_eq!(at(1200)[5], "Detecting drives...");
        assert_eq!(at(1200).len(), 6, "none found yet");
        let all = at(1200 + 220 * 3);
        assert_eq!(all.len(), 9);
        assert!(all[6].starts_with("  [1] /     "), "{:?}", all[6]);
        assert_eq!(all[7], "  [2] /mnt/nas", "no size asked of a network mount");
        assert!(all[8].starts_with("  [3] /media/stick"));
        assert!(!all.iter().any(|line| line.contains("/home/me")));
        assert_eq!(post.length(), Duration::from_millis(1200 + 220 * 3 + 900));

        // A machine that will not say its memory is still tested.
        let quiet = Post::new(None, &[]);
        assert_eq!(text(&quiet.lines(Duration::from_secs(1)))[3], "Memory Test: OK");
    }
}
