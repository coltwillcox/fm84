use super::*;

/// The branch follows the path, and is the first to go when the path
/// needs the room; who and where sit by the logo while they fit.
#[test]
fn the_branch_after_the_path_and_who_by_the_logo() {
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.left.dir = PathBuf::from("/home/colt/fm84");
    app_state.left.branch = Some("main".to_string());
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
    app_state.active_side = Side::Left;
    app_state.left.dir = "/tmp/b".into();
    app_state.left.tabs = crate::app::Tabs::new(vec!["/a".into(), "/tmp/b".into()], 1);
    app_state.left.tabs.strip = tab_strip(&app_state, Side::Left, Rect::new(0, 1, 40, 1)).1;

    // " " then " a " then " b ".
    assert_eq!(app_state.tab_at(0, 1), None);
    assert_eq!(app_state.tab_at(1, 1), Some((Side::Left, StripHit::Item(0))));
    assert_eq!(app_state.tab_at(3, 1), Some((Side::Left, StripHit::Item(0))));
    assert_eq!(app_state.tab_at(4, 1), Some((Side::Left, StripHit::Item(1))));
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
    app_state.left.dir = "/".into();
    app_state.mounts = (0..12)
        .map(|index| Mount {
            path: format!("/mnt/{index}").into(),
            label: format!("d{index}"),
            kind: MountKind::Disk,
        })
        .collect();
    let area = Rect::new(0, 0, 30, 1);
    let lay = |app_state: &mut AppState, area: Rect| app_state.drive_strips[Side::Left] = drive_strip(app_state, Side::Left, area).1;

    // Plain icons are five columns a slot: 30, less 4 for arrows, holds 5.
    lay(&mut app_state, area);
    let strip = &app_state.drive_strips[Side::Left];
    assert_eq!((strip.first, strip.visible), (0, 5));
    assert_eq!(strip.hits.first(), Some(&StripHit::Item(0)));
    assert_eq!(strip.hits.last(), Some(&StripHit::Forward));
    assert_eq!(app_state.drive_at(1, 0), None); // no back arrow yet
    assert_eq!(app_state.drive_at(2, 0), Some((Side::Left, StripHit::Item(0))));
    assert_eq!(app_state.drive_at(27, 0), Some((Side::Left, StripHit::Forward)));

    // A page along, and the back arrow appears with the icons where they were.
    app_state.scroll_drive_strip(Side::Left, true);
    lay(&mut app_state, area);
    assert_eq!(app_state.drive_at(0, 0), Some((Side::Left, StripHit::Back)));
    assert_eq!(app_state.drive_at(2, 0), Some((Side::Left, StripHit::Item(5))));

    // At the end, no forward arrow.
    app_state.scroll_drive_strip(Side::Left, true);
    lay(&mut app_state, area);
    assert_eq!(app_state.drive_strips[Side::Left].first, 7);
    assert!(!app_state.drive_strips[Side::Left].hits.contains(&StripHit::Forward));

    // Room for all: no arrows and the old layout.
    lay(&mut app_state, Rect::new(0, 0, 80, 1));
    assert_eq!(app_state.drive_strips[Side::Left].first, 0);
    assert_eq!(app_state.drive_at(1, 0), Some((Side::Left, StripHit::Item(0))));
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
    app_state.open_dir(Side::Left, dir.clone(), Some(nasty.as_ref()));
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
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), Some("file.txt".as_ref()));
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
                join: false,
            },
        ),
        (
            "Ctrl+Shift+F5",
            Dialog::Symlink {
                sources: vec![(std::path::PathBuf::from("/a"), false)],
                input: typed(),
                opened_with: String::new(),
                default: std::path::PathBuf::from("/b"),
                hard: false,
            },
        ),
        (
            "Alt+P",
            Dialog::Pack(Box::new(crate::app::PackForm {
                sources: vec![(std::path::PathBuf::from("/a"), false)],
                input: typed(),
                base: "a".to_string(),
                focus: 0,
                level: crate::fs_ops::PackLevel::Normal,
                separate: false,
                move_after: false,
                leave_out: TextInput::new(),
                password: TextInput::new(),
                again: TextInput::new(),
            })),
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
    app_state.open_history();
    app_state.dialog_input().unwrap().set("typed".to_string());
    render_ui(&mut terminal, &mut app_state);
    assert_eq!(reversed(&terminal), 1, "Alt+Down");
    assert!(!marked(&terminal), "Alt+Down: a mark reached the terminal");
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
    app_state.open_history();
    let history = app_state.dialog.take().unwrap();
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
                join: false,
            },
        ),
        (
            " Pack file ",
            Dialog::Pack(Box::new(crate::app::PackForm {
                sources: vec![(PathBuf::from("/a"), false)],
                input: TextInput::new(),
                base: "a".to_string(),
                focus: 1,
                level: crate::fs_ops::PackLevel::Best,
                separate: true,
                move_after: false,
                leave_out: TextInput::new(),
                password: TextInput::new(),
                again: TextInput::new(),
            })),
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
        (" Mixtapes ", Dialog::Mixtapes(Box::new(crate::app::MixtapeList { index: 0, naming: None }))),
        (" Directory History ", history),
        (
            " User Menu ",
            Dialog::UserMenu(Box::new(crate::app::UserMenu {
                entries: vec![crate::options::MenuEntry {
                    label: "Disk use".to_string(),
                    command: "du -sh {files}".to_string(),
                }],
                index: 0,
                file: None,
            })),
        ),
        (
            " Rename 2 Entries ",
            Dialog::MultiRename(Box::new(crate::app::MultiRename::new(
                ["a.txt", "b.txt"]
                    .iter()
                    .map(|name| crate::app::RenameEntry {
                        path: PathBuf::from("/").join(name),
                        old: name.to_string(),
                        readable: true,
                        meta: Default::default(),
                    })
                    .collect(),
            ))),
        ),
        (
            " Open a With ",
            Dialog::OpenWith(Box::new(crate::app::OpenWith {
                path: PathBuf::from("/a"),
                openers: vec![crate::opener::Opener {
                    label: "System default".to_string(),
                    detail: String::new(),
                    launch: crate::opener::Launch::Default,
                    terminal: false,
                }],
                index: 0,
            })),
        ),
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
    let help = help_lines(&Default::default());
    // The last line that stays put: INSERT COIN, last of all, blinks.
    let last = help.iter().rev().find(|(text, kind)| *kind != HelpLine::Coin && !text.trim().is_empty()).unwrap();
    assert!(screen.contains(last.0.trim()), "F1's last line");
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
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), Some("doc".as_ref()));
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
    app_state.left.dir = std::path::PathBuf::from("/left/dir");
    app_state.right.dir = std::path::PathBuf::from("/right/dir");
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
    let row = |terminal: &Terminal<ratatui::backend::TestBackend>, y: u16| -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect()
    };

    for (side, typed) in [(Side::Left, "/typed/here"), (Side::Right, "/somewhere/else"), (Side::Left, &format!("/{}/end", "long".repeat(30))[..])] {
        app_state.active_side = side;
        let mut input = TextInput::new();
        input.set(typed.to_string());
        app_state.dialog = Some(Dialog::Prompt(PromptKind::GoTo, input));
        render_ui(&mut terminal, &mut app_state);
        let bar = row(&terminal, 3);
        let (left, right) = bar.split_at(bar.find('┬').unwrap());
        let (typed_in, other) = if side.is_left() { (left, right) } else { (right, left) };
        assert!(typed_in.contains(if typed.len() > 40 { "long/end" } else { typed }), "{bar}");
        assert!(other.contains(if side.is_left() { "/right/dir" } else { "/left/dir" }), "{bar}");
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
    app_state.recall.find_term = "needle".to_string();
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
    let height = app_state.drawn.editor_viewport_height;
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
    assert_eq!(app_state.help_scroll, help_lines(&Default::default()).len() - app_state.drawn.help_room, "F1 past its end");

    // A diff whose last difference is near the end: jumping to it stops
    // with the last row at the bottom, not past it.
    app_state.dialog = None;
    let left: Vec<String> = (0..100).map(|n| n.to_string()).collect();
    let mut right = left.clone();
    right[98] = "changed".to_string();
    app_state.screen = Screen::Diff(crate::diff::DiffState::new(PathBuf::from("/a"), PathBuf::from("/b"), left, right));
    render_ui(&mut terminal, &mut app_state);
    let Screen::Diff(state) = &app_state.screen else { unreachable!() };
    assert_eq!(state.scroll_offset, state.rows.len() - app_state.drawn.diff_viewport_height, "the diff past its end");
}

/// A diff of two long paths in a narrow window shows the end of each - the
/// file's own name - with "..." where the rest was cut. The "..." was laid on
/// top of a path already as wide as its column, and pushed the last three
/// characters of the name off the edge.
#[test]
fn the_diff_header_keeps_the_end_of_a_long_path() {
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    let deep = "/home/user/projects/somewhere/rather/deep/inside";
    let (left, right) = (PathBuf::from(format!("{deep}/left_name.txt")), PathBuf::from(format!("{deep}/right_name.txt")));
    app_state.screen = Screen::Diff(crate::diff::DiffState::new(left, right, vec!["a".to_string()], vec!["b".to_string()]));
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(60, 20)).unwrap();
    render_ui(&mut terminal, &mut app_state);
    let buffer = terminal.backend().buffer();
    let rows: Vec<String> = (0..buffer.area.height).map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect()).collect();
    let header = rows.iter().find(|row| row.contains("...") && row.contains("_name")).expect("the paths are cut short");
    assert!(header.contains("left_name.txt") && header.contains("right_name.txt"), "{header}");
}

/// The cells a scroll bar's thumb is drawn in, by column and row, and which
/// of the two thumbs each is. Every one is in the title's colour.
fn thumb_cells(terminal: &Terminal<ratatui::backend::TestBackend>) -> Vec<(u16, u16, &'static str)> {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    let cells: Vec<(u16, u16, &'static str)> = (area.y..area.bottom())
        .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
        .filter_map(|(x, y)| {
            let (down, along) = ScrollBarStyle::Heavy.thumbs();
            [down, along].into_iter().find(|&thumb| buffer[(x, y)].symbol() == thumb).map(|thumb| (x, y, thumb))
        })
        .collect();
    for &(x, y, _) in &cells {
        assert_eq!(buffer[(x, y)].fg, crate::display::palette().title, "the thumb at {x},{y}");
    }
    cells
}

/// F3 and F4 have scroll bars in their borders, down the right for the lines
/// and along the bottom for the columns, only when there is more than fits -
/// and none across a wrapped view, where nothing runs past the edge.
#[test]
fn the_viewer_and_the_editor_show_scroll_bars_when_there_is_more() {
    let dir = std::env::temp_dir().join(format!("fm84-scroll-bars-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let short = dir.join("short.txt");
    std::fs::write(&short, "one\ntwo\n").unwrap();
    let tall = dir.join("tall.txt");
    std::fs::write(&tall, (1..=200).map(|n| format!("{n}\n")).collect::<String>()).unwrap();
    let wide = dir.join("wide.txt");
    std::fs::write(&wide, "x".repeat(500)).unwrap();

    let (width, height) = (80, 24);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    let mut shown = |app_state: &mut AppState| {
        render_ui(&mut terminal, app_state);
        thumb_cells(&terminal)
    };

    for edit in [false, true] {
        let open = |app_state: &mut AppState, path: &PathBuf| {
            if edit {
                app_state.open_editor(path.clone()).unwrap();
            } else {
                app_state.open_viewer(path.clone()).unwrap();
            }
        };
        open(&mut app_state, &short);
        assert_eq!(shown(&mut app_state), [], "everything fits (edit: {edit})");

        // Tall: a thumb down one column, at the top of its track.
        open(&mut app_state, &tall);
        let cells = shown(&mut app_state);
        assert!(!cells.is_empty(), "edit: {edit}");
        let column = cells[0].0;
        assert!(cells.iter().all(|&(x, _, thumb)| x == column && thumb == ScrollBarStyle::Heavy.thumbs().0), "one column: {cells:?}");
        let top = cells.iter().map(|&(_, y, _)| y).min().unwrap();
        // Scrolled to the end, the thumb is at the bottom of its track.
        if edit {
            app_state.editor_mut().unwrap().cursor_line = 199;
        } else {
            app_state.viewer_end();
        }
        let bottom = shown(&mut app_state).iter().map(|&(_, y, _)| y).max().unwrap();
        assert!(bottom > top + 10, "moved down: {top} to {bottom}");

        // Wide: a thumb along one row.
        open(&mut app_state, &wide);
        let cells = shown(&mut app_state);
        assert!(!cells.is_empty(), "edit: {edit}");
        let row = cells[0].1;
        assert!(cells.iter().all(|&(_, y, thumb)| y == row && thumb == ScrollBarStyle::Heavy.thumbs().1), "one row: {cells:?}");
    }

    // Wrapped, the long line fits across, and the bar goes.
    app_state.open_viewer(wide).unwrap();
    app_state.viewer_mut().unwrap().wrap = true;
    assert_eq!(shown(&mut app_state), []);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// F11's scroll bar style is what both thumbs are drawn with, stepped through
/// in turn, Heavy first.
#[test]
fn the_scroll_bar_style_from_f11_draws_both_thumbs() {
    let path = std::env::temp_dir().join(format!("fm84-bar-style-{}.txt", std::process::id()));
    std::fs::write(&path, (1..=200).map(|_| format!("{}\n", "x".repeat(300))).collect::<String>()).unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    assert_eq!(app_state.options.scroll_bar, ScrollBarStyle::Heavy);
    app_state.open_viewer(path.clone()).unwrap();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();

    let mut seen = Vec::new();
    for _ in ScrollBarStyle::ALL {
        let style = app_state.options.scroll_bar;
        seen.push(style);
        render_ui(&mut terminal, &mut app_state);
        let buffer = terminal.backend().buffer();
        let drawn = |bar: &crate::scroll_bar::ScrollBar| -> Vec<String> {
            let track = bar.track;
            (track.y..track.bottom()).flat_map(|y| (track.x..track.right()).map(move |x| (x, y))).map(|(x, y)| buffer[(x, y)].symbol().to_string()).collect()
        };
        let (down, along) = style.thumbs();
        for bar in &app_state.drawn.scroll_bars {
            let thumb = if bar.vertical { down } else { along };
            assert!(drawn(bar).iter().any(|symbol| symbol == thumb), "{style:?}: {:?}", drawn(bar));
        }
        assert_eq!(app_state.drawn.scroll_bars.len(), 2, "{style:?}");
        app_state.options.cycle(crate::options::OptionRow::ScrollBars, true);
    }
    assert_eq!(seen, ScrollBarStyle::ALL);
    assert_eq!(app_state.options.scroll_bar, ScrollBarStyle::Heavy, "round to the first again");
    assert_eq!(app_state.options.value(crate::options::OptionRow::ScrollBars), "Heavy ┃━");
    std::fs::remove_file(path).unwrap();
}

/// In a repository, what git says of each entry is marked at the far end of
/// its name, once git has answered; a name too long for the room is cut to
/// leave it showing.
#[test]
fn git_marks_sit_at_the_end_of_the_name() {
    let dir = std::env::temp_dir().join(format!("fm84-git-marks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |arguments: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["-c", "user.name=fm84", "-c", "user.email=fm84@localhost", "-c", "commit.gpgsign=false"])
            .args(arguments)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    if !run(&["init", "--quiet"]) {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let long = format!("{}.txt", "a".repeat(80));
    std::fs::write(dir.join("kept.txt"), "one").unwrap();
    std::fs::write(dir.join(&long), "one").unwrap();
    assert!(run(&["add", "."]) && run(&["commit", "--quiet", "-m", "first"]));
    std::fs::write(dir.join("kept.txt"), "two").unwrap();
    std::fs::write(dir.join(&long), "two").unwrap();
    std::fs::write(dir.join("new.txt"), "").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.options.column_ext = false;
    app_state.open_dir(Side::Left, dir.clone(), None);
    let started = std::time::Instant::now();
    while app_state.git_status(Side::Left).is_none() {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "git never answered");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_git_marks();
    }
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    render_ui(&mut terminal, &mut app_state);
    let buffer = terminal.backend().buffer();
    let rows: Vec<String> = (0..buffer.area.height).map(|y| (0..60).map(|x| buffer[(x, y)].symbol()).collect()).collect();
    // The Name column ends where its separator starts.
    let ends_with = |name: &str, mark: &str| {
        rows.iter().any(|row| {
            let before = row.split('│').nth(1).unwrap_or_default();
            before.contains(name) && before.trim_end().ends_with(&format!(" {mark}"))
        })
    };
    assert!(ends_with("kept.txt", "M"), "{rows:#?}");
    assert!(ends_with("new.txt", "?"), "{rows:#?}");
    assert!(ends_with("aaaa", "M"), "the long name cut for its mark: {rows:#?}");

    // Off in F11: gone with the reread that follows, and git not asked.
    app_state.options.git_marks = false;
    app_state.reload_panel(Side::Left, None);
    assert!(app_state.git_status(Side::Left).is_none() && app_state.left.git.dir == dir);
    render_ui(&mut terminal, &mut app_state);
    let buffer = terminal.backend().buffer();
    let marked = (0..buffer.area.height).any(|y| {
        let row: String = (0..60).map(|x| buffer[(x, y)].symbol()).collect();
        row.split('│').nth(1).unwrap_or_default().trim_end().ends_with(" M")
    });
    assert!(!marked);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// In Serbian, longer than English: the headings take the room they need,
/// arrow and all, and the F-key labels, the options and the Find files
/// form are drawn whole.
#[test]
fn another_language_fits_where_english_did() {
    crate::i18n::set_language(crate::i18n::Language::Serbian);
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.options.sort_key = SortKey::Size;
    let screen = |app_state: &mut AppState, width: u16, height: u16| {
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        render_ui(&mut terminal, app_state);
        let buffer = terminal.backend().buffer().clone();
        (0..height).map(|y| (0..width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>()
    };

    let rows = screen(&mut app_state, 160, 30);
    let headings = rows.iter().find(|row| row.contains("Ime")).expect("no headings");
    for heading in ["Ekst", "Veličina ↑", "Izmenjeno", "Atributi"] {
        assert!(headings.contains(heading), "{heading} cut in: {headings}");
    }
    let keys = rows.iter().rev().find(|row| row.contains("F1 ")).expect("no F-key bar");
    for label in ["F1 Pomoć", "F5 Kopiraj", "F8 Korpa", "F10 Izlaz", "F12 Prikaz"] {
        assert!(keys.contains(label), "{label} not whole in: {keys}");
    }

    app_state.dialog = Some(Dialog::Options { editing: None });
    let rows = screen(&mut app_state, 100, 60).join("\n");
    assert!(rows.contains("Prikaži skrivene datoteke") && rows.contains("Jezik") && rows.contains("Izgled"), "{rows}");

    app_state.dialog = Some(Dialog::FindFiles(Box::new(crate::app::FindFiles::Asking(crate::app::FindForm::default()))));
    let rows = screen(&mut app_state, 100, 30);
    // The labels in one column, as wide as the widest of them.
    let form = rows.iter().position(|row| row.contains("Sadrži")).expect("no form") - 1;
    let at = |label: &str| rows[form..].iter().find_map(|row| row.find(label).map(|byte| row[..byte].chars().count())).unwrap_or_else(|| panic!("{label} not drawn"));
    let column = at("Ime ");
    for label in ["Sadrži", "Veličina", "Izmenjeno", "Regex"] {
        assert_eq!(at(label), column, "{label} out of line");
    }
    crate::i18n::set_language(crate::i18n::Language::English);
}

/// Alt+Shift+Enter puts a bar behind each row, as long against the largest
/// entry as the entry is, and none behind the cursor's. Somewhere else, the
/// bars are gone.
#[test]
fn the_space_map_draws_a_bar_behind_each_row() {
    let dir = std::env::temp_dir().join(format!("fm84-space-map-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("big"), vec![0u8; 4000]).unwrap();
    std::fs::write(dir.join("small"), vec![0u8; 1000]).unwrap();
    std::fs::write(dir.join("under"), vec![0u8; 3000]).unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), Some("under".as_ref()));
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    // The cells of the left panel's row naming `name` that have a bar behind.
    let bar = |terminal: &Terminal<ratatui::backend::TestBackend>, name: &str| {
        let buffer = terminal.backend().buffer();
        let y = (0..30).find(|&y| (0..50).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().contains(name)).expect(name);
        (0..50).filter(|&x| buffer[(x, y)].bg == crate::display::space_bar()).count()
    };

    render_ui(&mut terminal, &mut app_state);
    assert_eq!(bar(&terminal, "big"), 0, "no map until asked for");

    app_state.size_all_dirs();
    render_ui(&mut terminal, &mut app_state);
    let (big, small) = (bar(&terminal, "big"), bar(&terminal, "small"));
    assert!(big > 40, "the largest runs the width: {big}");
    assert_eq!(small, (big + 2) / 4, "a quarter of the largest");
    assert_eq!(bar(&terminal, "under"), 0, "the cursor's row is the cursor's");

    // Elsewhere, and the map is not drawn there.
    app_state.panel_mut(Side::Left).dir = std::env::temp_dir();
    render_ui(&mut terminal, &mut app_state);
    assert_eq!(bar(&terminal, "big"), 0, "gone with the directory");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Alt+U: the entries as blocks by the room they take, counted as they are
/// drawn; the arrows go between them, Enter goes into a directory and maps
/// that, Backspace comes back out, and Esc leaves the panel's cursor on the
/// block chosen.
#[test]
fn the_space_map_screen_draws_blocks_by_size_and_goes_in_and_out() {
    let dir = std::env::temp_dir().join(format!("fm84-treemap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("big")).unwrap();
    std::fs::write(dir.join("big").join("inside.bin"), vec![0u8; 60_000]).unwrap();
    std::fs::write(dir.join("medium.txt"), vec![b'x'; 30_000]).unwrap();
    std::fs::write(dir.join("small.txt"), vec![b'x'; 10_000]).unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    let settle = |app_state: &mut AppState| {
        let started = std::time::Instant::now();
        loop {
            app_state.poll_listings();
            app_state.keep_treemap_sized();
            app_state.poll_dir_sizes();
            let counting = !app_state.workers.dir_sizing.is_empty() || !app_state.workers.dir_size_queue.is_empty();
            if !app_state.panel_busy(Side::Left) && !counting && app_state.panel(Side::Left).space_map.as_ref() == Some(&app_state.panel(Side::Left).dir) {
                break;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "never settled");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    app_state.open_dir(Side::Left, dir.clone(), None);
    while app_state.panel_busy(Side::Left) {
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_listings();
    }
    app_state.open_treemap();
    settle(&mut app_state);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    let mut draw = |app_state: &mut AppState| {
        render_ui(&mut terminal, app_state);
        let buffer = terminal.backend().buffer();
        (0..30).map(|y| (0..100).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
    };
    let screen = draw(&mut app_state);
    assert!(screen.contains("Space map"), "{screen}");
    let name = |app_state: &AppState, tile: crate::treemap::Tile| match tile {
        crate::treemap::Tile::Entry(index) => app_state.left.children[index].name_full.clone(),
        crate::treemap::Tile::Rest(..) => String::new(),
    };
    let names: Vec<String> = app_state.drawn.treemap.iter().map(|&(tile, _)| name(&app_state, tile)).collect();
    assert_eq!(names, ["big", "medium.txt", "small.txt"], "largest first, and nothing for ..");
    let cells = |rect: ratatui::layout::Rect| u32::from(rect.width) * u32::from(rect.height);
    assert!(cells(app_state.drawn.treemap[0].1) > cells(app_state.drawn.treemap[1].1) * 3 / 2);
    assert!(screen.contains("big/") && screen.contains("medium.txt"), "{screen}");

    // The first arrow lands on the largest; Enter goes into it.
    app_state.treemap_move(crate::treemap::Direction::Right);
    let chosen = |app_state: &AppState| app_state.left.state.selected().map(|index| app_state.left.children[index].name_full.clone());
    assert_eq!(chosen(&app_state).as_deref(), Some("big"));
    app_state.treemap_enter();
    settle(&mut app_state);
    assert_eq!(app_state.left.dir, dir.join("big"));
    draw(&mut app_state);
    assert_eq!(app_state.drawn.treemap.len(), 1);
    assert!(matches!(app_state.screen, crate::app::Screen::Treemap), "still the map");

    // Out again, onto the one come out of; a file chosen, Esc leaves the
    // panel on it.
    app_state.treemap_up();
    settle(&mut app_state);
    draw(&mut app_state);
    assert_eq!(chosen(&app_state).as_deref(), Some("big"));
    let medium = app_state.drawn.treemap.iter().position(|&(tile, _)| name(&app_state, tile) == "medium.txt");
    app_state.treemap_choose(medium);
    app_state.treemap_enter();
    assert!(matches!(app_state.screen, crate::app::Screen::Panels), "a file is no directory to go into");
    assert_eq!(chosen(&app_state).as_deref(), Some("medium.txt"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// F1 draws the README's logo between the keys and the high scores, centred, its
/// blocks in the directories' colour and the lines shading them in the
/// border's - whichever theme is on.
#[test]
fn f1_draws_the_logo_in_the_theme_colours() {
    let lines = help_lines(&Default::default());
    let scores = lines.iter().position(|(text, _)| text.trim() == "High scores").unwrap();
    let about = lines.iter().position(|(text, _)| text.trim() == "About").unwrap();
    let logo: Vec<usize> = lines.iter().enumerate().filter(|(_, (_, kind))| *kind == HelpLine::Logo).map(|(at, _)| at).collect();
    assert_eq!(logo.len(), 6);
    assert_eq!(logo[5] + 2, scores, "a blank row, then the high scores");
    assert!(scores < about, "About after the high scores");
    assert!(lines.last().unwrap().0.contains(env!("CARGO_PKG_REPOSITORY")), "About last");
    assert_eq!(lines[logo[0] - 1].0, "", "a blank row after the keys");
    assert!(lines[logo[0] - 2].0.contains("Type to search"), "after the last key");
    // Every heading has a blank row under it, before what it heads.
    for (at, (_, kind)) in lines.iter().enumerate() {
        if *kind == HelpLine::Heading {
            assert_eq!(lines[at + 1].0, "", "a blank row under {:?}", lines[at].0);
        }
    }

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.dialog = Some(Dialog::Help);
    app_state.help_scroll = logo[0];
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    render_ui(&mut terminal, &mut app_state);
    let buffer = terminal.backend().buffer();
    let at = |row: &str| {
        (0..40).find_map(|y| {
            let text: String = (0..120).map(|x| buffer[(x, y)].symbol().to_string()).collect();
            text.find(row).map(|byte| (text[..byte].chars().count() as u16, y))
        })
    };
    let (x, y) = at("███████╗███╗   ███╗ █████╗ ██╗  ██╗").expect("the logo's first row");
    assert_eq!(buffer[(x, y)].fg, crate::display::palette().directory, "a block");
    assert_eq!(buffer[(x + 7, y)].fg, crate::display::palette().border, "its shade");
    assert!(at("╚═╝     ╚═╝     ╚═╝ ╚════╝      ╚═╝").is_some_and(|(_, last)| last == y + 5), "six rows");
}

/// F1's keys stand apart from what they do: before the dash on a main
/// line, and chords, F-keys, letters and key names among the words of the
/// rest.
#[test]
fn f1_tells_keys_from_what_they_do() {
    let keys = |text: &str| key_runs(text).into_iter().filter(|(_, key)| *key).map(|(run, _)| run).collect::<Vec<_>>();
    assert_eq!(keys("   F4 - Edit file (Ctrl+S/F2 save)"), ["F4", "Ctrl+S/F2"]);
    assert_eq!(keys("   Ctrl+Left/Right - Dir to that panel"), ["Ctrl+Left/Right"]);
    assert_eq!(keys("     F fit/fill, +/- zoom, W wrap"), ["F", "+/-", "W"]);
    assert_eq!(keys("     with a selection: all by [N] [E] [C]"), ["N", "E", "C"]);
    assert_eq!(keys("   Type to search, Esc to clear"), ["Esc"]);
    assert_eq!(keys("     Shift+F3 (or =) compare two files"), ["Shift+F3", "="]);
    // Hungarian starts what a key does with an article.
    assert_eq!(keys("   Alt+S - A két mappa szinkronizálása"), ["Alt+S"]);
    assert_eq!(keys("   F3 - View file (X text/hex/image)"), ["F3", "X"]);
    // Nothing is lost or doubled on the way.
    let text = "     Alt+F5 (or Ctrl+Shift+F5) link, Tab hard";
    assert_eq!(key_runs(text).into_iter().map(|(run, _)| run).collect::<String>(), text);

    // Every main line under Keys has a key in it, in every language; a line
    // under one may only go on with what it does.
    for language in crate::i18n::Language::ALL {
        crate::i18n::set_language(language);
        for (text, kind) in help_lines(&Default::default()) {
            if kind == HelpLine::Key && text.len() - text.trim_start().len() == 3 {
                assert!(!keys(&text).is_empty(), "{language:?}: no key in {text:?}");
            }
        }
    }
    crate::i18n::set_language(crate::i18n::Language::English);
}

/// A popup's row of keys draws each key apart from what it does: before
/// the dash, or - with no dash - as far as its words look like keys.
#[test]
fn popup_hints_tell_keys_from_what_they_do() {
    let keys = |text: &str| key_hints(text).spans.into_iter().filter(|span| span.style == style_key()).map(|span| span.content.into_owned()).collect::<Vec<_>>();
    assert_eq!(keys("Y / Enter - Yes    N / Esc - No"), ["Y / Enter", "N / Esc"]);
    assert_eq!(keys("Type to narrow    Up/Down - Choose    Esc - Close"), ["Up/Down", "Esc"]);
    assert_eq!(keys("→ ← Copy  Del Delete  Space Leave  > < Mirror"), ["→ ←", "Del", "Space", "> <"]);
    // A description of more than one word, and figures that are no key.
    assert_eq!(keys("→ ← Kopiér  Del Slet  Space Lad være"), ["→ ←", "Del", "Space"]);
    assert_eq!(keys("3 of 10 entries    Esc - Cancel"), ["Esc"]);
    assert_eq!(keys("Comparing...    Esc - Stop and close"), ["Esc"]);
    // Nothing lost or doubled on the way.
    let text = "Tab - Field   ←/→ - Case   Enter - Rename   Esc - Cancel";
    assert_eq!(key_hints(text).spans.into_iter().map(|span| span.content.into_owned()).collect::<String>(), text);
}

/// The F-key bar and the bar under the editor draw a key apart from what
/// it does, and leave the name of the file and the position as they were.
#[test]
fn the_bottom_bars_tell_keys_from_what_they_do() {
    let dir = std::env::temp_dir().join(format!("fm84-bar-keys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "hello").unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    let (width, height) = (160, 30);
    let colour_of = |app_state: &mut AppState, text: &str| {
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        render_ui(&mut terminal, app_state);
        let buffer = terminal.backend().buffer().clone();
        // The lowest row it is on, by its first character's cell.
        (0..height)
            .rev()
            .find_map(|y| {
                let row: Vec<&str> = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
                let at = row.join("").find(text)?;
                let x = row.join("")[..at].chars().count() as u16;
                Some(buffer[(x, y)].fg)
            })
            .unwrap_or_else(|| panic!("{text} not drawn"))
    };
    let (key, said) = (style_key().fg, style_title().fg);
    assert_eq!(Some(colour_of(&mut app_state, "F5 ")), key);
    assert_eq!(Some(colour_of(&mut app_state, "Copy")), said);

    app_state.open_editor(dir.join("notes.txt")).unwrap();
    assert_eq!(Some(colour_of(&mut app_state, "Ctrl+F")), key);
    assert_eq!(Some(colour_of(&mut app_state, "Find")), said);
    assert_eq!(Some(colour_of(&mut app_state, "notes.txt")), said);
    assert_eq!(Some(colour_of(&mut app_state, "Ln 1")), said);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A row deleted for good bursts in place of what it showed, and the rows
/// beside it are drawn as ever.
#[test]
fn a_row_deleted_for_good_bursts() {
    let dir = std::env::temp_dir().join(format!("fm84-burst-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("doomed.txt"), "x").unwrap();
    std::fs::write(dir.join("spared.txt"), "x").unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.explode_then_delete(vec![(dir.join("doomed.txt"), false)]);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    render_ui(&mut terminal, &mut app_state);
    let buffer = terminal.backend().buffer();
    let screen: String = (0..30).map(|y| (0..120).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>() + "\n").collect();
    assert!(!screen.contains("doomed"), "{screen}");
    assert!(screen.contains("spared"), "{screen}");
    assert!(screen.contains('█'), "{screen}");
    app_state.explosion = None;
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The `colors` file's colours in the panel: the first line for a file wins,
/// a light theme takes the second of a pair, a style alone keeps the hashed
/// colour, and a directory or a file with no line is drawn as ever.
#[test]
fn files_are_drawn_in_the_colours_file_gives_them() {
    use ratatui::style::Color;
    let dir = std::env::temp_dir().join(format!("fm84-file-colours-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub.rs")).unwrap();
    for name in ["main.rs", "pack.tar.gz", "notes.md", "plain.txt", "Makefile"] {
        std::fs::write(dir.join(name), "").unwrap();
    }
    crate::display::set_file_colors(crate::options::parse_colors("tar.gz = #ff0000\ngz rs = #00ff00 / #004400\nmakefile = #0000ff\nmd = bold\nrs = #ffffff\n"));

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(Side::Left, dir.clone(), None);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    let mut drawn = |app_state: &mut AppState, name: &str| {
        crate::display::apply(&app_state.options);
        render_ui(&mut terminal, app_state);
        let buffer = terminal.backend().buffer();
        let (x, y) = (0..30)
            .find_map(|y| {
                let text: String = (0..60).map(|x| buffer[(x, y)].symbol().to_string()).collect();
                text.find(name).map(|byte| (text[..byte].chars().count() as u16, y))
            })
            .unwrap_or_else(|| panic!("{name} drawn"));
        (buffer[(x, y)].fg, buffer[(x, y)].modifier)
    };
    assert_eq!(drawn(&mut app_state, "pack").0, Color::Rgb(255, 0, 0), "the first line for it");
    assert_eq!(drawn(&mut app_state, "main").0, Color::Rgb(0, 255, 0), "not the later one");
    assert_eq!(drawn(&mut app_state, "Makefile").0, Color::Rgb(0, 0, 255), "a whole name");
    let (color, modifier) = drawn(&mut app_state, "notes");
    assert_eq!(color, crate::utils::extension_color("md", false), "a style alone keeps the colour");
    assert!(modifier.contains(Modifier::BOLD));
    assert_eq!(drawn(&mut app_state, "plain").0, crate::utils::extension_color("txt", false), "no line for it");
    assert_eq!(drawn(&mut app_state, "sub").0, palette().directory, "a directory is a directory");

    // On a light background - asked of the lookup itself, as a light theme
    // set for drawing would show in the tests drawing beside this one.
    let light = |name: &str, extension: &str| crate::display::file_style_on(name, extension, true).0;
    assert_eq!(light("main.rs", "rs"), Color::Rgb(0, 0x44, 0), "light takes the second");
    assert_eq!(light("pack.tar.gz", "gz"), Color::Rgb(255, 0, 0), "or the one there is");
    assert_eq!(light("plain.txt", "txt"), crate::utils::extension_color("txt", true), "and the hashed one its own");

    // Ctrl+B names each file by its path from the panel: a whole name is
    // still the file's own.
    std::fs::create_dir_all(dir.join("deep")).unwrap();
    std::fs::write(dir.join("deep").join("GNUmakefile"), "").unwrap();
    crate::display::set_file_colors(crate::options::parse_colors("gnumakefile = #0000ff\n"));
    app_state.toggle_flat();
    let started = std::time::Instant::now();
    while app_state.panel_busy(Side::Left) {
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "the flat listing never came");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app_state.poll_listings();
    }
    assert_eq!(drawn(&mut app_state, "GNUmakefile").0, Color::Rgb(0, 0, 255), "a whole name, flat");

    crate::display::set_file_colors(Vec::new());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// F11's full-height column lines: off, the line between the columns stops
/// at the last entry; on, it runs from the heading to the bottom of the
/// panel, and the entries' rows read as they did.
#[test]
fn column_lines_can_run_the_whole_height() {
    let dir = std::env::temp_dir().join(format!("fm84-full-height-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("one.txt"), "1").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.open_dir(Side::Right, dir.clone(), None);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    let mut rows = |app_state: &mut AppState| {
        crate::display::apply(&app_state.options);
        render_ui(&mut terminal, app_state);
        let buffer = terminal.backend().buffer();
        (0..30).map(|y| (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect::<Vec<String>>()
    };
    let lines = |row: &String| row.matches('│').count();
    let off = rows(&mut app_state);
    let entry = off.iter().position(|row| row.contains("one ")).expect("the entry drawn");
    let heading = off.iter().position(|row| row.contains("Name")).expect("the heading drawn");
    assert!(lines(&off[entry]) > lines(&off[entry + 2]), "the lines stop at the last entry: {off:#?}");

    app_state.options.full_height_lines = true;
    let on = rows(&mut app_state);
    assert_eq!(on[entry], off[entry], "an entry as it was");
    assert!((heading + 1..entry).all(|y| on[y] == off[y]), "and those above it");
    let bottom = on.iter().rposition(|row| lines(row) == lines(&on[entry])).unwrap();
    assert!(bottom > entry + 10, "down to the bottom: {on:#?}");
    assert!((entry..=bottom).all(|y| lines(&on[y]) == lines(&on[entry])), "{on:#?}");
    assert_eq!(lines(&on[heading]), lines(&on[entry]), "through the heading: {on:#?}");

    std::fs::remove_dir_all(&dir).unwrap();
}

/// F11's directory brackets: on, as they start, a directory reads [name];
/// off, its name alone - a file either way as it was.
#[test]
fn directory_brackets_can_be_turned_off() {
    let dir = std::env::temp_dir().join(format!("fm84-dir-brackets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inner")).unwrap();
    std::fs::write(dir.join("one.txt"), "1").unwrap();

    let mut app_state = AppState::new();
    app_state.options = Options::default();
    assert!(app_state.options.dir_brackets, "on to start with");
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.open_dir(Side::Right, dir.clone(), None);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    let mut screen = |app_state: &mut AppState| {
        render_ui(&mut terminal, app_state);
        let buffer = terminal.backend().buffer();
        (0..30).map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect::<Vec<String>>().join("\n")
    };
    let on = screen(&mut app_state);
    assert!(on.contains("[inner]") && on.contains("one "), "{on}");

    app_state.options.dir_brackets = false;
    let off = screen(&mut app_state);
    assert!(!off.contains("[inner]") && off.contains("inner") && off.contains("one "), "{off}");

    std::fs::remove_dir_all(&dir).unwrap();
}

/// F11's Turbo: the neon turned up, the panels in scanlines, and the logo
/// in F1 blinking - and none of it with Turbo off.
#[test]
fn turbo_turns_the_neon_up() {
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    crate::display::apply(&app_state.options);
    let plain = (palette().title, palette().background);
    app_state.options.turbo = true;
    crate::display::apply(&app_state.options);
    assert_ne!(palette().title, plain.0, "text turned up");
    assert_eq!(palette().background, plain.1, "what is behind it left alone");

    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    render_ui(&mut terminal, &mut app_state);
    let buffer = terminal.backend().buffer().clone();
    // Inside the panels, every other row dimmed.
    let dimmed = |y: u16| (1..99).all(|x| buffer[(x, y)].modifier.contains(Modifier::DIM));
    let rows: Vec<bool> = (6..12).map(dimmed).collect();
    assert!(rows.windows(2).all(|pair| pair[0] != pair[1]), "{rows:?}");

    // The logo in F1 goes out and comes back, by the clock: drawn over a
    // second, some frames have its blocks lit and some not.
    app_state.dialog = Some(Dialog::Help);
    app_state.help_scroll = help_lines(&app_state.scores).iter().position(|(_, kind)| *kind == HelpLine::Logo).unwrap();
    let mut seen = std::collections::HashSet::new();
    let started = std::time::Instant::now();
    while seen.len() < 2 && started.elapsed() < std::time::Duration::from_millis(1500) {
        render_ui(&mut terminal, &mut app_state);
        let buffer = terminal.backend().buffer().clone();
        let block = (0..30).flat_map(|y| (0..100).map(move |x| (x, y))).find(|&(x, y)| buffer[(x, y)].symbol() == "█").unwrap();
        seen.insert(buffer[block].modifier.contains(Modifier::BOLD));
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(seen.len(), 2, "the logo blinks");

    app_state.options.turbo = false;
    crate::display::apply(&app_state.options);
    assert_eq!(palette().title, plain.0);
}

/// F11's night drive: left alone long enough on the panels, and only with
/// the option on, the screen goes to the grid with the logo over it; what
/// comes in next only wakes it, and the panels are back as they were.
#[test]
fn left_alone_fm84_drives_until_a_key() {
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    crate::display::apply(&app_state.options);
    let minutes = |count: u64| std::time::Instant::now().checked_sub(std::time::Duration::from_secs(count * 60)).unwrap();
    let long_ago = minutes(5);
    app_state.last_input = long_ago;
    app_state.poll_night_drive();
    assert!(app_state.night_drive.is_none(), "not with the option off");

    // As long as F11 says, and no sooner.
    app_state.options.night_drive_minutes = 5;
    app_state.last_input = minutes(4);
    app_state.poll_night_drive();
    assert!(app_state.night_drive.is_none(), "not before it has been long enough");

    app_state.last_input = long_ago;
    app_state.dialog = Some(Dialog::Help);
    app_state.poll_night_drive();
    assert!(app_state.night_drive.is_none(), "not over something open");
    app_state.dialog = None;

    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    render_ui(&mut terminal, &mut app_state);
    let panels: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();

    app_state.poll_night_drive();
    assert!(app_state.night_drive.is_some());
    render_ui(&mut terminal, &mut app_state);
    let screen: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert!(screen.contains("╚═╝     ╚═╝     ╚═╝"), "the logo");
    assert!(!screen.contains("F10"), "the key bar gone with the panels");

    // Woken, and nothing else: the panels as they were.
    assert!(app_state.wake(), "a key that only wakes");
    assert!(!app_state.wake(), "the next one goes through");
    render_ui(&mut terminal, &mut app_state);
    let back: String = terminal.backend().buffer().content().iter().map(|cell| cell.symbol()).collect();
    assert_eq!(back, panels);
}

/// A ghost hunt's rows are drawn faded, a ghost beside each; a panel fed
/// anything else is not.
#[test]
fn ghosts_are_drawn_faded() {
    let dir = std::env::temp_dir().join(format!("fm84-ghost-rows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("boo.txt"), "boo").unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.options.icon_style = crate::options::IconStyle::Plain;
    app_state.open_dir(Side::Left, dir.clone(), None);
    let row = |app_state: &mut AppState, kind| {
        app_state.left.flat = Some(crate::app::Flat {
            dir: dir.clone(),
            feed: Some(std::sync::Arc::new(vec![(PathBuf::from("boo.txt"), false)])),
            kind,
        });
        app_state.reload_panel(Side::Left, None);
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        render_ui(&mut terminal, app_state);
        let buffer = terminal.backend().buffer().clone();
        let (x, y) = (0..30).flat_map(|y| (0..50).map(move |x| (x, y))).find(|&(x, y)| buffer[(x, y)].symbol() == "b" && buffer[(x + 1, y)].symbol() == "o").expect("the row");
        let before: String = (0..x).map(|at| buffer[(at, y)].symbol()).collect();
        (before.contains('~'), buffer[(x, y)].modifier.contains(Modifier::DIM))
    };
    assert_eq!(row(&mut app_state, crate::app::FeedKind::Ghosts), (true, true));
    assert_eq!(row(&mut app_state, crate::app::FeedKind::Found), (false, false));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Alt+F10's tree: the lines between the directories as ncd drew them, in
/// box lines or ASCII, each marked for what it holds - and drawn in place of
/// the panel's files.
#[test]
fn the_tree_is_drawn_with_its_lines() {
    use crate::app::{TreeMark, TreeRow};
    let row = |last: Vec<bool>, mark| TreeRow { path: PathBuf::from("/x"), last, mark };
    assert_eq!(tree_prefix(&row(vec![], TreeMark::Open), false), "▾ ");
    assert_eq!(tree_prefix(&row(vec![false], TreeMark::Closed), false), "├─▸ ");
    assert_eq!(tree_prefix(&row(vec![false, true], TreeMark::Empty), false), "│  └── ");
    assert_eq!(tree_prefix(&row(vec![true, false], TreeMark::Reading), false), "   ├─… ");
    assert_eq!(tree_prefix(&row(vec![false, true], TreeMark::Unread), true), "|  `-+ ");

    let dir = std::env::temp_dir().join(format!("fm84-tree-draw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("inner")).unwrap();
    std::fs::write(dir.join("listed-file.txt"), "x").unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.toggle_tree();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    render_ui(&mut terminal, &mut app_state);
    let left: String = (0..30).flat_map(|y| (0..50).map(move |x| (x, y))).map(|(x, y)| terminal.backend().buffer()[(x, y)].symbol().to_string()).collect();
    assert!(left.contains("Directory tree"));
    assert!(left.contains("inner"));
    assert!(!left.contains("listed-file"), "the files are not drawn under it");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A click on a tree's row puts the cursor there, one on its mark opens or
/// closes it, and a double click is Enter.
#[test]
fn the_tree_answers_the_mouse() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let dir = std::env::temp_dir().join(format!("fm84-tree-mouse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("one/inner")).unwrap();
    std::fs::create_dir_all(dir.join("two")).unwrap();
    let mut app_state = AppState::new();
    app_state.options = Options::default();
    app_state.active_side = Side::Left;
    app_state.open_dir(Side::Left, dir.clone(), None);
    app_state.toggle_tree();
    app_state.tree_expand();
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
    let find = |terminal: &Terminal<ratatui::backend::TestBackend>, name: &str| {
        let buffer = terminal.backend().buffer();
        (0..40u16).find_map(|y| {
            let line: String = (0..50u16).map(|x| buffer[(x, y)].symbol().to_string()).collect();
            line.find(name).map(|at| (line[..at].chars().count() as u16, y))
        })
    };
    let click = |app_state: &mut AppState, (x, y): (u16, u16)| {
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        crate::input::handle_mouse_for_tests(app_state, event);
    };
    render_ui(&mut terminal, &mut app_state);
    let (x, y) = find(&terminal, "one").expect("drawn");
    click(&mut app_state, (x, y));
    let tree = app_state.left.tree.as_ref().unwrap();
    assert_eq!(tree.rows[tree.cursor].path, dir.join("one"));
    // On its mark, two cells before the name: opened.
    click(&mut app_state, (x - 2, y));
    render_ui(&mut terminal, &mut app_state);
    assert!(find(&terminal, "inner").is_some(), "opened");
    // Twice on the name, a moment apart: Enter, the other panel sent there.
    let (x, y) = find(&terminal, "two").unwrap();
    click(&mut app_state, (x, y));
    click(&mut app_state, (x, y));
    assert_eq!(app_state.right.dir, dir.join("two"));
    std::fs::remove_dir_all(&dir).unwrap();
}
