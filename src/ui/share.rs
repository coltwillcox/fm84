//! Alt+Q's and Alt+Shift+Q's popups: the code a phone scans, the address it
//! opens, and how the download, or what is being sent, is going.

use super::*;
use crate::app::{ReceiveView, ShareStatus, ShareView};
use ratatui::style::Color;

/// Light modules round the code on each side, which a camera needs to find
/// where it starts.
const QUIET: usize = 2;

/// Blank rows between the title and the code.
const MARGIN: usize = 1;

/// Alt+Q: the code, then the name of the download, the address, how it is
/// going and the key that stops it.
pub(super) fn render_share_popup(f: &mut ratatui::Frame<'_>, area: Rect, view: &ShareView) {
    let name = t!("Scan to download {name}", name = printable_name(&view.serving.name));
    let status = match &view.status {
        ShareStatus::Waiting => t!("Waiting for the download…").to_string(),
        ShareStatus::Sending { to, sent } => match view.serving.total {
            Some(total) => t!("Sending to {to}: {sent} of {total}", to, sent = format_size(*sent), total = format_size(total)),
            None => t!("Sending to {to}: {sent}", to, sent = format_size(*sent)),
        },
        ShareStatus::Done(to) => t!("Sent to {to}", to),
        ShareStatus::Failed(message) => t!("Cannot share: {e}", e = printable_name(message)),
    };
    // Sent, it goes on answering - a phone may ask again - until Esc; only a
    // server that could not go on is over.
    let finished = matches!(view.status, ShareStatus::Failed(_));
    let hint = keys(finished, view.addresses.len(), t!("Esc - Stop sharing"));
    let url = numbered(view.url(), view.chosen, view.addresses.len());
    render_code_popup(f, area, t!("Share over the network"), &view.code, [name, url, status], &hint);
}

/// Alt+Shift+Q: the code, then where the files go, the address, what has
/// come or is coming, and the key that stops it.
pub(super) fn render_receive_popup(f: &mut ratatui::Frame<'_>, area: Rect, view: &ReceiveView) {
    let dir = view.receiving.dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| view.receiving.dir.display().to_string());
    let into = t!("Scan to send files into {dir}", dir = printable_name(&dir));
    let status = if let Some(message) = &view.failed {
        t!("Cannot receive: {e}", e = printable_name(message))
    } else if let Some((_, name, got, size)) = view.coming.first() {
        let receiving = t!("Receiving {name}: {got} of {size}", name = printable_name(name), got = format_size(*got), size = format_size(*size));
        // More than one at once: how many more.
        match view.coming.len() {
            1 => receiving,
            count => format!("{receiving} (+{})", count - 1),
        }
    } else if let Some((name, reason)) = &view.lost {
        t!("Not received {name}: {reason}", name = printable_name(name), reason = printable_name(reason))
    } else if view.received > 0 {
        t!("Files received: {count} ({size})", count = view.received, size = format_size(view.received_bytes))
    } else {
        t!("Waiting for files…").to_string()
    };
    let hint = keys(view.failed.is_some(), view.addresses.len(), t!("Esc - Stop receiving"));
    let url = numbered(view.url(), view.chosen, view.addresses.len());
    render_code_popup(f, area, t!("Receive from a phone"), &view.code, [into, url, status], &hint);
}

/// The keys under a code: Esc to close it once it can do no more, or to stop
/// it, with Tab before it when there is another address to show.
fn keys(finished: bool, addresses: usize, stop: &str) -> String {
    match (finished, addresses > 1) {
        (true, _) => t!("Esc - Close").to_string(),
        (false, true) => format!("{}    {}", t!("Tab - Another address"), stop),
        (false, false) => stop.to_string(),
    }
}

/// The address, and which of the machine's it is when it has more than one.
fn numbered(url: String, chosen: usize, count: usize) -> String {
    if count > 1 { format!("{url}  {}/{count}", chosen + 1) } else { url }
}

/// A popup of a code for a phone to scan: `title` over it, the code a row
/// below, then `text` - what it is for, the address, how it is going - and
/// `hint`'s keys. Over the whole screen rather than the panels: a code of an
/// address takes more rows than a popup there has. A terminal too small for
/// it shows the address alone, to be typed in.
fn render_code_popup(f: &mut ratatui::Frame<'_>, area: Rect, title: &str, code: &(usize, Vec<bool>), text: [String; 3], hint: &str) {
    let (modules_wide, modules) = (&code.0, &code.1);
    let side = modules_wide + 2 * QUIET;
    // Two rows of modules to a row of text, each a half block.
    let code_rows = side.div_ceil(2);

    let width = (area.width as usize * POPUP_WIDTH as usize / 100).max(side + 4).min(area.width as usize);
    let inner_width = width.saturating_sub(4);
    let hints = hint_rows(&[hint], inner_width);
    // The code with a blank row between it and the title above. Then the
    // text and the keys - with a blank row above and one before the keys,
    // where the screen has the rows: an 80 by 24 terminal has the code's
    // but not theirs.
    let code_height = MARGIN + code_rows;
    let roomy = |rows: usize| 5 + hints.len() + rows + 2 <= area.height as usize;
    let shows_code = area.width as usize >= side + 4 && 3 + hints.len() + code_height + 2 <= area.height as usize;
    let spaced = if shows_code { roomy(code_height) } else { roomy(1) };
    let text_rows = 3 + hints.len() + if spaced { 2 } else { 0 };
    let height = (if shows_code { code_height } else { 1 }) + text_rows + 2;
    let popup_area = centered(area, width, height);
    let block = Block::default()
        .title(Line::from(Span::styled(format!(" {title} "), style_title())).centered())
        .borders(Borders::ALL)
        .border_type(crate::display::border_type())
        .style(style_border());
    clear(f, popup_area);
    f.render_widget(block, popup_area);
    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let mut lines = Vec::new();
    if shows_code {
        // Black on white, whatever the theme: what a camera reads best.
        let (dark, light) = (Color::Rgb(0, 0, 0), Color::Rgb(255, 255, 255));
        let module = |row: usize, column: usize| -> bool {
            let (row, column) = (row.wrapping_sub(QUIET), column.wrapping_sub(QUIET));
            row < *modules_wide && column < *modules_wide && modules[row * modules_wide + column]
        };
        let left = inner.x + (inner.width.saturating_sub(side as u16)) / 2;
        let buffer = f.buffer_mut();
        for text_row in 0..code_rows.min((inner.height as usize).saturating_sub(MARGIN)) {
            for column in 0..side {
                let (top, bottom) = (module(text_row * 2, column), module(text_row * 2 + 1, column));
                let cell = &mut buffer[(left + column as u16, inner.y + (MARGIN + text_row) as u16)];
                cell.set_symbol("▀").set_fg(if top { dark } else { light }).set_bg(if bottom { dark } else { light });
            }
        }
    } else {
        lines.push(Line::from(Span::styled(cut_to(t!("Too small for the code: type the address on the phone"), inner_width), style_columns())).centered());
    }
    if spaced {
        lines.push(Line::from(""));
    }
    let [what, url, status] = text;
    lines.push(Line::from(Span::styled(cut_to(&what, inner_width), style_file())).centered());
    lines.push(Line::from(Span::styled(cut_to(&url, inner_width), style_title().add_modifier(Modifier::BOLD))).centered());
    lines.push(Line::from(Span::styled(cut_to(&status, inner_width), style_columns())).centered());
    if spaced {
        lines.push(Line::from(""));
    }
    lines.extend(hints);
    // Under the code, which is drawn straight into the cells.
    let above = if shows_code { code_height as u16 } else { 0 };
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            y: inner.y + above,
            height: inner.height.saturating_sub(above),
            ..inner
        },
    );
}
