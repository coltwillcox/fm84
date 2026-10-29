//! Checksums - Alt+H: worked out for what is chosen, written and read the
//! way sha256sum and md5sum write and read them, and checked.

use crate::fs_ops::{Progress, Step};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Algorithm {
    Sha256,
    Sha512,
    Md5,
}

impl Algorithm {
    /// In the order Tab goes through them.
    pub const ALL: [Algorithm; 3] = [Algorithm::Sha256, Algorithm::Sha512, Algorithm::Md5];

    pub fn name(self) -> &'static str {
        match self {
            Algorithm::Sha256 => "SHA-256",
            Algorithm::Sha512 => "SHA-512",
            Algorithm::Md5 => "MD5",
        }
    }

    /// How a BSD-style line names it: `SHA256 (file) = ...`.
    fn tag(self) -> &'static str {
        match self {
            Algorithm::Sha256 => "SHA256",
            Algorithm::Sha512 => "SHA512",
            Algorithm::Md5 => "MD5",
        }
    }

    /// The file one is written to for several files, as GNU names them.
    pub fn sums_file(self) -> &'static str {
        match self {
            Algorithm::Sha256 => "SHA256SUMS",
            Algorithm::Sha512 => "SHA512SUMS",
            Algorithm::Md5 => "MD5SUMS",
        }
    }

    /// And the ending of one written for a single file.
    pub fn extension(self) -> &'static str {
        match self {
            Algorithm::Sha256 => ".sha256",
            Algorithm::Sha512 => ".sha512",
            Algorithm::Md5 => ".md5",
        }
    }

    pub fn next(self) -> Self {
        let at = Algorithm::ALL.iter().position(|&algorithm| algorithm == self).unwrap_or(0);
        Algorithm::ALL[(at + 1) % Algorithm::ALL.len()]
    }

    /// The algorithm a checksum of this many hex digits was worked out with.
    fn by_length(digits: usize) -> Option<Self> {
        Algorithm::ALL.into_iter().find(|algorithm| algorithm.hasher().digits() == digits)
    }

    fn hasher(self) -> Hasher {
        use md5::Digest;
        match self {
            Algorithm::Sha256 => Hasher::Sha256(sha2::Sha256::new()),
            Algorithm::Sha512 => Hasher::Sha512(sha2::Sha512::new()),
            Algorithm::Md5 => Hasher::Md5(md5::Md5::new()),
        }
    }
}

enum Hasher {
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
    Md5(md5::Md5),
}

impl Hasher {
    fn update(&mut self, bytes: &[u8]) {
        use md5::Digest;
        match self {
            Hasher::Sha256(hasher) => hasher.update(bytes),
            Hasher::Sha512(hasher) => hasher.update(bytes),
            Hasher::Md5(hasher) => hasher.update(bytes),
        }
    }

    fn hex(self) -> String {
        use md5::Digest;
        let bytes: Vec<u8> = match self {
            Hasher::Sha256(hasher) => hasher.finalize().to_vec(),
            Hasher::Sha512(hasher) => hasher.finalize().to_vec(),
            Hasher::Md5(hasher) => hasher.finalize().to_vec(),
        };
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn digits(&self) -> usize {
        match self {
            Hasher::Sha256(_) => 64,
            Hasher::Sha512(_) => 128,
            Hasher::Md5(_) => 32,
        }
    }
}

/// The checksum of a file, its bytes counted onto the bar as they are read.
/// None when the bar says to stop. A pipe or a device is refused: it would
/// wait for good, or never end.
pub fn hash_file(path: &Path, algorithm: Algorithm, report: &mut dyn Progress) -> io::Result<Option<String>> {
    if !crate::fs_ops::is_regular_file(path) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Not a regular file"));
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = algorithm.hasher();
    let mut chunk = vec![0; 256 * 1024];
    loop {
        let read = match file.read(&mut chunk) {
            Ok(0) => return Ok(Some(hasher.hex())),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        hasher.update(&chunk[..read]);
        if !report.step(Step::Advanced(read as u64)) {
            return Ok(None);
        }
    }
}

/// A file to work out the checksum of: where it is, and the name it is
/// listed by - from the directory the panel is in, as sha256sum run there
/// would list it.
pub struct Target {
    pub path: PathBuf,
    pub name: PathBuf,
    pub size: u64,
}

/// The files in what is chosen, a directory's all the way down. Links to
/// files are followed, as sha256sum follows them; links to directories are
/// not, so a link to an ancestor cannot go round for ever. A directory that
/// cannot be read is listed by itself, to say so.
pub fn targets(sources: &[(PathBuf, bool)], base: &Path) -> (Vec<Target>, Vec<(PathBuf, String)>) {
    let (mut found, mut unreadable) = (Vec::new(), Vec::new());
    let name_of = |path: &Path| path.strip_prefix(base).map_or_else(|_| path.to_path_buf(), Path::to_path_buf);
    let mut stack: Vec<PathBuf> = sources.iter().map(|(path, _)| path.clone()).collect();
    stack.reverse();
    while let Some(path) = stack.pop() {
        let Ok(own) = path.symlink_metadata() else {
            unreadable.push((name_of(&path), "Gone".to_string()));
            continue;
        };
        if own.is_dir() {
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    let mut inside: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
                    inside.sort();
                    stack.extend(inside.into_iter().rev());
                }
                Err(error) => unreadable.push((name_of(&path), error.to_string())),
            }
        } else if let Some(followed) = std::fs::metadata(&path).ok().filter(|metadata| metadata.is_file()) {
            found.push(Target {
                name: name_of(&path),
                size: followed.len(),
                path,
            });
        } else if !own.file_type().is_symlink() {
            unreadable.push((name_of(&path), "Not a regular file".to_string()));
        }
    }
    (found, unreadable)
}

/// One line of a checksum file as sha256sum writes it: the checksum, two
/// spaces, the name. A name with a newline or a backslash in it is escaped,
/// and the line marked with a backslash first to say so.
pub fn sums_line(hex: &str, name: &Path) -> Vec<u8> {
    let bytes = name.as_os_str().as_encoded_bytes();
    let escape = bytes.iter().any(|&byte| byte == b'\n' || byte == b'\\');
    let mut line = Vec::with_capacity(hex.len() + bytes.len() + 4);
    if escape {
        line.push(b'\\');
    }
    line.extend_from_slice(hex.as_bytes());
    line.extend_from_slice(b"  ");
    for &byte in bytes {
        match byte {
            b'\n' if escape => line.extend_from_slice(b"\\n"),
            b'\\' if escape => line.extend_from_slice(b"\\\\"),
            _ => line.push(byte),
        }
    }
    line.push(b'\n');
    line
}

/// A checksum a file says another should have.
#[derive(Debug, PartialEq, Eq)]
pub struct Expected {
    pub name: PathBuf,
    pub hex: String,
    pub algorithm: Algorithm,
}

/// What a checksum file says, as sha256sum or md5sum wrote it - `hex  name`,
/// or `hex *name` for one read as binary - or as BSD tools write it,
/// `SHA256 (name) = hex`. The algorithm is told by the length of each
/// checksum. Blank lines and `#` comments are passed over; the count is of
/// lines that are neither and could not be read.
pub fn parse_sums(text: &[u8]) -> (Vec<Expected>, usize) {
    let (mut expected, mut not_understood) = (Vec::new(), 0);
    for line in text.split(|&byte| byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(u8::is_ascii_whitespace) || line.starts_with(b"#") {
            continue;
        }
        match parse_bsd(line).or_else(|| parse_gnu(line)) {
            Some(found) => expected.push(found),
            None => not_understood += 1,
        }
    }
    (expected, not_understood)
}

fn checksum(hex: &[u8]) -> Option<(String, Algorithm)> {
    let algorithm = Algorithm::by_length(hex.len())?;
    hex.iter().all(u8::is_ascii_hexdigit).then(|| (String::from_utf8_lossy(hex).to_lowercase(), algorithm))
}

fn parse_gnu(line: &[u8]) -> Option<Expected> {
    let (escaped, line) = match line.strip_prefix(b"\\") {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    let space = line.iter().position(|&byte| byte == b' ')?;
    let (hex, algorithm) = checksum(&line[..space])?;
    // One space more, or a star for a file read as binary.
    let name = line.get(space + 1..).filter(|rest| matches!(rest.first(), Some(b' ' | b'*')))?.get(1..)?;
    if name.is_empty() {
        return None;
    }
    let name = if escaped { unescape(name)? } else { name.to_vec() };
    Some(Expected { name: path_from_bytes(name), hex, algorithm })
}

fn parse_bsd(line: &[u8]) -> Option<Expected> {
    let algorithm = Algorithm::ALL.into_iter().find(|algorithm| line.starts_with(format!("{} (", algorithm.tag()).as_bytes()))?;
    let rest = &line[algorithm.tag().len() + 2..];
    let split = rest.windows(4).rposition(|window| window == b") = ")?;
    let (hex, found) = checksum(&rest[split + 4..])?;
    (found == algorithm).then(|| Expected {
        name: path_from_bytes(rest[..split].to_vec()),
        hex,
        algorithm,
    })
}

/// `\n` and `\\` back to what they stand for; anything else after a
/// backslash is not something sha256sum writes.
fn unescape(name: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(name.len());
    let mut bytes = name.iter();
    while let Some(&byte) = bytes.next() {
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        match bytes.next()? {
            b'n' => out.push(b'\n'),
            b'\\' => out.push(b'\\'),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// Whether a file is one of checksums, by its name: what Alt+H checks
/// rather than works out the checksum of.
pub fn is_sums_file(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    let endings = [".md5", ".sha256", ".sha512", ".md5sum", ".sha256sum", ".sha512sum"];
    endings.iter().any(|ending| name.ends_with(ending)) || ["md5sums", "sha256sums", "sha512sums"].contains(&name.as_str())
}

/// How one file came out against what a checksum file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    Ok,
    Failed,
    Missing,
    Unreadable(String),
}

/// What Alt+H found, for the popup.
#[derive(Debug)]
pub enum Report {
    /// Worked out: each name with its checksum, or why it has none.
    Computed {
        algorithm: Algorithm,
        base: PathBuf,
        sources: Vec<(PathBuf, bool)>,
        sums: Vec<(PathBuf, Result<String, String>)>,
    },
    /// Checked against `file`: each name with how it came out.
    Checked { file: PathBuf, results: Vec<(PathBuf, Algorithm, Check)>, not_understood: usize },
}

impl Report {
    /// The lines it is shown as, and copied as: a checksum file's own lines
    /// for what was worked out, and for what was checked, what failed first.
    pub fn lines(&self) -> Vec<String> {
        match self {
            Report::Computed { sums, .. } => sums
                .iter()
                .map(|(name, sum)| match sum {
                    Ok(hex) => format!("{hex}  {}", name.display()),
                    Err(error) => format!("{}  {}: {error}", "-".repeat(8), name.display()),
                })
                .collect(),
            Report::Checked { results, .. } => {
                let rank = |check: &Check| match check {
                    Check::Failed => 0,
                    Check::Missing => 1,
                    Check::Unreadable(_) => 2,
                    Check::Ok => 3,
                };
                let mut ordered: Vec<&(PathBuf, Algorithm, Check)> = results.iter().collect();
                ordered.sort_by_key(|(_, _, check)| rank(check));
                ordered
                    .into_iter()
                    .map(|(name, _, check)| match check {
                        Check::Ok => format!("OK       {}", name.display()),
                        Check::Failed => format!("FAILED   {}", name.display()),
                        Check::Missing => format!("missing  {}", name.display()),
                        Check::Unreadable(error) => format!("unread   {}: {error}", name.display()),
                    })
                    .collect()
            }
        }
    }

    /// One line to sum it up, above the rest.
    pub fn summary(&self) -> String {
        match self {
            Report::Computed { sums, .. } => {
                let failed = sums.iter().filter(|(_, sum)| sum.is_err()).count();
                let files = if sums.len() == 1 { "file" } else { "files" };
                if failed == 0 { format!("{} {files}", sums.len()) } else { format!("{} {files}, {failed} could not be read", sums.len()) }
            }
            Report::Checked { results, not_understood, .. } => {
                let count = |wanted: fn(&Check) -> bool| results.iter().filter(|(_, _, check)| wanted(check)).count();
                let mut parts = vec![format!("{} OK", count(|check| *check == Check::Ok))];
                for (number, word) in [
                    (count(|check| *check == Check::Failed), "FAILED"),
                    (count(|check| *check == Check::Missing), "missing"),
                    (count(|check| matches!(check, Check::Unreadable(_))), "could not be read"),
                    (*not_understood, "lines not understood"),
                ] {
                    if number > 0 {
                        parts.push(format!("{number} {word}"));
                    }
                }
                parts.join(", ")
            }
        }
    }

    /// What C copies: for a single checksum worked out, the checksum alone -
    /// what is pasted to compare with one somewhere else - and otherwise
    /// every line.
    pub fn to_copy(&self) -> String {
        match self {
            Report::Computed { sums, .. } if sums.len() == 1 => sums[0].1.clone().unwrap_or_default(),
            _ => self.lines().join("\n"),
        }
    }

    /// The checksum file W writes, and the name it is written under: the
    /// file's own name with the ending for one file, the GNU name for more.
    /// What could not be read is left out.
    pub fn sums_file(&self) -> Option<(PathBuf, Vec<u8>)> {
        let Report::Computed { algorithm, base, sums, .. } = self else {
            return None;
        };
        let written: Vec<(&PathBuf, &String)> = sums.iter().filter_map(|(name, sum)| sum.as_ref().ok().map(|hex| (name, hex))).collect();
        let name = match &written[..] {
            [] => return None,
            [(single, _)] => format!("{}{}", single.file_name()?.to_string_lossy(), algorithm.extension()),
            _ => algorithm.sums_file().to_string(),
        };
        let text = written.into_iter().flat_map(|(name, hex)| sums_line(hex, name)).collect();
        Some((base.join(name), text))
    }
}

/// Work out the checksum of each target. None when cancelled.
pub fn compute(targets: &[Target], algorithm: Algorithm, report: &mut dyn Progress) -> Option<Vec<(PathBuf, Result<String, String>)>> {
    let mut sums = Vec::with_capacity(targets.len());
    for target in targets {
        if !report.step(Step::Starting(&target.path)) {
            return None;
        }
        sums.push((target.name.clone(), hash_file(&target.path, algorithm, report).transpose()?.map_err(|error| error.to_string())));
    }
    Some(sums)
}

/// Check what `expected` says against the files, each from `base`, the
/// directory the checksum file is in. None when cancelled.
pub fn check(expected: &[Expected], base: &Path, report: &mut dyn Progress) -> Option<Vec<(PathBuf, Algorithm, Check)>> {
    let mut results = Vec::with_capacity(expected.len());
    for line in expected {
        let path = base.join(&line.name);
        if !report.step(Step::Starting(&path)) {
            return None;
        }
        let check = if crate::fs_ops::path_exists(&path) {
            match hash_file(&path, line.algorithm, report) {
                Ok(Some(hex)) => {
                    if hex == line.hex {
                        Check::Ok
                    } else {
                        Check::Failed
                    }
                }
                Ok(None) => return None,
                Err(error) => Check::Unreadable(error.to_string()),
            }
        } else {
            Check::Missing
        };
        results.push((line.name.clone(), line.algorithm, check));
    }
    Some(results)
}

#[cfg(test)]
mod tests;
