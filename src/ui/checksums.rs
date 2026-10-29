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
    // One size whatever it holds, and whichever algorithm is shown: as wide
    // as every popup, and as tall as the larger ones. What does not fit
    // wraps, and scrolls.
    let popup_area = popup_rect(60, area);
    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let rows: Vec<String> = view.report.lines().iter().flat_map(|line| rows_of(&printable_name(line), (inner.width as usize).max(1))).collect();

    let (title, keys): (String, &[&str]) = match &view.report {
        Report::Computed { .. } => (" Checksums ".to_string(), &["Tab - Algorithm", "C - Copy", "Enter - Write", "Esc - Close"]),
        Report::Checked { file, .. } => (format!(" Checked against {} ", shown_name(file)), &["C - Copy", "Esc - Close"]),
    };
    // As many rows as the keys take at this width, each key whole.
    let keys = keys_in_rows(keys, inner.width as usize);

    // Above the list, what it sums up to, and for checksums worked out, the
    // algorithms there are to choose between - all worked out already, the
    // one shown marked.
    let mut head = vec![Line::from(Span::styled(view.report.summary(), style_title())), Line::from("")];
    if let Some(shown) = view.report.shown() {
        let mut options = Vec::new();
        for algorithm in crate::checksum::Algorithm::ALL {
            if !options.is_empty() {
                options.push(Span::raw("  "));
            }
            let style = if algorithm == shown { style_selection() } else { style_columns() };
            options.push(Span::styled(format!(" {} ", algorithm.name()), style));
        }
        head.extend([Line::from(options), Line::from("")]);
    }

    // The head above, a blank row and the keys below.
    let room = (inner.height as usize).saturating_sub(head.len() + 1 + keys.len()).max(1);
    view.room = room;
    view.scroll = view.scroll.min(rows.len().saturating_sub(room));

    let mut block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());
    if room < rows.len() {
        let shown = format!(" {}-{}/{} ", view.scroll + 1, (view.scroll + room).min(rows.len()), rows.len());
        block = block.title_bottom(Line::from(Span::styled(shown, style_columns())).right_aligned());
    }
    clear(f, popup_area);
    f.render_widget(block, popup_area);

    let mut shown = head;
    shown.extend(rows.into_iter().skip(view.scroll).take(room).map(|row| Line::from(Span::styled(row, style_file()))));
    f.render_widget(Paragraph::new(shown), inner);
    // The keys on the bottom rows, wherever the list ends.
    let height = (keys.len() as u16).min(inner.height);
    let bottom = Rect { y: inner.bottom() - height, height, ..inner };
    f.render_widget(Paragraph::new(keys.into_iter().map(|row| Line::from(Span::styled(row, style_columns()))).collect::<Vec<_>>()), bottom);
}

/// Keys laid out as rows of `width` columns, apart as every popup sets them
/// apart, breaking between keys and never inside one.
fn keys_in_rows(keys: &[&str], width: usize) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    for key in keys {
        match rows.last_mut() {
            Some(row) if display_width(row) + 4 + display_width(key) <= width => {
                row.push_str("    ");
                row.push_str(key);
            }
            _ => rows.push((*key).to_string()),
        }
    }
    rows
}
