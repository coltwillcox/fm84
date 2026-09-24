use ratatui::style::Color;
use std::time::Duration;

// How often to check whether a panel's directory changed on disk.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub const TITLE: &str = "File Manager '84";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const ICON_FOLDER: &str = " "; // Added space after icon is workaround for small icons bug in table columns
pub const ICON_FILE: &str = " ";
pub const ICON_LOGO: &str = " ";

// Drive icons for the mount strip, same Nerd Font range as the icons above.
pub const ICON_DRIVE: &str = "";
pub const ICON_HOME: &str = "";
pub const ICON_REMOVABLE: &str = "";
pub const ICON_NETWORK: &str = "";
pub const ICON_OPTICAL: &str = "";

// Default synthwave color palette
pub const COLOR_BORDER: Color = Color::Rgb(116, 58, 213);                    // Violet
pub const COLOR_COLUMNS: Color = Color::Rgb(0, 255, 255);                    // Cyan
pub const COLOR_DIRECTORY: Color = Color::Rgb(255, 0, 255);                  // Magenta
pub const COLOR_DIRECTORY_DARK: Color = Color::Rgb(150, 0, 150);             // Dark magenta
pub const COLOR_DIRECTORY_FIX: Color = Color::Rgb(255, 0, 255);              // Magenta
pub const COLOR_FILE: Color = Color::Rgb(114, 137, 218);                     // Soft purple/blue
pub const COLOR_RENAME_BACKGROUND: Color = Color::Rgb(255, 0, 128);          // Hot pink
pub const COLOR_SELECTED_BACKGROUND: Color = Color::Rgb(148, 0, 211);        // Purple
pub const COLOR_SELECTED_BACKGROUND_INACTIVE: Color = Color::Rgb(45, 0, 75); // Dark purple
pub const COLOR_SELECTED_FOREGROUND: Color = Color::Rgb(0, 255, 255);        // Cyan
pub const COLOR_TITLE: Color = Color::Rgb(242, 34, 255);                     // Purple
pub const COLOR_SELECTED_MARKER: Color = Color::Rgb(255, 215, 0);            // Gold/Yellow for selection marker

// Past this the viewer and editor ask before pulling the file into memory.
pub const LARGE_FILE_SIZE: u64 = 64 * 1024 * 1024;
// Past this the editor opens the file without syntax highlighting. The initial
// parse measures about 0.9s per megabyte, so this caps the wait at roughly half
// a second; editing stays fast afterwards regardless of length.
pub const MAX_HIGHLIGHT_SIZE: u64 = 512 * 1024;

// A preview reloads every time the cursor moves, so it only ever reads the head
// of a file - never the whole thing, and never with the large-file prompt.
// Terminals commonly reject oversized OSC 52 payloads, and a megabyte of
// base64 is not worth sending anyway; the internal clipboard still holds it.
// Editing steps kept for undo. Each holds only the lines it replaced, so this
// is bounded by what was edited rather than by the size of the file.
pub const UNDO_LIMIT: usize = 200;

pub const OSC52_MAX_BYTES: usize = 64 * 1024;

// A hexdump -C line: offset, sixteen bytes, then the ASCII gutter.
pub const HEX_BYTES_PER_LINE: usize = 16;
pub const HEX_LINE_WIDTH: usize = 78;

// Images are kept downscaled to this many pixels on the longer side. Wider than
// any terminal is columns, so refitting on resize never works from the original.
pub const IMAGE_MAX_SIDE: u32 = 1024;
// Characters from light to dense, for a light-on-dark terminal.
pub const IMAGE_RAMP: &[u8] = b" .:-=+*#%@";

pub const PREVIEW_MAX_BYTES: u64 = 64 * 1024;
pub const PREVIEW_MAX_LINES: usize = 500;

pub const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
pub const TAB_SPACES: &str = "    ";
