//! Everything F11 can change, and the files fm84 keeps between runs.

use std::fs;
use std::io;
use std::path::PathBuf;

mod config;
mod rows;
mod session;
#[cfg(test)]
mod tests;

pub use self::{config::*, rows::*, session::*};

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
    Military,
    Picard,
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
    pub const ALL: [Theme; 17] = [
        Theme::Synthwave,
        Theme::Outrun,
        Theme::Vaporwave,
        Theme::HighContrast,
        Theme::Military,
        Theme::Picard,
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
            Theme::Military => "Military",
            Theme::Picard => "Picard",
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
            Theme::Military => "military",
            Theme::Picard => "picard",
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
    /// Keep what is there, and write each file beside it under a name of
    /// its own.
    KeepBoth,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clock {
    Hours24,
    Hours12,
    Off,
}

/// The cursor in a field being typed into and in the editor. All but Classic
/// are the terminal's own cursor, in that shape; Classic is a cell drawn in
/// reverse, for a terminal that cannot be told a shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorShape {
    Bar,
    Block,
    Underline,
    Classic,
}

/// What the thumbs of the viewer's and the editor's scroll bars are drawn
/// with. Heavy is the border's own line drawn thicker; ASCII is for a font
/// without box drawing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollBarStyle {
    Heavy,
    Double,
    Dashed,
    Block,
    Shade,
    Ascii,
}

impl ScrollBarStyle {
    pub const ALL: [ScrollBarStyle; 6] = [ScrollBarStyle::Heavy, ScrollBarStyle::Double, ScrollBarStyle::Dashed, ScrollBarStyle::Block, ScrollBarStyle::Shade, ScrollBarStyle::Ascii];

    /// The thumb of a bar down the side, and of one along the bottom. All one
    /// column wide in every terminal: none of East Asian ambiguous width.
    pub fn thumbs(self) -> (&'static str, &'static str) {
        match self {
            ScrollBarStyle::Heavy => ("┃", "━"),
            ScrollBarStyle::Double => ("║", "═"),
            ScrollBarStyle::Dashed => ("╏", "╍"),
            ScrollBarStyle::Block => ("█", "█"),
            ScrollBarStyle::Shade => ("▓", "▓"),
            ScrollBarStyle::Ascii => ("#", "#"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ScrollBarStyle::Heavy => "Heavy",
            ScrollBarStyle::Double => "Double",
            ScrollBarStyle::Dashed => "Dashed",
            ScrollBarStyle::Block => "Block",
            ScrollBarStyle::Shade => "Shade",
            ScrollBarStyle::Ascii => "ASCII",
        }
    }

    /// How the config file names it.
    pub(super) fn key(self) -> &'static str {
        match self {
            ScrollBarStyle::Heavy => "heavy",
            ScrollBarStyle::Double => "double",
            ScrollBarStyle::Dashed => "dashed",
            ScrollBarStyle::Block => "block",
            ScrollBarStyle::Shade => "shade",
            ScrollBarStyle::Ascii => "ascii",
        }
    }
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
    /// Mark what git says of each entry - modified, staged, untracked,
    /// ignored, in conflict - at the end of its name.
    pub git_marks: bool,
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
    pub cursor: CursorShape,
    /// Whether the terminal's cursor blinks; nothing to Classic.
    pub cursor_blink: bool,
    pub tab_width: usize,
    /// The gutter in the viewer and the editor.
    pub line_numbers: bool,
    /// What the scroll bars' thumbs are drawn with.
    pub scroll_bar: ScrollBarStyle,
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
            git_marks: true,
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
            cursor: CursorShape::Bar,
            cursor_blink: true,
            tab_width: 4,
            line_numbers: true,
            scroll_bar: ScrollBarStyle::Heavy,
            highlight_limit_kib: 512,
            large_file_mib: 64,
            image_fill: false,
            image_backgrounds: true,
            image_prefetch: true,
        }
    }
}

impl Options {
    pub fn highlight_limit_bytes(&self) -> u64 {
        self.highlight_limit_kib * 1024
    }

    pub fn large_file_bytes(&self) -> u64 {
        self.large_file_mib * 1024 * 1024
    }
}
