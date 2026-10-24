//! The config file: plain `key = value` lines, read forgivingly and written
//! whole, beside the session and bookmarks files.

use super::*;

/// The `key = value` lines of a file, skipping blanks, comments and anything
/// without an equals sign.
pub(super) fn key_values(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim(), value.trim()))
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "on" | "yes" => Some(true),
        "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

/// Written the way the editor saves: beside the file and renamed over it.
/// Written straight over, a crash or a full disk partway left it empty, and
/// every option with it - and two fm84s quitting at once could each catch the
/// other's half-written session.
pub(super) fn write_config_file(name: &str, contents: &str) -> io::Result<()> {
    // A test that changes an option must not write over the config of
    // whoever runs the tests.
    if cfg!(test) {
        return Ok(());
    }
    let path = config_path(name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No config directory"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    crate::fs_ops::save_file(&path, contents.as_bytes())
}

/// `%APPDATA%\fm84\<name>` on Windows, `$XDG_CONFIG_HOME/fm84/<name>` or
/// `~/.config/fm84/<name>` everywhere else.
pub fn config_path(name: &str) -> Option<PathBuf> {
    let non_empty = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        non_empty("APPDATA")?
    } else {
        non_empty("XDG_CONFIG_HOME").or_else(|| non_empty("HOME").map(|home| home.join(".config")))?
    };
    Some(base.join("fm84").join(name))
}

impl Options {
    /// Read the config file. A missing file, or a line that makes no sense,
    /// leaves that setting at its default rather than refusing to start.
    pub fn load() -> Self {
        config_path("config").and_then(|path| fs::read_to_string(path).ok()).map(|text| Self::parse(&text)).unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        write_config_file("config", &self.serialize())
    }

    pub(super) fn parse(text: &str) -> Self {
        let mut options = Self::default();
        for (key, value) in key_values(text) {
            // A number in the given range, or None for anything else.
            let number = |range: std::ops::RangeInclusive<u64>| value.parse::<u64>().ok().filter(|n| range.contains(n));
            let flag = |current: bool| parse_bool(value).unwrap_or(current);
            match key {
                "show_hidden" => options.show_hidden = flag(options.show_hidden),
                "sort" => {
                    options.sort_key = match value {
                        "name" => SortKey::Name,
                        "extension" => SortKey::Extension,
                        "size" => SortKey::Size,
                        "modified" => SortKey::Modified,
                        _ => options.sort_key,
                    }
                }
                "sort_descending" => options.sort_descending = flag(options.sort_descending),
                "dirs_first" => options.dirs_first = flag(options.dirs_first),
                "case_sensitive" => options.case_sensitive = flag(options.case_sensitive),
                "column_ext" => options.column_ext = flag(options.column_ext),
                "column_size" => options.column_size = flag(options.column_size),
                "column_modified" => options.column_modified = flag(options.column_modified),
                "column_attributes" => options.column_attributes = flag(options.column_attributes),
                "date_format" => {
                    options.date_format = match value {
                        "short" => DateFormat::Short,
                        "iso" => DateFormat::Iso,
                        "relative" => DateFormat::Relative,
                        _ => options.date_format,
                    }
                }
                "size_units" => {
                    options.size_units = match value {
                        "binary" => SizeUnits::Binary,
                        "decimal" => SizeUnits::Decimal,
                        _ => options.size_units,
                    }
                }
                "confirm_delete" => options.confirm_delete = flag(options.confirm_delete),
                "delete_to_trash" => options.delete_to_trash = flag(options.delete_to_trash),
                "confirm_copy_move" => options.confirm_copy_move = flag(options.confirm_copy_move),
                "on_existing" => {
                    options.on_existing = match value {
                        "ask" => OnExisting::Ask,
                        "overwrite" => OnExisting::Overwrite,
                        "refuse" => OnExisting::Refuse,
                        _ => options.on_existing,
                    }
                }
                "terminal" => options.terminal = value.to_string(),
                "editor" => options.editor = value.to_string(),
                "remember_dirs" => options.remember_dirs = flag(options.remember_dirs),
                "preview_on_start" => options.preview_on_start = flag(options.preview_on_start),
                "theme" => options.theme = Theme::ALL.into_iter().find(|theme| theme.key() == value).unwrap_or(options.theme),
                "background" => {
                    options.theme_background = match value {
                        "theme" => true,
                        "terminal" => false,
                        _ => options.theme_background,
                    }
                }
                "icons" => {
                    options.icon_style = match value {
                        "nerd-font" => IconStyle::NerdFont,
                        "nerd-font-mono" => IconStyle::NerdFontMono,
                        "plain" => IconStyle::Plain,
                        _ => options.icon_style,
                    }
                }
                "clock" => {
                    options.clock = match value {
                        "24" => Clock::Hours24,
                        "12" => Clock::Hours12,
                        "off" => Clock::Off,
                        _ => options.clock,
                    }
                }
                "cursor" => {
                    options.cursor = match value {
                        "bar" => CursorShape::Bar,
                        "block" => CursorShape::Block,
                        "underline" => CursorShape::Underline,
                        "classic" => CursorShape::Classic,
                        _ => options.cursor,
                    }
                }
                "cursor_blink" => options.cursor_blink = flag(options.cursor_blink),
                "tab_width" => options.tab_width = number(1..=16).map_or(options.tab_width, |n| n as usize),
                "line_numbers" => options.line_numbers = flag(options.line_numbers),
                "scroll_bar" => options.scroll_bar = ScrollBarStyle::ALL.into_iter().find(|style| style.key() == value).unwrap_or(options.scroll_bar),
                // Past 64 MiB the first parse alone would take about a minute.
                "highlight_limit_kib" => options.highlight_limit_kib = number(0..=65536).unwrap_or(options.highlight_limit_kib),
                "large_file_mib" => options.large_file_mib = number(1..=1_048_576).unwrap_or(options.large_file_mib),
                "image" => {
                    options.image_fill = match value {
                        "fit" => false,
                        "fill" => true,
                        _ => options.image_fill,
                    }
                }
                "image_backgrounds" => options.image_backgrounds = flag(options.image_backgrounds),
                "image_prefetch" => options.image_prefetch = flag(options.image_prefetch),
                _ => {}
            }
        }
        options
    }

    pub(super) fn serialize(&self) -> String {
        let sort = match self.sort_key {
            SortKey::Name => "name",
            SortKey::Extension => "extension",
            SortKey::Size => "size",
            SortKey::Modified => "modified",
        };
        let date_format = match self.date_format {
            DateFormat::Short => "short",
            DateFormat::Iso => "iso",
            DateFormat::Relative => "relative",
        };
        let size_units = match self.size_units {
            SizeUnits::Binary => "binary",
            SizeUnits::Decimal => "decimal",
        };
        let icons = match self.icon_style {
            IconStyle::NerdFont => "nerd-font",
            IconStyle::NerdFontMono => "nerd-font-mono",
            IconStyle::Plain => "plain",
        };
        let clock = match self.clock {
            Clock::Hours24 => "24",
            Clock::Hours12 => "12",
            Clock::Off => "off",
        };
        let cursor = match self.cursor {
            CursorShape::Bar => "bar",
            CursorShape::Block => "block",
            CursorShape::Underline => "underline",
            CursorShape::Classic => "classic",
        };
        format!(
            "# fm84 options, written by the F11 dialog.\n\
             \n\
             # Panels\n\
             show_hidden = {}\n\
             sort = {}\n\
             sort_descending = {}\n\
             dirs_first = {}\n\
             case_sensitive = {}\n\
             column_ext = {}\n\
             column_size = {}\n\
             column_modified = {}\n\
             column_attributes = {}\n\
             # short, iso or relative.\n\
             date_format = {}\n\
             # binary (KiB) or decimal (kB).\n\
             size_units = {}\n\
             \n\
             # Behaviour\n\
             confirm_delete = {}\n\
             # F8 to the trash; Shift+F8 always deletes for good.\n\
             delete_to_trash = {}\n\
             confirm_copy_move = {}\n\
             # ask, overwrite or refuse, when a copy or move finds its name taken.\n\
             on_existing = {}\n\
             # Empty picks one automatically. {{}} stands for the directory.\n\
             terminal = {}\n\
             # Empty is the built-in editor. {{}} stands for the file, which otherwise goes last.\n\
             editor = {}\n\
             remember_dirs = {}\n\
             preview_on_start = {}\n\
             \n\
             # Appearance\n\
             theme = {}\n\
             # theme paints the theme's own background; terminal keeps the terminal's.\n\
             background = {}\n\
             icons = {}\n\
             # 24, 12 or off.\n\
             clock = {}\n\
             # bar, block or underline is the terminal's cursor; classic a reversed cell.\n\
             cursor = {}\n\
             cursor_blink = {}\n\
             \n\
             # Viewer and editor\n\
             tab_width = {}\n\
             line_numbers = {}\n\
             # heavy, double, dashed, block, shade or ascii.\n\
             scroll_bar = {}\n\
             # Zero turns syntax highlighting off.\n\
             highlight_limit_kib = {}\n\
             large_file_mib = {}\n\
             image = {}\n\
             image_backgrounds = {}\n\
             image_prefetch = {}\n",
            self.show_hidden,
            sort,
            self.sort_descending,
            self.dirs_first,
            self.case_sensitive,
            self.column_ext,
            self.column_size,
            self.column_modified,
            self.column_attributes,
            date_format,
            size_units,
            self.confirm_delete,
            self.delete_to_trash,
            self.confirm_copy_move,
            match self.on_existing {
                OnExisting::Ask => "ask",
                OnExisting::Overwrite => "overwrite",
                OnExisting::Refuse => "refuse",
            },
            self.terminal,
            self.editor,
            self.remember_dirs,
            self.preview_on_start,
            self.theme.key(),
            if self.theme_background { "theme" } else { "terminal" },
            icons,
            clock,
            cursor,
            self.cursor_blink,
            self.tab_width,
            self.line_numbers,
            self.scroll_bar.key(),
            self.highlight_limit_kib,
            self.large_file_mib,
            if self.image_fill { "fill" } else { "fit" },
            self.image_backgrounds,
            self.image_prefetch,
        )
    }
}
