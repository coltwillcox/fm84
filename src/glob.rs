//! Shell-style patterns, for selecting files by name with + and -. Small
//! enough to write rather than bring in a crate for.

/// One or more patterns, separated by `;` - "*.jpg;*.png". `*` is any run of
/// characters, `?` any one, `[abc]` and `[a-z]` one of those, `[!abc]` one
/// that is not. Unlike the shell's, `*` matches a leading dot too: hidden
/// files are files like any other here.
///
/// Smart case, as Find is: all in lower case matches either case, and a
/// capital anywhere makes the match exact.
///
/// A pattern ending in `/` is for directories - `*/` is every one of them -
/// and one without is for files, so that `+` then Enter on `*` takes the files
/// and leaves the directories, whose selection would sweep whole trees into
/// the next copy or delete.
pub struct Glob {
    patterns: Vec<(Vec<char>, bool)>,
    fold: bool,
}

impl Glob {
    /// None when there is no pattern in it at all.
    pub fn new(text: &str) -> Option<Self> {
        let fold = !text.chars().any(char::is_uppercase);
        let patterns: Vec<(Vec<char>, bool)> = text
            .split(';')
            .map(str::trim)
            .filter(|pattern| !pattern.is_empty())
            .map(|pattern| match pattern.strip_suffix('/') {
                Some(directory) => (directory, true),
                None => (pattern, false),
            })
            .filter(|(pattern, _)| !pattern.is_empty())
            .map(|(pattern, for_dirs)| (pattern.chars().map(|c| fold_if(c, fold)).collect(), for_dirs))
            .collect();
        if patterns.is_empty() { None } else { Some(Glob { patterns, fold }) }
    }

    pub fn matches(&self, name: &str, is_dir: bool) -> bool {
        let name: Vec<char> = name.chars().map(|c| fold_if(c, self.fold)).collect();
        self.patterns.iter().any(|(pattern, for_dirs)| *for_dirs == is_dir && matches(pattern, &name))
    }
}

fn fold_if(c: char, fold: bool) -> char {
    if fold { crate::find::fold_char(c) } else { c }
}

/// The usual backtracking match: on a mismatch, let the last `*` take one more
/// character and carry on from there. Linear in practice, and never worse
/// than the pattern's length times the name's.
fn matches(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() {
            match pattern[p] {
                '*' => {
                    star = Some((p, n));
                    p += 1;
                    continue;
                }
                '?' => {
                    p += 1;
                    n += 1;
                    continue;
                }
                '[' => {
                    if let Some((hit, next)) = class(pattern, p, name[n]) {
                        if hit {
                            p = next;
                            n += 1;
                            continue;
                        }
                    } else if name[n] == '[' {
                        // Never closed, so it is only a bracket.
                        p += 1;
                        n += 1;
                        continue;
                    }
                }
                c if c == name[n] => {
                    p += 1;
                    n += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((star_p, star_n)) => {
                star = Some((star_p, star_n + 1));
                p = star_p + 1;
                n = star_n + 1;
            }
            None => return false,
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Whether `c` is in the class opening at `pattern[open]`, and where the
/// pattern goes on after it. None when the class is never closed.
fn class(pattern: &[char], open: usize, c: char) -> Option<(bool, usize)> {
    let mut index = open + 1;
    let negated = matches!(pattern.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }
    let mut hit = false;
    let mut first = true;
    loop {
        let &start = pattern.get(index)?;
        // A ] straight after the opening is one of the class, not its end.
        if start == ']' && !first {
            return Some((hit != negated, index + 1));
        }
        first = false;
        if pattern.get(index + 1) == Some(&'-') && pattern.get(index + 2).is_some_and(|&end| end != ']') {
            hit |= (start..=pattern[index + 2]).contains(&c);
            index += 3;
        } else {
            hit |= start == c;
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(pattern: &str, name: &str) -> bool {
        Glob::new(pattern).unwrap().matches(name, false)
    }

    #[test]
    fn stars_and_question_marks() {
        assert!(file("*", "anything"));
        assert!(file("*", ".hidden"));
        assert!(file("*.rs", "main.rs"));
        assert!(!file("*.rs", "main.rsx"));
        assert!(file("a*b*c", "aXXbYYc"));
        assert!(!file("a*b*c", "aXXbYY"));
        assert!(file("?.txt", "a.txt"));
        assert!(!file("?.txt", "ab.txt"));
        assert!(file("**x", "x"));
    }

    #[test]
    fn classes() {
        assert!(file("[abc].txt", "b.txt"));
        assert!(!file("[abc].txt", "d.txt"));
        assert!(file("file[0-9]", "file7"));
        assert!(!file("file[!0-9]", "file7"));
        assert!(file("file[!0-9]", "fileX"));
        assert!(file("[]]", "]"));
        assert!(file("[a-]", "-"));
        // Never closed, so a plain bracket.
        assert!(file("[ab", "[ab"));
    }

    #[test]
    fn smart_case_as_find_does() {
        assert!(file("*.jpg", "PHOTO.JPG"));
        assert!(file("*.JPG", "PHOTO.JPG"));
        assert!(!file("*.JPG", "photo.jpg"));
    }

    #[test]
    fn several_patterns_and_directories_apart() {
        let glob = Glob::new("*.jpg; *.png").unwrap();
        assert!(glob.matches("a.png", false));
        assert!(glob.matches("b.jpg", false));
        assert!(!glob.matches("c.gif", false));

        let files = Glob::new("*").unwrap();
        assert!(!files.matches("src", true));
        let dirs = Glob::new("*/").unwrap();
        assert!(dirs.matches("src", true));
        assert!(!dirs.matches("main.rs", false));

        assert!(Glob::new("").is_none());
        assert!(Glob::new(" ; /").is_none());
    }
}
