//! Alt+R: a directory's dossier, a section at a time.

use super::*;
use crate::app::{DOSSIER_WEEKS, Dossier, DossierView};

/// The kinds of file listed by name; the rest are summed up under one row.
const KINDS: usize = 8;
/// The languages listed.
const LANGUAGES: usize = 8;
/// Cells in a share's bar.
const BAR: u64 = 10;

/// `part` of `whole` as a bar, in the glyphs the disk meter uses.
fn bar(part: u64, whole: u64) -> String {
    let filled = part.saturating_mul(BAR).checked_div(whole).unwrap_or(0).min(BAR);
    "▪".repeat(filled as usize) + &"▫".repeat((BAR - filled) as usize)
}

/// Commits a week as a line of eighth blocks, a week a cell: none a dot,
/// the busiest week the full block.
fn sparkline(weeks: &[u32; DOSSIER_WEEKS]) -> String {
    const STEPS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let most = weeks.iter().copied().max().unwrap_or(0).max(1);
    weeks
        .iter()
        .map(|&count| if count == 0 { '·' } else { STEPS[((count as usize * STEPS.len()).div_ceil(most as usize)).clamp(1, STEPS.len()) - 1] })
        .collect()
}

/// Every row the dossier has, the headings among them.
pub(super) fn dossier_lines(dossier: &Dossier, date_format: crate::options::DateFormat) -> Vec<Line<'static>> {
    let heading = |text: &str| Line::from(Span::styled(text.to_string(), style_title().add_modifier(Modifier::BOLD)));
    let row = |parts: Vec<(String, Style)>| Line::from(std::iter::once(Span::raw("  ")).chain(parts.into_iter().map(|(text, style)| Span::styled(text, style))).collect::<Vec<_>>());
    let path = |path: &std::path::Path| printable_name(&path.to_string_lossy());
    let mut lines = vec![
        Line::from(Span::styled(
            t!("Files: {files}   Directories: {dirs}   Size: {size}", files = grouped(dossier.files), dirs = grouped(dossier.dirs), size = format_size(dossier.bytes)),
            style_title(),
        )),
        Line::from(""),
    ];

    if !dossier.kinds.is_empty() {
        lines.push(heading(t!("Kinds of file")));
        let named: Vec<(String, u64, u64)> = dossier.kinds.iter().take(KINDS).cloned().collect();
        let (other_bytes, other_files) = dossier.kinds.iter().skip(KINDS).fold((0, 0), |(bytes, files), kind| (bytes + kind.1, files + kind.2));
        let mut rows: Vec<(String, u64, u64)> = named
            .into_iter()
            .map(|(extension, bytes, files)| (if extension.is_empty() { t!("(no extension)").to_string() } else { format!(".{extension}") }, bytes, files))
            .collect();
        if other_files > 0 {
            rows.push((t!("others").to_string(), other_bytes, other_files));
        }
        let width = rows.iter().map(|(name, _, _)| display_width(name)).max().unwrap_or(0);
        for (name, bytes, files) in rows {
            let percent = (bytes as f64 * 100.0 / dossier.bytes.max(1) as f64).round() as u64;
            let pad = " ".repeat(width.saturating_sub(display_width(&name)));
            lines.push(row(vec![
                (bar(bytes, dossier.bytes), style_dir()),
                (format!(" {percent:>3}%  "), style_columns()),
                (format!("{name}{pad}  "), style_file()),
                (format!("{:>10}  ×{}", format_size(bytes), grouped(files)), style_columns()),
            ]));
        }
        lines.push(Line::from(""));
    }

    if !dossier.largest.is_empty() {
        lines.push(heading(t!("Largest")));
        for (at, size) in &dossier.largest {
            lines.push(row(vec![(format!("{:>10}  ", format_size(*size)), style_columns()), (path(at), style_file())]));
        }
        lines.push(Line::from(""));
    }

    if !dossier.newest.is_empty() {
        lines.push(heading(t!("Newest")));
        let now = std::time::SystemTime::now();
        let dates: Vec<String> = dossier.newest.iter().map(|(_, modified)| format_modified(*modified, date_format, now)).collect();
        let width = dates.iter().map(|date| display_width(date)).max().unwrap_or(0);
        for ((at, _), date) in dossier.newest.iter().zip(dates) {
            let pad = " ".repeat(width.saturating_sub(display_width(&date)));
            lines.push(row(vec![(format!("{pad}{date}  "), style_columns()), (path(at), style_file())]));
        }
        lines.push(Line::from(""));
    }

    if !dossier.languages.is_empty() {
        lines.push(heading(t!("Languages, by lines")));
        let most = dossier.languages.first().map_or(1, |(_, lines, _)| *lines);
        let width = dossier.languages.iter().take(LANGUAGES).map(|(name, _, _)| display_width(name)).max().unwrap_or(0);
        for (name, count, _) in dossier.languages.iter().take(LANGUAGES) {
            let pad = " ".repeat(width.saturating_sub(display_width(name)));
            lines.push(row(vec![(bar(*count, most), style_dir()), (format!("  {name}{pad}  "), style_file()), (format!("{:>10}", grouped(*count)), style_columns())]));
        }
        lines.push(Line::from(""));
    }

    if let Some(weeks) = &dossier.weeks {
        lines.push(heading(t!("Git, the last 8 weeks")));
        let commits: u32 = weeks.iter().sum();
        lines.push(row(vec![(sparkline(weeks), style_dir()), (format!("  {}", t!("Commits: {count}", count = grouped(commits.into()))), style_columns())]));
        lines.push(Line::from(""));
    }

    lines.push(heading(t!("Duplicates")));
    lines.push(row(vec![(
        if dossier.copies == 0 {
            t!("None found").to_string()
        } else {
            t!("Copies of another: {count}, taking {size}", count = grouped(dossier.copies), size = format_size(dossier.wasted))
        },
        style_file(),
    )]));
    lines
}

/// Returns the rows the dossier had room for, and its scroll kept to them.
pub(super) fn render_dossier_popup(f: &mut ratatui::Frame<'_>, area: Rect, view: &DossierView, date_format: crate::options::DateFormat) -> (usize, usize) {
    let popup_area = popup_rect(80, area);
    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let rows = dossier_lines(&view.dossier, date_format);
    // A blank row, and the key, below.
    let room = (inner.height as usize).saturating_sub(2).max(1);
    let scroll = view.scroll.min(rows.len().saturating_sub(room));

    let title = format!(" {} ", t!("Dossier: {name}", name = shown_name(&view.dossier.dir)));
    let mut block = Block::default().title(Line::from(Span::styled(title, style_title())).centered()).borders(Borders::ALL).style(style_border());
    if room < rows.len() {
        let shown = format!(" {}-{}/{} ", scroll + 1, (scroll + room).min(rows.len()), rows.len());
        block = block.title_bottom(Line::from(Span::styled(shown, style_columns())).right_aligned());
    }
    clear(f, popup_area);
    f.render_widget(block, popup_area);
    f.render_widget(Paragraph::new(rows.into_iter().skip(scroll).take(room).collect::<Vec<_>>()), inner);
    let bottom = Rect {
        y: inner.bottom().saturating_sub(1),
        height: 1.min(inner.height),
        ..inner
    };
    f.render_widget(Paragraph::new(key_hints(t!("Esc - Close"))), bottom);
    (room, scroll)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weeks_are_drawn_against_the_busiest() {
        assert_eq!(sparkline(&[0, 1, 2, 4, 8, 0, 0, 8]), "·▁▂▄█··█");
        assert_eq!(sparkline(&[0; DOSSIER_WEEKS]), "········");
        assert_eq!(bar(1, 4), "▪▪▫▫▫▫▫▫▫▫");
        assert_eq!(bar(0, 0), "▫".repeat(10));
    }
}
