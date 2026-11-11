use super::*;

#[test]
fn a_saved_file_reads_back_the_same() {
    let options = Options {
        show_hidden: false,
        sort_key: SortKey::Modified,
        sort_descending: true,
        dirs_first: false,
        case_sensitive: true,
        column_ext: false,
        column_size: false,
        column_modified: false,
        column_attributes: false,
        column_info: true,
        git_marks: false,
        date_format: DateFormat::Relative,
        size_units: SizeUnits::Decimal,
        confirm_delete: false,
        delete_to_trash: false,
        confirm_copy_move: false,
        on_existing: OnExisting::KeepBoth,
        verify_copies: true,
        terminal: "kitty --directory {}".to_string(),
        editor: "nvim".to_string(),
        remember_dirs: true,
        preview_on_start: true,
        language: Some(crate::i18n::Language::Serbian),
        theme: Theme::GithubLight,
        theme_background: false,
        icon_style: IconStyle::Plain,
        clock: Clock::Off,
        cursor: CursorShape::Underline,
        cursor_blink: false,
        tab_width: 8,
        line_numbers: false,
        scroll_bar: ScrollBarStyle::Ascii,
        highlight_limit_kib: 0,
        large_file_mib: 256,
        image_fill: true,
        image_backgrounds: false,
        image_prefetch: false,
        outrun_bar: true,
        burst_delete: true,
        turbo: true,
        night_drive_minutes: 5,
    };
    assert_eq!(Options::parse(&options.serialize()), options);
    assert_eq!(Options::parse(&Options::default().serialize()), Options::default());
}

#[test]
fn a_config_that_never_heard_of_backgrounds_still_gets_them() {
    // Parsing starts from the defaults, so a file written before the option
    // existed picks it up rather than reading as "off".
    let old = "show_hidden = true\nimage = fit\n";
    assert!(Options::parse(old).image_backgrounds);
    assert!(Options::default().image_backgrounds);
}

#[test]
fn nonsense_leaves_the_default() {
    let options = Options::parse(
        "show_hidden = maybe\nsort = colour\nno equals sign\nunknown = 1\n\
         tab_width = 0\ntab_width = lots\nlarge_file_mib = 0\nhighlight_limit_kib = -5\ntheme = beige\n\
         clock = 25\nlanguage = klingon\ncursor = triangle\ncursor_blink = sometimes\ndate_format = roman\nsize_units = furlongs\non_existing = sometimes\n",
    );
    assert_eq!(options, Options::default());
    assert_eq!(Options::parse(""), Options::default());
}

#[test]
fn cycling_wraps_both_ways() {
    let mut options = Options::default();
    options.cycle(OptionRow::SortKey, false);
    assert_eq!(options.sort_key, SortKey::Name);
    options.cycle(OptionRow::SortKey, false);
    assert_eq!(options.sort_key, SortKey::Info);
    options.cycle(OptionRow::SortKey, true);
    assert_eq!(options.sort_key, SortKey::Name);

    options.cycle(OptionRow::HighlightLimit, true);
    assert_eq!(options.highlight_limit_kib, 2048);
    options.cycle(OptionRow::HighlightLimit, true);
    assert_eq!(options.highlight_limit_kib, 0);
}

#[test]
fn a_hand_edited_value_steps_onto_the_list() {
    let mut options = Options::parse("tab_width = 3\n");
    assert_eq!(options.tab_width, 3);
    assert_eq!(options.value(OptionRow::TabWidth), "3");
    options.cycle(OptionRow::TabWidth, true);
    assert_eq!(options.tab_width, 2);
}

#[test]
fn a_session_needs_both_panels() {
    let (left, right) = parse_session("left = /home/colt\nright = /tmp\n").unwrap();
    assert_eq!((left.dir(), right.dir()), (std::path::Path::new("/home/colt"), std::path::Path::new("/tmp")));
    assert_eq!(left.tabs.len(), 1);
    assert_eq!(parse_session("left = /home/colt\n"), None);
    assert_eq!(parse_session("left =\nright = /tmp\n"), None);
}

#[test]
fn a_session_path_comes_back_as_it_went() {
    for path in ["/home/colt", " edged ", "/tmp/100%", "/tmp/tab\there", "/tmp/caf\u{e9}"] {
        let line = format!("left = {}\nright = /\n", escape_path(std::path::Path::new(path)));
        assert_eq!(parse_session(&line).unwrap().0.dir(), std::path::Path::new(path), "{line:?}");
    }
    // Written before paths were escaped: a % with no hex after it is itself.
    assert_eq!(unescape_path("/tmp/50%off"), PathBuf::from("/tmp/50%off"));
    // A sign is not a digit, though from_str_radix takes one.
    assert_eq!(unescape_path("/tmp/%+1x"), PathBuf::from("/tmp/%+1x"));
}

#[test]
fn bookmarks_come_back_in_order_and_skip_anything_else() {
    let text = "# kept by hand\nbookmark = /home/colt\nbookmark =\nother = /x\nbookmark =  edged%20\nbookmark = /tmp/100%25\n";
    assert_eq!(parse_bookmarks(text), [PathBuf::from("/home/colt"), PathBuf::from("edged "), PathBuf::from("/tmp/100%")]);
    let line = format!("bookmark = {}\n", escape_path(std::path::Path::new(" both ")));
    assert_eq!(parse_bookmarks(&line), [PathBuf::from(" both ")]);
}

#[cfg(unix)]
#[test]
fn a_session_keeps_a_name_that_is_not_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9"));
    let line = format!("left = {}\nright = /\n", escape_path(path));
    assert_eq!(line, "left = /tmp/caf%E9\nright = /\n");
    assert_eq!(parse_session(&line).unwrap().0.dir(), path);
}

#[test]
fn a_session_keeps_each_panels_tabs() {
    let text = "left = /b\nright = /r\nleft_tab = /a\nleft_tab = /b\nleft_tab = /c\nleft_tab_active = 1\n";
    let (left, right) = parse_session(text).unwrap();
    assert_eq!(
        left,
        PanelSession {
            tabs: vec!["/a".into(), "/b".into(), "/c".into()],
            active: 1
        }
    );
    assert_eq!(right, PanelSession { tabs: vec!["/r".into()], active: 0 });

    // An active tab past the end, as a hand edit might leave, is the last.
    let (left, _) = parse_session("left = /a\nright = /r\nleft_tab = /a\nleft_tab_active = 7\n").unwrap();
    assert_eq!(left.active, 0);
}

#[test]
fn every_row_sits_under_a_heading() {
    assert!(OPTION_ROWS[0].section().is_some());
    let headings = OPTION_ROWS.iter().filter(|row| row.section().is_some()).count();
    assert_eq!(headings, 4);
}

#[test]
fn menu_lines_read_as_label_and_command() {
    let text = "# a comment\nDisk use = du -sh {files}\n\n= no label\nno command =\nCount = wc -l {file} | sort = fine\n";
    let menu = parse_menu(text);
    let pairs: Vec<(&str, &str)> = menu.iter().map(|entry| (entry.label.as_str(), entry.command.as_str())).collect();
    // Split at the first =, so the command may hold more.
    assert_eq!(pairs, [("Disk use", "du -sh {files}"), ("Count", "wc -l {file} | sort = fine")]);
}

#[test]
fn open_lines_read_as_extensions_and_command() {
    let text = "# pdf = nothing\npdf  .EPUB,djvu = zathura {file}\nmd = ! vim\n = no extensions\ntxt =\n* = !less\n";
    let lines = parse_associations(text);
    let read: Vec<(Vec<&str>, &str, bool)> = lines.iter().map(|line| (line.extensions.iter().map(String::as_str).collect(), line.command.as_str(), line.terminal)).collect();
    assert_eq!(read, [(vec!["pdf", "EPUB", "djvu"], "zathura {file}", false), (vec!["md"], "vim", true), (vec!["*"], "less", true)]);
}

/// F11's night drive steps through its minutes, Off among them, and a
/// config can name any up to a day.
#[test]
fn the_night_drive_waits_as_many_minutes_as_asked() {
    let mut options = Options::default();
    assert_eq!(options.night_drive_minutes, 0, "off to begin with");
    assert_eq!(options.value(OptionRow::NightDrive), t!("Off"));
    options.cycle(OptionRow::NightDrive, true);
    options.cycle(OptionRow::NightDrive, true);
    assert_eq!(options.night_drive_minutes, 3);
    assert_eq!(options.value(OptionRow::NightDrive), "3 min");
    options.cycle(OptionRow::NightDrive, false);
    options.cycle(OptionRow::NightDrive, false);
    options.cycle(OptionRow::NightDrive, false);
    assert_eq!(options.night_drive_minutes, 30, "round from Off to the longest");
    assert_eq!(Options::parse("night_drive = 7\n").night_drive_minutes, 7);
    assert_eq!(Options::parse("night_drive = forever\n").night_drive_minutes, 0);
}

/// The mixtapes file: each tape's name and what is on it, odd names and
/// all, and which is playing.
#[test]
fn mixtapes_read_back_the_same() {
    let mixtapes = Mixtapes {
        tapes: vec![
            Tape {
                name: "Release files".into(),
                items: vec![(PathBuf::from("/srv/out/fm84.tar.gz"), false), (PathBuf::from(" edged %"), true)],
            },
            Tape { name: "Tax 2026".into(), items: Vec::new() },
        ],
        playing: 1,
    };
    assert_eq!(Mixtapes::parse(&mixtapes.serialize()), mixtapes);
    // A file before any tape has nowhere to go, and a tape not there does
    // not play.
    let read = Mixtapes::parse("file = /lost\nplaying = Gone\ntape = One\nfile = /a\n");
    assert_eq!(
        read.tapes,
        vec![Tape {
            name: "One".into(),
            items: vec![(PathBuf::from("/a"), false)]
        }]
    );
    assert_eq!(read.playing, 0);
    assert_eq!(Mixtapes::default().fresh_name("Mixtape"), "Mixtape");
    assert_eq!(mixtapes.fresh_name("Tax 2026"), "Tax 2026 2");
}

/// The `colors` file: colours by hex, short hex, number or name, a pair for
/// dark themes and light ones, styles with a colour or alone - and a line
/// that makes no sense passed over.
#[test]
fn the_colors_file_reads_colours_pairs_and_styles() {
    use ratatui::style::{Color, Modifier};
    let rules = parse_colors(
        "# a comment\n\
         rs .TOML = #dea584\n\
         jpg png = bright-magenta\n\
         tar.gz = bold 202\n\
         md = #7af / #1a4b95\n\
         log = dim\n\
         makefile = italic underline cyan\n\
         bad = nocolour\n\
         two = red blue\n\
         three = red / green / blue\n\
         = red\n\
         empty =\n",
    );
    let line = |extension: &str| rules.iter().find(|rule| rule.extensions.iter().any(|it| it == extension)).cloned();
    assert_eq!(rules.len(), 6, "{rules:#?}");
    let rs = line("rs").unwrap();
    assert_eq!((rs.dark, rs.light, rs.modifiers), (Some(Color::Rgb(0xde, 0xa5, 0x84)), Some(Color::Rgb(0xde, 0xa5, 0x84)), Modifier::empty()));
    assert_eq!(rs.extensions, ["rs", "TOML"], "the dot gone, the case kept");
    assert_eq!(line("png").unwrap().dark, Some(Color::LightMagenta));
    let tar = line("tar.gz").unwrap();
    assert_eq!((tar.dark, tar.modifiers), (Some(Color::Indexed(202)), Modifier::BOLD));
    let md = line("md").unwrap();
    assert_eq!((md.dark, md.light), (Some(Color::Rgb(0x77, 0xaa, 0xff)), Some(Color::Rgb(0x1a, 0x4b, 0x95))));
    let log = line("log").unwrap();
    assert_eq!((log.dark, log.light, log.modifiers), (None, None, Modifier::DIM), "a style alone keeps the colour");
    assert_eq!(line("makefile").unwrap().modifiers, Modifier::ITALIC | Modifier::UNDERLINED);
    for bad in ["bad", "two", "three", "empty"] {
        assert!(line(bad).is_none(), "{bad}");
    }

    // Which file a line is for: by the end of its name, any case, a whole
    // name too, and * for every one.
    assert!(rs.applies("main.rs") && rs.applies("Cargo.toml") && rs.applies("MAIN.RS"));
    assert!(!rs.applies("main.rsx") && !rs.applies("rs.txt") && !rs.applies("ars"));
    assert!(tar.applies("x.tar.gz") && !tar.applies("x.gz"));
    assert!(line("makefile").unwrap().applies("Makefile") && rs.applies("rs"));
    assert!(parse_colors("* = red")[0].applies("anything"));
}
