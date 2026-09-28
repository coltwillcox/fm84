use std::fs;
use std::io;
use std::path::PathBuf;

/// What the file rows are ordered by. Directories always come first, and the
/// order applies within the directories and within the files alike.
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
    pub confirm_delete: bool,
    pub theme: Theme,
    pub icon_style: IconStyle,
    pub tab_width: usize,
    /// The gutter in the viewer and the editor.
    pub line_numbers: bool,
    /// The editor highlights files up to this size, in KiB; zero never does.
    pub highlight_limit_kib: u64,
    /// The viewer and editor ask before loading a file past this, in MiB.
    pub large_file_mib: u64,
    /// Whether a picture opens filling the viewer rather than fitting inside it.
    pub image_fill: bool,
    /// The F9 command. Empty means pick one per platform, as before there was
    /// a choice; `{}` in it stands for the panel's directory.
    pub terminal: String,
}

impl Default for Options {
    // What fm84 did before any of this was configurable.
    fn default() -> Self {
        Self {
            show_hidden: true,
            sort_key: SortKey::Extension,
            sort_descending: false,
            confirm_delete: true,
            theme: Theme::Synthwave,
            icon_style: IconStyle::NerdFont,
            tab_width: 4,
            line_numbers: true,
            highlight_limit_kib: 512,
            large_file_mib: 64,
            image_fill: false,
            terminal: String::new(),
        }
    }
}

/// The rows of the options popup, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionRow {
    ShowHidden,
    SortKey,
    SortDirection,
    ConfirmDelete,
    Theme,
    IconStyle,
    TabWidth,
    LineNumbers,
    HighlightLimit,
    LargeFile,
    ImageDefault,
    Terminal,
}

pub const OPTION_ROWS: [OptionRow; 12] = [
    OptionRow::ShowHidden,
    OptionRow::SortKey,
    OptionRow::SortDirection,
    OptionRow::ConfirmDelete,
    OptionRow::Theme,
    OptionRow::IconStyle,
    OptionRow::TabWidth,
    OptionRow::LineNumbers,
    OptionRow::HighlightLimit,
    OptionRow::LargeFile,
    OptionRow::ImageDefault,
    OptionRow::Terminal,
];

impl OptionRow {
    pub fn label(self) -> &'static str {
        match self {
            OptionRow::ShowHidden => "Show hidden files",
            OptionRow::SortKey => "Sort by",
            OptionRow::SortDirection => "Sort direction",
            OptionRow::ConfirmDelete => "Confirm delete",
            OptionRow::Theme => "Theme",
            OptionRow::IconStyle => "Icons",
            OptionRow::TabWidth => "Tab width",
            OptionRow::LineNumbers => "Line numbers",
            OptionRow::HighlightLimit => "Highlight files up to",
            OptionRow::LargeFile => "Ask before opening over",
            OptionRow::ImageDefault => "Images open as",
            OptionRow::Terminal => "Terminal (F9)",
        }
    }

    /// True for a row edited by typing rather than by stepping through values.
    pub fn is_text(self) -> bool {
        self == OptionRow::Terminal
    }

    /// Whether changing this row changes what the panels list.
    pub fn affects_listing(self) -> bool {
        matches!(self, OptionRow::ShowHidden | OptionRow::SortKey | OptionRow::SortDirection)
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
            OptionRow::ConfirmDelete => on_off(self.confirm_delete),
            OptionRow::Theme => match self.theme {
                Theme::Synthwave => "Synthwave",
                Theme::Outrun => "Outrun",
                Theme::Vaporwave => "Vaporwave",
                Theme::HighContrast => "High contrast",
            }
            .to_string(),
            OptionRow::IconStyle => match self.icon_style {
                IconStyle::NerdFont => "Nerd Font",
                IconStyle::NerdFontMono => "Nerd Font Mono",
                IconStyle::Plain => "Plain",
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
            OptionRow::Terminal if self.terminal.is_empty() => "Automatic".to_string(),
            OptionRow::Terminal => self.terminal.clone(),
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
            OptionRow::ConfirmDelete => self.confirm_delete = !self.confirm_delete,
            OptionRow::Theme => {
                let themes = [Theme::Synthwave, Theme::Outrun, Theme::Vaporwave, Theme::HighContrast];
                self.theme = step(&themes, self.theme, forward);
            }
            OptionRow::IconStyle => {
                let styles = [IconStyle::NerdFont, IconStyle::NerdFontMono, IconStyle::Plain];
                self.icon_style = step(&styles, self.icon_style, forward);
            }
            OptionRow::TabWidth => self.tab_width = step(&TAB_WIDTHS, self.tab_width, forward),
            OptionRow::LineNumbers => self.line_numbers = !self.line_numbers,
            OptionRow::HighlightLimit => {
                self.highlight_limit_kib = step(&HIGHLIGHT_LIMITS_KIB, self.highlight_limit_kib, forward)
            }
            OptionRow::LargeFile => self.large_file_mib = step(&LARGE_FILE_LIMITS_MIB, self.large_file_mib, forward),
            OptionRow::ImageDefault => self.image_fill = !self.image_fill,
            OptionRow::Terminal => {}
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
        config_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = config_path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No config directory"))?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, self.serialize())
    }

    fn parse(text: &str) -> Self {
        let mut options = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            // A number in the given range, or None for anything else.
            let number = |range: std::ops::RangeInclusive<u64>| value.parse::<u64>().ok().filter(|n| range.contains(n));
            match key.trim() {
                "show_hidden" => options.show_hidden = parse_bool(value).unwrap_or(options.show_hidden),
                "sort" => {
                    options.sort_key = match value {
                        "name" => SortKey::Name,
                        "extension" => SortKey::Extension,
                        "size" => SortKey::Size,
                        "modified" => SortKey::Modified,
                        _ => options.sort_key,
                    }
                }
                "sort_descending" => options.sort_descending = parse_bool(value).unwrap_or(options.sort_descending),
                "confirm_delete" => options.confirm_delete = parse_bool(value).unwrap_or(options.confirm_delete),
                "theme" => {
                    options.theme = match value {
                        "synthwave" => Theme::Synthwave,
                        "outrun" => Theme::Outrun,
                        "vaporwave" => Theme::Vaporwave,
                        "high-contrast" => Theme::HighContrast,
                        _ => options.theme,
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
                "tab_width" => options.tab_width = number(1..=16).map_or(options.tab_width, |n| n as usize),
                "line_numbers" => options.line_numbers = parse_bool(value).unwrap_or(options.line_numbers),
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
                "terminal" => options.terminal = value.to_string(),
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
        let theme = match self.theme {
            Theme::Synthwave => "synthwave",
            Theme::Outrun => "outrun",
            Theme::Vaporwave => "vaporwave",
            Theme::HighContrast => "high-contrast",
        };
        let icons = match self.icon_style {
            IconStyle::NerdFont => "nerd-font",
            IconStyle::NerdFontMono => "nerd-font-mono",
            IconStyle::Plain => "plain",
        };
        format!(
            "# fm84 options, written by the F11 dialog.\n\
             show_hidden = {}\n\
             sort = {}\n\
             sort_descending = {}\n\
             confirm_delete = {}\n\
             theme = {}\n\
             icons = {}\n\
             tab_width = {}\n\
             line_numbers = {}\n\
             # Zero turns syntax highlighting off.\n\
             highlight_limit_kib = {}\n\
             large_file_mib = {}\n\
             image = {}\n\
             # Empty picks one automatically. {{}} stands for the directory.\n\
             terminal = {}\n",
            self.show_hidden,
            sort,
            self.sort_descending,
            self.confirm_delete,
            theme,
            icons,
            self.tab_width,
            self.line_numbers,
            self.highlight_limit_kib,
            self.large_file_mib,
            if self.image_fill { "fill" } else { "fit" },
            self.terminal
        )
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" | "on" | "yes" => Some(true),
        "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

/// `%APPDATA%\fm84\config` on Windows, `$XDG_CONFIG_HOME/fm84/config` or
/// `~/.config/fm84/config` everywhere else.
fn config_path() -> Option<PathBuf> {
    let non_empty = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        non_empty("APPDATA")?
    } else {
        non_empty("XDG_CONFIG_HOME").or_else(|| non_empty("HOME").map(|home| home.join(".config")))?
    };
    Some(base.join("fm84").join("config"))
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
            confirm_delete: false,
            theme: Theme::HighContrast,
            icon_style: IconStyle::Plain,
            tab_width: 8,
            line_numbers: false,
            highlight_limit_kib: 0,
            large_file_mib: 256,
            image_fill: true,
            terminal: "kitty --directory {}".to_string(),
        };
        assert_eq!(Options::parse(&options.serialize()), options);
        assert_eq!(Options::parse(&Options::default().serialize()), Options::default());
    }

    #[test]
    fn nonsense_leaves_the_default() {
        let options = Options::parse(
            "show_hidden = maybe\nsort = colour\nno equals sign\nunknown = 1\n\
             tab_width = 0\ntab_width = lots\nlarge_file_mib = 0\nhighlight_limit_kib = -5\ntheme = beige\n",
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
}
