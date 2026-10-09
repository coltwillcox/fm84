use std::fs;
use std::io;
use std::path::PathBuf;

/// What the file rows are ordered by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Name,
    Extension,
    Size,
    Modified,
}

/// Which glyphs to draw. Nerd Font icons count as one cell but most Nerd Fonts
/// draw them across two, so the layout leaves room for the overflow; the Mono
/// variants draw them in one, and that room would push them off centre.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconStyle {
    NerdFont,
    NerdFontMono,
    Plain,
}

impl IconStyle {
    /// Whether a glyph spills into the cell after it.
    pub fn is_wide(self) -> bool {
        self == IconStyle::NerdFont
    }
}

/// The colour palette. The palettes themselves live in `display`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Synthwave,
    Outrun,
    Vaporwave,
    HighContrast,
    Dracula,
    Monokai,
    Nord,
    GruvboxDark,
    SolarizedDark,
    TokyoNight,
    CatppuccinMocha,
    SolarizedLight,
    GruvboxLight,
    CatppuccinLatte,
    GithubLight,
}

impl Theme {
    /// In the order F11 steps through them: fm84's own, then the well-known
    /// dark schemes, then the light ones.
    pub const ALL: [Theme; 15] = [
        Theme::Synthwave,
        Theme::Outrun,
        Theme::Vaporwave,
        Theme::HighContrast,
        Theme::Dracula,
        Theme::Monokai,
        Theme::Nord,
        Theme::GruvboxDark,
        Theme::SolarizedDark,
        Theme::TokyoNight,
        Theme::CatppuccinMocha,
        Theme::SolarizedLight,
        Theme::GruvboxLight,
        Theme::CatppuccinLatte,
        Theme::GithubLight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Theme::Synthwave => "Synthwave",
            Theme::Outrun => "Outrun",
            Theme::Vaporwave => "Vaporwave",
            Theme::HighContrast => "High contrast",
            Theme::Dracula => "Dracula",
            Theme::Monokai => "Monokai",
            Theme::Nord => "Nord",
            Theme::GruvboxDark => "Gruvbox Dark",
            Theme::SolarizedDark => "Solarized Dark",
            Theme::TokyoNight => "Tokyo Night",
            Theme::CatppuccinMocha => "Catppuccin Mocha",
            Theme::SolarizedLight => "Solarized Light",
            Theme::GruvboxLight => "Gruvbox Light",
            Theme::CatppuccinLatte => "Catppuccin Latte",
            Theme::GithubLight => "GitHub Light",
        }
    }

    /// How the config file names it.
    fn key(self) -> &'static str {
        match self {
            Theme::Synthwave => "synthwave",
            Theme::Outrun => "outrun",
            Theme::Vaporwave => "vaporwave",
            Theme::HighContrast => "high-contrast",
            Theme::Dracula => "dracula",
            Theme::Monokai => "monokai",
            Theme::Nord => "nord",
            Theme::GruvboxDark => "gruvbox-dark",
            Theme::SolarizedDark => "solarized-dark",
            Theme::TokyoNight => "tokyo-night",
            Theme::CatppuccinMocha => "catppuccin-mocha",
            Theme::SolarizedLight => "solarized-light",
            Theme::GruvboxLight => "gruvbox-light",
            Theme::CatppuccinLatte => "catppuccin-latte",
            Theme::GithubLight => "github-light",
        }
    }
}

/// How the Modified column writes a date.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DateFormat {
    /// 28/09/26 19:48
    Short,
    /// 2026-09-28 19:48
    Iso,
    /// 3 h ago
    Relative,
}

/// Powers of 1024 (KiB) or of 1000 (kB).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeUnits {
    Binary,
    Decimal,
}

/// What a copy or move does when a name it is writing is already taken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnExisting {
    /// Stop and ask, naming what is in the way.
    Ask,
    /// Replace files and merge directories without asking.
    Overwrite,
    /// Refuse the whole transfer, as fm84 always did.
    Refuse,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clock {
    Hours24,
    Hours12,
    Off,
}

// The values the stepped numeric options offer. A hand-edited config may hold
// something else; it is kept, and the first step from it lands on the list.
const TAB_WIDTHS: [usize; 3] = [2, 4, 8];
/// Zero turns highlighting off.
const HIGHLIGHT_LIMITS_KIB: [u64; 4] = [0, 256, 512, 2048];
const LARGE_FILE_LIMITS_MIB: [u64; 3] = [16, 64, 256];

/// Everything F11 can change, as it is kept in the config file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Options {
    pub show_hidden: bool,
    pub sort_key: SortKey,
    pub sort_descending: bool,
    pub dirs_first: bool,
    pub case_sensitive: bool,
    pub column_ext: bool,
    pub column_size: bool,
    pub column_modified: bool,
    pub column_attributes: bool,
    pub date_format: DateFormat,
    pub size_units: SizeUnits,
    pub confirm_delete: bool,
    /// F8 moves to the system's trash rather than deleting for good, which
    /// Shift+F8 still does.
    pub delete_to_trash: bool,
    pub confirm_copy_move: bool,
    pub on_existing: OnExisting,
    /// The F9 command. Empty means pick one per platform, as before there was
    /// a choice; `{}` in it stands for the panel's directory.
    pub terminal: String,
    /// The F4 command. Empty means the built-in editor; `{}` stands for the
    /// file, which otherwise goes on the end.
    pub editor: String,
    /// Reopen both panels where they were when fm84 last quit.
    pub remember_dirs: bool,
    pub preview_on_start: bool,
    pub theme: Theme,
    /// Paint the theme's background, rather than drawing on the terminal's.
    /// A light theme needs it; on a dark terminal its text would vanish.
    pub theme_background: bool,
    pub icon_style: IconStyle,
    pub clock: Clock,
    pub tab_width: usize,
    /// The gutter in the viewer and the editor.
    pub line_numbers: bool,
    /// The editor highlights files up to this size, in KiB; zero never does.
    pub highlight_limit_kib: u64,
    /// The viewer and editor ask before loading a file past this, in MiB.
    pub large_file_mib: u64,
    /// Whether a picture opens filling the viewer rather than fitting inside it.
    pub image_fill: bool,
    /// Colour each character's background as well as the character itself. A
    /// glyph alone leaves dark parts of a picture showing the terminal through
    /// the gaps, so they lose their colour entirely; a background keeps it, at
    /// the cost of roughly twice the escape sequences per frame.
    pub image_backgrounds: bool,
    /// Decode the pictures either side of the one being looked at, so stepping
    /// through a folder does not wait for each one. Costs a few megabytes and
    /// a thread that wakes only while a picture is open.
    pub image_prefetch: bool,
}

impl Default for Options {
    // What fm84 did before any of this was configurable.
    fn default() -> Self {
        Self {
            show_hidden: true,
            sort_key: SortKey::Extension,
            sort_descending: false,
            dirs_first: true,
            case_sensitive: false,
            column_ext: true,
            column_size: true,
            column_modified: true,
            column_attributes: true,
            date_format: DateFormat::Short,
            size_units: SizeUnits::Binary,
            confirm_delete: true,
            delete_to_trash: true,
            confirm_copy_move: true,
            on_existing: OnExisting::Ask,
            terminal: String::new(),
            editor: String::new(),
            remember_dirs: false,
            preview_on_start: false,
            theme: Theme::Synthwave,
            theme_background: true,
            icon_style: IconStyle::NerdFont,
            clock: Clock::Hours24,
            tab_width: 4,
            line_numbers: true,
            highlight_limit_kib: 512,
            large_file_mib: 64,
            image_fill: false,
            image_backgrounds: true,
            image_prefetch: true,
        }
    }
}

/// The rows of the options popup, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionRow {
    ShowHidden,
    SortKey,
    SortDirection,
    DirsFirst,
    CaseSensitive,
    ColumnExt,
    ColumnSize,
    ColumnModified,
    ColumnAttributes,
    DateFormat,
    SizeUnits,
    ConfirmDelete,
    DeleteToTrash,
    ConfirmCopyMove,
    OnExisting,
    Terminal,
    Editor,
    RememberDirs,
    PreviewOnStart,
    Theme,
    ThemeBackground,
    IconStyle,
    Clock,
    TabWidth,
    LineNumbers,
    HighlightLimit,
    LargeFile,
    ImageDefault,
    ImageBackgrounds,
    ImagePrefetch,
}

pub const OPTION_ROWS: [OptionRow; 30] = [
    OptionRow::ShowHidden,
    OptionRow::SortKey,
    OptionRow::SortDirection,
    OptionRow::DirsFirst,
    OptionRow::CaseSensitive,
    OptionRow::ColumnExt,
    OptionRow::ColumnSize,
    OptionRow::ColumnModified,
    OptionRow::ColumnAttributes,
    OptionRow::DateFormat,
    OptionRow::SizeUnits,
    OptionRow::ConfirmDelete,
    OptionRow::DeleteToTrash,
    OptionRow::ConfirmCopyMove,
    OptionRow::OnExisting,
    OptionRow::Terminal,
    OptionRow::Editor,
    OptionRow::RememberDirs,
    OptionRow::PreviewOnStart,
    OptionRow::Theme,
    OptionRow::ThemeBackground,
    OptionRow::IconStyle,
    OptionRow::Clock,
    OptionRow::TabWidth,
    OptionRow::LineNumbers,
    OptionRow::HighlightLimit,
    OptionRow::LargeFile,
    OptionRow::ImageDefault,
    OptionRow::ImageBackgrounds,
    OptionRow::ImagePrefetch,
];

impl OptionRow {
    pub fn label(self) -> &'static str {
        match self {
            OptionRow::ShowHidden => "Show hidden files",
            OptionRow::SortKey => "Sort by",
            OptionRow::SortDirection => "Sort direction",
            OptionRow::DirsFirst => "Directories first",
            OptionRow::CaseSensitive => "Case-sensitive sort",
            OptionRow::ColumnExt => "Ext column",
            OptionRow::ColumnSize => "Size column",
            OptionRow::ColumnModified => "Modified column",
            OptionRow::ColumnAttributes => "Attributes column",
            OptionRow::DateFormat => "Date format",
            OptionRow::SizeUnits => "Size units",
            OptionRow::ConfirmDelete => "Confirm delete",
            OptionRow::DeleteToTrash => "Delete to trash (F8)",
            OptionRow::ConfirmCopyMove => "Confirm copy and move",
            OptionRow::OnExisting => "When destination exists",
            OptionRow::Terminal => "Terminal (F9)",
            OptionRow::Editor => "Editor (F4)",
            OptionRow::RememberDirs => "Remember directories",
            OptionRow::PreviewOnStart => "Preview on startup",
            OptionRow::Theme => "Theme",
            OptionRow::ThemeBackground => "Background",
            OptionRow::IconStyle => "Icons",
            OptionRow::Clock => "Clock",
            OptionRow::TabWidth => "Tab width",
            OptionRow::LineNumbers => "Line numbers",
            OptionRow::HighlightLimit => "Highlight files up to",
            OptionRow::LargeFile => "Ask before opening over",
            OptionRow::ImageDefault => "Images open as",
            OptionRow::ImageBackgrounds => "Image backgrounds",
            OptionRow::ImagePrefetch => "Read pictures ahead",
        }
    }

    /// The heading this row opens, when it is the first of its group.
    pub fn section(self) -> Option<&'static str> {
        match self {
            OptionRow::ShowHidden => Some("Panels"),
            OptionRow::ConfirmDelete => Some("Behaviour"),
            OptionRow::Theme => Some("Appearance"),
            OptionRow::TabWidth => Some("Viewer and editor"),
            _ => None,
        }
    }

    /// True for a row edited by typing rather than by stepping through values.
    pub fn is_text(self) -> bool {
        matches!(self, OptionRow::Terminal | OptionRow::Editor)
    }

    /// Whether changing this row changes what the panels hold, rather than
    /// only how they are drawn. The sizes are written out as the rows load.
    pub fn affects_listing(self) -> bool {
        matches!(
            self,
            OptionRow::ShowHidden
                | OptionRow::SortKey
                | OptionRow::SortDirection
                | OptionRow::DirsFirst
                | OptionRow::CaseSensitive
                | OptionRow::SizeUnits
        )
    }
}

impl Options {
    pub fn value(&self, row: OptionRow) -> String {
        let on_off = |on: bool| if on { "On" } else { "Off" }.to_string();
        match row {
            OptionRow::ShowHidden => on_off(self.show_hidden),
            OptionRow::SortKey => match self.sort_key {
                SortKey::Name => "Name",
                SortKey::Extension => "Extension",
                SortKey::Size => "Size",
                SortKey::Modified => "Modified",
            }
            .to_string(),
            OptionRow::SortDirection => if self.sort_descending { "Descending" } else { "Ascending" }.to_string(),
            OptionRow::DirsFirst => on_off(self.dirs_first),
            OptionRow::CaseSensitive => on_off(self.case_sensitive),
            OptionRow::ColumnExt => on_off(self.column_ext),
            OptionRow::ColumnSize => on_off(self.column_size),
            OptionRow::ColumnModified => on_off(self.column_modified),
            OptionRow::ColumnAttributes => on_off(self.column_attributes),
            OptionRow::DateFormat => match self.date_format {
                DateFormat::Short => "dd/mm/yy",
                DateFormat::Iso => "yyyy-mm-dd",
                DateFormat::Relative => "Relative",
            }
            .to_string(),
            OptionRow::SizeUnits => match self.size_units {
                SizeUnits::Binary => "KiB (1024)",
                SizeUnits::Decimal => "kB (1000)",
            }
            .to_string(),
            OptionRow::ConfirmDelete => on_off(self.confirm_delete),
            OptionRow::DeleteToTrash => on_off(self.delete_to_trash),
            OptionRow::ConfirmCopyMove => on_off(self.confirm_copy_move),
            OptionRow::OnExisting => match self.on_existing {
                OnExisting::Ask => "Ask",
                OnExisting::Overwrite => "Overwrite",
                OnExisting::Refuse => "Refuse",
            }
            .to_string(),
            OptionRow::Terminal if self.terminal.is_empty() => "Automatic".to_string(),
            OptionRow::Terminal => self.terminal.clone(),
            OptionRow::Editor if self.editor.is_empty() => "Built-in".to_string(),
            OptionRow::Editor => self.editor.clone(),
            OptionRow::RememberDirs => on_off(self.remember_dirs),
            OptionRow::PreviewOnStart => on_off(self.preview_on_start),
            OptionRow::Theme => self.theme.label().to_string(),
            OptionRow::ThemeBackground => if self.theme_background { "Theme" } else { "Terminal" }.to_string(),
            OptionRow::IconStyle => match self.icon_style {
                IconStyle::NerdFont => "Nerd Font",
                IconStyle::NerdFontMono => "Nerd Font Mono",
                IconStyle::Plain => "Plain",
            }
            .to_string(),
            OptionRow::Clock => match self.clock {
                Clock::Hours24 => "24-hour",
                Clock::Hours12 => "12-hour",
                Clock::Off => "Off",
            }
            .to_string(),
            OptionRow::TabWidth => self.tab_width.to_string(),
            OptionRow::LineNumbers => on_off(self.line_numbers),
            OptionRow::HighlightLimit => match self.highlight_limit_kib {
                0 => "Off".to_string(),
                kib if kib % 1024 == 0 => format!("{} MiB", kib / 1024),
                kib => format!("{} KiB", kib),
            },
            OptionRow::LargeFile => format!("{} MiB", self.large_file_mib),
            OptionRow::ImageDefault => if self.image_fill { "Fill" } else { "Fit" }.to_string(),
            OptionRow::ImageBackgrounds => on_off(self.image_backgrounds),
            OptionRow::ImagePrefetch => on_off(self.image_prefetch),
        }
    }

    /// The text a text row holds, for typing into.
    pub fn text(&self, row: OptionRow) -> &str {
        match row {
            OptionRow::Editor => &self.editor,
            _ => &self.terminal,
        }
    }

    pub fn set_text(&mut self, row: OptionRow, text: String) {
        match row {
            OptionRow::Editor => self.editor = text,
            OptionRow::Terminal => self.terminal = text,
            _ => {}
        }
    }

    /// Step a row to its next value, or its previous one. A text row does not
    /// step; it is typed into instead.
    pub fn cycle(&mut self, row: OptionRow, forward: bool) {
        fn step<T: Copy + PartialEq>(values: &[T], current: T, forward: bool) -> T {
            let Some(index) = values.iter().position(|&value| value == current) else {
                return values[0];
            };
            let next = if forward { index + 1 } else { index + values.len() - 1 };
            values[next % values.len()]
        }

        match row {
            OptionRow::ShowHidden => self.show_hidden = !self.show_hidden,
            OptionRow::SortKey => {
                let keys = [SortKey::Name, SortKey::Extension, SortKey::Size, SortKey::Modified];
                self.sort_key = step(&keys, self.sort_key, forward);
            }
            OptionRow::SortDirection => self.sort_descending = !self.sort_descending,
            OptionRow::DirsFirst => self.dirs_first = !self.dirs_first,
            OptionRow::CaseSensitive => self.case_sensitive = !self.case_sensitive,
            OptionRow::ColumnExt => self.column_ext = !self.column_ext,
            OptionRow::ColumnSize => self.column_size = !self.column_size,
            OptionRow::ColumnModified => self.column_modified = !self.column_modified,
            OptionRow::ColumnAttributes => self.column_attributes = !self.column_attributes,
            OptionRow::DateFormat => {
                let formats = [DateFormat::Short, DateFormat::Iso, DateFormat::Relative];
                self.date_format = step(&formats, self.date_format, forward);
            }
            OptionRow::SizeUnits => {
                self.size_units = if self.size_units == SizeUnits::Binary { SizeUnits::Decimal } else { SizeUnits::Binary }
            }
            OptionRow::ConfirmDelete => self.confirm_delete = !self.confirm_delete,
            OptionRow::DeleteToTrash => self.delete_to_trash = !self.delete_to_trash,
            OptionRow::ConfirmCopyMove => self.confirm_copy_move = !self.confirm_copy_move,
            OptionRow::OnExisting => {
                let policies = [OnExisting::Ask, OnExisting::Overwrite, OnExisting::Refuse];
                self.on_existing = step(&policies, self.on_existing, forward);
            }
            OptionRow::RememberDirs => self.remember_dirs = !self.remember_dirs,
            OptionRow::PreviewOnStart => self.preview_on_start = !self.preview_on_start,
            OptionRow::Theme => {
                self.theme = step(&Theme::ALL, self.theme, forward);
            }
            OptionRow::ThemeBackground => {
                self.theme_background = !self.theme_background;
            }
            OptionRow::IconStyle => {
                let styles = [IconStyle::NerdFont, IconStyle::NerdFontMono, IconStyle::Plain];
                self.icon_style = step(&styles, self.icon_style, forward);
            }
            OptionRow::Clock => {
                let clocks = [Clock::Hours24, Clock::Hours12, Clock::Off];
                self.clock = step(&clocks, self.clock, forward);
            }
            OptionRow::TabWidth => self.tab_width = step(&TAB_WIDTHS, self.tab_width, forward),
            OptionRow::LineNumbers => self.line_numbers = !self.line_numbers,
            OptionRow::HighlightLimit => {
                self.highlight_limit_kib = step(&HIGHLIGHT_LIMITS_KIB, self.highlight_limit_kib, forward)
            }
            OptionRow::LargeFile => self.large_file_mib = step(&LARGE_FILE_LIMITS_MIB, self.large_file_mib, forward),
            OptionRow::ImageDefault => self.image_fill = !self.image_fill,
            OptionRow::ImageBackgrounds => self.image_backgrounds = !self.image_backgrounds,
            OptionRow::ImagePrefetch => self.image_prefetch = !self.image_prefetch,
            OptionRow::Terminal | OptionRow::Editor => {}
        }
    }

    pub fn highlight_limit_bytes(&self) -> u64 {
        self.highlight_limit_kib * 1024
    }

    pub fn large_file_bytes(&self) -> u64 {
        self.large_file_mib * 1024 * 1024
    }

    /// Read the config file. A missing file, or a line that makes no sense,
    /// leaves that setting at its default rather than refusing to start.
    pub fn load() -> Self {
        config_path("config")
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        write_config_file("config", &self.serialize())
    }

    fn parse(text: &str) -> Self {
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
                "tab_width" => options.tab_width = number(1..=16).map_or(options.tab_width, |n| n as usize),
                "line_numbers" => options.line_numbers = flag(options.line_numbers),
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

    fn serialize(&self) -> String {
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
             \n\
             # Viewer and editor\n\
             tab_width = {}\n\
             line_numbers = {}\n\
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
            self.tab_width,
            self.line_numbers,
            self.highlight_limit_kib,
            self.large_file_mib,
            if self.image_fill { "fill" } else { "fit" },
            self.image_backgrounds,
            self.image_prefetch,
        )
    }
}

/// One panel as the session file keeps it: the directory of each of its
/// tabs, and which one it was showing.
#[derive(Debug, PartialEq, Eq)]
pub struct PanelSession {
    pub tabs: Vec<PathBuf>,
    pub active: usize,
}

impl PanelSession {
    /// The directory the panel was showing.
    pub fn dir(&self) -> &std::path::Path {
        &self.tabs[self.active]
    }
}

/// Where the panels were when fm84 last quit, for Remember directories. Kept
/// apart from the options: it changes on every run, and the options only when
/// asked to.
pub fn load_session() -> Option<(PanelSession, PanelSession)> {
    let text = fs::read_to_string(config_path("session")?).ok()?;
    parse_session(&text)
}

/// `left` and `right` are the directories shown, as they always were, so a
/// session reads the same in a version from before tabs. Each tab follows as
/// a `left_tab` or `right_tab` line, in order, with which one was showing.
pub fn save_session(left: &PanelSession, right: &PanelSession) -> io::Result<()> {
    let mut text = format!("left = {}\nright = {}\n", escape_path(left.dir()), escape_path(right.dir()));
    for (side, panel) in [("left", left), ("right", right)] {
        for tab in &panel.tabs {
            text.push_str(&format!("{side}_tab = {}\n", escape_path(tab)));
        }
        text.push_str(&format!("{side}_tab_active = {}\n", panel.active));
    }
    write_config_file("session", &text)
}

/// A path as a session line holds it. `display()` would lose any name that is
/// not valid UTF-8, and the reader trims each value, so spaces at either end
/// would go too. Those bytes, control characters and `%` itself are written
/// as `%XX`; everything else stays as it reads, so the file is still fine to
/// look at and edit by hand.
fn escape_path(path: &std::path::Path) -> String {
    let escaped = |bytes: &[u8]| bytes.iter().map(|byte| format!("%{byte:02X}")).collect::<String>();

    let mut out = String::new();
    for chunk in path.as_os_str().as_encoded_bytes().utf8_chunks() {
        for ch in chunk.valid().chars() {
            if ch == '%' || ch.is_control() {
                out.push_str(&escaped(ch.encode_utf8(&mut [0; 4]).as_bytes()));
            } else {
                out.push(ch);
            }
        }
        out.push_str(&escaped(chunk.invalid()));
    }

    let start = out.len() - out.trim_start().len();
    let end = start + out.trim().len();
    format!("{}{}{}", escaped(&out.as_bytes()[..start]), &out[start..end], escaped(&out.as_bytes()[end..]))
}

/// The path escape_path wrote. A `%` not followed by two hex digits is kept
/// as it is, so a session written before paths were escaped still reads.
fn unescape_path(text: &str) -> PathBuf {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes.get(index + 1..index + 3).and_then(|pair| std::str::from_utf8(pair).ok());
        match (bytes[index], hex.and_then(|pair| u8::from_str_radix(pair, 16).ok())) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    path_from_bytes(out)
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

/// Elsewhere a path is not arbitrary bytes, and a hand-edited file could hold
/// anything, so it is read as text.
#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

fn parse_session(text: &str) -> Option<(PanelSession, PanelSession)> {
    let (mut left, mut right) = (None, None);
    let (mut left_tabs, mut right_tabs) = (Vec::new(), Vec::new());
    let (mut left_active, mut right_active) = (0, 0);
    for (key, value) in key_values(text) {
        match key {
            "left" if !value.is_empty() => left = Some(unescape_path(value)),
            "right" if !value.is_empty() => right = Some(unescape_path(value)),
            "left_tab" if !value.is_empty() => left_tabs.push(unescape_path(value)),
            "right_tab" if !value.is_empty() => right_tabs.push(unescape_path(value)),
            "left_tab_active" => left_active = value.parse().unwrap_or(0),
            "right_tab_active" => right_active = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    // Without tab lines - written before there were tabs, or by hand - a
    // panel is the one directory it names.
    let panel = |dir: PathBuf, tabs: Vec<PathBuf>, active: usize| {
        if tabs.is_empty() {
            PanelSession { tabs: vec![dir], active: 0 }
        } else {
            let active = active.min(tabs.len() - 1);
            PanelSession { tabs, active }
        }
    };
    Some((panel(left?, left_tabs, left_active), panel(right?, right_tabs, right_active)))
}

/// The `key = value` lines of a file, skipping blanks, comments and anything
/// without an equals sign.
fn key_values(text: &str) -> impl Iterator<Item = (&str, &str)> {
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

fn write_config_file(name: &str, contents: &str) -> io::Result<()> {
    let path = config_path(name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No config directory"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, contents)
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

#[cfg(test)]
mod tests {
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
            tab_width: 8,
            line_numbers: false,
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
             clock = 25\ndate_format = roman\nsize_units = furlongs\non_existing = sometimes\n",
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
        assert_eq!(left, PanelSession { tabs: vec!["/a".into(), "/b".into(), "/c".into()], active: 1 });
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
}