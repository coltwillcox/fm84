# 📼 Changelog

All notable changes to FM84 will be documented in this file.

*The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).*

---

## [0.9.0] - 2026-09-22

### ✨ Added
- 🔄 **Automatic refresh** - panels reread themselves when their directory changes on disk, checked once a second and suppressed while a dialog, Viewer or Editor is open
- ⌨️ **Ctrl+R** - reload both panels immediately
- 🧭 **Vanished directories** - a panel whose directory is deleted climbs to the nearest surviving parent instead of stopping on an error
- ❓ **Large file prompt** - F3/F4 on a file above 64 MiB asks before loading it rather than refusing or stalling
- 🛡️ **Panic safety** - raw mode, the alternate screen and the cursor are restored before a panic report prints, so a crash no longer leaves an unusable terminal

### 🛠️ Changed
- ⚡ **Incremental syntax highlighting** - an edit re-parses from the changed line instead of the whole file, with cached per-line parser state: a keystroke in a 50,000-line file went from ~2.7s to ~60µs
- 📏 **Highlighting skipped above 512 KiB** - large files open as plain text instead of pausing to parse
- 🔗 **Symlinked directories behave as directories** - listed as `<DIR>`, sorted with directories and enterable, while copy and delete still act on the link itself
- 📋 **Symlinks are copied as links**, matching `cp -r`, instead of having their targets copied
- 🗂️ **Selections are tracked by filename** rather than row index, so they keep pointing at the file you picked
- 🖱️ **Mouse hit-testing uses the rendered layout** instead of a second copy of the layout arithmetic

### 🐛 Fixed
- 💥 **Viewer crash on an empty file** - it claimed a line it did not hold
- 💥 **Crash on non-ASCII paths** - the path bar truncated on a byte offset, splitting multi-byte characters
- 💥 **Crash on F2 in an empty directory** - the cursor could point past the only row
- 📐 **Border alignment** - the path bar and status bars measure text in display columns, so accented and CJK paths no longer draw short
- 📝 **CRLF line endings are preserved** when saving; a one-character edit no longer rewrites every line
- ✏️ **Rename no longer silently overwrites** an existing file, and no longer offers to rename `..`
- 📦 **Copy and Move check every destination first** - a collision partway through no longer leaves half the items copied
- ♾️ **Copying a directory containing a link to its own ancestor** no longer recurses until the path limit
- 🪟 **Windows key handling** - each keypress registered twice, which made toggles cancel themselves out
- 🎹 **Ctrl and Alt chords** no longer type their plain character into a file, a filename or a confirmation prompt
- 🚪 **`q` no longer quits** - F10 does, and `q` works in quick search again
- 🙈 **F2, F7 and F9 no longer act behind an error or help popup**, where their dialogs were invisible but still committed
- ↔️ **Viewer horizontal scrolling stops** at the end of the longest line instead of scrolling into empty space
- ␀ **An empty rename name is treated as a cancel**

---

## [0.8.2] - 2026-02-13

### ✨ Added
- ↔️ **Horizontal scrolling in Viewer** - Left/Right arrow keys and mouse scroll wheel scroll content horizontally
- ↔️ **Horizontal auto-scroll in Editor** - viewport follows cursor past the right edge; Home key resets horizontal scroll
- 🖱️ **Mouse horizontal scroll** - ScrollLeft/ScrollRight events scroll content in Viewer and Editor
- 🖱️ **Mouse vertical scroll in Editor** - scroll wheel moves viewport without moving cursor; cursor position is preserved even when offscreen
- 🖱️ **Mouse click in Editor** - click anywhere in the content area to position cursor, accounting for scroll offsets and tab expansion

---

## [0.8.1] - 2026-02-13

### 🛠️ Changed
- **Eliminated O(N log N) stat syscalls during directory sort** - items are now built first with metadata, then sorted on pre-computed fields instead of calling `is_dir()` on each comparison
- **Single metadata call per directory entry** - previously called both `path.is_dir()` and `entry.metadata()` separately (two stat syscalls each); now one call provides `is_dir`, size, and modified time
- **Zero-copy file transfers on Linux** - replaced manual 64KB buffer loop with `io::copy` which uses `copy_file_range` in kernel space for File-to-File transfers
- **Faster directory size calculation** - uses `entry.file_type()` (readdir's `d_type` field) instead of stat syscall per entry
- **Pre-allocated vectors** throughout codebase to reduce heap reallocations
- **Removed unnecessary string clones** in directory listing and viewer
- **Idiomatic `&Path` signatures** instead of `&PathBuf` in public and internal APIs

---

## [0.8.0] - 2026-02-13

### ✨ Added
- 🗂️ **Multi-select for F5 Copy, F6 Move, F8 Delete** - operations now work on all selected items when a selection exists, falling back to the cursor item when nothing is selected
- ⌨️ **Delete key** - triggers delete (same as F8) in file panels, forward-delete in Rename/Create dialogs
- 📊 **Panel status bar** - bottom bar shows selected/total file count and selected/total size for each panel
- 🎨 **Active/inactive status styling** - active panel stats use title color, inactive uses dimmed color

### 🛠️ Changed
- 🔧 **Code optimization** - extracted `TextInput` struct to deduplicate rename/create input logic, pre-computed UI styles as constants, unified copy/move popup rendering, extracted shared helpers

---

## [0.7.0] - 2026-02-10

### ✨ Added
- 🚀 **Open files with default program** - Enter key and double-click on files opens them with the OS default application (`xdg-open`, `open`, `start`)

### 🛠️ Changed
- 🎨 **Syntax detection by extension** - Editor syntax highlighting now uses `find_syntax_by_extension` for more reliable language detection

---

## [0.6.0] - 2026-02-10

### ✨ Added
- 🕐 **Modified column** - both panels now display last modification time in `DD/MM/YY HH:MM` format

---

## [0.5.4] - 2026-02-10

### ✨ Added
- ⌨️ **Insert key selection** - Insert key toggles file selection like Space, without calculating directory size

---

## [0.5.3] - 2026-02-10

### 🛠️ Fixed
- 🖥️ **Small terminal crash** - program no longer crashes when terminal is resized too small
- 🔄 **No wrap navigation** - Up/Down keys and mouse scroll no longer loop through the file list
- 📐 **Help popup justified** - F1 Help lines are now uniform width

---

## [0.5.2] - 2026-02-10

### 🛠️ Changed
- 🎨 **Updated logo** - new ASCII art logo in README
- 📸 **Updated screenshot** - refreshed main screenshot

---

## [0.5.1] - 2026-02-10

### 🛠️ Fixed
- 💻 **Detached terminal process** - F9 Terminal now spawns in its own process group, survives FM84 exit
- 🔇 **Suppressed terminal output** - spawned terminal emulators no longer leak log messages to FM84

---

## [0.5.0] - 2026-02-10

### ✨ Added
- 🖼️ **Viewer/Editor borders** - F3 Viewer and F4 Editor now render with bordered frames and title bars
- 🎨 **Editor syntax highlighting** - F4 Editor uses syntax highlighting with live re-highlighting on edits
- 💾 **Unsaved changes prompt** - closing F4 Editor with unsaved changes shows a Save/Discard/Cancel dialog
- 🖱️ **Mouse scroll in Viewer/Editor** - scroll wheel navigates content in F3 Viewer and F4 Editor

### 🛠️ Changed
- 👁️ **F3 Viewer** no longer uses syntax highlighting (plain text for faster viewing)
- 📂 **Directory sizes persist** - calculated directory sizes stay visible after deselecting
- 🎨 **Inactive panel path dimmed** - inactive panel's file path shown in a darker color

---

## [0.4.0] - 2026-02-08

### ✨ Added
- 💻 **F9 Terminal** - open external terminal window in current directory
  - Uses `$TERMINAL` env var, falls back to common terminal emulators

### 🛠️ Changed
- Version constant now reads from `Cargo.toml` at compile time (`env!("CARGO_PKG_VERSION")`)

---

## [0.3.2] - 2026-02-08

### 🛠️ Changed
- Display version number in title bar
- Updated screenshot

---

## [0.3.1] - 2026-02-07

### ✨ Added
- 📦 **F6 Move** - move files and directories to the other panel
  - Cross-filesystem support (copy + delete fallback)
- 🖱️ **Double-click** - open directories or view files with double-click
- ✅ **Multiple selection** - select multiple files for batch operations
- 📏 **Directory size** - show calculated size for selected directories
- 🎨 **File type colors** - files colored by extension for easy visual distinction
- 📂 **Sort by extension** - files ordered by extension by default

### 🛠️ Fixed
- Cross-filesystem copy/move operations
- Error handling on directory size calculation
- Rename click behavior
- Panel click behavior

---

## [0.2.0] - 2025-02-07

### ✨ Added
- 📋 **F5 Copy** - copy files and directories to the other panel
  - Recursive directory copying
  - Confirmation dialog with destination path
  - Duplicate detection

---

## [0.1.1] - 2025-02-07

### 🛠️ Fixed
- Cross-platform compatibility for Windows builds

---

## [0.1.0] - 2025-02-07

### ✨ Added
- 📁 **Dual-pane file manager** - navigate with style
- ⌨️ **Keyboard navigation** - Arrow keys, Home/End, PageUp/PageDown
- 🔀 **Tab switching** - flip between panels like cassettes
- ↩️ **Enter/Backspace** - dive into directories, ascend to parent
- 🔍 **Quick search** - type-ahead filtering with Up/Down navigation
- 💡 **F1 Help** - in-app help popup
- ✏️ **F2 Rename** - rename files and folders
- 👁️ **F3 View** - file viewer with syntax highlighting
  - Support for Rust, Python, JS, TS, JSON, TOML, YAML, Markdown, Shell, C/C++, HTML, CSS
  - Line numbers in gutter
  - Binary file detection
- 📝 **F4 Edit** - built-in text editor
  - Full cursor navigation
  - Insert, delete, backspace
  - F2/Ctrl+S to save
  - Modified indicator
- 📂 **F7 Create** - create new directories
- 🗑️ **F8 Delete** - delete files and folders with confirmation
- 🚪 **F10 Quit** - exit to the void
- 🎨 **Synthwave aesthetic** - violet borders, purple selections, magenta directories
- 🕐 **Live clock** - retro vibes in the header
- 📀 **GitHub Actions release workflow** - cross-platform binaries

---

<p align="center">
  <code>▀▄▀▄ SYNTHWAVE FOREVER ▄▀▄▀</code>
</p>
