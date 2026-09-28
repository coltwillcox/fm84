//! The settings that drawing reads everywhere, down in helpers that are never
//! handed the app state: which palette is showing, and how wide a tab is.
//! `AppState.options` stays the one source of truth; `apply` copies these two
//! here whenever it changes, so a draw never has to thread them through.

use crate::options::{Options, Theme};
use ratatui::style::Color;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// Every colour the interface uses, one palette per theme.
pub struct Palette {
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
    pub selected_foreground: Color,
    pub title: Color,
    pub selected_marker: Color,
}

const SYNTHWAVE: Palette = Palette {
    border: Color::Rgb(116, 58, 213),                   // Violet
    columns: Color::Rgb(0, 255, 255),                   // Cyan
    directory: Color::Rgb(255, 0, 255),                 // Magenta
    directory_dark: Color::Rgb(150, 0, 150),            // Dark magenta
    directory_bracket: Color::Rgb(255, 0, 255),         // Magenta
    file: Color::Rgb(114, 137, 218),                    // Soft purple/blue
    rename_background: Color::Rgb(255, 0, 128),         // Hot pink
    selected_background: Color::Rgb(148, 0, 211),       // Purple
    selected_background_inactive: Color::Rgb(45, 0, 75), // Dark purple
    selected_foreground: Color::Rgb(0, 255, 255),       // Cyan
    title: Color::Rgb(242, 34, 255),                    // Purple
    selected_marker: Color::Rgb(255, 215, 0),           // Gold
};

// A sunset over the highway: orange and hot pink, with teal for contrast.
const OUTRUN: Palette = Palette {
    border: Color::Rgb(255, 60, 120),                    // Hot pink
    columns: Color::Rgb(255, 190, 0),                    // Amber
    directory: Color::Rgb(255, 120, 40),                 // Orange
    directory_dark: Color::Rgb(160, 70, 30),             // Burnt orange
    directory_bracket: Color::Rgb(255, 120, 40),         // Orange
    file: Color::Rgb(120, 160, 255),                     // Dusk blue
    rename_background: Color::Rgb(255, 0, 80),           // Red pink
    selected_background: Color::Rgb(200, 30, 90),        // Magenta red
    selected_background_inactive: Color::Rgb(60, 10, 40), // Deep plum
    selected_foreground: Color::Rgb(255, 230, 120),      // Pale yellow
    title: Color::Rgb(255, 110, 60),                     // Sunset orange
    selected_marker: Color::Rgb(0, 255, 200),            // Teal
};

// Pastels: lavender, mint and pink.
const VAPORWAVE: Palette = Palette {
    border: Color::Rgb(150, 120, 255),                   // Lavender
    columns: Color::Rgb(120, 255, 230),                  // Mint
    directory: Color::Rgb(255, 130, 220),                // Pink
    directory_dark: Color::Rgb(150, 80, 130),            // Dusty pink
    directory_bracket: Color::Rgb(255, 130, 220),        // Pink
    file: Color::Rgb(180, 200, 255),                     // Pale blue
    rename_background: Color::Rgb(255, 100, 180),        // Bubblegum
    selected_background: Color::Rgb(110, 80, 200),       // Violet
    selected_background_inactive: Color::Rgb(40, 30, 70), // Midnight violet
    selected_foreground: Color::Rgb(255, 240, 250),      // Near white
    title: Color::Rgb(255, 160, 230),                    // Light pink
    selected_marker: Color::Rgb(255, 230, 120),          // Butter
};

// Plain and bright, for a washed-out screen or eyes that need the contrast.
const HIGH_CONTRAST: Palette = Palette {
    border: Color::Rgb(255, 255, 255),
    columns: Color::Rgb(255, 255, 0),
    directory: Color::Rgb(0, 255, 255),
    directory_dark: Color::Rgb(0, 170, 170),
    directory_bracket: Color::Rgb(0, 255, 255),
    file: Color::Rgb(235, 235, 235),
    rename_background: Color::Rgb(200, 0, 0),
    selected_background: Color::Rgb(0, 80, 220),
    selected_background_inactive: Color::Rgb(70, 70, 70),
    selected_foreground: Color::Rgb(255, 255, 255),
    title: Color::Rgb(255, 255, 255),
    selected_marker: Color::Rgb(255, 255, 0),
};

static THEME: AtomicU8 = AtomicU8::new(0);
static TAB_WIDTH: AtomicUsize = AtomicUsize::new(4);

/// Take up the options that drawing reads. Called at startup and after every
/// change in the options dialog.
pub fn apply(options: &Options) {
    THEME.store(options.theme as u8, Ordering::Relaxed);
    TAB_WIDTH.store(options.tab_width, Ordering::Relaxed);
}

pub fn palette() -> &'static Palette {
    match THEME.load(Ordering::Relaxed) {
        x if x == Theme::Outrun as u8 => &OUTRUN,
        x if x == Theme::Vaporwave as u8 => &VAPORWAVE,
        x if x == Theme::HighContrast as u8 => &HIGH_CONTRAST,
        _ => &SYNTHWAVE,
    }
}

/// How many columns a tab expands to.
pub fn tab_width() -> usize {
    TAB_WIDTH.load(Ordering::Relaxed)
}
