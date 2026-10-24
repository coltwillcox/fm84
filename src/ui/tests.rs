use super::*;

/// The branch follows the path, and is the first to go when the path
/// needs the room; who and where sit by the logo while they fit.
#[test]
fn the_branch_after_the_path_and_who_by_the_logo() {
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.dir_left = PathBuf::from("/home/colt/fm84");
    app_state.branch_left = Some("main".to_string());
    app_state.user_at_host = ("colt@laserbeak".to_string(), false);
    let row = |width: u16, y: u16, app_state: &mut AppState| {
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, 30)).unwrap();
        render_ui(&mut terminal, app_state);
        (0..width).map(|x| terminal.backend().buffer()[(x, y)].symbol().to_string()).collect::<String>()
    };

    let path_bar = row(100, 3, &mut app_state);
    assert!(path_bar.contains(&format!("/home/colt/fm84 ─ {ICON_BRANCH} main ─")), "{path_bar}");
    let top = row(100, 0, &mut app_state);
    assert!(top.contains(&format!("{} ─ colt@laserbeak ─", logo_icon(IconStyle::NerdFont))), "{top}");
    app_state.options.icon_style = IconStyle::Plain;
    assert!(row(100, 3, &mut app_state).contains("/home/colt/fm84 ─ git:main ─"));

    // Too narrow for both: the path stays and the branch goes, and so does
    // who and where.
    let narrow = row(44, 3, &mut app_state);
    assert!(!narrow.contains("git:main"), "{narrow}");
    assert!(!row(44, 0, &mut app_state).contains("colt@"));
}

/// A tab is clicked where it is drawn, and a long name is cut short.
#[test]
fn tabs_are_drawn_and_clicked_where_they_are() {
    assert_eq!(tab_label(Path::new("/")), "/");
    assert_eq!(tab_label(Path::new("/home/colt/src")), "src");
    assert_eq!(tab_label(Path::new("/tmp/a-rather-long-directory-name")), "a-rather-long-d…");

    let mut app_state = AppState::new();
    app_state.is_left_active = true;
    app_state.dir_left = "/tmp/b".into();
    app_state.tabs_left = crate::app::Tabs::new(vec!["/a".into(), "/tmp/b".into()], 1);
    tab_strip(&mut app_state, true, Rect::new(0, 1, 40, 1));

    // " " then " a " then " b ".
    assert_eq!(app_state.tab_at(0, 1), None);
    assert_eq!(app_state.tab_at(1, 1), Some((true, StripHit::Item(0))));
    assert_eq!(app_state.tab_at(3, 1), Some((true, StripHit::Item(0))));
    assert_eq!(app_state.tab_at(4, 1), Some((true, StripHit::Item(1))));
    assert_eq!(app_state.tab_at(7, 1), None);
    assert_eq!(app_state.tab_at(4, 0), None);
}

/// Too many drives for the half they are in: a window onto them, with
/// arrows that are there only while there is more that way, and every
/// click mapped to what is drawn under it.
#[test]
fn a_crowded_drive_strip_scrolls() {
    use crate::fs_ops::{Mount, MountKind};
    let mut app_state = AppState::new();
    app_state.options.icon_style = IconStyle::Plain;
    app_state.dir_left = "/".into();
    app_state.mounts = (0..12)
        .map(|index| Mount {
            path: format!("/mnt/{index}").into(),
            label: format!("d{index}"),
            kind: MountKind::Disk,
        })
        .collect();
    let area = Rect::new(0, 0, 30, 1);

    // Plain icons are five columns a slot: 30, less 4 for arrows, holds 5.
    drive_strip(&mut app_state, true, area);
    let strip = &app_state.drive_strip_left;
    assert_eq!((strip.first, strip.visible), (0, 5));
    assert_eq!(strip.hits.first(), Some(&StripHit::Item(0)));
    assert_eq!(strip.hits.last(), Some(&StripHit::Forward));
    assert_eq!(app_state.drive_at(1, 0), None); // no back arrow yet
    assert_eq!(app_state.drive_at(2, 0), Some((true, StripHit::Item(0))));
    assert_eq!(app_state.drive_at(27, 0), Some((true, StripHit::Forward)));

    // A page along, and the back arrow appears with the icons where they were.
    app_state.scroll_drive_strip(true, true);
    drive_strip(&mut app_state, true, area);
    assert_eq!(app_state.drive_at(0, 0), Some((true, StripHit::Back)));
    assert_eq!(app_state.drive_at(2, 0), Some((true, StripHit::Item(5))));

    // At the end, no forward arrow.
    app_state.scroll_drive_strip(true, true);
    drive_strip(&mut app_state, true, area);
    assert_eq!(app_state.drive_strip_left.first, 7);
    assert!(!app_state.drive_strip_left.hits.contains(&StripHit::Forward));

    // Room for all: no arrows and the old layout.
    drive_strip(&mut app_state, true, Rect::new(0, 0, 80, 1));
    assert_eq!(app_state.drive_strip_left.first, 0);
    assert_eq!(app_state.drive_at(1, 0), Some((true, StripHit::Item(0))));
}

/// A file name can hold anything but '/' and NUL - an escape sequence, a
/// tab, a bell - and what a widget is handed goes to the terminal as it is.
/// None of it may reach a cell: not in the row, the detail lines under it,
/// the preview beside it, the status bar, or a popup that names the entry.
#[test]
fn a_hostile_name_never_reaches_the_terminal() {
    let dir = std::env::temp_dir().join(format!("fm84-hostile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let nasty = "a\u{1b}[41;97mHACKED\u{1b}[0m\tbell\u{7}.txt";
    std::fs::write(dir.join(nasty), "contents").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.show_preview = true;
    app_state.open_dir(true, dir.clone(), Some(nasty.as_ref()));
    app_state.refresh_cursor_detail();
    app_state.refresh_preview();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();

    let mut clean = |app_state: &mut AppState, what: &str| {
        render_ui(&mut terminal, app_state);
        let dirty: Vec<String> = terminal.backend().buffer().content().iter().filter(|cell| cell.symbol().chars().any(char::is_control)).map(|cell| format!("{:?}", cell.symbol())).collect();
        assert!(dirty.is_empty(), "{what} drew {dirty:?}");
    };

    clean(&mut app_state, "the panel");

    app_state.dialog = Some(Dialog::Delete {
        items: vec![(dir.join(nasty), false)],
        to_trash: true,
    });
    clean(&mut app_state, "the delete popup");

    let mut input = TextInput::new();
    input.set(nasty.to_string());
    app_state.dialog = Some(Dialog::Rename(input));
    clean(&mut app_state, "the rename field");
    let mut input = TextInput::new();
    input.set(nasty.to_string());
    app_state.dialog = Some(Dialog::Prompt(crate::app::PromptKind::GoTo, input));
    clean(&mut app_state, "the go to field");
    app_state.dialog = None;

    app_state.error = Some(format!("No such file or directory: {nasty}"));
    clean(&mut app_state, "the error popup");

    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every field being typed into is drawn as a name being renamed is, in
/// the same colours, with the terminal's cursor in it - or, for Classic,
/// the cell it is on reversed. The marks that found it never reach the
/// terminal.
#[test]
fn every_input_field_wears_the_rename_colours() {
    use crate::app::PromptKind;
    use crate::options::CursorShape;
    use ratatui::backend::Backend;
    // A file for F2 to be renaming.
    let dir = std::env::temp_dir().join(format!("fm84-input-fields-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("file.txt"), "").unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("file.txt".as_ref()));
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    let typed = || {
        let mut input = TextInput::new();
        input.set("typed".to_string());
        input
    };
    let dialogs: Vec<(&str, Dialog)> = vec![
        ("F2", Dialog::Rename(typed())),
        ("F7", Dialog::Create { is_dir: true, input: typed() }),
        (
            "F5",
            Dialog::Transfer {
                sources: vec![(std::path::PathBuf::from("/a"), false)],
                is_copy: true,
                input: typed(),
                opened_with: String::new(),
                default: std::path::PathBuf::from("/b"),
            },
        ),
        (
            "Ctrl+Shift+F5",
            Dialog::Symlink {
                sources: vec![(std::path::PathBuf::from("/a"), false)],
                input: typed(),
                opened_with: String::new(),
                default: std::path::PathBuf::from("/b"),
            },
        ),
        ("+", Dialog::Prompt(PromptKind::Select, typed())),
        ("Ctrl+L", Dialog::Prompt(PromptKind::GoTo, typed())),
        ("Options", Dialog::Options { editing: Some(typed()) }),
    ];
    let reversed = |terminal: &Terminal<ratatui::backend::TestBackend>| terminal.backend().buffer().content().iter().filter(|cell| cell.modifier.contains(Modifier::REVERSED) && cell.bg == palette().rename_background).count();
    let marked = |terminal: &Terminal<ratatui::backend::TestBackend>| {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.modifier.intersects(FIELD_CURSOR | EDITOR_CURSOR | crate::app::CURSOR_AFTER | crate::app::PAST_END))
    };
    for (what, dialog) in dialogs {
        app_state.dialog = Some(dialog);
        for shape in [CursorShape::Bar, CursorShape::Classic] {
            app_state.options.cursor = shape;
            render_ui(&mut terminal, &mut app_state);
            assert!(!marked(&terminal), "{what}, {shape:?}: a mark reached the terminal");
            if shape == CursorShape::Classic {
                assert_eq!(reversed(&terminal), 1, "{what}: no cursor in the rename colours");
            } else {
                // Straight after what is typed.
                let at = terminal.backend_mut().get_cursor_position().unwrap();
                let last = &terminal.backend().buffer()[(at.x - 1, at.y)];
                assert_eq!((last.symbol(), last.bg), ("d", palette().rename_background), "{what}: the cursor is not after the text");
                assert_eq!(reversed(&terminal), 0, "{what}: a cell reversed as well");
            }
            let text = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .filter(|cell| cell.bg == palette().rename_background && cell.fg == palette().selected_foreground && !cell.modifier.contains(Modifier::REVERSED))
                .count();
            assert!(text >= "typed".len() - 1, "{what}: typed text not in the rename colours");
        }
    }
    app_state.open_find_files();
    render_ui(&mut terminal, &mut app_state);
    assert_eq!(reversed(&terminal), 1, "Alt+F7");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every popup is one width, and no taller than the panels it sits over,
/// leaving the bars above and below be. F11 and F1 scroll within that.
#[test]
fn popups_share_a_width_and_keep_to_the_panels() {
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    // The popup titled `title`: its left edge, width, top and bottom rows.
    let frame = |terminal: &Terminal<ratatui::backend::TestBackend>, title: &str| {
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..buffer.area.height).map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect()).collect();
        let top = rows.iter().position(|row| row.contains(title)).unwrap_or_else(|| panic!("{title} not drawn"));
        // The corners either side of the title: on a popup as tall as
        // the screen, the screen's own are on that row too.
        let chars: Vec<char> = rows[top].chars().collect();
        let at = rows[top].find(title).map(|byte| rows[top][..byte].chars().count()).unwrap();
        let left = chars[..at].iter().rposition(|&c| c == '┌').unwrap();
        let right = at + chars[at..].iter().position(|&c| c == '┐').unwrap();
        let bottom = (top..rows.len()).find(|&y| rows[y].chars().nth(left) == Some('└')).unwrap();
        (left, right - left + 1, top, bottom)
    };
    let mut input = TextInput::new();
    input.set("/b".to_string());
    let dialogs = vec![
        (" Help/About ", Dialog::Help),
        (" Create Directory ", Dialog::Create { is_dir: true, input: TextInput::new() }),
        (
            " Copy file ",
            Dialog::Transfer {
                sources: vec![(PathBuf::from("/a"), false)],
                is_copy: true,
                input,
                opened_with: String::new(),
                default: PathBuf::from("/b"),
            },
        ),
        (" Options ", Dialog::Options { editing: None }),
        (
            " Delete: ",
            Dialog::Delete {
                items: vec![(PathBuf::from("/a"), false)],
                to_trash: false,
            },
        ),
        (" Unsaved Changes ", Dialog::SaveChanges { then_quit: false }),
        (" Bookmarks ", Dialog::Bookmarks { list: vec![PathBuf::from("/a")], index: 0 }),
    ];
    let mut widths = Vec::new();
    for (title, dialog) in dialogs {
        app_state.dialog = Some(dialog);
        render_ui(&mut terminal, &mut app_state);
        let (left, width, top, bottom) = frame(&terminal, title);
        widths.push((title, left, width));
        // Three rows of top bar and the path bar above the panels; the
        // status bar and three of keys below.
        assert!(top >= 4 && bottom <= 30 - 5, "{title} runs from row {top} to {bottom}");
        let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
        if title == " Options " {
            assert!(screen.contains(&format!(" 1/{} ", OPTION_ROWS.len())), "F11 does not say it scrolls");
        }
        if title == " Help/About " {
            assert!(screen.contains(" 1-"), "F1 does not say it scrolls");
        }
    }
    assert!(widths.iter().all(|&(_, left, width)| (left, width) == (widths[0].1, widths[0].2)), "{widths:?}");

    // Scrolled past its end, F1 stops with its last line at the bottom.
    app_state.dialog = Some(Dialog::Help);
    app_state.help_scroll = usize::MAX;
    render_ui(&mut terminal, &mut app_state);
    let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    let help = help_lines();
    assert!(screen.contains(help[help.len() - 1].0.trim()), "F1's last line");
    assert!(screen.contains(" About "), "the About heading");
    assert!(screen.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))), "F1's About");
    assert!(screen.contains(&format!("-{}/{} ", help.len(), help.len())), "F1's corner");
}

/// With the cursor at the end, a directory being renamed reads `[doc]`
/// and a path being typed ends where it ends: the terminal's cursor sits
/// on what follows, with no space put in for it. Classic's cursor is a
/// cell, and has one.
#[test]
fn nothing_is_added_past_the_end_for_the_cursor() {
    use crate::options::CursorShape;
    let dir = std::env::temp_dir().join(format!("fm84-past-end-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("doc")).unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.is_left_active = true;
    app_state.open_dir(true, dir.clone(), Some("doc".as_ref()));
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    let screen = |terminal: &Terminal<ratatui::backend::TestBackend>| terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect::<String>();
    let typed = |text: &str| {
        let mut input = TextInput::new();
        input.set(text.to_string());
        input
    };

    app_state.dialog = Some(Dialog::Rename(typed("doc")));
    render_ui(&mut terminal, &mut app_state);
    assert!(screen(&terminal).contains("[doc]"), "F2 on a directory");
    app_state.options.cursor = CursorShape::Classic;
    render_ui(&mut terminal, &mut app_state);
    assert!(screen(&terminal).contains("[doc ]"), "Classic's cursor has a cell");

    app_state.options.cursor = CursorShape::Bar;
    app_state.dialog = Some(Dialog::Prompt(PromptKind::GoTo, typed("/somewhere")));
    render_ui(&mut terminal, &mut app_state);
    assert!(screen(&terminal).contains("/somewhere ─"), "Ctrl+L");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The editor's cursor is the terminal's, on the character it is before;
/// a prompt over the editor takes it, as that is where the typing goes.
/// Classic paints the cell instead, in the colours it always had.
#[test]
fn the_editor_cursor_gives_way_to_a_prompt() {
    use crate::options::CursorShape;
    use ratatui::backend::Backend;
    let file = std::env::temp_dir().join(format!("fm84-editor-cursor-{}.txt", std::process::id()));
    std::fs::write(&file, "hello\n").unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_editor(file.clone()).unwrap();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    render_ui(&mut terminal, &mut app_state);
    let in_editor = terminal.backend_mut().get_cursor_position().unwrap();
    assert_eq!(terminal.backend().buffer()[in_editor].symbol(), "h");

    app_state.open_prompt(crate::app::PromptKind::Find);
    app_state.dialog_input().unwrap().set("he".to_string());
    render_ui(&mut terminal, &mut app_state);
    let at = terminal.backend_mut().get_cursor_position().unwrap();
    let last = &terminal.backend().buffer()[(at.x - 1, at.y)];
    assert_eq!((last.symbol(), last.bg), ("e", palette().rename_background), "the prompt has the cursor");

    app_state.dialog = None;
    app_state.options.cursor = CursorShape::Classic;
    render_ui(&mut terminal, &mut app_state);
    let cell = &terminal.backend().buffer()[in_editor];
    assert_eq!((cell.symbol(), cell.bg), ("h", palette().selected_background));
    std::fs::remove_file(&file).unwrap();
}

/// Ctrl+L types over the active panel's path, in the line above it, and
/// leaves the other panel's and the foot of the screen as they were. A
/// path longer than the room keeps its end, where the typing is, in view.
#[test]
fn go_to_is_typed_over_the_panels_own_path() {
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.dir_left = std::path::PathBuf::from("/left/dir");
    app_state.dir_right = std::path::PathBuf::from("/right/dir");
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
    let row = |terminal: &Terminal<ratatui::backend::TestBackend>, y: u16| -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect()
    };

    for (is_left, typed) in [(true, "/typed/here"), (false, "/somewhere/else"), (true, &format!("/{}/end", "long".repeat(30))[..])] {
        app_state.is_left_active = is_left;
        let mut input = TextInput::new();
        input.set(typed.to_string());
        app_state.dialog = Some(Dialog::Prompt(PromptKind::GoTo, input));
        render_ui(&mut terminal, &mut app_state);
        let bar = row(&terminal, 3);
        let (left, right) = bar.split_at(bar.find('┬').unwrap());
        let (typed_in, other) = if is_left { (left, right) } else { (right, left) };
        assert!(typed_in.contains(if typed.len() > 40 { "long/end" } else { typed }), "{bar}");
        assert!(other.contains(if is_left { "/right/dir" } else { "/left/dir" }), "{bar}");
        assert!(bar.starts_with("├──") && bar.ends_with("─┤") && bar.chars().count() == 80, "{bar}");
        assert!(!row(&terminal, 19).contains("Go to"));
    }
}

/// The hexdump takes the widest layout its pane holds and never runs past
/// it, at every width from one that fits nothing to one that fits the lot.
#[test]
fn hex_preview_fits_the_pane_it_is_given() {
    let bytes: Vec<u8> = (0..=255u8).collect();
    let mut seen = Vec::new();
    for width in 1..100u16 {
        let rows = hex_preview(&bytes, width, 4);
        let widest = rows.iter().map(|row| row.chars().count()).max().unwrap();
        // One column is the space every preview line opens with. The
        // narrowest layout is wider than the narrowest pane, which is the
        // one case where the text is left to be clipped.
        if width > 1 + crate::viewer::hex_row_width(4, 0) as u16 {
            assert!(widest < width as usize, "{width} columns held a {widest}-column row");
        }
        let bytes_shown: usize = rows.iter().map(|row| row.chars().filter(|c| *c == '|').count()).sum();
        assert_eq!(bytes_shown, 8, "{width}: every row keeps its gutter");
        seen.push((width, widest));
    }
    // Widest first, so a wider pane never shows less.
    assert!(seen.windows(2).all(|pair| pair[0].1 <= pair[1].1), "{seen:?}");
    assert_eq!(hex_preview(&bytes, 100, 4)[0].chars().count(), crate::viewer::hex_row_width(16, 4));
}

#[test]
fn scrolling_sideways_goes_past_what_a_u16_holds() {
    let text = |spans: &[Span<'static>]| spans.iter().map(|span| span.content.as_ref()).collect::<String>();
    let spans = vec![Span::raw("ab"), Span::styled("cdef", style_title())];
    assert_eq!(text(&skip_columns(spans.clone(), 0)), "abcdef");
    assert_eq!(text(&skip_columns(spans.clone(), 3)), "def");
    // The style stays with what is left of its span.
    assert_eq!(skip_columns(spans.clone(), 3)[0].style, style_title());
    assert!(skip_columns(spans, 10).is_empty());

    // Half of a wide character is a space.
    assert_eq!(text(&skip_columns(vec![Span::raw("界x")], 1)), " x");

    // A minified line, scrolled further than 65,535 columns.
    let long = format!("{}END", "x".repeat(70_000));
    assert_eq!(text(&skip_columns(vec![Span::raw(long)], 70_000)), "END");
}

#[test]
fn popup_text_wraps_at_spaces_and_keeps_the_last_line_in_view() {
    assert_eq!(wrap_text("one two three", 7), ["one two", "three"]);
    assert_eq!(wrap_text("short", 20), ["short"]);
    // A word wider than a row is broken inside it.
    assert_eq!(wrap_text("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(wrap_text("", 4), [""]);
    // Wide characters count as the two columns they take.
    assert_eq!(wrap_text("界界界", 4), ["界界", "界"]);

    // Twenty rows of names, a destination and the keys, in seven rows:
    // the names give way, and the keys are still there.
    let names = (0..60).map(|n| format!("file{n:02}")).collect::<Vec<_>>().join(", ");
    let lines = vec![Line::from(names), Line::from("to: /tmp"), Line::from("Y / Enter - Yes")];
    let blocks = fit_rows(lines.into_iter().map(|line| wrap_line(line, 30)).collect(), 7);
    let rows: Vec<String> = blocks.iter().flatten().map(|line| line.to_string()).collect();
    assert_eq!(rows.len(), 7);
    assert!(rows[4].ends_with('…'), "{rows:?}");
    assert_eq!(rows[5..], ["to: /tmp", "Y / Enter - Yes"]);
}

/// On a wrapped line, the matches on each row are marked where they are,
/// however far along the line the row is - and only those.
#[test]
fn matches_on_wrapped_rows_are_marked_where_they_fall() {
    let path = std::env::temp_dir().join(format!("fm84-wrap-marks-{}.txt", std::process::id()));
    let words: Vec<String> = (0..40).map(|n| if n % 7 == 3 { "needle".to_string() } else { format!("w{n:02}") }).collect();
    std::fs::write(&path, words.join(" ")).unwrap();
    let mut app_state = AppState::new();
    app_state.open_viewer(path.clone()).unwrap();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(40, 20)).unwrap();
    render_ui(&mut terminal, &mut app_state);
    app_state.viewer_toggle_wrap();
    app_state.find_term = "needle".to_string();
    app_state.find_shown = true;
    render_ui(&mut terminal, &mut app_state);

    // Every run of marked cells, row by row, reads "needle" - one for each
    // of the six in the line.
    let buffer = terminal.backend().buffer().clone();
    let mut marked = Vec::new();
    for y in 0..20 {
        let mut run = String::new();
        for x in 0..40 {
            let cell = &buffer[(x, y)];
            if cell.modifier.contains(Modifier::UNDERLINED) {
                run.push_str(cell.symbol());
            } else if !run.is_empty() {
                marked.push(std::mem::take(&mut run));
            }
        }
        if !run.is_empty() {
            marked.push(run);
        }
    }
    assert_eq!(marked, vec!["needle"; 6]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn centered_sits_in_the_middle_and_never_overflows() {
    let area = Rect::new(10, 5, 80, 20);
    assert_eq!(centered(area, 40, 20), Rect::new(30, 5, 40, 20));
    assert_eq!(centered(area, 80, 10), Rect::new(10, 10, 80, 10));
    // Larger than the area, as a filled image is: clipped to it, not shifted.
    assert_eq!(centered(area, 200, 90), area);
    // An odd leftover puts the extra column on the right.
    assert_eq!(centered(area, 79, 20).x, 10);
}

/// An editor open on a file of `lines` numbered lines, and a terminal to
/// draw it on. The file is removed again when the test is done with it.
fn editor_on(name: &str, lines: usize) -> (AppState, Terminal<ratatui::backend::TestBackend>, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("fm84-{name}-{}.txt", std::process::id()));
    let text: String = (1..=lines).map(|n| format!("{n}\n")).collect();
    std::fs::write(&path, text).unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_editor(path.clone()).unwrap();
    let terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    (app_state, terminal, path)
}

#[test]
fn deleting_everything_from_the_bottom_of_a_long_file_draws() {
    let (mut app_state, mut terminal, path) = editor_on("select-all", 100);
    render_ui(&mut terminal, &mut app_state);
    // Ctrl+A scrolls to the end; Backspace then leaves a single line. The
    // scroll used to stay where Ctrl+A put it, and the next frame sliced
    // the lines with its start past its end.
    app_state.editor_select_all();
    app_state.editor_backspace();
    render_ui(&mut terminal, &mut app_state);
    let state = app_state.editor().unwrap();
    assert_eq!(state.lines, [""]);
    assert_eq!(state.scroll_offset, 0);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_long_paste_leaves_the_cursor_on_screen() {
    let (mut app_state, mut terminal, path) = editor_on("paste", 3);
    render_ui(&mut terminal, &mut app_state);
    let pasted: String = (0..200).map(|n| format!("pasted {n}\n")).collect();
    app_state.editor_insert_text(&pasted);
    render_ui(&mut terminal, &mut app_state);
    let state = app_state.editor().unwrap();
    let height = app_state.editor_viewport_height;
    assert!(state.cursor_line >= state.scroll_offset && state.cursor_line < state.scroll_offset + height);
    std::fs::remove_file(path).unwrap();
}

/// Scrolled as far as it goes and then given more room, F1 and the diff
/// keep to what the room allows: the first key back up moves them.
#[test]
fn scrolling_keeps_to_the_room_there_is() {
    let mut app_state = AppState::new();
    app_state.options = Options::default();

    // F1, scrolled to the end of a short screen, then the screen grows.
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    app_state.dialog = Some(Dialog::Help);
    app_state.help_scroll = usize::MAX;
    render_ui(&mut terminal, &mut app_state);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 50)).unwrap();
    render_ui(&mut terminal, &mut app_state);
    assert_eq!(app_state.help_scroll, help_lines().len() - app_state.help_room, "F1 past its end");

    // A diff whose last difference is near the end: jumping to it stops
    // with the last row at the bottom, not past it.
    app_state.dialog = None;
    let left: Vec<String> = (0..100).map(|n| n.to_string()).collect();
    let mut right = left.clone();
    right[98] = "changed".to_string();
    app_state.screen = Screen::Diff(crate::diff::DiffState::new(PathBuf::from("/a"), PathBuf::from("/b"), left, right));
    render_ui(&mut terminal, &mut app_state);
    let Screen::Diff(state) = &app_state.screen else { unreachable!() };
    assert_eq!(state.scroll_offset, state.rows.len() - app_state.diff_viewport_height, "the diff past its end");
}
