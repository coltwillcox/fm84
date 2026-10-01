//! Finding text in the viewer and the editor. Both hold their text as lines
//! and place things by (line, character column), so the search works in those
//! terms and leaves turning a match into a selection, a cursor or a scroll to
//! each of them.

/// What Ctrl+F looks for. Smart case, as in less and vim: a term written all
/// in lower case matches either case, and one with a capital in it matches
/// exactly - "readme" finds README, "README" finds only that.
pub struct Needle {
    chars: Vec<char>,
    fold: bool,
}

impl Needle {
    /// None for an empty term, which would match everywhere.
    pub fn new(term: &str) -> Option<Self> {
        if term.is_empty() {
            return None;
        }
        let fold = !term.chars().any(char::is_uppercase);
        let chars = term.chars().map(|c| if fold { fold_char(c) } else { c }).collect();
        Some(Needle { chars, fold })
    }

    /// Every match in `line`, as character ranges, left to right and not
    /// overlapping - so "aa" in "aaaa" is found twice, as an editor would.
    pub fn find_all(&self, line: &str) -> Vec<(usize, usize)> {
        let haystack: Vec<char> = line.chars().map(|c| if self.fold { fold_char(c) } else { c }).collect();
        let length = self.chars.len();
        let mut found = Vec::new();
        let mut index = 0;
        while index + length <= haystack.len() {
            if haystack[index..index + length] == self.chars[..] {
                found.push((index, index + length));
                index += length;
            } else {
                index += 1;
            }
        }
        found
    }
}

/// Lower case one character for one character. A few have a lower case that
/// is two (İ is i and a combining dot); those are left as they are, so that a
/// column counted in the folded line is the same column in the real one.
fn fold_char(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

/// A match, in characters, and whether reaching it went round an end of the
/// text to get there.
#[derive(Debug, PartialEq, Eq)]
pub struct Found {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub wrapped: bool,
}

/// The match nearest `from` in the direction asked for, going round the end of
/// the text and back to where it started if it has to.
///
/// Forward takes the first match starting at `from` or after it, so searching
/// from the end of one match finds the one after. Backward takes the last
/// match starting before `from`, so searching from the start of one finds the
/// one before. A single match in the whole text is found again either way,
/// marked as having wrapped.
pub fn search(needle: &Needle, count: usize, line_at: impl Fn(usize) -> String, from: (usize, usize), forward: bool) -> Option<Found> {
    if count == 0 {
        return None;
    }
    let (from_line, from_col) = (from.0.min(count - 1), from.1);

    for step in 0..=count {
        let (line, wrapped) = if forward {
            ((from_line + step) % count, from_line + step >= count)
        } else {
            ((from_line + count - step) % count, step > from_line)
        };
        let matches = needle.find_all(&line_at(line));
        // The line the search starts on comes round twice: once for the part
        // ahead of `from`, and once at the end for the part behind it.
        let wanted = |&&(start, _): &&(usize, usize)| match (step, forward) {
            (0, true) => start >= from_col,
            (0, false) => start < from_col,
            (last, true) if last == count => start < from_col,
            (last, false) if last == count => start >= from_col,
            _ => true,
        };
        let hit = if forward { matches.iter().find(wanted) } else { matches.iter().rev().find(wanted) };
        if let Some(&(start, end)) = hit {
            return Some(Found { line, start, end, wrapped });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &'static str) -> (usize, impl Fn(usize) -> String) {
        let lines: Vec<&str> = text.lines().collect();
        (lines.len(), move |index| lines[index].to_string())
    }

    #[test]
    fn lower_case_finds_either_case_and_a_capital_finds_only_itself() {
        let needle = Needle::new("readme").unwrap();
        assert_eq!(needle.find_all("README and readme"), [(0, 6), (11, 17)]);
        let needle = Needle::new("README").unwrap();
        assert_eq!(needle.find_all("README and readme"), [(0, 6)]);
        assert!(Needle::new("").is_none());
    }

    #[test]
    fn columns_are_characters_not_bytes() {
        let needle = Needle::new("fé").unwrap();
        assert_eq!(needle.find_all("café fé"), [(2, 4), (5, 7)]);
        // A letter whose lower case is two characters does not shift the
        // columns after it.
        let needle = Needle::new("x").unwrap();
        assert_eq!(needle.find_all("İx"), [(1, 2)]);
    }

    #[test]
    fn forward_goes_on_from_the_end_of_a_match_and_wraps() {
        let (count, line_at) = lines("one two\ntwo\nthree two");
        let needle = Needle::new("two").unwrap();

        let first = search(&needle, count, &line_at, (0, 0), true).unwrap();
        assert_eq!(first, Found { line: 0, start: 4, end: 7, wrapped: false });
        let second = search(&needle, count, &line_at, (first.line, first.end), true).unwrap();
        assert_eq!(second, Found { line: 1, start: 0, end: 3, wrapped: false });
        let third = search(&needle, count, &line_at, (second.line, second.end), true).unwrap();
        assert_eq!(third, Found { line: 2, start: 6, end: 9, wrapped: false });
        let round = search(&needle, count, &line_at, (third.line, third.end), true).unwrap();
        assert_eq!(round, Found { line: 0, start: 4, end: 7, wrapped: true });
    }

    #[test]
    fn backward_goes_on_from_the_start_of_a_match_and_wraps() {
        let (count, line_at) = lines("one two\ntwo\nthree two");
        let needle = Needle::new("two").unwrap();

        let before = search(&needle, count, &line_at, (1, 0), false).unwrap();
        assert_eq!(before, Found { line: 0, start: 4, end: 7, wrapped: false });
        let round = search(&needle, count, &line_at, (before.line, before.start), false).unwrap();
        assert_eq!(round, Found { line: 2, start: 6, end: 9, wrapped: true });
    }

    #[test]
    fn a_lone_match_is_found_again_and_none_is_none() {
        let (count, line_at) = lines("a\nneedle here\nb");
        let needle = Needle::new("needle").unwrap();
        let only = search(&needle, count, &line_at, (1, 6), true).unwrap();
        assert_eq!(only, Found { line: 1, start: 0, end: 6, wrapped: true });
        let only = search(&needle, count, &line_at, (1, 0), false).unwrap();
        assert_eq!(only, Found { line: 1, start: 0, end: 6, wrapped: true });

        let missing = Needle::new("absent").unwrap();
        assert_eq!(search(&missing, count, &line_at, (0, 0), true), None);
        assert_eq!(search(&missing, 0, |_| String::new(), (0, 0), true), None);
    }
}
