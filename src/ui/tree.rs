//! Alt+F10's tree, drawn in place of a panel's files: a line for each
//! directory on show, the lines between them as ncd drew them, each marked
//! for whether it is open, closed, being read or holds nothing.

use super::*;
use crate::app::{Tree, TreeMark, TreeRow};

/// What goes before a row's name: the lines down from the levels above it,
/// its own branch, and its mark - in box lines, or ASCII with plain icons.
pub fn tree_prefix(row: &TreeRow, plain: bool) -> String {
    let (down, none, branch, last) = if plain { ("|  ", "   ", "|-", "`-") } else { ("│  ", "   ", "├─", "└─") };
    let mut prefix = String::new();
    if let Some((own, above)) = row.last.split_last() {
        for &was_last in above {
            prefix.push_str(if was_last { none } else { down });
        }
        prefix.push_str(if *own { last } else { branch });
    }
    prefix.push_str(match (row.mark, plain) {
        (TreeMark::Open, false) => "▾ ",
        (TreeMark::Closed | TreeMark::Unread, false) => "▸ ",
        (TreeMark::Open, true) => "- ",
        (TreeMark::Closed | TreeMark::Unread, true) => "+ ",
        (TreeMark::Reading, _) => "… ",
        (TreeMark::Empty, false) => "─ ",
        (TreeMark::Empty, true) => "- ",
    });
    prefix
}

/// `tree` in `area`, the inside of the panel's border: a heading where the
/// table's is, then the rows, scrolled to keep the cursor in the middle as
/// the table keeps its. The directory the panel lists is set apart.
/// Where the rows went, and the row the first is, for the mouse.
pub(super) fn render_tree(f: &mut ratatui::Frame<'_>, area: Rect, tree: &Tree, active: bool, here: &Path, plain: bool) -> (Rect, usize) {
    clear(f, area);
    // A column in from the border, as the files' icons sit.
    let area = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(1),
        ..area
    };
    let width = area.width as usize;
    let room = area.height.saturating_sub(1) as usize;
    let mut lines = vec![Line::from(Span::styled(format!("{:<width$}", t!("Directory tree")), style_columns().add_modifier(Modifier::BOLD)))];

    let total = tree.rows.len();
    let half = room / 2;
    let start = if tree.cursor <= half {
        0
    } else if tree.cursor + half >= total {
        total.saturating_sub(room)
    } else {
        tree.cursor - half
    };
    let cursor_style = Style::default()
        .bg(if active { palette().selected_background } else { palette().selected_background_inactive })
        .fg(palette().selected_foreground)
        .add_modifier(Modifier::BOLD);
    for (index, row) in tree.rows.iter().enumerate().skip(start).take(room) {
        let prefix = tree_prefix(row, plain);
        // The root by its whole path, the rest by their names.
        let name = if row.last.is_empty() {
            row.path.display().to_string()
        } else {
            row.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
        };
        let name = printable_name(&name);
        let text = format!("{prefix}{name}");
        let text = if display_width(&text) > width { text.chars().take(width).collect() } else { text };
        let pad = " ".repeat(width.saturating_sub(display_width(&text)));
        let style = if index == tree.cursor {
            cursor_style
        } else if row.path == here {
            style_dir().add_modifier(Modifier::BOLD)
        } else {
            style_dir()
        };
        lines.push(Line::from(vec![Span::styled(text, style), Span::styled(pad, style)]));
    }
    f.render_widget(Paragraph::new(lines), area);
    (
        Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        },
        start,
    )
}
