//! Alt+N's and Alt+Shift+N's popups: the fm84s found listening, to pick
//! one to send to; and this one listening, with who is asking or sending.

use super::*;
use crate::app::{ListenView, PickView};

/// Alt+Shift+N: who this machine is to the others, where what they send
/// goes, and how it is going - or the question to a sender asking.
pub(super) fn render_null_modem_listen(f: &mut ratatui::Frame<'_>, area: Rect, view: &ListenView) {
    let popup_area = popup_rect(40, area);
    let block = Block::default()
        .title(Line::from(Span::styled(format!(" {} ", t!("Null modem")), style_title())).centered())
        .borders(Borders::ALL)
        .border_type(crate::display::border_type())
        .style(style_border());
    clear(f, popup_area);
    f.render_widget(block, popup_area);
    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;

    let dir = view.listening.dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| view.listening.dir.display().to_string());
    let mut lines = vec![
        Line::from(Span::styled(cut_to(&t!("Listening as {name}, port {port}", name = printable_name(&view.name), port = view.listening.port), width), style_title())),
        Line::from(Span::styled(cut_to(&t!("Files go into {dir}", dir = printable_name(&dir)), width), style_file())),
        Line::from(""),
    ];
    let hint = if let Some(asking) = &view.asking {
        let what = tn!(asking.files, "{count} file", "{count} files", count = asking.files);
        lines.push(Line::from(Span::styled(
            cut_to(&t!("{name} wants to send {what} ({size})", name = printable_name(&asking.from), what, size = format_size(asking.bytes)), width),
            style_title().add_modifier(Modifier::BOLD),
        )));
        t!("Y - Accept    N - Refuse")
    } else {
        let status = if let Some(message) = &view.failed {
            t!("Cannot listen: {e}", e = printable_name(message))
        } else if let Some((name, got, size)) = &view.coming {
            t!("Receiving {name}: {got} of {size}", name = printable_name(name), got = format_size(*got), size = format_size(*size))
        } else if let Some(from) = &view.sender {
            t!("Connected to {name}", name = printable_name(from))
        } else if let Some((name, reason)) = &view.lost {
            t!("Not received {name}: {reason}", name = printable_name(name), reason = printable_name(reason))
        } else if let Some((from, complete)) = &view.ended {
            let files = t!("Files received: {count} ({size})", count = view.received, size = format_size(view.received_bytes));
            if *complete {
                format!("{files} - {}", t!("from {name}", name = printable_name(from)))
            } else {
                format!("{files} - {}", t!("{name} went away", name = printable_name(from)))
            }
        } else if view.received > 0 {
            t!("Files received: {count} ({size})", count = view.received, size = format_size(view.received_bytes))
        } else {
            t!("Waiting for another fm84 to send (Alt+N there)…").to_string()
        };
        lines.push(Line::from(Span::styled(cut_to(&status, width), style_columns())));
        t!("Esc - Stop listening")
    };
    lines.push(Line::from(""));
    lines.extend(hint_rows(&[hint], width));
    f.render_widget(Paragraph::new(lines), inner);
}

/// Alt+N: the fm84s listening, by who each is and where what is sent goes,
/// as they are heard from - and how much is to go.
pub(super) fn render_null_modem_pick(f: &mut ratatui::Frame<'_>, area: Rect, view: &PickView) {
    let popup_area = popup_rect(50, area);
    let title = format!(" {} ", t!("Null modem"));
    let block = Block::default()
        .title(Line::from(Span::styled(title, style_title())).centered())
        .borders(Borders::ALL)
        .border_type(crate::display::border_type())
        .style(style_border());
    clear(f, popup_area);
    f.render_widget(block, popup_area);
    let inner = popup_area.inner(Margin { vertical: 1, horizontal: 2 });
    let width = inner.width as usize;
    let hints = hint_rows(&[t!("Enter - Send    Esc - Close")], width);
    let rows = (inner.height as usize).saturating_sub(2 + hints.len());

    let count = view.items.len();
    let what = tn!(count, "{count} entry", "{count} entries", count);
    let mut lines = vec![Line::from(Span::styled(cut_to(&t!("Send {what} to:", what), width), style_title()))];
    if view.peers.is_empty() {
        lines.push(Line::from(Span::styled(cut_to(t!("Looking for fm84s listening…"), width), style_columns())));
        lines.push(Line::from(Span::styled(cut_to(t!("Alt+Shift+N on the other machine listens."), width), style_file())));
    }
    let name_width = view.peers.iter().map(|(peer, _)| display_width(&printable_name(&peer.name))).max().unwrap_or(0).min(width / 2);
    let first = if rows > 0 && view.index >= rows { view.index + 1 - rows } else { 0 };
    for (index, (peer, _)) in view.peers.iter().enumerate().skip(first).take(rows) {
        let name = cut_to(&printable_name(&peer.name), name_width);
        let name = format!("{name}{}", " ".repeat(name_width.saturating_sub(display_width(&name))));
        let detail = format!("{}  {}", printable_name(&peer.dir), peer.address.ip());
        let detail = cut_to(&detail, width.saturating_sub(name_width + 3));
        let pad = " ".repeat(width.saturating_sub(name_width + 3 + display_width(&detail)));
        let (style, quiet) = if index == view.index {
            (style_title().bg(palette().selected_background), style_columns().bg(palette().selected_background))
        } else {
            (style_dir(), style_columns())
        };
        lines.push(Line::from(vec![Span::styled(format!("{name}   "), style), Span::styled(format!("{detail}{pad}"), quiet)]));
    }
    while lines.len() < rows + 2 {
        lines.push(Line::from(""));
    }
    lines.extend(hints);
    f.render_widget(Paragraph::new(lines), inner);
}
