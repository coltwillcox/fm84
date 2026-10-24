//! Shift+F3: two files side by side, line against line, with what differs
//! marked. The diff is Myers', the one `diff` and git use by default, run on
//! lines after the ends the files share are set aside.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Past this many lines added and removed between the shared ends, the rest
/// is shown as one block replaced rather than worked out line by line. Myers
/// keeps a step of history per edit, which grows with the square of them -
/// about 34 MB at this many - and two files this different have little left
/// to line up anyway.
const MAX_EDITS: usize = 2048;

/// Lines of the same text kept above a difference jumped to, so it is seen
/// in its place rather than at the very top.
pub const CONTEXT: usize = 3;

/// One row of the two columns: a line from each side, or from one with a gap
/// facing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub left: Option<usize>,
    pub right: Option<usize>,
    /// The two lines are the same text.
    pub same: bool,
}

impl Row {
    /// The mark between the columns, as `diff -y` writes it.
    pub fn marker(&self) -> char {
        match (self.left, self.right) {
            _ if self.same => ' ',
            (Some(_), Some(_)) => '|',
            (Some(_), None) => '<',
            _ => '>',
        }
    }
}

pub struct DiffState {
    pub left_path: PathBuf,
    pub right_path: PathBuf,
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub rows: Vec<Row>,
    /// The row each run of differing rows starts on, top to bottom.
    pub hunks: Vec<usize>,
    /// The hunk last jumped to, which N and P step on from.
    pub current: Option<usize>,
    pub scroll_offset: usize,
    pub horizontal_offset: usize,
    /// The widest line on either side, as drawn, so scrolling sideways stops
    /// at the end of it.
    pub max_line_width: usize,
}

impl DiffState {
    pub fn new(left_path: PathBuf, right_path: PathBuf, left: Vec<String>, right: Vec<String>) -> Self {
        let rows = rows(&left, &right);
        let hunks = rows.iter().enumerate().filter(|&(index, row)| !row.same && (index == 0 || rows[index - 1].same)).map(|(index, _)| index).collect();
        let max_line_width = left.iter().chain(&right).map(|line| crate::utils::line_display_width(line)).max().unwrap_or(0);
        let mut state = DiffState {
            left_path,
            right_path,
            left,
            right,
            rows,
            hunks,
            current: None,
            scroll_offset: 0,
            horizontal_offset: 0,
            max_line_width,
        };
        state.step(true);
        state
    }

    /// Jump to the next difference, or the previous one, from the one last
    /// jumped to. Stops at the ends.
    pub fn step(&mut self, forward: bool) {
        let next = match (self.current, forward) {
            (None, _) => 0,
            (Some(at), true) => at + 1,
            (Some(at), false) => match at.checked_sub(1) {
                Some(previous) => previous,
                None => return,
            },
        };
        if let Some(&row) = self.hunks.get(next) {
            self.current = Some(next);
            self.scroll_offset = row.saturating_sub(CONTEXT);
        }
    }
}

/// What comparing two files came to.
pub enum Compared {
    /// Byte for byte the same.
    Identical,
    /// The same lines, ended differently - CRLF against LF, or one without a
    /// newline at the end.
    LineEndings,
    /// Not text, and different from the byte given on; and whether the sizes
    /// differ too.
    Binary {
        at: u64,
        sizes_differ: bool,
    },
    Text(Box<DiffState>),
}

/// Read both files and compare them. `limit` is the most of either one to
/// read, as the viewer asks before opening more.
pub fn compare(left_path: &Path, right_path: &Path, limit: u64) -> Result<Compared, String> {
    let read = |path: &Path| -> Result<Vec<u8>, String> {
        let metadata = std::fs::metadata(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        // A pipe or a device could block for good, or never end.
        if !metadata.is_file() {
            return Err(format!("Not a regular file: {}", path.display()));
        }
        if metadata.len() > limit {
            return Err(format!("Too large to compare: {} ({}; F11 sets the limit)", path.display(), crate::utils::format_size(metadata.len())));
        }
        std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))
    };
    let (left_bytes, right_bytes) = (read(left_path)?, read(right_path)?);
    if left_bytes == right_bytes {
        return Ok(Compared::Identical);
    }
    let (Ok(left_text), Ok(right_text)) = (std::str::from_utf8(&left_bytes), std::str::from_utf8(&right_bytes)) else {
        return Ok(binary(&left_bytes, &right_bytes));
    };
    if left_text.contains('\0') || right_text.contains('\0') {
        return Ok(binary(&left_bytes, &right_bytes));
    }
    let lines = |text: &str| text.lines().map(str::to_string).collect::<Vec<_>>();
    let state = DiffState::new(left_path.to_path_buf(), right_path.to_path_buf(), lines(left_text), lines(right_text));
    if state.hunks.is_empty() {
        return Ok(Compared::LineEndings);
    }
    Ok(Compared::Text(Box::new(state)))
}

fn binary(left: &[u8], right: &[u8]) -> Compared {
    let at = left.iter().zip(right).position(|(a, b)| a != b).unwrap_or(left.len().min(right.len()));
    Compared::Binary {
        at: at as u64,
        sizes_differ: left.len() != right.len(),
    }
}

/// What a step of the diff does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edit {
    Same,
    /// A line only the left has.
    Delete,
    /// A line only the right has.
    Insert,
}

/// The rows the two columns are drawn from. Within each run of differences,
/// lines removed and lines added are paired off side by side, as changed;
/// whichever side has more carries on alone.
fn rows(left: &[String], right: &[String]) -> Vec<Row> {
    let mut rows = Vec::with_capacity(left.len().max(right.len()));
    let (mut x, mut y) = (0, 0);
    let (mut deleted, mut inserted) = (Vec::new(), Vec::new());
    let flush = |rows: &mut Vec<Row>, deleted: &mut Vec<usize>, inserted: &mut Vec<usize>| {
        for index in 0..deleted.len().max(inserted.len()) {
            rows.push(Row {
                left: deleted.get(index).copied(),
                right: inserted.get(index).copied(),
                same: false,
            });
        }
        deleted.clear();
        inserted.clear();
    };
    for edit in edits(left, right) {
        match edit {
            Edit::Same => {
                flush(&mut rows, &mut deleted, &mut inserted);
                rows.push(Row { left: Some(x), right: Some(y), same: true });
                x += 1;
                y += 1;
            }
            Edit::Delete => {
                deleted.push(x);
                x += 1;
            }
            Edit::Insert => {
                inserted.push(y);
                y += 1;
            }
        }
    }
    flush(&mut rows, &mut deleted, &mut inserted);
    rows
}

/// The edits that turn `left` into `right`, fewest first. The lines both
/// start and end with are taken as they are, and only what lies between is
/// diffed - most changes to a file are a small part of it.
fn edits(left: &[String], right: &[String]) -> Vec<Edit> {
    let prefix = left.iter().zip(right).take_while(|(a, b)| a == b).count();
    let suffix = left[prefix..].iter().rev().zip(right[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
    let (left_middle, right_middle) = (&left[prefix..left.len() - suffix], &right[prefix..right.len() - suffix]);

    // Compared as numbers, one for each different line, rather than as text.
    fn numbered<'a>(lines: &'a [String], ids: &mut HashMap<&'a str, u32>) -> Vec<u32> {
        lines
            .iter()
            .map(|line| {
                let next = ids.len() as u32;
                *ids.entry(line.as_str()).or_insert(next)
            })
            .collect()
    }
    let mut ids = HashMap::new();
    let a = numbered(left_middle, &mut ids);
    let b = numbered(right_middle, &mut ids);

    let middle = myers(&a, &b).unwrap_or_else(|| {
        let mut replaced = vec![Edit::Delete; a.len()];
        replaced.extend(std::iter::repeat_n(Edit::Insert, b.len()));
        replaced
    });
    let mut all = vec![Edit::Same; prefix];
    all.extend(middle);
    all.extend(std::iter::repeat_n(Edit::Same, suffix));
    all
}

/// Myers' O(ND) diff: the shortest run of edits from `a` to `b`, or None when
/// it would take more than MAX_EDITS. Each step `d` finds, for every diagonal
/// `k` it can reach, how far along `a` the furthest path with `d` edits gets;
/// the steps are kept so the path can be traced back from the end.
fn myers(a: &[u32], b: &[u32]) -> Option<Vec<Edit>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    let offset = max as isize + 1;
    let mut v = vec![0isize; 2 * max + 3];
    // trace[d] holds v[-d..=d] as it stood after step d.
    let mut trace: Vec<Vec<isize>> = Vec::new();

    'search: for d in 0..=max as isize {
        if d as usize > MAX_EDITS {
            return None;
        }
        for k in (-d..=d).step_by(2) {
            let index = (offset + k) as usize;
            let mut x = if k == -d || (k != d && v[index - 1] < v[index + 1]) { v[index + 1] } else { v[index - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[index] = x;
            if x >= n && y >= m {
                trace.push(v[(offset - d) as usize..=(offset + d) as usize].to_vec());
                break 'search;
            }
        }
        trace.push(v[(offset - d) as usize..=(offset + d) as usize].to_vec());
    }

    let mut edits = Vec::with_capacity(max);
    let (mut x, mut y) = (n, m);
    for d in (1..trace.len() as isize).rev() {
        let previous = &trace[d as usize - 1];
        let at = |k: isize| previous[(k + d - 1) as usize];
        let k = x - y;
        let previous_k = if k == -d || (k != d && at(k - 1) < at(k + 1)) { k + 1 } else { k - 1 };
        let previous_x = at(previous_k);
        let previous_y = previous_x - previous_k;
        while x > previous_x && y > previous_y {
            edits.push(Edit::Same);
            x -= 1;
            y -= 1;
        }
        if x == previous_x {
            edits.push(Edit::Insert);
            y -= 1;
        } else {
            edits.push(Edit::Delete);
            x -= 1;
        }
    }
    edits.extend(std::iter::repeat_n(Edit::Same, x as usize));
    edits.reverse();
    Some(edits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split(' ').filter(|line| !line.is_empty()).map(str::to_string).collect()
    }

    /// Apply the edits to `left` and check they make `right`, and that no
    /// shorter set would - against the length of the longest common
    /// subsequence, worked out the slow way.
    fn check(left: &str, right: &str) {
        let (a, b) = (lines(left), lines(right));
        let edits = edits(&a, &b);
        let (mut x, mut y, mut made) = (0, 0, Vec::new());
        for edit in &edits {
            match edit {
                Edit::Same => {
                    assert_eq!(a[x], b[y], "{left} -> {right}");
                    made.push(a[x].clone());
                    x += 1;
                    y += 1;
                }
                Edit::Delete => x += 1,
                Edit::Insert => {
                    made.push(b[y].clone());
                    y += 1;
                }
            }
        }
        assert_eq!((x, y, &made), (a.len(), b.len(), &b), "{left} -> {right}");

        let mut lcs = vec![vec![0; b.len() + 1]; a.len() + 1];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
            }
        }
        let same = edits.iter().filter(|&&edit| edit == Edit::Same).count();
        assert_eq!(same, lcs[0][0], "{left} -> {right}: not the fewest edits");
    }

    #[test]
    fn the_edits_are_right_and_fewest() {
        for (left, right) in [
            ("", ""),
            ("a", ""),
            ("", "a"),
            ("a b c", "a b c"),
            ("a b c a b b a", "c b a b a c"),
            ("a b c d", "x a b y d z"),
            ("x y z", "a b c"),
            ("a a a b", "b a a a"),
            ("p q r s t u v", "p r s x t v w"),
        ] {
            check(left, right);
            check(right, left);
        }
        // And a few hundred more, made up from a small alphabet so lines
        // repeat and line up in more than one way.
        let mut seed: u32 = 84;
        let mut next = |below: u32| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 16) % below
        };
        for _ in 0..500 {
            let mut make = || (0..next(12)).map(|_| ["a", "b", "c"][next(3) as usize]).collect::<Vec<_>>().join(" ");
            let (left, right) = (make(), make());
            check(&left, &right);
        }
    }

    #[test]
    fn changes_pair_off_and_the_rest_stands_alone() {
        let rows = rows(&lines("a b c d"), &lines("a x y d e"));
        let shown: Vec<(Option<usize>, Option<usize>, char)> = rows.iter().map(|row| (row.left, row.right, row.marker())).collect();
        assert_eq!(shown, [(Some(0), Some(0), ' '), (Some(1), Some(1), '|'), (Some(2), Some(2), '|'), (Some(3), Some(3), ' '), (None, Some(4), '>'),]);
        let state = DiffState::new(PathBuf::new(), PathBuf::new(), lines("a b c d"), lines("a x y d e"));
        assert_eq!(state.hunks, [1, 4]);
    }

    #[test]
    fn stepping_goes_hunk_to_hunk_and_stops_at_the_ends() {
        let left: Vec<String> = (0..100).map(|n| n.to_string()).collect();
        let mut right = left.clone();
        right[10] = "ten".into();
        right[50] = "fifty".into();
        let mut state = DiffState::new(PathBuf::new(), PathBuf::new(), left, right);
        assert_eq!((state.current, state.scroll_offset), (Some(0), 10 - CONTEXT), "opens on the first");
        state.step(true);
        assert_eq!((state.current, state.scroll_offset), (Some(1), 50 - CONTEXT));
        state.step(true);
        assert_eq!(state.current, Some(1), "stays on the last");
        state.step(false);
        state.step(false);
        assert_eq!(state.current, Some(0), "stays on the first");
    }

    #[test]
    fn too_many_edits_are_one_block_replaced() {
        let a: Vec<u32> = (0..3000).collect();
        let b: Vec<u32> = (10_000..13_000).collect();
        assert!(myers(&a, &b).is_none());
        let (left, right): (Vec<String>, Vec<String>) = (a.iter().map(u32::to_string).collect(), b.iter().map(u32::to_string).collect());
        let edits = edits(&left, &right);
        assert_eq!(edits.len(), 6000);
        assert!(edits[..3000].iter().all(|&edit| edit == Edit::Delete));
    }

    #[test]
    fn files_are_told_apart_by_what_differs() {
        let dir = std::env::temp_dir().join(format!("fm84-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, bytes: &[u8]| {
            std::fs::write(dir.join(name), bytes).unwrap();
            dir.join(name)
        };
        let unix = file("unix.txt", b"one\ntwo\n");
        let same = file("same.txt", b"one\ntwo\n");
        let dos = file("dos.txt", b"one\r\ntwo");
        let other = file("other.txt", b"one\nthree\n");
        let binary = file("binary.bin", b"\0\x01\x02\x03");
        let longer = file("longer.bin", b"\0\x01\x09\x03\x04");

        assert!(matches!(compare(&unix, &same, 1024), Ok(Compared::Identical)));
        assert!(matches!(compare(&unix, &dos, 1024), Ok(Compared::LineEndings)));
        assert!(matches!(compare(&binary, &longer, 1024), Ok(Compared::Binary { at: 2, sizes_differ: true })));
        assert!(matches!(compare(&unix, &binary, 1024), Ok(Compared::Binary { at: 0, .. })));
        let Ok(Compared::Text(state)) = compare(&unix, &other, 1024) else { panic!("not a text diff") };
        assert_eq!(state.rows[1], Row { left: Some(1), right: Some(1), same: false });
        assert!(matches!(compare(&unix, &other, 4), Err(e) if e.starts_with("Too large to compare")));
        assert!(matches!(compare(&unix, &dir, 1024), Err(e) if e.starts_with("Not a regular file")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
