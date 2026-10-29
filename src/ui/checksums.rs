//! Alt+H: the checksums worked out, or how the files a checksum file names
//! came out against it.

use super::*;
use crate::app::ChecksumView;
use crate::checksum::Report;

/// A line cut into rows of `width` columns, by the character: a checksum
/// has no spaces to break at, and is no use shown in part.
fn rows_of(line: &str, width: usize) -> Vec<String> {
    let mut rows = vec![String::new()];
    let mut used = 0;
    for character in line.chars() {
        let cell = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if used + cell > width && used > 0 {
            rows.push(String::new());
            used = 0;
        }
        rows.last_mut().expect("never empty").push(character);
        used += cell;
    }
    rows
}

pub(super) fn render_checksums_popup(f: &mut ratatui::Frame<'_>, area: Rect, view: &mut ChecksumView) {
    let lines: Vec<String> = view.report.lines().iter().map(|line| printable_name(line)).collect();
    // As wide as the longest line wants, between the width every popup has
    // and the screen's: a SHA-256 and a name fit whole where they can.
    let longest = lines.iter().map(|line| display_width(line)).max().unwrap_or(0);
    let least = area.width as usize * POPUP_WIDTH as usize / 100;
    let width = (longest + 6).clamp(least, (area.width as usize).saturating_sub(2).max(least));
    let inner_width = width.saturating_sub(4).max(1);
    let rows: Vec<String> = lines.iter().flat_map(|line| rows_of(line, inner_width)).collect();

    let (title, keys) = match &view.report {
        Report::Computed { algorithm, .. } => {
            let file = view.report.sums_file().and_then(|(path, _)| path.file_name().map(|name| name.to_string_lossy().into_owned()));
            let write = file.map(|name| format!("W - Write {name}    ")).unwrap_or_default();
            (format!(" {} ", algorithm.name()), format!("C - Copy    {write}Tab - {}    Esc - Close", algorithm.next().name()))
        }
        Report::Checked { file, .. } => (format!(" Checked against {} ", shown_name(file)), "C - Copy    Esc - Close".to_string()),
    };

    // A border, the summary and a blank row above, a blank row and the keys
    // below.
    let popup_area = centered(screen_layout(area)[2], width, rows.len() + 6);
    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let room = (inner.height as usize).saturating_sub(4).max(1);
    view.room = room;
    view.scroll = view.scroll.min(rows.len().saturating_sub(room));

    let mut block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());
    if room < rows.len() {
        let shown = format!(" {}-{}/{} ", view.scroll + 1, (view.scroll + room).min(rows.len()), rows.len());
        block = block.title_bottom(Line::from(Span::styled(shown, style_columns())).right_aligned());
    }
    clear(f, popup_area);
    f.render_widget(block, popup_area);

    let mut shown = vec![Line::from(Span::styled(view.report.summary(), style_title())), Line::from("")];
    shown.extend(rows.into_iter().skip(view.scroll).take(room).map(|row| Line::from(Span::styled(row, style_file()))));
    shown.push(Line::from(""));
    shown.push(Line::from(Span::styled(keys, style_columns())));
    f.render_widget(Paragraph::new(shown), inner);
}
