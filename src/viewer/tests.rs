use super::*;

/// Every hexdump layout measures what it says it does, and the viewer's own
/// still comes to the width the horizontal scroll is bounded by.
#[test]
fn lines_wrap_at_spaces_and_only_split_a_word_too_long_for_a_row() {
    let pieces = |text: &str, width: usize| -> Vec<String> {
        let characters: Vec<char> = text.chars().collect();
        super::wrap_ranges(text, width).into_iter().map(|(from, to)| characters[from..to].iter().collect()).collect()
    };
    // After the space, which stays with the word before it.
    assert_eq!(pieces("the quick brown fox", 10), ["the quick ", "brown fox"]);
    // A word wider than a row is split where the row ends.
    assert_eq!(pieces("abcdefghijkl xy", 5), ["abcde", "fghij", "kl xy"]);
    assert_eq!(pieces("abcdefghijkl wxyz", 5), ["abcde", "fghij", "kl ", "wxyz"]);
    // Wide characters take the two columns they are drawn in.
    assert_eq!(pieces("界界界", 4), ["界界", "界"]);
    // One that does not fit at all still moves on, a row of its own.
    assert_eq!(pieces("界x", 1), ["界", "x"]);
    // An empty line keeps its row.
    assert_eq!(super::wrap_ranges("", 10), [(0, 0)]);
}

#[test]
fn hex_rows_measure_what_they_claim() {
    let bytes: Vec<u8> = (0..=255u8).collect();
    for (per_line, digits) in [(16, 8), (8, 4), (8, 0), (4, 0), (1, 8)] {
        let full = hex_row(0, &bytes[..per_line], per_line, digits);
        assert_eq!(full.chars().count(), hex_row_width(per_line, digits), "{per_line} bytes, {digits} digits: {full}");
        // A short last row pads its bytes, so only the gutter is shorter.
        let short = hex_row(0, &bytes[..1], per_line, digits);
        assert_eq!(short.chars().count(), hex_row_width(per_line, digits) - (per_line - 1), "{short}");
    }
    assert_eq!(hex_row_width(HEX_BYTES_PER_LINE, HEX_OFFSET_DIGITS), crate::constants::HEX_LINE_WIDTH);
    assert_eq!(hex_line(0, &bytes[..16]), hex_row(0, &bytes[..16], HEX_BYTES_PER_LINE, HEX_OFFSET_DIGITS));
}

/// A binary preview hands over bytes for the pane to lay out; a text one
/// still comes as lines.
#[test]
fn previews_binary_as_bytes_and_text_as_lines() {
    let dir = std::env::temp_dir().join(format!("fm84-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let binary = dir.join("binary");
    std::fs::write(&binary, [0x7f, b'E', b'L', b'F', 0, 1, 2, 3]).unwrap();
    let Preview::Bytes(bytes) = load_preview(&binary, 1024, 10) else {
        panic!("a file with a null byte in it is a binary");
    };
    assert_eq!(bytes, [0x7f, b'E', b'L', b'F', 0, 1, 2, 3]);

    let text = dir.join("text");
    std::fs::write(&text, "one\ntwo\nthree\n").unwrap();
    let Preview::Lines(lines) = load_preview(&text, 1024, 2) else {
        panic!("text is still lines");
    };
    assert_eq!(lines, ["one", "two"]);

    std::fs::remove_dir_all(&dir).unwrap();
}

fn sample(count: usize) -> Vec<String> {
    let template = [
        "pub fn item_{}(value: u32) -> Result<String, Error> {",
        "    let text = format!(\"item {} of the set\", value);",
        "    match value { 0 => Err(Error::Empty), _ => Ok(text) }",
        "}",
    ];
    (0..count).map(|i| template[i % template.len()].replace("{}", &i.to_string())).collect()
}

/// Spans reduced to comparable data.
fn shape(spans: &[Vec<Span<'static>>]) -> Vec<Vec<(String, Option<Color>)>> {
    spans.iter().map(|line| line.iter().map(|s| (s.content.to_string(), s.style.fg)).collect()).collect()
}

fn assert_matches_full(lines: &[String], spans: &[Vec<Span<'static>>], states: &[LineState], case: &str) {
    let (expected, _) = highlight_all(lines, "rs");
    assert_eq!(shape(spans), shape(&expected), "spans diverged after {case}");
    assert_eq!(spans.len(), lines.len(), "span count wrong after {case}");
    assert_eq!(states.len(), lines.len(), "state count wrong after {case}");
}

#[test]
fn incremental_matches_full_rehighlight() {
    let mut lines = sample(200);
    let (mut spans, mut states) = highlight_all(&lines, "rs");
    assert_matches_full(&lines, &spans, &states, "initial load");

    // Edit inside one line - should converge almost immediately.
    lines[50].push_str(" // trailing note");
    highlight_from(&lines, "rs", 50, &mut spans, &mut states);
    assert_matches_full(&lines, &spans, &states, "in-line edit");

    // Open a block comment: every line below changes meaning, so the
    // convergence check must NOT stop early here.
    lines.insert(10, "/* opening a block".to_string());
    highlight_from(&lines, "rs", 10, &mut spans, &mut states);
    assert_matches_full(&lines, &spans, &states, "block comment opened");

    // Close it again.
    lines.insert(11, "closing it */".to_string());
    highlight_from(&lines, "rs", 11, &mut spans, &mut states);
    assert_matches_full(&lines, &spans, &states, "block comment closed");

    // Remove a line (cache shifts the other way).
    lines.remove(11);
    highlight_from(&lines, "rs", 11, &mut spans, &mut states);
    assert_matches_full(&lines, &spans, &states, "line removed");

    // Edit the very last line.
    let last = lines.len() - 1;
    lines[last].push_str("// end");
    highlight_from(&lines, "rs", last, &mut spans, &mut states);
    assert_matches_full(&lines, &spans, &states, "last-line edit");

    // Append past the old end.
    lines.push("fn tail() {}".to_string());
    highlight_from(&lines, "rs", lines.len() - 1, &mut spans, &mut states);
    assert_matches_full(&lines, &spans, &states, "appended line");
}
