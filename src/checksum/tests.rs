use super::*;
use crate::archive::fixtures::scratch;

/// Takes everything, stops nothing.
struct Quiet;

impl Progress for Quiet {
    fn step(&mut self, _step: Step<'_>) -> bool {
        true
    }
}

/// The checksums the standards give for "abc".
#[test]
fn each_algorithm_gives_the_standard_checksum() {
    let dir = scratch("checksum-abc");
    let path = dir.join("abc");
    std::fs::write(&path, "abc").unwrap();
    let sum = |algorithm| hash_file(&path, algorithm, &mut Quiet).unwrap().unwrap();
    assert_eq!(sum(Algorithm::Md5), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(sum(Algorithm::Sha256), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(sum(Algorithm::Sha512), "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What sha256sum writes, read back as it was written - a name with a
/// newline or a backslash in it escaped, and unescaped again.
#[test]
fn a_line_written_reads_back() {
    let hex = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    for name in ["plain.txt", "dir/with space.txt", "odd\nname", "back\\slash"] {
        let line = sums_line(hex, Path::new(name));
        assert_eq!(line.starts_with(b"\\"), name.contains(['\n', '\\']), "{name:?}");
        let (expected, not_understood) = parse_sums(&line);
        assert_eq!(
            (expected, not_understood),
            (
                vec![Expected {
                    name: PathBuf::from(name),
                    hex: hex.to_string(),
                    algorithm: Algorithm::Sha256
                }],
                0
            ),
            "{name:?}"
        );
    }
}

/// Both ways checksum files are written, binary marks, upper-case digits,
/// CRLF, comments, and lines that are none of these.
#[test]
fn checksum_files_are_read_in_either_style() {
    let text = b"# made by hand\r\n\
900150983cd24fb0d6963f7d28e17f72  a.txt\r\n\
BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD *b.bin\n\
MD5 (c (1).txt) = 900150983cd24fb0d6963f7d28e17f72\n\
\n\
not a checksum line\n\
1234  too short\n";
    let (expected, not_understood) = parse_sums(text);
    let names: Vec<(&str, Algorithm, &str)> = expected.iter().map(|line| (line.name.to_str().unwrap(), line.algorithm, &line.hex[..4])).collect();
    assert_eq!(names, [("a.txt", Algorithm::Md5, "9001"), ("b.bin", Algorithm::Sha256, "ba78"), ("c (1).txt", Algorithm::Md5, "9001")]);
    assert_eq!(not_understood, 2);
}

/// The files in what is chosen, a directory's in order and all the way
/// down, named from where they were chosen.
#[test]
fn targets_walk_directories_and_name_from_the_panel() {
    let dir = scratch("checksum-targets");
    std::fs::create_dir_all(dir.join("photos/june")).unwrap();
    std::fs::write(dir.join("photos/b.jpg"), "bb").unwrap();
    std::fs::write(dir.join("photos/june/a.jpg"), "a").unwrap();
    std::fs::write(dir.join("notes.txt"), "n").unwrap();
    let (found, unreadable) = targets(&[(dir.join("photos"), true), (dir.join("notes.txt"), false)], &dir);
    let names: Vec<(String, u64)> = found.iter().map(|target| (target.name.display().to_string(), target.size)).collect();
    assert_eq!(names, [("photos/b.jpg".to_string(), 2), ("photos/june/a.jpg".to_string(), 1), ("notes.txt".to_string(), 1)]);
    assert!(unreadable.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A checksum file written for files checks out against them; one changed
/// since fails, one gone is missing - and the worst comes first.
#[test]
fn a_checksum_file_written_checks_out() {
    let dir = scratch("checksum-check");
    for (name, text) in [("a.txt", "alpha"), ("b.txt", "bravo"), ("c.txt", "charlie")] {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let sources: Vec<(PathBuf, bool)> = ["a.txt", "b.txt", "c.txt"].iter().map(|name| (dir.join(name), false)).collect();
    let (found, _) = targets(&sources, &dir);
    let sums = compute(&found, Algorithm::Sha256, &mut Quiet).unwrap();
    let report = Report::Computed {
        algorithm: Algorithm::Sha256,
        base: dir.clone(),
        sources,
        sums,
    };
    let (file, text) = report.sums_file().unwrap();
    assert_eq!(file, dir.join("SHA256SUMS"));
    std::fs::write(&file, &text).unwrap();

    std::fs::write(dir.join("b.txt"), "changed").unwrap();
    std::fs::remove_file(dir.join("c.txt")).unwrap();
    let (expected, _) = parse_sums(&std::fs::read(&file).unwrap());
    let results = check(&expected, &dir, &mut Quiet).unwrap();
    let report = Report::Checked { file, results, not_understood: 0 };
    assert_eq!(report.summary(), "1 OK, 1 FAILED, 1 missing");
    assert_eq!(report.lines(), ["FAILED   b.txt", "missing  c.txt", "OK       a.txt"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// One file's checksum is written under its own name, and copied alone.
#[test]
fn a_single_file_gets_a_file_of_its_own() {
    let report = Report::Computed {
        algorithm: Algorithm::Md5,
        base: PathBuf::from("/x"),
        sources: Vec::new(),
        sums: vec![(PathBuf::from("big.iso"), Ok("900150983cd24fb0d6963f7d28e17f72".to_string()))],
    };
    let (file, text) = report.sums_file().unwrap();
    assert_eq!(file, Path::new("/x/big.iso.md5"));
    assert_eq!(text, b"900150983cd24fb0d6963f7d28e17f72  big.iso\n");
    assert_eq!(report.to_copy(), "900150983cd24fb0d6963f7d28e17f72");
    assert!(is_sums_file(&file) && is_sums_file(Path::new("SHA256SUMS")) && !is_sums_file(Path::new("big.iso")));
}
