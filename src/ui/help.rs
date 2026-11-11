//! F1: the keys, then what fm84 is.

use super::*;

/// What F1 lists, under a heading each as F11's options are: the keys, then
/// what fm84 is. The version, description, licence and address come from
/// Cargo.toml, so they never fall behind it.
fn help() -> Vec<(&'static str, Vec<String>)> {
    // The description from Cargo.toml, in the language shown while it is
    // still the one written here.
    let description = if env!("CARGO_PKG_DESCRIPTION") == "A synthwave dual-pane TUI file manager" {
        t!("A synthwave dual-pane TUI file manager")
    } else {
        env!("CARGO_PKG_DESCRIPTION")
    };
    vec![
        (
            t!("Keys"),
            vec![
                t!("F1 - This help").to_string(),
                t!("F2 - Rename folder/file").to_string(),
                format!("  {}", t!("with a selection: all by [N] [E] [C]")),
                format!("  {}", t!("[Exif:date] [Tag:artist] [Pdf:title] too")),
                format!("  {}", t!("Shift+F2 compare directories")),
                t!("F3 - View file (X text/hex/image)").to_string(),
                format!("  {}", t!("F fit/fill, +/- zoom, W wrap")),
                format!("  {}", t!("F on text follows it as it grows")),
                format!("  {}", t!("Alt+Left/Right through its git history")),
                format!("  {}", t!("Shift+F3 (or =) compare two files")),
                t!("F4 - Edit file (Ctrl+S/F2 save)").to_string(),
                format!("  {}", t!("Ctrl+F find, F3/Shift+F3 next/prev")),
                format!("  {}", t!("Ctrl+G go to line, Ctrl+R/F4 replace")),
                format!("  {}", t!("Ctrl+Z undo, Ctrl+Y redo, Ctrl+X/C/V")),
                format!("  {}", t!("Shift+arrows select, Ctrl+A all")),
                format!("  {}", t!("Tab/Shift+Tab indent, Ctrl+/ comment")),
                format!("  {}", t!("Alt+R in Find or Replace: regex")),
                t!("F5 - Copy (to the other panel, or where typed)").to_string(),
                format!("  {}", t!("Alt+F5 (or Ctrl+Shift+F5) link, Tab hard")),
                format!("  {}", t!("on a .001, joins the pieces; too big, split")),
                t!("F6 - Move (to the other panel, or where typed)").to_string(),
                t!("F7 - Create directory").to_string(),
                format!("  {}", t!("Alt+F7 (or Ctrl+F7) find files")),
                format!("  {}", t!("by name, text, regex, size, date")),
                format!("  {}", t!("F on the results feeds them to the panel")),
                t!("Shift+F4 - Create file").to_string(),
                t!("F8 - Move to trash").to_string(),
                format!("  {}", t!("Shift+F8 delete permanently")),
                t!("Alt+T - The trash as a folder").to_string(),
                format!("  {}", t!("F6 or Enter puts back, F8 empties")),
                t!("Ctrl+Z - Undo the last rename, copy, move or trash").to_string(),
                t!("Alt+A - Permissions, owner, time").to_string(),
                t!("Alt+P - Pack: Tab format, level, password").to_string(),
                t!("Alt+H - Checksums (Tab algorithm); check a file").to_string(),
                t!("Alt+S - Synchronize the two directories").to_string(),
                t!("Alt+D - Find duplicate files").to_string(),
                t!("Alt+R - Dossier: the directory summed up").to_string(),
                t!("Alt+Shift+G - Ghost hunt: untouched for years").to_string(),
                t!("Alt+G - Git: stage, unstage, discard, compare").to_string(),
                format!("  {}", t!("R opens a branch, tag or commit as a folder")),
                t!("Alt+B - Onto the mixtape; Alt+Shift+B lists them").to_string(),
                t!("F9 - Open terminal").to_string(),
                t!("Alt+M - User menu (E edits it)").to_string(),
                t!("F10 - Quit").to_string(),
                format!("  {}", t!("Shift+F10 quit, cd84 staying put")),
                format!("  {}", t!("Alt+F10 directory tree, Enter sends the other")),
                t!("F11 - Options (Alt+F1/F2 drives)").to_string(),
                format!("  {}", t!("E on Theme: file colours by extension")),
                t!("F12 - Preview in other panel").to_string(),
                t!("Enter - Open dir, or go into an archive").to_string(),
                format!("  {}", t!(".zip .7z .tar(.gz .zst .xz .bz2): F5 F6 F7 F8 inside")),
                format!("  {}", t!(".jar .apk .docx and other zips, .deb .iso: read")),
                format!("  {}", t!("F5 copies out, Alt+F9 (or Ctrl+F9) all of it")),
                format!("  {}", t!("Shift+Enter (or Ctrl+O) open as the system does")),
                format!("  {}", t!("Alt+O open with... (E per extension)")),
                t!("Ctrl+Left/Right - Dir to that panel").to_string(),
                format!("  {}", t!("Ctrl+U swap panels, Alt+I same dir there")),
                t!("Ctrl+L - Go to path (or click it)").to_string(),
                format!("  {}", t!("Tab completes a name")),
                format!("  {}", t!("Alt+Left/Right back/forward")),
                format!("  {}", t!("Alt+Down every directory visited")),
                format!("  {}", t!("Click the git branch to switch it")),
                t!("Ctrl+D - Bookmarks").to_string(),
                t!("Ctrl+T/Ctrl+W - New/close tab").to_string(),
                format!("  {}", t!("Ctrl+PgUp/PgDn, Alt+1..9 switch")),
                t!("Space - Select/deselect file").to_string(),
                format!("  {}", t!("Alt+Shift+Enter size and map every directory")),
                t!("+/- select/deselect by pattern, * invert").to_string(),
                format!("  {}", t!("Alt+* invert with directories")),
                t!("Ctrl+S - Filter the panel by pattern").to_string(),
                t!("Ctrl+B - Every file below, flat").to_string(),
                t!("Ctrl+E - Edit the names as text").to_string(),
                t!("Ctrl+C/Alt+C - Copy paths/names").to_string(),
                t!("Ctrl+F3..F6 - Sort name/ext/date/size").to_string(),
                t!("Ctrl+R - Reload both panels").to_string(),
                t!("Type to search, Esc to clear").to_string(),
            ],
        ),
        (
            t!("About"),
            vec![
                concat!("FM84 - File Manager '84, v", env!("CARGO_PKG_VERSION")).to_string(),
                description.to_string(),
                t!("Built with Rust, Ratatui and Crossterm").to_string(),
                "Copyright (c) 2025-2026 Colt Willcox".to_string(),
                t!("Released under the {license} License", license = env!("CARGO_PKG_LICENSE")),
                env!("CARGO_PKG_REPOSITORY").to_string(),
            ],
        ),
    ]
}

/// The logo from the README, between the keys and what fm84 is.
pub(super) const LOGO: [&str; 6] = [
    "███████╗███╗   ███╗ █████╗ ██╗  ██╗",
    "██╔════╝████╗ ████║██╔══██╗██║  ██║",
    "█████╗  ██╔████╔██║╚█████╔╝███████║",
    "██╔══╝  ██║╚██╔╝██║██╔══██╗╚════██║",
    "██║     ██║ ╚═╝ ██║╚█████╔╝     ██║",
    "╚═╝     ╚═╝     ╚═╝ ╚════╝      ╚═╝",
];

/// What a line of F1 is, for how it is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HelpLine {
    Heading,
    /// Under Keys: the keys in one colour, what they do in another.
    Key,
    Item,
    /// A row of the logo, centred, in the theme's colours.
    Logo,
    /// A high score: what it is, then from this byte on, the figure.
    Score(usize),
    /// An achievement, and whether it has been had.
    Achievement(bool),
    /// INSERT COIN, for a table with nothing on it yet.
    Coin,
}

/// How wide a high score's name is drawn, dots and all.
const SCORE_WIDTH: usize = 28;

/// The high scores, as an arcade cabinet lists them: each name in capitals,
/// dots up to the figure; then the achievements, starred when had.
fn score_lines(scores: &crate::options::Scores) -> Vec<(String, HelpLine)> {
    let mut lines = vec![(t!("High scores").to_string(), HelpLine::Heading), (String::new(), HelpLine::Item)];
    if scores.is_empty() {
        lines.push(("INSERT COIN".to_string(), HelpLine::Coin));
        return lines;
    }
    let seconds = scores.longest_job;
    let copies = if scores.copies_found == 0 {
        "0".to_string()
    } else {
        format!("{} ({})", grouped(scores.copies_found), format_size(scores.copy_bytes_found))
    };
    let rows = [
        (t!("Bytes copied"), format_size(scores.bytes_copied)),
        (t!("Bytes moved"), format_size(scores.bytes_moved)),
        (t!("Files renamed"), grouped(scores.files_renamed)),
        (t!("Deleted or trashed"), grouped(scores.deleted)),
        (t!("Copies found"), copies),
        (t!("Longest job"), format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)),
        (t!("Jobs done"), grouped(scores.jobs)),
        (t!("Undos"), grouped(scores.undos)),
    ];
    for (name, figure) in rows {
        let name = name.to_uppercase();
        let dots = ".".repeat(SCORE_WIDTH.saturating_sub(name.chars().count() + 2).max(2));
        let text = format!("   {name} {dots} ");
        let at = text.len();
        lines.push((format!("{text}{figure}"), HelpLine::Score(at)));
    }
    lines.push((String::new(), HelpLine::Item));
    for achievement in scores.achievements() {
        let star = if achievement.unlocked { '★' } else { '☆' };
        lines.push((format!("   {star} {}", achievement.name), HelpLine::Achievement(achievement.unlocked)));
    }
    lines
}

/// F1's lines as they are drawn, and what each is: a heading centred over
/// each part, with a blank row before every one but the first and one under
/// it, and the lines under it set in. The logo stands after the keys, a
/// blank row either side of it, then the high scores, and About last.
pub fn help_lines(scores: &crate::options::Scores) -> Vec<(String, HelpLine)> {
    let mut lines = Vec::new();
    for (index, (heading, items)) in help().into_iter().enumerate() {
        if !lines.is_empty() {
            lines.push((String::new(), HelpLine::Item));
        }
        if index == 1 {
            lines.extend(LOGO.iter().map(|row| (row.to_string(), HelpLine::Logo)));
            lines.push((String::new(), HelpLine::Item));
            lines.extend(score_lines(scores));
            lines.push((String::new(), HelpLine::Item));
        }
        lines.push((heading.to_string(), HelpLine::Heading));
        lines.push((String::new(), HelpLine::Item));
        let kind = if index == 0 { HelpLine::Key } else { HelpLine::Item };
        lines.extend(items.iter().map(|item| (format!("   {item}"), kind)));
    }
    lines
}

/// A line under Keys cut into runs, each true where it is a key. One of the
/// main lines - set in by three - is a key, a dash and what it does, and
/// all before the dash is the key; elsewhere the keys are told word by word,
/// the brackets and the commas around them left out of them.
pub fn key_runs(text: &str) -> Vec<(String, bool)> {
    let mut runs: Vec<(String, bool)> = Vec::new();
    let mut push = |piece: &str, key: bool| match runs.last_mut() {
        Some((run, was)) if *was == key => run.push_str(piece),
        _ if !piece.is_empty() => runs.push((piece.to_string(), key)),
        _ => {}
    };
    let indent = text.len() - text.trim_start().len();
    push(&text[..indent], false);
    let mut rest = &text[indent..];
    // What a key does starts as a sentence does, and in Hungarian that can
    // be with "A", which is no key.
    let mut sentence = false;
    if indent == 3
        && let Some(at) = rest.find(" - ")
    {
        push(&rest[..at], true);
        push(" - ", false);
        rest = &rest[at + 3..];
        sentence = true;
    }
    for piece in rest.split_inclusive(' ') {
        let word = piece.trim_end_matches(' ');
        let core = word.trim_matches(|c: char| "()[],;:.".contains(c));
        let starts = std::mem::take(&mut sentence);
        if core.is_empty() || !is_key(core) || (starts && core.chars().count() == 1) {
            push(piece, false);
            continue;
        }
        let start = word.find(core).unwrap_or(0);
        push(&word[..start], false);
        push(core, true);
        push(&piece[start + core.len()..], false);
    }
    runs
}

/// Whether something blinking is lit this moment: half a second on, half
/// off. Kept by the clock, as the frames come at least ten times a second,
/// rather than by the terminal - many do not blink, and the blink modifiers
/// are the marks the cursors are found by.
pub(super) fn blink_lit() -> bool {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(true, |since| since.as_millis() / 500 % 2 == 0)
}

/// A row of the logo, centred in `width`: its solid blocks in the colour
/// directories have, the lines that shade them in the border's.
fn logo_row(row: &str, width: usize, lit: bool) -> Line<'static> {
    let used = row.chars().count();
    let before = width.saturating_sub(used) / 2;
    let after = width.saturating_sub(used + before);
    let shade = Style::default().fg(palette().border);
    // Out, its blocks are drawn as their shading is.
    let solid = if lit { Style::default().fg(palette().directory).add_modifier(Modifier::BOLD) } else { shade };
    let mut spans = vec![Span::raw(" ".repeat(before))];
    let mut run = String::new();
    let mut run_solid = None;
    for character in row.chars() {
        // Spaces go with whatever they are beside: they take no colour.
        let is_solid = match character {
            '█' => Some(true),
            ' ' => run_solid,
            _ => Some(false),
        };
        if is_solid != run_solid && !run.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut run), if run_solid == Some(true) { solid } else { shade }));
        }
        run_solid = is_solid;
        run.push(character);
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, if run_solid == Some(true) { solid } else { shade }));
    }
    spans.push(Span::raw(" ".repeat(after)));
    Line::from(spans)
}

/// F1: the keys, over the panels and no taller than them. What does not fit
/// scrolls, from `scroll` lines down, and the corner says which lines show.
/// Gives back how many lines there is room for, for the keys to scroll by.
pub(super) fn render_help_popup(f: &mut ratatui::Frame<'_>, area: Rect, scroll: usize, scores: &crate::options::Scores) -> usize {
    // A border and a blank row above and below the lines.
    let popup_width = area.width as usize * POPUP_WIDTH as usize / 100;
    let help = help_lines(scores);
    let popup_area = centered(screen_layout(area)[2], popup_width.max(1), help.len() + 4);
    let inner = popup_area.inner(Margin { vertical: 2, horizontal: 2 });
    let room = (inner.height as usize).max(1);
    let first = scroll.min(help.len().saturating_sub(room));

    let mut popup_block = Block::default().title(Line::from(Span::styled(format!(" {} ", t!("Help/About")), style_title())).centered()).borders(Borders::ALL).style(style_border());
    if room < help.len() {
        let shown = format!(" {}-{}/{} ", first + 1, (first + room).min(help.len()), help.len());
        popup_block = popup_block.title_bottom(Line::from(Span::styled(shown, style_columns())).right_aligned());
    }

    clear(f, popup_area);
    f.render_widget(popup_block, popup_area);

    // Characters, not bytes, as format! pads by characters. All padded to
    // the longest, so the block is centred and its lines start together.
    let max_len = help.iter().map(|(text, _)| text.chars().count()).max().unwrap_or(0);
    let lines: Vec<Line> = help
        .iter()
        .skip(first)
        .take(room)
        .map(|(text, kind)| match kind {
            // Turbo has it blinking, as an attract screen's.
            HelpLine::Logo => logo_row(text, max_len, !crate::display::turbo() || blink_lit()),
            // Centred over what it heads, as the logo is.
            HelpLine::Heading => Line::from(Span::styled(format!("{text:^max_len$}"), style_title().add_modifier(Modifier::BOLD))),
            HelpLine::Item => Line::from(Span::styled(format!("{text:<max_len$}"), style_title())),
            HelpLine::Score(at) => {
                let pad = " ".repeat(max_len.saturating_sub(text.chars().count()));
                Line::from(vec![Span::styled(text[..*at].to_string(), style_title()), Span::styled(format!("{}{pad}", &text[*at..]), style_key())])
            }
            HelpLine::Achievement(unlocked) => {
                let style = if *unlocked { style_dir().add_modifier(Modifier::BOLD) } else { style_border() };
                Line::from(Span::styled(format!("{text:<max_len$}"), style))
            }
            HelpLine::Coin => {
                // Centred in the block, and blinking, as on the cabinet.
                let used = text.chars().count();
                let before = max_len.saturating_sub(used) / 2;
                let shown = if blink_lit() { text.clone() } else { " ".repeat(used) };
                Line::from(vec![Span::raw(" ".repeat(before)), Span::styled(shown, style_key()), Span::raw(" ".repeat(max_len.saturating_sub(used + before)))])
            }
            HelpLine::Key => {
                let pad = " ".repeat(max_len.saturating_sub(text.chars().count()));
                let key = style_key();
                let mut spans: Vec<Span> = key_runs(text).into_iter().map(|(run, is_key)| Span::styled(run, if is_key { key } else { style_title() })).collect();
                spans.push(Span::raw(pad));
                Line::from(spans)
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
    room
}
