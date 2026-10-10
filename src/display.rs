//! The settings that drawing reads everywhere, down in helpers that are never
//! handed the app state: which palette is showing and whether its background
//! is painted, how wide a tab is, and which units a size is written in.
//! `AppState.options` stays the one source of truth; `apply` copies these here
//! whenever it changes, so a draw never has to thread them through.

use crate::options::{Options, SizeUnits, Theme};
use ratatui::style::Color;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

/// Every colour the interface uses, one palette per theme.
pub struct Palette {
    /// Painted behind everything, unless F11 leaves the terminal's own.
    pub background: Color,
    /// Behind the line numbers in the viewer and the editor.
    pub gutter: Color,
    /// Dark text on a light background. Extension colours, the picture ramp
    /// and the syntax theme all turn around for it.
    pub light: bool,
    /// The syntect theme the viewer and editor highlight with.
    pub syntax_theme: &'static str,
    pub border: Color,
    pub columns: Color,
    pub directory: Color,
    pub directory_dark: Color,
    pub directory_bracket: Color,
    /// Files with no extension. The rest get a colour hashed from theirs.
    pub file: Color,
    pub rename_background: Color,
    pub selected_background: Color,
    pub selected_background_inactive: Color,
    /// On both selection backgrounds, and on the rename one.
    pub selected_foreground: Color,
    pub title: Color,
    pub selected_marker: Color,
}

const fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color::Rgb(red, green, blue)
}

const DARK_SYNTAX: &str = "base16-ocean.dark";
const LIGHT_SYNTAX: &str = "InspiredGitHub";

const SYNTHWAVE: Palette = Palette {
    background: rgb(0, 0, 0), // Black
    gutter: rgb(14, 7, 26),   // Violet black
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(116, 58, 213),                    // Violet
    columns: rgb(0, 255, 255),                    // Cyan
    directory: rgb(255, 0, 255),                  // Magenta
    directory_dark: rgb(150, 0, 150),             // Dark magenta
    directory_bracket: rgb(255, 0, 255),          // Magenta
    file: rgb(114, 137, 218),                     // Soft purple/blue
    rename_background: rgb(255, 0, 128),          // Hot pink
    selected_background: rgb(148, 0, 211),        // Purple
    selected_background_inactive: rgb(45, 0, 75), // Dark purple
    selected_foreground: rgb(0, 255, 255),        // Cyan
    title: rgb(242, 34, 255),                     // Purple
    selected_marker: rgb(255, 215, 0),            // Gold
};

// A sunset over the highway: orange and hot pink, with teal for contrast.
const OUTRUN: Palette = Palette {
    background: rgb(24, 10, 30),
    gutter: rgb(16, 6, 22),
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(255, 60, 120),                     // Hot pink
    columns: rgb(255, 190, 0),                     // Amber
    directory: rgb(255, 120, 40),                  // Orange
    directory_dark: rgb(160, 70, 30),              // Burnt orange
    directory_bracket: rgb(255, 120, 40),          // Orange
    file: rgb(120, 160, 255),                      // Dusk blue
    rename_background: rgb(255, 0, 80),            // Red pink
    selected_background: rgb(200, 30, 90),         // Magenta red
    selected_background_inactive: rgb(60, 10, 40), // Deep plum
    selected_foreground: rgb(255, 230, 120),       // Pale yellow
    title: rgb(255, 110, 60),                      // Sunset orange
    selected_marker: rgb(0, 255, 200),             // Teal
};

// Pastels: lavender, mint and pink.
const VAPORWAVE: Palette = Palette {
    background: rgb(32, 26, 52),
    gutter: rgb(24, 18, 40),
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(150, 120, 255),                    // Lavender
    columns: rgb(120, 255, 230),                   // Mint
    directory: rgb(255, 130, 220),                 // Pink
    directory_dark: rgb(150, 80, 130),             // Dusty pink
    directory_bracket: rgb(255, 130, 220),         // Pink
    file: rgb(180, 200, 255),                      // Pale blue
    rename_background: rgb(200, 50, 130),          // Deep bubblegum
    selected_background: rgb(110, 80, 200),        // Violet
    selected_background_inactive: rgb(40, 30, 70), // Midnight violet
    selected_foreground: rgb(255, 240, 250),       // Near white
    title: rgb(255, 160, 230),                     // Light pink
    selected_marker: rgb(255, 230, 120),           // Butter
};

// Plain and bright, for a washed-out screen or eyes that need the contrast.
const HIGH_CONTRAST: Palette = Palette {
    background: rgb(0, 0, 0),
    gutter: rgb(24, 24, 24),
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(255, 255, 255),
    columns: rgb(255, 255, 0),
    directory: rgb(0, 255, 255),
    directory_dark: rgb(0, 170, 170),
    directory_bracket: rgb(0, 255, 255),
    file: rgb(235, 235, 235),
    rename_background: rgb(200, 0, 0),
    selected_background: rgb(0, 80, 220),
    selected_background_inactive: rgb(70, 70, 70),
    selected_foreground: rgb(255, 255, 255),
    title: rgb(255, 255, 255),
    selected_marker: rgb(255, 255, 0),
};

// The schemes below take their colours from each one's published palette.

const DRACULA: Palette = Palette {
    background: rgb(40, 42, 54), // Background
    gutter: rgb(33, 34, 44),     // Darker background
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(98, 114, 164),                     // Comment
    columns: rgb(139, 233, 253),                   // Cyan
    directory: rgb(189, 147, 249),                 // Purple
    directory_dark: rgb(98, 114, 164),             // Comment
    directory_bracket: rgb(189, 147, 249),         // Purple
    file: rgb(248, 248, 242),                      // Foreground
    rename_background: rgb(200, 50, 60),           // Red, darkened for the text on it
    selected_background: rgb(98, 114, 164),        // Comment
    selected_background_inactive: rgb(68, 71, 90), // Current line
    selected_foreground: rgb(248, 248, 242),       // Foreground
    title: rgb(255, 121, 198),                     // Pink
    selected_marker: rgb(241, 250, 140),           // Yellow
};

const MONOKAI: Palette = Palette {
    background: rgb(39, 40, 34),
    gutter: rgb(30, 31, 27),
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(117, 113, 94),    // Comment
    columns: rgb(102, 217, 239),  // Blue
    directory: rgb(166, 226, 46), // Green
    directory_dark: rgb(110, 140, 40),
    directory_bracket: rgb(166, 226, 46), // Green
    file: rgb(248, 248, 242),             // Foreground
    rename_background: rgb(190, 20, 85),  // Pink, darkened for the text on it
    selected_background: rgb(90, 88, 72),
    selected_background_inactive: rgb(62, 61, 50), // Line highlight
    selected_foreground: rgb(230, 219, 116),       // Yellow
    title: rgb(249, 38, 114),                      // Pink
    selected_marker: rgb(253, 151, 31),            // Orange
};

const NORD: Palette = Palette {
    background: rgb(46, 52, 64), // Polar night 0
    gutter: rgb(41, 46, 57),
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(76, 86, 106),      // Polar night 3
    columns: rgb(136, 192, 208),   // Frost 1
    directory: rgb(129, 161, 193), // Frost 2
    directory_dark: rgb(94, 110, 140),
    directory_bracket: rgb(129, 161, 193),         // Frost 2
    file: rgb(216, 222, 233),                      // Snow storm 0
    rename_background: rgb(191, 97, 106),          // Aurora red
    selected_background: rgb(94, 129, 172),        // Frost 3
    selected_background_inactive: rgb(59, 66, 82), // Polar night 1
    selected_foreground: rgb(236, 239, 244),       // Snow storm 2
    title: rgb(143, 188, 187),                     // Frost 0
    selected_marker: rgb(235, 203, 139),           // Aurora yellow
};

const GRUVBOX_DARK: Palette = Palette {
    background: rgb(40, 40, 40), // bg
    gutter: rgb(29, 32, 33),     // bg0_h
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(146, 131, 116),                    // Gray
    columns: rgb(142, 192, 124),                   // Aqua
    directory: rgb(250, 189, 47),                  // Yellow
    directory_dark: rgb(181, 118, 20),             // Faded yellow
    directory_bracket: rgb(250, 189, 47),          // Yellow
    file: rgb(235, 219, 178),                      // fg
    rename_background: rgb(204, 36, 29),           // Red
    selected_background: rgb(102, 92, 84),         // bg3
    selected_background_inactive: rgb(60, 56, 54), // bg1
    selected_foreground: rgb(251, 241, 199),       // fg0
    title: rgb(254, 128, 25),                      // Orange
    selected_marker: rgb(184, 187, 38),            // Green
};

const SOLARIZED_DARK: Palette = Palette {
    background: rgb(0, 43, 54), // base03
    gutter: rgb(0, 36, 46),
    light: false,
    syntax_theme: "Solarized (dark)",
    border: rgb(88, 110, 117),    // base01
    columns: rgb(42, 161, 152),   // Cyan
    directory: rgb(38, 139, 210), // Blue
    directory_dark: rgb(30, 95, 140),
    directory_bracket: rgb(38, 139, 210),         // Blue
    file: rgb(131, 148, 150),                     // base0
    rename_background: rgb(220, 50, 47),          // Red
    selected_background: rgb(88, 110, 117),       // base01
    selected_background_inactive: rgb(7, 54, 66), // base02
    selected_foreground: rgb(253, 246, 227),      // base3
    title: rgb(211, 54, 130),                     // Magenta
    selected_marker: rgb(181, 137, 0),            // Yellow
};

const TOKYO_NIGHT: Palette = Palette {
    background: rgb(26, 27, 38),
    gutter: rgb(22, 22, 30),
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(86, 95, 137),      // Comment
    columns: rgb(125, 207, 255),   // Cyan
    directory: rgb(122, 162, 247), // Blue
    directory_dark: rgb(70, 90, 150),
    directory_bracket: rgb(122, 162, 247),         // Blue
    file: rgb(192, 202, 245),                      // Foreground
    rename_background: rgb(170, 50, 75),           // Red, darkened for the text on it
    selected_background: rgb(51, 70, 124),         // Selection
    selected_background_inactive: rgb(41, 46, 66), // Highlight
    selected_foreground: rgb(192, 202, 245),       // Foreground
    title: rgb(187, 154, 247),                     // Magenta
    selected_marker: rgb(224, 175, 104),           // Yellow
};

const CATPPUCCIN_MOCHA: Palette = Palette {
    background: rgb(30, 30, 46), // Base
    gutter: rgb(24, 24, 37),     // Mantle
    light: false,
    syntax_theme: DARK_SYNTAX,
    border: rgb(108, 112, 134),    // Overlay 0
    columns: rgb(148, 226, 213),   // Teal
    directory: rgb(137, 180, 250), // Blue
    directory_dark: rgb(90, 120, 170),
    directory_bracket: rgb(137, 180, 250),         // Blue
    file: rgb(205, 214, 244),                      // Text
    rename_background: rgb(170, 60, 90),           // Red, darkened for the text on it
    selected_background: rgb(88, 91, 112),         // Surface 2
    selected_background_inactive: rgb(49, 50, 68), // Surface 0
    selected_foreground: rgb(205, 214, 244),       // Text
    title: rgb(203, 166, 247),                     // Mauve
    selected_marker: rgb(249, 226, 175),           // Yellow
};

// The light schemes select with a pale tint and dark text, since the
// foreground is shared by both selection backgrounds and the rename one.

const SOLARIZED_LIGHT: Palette = Palette {
    background: rgb(253, 246, 227), // base3
    gutter: rgb(238, 232, 213),     // base2
    light: true,
    syntax_theme: "Solarized (light)",
    border: rgb(147, 161, 161),   // base1
    columns: rgb(30, 125, 118),   // Cyan, darkened for the pale background
    directory: rgb(38, 139, 210), // Blue
    directory_dark: rgb(110, 150, 180),
    directory_bracket: rgb(38, 139, 210), // Blue
    file: rgb(101, 123, 131),             // base00
    rename_background: rgb(246, 196, 190),
    selected_background: rgb(197, 222, 240),
    selected_background_inactive: rgb(238, 232, 213), // base2
    selected_foreground: rgb(7, 54, 66),              // base02
    title: rgb(211, 54, 130),                         // Magenta
    selected_marker: rgb(203, 75, 22),                // Orange
};

const GRUVBOX_LIGHT: Palette = Palette {
    background: rgb(251, 241, 199), // bg
    gutter: rgb(242, 229, 188),     // bg0_s
    light: true,
    syntax_theme: "base16-ocean.light",
    border: rgb(146, 131, 116),  // Gray
    columns: rgb(66, 123, 88),   // Aqua
    directory: rgb(7, 102, 120), // Blue
    directory_dark: rgb(100, 140, 150),
    directory_bracket: rgb(7, 102, 120), // Blue
    file: rgb(60, 56, 54),               // fg
    rename_background: rgb(240, 180, 160),
    selected_background: rgb(213, 196, 161),          // bg2
    selected_background_inactive: rgb(235, 219, 178), // bg1
    selected_foreground: rgb(40, 40, 40),             // fg0
    title: rgb(175, 58, 3),                           // Orange
    selected_marker: rgb(143, 63, 113),               // Purple
};

const CATPPUCCIN_LATTE: Palette = Palette {
    background: rgb(239, 241, 245), // Base
    gutter: rgb(230, 233, 239),     // Mantle
    light: true,
    syntax_theme: LIGHT_SYNTAX,
    border: rgb(156, 160, 176),   // Overlay 0
    columns: rgb(23, 146, 153),   // Teal
    directory: rgb(30, 102, 245), // Blue
    directory_dark: rgb(120, 150, 220),
    directory_bracket: rgb(30, 102, 245), // Blue
    file: rgb(76, 79, 105),               // Text
    rename_background: rgb(240, 180, 195),
    selected_background: rgb(188, 192, 204),          // Surface 1
    selected_background_inactive: rgb(220, 224, 232), // Crust
    selected_foreground: rgb(76, 79, 105),            // Text
    title: rgb(136, 57, 239),                         // Mauve
    selected_marker: rgb(254, 100, 11),               // Peach
};

const GITHUB_LIGHT: Palette = Palette {
    background: rgb(255, 255, 255),
    gutter: rgb(246, 248, 250),
    light: true,
    syntax_theme: LIGHT_SYNTAX,
    border: rgb(175, 184, 193),
    columns: rgb(26, 127, 55),   // Green
    directory: rgb(9, 105, 218), // Blue
    directory_dark: rgb(84, 140, 210),
    directory_bracket: rgb(9, 105, 218), // Blue
    file: rgb(31, 35, 40),               // Foreground
    rename_background: rgb(255, 210, 210),
    selected_background: rgb(200, 225, 255),
    selected_background_inactive: rgb(234, 238, 242),
    selected_foreground: rgb(31, 35, 40), // Foreground
    title: rgb(130, 80, 223),             // Purple
    selected_marker: rgb(188, 76, 0),     // Orange
};

pub fn palette_of(theme: Theme) -> &'static Palette {
    match theme {
        Theme::Synthwave => &SYNTHWAVE,
        Theme::Outrun => &OUTRUN,
        Theme::Vaporwave => &VAPORWAVE,
        Theme::HighContrast => &HIGH_CONTRAST,
        Theme::Dracula => &DRACULA,
        Theme::Monokai => &MONOKAI,
        Theme::Nord => &NORD,
        Theme::GruvboxDark => &GRUVBOX_DARK,
        Theme::SolarizedDark => &SOLARIZED_DARK,
        Theme::TokyoNight => &TOKYO_NIGHT,
        Theme::CatppuccinMocha => &CATPPUCCIN_MOCHA,
        Theme::SolarizedLight => &SOLARIZED_LIGHT,
        Theme::GruvboxLight => &GRUVBOX_LIGHT,
        Theme::CatppuccinLatte => &CATPPUCCIN_LATTE,
        Theme::GithubLight => &GITHUB_LIGHT,
    }
}

/// Stored as a position in `Theme::ALL`.
static THEME: AtomicU8 = AtomicU8::new(0);
static THEME_BACKGROUND: AtomicBool = AtomicBool::new(true);
static TAB_WIDTH: AtomicUsize = AtomicUsize::new(4);
static DECIMAL_SIZES: AtomicBool = AtomicBool::new(false);

/// Take up the options that drawing reads. Called at startup and after every
/// change in the options dialog.
pub fn apply(options: &Options) {
    let index = Theme::ALL.iter().position(|&theme| theme == options.theme).unwrap_or(0);
    THEME.store(index as u8, Ordering::Relaxed);
    THEME_BACKGROUND.store(options.theme_background, Ordering::Relaxed);
    TAB_WIDTH.store(options.tab_width, Ordering::Relaxed);
    DECIMAL_SIZES.store(options.size_units == SizeUnits::Decimal, Ordering::Relaxed);
}

pub fn palette() -> &'static Palette {
    let index = THEME.load(Ordering::Relaxed) as usize;
    palette_of(Theme::ALL.get(index).copied().unwrap_or(Theme::Synthwave))
}

/// The colour to paint behind everything, or None to leave the terminal's.
pub fn background() -> Option<Color> {
    THEME_BACKGROUND.load(Ordering::Relaxed).then(|| palette().background)
}

/// Whether what is drawn sits on a light background: a light theme, with its
/// background painted. Left on the terminal's, fm84 cannot know how light that
/// is, and keeps to the dark-background choices it has always made.
pub fn light_background() -> bool {
    background().is_some() && palette().light
}

/// Behind the line numbers. With the terminal's background showing it stays
/// black, as it always was; the theme's own gutter is picked to sit on the
/// theme's own background, and could clash with anything else.
pub fn gutter() -> Color {
    if THEME_BACKGROUND.load(Ordering::Relaxed) { palette().gutter } else { Color::Black }
}

/// How many columns a tab expands to.
pub fn tab_width() -> usize {
    TAB_WIDTH.load(Ordering::Relaxed)
}

/// Sizes in powers of 1000 rather than 1024.
pub fn decimal_sizes() -> bool {
    DECIMAL_SIZES.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels(color: Color) -> (f64, f64, f64) {
        match color {
            Color::Rgb(red, green, blue) => (red as f64, green as f64, blue as f64),
            other => panic!("palettes are all RGB, not {other:?}"),
        }
    }

    /// WCAG relative luminance, 0 for black to 1 for white.
    fn luminance(color: Color) -> f64 {
        let linear = |channel: f64| {
            let c = channel / 255.0;
            if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        let (red, green, blue) = channels(color);
        0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn every_theme_can_be_read() {
        let mut failures = Vec::new();
        for theme in Theme::ALL {
            let palette = palette_of(theme);
            let name = theme.label();
            assert_eq!(palette.light, luminance(palette.background) > 0.5, "{name}: light flag");
            // Text on the background, and the selected row's text on each of
            // the backgrounds it is drawn on. 3:1 is WCAG's floor for large
            // text; file names are short, bold when selected, and often
            // coloured, so this is the least they can have.
            for (what, fg, bg) in [
                ("file", palette.file, palette.background),
                ("directory", palette.directory, palette.background),
                ("columns", palette.columns, palette.background),
                ("title", palette.title, palette.background),
                ("cursor", palette.selected_foreground, palette.selected_background),
                ("inactive cursor", palette.selected_foreground, palette.selected_background_inactive),
                ("rename", palette.selected_foreground, palette.rename_background),
            ] {
                let ratio = contrast(fg, bg);
                if ratio < 3.0 {
                    failures.push(format!("{name}: {what} contrast {ratio:.2}"));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn extension_colours_read_on_every_background() {
        // Enough extensions to land on every hue the hash can give.
        let extensions: Vec<String> = (b'a'..=b'z').flat_map(|a| (b'a'..=b'z').map(move |b| format!("{}{}", a as char, b as char))).collect();
        let mut worst = (f64::MAX, String::new());
        for theme in Theme::ALL {
            let palette = palette_of(theme);
            for extension in &extensions {
                let ratio = contrast(crate::utils::extension_color(extension, palette.light), palette.background);
                if ratio < worst.0 {
                    worst = (ratio, format!("{} .{}", theme.label(), extension));
                }
            }
        }
        assert!(worst.0 >= 3.0, "{}: contrast {:.2}", worst.1, worst.0);
    }

    #[test]
    fn every_syntax_theme_exists() {
        let themes = syntect::highlighting::ThemeSet::load_defaults();
        for theme in Theme::ALL {
            let name = palette_of(theme).syntax_theme;
            assert!(themes.themes.contains_key(name), "{}: no syntect theme {name}", theme.label());
        }
    }
}
