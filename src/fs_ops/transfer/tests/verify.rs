//! Copies read back from where they went, and checked against the original.

use super::*;

/// Verifies, counts what it is told, and answers any failure with Skip,
/// remembering what failed and why.
#[derive(Default)]
struct Verifying {
    counted: u64,
    failed: Vec<(PathBuf, String)>,
}

impl Progress for Verifying {
    fn step(&mut self, step: Step<'_>) -> bool {
        if let Step::Advanced(amount) = step {
            self.counted += amount;
        }
        true
    }

    fn failed(&mut self, path: &Path, error: &Error) -> Reply {
        self.failed.push((path.to_path_buf(), error.to_string()));
        Reply::Skip
    }

    fn verify(&self) -> bool {
        true
    }
}

/// A verified copy is the same file, with its time kept, and the bar counts
/// each byte twice: once copied, once read back.
#[test]
fn a_verified_copy_reads_back_and_is_counted_twice() {
    let dir = scratch("verify");
    let content: Vec<u8> = (0..300_000u32).map(|n| (n % 251) as u8).collect();
    fs::write(dir.join("a"), &content).unwrap();
    let mut report = Verifying::default();
    assert_eq!(copy_path(dir.join("a"), dir.join("b"), false, &mut report).unwrap(), Transfer::Done);
    assert_eq!(fs::read(dir.join("b")).unwrap(), content);
    assert_eq!(report.counted, 2 * content.len() as u64);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(fs::metadata(dir.join("b")).unwrap().modified().unwrap(), fs::metadata(dir.join("a")).unwrap().modified().unwrap());
    // Nothing of the copy's own left beside it.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
    fs::remove_dir_all(&dir).unwrap();
}

/// What reads back as something else is not what was copied: the check says
/// so, and a copy that does not pass it never takes its place - nothing is
/// left where it was going, and nothing of the copy's own beside it.
#[test]
fn a_copy_that_reads_back_differently_does_not_take_its_place() {
    use sha2::Digest;
    let dir = scratch("verify-differs");
    fs::write(dir.join("kept"), "what the disk kept").unwrap();
    let mut said = sha2::Sha256::default();
    said.update(b"what was written");
    let mut report = Verifying::default();
    let error = read_back(&dir.join("kept"), &std::cell::RefCell::new(said), &mut report).unwrap_err();
    assert_eq!(error.to_string(), "The copy reads back differently from the original");

    let mut same = sha2::Sha256::default();
    same.update(b"what the disk kept");
    assert_eq!(read_back(&dir.join("kept"), &std::cell::RefCell::new(same), &mut report).unwrap(), Transfer::Done);

    let failing = |_: &Path, _: Report<'_>| -> Result<Transfer, Error> { Err(Error::other("differs")) };
    let outcome = write_beside(&dir.join("new"), &mut "bytes".as_bytes(), &mut report, &|_, _| {}, Some(&failing));
    assert_eq!(outcome.unwrap_err().to_string(), "differs");
    assert!(!dir.join("new").exists());
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "the temporary is gone");
    fs::remove_dir_all(&dir).unwrap();
}
