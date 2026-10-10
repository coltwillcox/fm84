use std::time::Duration;

// How often to check whether a panel's directory changed on disk.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
// How long the UI waits on a directory being read before going on without it,
// leaving the panel as it was and saying it is still reading. A disk that
// answers lists even a large directory well inside this; only one that has
// stopped answering runs past it.
pub const LISTING_WAIT: Duration = Duration::from_millis(150);
// The same for the detail lines and the preview, which follow the cursor and
// so are asked for on every key: short enough that holding an arrow key over a
// dead mount still moves, long enough that a live one shows them at once.
pub const DETAIL_WAIT: Duration = Duration::from_millis(30);

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

// Editing steps kept for undo. Each holds only the lines it replaced, so this
// is bounded by what was edited rather than by the size of the file.
pub const UNDO_LIMIT: usize = 200;

// Terminals commonly reject oversized OSC 52 payloads, and a megabyte of
// base64 is not worth sending anyway; the internal clipboard still holds it.
pub const OSC52_MAX_BYTES: usize = 64 * 1024;

// A hexdump -C line: offset, sixteen bytes, then the ASCII gutter.
pub const HEX_BYTES_PER_LINE: usize = 16;
pub const HEX_LINE_WIDTH: usize = 78;

// Images are kept downscaled to this many pixels on the longer side. Wider than
// any terminal is columns, so refitting on resize never works from the original.
pub const IMAGE_MAX_SIDE: u32 = 1024;
// Characters from light to dense, for a light-on-dark terminal.
pub const IMAGE_RAMP: &[u8] = b" .:-=+*#%@";
// Low bits dropped from each colour channel, so neighbouring characters share
// a colour and one escape sequence. Three cuts a full-screen image frame from
// about 170 KB to 110 KB with no visible banding; four starts to show it.
pub const IMAGE_COLOR_DROP_BITS: u32 = 3;
// How far a filled image may overflow the viewer, in screenfuls each way. An
// extreme aspect ratio would otherwise draw hundreds of thousands of rows -
// slow to build, large to hold, and showing nothing the first few screens do
// not - and would rebuild them on every resize.
pub const IMAGE_MAX_OVERFLOW: usize = 4;
// Past this the viewer asks before decoding a picture. Measured in pixels, not
// file size: a few hundred KB of PNG can unpack to hundreds of MB. Set below
// the decoder's own 512 MB ceiling, so the question is asked while there is
// still an answer - beyond that it refuses outright and the file opens as hex.
pub const IMAGE_MAX_DECODED: u64 = 256 * 1024 * 1024;
// How many times taller a terminal cell is than it is wide, for a terminal that
// will not say. A picture's proportions are kept by drawing half as many rows as
// columns, so the figure has to come from somewhere; 2 is what an ordinary
// monospace cell comes to, and what fm84 assumed for every terminal until it
// learned to ask.
pub const CELL_ASPECT_FALLBACK: f64 = 2.0;
// What a reported cell ratio has to fall inside to be believed. Outside it the
// terminal is reporting something other than a cell - a tmux pane's pixel size
// is its whole window's, and some report the window in place of the cell - and
// a picture drawn to it would be worse than one drawn to the assumption.
pub const CELL_ASPECT_RANGE: std::ops::RangeInclusive<f64> = 1.0..=4.0;
// The sizes + and - step through, as a percentage of the Fit or Fill size.
// A ladder rather than a constant factor, so the figure on the status bar is
// always a round one. The top of it is IMAGE_MAX_OVERFLOW, which is where the
// drawing is capped anyway - a step past that would change nothing on screen.
pub const IMAGE_ZOOM_STEPS: [u16; 8] = [25, 50, 75, 100, 150, 200, 300, 400];
pub const IMAGE_ZOOM_NORMAL: u16 = 100;
// How far a character is pulled away from the colour behind it, when a picture
// is drawn with backgrounds. Enough to read the glyph against its own cell
// without washing the colour out.
pub const IMAGE_GLYPH_CONTRAST: f64 = 0.45;

// Bytes copied between progress reports. io::copy over a reader limited to this
// keeps whatever fast copy path the platform has - measured at 2750 MiB/s
// against 2346 for a hand-rolled buffer of the same size - while still coming
// up for air often enough to move the bar and notice a cancel. On slow media
// that is an update every 30ms or so; a larger chunk measured no faster and
// would only coarsen both.
pub const COPY_CHUNK: u64 = 1024 * 1024;
// A transfer that finishes inside this shows no progress popup at all. Most
// copies are small and a popup that appears and vanishes reads as a glitch.
pub const TRANSFER_POPUP_DELAY: Duration = Duration::from_millis(150);
// How often a directory being sized reports its running total. A walk finds
// thousands of files a second, and a message for each would only be thrown away.
pub const SIZE_PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

// Hex digits in the offset a hexdump row opens with, which 4 GB of file fits in.
pub const HEX_OFFSET_DIGITS: usize = 8;
// How much of a binary file the preview reads for its hexdump. Enough to fill
// the tallest pane at the widest row, and nothing there scrolls.
pub const PREVIEW_HEX_BYTES: u64 = 4 * 1024;

// A preview reloads every time the cursor moves, so it only ever reads the head
// of a file - never the whole thing, and never with the large-file prompt.
pub const PREVIEW_MAX_BYTES: u64 = 64 * 1024;
pub const PREVIEW_MAX_LINES: usize = 500;

pub const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
pub const DECIMAL_UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
