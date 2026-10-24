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
        date_format: DateFormat::Relative,
        size_units: SizeUnits::Decimal,
        confirm_delete: false,
        delete_to_trash: false,
        confirm_copy_move: false,
        on_existing: OnExisting::Overwrite,
        terminal: "kitty --directory {}".to_string(),
        editor: "nvim".to_string(),
        remember_dirs: true,
        preview_on_start: true,
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
         clock = 25\ncursor = triangle\ncursor_blink = sometimes\ndate_format = roman\nsize_units = furlongs\non_existing = sometimes\n",
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
    assert_eq!(options.sort_key, SortKey::Modified);
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
