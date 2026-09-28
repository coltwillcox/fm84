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

/// Everything F11 can change, as it is kept in the config file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Options {
    pub show_hidden: bool,
    pub sort_key: SortKey,
    pub sort_descending: bool,
    pub confirm_delete: bool,
    pub icon_style: IconStyle,
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
            icon_style: IconStyle::NerdFont,
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
    IconStyle,
    Terminal,
}

pub const OPTION_ROWS: [OptionRow; 6] = [
    OptionRow::ShowHidden,
    OptionRow::SortKey,
    OptionRow::SortDirection,
    OptionRow::ConfirmDelete,
    OptionRow::IconStyle,
    OptionRow::Terminal,
];

impl OptionRow {
    pub fn label(self) -> &'static str {
        match self {
            OptionRow::ShowHidden => "Show hidden files",
            OptionRow::SortKey => "Sort by",
            OptionRow::SortDirection => "Sort direction",
            OptionRow::ConfirmDelete => "Confirm delete",
            OptionRow::IconStyle => "Icons",
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
            OptionRow::IconStyle => match self.icon_style {
                IconStyle::NerdFont => "Nerd Font",
                IconStyle::NerdFontMono => "Nerd Font Mono",
                IconStyle::Plain => "Plain",
            }
            .to_string(),
            OptionRow::Terminal if self.terminal.is_empty() => "Automatic".to_string(),
            OptionRow::Terminal => self.terminal.clone(),
        }
    }

    /// Step a row to its next value, or its previous one. A text row does not
    /// step; it is typed into instead.
    pub fn cycle(&mut self, row: OptionRow, forward: bool) {
        fn step<T: Copy + PartialEq>(values: &[T], current: T, forward: bool) -> T {
            let index = values.iter().position(|&value| value == current).unwrap_or(0);
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
            OptionRow::IconStyle => {
                let styles = [IconStyle::NerdFont, IconStyle::NerdFontMono, IconStyle::Plain];
                self.icon_style = step(&styles, self.icon_style, forward);
            }
            OptionRow::Terminal => {}
        }
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
                "icons" => {
                    options.icon_style = match value {
                        "nerd-font" => IconStyle::NerdFont,
                        "nerd-font-mono" => IconStyle::NerdFontMono,
                        "plain" => IconStyle::Plain,
                        _ => options.icon_style,
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
             icons = {}\n\
             # Empty picks one automatically. {{}} stands for the directory.\n\
             terminal = {}\n",
            self.show_hidden, sort, self.sort_descending, self.confirm_delete, icons, self.terminal
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
            icon_style: IconStyle::Plain,
            terminal: "kitty --directory {}".to_string(),
        };
        assert_eq!(Options::parse(&options.serialize()), options);
        assert_eq!(Options::parse(&Options::default().serialize()), Options::default());
    }

    #[test]
    fn nonsense_leaves_the_default() {
        let options = Options::parse("show_hidden = maybe\nsort = colour\nno equals sign\nunknown = 1\n");
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
    }
}
