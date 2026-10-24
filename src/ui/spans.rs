//! Pieces of a line of text, cut, marked and scrolled: what the viewer, the
//! editor and the diff draw their lines with.

use super::*;

/// A line of the viewer laid out for drawing: which line it is, its
/// characters, and where the matches on it are.
pub(super) type LaidOut = (usize, Vec<char>, Vec<(usize, usize)>);

/// A line of image characters, one span per run of the same colour.
pub(super) fn colored_spans(text: &str, colors: &[ratatui::style::Color], backgrounds: Option<&[ratatui::style::Color]>) -> Vec<Span<'static>> {
    // A run has to agree on both colours, so a background breaks one wherever
    // either changes. Since the background is derived from the same pixel, that
    // is the same place the foreground already broke.
    let paint = |(fg, bg): (ratatui::style::Color, Option<ratatui::style::Color>)| match bg {
        Some(bg) => Style::new().fg(fg).bg(bg),
        None => Style::new().fg(fg),
    };

    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_color: Option<(ratatui::style::Color, Option<ratatui::style::Color>)> = None;
    for (index, character) in text.chars().enumerate() {
        let Some(&color) = colors.get(index) else {
            break;
        };
        let pair = (color, backgrounds.and_then(|row| row.get(index)).copied());
        if let Some(previous) = run_color
            && previous != pair
        {
            spans.push(Span::styled(std::mem::take(&mut run), paint(previous)));
        }
        run_color = Some(pair);
        run.push(character);
    }
    if let Some(pair) = run_color {
        spans.push(Span::styled(run, paint(pair)));
    }
    spans
}

/// Visual column of a character index, with tabs expanded the way the editor
/// draws them.
pub(super) fn visual_column(line: &str, char_col: usize) -> usize {
    line.chars().take(char_col).map(|c| if c == '\t' { tab_width() } else { 1 }).sum()
}

/// Merge `extra` into the styles covering visual columns [from, to), splitting
/// spans at the boundaries so the syntax colours survive underneath.
pub(super) fn overlay_range(spans: Vec<Span<'static>>, from: usize, to: usize, extra: Style) -> Vec<Span<'static>> {
    if from >= to {
        return spans;
    }

    let mut result = Vec::with_capacity(spans.len() + 2);
    let mut column = 0;
    for span in spans {
        let text = span.content.into_owned();
        let length = text.chars().count();
        let span_end = column + length;

        if span_end <= from || column >= to {
            result.push(Span::styled(text, span.style));
        } else {
            let characters: Vec<char> = text.chars().collect();
            let head = from.saturating_sub(column).min(length);
            let tail = to.saturating_sub(column).min(length);
            if head > 0 {
                result.push(Span::styled(characters[..head].iter().collect::<String>(), span.style));
            }
            result.push(Span::styled(characters[head..tail].iter().collect::<String>(), span.style.patch(extra)));
            if tail < length {
                result.push(Span::styled(characters[tail..].iter().collect::<String>(), span.style));
            }
        }
        column = span_end;
    }
    result
}

/// Drop the first `columns` display columns of a line, for horizontal
/// scrolling. Paragraph::scroll would do it, but it takes a u16: on a
/// minified file, whose lines run past 65,535 columns, the offset wrapped
/// round and the view jumped back to the start. A wide character cut in two
/// leaves a space where its second half was.
pub(super) fn skip_columns(spans: Vec<Span<'static>>, columns: usize) -> Vec<Span<'static>> {
    if columns == 0 {
        return spans;
    }
    let mut left = columns;
    let mut result = Vec::with_capacity(spans.len());
    for span in spans {
        if left == 0 {
            result.push(span);
            continue;
        }
        let mut kept = String::new();
        for character in span.content.chars() {
            if left == 0 {
                kept.push(character);
                continue;
            }
            let width = UnicodeWidthChar::width(character).unwrap_or(0);
            if width > left {
                kept.extend(std::iter::repeat_n(' ', width - left));
                left = 0;
            } else {
                left -= width;
            }
        }
        if !kept.is_empty() {
            result.push(Span::styled(kept, span.style));
        }
    }
    result
}

/// Draw the cursor cell, padding out to it when it sits past end-of-line.
pub(super) fn place_cursor(mut spans: Vec<Span<'static>>, column: usize, style: Style) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|span| span.content.chars().count()).sum();
    if column < total {
        return overlay_range(spans, column, column + 1, style);
    }
    if column > total {
        spans.push(Span::raw(" ".repeat(column - total)));
    }
    spans.push(Span::styled(" ", style));
    spans
}
