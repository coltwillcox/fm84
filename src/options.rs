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
}

pub const OPTION_ROWS: [OptionRow; 28] = [
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
             image_backgrounds = {}\n",
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
        )
    }
}

/// Where the panels were when fm84 last quit, for Remember directories. Kept
/// apart from the options: it changes on every run, and the options only when
/// asked to.
pub fn load_session() -> Option<(PathBuf, PathBuf)> {
    let text = fs::read_to_string(config_path("session")?).ok()?;
    parse_session(&text)
}

pub fn save_session(left: &std::path::Path, right: &std::path::Path) -> io::Result<()> {
    write_config_file("session", &format!("left = {}\nright = {}\n", left.display(), right.display()))
}

fn parse_session(text: &str) -> Option<(PathBuf, PathBuf)> {
    let (mut left, mut right) = (None, None);
    for (key, value) in key_values(text) {
        match key {
            "left" if !value.is_empty() => left = Some(PathBuf::from(value)),
            "right" if !value.is_empty() => right = Some(PathBuf::from(value)),
            _ => {}
        }
    }
    Some((left?, right?))
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
fn config_path(name: &str) -> Option<PathBuf> {
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
        };
        assert_eq!(Options::parse(&options.serialize()), options);
        assert_eq!(Options::parse(&Options::default().serialize()), Options::default());
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
        assert_eq!((left, right), (PathBuf::from("/home/colt"), PathBuf::from("/tmp")));
        assert_eq!(parse_session("left = /home/colt\n"), None);
        assert_eq!(parse_session("left =\nright = /tmp\n"), None);
    }

    #[test]
    fn every_row_sits_under_a_heading() {
        assert!(OPTION_ROWS[0].section().is_some());
        let headings = OPTION_ROWS.iter().filter(|row| row.section().is_some()).count();
        assert_eq!(headings, 4);
    }
}
