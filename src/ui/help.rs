//! F1: the keys, then what fm84 is.

use super::*;

/// What F1 lists, under a heading each as F11's options are: the keys, then
/// what fm84 is. The version, description, licence and address come from
/// Cargo.toml, so they never fall behind it.
const HELP: &[(&str, &[&str])] = &[
    (
        "Keys",
        &[
            "F1 - This help",
            "F2 - Rename folder/file",
            "  Shift+F2 compare directories",
            "F3 - View file (X text/hex/image)",
            "  F fit/fill, +/- zoom, W wrap",
            "  Shift+F3 (or =) compare two files",
            "F4 - Edit file (Ctrl+S/F2 save)",
            "  Ctrl+F find, F3/Shift+F3 next/prev",
            "  Ctrl+G go to line, Ctrl+R/F4 replace",
            "  Ctrl+Z undo, Ctrl+Y redo, Ctrl+X/C/V",
            "  Shift+arrows select, Ctrl+A all",
            "F5 - Copy (to the other panel, or where typed)",
            "  Alt+F5 (or Ctrl+Shift+F5) symlink",
            "F6 - Move (to the other panel, or where typed)",
            "F7 - Create directory",
            "  Alt+F7 (or Ctrl+F7) find files",
            "Shift+F4 - Create file",
            "F8 - Move to trash",
            "  Shift+F8 delete permanently",
            "Alt+A - Permissions, owner, time",
            "F9 - Open terminal",
            "F10 - Quit",
            "F11 - Options (Alt+F1/F2 drives)",
            "F12 - Preview in other panel",
            "Enter - Open dir, or go into an archive",
            "  .zip .tar .tar.gz: read-only, F5 copies out",
            "  Shift+Enter (or Ctrl+O) open as the system does",
            "Ctrl+Left/Right - Dir to that panel",
            "  Ctrl+U swap panels, Alt+I same dir there",
            "Ctrl+L - Go to path (or click it)",
            "  Tab completes a name",
            "  Alt+Left/Right back/forward",
            "  Click the git branch to switch it",
            "Ctrl+D - Bookmarks",
            "Ctrl+T/Ctrl+W - New/close tab",
            "  Ctrl+PgUp/PgDn, Alt+1..9 switch",
            "Space - Select/deselect file",
            "  Alt+Shift+Enter size every directory",
            "+/- select/deselect by pattern, * invert",
            "  Alt+* invert with directories",
            "Ctrl+C/Alt+C - Copy paths/names",
            "Ctrl+F3..F6 - Sort name/ext/date/size",
            "Ctrl+R - Reload both panels",
            "Type to search, Esc to clear",
        ],
    ),
    (
        "About",
        &[
            concat!("FM84 - File Manager '84, v", env!("CARGO_PKG_VERSION")),
            env!("CARGO_PKG_DESCRIPTION"),
            "Built with Rust, Ratatui and Crossterm",
            "Copyright (c) 2025-2026 Colt Willcox",
            concat!("Released under the ", env!("CARGO_PKG_LICENSE"), " License"),
            env!("CARGO_PKG_REPOSITORY"),
        ],
    ),
];

/// F1's lines as they are drawn, and whether each is a heading: a heading
/// over each part, with a blank row before every one but the first, and the
/// lines under it set in from it - laid out as F11 lays out its options.
pub fn help_lines() -> Vec<(String, bool)> {
    let mut lines = Vec::new();
    for (heading, items) in HELP {
        if !lines.is_empty() {
            lines.push((String::new(), false));
        }
        lines.push((format!(" {heading}"), true));
        lines.extend(items.iter().map(|item| (format!("   {item}"), false)));
    }
    lines
}

/// F1: the keys, over the panels and no taller than them. What does not fit
/// scrolls, from `scroll` lines down, and the corner says which lines show.
/// Gives back how many lines there is room for, for the keys to scroll by.
pub(super) fn render_help_popup(f: &mut ratatui::Frame<'_>, area: Rect, scroll: usize) -> usize {
    // A border and a blank row above and below the lines.
    let popup_width = area.width as usize * POPUP_WIDTH as usize / 100;
    let help = help_lines();
    let popup_area = centered(screen_layout(area)[2], popup_width.max(1), help.len() + 4);
    let inner = popup_area.inner(Margin { vertical: 2, horizontal: 2 });
    let room = (inner.height as usize).max(1);
    let first = scroll.min(help.len().saturating_sub(room));

    let mut popup_block = Block::default().title(Line::from(Span::styled(" Help/About ", style_title())).centered()).borders(Borders::ALL).style(style_border());
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
        .map(|(text, heading)| {
            let style = if *heading { style_title().add_modifier(Modifier::BOLD) } else { style_title() };
            Line::from(Span::styled(format!("{text:<max_len$}"), style))
        })
        .collect();
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
    room
}
