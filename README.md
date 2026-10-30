# 🌆 FM84 - File Manager '84

<div align="center">
<pre>
███████╗███╗   ███╗ █████╗ ██╗  ██╗
██╔════╝████╗ ████║██╔══██╗██║  ██║
█████╗  ██╔████╔██║╚█████╔╝███████║
██╔══╝  ██║╚██╔╝██║██╔══██╗╚════██║
██║     ██║ ╚═╝ ██║╚█████╔╝     ██║
╚═╝     ╚═╝     ╚═╝ ╚════╝      ╚═╝
</pre>
</div>

<div align="center">

💜 *A synthwave-infused dual-pane TUI file manager, forged in Rust* 💜

**Version 0.40.0** ▀▄▀▄ *Neon Dreams Edition*

</div>

---

> ⚠️ **WARNING: ALPHA SOFTWARE** ⚠️
>
> 🚧 *This is a work in progress!* 🚧
>
> Things **will** break. Features **may** eat your files. Use at your own risk.
> Back up your data. Trust no one. Not even this README.
>
> *We're still soldering the circuits on this one, choom.* 🔧

<table align="center">
<tr>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-0.png" width="380"><br><sub><b>Dual panes</b><br>198 ROM folders beside a package tree. Each pane carries its own drive strip, the Name/Ext/Size/Modified/Attributes columns, and the free space on its own filesystem.</sub></td>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-1.png" width="380"><br><sub><b>F11 - Options</b><br>Panels, behaviour, appearance, viewer and editor. Every row changes in place with <code>Enter</code> or <code>&larr;</code>/<code>&rarr;</code>.</sub></td>
</tr>
<tr>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-2.png" width="380"><br><sub><b>Outrun</b><br>One of seventeen palettes - hot pink on near-black.</sub></td>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-3.png" width="380"><br><sub><b>Nord</b><br>The muted blue-grey end of the same range.</sub></td>
</tr>
<tr>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-4.png" width="380"><br><sub><b>Picard</b><br>A starship's console - orange, periwinkle and tan on black, the cursor a deep blue band.</sub></td>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-5.png" width="380"><br><sub><b>Military</b><br>Olive drab and khaki, with brass titles and an amber marker.</sub></td>
</tr>
<tr>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-6.png" width="380"><br><sub><b>Catppuccin Latte</b><br>Light themes too, and the whole interface follows the palette, viewer and editor included.</sub></td>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-7.png" width="380"><br><sub><b>F12 - Preview</b><br>The opposite pane shows whatever the cursor is on, without leaving the panel you are in.</sub></td>
</tr>
<tr>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-8.png" width="380"><br><sub><b>F3 - Images</b><br>A JPEG as coloured ASCII, each character on its own colour. <code>F</code> switches Fit and Fill, <code>+</code>/<code>-</code> zoom, <code>X</code> cycles Image, Text and Hex.</sub></td>
<td align="center" valign="top" width="50%"><img src="https://raw.githubusercontent.com/coltwillcox/fm84/master/images/screen-main-9.png" width="380"><br><sub><b>F4 - Editor</b><br>Syntax highlighting, line numbers, selection and undo - editing fm84's own source.</sub></td>
</tr>
</table>

---

## 🔮 The Vibe

Step into the neon-lit streets of '84. Where files flow like synth waves and directories pulse with purple energy. FM84 is your retro-futuristic companion for navigating the filesystem - dual-pane style, just like the legends intended.

Built with 💜 in **Rust** using **Ratatui** + **Crossterm**.

---

## ⚡ Features

### 🗂️ Navigation
- 📁 **Dual-pane layout** - because one panel is never enough
- ⌨️ **Arrow keys** - glide through your files
- 🏠 **Home/End** - teleport to the edges
- 📄 **PageUp/PageDown** - cruise in style
- 🔀 **Tab** - switch between panels like flipping cassettes
- ↔️ **Ctrl+← / Ctrl+→** - send the left or right panel to the directory under the cursor, keeping the focus where it is; on a file, to the directory it is in
- 🔁 **Ctrl+U** - swap the panels, as in Midnight Commander: each takes its tabs, cursor, selection and history to the other side, and the focus goes with the one it was in
- 🪞 **Alt+I** - send the other panel to this one's directory, its cursor on the same entry
- ↩️ **Enter** - dive into directories, and into archives - see [Archives](#-archives)
- 🚀 **Shift+Enter** / **Ctrl+O** - open what is under the cursor as the system would, with the program it opens that kind of file with; an archive too. Ctrl+O is for terminals that send Shift+Enter as plain Enter - GNOME Terminal, Konsole, xterm, Terminal.app, tmux - where only kitty's keyboard protocol (kitty, foot, WezTerm, Ghostty, Alacritty) or Windows tells them apart
- ⬅️ **Backspace** - ascend to parent realm
- 🧭 **Ctrl+L** - or a click on a panel's path - edit the active panel's path in place, in the line above it, and go there with **Enter** (**Esc**, or a click anywhere else, leaves it as it was). **Tab** fills out a name as far as the entries agree, `~` is home, and a path without a leading `/` starts from the panel. A file is landed on, in the directory it is in; somewhere that is not there is said to be, and the panel stays put
- ⏪ **Alt+← / Alt+→** - back and forward through where the tab has been, as in a browser, landing on the entry the cursor was on. Each tab keeps its own, up to 50 steps back; a directory gone since is passed over
- 🕰️ **Alt+↓ directory history** - every directory either panel has been in this run, across all their tabs, newest first and each once, opening on the newest that is not where you are - so Alt+↓ then Enter goes back to where you just were. Typing narrows it to the paths that hold every word typed, in any order (`mus k` finds `~/music/rock`), lower case matching either case; **↑/↓** and **PgUp/PgDn** choose, **Enter** goes, **Esc** closes. One gone since is said to be, and the panel stays put
- 🔖 **Bookmarks** - **Ctrl+D** lists the directories kept to go back to: **Enter**, or **1**…**9**, takes the panel there, **A** or **Insert** keeps the panel's directory, **D** or **Delete** forgets the one highlighted, **Shift+↑/↓** move it up or down the list. The panel's own directory is marked. One that has gone since is said to be, and the panel stays put. Kept in `bookmarks` beside the config, a `bookmark = path` line each, fine to edit by hand
- 🔗 **Symlinked directories** - listed and entered like the real thing
- 🔒 **Directories you cannot read** - refused with the reason, and the panel stays where it was
- 🌿 **Git branch** - after a panel's path, the branch its directory is on when it is in a repository, or the commit when none is checked out. Read from the repository's own files, worktrees and submodules too, on the thread that reads the directory - no git needed, and no wait. It is the first to give way when the path needs the room
- 🔀 **Switch branches** - click the branch and a popup lists the repository's local branches, the one checked out marked `•`. Pick one with the arrows and Enter, or click it and click it again; both panels are read again, so the label and the files follow. When git refuses - edits that the switch would lose - its own reason is shown and the branch stays as it was. This part runs git itself, so it needs git 2.23 or newer
- 👤 **user@host** - who fm84 runs as and where, beside the logo, reversed when it is root. Left off on a terminal too narrow for it
- 💿 **Drive switcher** - click a drive icon, or use Alt+F1 / Alt+F2 (or Ctrl+F1 / Ctrl+F2), to send either panel to a mount; the one each panel is on sits in a block of colour, and removable and optical media get their own icons, including a USB stick mounted under `/run/media`. With more drives than fit, the strip shows a window onto them: `<` and `>` at its ends, or the wheel over it, move it a page along, and it follows the panel's drive and the one being chosen
- 📑 **Tabs** - each panel keeps its own, in the row under its drives. **Ctrl+T** opens one beside the current tab in the same directory, **Ctrl+W** closes it (a panel keeps its last), **Ctrl+PgUp** / **Ctrl+PgDn** go round them and **Alt+1**…**Alt+8** to that tab, **Alt+9** to the last. Click a tab to show it, middle-click to close it. Each remembers its directory, cursor and selection, and with Remember directories on (F11) they are all there next time

### 🔍 Quick Search
- 🔎 **Type-ahead search** - just start typing to find files
- ⬆️⬇️ **Navigate matches** - Up/Down arrows jump between results
- 🧹 **Esc** - clear the search vibes
- 🧪 **Ctrl+S filter** - narrows the panel to what a pattern matches, as it is typed, for a directory of thousands where search only moves the cursor. A word on its own is looked for anywhere in the name (`rep` finds `report.txt` and `prep/`); `*`, `?` and `[a-z]` work as for `+`, several separated by `;`, and one ending in `/` keeps only directories. Lower case matches either case. **Enter** keeps it, shown in the path bar; **Esc** puts back the filter there was; an empty one shows everything again. `..` always stays. What is selected but filtered out is let go, so a copy or delete never takes what cannot be seen. The filter belongs to its directory: rereads are narrowed too, Ctrl+U takes it along, and going anywhere else drops it
- 🌳 **Ctrl+B flat view** - every file under the panel's directory in one list, as Total Commander's branch view, each by its path from there (`sub/deep/notes.txt`). Sorted by size (Ctrl+F6), it finds what is taking the space. Directories are walked rather than listed; links to them are not followed, nothing in `/proc`, `/sys` and the like is gone into, and what is hidden stays hidden unless F11 shows it. Copy, move, delete, view, edit, compare and select work on the files as anywhere - F5 copies them without the directories they are in - and F2 renames one where it is. Inside an archive too. The path bar says `flat`; it combines with Ctrl+S, so `*.rs` across a whole tree works. Ctrl+B again goes back, onto what holds the file the cursor was on, and going anywhere else drops it. A tree too big to wait for gets **Esc** as any slow directory does, and the list stops at 100000 files, saying so

### 🔭 Find Files (Alt+F7)
- 🎯 **By name, and by what is in them** - a name pattern as `+` takes it (`*.jpg;*.png`, `*/` for directories), and text the file has to hold, lower case matching either case. **Ctrl+F7** works too, for desktops that take Alt+F7 for moving windows. The last search is offered again
- 🌳 **Down from the panel you are in** - each directory's contents straight after it, in the order the panels list them, as `find` lists a tree. Links to directories are not followed, and hidden entries are passed over unless F11 shows them. The system's own trees - `/proc`, `/sys`, `/dev` and `/run`, all but `/run/media` - are left out unless the search starts in one: their entries pass for files and are not, and some never answer a read
- 🪶 **Never in the way** - it runs on a thread of its own, and results arrive as they are found while it says where it is looking. Files are read 64 KiB at a time, never whole, and pipes and devices are never opened. Directories that cannot be read are counted, not fatal
- ↩️ **Enter goes there** - the panel moves to the directory of the result chosen, cursor on it. Esc closes the list, stopping a search still under way

### 📝 File Operations
- **F1** 💡 - Help/About: the keys, then what fm84 is - its version, licence and where it lives. Like every popup it sits over the panels, no taller than they are; when the list is longer, the arrows, PageUp/PageDown, Home/End and the wheel scroll it, and the corner says which lines show
- **F2** ✏️ - Rename files & folders (arrows, Home/End, Backspace and Delete while typing, the field scrolling to keep the cursor in view on a long name); a change of case alone works on macOS and Windows too
- **F3** 👁️ - View files (text, hexdump, or images as ASCII art); Esc closes the viewer, F3 inside it finds the next match
- **F4** 📝 - Edit files with **syntax highlighting** (Ctrl+S to save, Esc to close with an unsaved changes prompt, mouse click to position cursor)
- **F5** 📋 - Copy (selected items or cursor item), keeping permissions and modified times where the filesystem can hold them
- **F6** 📦 - Move (selected items or cursor item); a move to another disk keeps modified times too, so it looks just as a move on one disk does
- 🎯 **Where to** - F5 and F6 ask where, with the answer filled in: for a single file its path in the other panel, ready for a new name; for anything else the other panel's directory. Enter alone sends it there. Read as `cp` and `mv` read it: into a directory that is there, or one typed with a `/` on the end; anything else is a new name for the one item. A path that is not absolute starts from the panel you are in, so a bare name copies beside the original - and F6 with one renames it. `~` is home
- ♻️ **Names already taken** - each file already there is asked about as the job reaches it, with both shown, size and date and which is newer: **Y** overwrite, **N** skip, **K** keep both, **A** overwrite all, **S** skip all, **B** keep both all, **O** overwrite all older, **Esc** abort. Keep both leaves what is there and writes the new one beside it as `name (2).ext` - `src (2).tar.gz` for an archive, `.bashrc (2)` for a dotfile - numbering on past any already taken. Directories are merged, and asked about file by file. F11 can make it overwrite, or refuse the whole job, without asking
- 📊 **Progress while copying, moving and deleting** - a bar with the current file, how far along it is and the transfer rate; Esc cancels. Each file is written beside its destination and renamed into place only once it is whole, so a copy that is cancelled, fails or is cut short by quitting never leaves half a file under the real name. The work runs off the interface thread, so a slow or stalled disk cannot freeze the display
- 🧯 **One bad entry doesn't sink the job** - a file that will not copy, move or delete (a socket, a pipe, a permission refused, a disk full) stops the job and asks: **R** retry, **S** skip, **A** skip all, **Esc** abort. A skipped entry is left where it was, and a directory holding one stays with it, so a move never leaves anything gone from both sides. The end says how many were skipped
- 🚪 **A way out of an operation that will not finish** - F10 during one offers to leave; a second F10 takes it, for a disk that has stopped answering and never notices the cancel
- **Alt+F5** 🔗 - Make a symbolic link to what is chosen. **Ctrl+Shift+F5** works too, as in Total Commander, where the terminal lets it through - kitty keeps it for reloading its config. It asks where, as F5 does, opening on the entry's name in the other panel - a directory too, since the link is the new entry - or, for several, on the other panel's directory, with a link in it for each. Each points at the absolute path of what it links to. **Tab** makes hard links instead - another name for the same file, which outlives the original; files only, on the same filesystem. Nothing already there is written over: an entry that cannot be linked is named, and the rest are still made. On Windows a symbolic link takes Developer Mode or the right to make links
- **Shift+F2** ⚖️ - Compare directories, as in Total Commander: each panel selects what differs from the other - an entry only it has, and of a file both have, the newer one, or both when the times agree and the sizes do not. Times within two seconds count as the same, as FAT keeps them no finer. Directories both have are not gone into. What was selected before is replaced, ready for F5 to copy across, and the status bar says how many differ
- **F7** 📂 - Create new directories (**Shift+F4** for an empty file); same editing keys as rename
- **F8** / **Delete** 🗑️ - Move files & folders to the system's trash (selected items or cursor item, with confirmation - which F11 can turn off - and progress). What the trash will not take - a read-only disk, one with nowhere to keep a trash - is asked about, with **D** to delete it for good. F11 can make F8 delete permanently instead
- **Shift+F8** / **Shift+Delete** 💀 - Delete permanently, always asking first
- **Alt+H** #️⃣ - Checksums of what is selected, or of the file under the cursor, a directory's files all the way down. SHA-256, SHA-512 and MD5 are all worked out in one reading of each file, as a job with the progress bar and Esc, and shown in one popup - wrapped, never cut, however long - with the algorithms in a row above them: **Tab** or **→** shows the next one's, **Shift+Tab** or **←** the one before, at once. **C** copies them - one file's checksum alone, for pasting where it is to be compared - and **Enter** writes them as `sha256sum` does, to `name.sha256` for one file or `SHA256SUMS` for several, and lands on it; a name already taken goes by F11's When destination exists. On a checksum file - `*.sha256`, `*.md5`, `SHA256SUMS` and the like, written by `sha256sum`, `md5sum` or in BSD's `SHA256 (name) = …` - it checks the files it names instead, and says what failed, what is missing and what is OK, the failures first. What cannot be read is listed as such, not asked about. Inside an archive, copy out first
- **Alt+A** 🔐 - Change attributes, as in Total Commander: the permissions, owner, group and modified time of what is selected, or of the entry under the cursor. Each field opens on what the entries have in common, and one left as it is, or emptied, changes nothing. The mode is octal (`755`) or as `chmod` takes it (`u+x,go-w`, `a=rX`), so several entries whose modes differ can each gain or lose a bit; with one entry, what it comes to is shown beside it. Owner and group go by name or number, and the time as `2026-10-23 14:02:11`. **Tab** or a click moves between the fields, a click putting the cursor where it lands. Something typed that cannot be read is said so before anything changes; an entry that refuses is passed over and named. On Windows only the time
- **F9** 💻 - Open external terminal in current directory (which one is set under F11)
- **F10** 🚪 - Exit to the void - asking first about unsaved changes in the editor. **Shift+F10** leaves `cd84` in the directory it started from
- **Space** / **Insert** ✅ - Select/deselect files for batch operations
- **+** / **-** / **\*** 🎯 - Select or deselect by a pattern such as `*.jpg;*.png` (`*`, `?`, `[a-z]`, `[!0-9]`; lower case matches either case), or invert the selection of files - **Alt+\*** inverts directories too. Patterns pick files; end one with `/` for directories, as in `*/`. Once a quick search is under way, `-` types into it instead
- 📋 **Ctrl+C** / **Alt+C** - copy the full paths, or just the names, of what is selected - or of the entry under the cursor - to the clipboard, a line each; on `..`, the panel's own directory. It goes over OSC 52, so it reaches the system clipboard over SSH too, and the built-in editor pastes it either way
- 🖱️ **Double-click** - open directories and archives, or open files as Enter does
- 🖱️ **Mouse scroll** - scroll content in Viewer, Editor, and file panels

### 🔄 Live Panels
- 👀 **Automatic refresh** - panels notice when their directory changes on disk and reread themselves, and the preview and the detail lines are gathered again with them
- ⌨️ **Ctrl+R** - force an immediate reload of both panels
- 🧭 **Vanished directories** - if the open directory is deleted, the panel climbs to the nearest surviving parent
- 🤫 **Stays out of the way** - never reloads while a dialog, Viewer or Editor is open
- 🐌 **Slow and dead mounts** - directories are read, watched and described on threads of their own. A network mount that stops answering no longer freezes fm84: the panel keeps showing what it did, the status bar says it is reading, and Esc stops waiting. The other panel works as ever

### 🧱 Columns
- 📋 **Name, Ext, Size, Modified, Attributes** - permissions written the way `ls -l` writes them
- 📐 **Priority when space runs short** - columns drop from the right, Name always stays and keeps the leftover width
- 🔃 **Sort from the panel** - **Ctrl+F3** by name, **Ctrl+F4** by extension, **Ctrl+F5** by date and **Ctrl+F6** by size, as in Total Commander, or click a column heading; the same one again turns the order round. The heading sorted by carries an arrow, ↑ or ↓, and both panels follow, kept as F11 keeps it
- 🎛️ **Your choice of them** - F11 turns each one off, and sets how dates and sizes are written
- 🔤 **Any name the filesystem allows** - a name that is not valid UTF-8 is shown as near as it can be read and still opens, renames, copies and deletes as itself; operations work from the name the filesystem holds, never from what it looked like on screen - selection, the F4 and F9 commands and remembered directories included
- 🛡️ **Names neutralised** - a file name can hold anything but `/`, escape sequences and tabs included. They are drawn as dots rather than sent to the terminal, wherever a name appears: the rows, the detail lines, the preview, the titles and every popup

### 📊 Status Bar
- 📈 **Panel stats** - selected/total file count and size shown per panel
- 💽 **Disk usage** - a meter and used/total for each panel's filesystem, shortening then stepping aside on narrow terminals
- 🎨 **Active/inactive styling** - active panel stats highlighted, inactive dimmed
- 🎹 **Key bar** - the F-keys along the bottom border, as in Midnight Commander: those the panels, the Viewer or the Editor answer, F8 named Trash or Delete as F11 has it. Click one to press it. On a narrow terminal they drop off whole from the end

### 🔎 Detail Lines
- 📋 **Two lines under the panels** - what the columns have no room for: the whole name however long, the exact byte count, the timestamp to the second, `owner:group`, the permission bits, and where a symlink points
- ✂️ **Narrow terminals** - whole fields are given up from the right rather than cut mid-figure, and the name shortens from the front so the extension survives

### 🎨 Viewer (F3)
- 🖼️ **Bordered frame** with filename title bar
- 📊 **Line numbers** in the gutter (F11 can hide them)
- 🔢 **Status bar** - filename, line count, file size, detected syntax
- 🔢 **Hex view** - binaries open as a `hexdump -C`; `X` toggles hex for any file, so you can eyeball a BOM or CRLF endings
- 🖼️ **Images as ASCII art** - PNG, JPEG, GIF, WebP and BMP drawn in colour, each character on its own coloured background so the dark parts keep their colour; `X` cycles Image, Text and Hex, `F` switches between fitting the whole picture in, centred, and filling the viewer; which one a picture opens with, and whether backgrounds are drawn, is set under F11
- 📄 **Step through a folder's pictures** - PageUp and PageDown move to the next picture and the previous one, wrapping round and stepping over anything that is not one; the neighbours are decoded ahead so each arrives without a pause (F11 can turn that off)
- 🖱️ **Drag a picture** - hold the mouse down on it and it follows the pointer, both directions at once
- 📐 **True proportions** - the shape of a picture is worked out from the size of your terminal's cells, which it is asked for, rather than from an assumption about them; a terminal that will not say gets the usual 2:1
- 🔍 **Zoom** - `+` and `-` scale a picture from a quarter of the fitted size to four times it, keeping whatever is in the middle of the view in the middle
- 🖱️ **Mouse selection** - drag to select, Ctrl+C to copy
- 🔎 **Find** - Ctrl+F asks what to look for, F3 and Shift+F3 go to the next and previous match, round the ends of the file. Every match on screen is underlined and the current one selected, ready for Ctrl+C. Lower case matches either case, a capital only itself. In hex view it searches the dump as shown
- 🔢 **Go to line** - Ctrl+G, then the number
- 🛡️ **Escape sequences neutralised** - a file full of control codes can't hijack your terminal
- 🔤 **Stray bytes** - a text file with a byte that is not UTF-8 in it still opens, with a placeholder where the byte was; `X` shows it as it is
- ❓ **Large file prompt** - asks before pulling anything over 64 MiB into memory (16 or 256 under F11), and before decoding a picture that needs over 256 MiB of it - a few hundred KB of PNG can unpack to hundreds of MB
- ↔️ **Horizontal scrolling** - Left/Right keys and mouse scroll wheel, stopping at the longest line
- ┃ **Scroll bars** - drawn into the frame's border, down the right for the lines and along the bottom for the columns, each only when there is more than fits; the text keeps every column it had. Click the track for a page that way, or drag the thumb, which follows the pointer until the button comes up. F11 sets what the thumbs are drawn with
- 📜 **Wrap** - `W` breaks long lines onto as many rows as they need, after a space where there is one; scrolling goes by rows, so a line longer than the screen can be read to its end. Finding, selecting and copying go on by whole lines, so a copy never gains a break that was only on screen, and line numbers and Go to line count lines, not rows. The next file opened wraps or not as the last was left
- 🖱️ **Mouse scroll** - vertical and horizontal scrolling with the scroll wheel

### ✍️ Editor (F4)
- 🌈 **Syntax highlighting** for Rust, Python, JS, TS, JSON, TOML, YAML, Markdown, Shell, C/C++, HTML, CSS
- ⚡ **Incremental highlighting** - typing re-parses from the edited line, so speed doesn't fall off in long files
- 📏 **Highlighting limit** - files over 512 KiB open as plain text, since the first parse takes about 0.9s per MB; F11 moves it to 256 KiB or 2 MiB, or turns highlighting off
- ↩️ **Line endings preserved** - a CRLF file stays CRLF when saved
- 🖼️ **Bordered frame** with filename and modified indicator in title bar
- 📄 **Full text editing** - cursor navigation, insert, delete
- ✂️ **Select, cut, copy, paste** - Shift+arrows to select, Ctrl+A for all, Ctrl+X/C/V (or Ctrl+Insert, Shift+Insert, Shift+Delete)
- ↩️ **Undo and redo** - Ctrl+Z, then Ctrl+Y or Ctrl+Shift+Z, 200 steps deep, one step per action rather than per keystroke burst; a key that changes nothing costs no step
- 📋 **System clipboard** - copies reach it over OSC 52, pastes arrive as bracketed paste, both without linking a clipboard library
- 💾 **Save** - F2 or Ctrl+S, written beside the file and swapped in whole, so a full disk or a crash mid-save leaves the old file rather than half of the new one; links, permissions and owner are kept
- 🛑 **Changed on disk** - when something else has written the file since it was opened, a save stops and says so, the file as it was and as it is now: **O** overwrites it, **R** reloads it and drops the edits, **Esc** goes back to them. A file deleted meanwhile can be saved again with O
- 📍 **Line/Column tracking** - always know where you are
- 🔎 **Find and go to line** - Ctrl+F, F3 / Shift+F3 and Ctrl+G, as in the Viewer; the match found is selected, so typing replaces it
- 🔁 **Replace** - Ctrl+R or F4 asks what to replace (the selection, or the last thing looked for) and what with, then goes from the cursor a match at a time: **Y** replaces, **N** skips, **A** replaces the rest, **Esc** stops. It goes round the end of the file and stops where it began, never into the text it has just put in; Enter is not an answer. Replace all is one step for Ctrl+Z
- ⚠️ **Unsaved changes prompt** - Save/Discard/Cancel dialog on close, and on F10; a save that fails keeps the editor open, edits and all
- 🔤 **UTF-8 only** - a file that is not UTF-8 text is refused rather than opened with its odd bytes replaced, which saving would make permanent; F3 shows it
- ↔️ **Horizontal auto-scroll** - viewport follows cursor past the right edge
- ┃ **Scroll bars** - as in the Viewer: click the track for a page, drag the thumb; like the wheel they move the view and leave the cursor where it is
- 🖱️ **Mouse scroll** - vertical and horizontal scrolling with the scroll wheel
- 🖱️ **Mouse click and drag** - click to position the cursor, drag to select

### ⚖️ Compare Files (= or Shift+F3)
- 🎯 **Which two** - two files selected in the panel you are in; otherwise the file under the cursor against the one of the same name in the other panel, or failing that the one under the cursor there. The left panel's file is always on the left
- 🪞 **Side by side** - each file under its path, numbered as F11 numbers lines, with the mark `diff -y` uses between them: `|` changed, `<` only on the left, `>` only on the right. Lines that differ are shaded. Lines removed and added in one place are paired off, and whichever side has more carries on alone
- ⏭️ **From one difference to the next** - it opens on the first; **F3** or **N** goes to the next, **Shift+F3** or **P** to the previous, and the status bar says which it is on. Arrows, PageUp/PageDown, Home/End and the wheel scroll both sides together; **Esc** closes it
- 🧮 **The diff `diff` and git use** - Myers', on lines, after the lines both files start and end with are set aside. Two files that differ in more than 2,048 lines between those are shown as one block replaced
- 💬 **Nothing to show** - files that are the same, that differ only in line endings (CRLF against LF, or a missing newline at the end), or that are not text are only spoken of, in the status bar - for binaries, the first byte that differs. Either file over the size F11 asks before opening is refused
- ⌨️ **Shift+F3 needs a terminal that can send it** - most send F3 with a modifier as they report where their cursor is, and fm84 cannot tell the two apart. fm84 asks for kitty's keyboard protocol, which settles it, where the terminal has it - kitty, foot, WezTerm, Ghostty, Alacritty. Elsewhere, `=` (Ctrl+F3 sort and the Viewer's Shift+F3 have the same trouble there)

### 👁️ Preview (F12)
- 🪞 **Opposite panel** - shows the head of whatever the cursor is on, and follows it
- 📁 **Directories** - item count instead of contents
- 🔢 **Binaries as a hexdump** - no longer just "Binary file": the head of it in hex with the printable characters beside it, at 16, 8 or 4 bytes a row depending on how much room the pane has
- 🪶 **Bounded** - reads at most 64 KiB of a text file and 4 KiB of a binary, never prompts, never loads a whole file, and never opens a pipe or a device

### ⚙️ Options (F11)
Grouped under four headings, in a popup no taller than the panels: the list scrolls within it, with ↑/↓ or the mouse wheel, which stop at its first and last row, and the corner says which row of how many the cursor is on.

**Panels**
- 🙈 **Show hidden files** - dotfiles, or on Windows anything with the hidden attribute
- 🔃 **Sort by** Name, Extension, Size or Modified, **ascending or descending**
- 📁 **Directories first** - or mixed in among the files
- 🔠 **Case-sensitive sort** - off by default, so `readme` and `README` sit together
- 🧱 **Ext, Size, Modified and Attributes columns** - each on or off; with Ext off, Name shows the whole file name
- 📅 **Date format** - `dd/mm/yy`, `yyyy-mm-dd`, or relative (`3 h ago`)
- 📏 **Size units** - KiB (1024) or kB (1000), everywhere a size is shown

**Behaviour**
- 🗑️ **Confirm delete** and ✅ **Confirm copy and move** - turn off to skip the question
- ♻️ **Delete to trash (F8)** - on, F8 moves to the trash; off, it deletes for good. Shift+F8 always deletes for good
- ♻️ **When destination exists** - Ask about each file as the copy or move reaches it, Overwrite them all (directories merged), Keep both - each new file beside the one in its way, as `name (2).ext` - or Refuse a job with any name taken before it starts. Alt+P goes by it too, for an archive already at the name. A file never replaces a directory or the other way round, and nothing is copied onto itself
- 💻 **Terminal (F9)** - any command, `{}` standing for the directory; left empty, one is picked for you
- 📝 **Editor (F4)** - an external editor such as `nvim` or `hx`, which gets the terminal until it exits; `{}` stands for the file, which otherwise goes last. Left empty, the built-in editor opens
- 📍 **Remember directories** - reopen both panels where they were when fm84 last quit
- 👁️ **Preview on startup** - start with F12's preview open

**Appearance**
- 🎨 **Theme** - switched on the spot, 17 of them:
  - fm84's own: **Synthwave**, **Outrun**, **Vaporwave**, **High contrast**, **Military**, **Picard**
  - dark: **Dracula**, **Monokai**, **Nord**, **Gruvbox Dark**, **Solarized Dark**, **Tokyo Night**, **Catppuccin Mocha**
  - light: **Solarized Light**, **Gruvbox Light**, **Catppuccin Latte**, **GitHub Light**
  - each brings its own background, gutter and syntax colours; on a light one, extension colours darken and pictures draw dark ink on the light background
- 🖌️ **Background** - the theme's own, or the terminal's for a transparent or custom one. A light theme needs its own: on a dark terminal its text would all but vanish
- 🔣 **Icons** - Nerd Font, Nerd Font Mono (drops the padding a wide glyph needs, so icons stay centred), or Plain letters for a terminal without a Nerd Font
- 🕐 **Clock** - 24-hour, 12-hour, or off
- ▏ **Cursor** - in every field being typed into and in the editor: the terminal's own cursor as a **Bar**, a **Block** or an **Underline**, or **Classic**, the reversed cell fm84 drew before, for a terminal that cannot be told a shape (the Linux console). On exit the terminal gets its own shape back
- ✨ **Cursor blinks** - on or off. How fast, and whether at all, is up to the terminal: kitty's `cursor_blink_interval 0` keeps it still whatever this says

**Viewer and editor**
- ⇥ **Tab width** - 2, 4 or 8 columns, in the Viewer, the Editor and the preview
- 📊 **Line numbers** - on or off, in the Viewer and the Editor
- ┃ **Scroll bars** - what their thumbs are drawn with: **Heavy** `┃━`, the border's own line drawn thicker, **Double** `║═`, **Dashed** `╏╍`, **Block** `█`, **Shade** `▓`, or **ASCII** `#` for a font without box drawing
- 🌈 **Highlight files up to** 256 KiB, 512 KiB or 2 MiB, or never
- ❓ **Ask before opening over** 16, 64 or 256 MiB
- 🖼️ **Images open as** Fit or Fill
- 🎨 **Image backgrounds** - on or off. On, each character sits on its own colour, so the dark parts of a picture keep theirs instead of showing the terminal through the gaps in the glyphs. Off costs less to send, which matters over a slow link
- ⚡ **Read pictures ahead** - decode the pictures either side of the one open, so PageUp and PageDown do not wait for each. Costs a few megabytes and a thread that wakes only while a picture is open

- ⌨️ **Up/Down** to move, **Enter**, **Space** or **Left/Right** to change, Enter again to save a typed command
- 💾 **Saved as you go** to `~/.config/fm84/config` (`$XDG_CONFIG_HOME` if set, `%APPDATA%\fm84\config` on Windows) - plain `key = value` lines, fine to edit by hand. Remembered directories go in `session` beside it, and bookmarks in `bookmarks`

### 📦 Archives
- 📂 **Gone into like directories** - Enter on a `.zip`, `.7z`, `.tar`, `.tar.gz`, `.tar.zst`, `.tar.xz` or `.tar.bz2` (and `.tgz`, `.tzst`, `.txz`, `.tbz2`) opens it in the panel, and its directories open as any do. The path runs through it - `~/photos.zip/2024/june` - so tabs, history, bookmarks, Ctrl+L and remembered directories all work inside one, and Backspace at its top comes back out onto the archive
- 📋 **Copied out with F5** - to the other panel, or wherever the dialog is told, a directory whole, asked about files in the way as any copy is, with the same progress bar and Esc. Times and permissions come out as the archive kept them; links come out as links
- 📤 **All of it with Alt+F9** - as in Total Commander, the whole of the archive under the cursor, or of the one the panel is in, into the other panel or wherever the dialog is told. **Ctrl+F9** too, for desktops that take Alt+F9 for hiding a window
- ⚖️ **Compared with Shift+F3** - a file inside an archive against one on the disk, or inside another archive: each is copied out first, and the diff is headed with the paths the panels show
- 🗜️ **Every way a tar is packed** - gzip, zstd, xz and bzip2, each read on past its first stream, so files joined end to end - as pigz, pzstd and pbzip2 write them - are one archive. A 7z is listed from its header without unpacking anything; one packed solid, as 7-Zip does by default, unpacks what comes before a file to reach it, which the progress bar covers
- 👁️ **F3, F12 and the detail lines** - a file is copied out to a scratch directory and shown, the preview reads the head of it, and the lines under the panels give its exact size, time, mode and, from a tar, its owner. Enter or Ctrl+O open it with the system's program the same way. The scratch directory goes when fm84 quits
- 📏 **Sizes at once** - Space and Alt+Shift+Enter size directories inside an archive from what it says, with nothing to walk
- 🗃️ **Made with Alt+P** - what is chosen, packed into a `.zip`, `.7z`, `.tar.gz`, `.tar.xz`, `.tar.bz2` or `.tar`, as a job with the progress bar and Esc. The dialog opens on the other panel's directory with a name from what is packed - a file's without its ending, a directory's, or for several the one they are in - and **Tab** goes through the formats by the ending of the name. Modes, times and links go in as they are; a zip or a 7z cannot hold a name that is not UTF-8, which is asked about, and a tar keeps it as it is. A 7z is packed solid, as 7-Zip packs one. It is written beside where it goes and put there whole, so a cancel leaves nothing, and an archive made inside the directory being packed leaves itself out. One already at the name is written over only when F11 says Overwrite. `.tar.zst` is read but not made: the pure-Rust zstd encoder cannot say when a write fails
- 🔒 **Read-only** - renaming, making, deleting, moving out, moving or copying in, links and attributes all say "Archives are read-only" and leave it as it was. F4 too: F3 shows a file instead. Find files does not reach inside one yet
- 🛡️ **Safe to open** - a name that would land outside where it is copied to - `../`, an absolute path, a drive - never gets into the listing, nor anything that would be written through a link in the archive. An encrypted zip entry says so in the detail lines, and copying it out says it needs the password; an encrypted 7z says it needs the password as a file is copied out of it - or as it is gone into, when its names are encrypted too
- ♻️ **Kept up to date** - an archive written while a panel is in it is read again, climbing out of a directory it no longer has, and one deleted is left. A large `.tar.gz` is read once and kept, so going in and out of its directories does not unpack it again
- 💬 **Not an archive after all** - a file named like one that will not read says why, and the panel stays where it was

### 📂 Directory Sizes
- 📏 **Calculated on select** - press Space on a directory to calculate its size; a link to one is selected but not counted, for the same reason as below
- 🗂️ **Every directory at once** - `Alt+Shift+Enter`, as in Total Commander (`Alt+Enter` too, for a terminal that cannot report Shift with Enter), sizes every directory in the panel, top to bottom, four at a time; the rest show `…` while they wait their turn. Links to directories are left out, since one to `/` or to a slow mount would walk all of it
- 🔗 **Counted as du counts** - a file with several hard links counts once, and filesystems the kernel makes up - `/proc`, `/sys` and their kin - count nothing, so sizing `/` gives what is on the disk rather than the size of all memory
- 🧵 **Counted in the background** - the Size column shows the running total with a `…` while it counts, the panels keep answering, and Esc stops it, as does leaving the directory (unless the other panel is in it too); a slow disk or a huge tree no longer holds up the interface
- 📌 **Persistent display** - sizes stay visible after deselecting, until a copy, move or delete may have changed them

---

## 🎹 Keybindings

| Key | Action |
|-----|--------|
| `↑` `↓` `←` `→` | Navigate |
| `Tab` | Switch panels |
| `Ctrl+←` / `Ctrl+→` | Open the directory under the cursor in the left / right panel |
| `Ctrl+U` | Swap the panels |
| `Alt+I` | Same directory in the other panel |
| `Ctrl+L` | Go to a path (`Tab` completes a name; or click the path) |
| `Alt+←` / `Alt+→` | Back / forward through where the tab has been |
| `Alt+↓` | Every directory visited, across the tabs (type to narrow, `Enter` go) |
| `Ctrl+D` | Bookmarks (`A` add, `D` remove, `Enter` or `1`…`9` go) |
| `Enter` | Open directory, go into an archive, or open a file |
| `Shift+Enter` / `Ctrl+O` | Open as the system would (an archive too) |
| `Backspace` | Go to parent directory |
| `Home` / `End` | Jump to first / last item |
| `PageUp` / `PageDown` | Page navigation |
| `[a-z0-9]` | Quick search |
| `Ctrl+S` | Filter the panel by a pattern, as it is typed (`Esc` puts back the one there was) |
| `Ctrl+B` | Flat view: every file under the directory in one list; again to go back |
| `Esc` | Clear search / Close dialogs, the Viewer and the Editor / Stop directory sizing / Stop waiting on a directory that has not answered |
| `F1` | Help |
| `F2` | Rename |
| `Shift+F2` | Compare directories: select what differs in each panel |
| `F3` | View file |
| `=` / `Shift+F3` | Compare two files side by side |
| `F3` / `Shift+F3` or `N` / `P` | Next / previous difference (in Compare) |
| `F4` | Edit file (built-in, or the editor set under F11) |
| `F5` | Copy (to the other panel, or wherever the dialog is told) |
| `Alt+F5` / `Ctrl+Shift+F5` | Make a symbolic link (in the other panel, or wherever the dialog is told; `Tab` for a hard link) |
| `F6` | Move (to the other panel, or wherever the dialog is told) |
| `F7` | Create directory |
| `Shift+F4` | Create empty file |
| `F8` / `Delete` | Move to trash (selected items or cursor item) |
| `Shift+F8` / `Shift+Delete` | Delete permanently |
| `Alt+A` | Change permissions, owner, group and modified time |
| `Esc` | Cancel a running copy, move or delete |
| `R` / `S` / `A` | Retry / Skip / Skip all, when an entry in a copy, move or delete fails |
| `Y` / `N` / `A` / `S` / `O` | Overwrite / Skip / Overwrite all / Skip all / Overwrite all older, when a file is already there |
| `D` | Delete permanently, when the trash will not take an entry |
| `F9` | Open terminal (the one set under F11, if any) |
| `F10` | Quit (twice during an operation; asks first about unsaved edits) |
| `Shift+F10` | Quit, leaving `cd84` in the directory it started from |
| `F11` | Options - panels, behaviour, appearance, viewer and editor |
| `F12` | Preview cursor file in other panel |
| `Alt+F1` / `Alt+F2` | Choose a drive for the left / right panel (Ctrl works too, or click an icon) |
| `Alt+F7` / `Ctrl+F7` | Find files by name and content |
| `Alt+F9` / `Ctrl+F9` | Extract the whole of an archive |
| `Alt+P` | Pack what is chosen into an archive (`Tab` changes the format) |
| `Alt+H` | Checksums of what is chosen (`Tab` / `Shift+Tab` the algorithm, `C` copies, `Enter` writes), or check a checksum file |
| `X` | Toggle hex view; cycle Image, Text, Hex for images (in Viewer) |
| `F` | Fit or fill an image (in Viewer) |
| `W` | Wrap long lines, or scroll them sideways (in Viewer) |
| `PageUp` / `PageDown` | Previous / next picture in the folder (in Viewer) |
| `Drag` | Move a picture about (in Viewer) |
| `+` / `-` | Zoom an image in or out (in Viewer) |
| `Shift`+arrows | Select text (in Editor) |
| `Ctrl+A` | Select all (in Editor) |
| `Ctrl+X` / `Ctrl+C` / `Ctrl+V` | Cut / Copy / Paste (in Editor) |
| `Ctrl+Z` | Undo (in Editor) |
| `Ctrl+Y` / `Ctrl+Shift+Z` | Redo (in Editor) |
| `Ctrl+C` | Copy selection (in Viewer) |
| `Ctrl+F` | Find (in Viewer and Editor) |
| `F3` / `Shift+F3` | Next / previous match (in Viewer and Editor) |
| `Ctrl+G` | Go to line (in Viewer and Editor) |
| `Ctrl+R` / `F4` | Replace (in Editor) |
| `Y` / `N` / `A` | Replace / Skip / Replace all, when replacing (in Editor) |
| `O` / `R` | Overwrite / Reload, when the file changed on disk since it was opened (in Editor) |
| `Space` / `Insert` | Select/deselect file |
| `Alt+Shift+Enter` / `Alt+Enter` | Size every directory in the panel |
| `+` / `-` | Select / deselect by pattern |
| `*` | Invert the selection of files |
| `Alt+*` | Invert the selection of files and directories |
| `Ctrl+C` / `Alt+C` | Copy full paths / names to the clipboard |
| `Ctrl+F3` / `Ctrl+F4` / `Ctrl+F5` / `Ctrl+F6` | Sort by name / extension / date / size, again to reverse (or click a heading) |
| `Ctrl+R` | Reload both panels |
| `Ctrl+T` / `Ctrl+W` | New tab / close tab |
| `Ctrl+PgUp` / `Ctrl+PgDn` | Previous / next tab |
| `Alt+1`…`Alt+9` | Go to tab 1…8, or the last |
| `Scroll` | Scroll content (panels, Viewer, Editor) |
| Click / drag a scroll bar | A page that way / follow the pointer (Viewer, Editor) |
| Click the git branch | List the repository's branches, to switch to another |

---

## 🛠️ Install, Build & Run

```bash
# 📦 Straight from crates.io
cargo install --locked fm84
```

*`--locked` builds with the exact dependency versions fm84 was tested against. The binary lands in `~/.cargo/bin`, and the same command again updates it.*

```bash
# 🦀 Clone the future
git clone https://github.com/coltwillcox/fm84.git
cd fm84

# ⚙️ Compile with cargo
cargo build --release

# 🚀 Launch into the neon grid
cargo run --release
```

### ⌨️ Command line

```bash
fm84                    # panels and tabs where you left them (or the current directory)
fm84 ~/Music            # left panel in ~/Music
fm84 ~/Music /mnt       # left in ~/Music, right in /mnt
fm84 --shell-init bash  # print cd84 for bash (or zsh, fish, powershell)
fm84 --choosedir FILE   # on quitting, write the active panel's directory to FILE
fm84 --help             # usage, and where the config lives
fm84 --version          # fm84 0.40.0
```

📖 `man ./doc/fm84.1` reads the manual page from a checkout; the release archives carry `fm84.1` beside the binary, for `/usr/local/share/man/man1`.

A directory given takes the place of the tab that would have shown; the panel's other tabs stay. A directory that does not exist is reported in the shell before anything starts. `--` ends the options, for a directory whose name starts with a dash.

*Debug builds compile dependencies optimised too, so a plain `cargo run` still decodes images at full speed. The first debug build takes a little longer for it.*

### 📍 cd84 - leave the shell where fm84 was left

A program cannot change its shell's directory, so quitting fm84 normally drops you back where you started. `cd84` is a shell function that fixes that: it runs fm84, and on quitting goes to the directory of the active panel. Arguments pass through (`cd84 ~/Music`), and so does fm84's exit status. fm84 prints the function itself, so it always matches the binary it came with - `cargo install` users get it too. Add one line to your shell's startup file:

```bash
eval "$(fm84 --shell-init bash)"                               # ~/.bashrc
eval "$(fm84 --shell-init zsh)"                                # ~/.zshrc
fm84 --shell-init fish | source                                # ~/.config/fish/config.fish
fm84 --shell-init powershell | Out-String | Invoke-Expression  # $PROFILE
```

Then start fm84 as `cd84`. It works for any directory name, a trailing newline or bytes that are not UTF-8 included, and stays put if fm84 did not quit normally - or quit with **Shift+F10**, for the times you only wanted a look around. Underneath, `--choosedir FILE` has fm84 write the directory to `FILE` on quitting, for any other wrapper that wants it.

---

## 📀 Releases

Pre-built binaries beam down from the neon sky:

| Platform | Architecture | Format |
|----------|--------------|--------|
| 🐧 **Linux** | x86_64, ARM64 | `.tar.gz` |
| 🍎 **macOS** | Intel, Apple Silicon | `.tar.gz` |
| 🪟 **Windows** | x86_64 | `.zip` |

```bash
# 📥 Download from GitHub Releases
# https://github.com/coltwillcox/fm84/releases

# 🎮 Extract and run
tar -xzf fm84-v*.tar.gz
./fm84
```

*No cargo? No problem. Grab a binary and jack in.* 🔌

---

## 📦 Dependencies

- 🦀 **Rust** 1.93 or newer (2024 edition)
- 🖥️ **ratatui** - TUI framework
- ⌨️ **crossterm** - Terminal magic
- 🎨 **syntect** - Syntax highlighting
- 🖼️ **image** - Decoding pictures for the Viewer
- 🕐 **chrono** - Time vibes

---

## 🌃 Aesthetic

<div align="center">
<pre>
╔══════════════════════════════════════╗
║  VIOLET DREAMS • PURPLE HAZE • NEON  ║
║    ▓▓▓▓▓ SYNTHWAVE FOREVER ▓▓▓▓▓     ║
╚══════════════════════════════════════╝
</pre>
</div>

The default Synthwave palette channels pure 80s energy (F11 swaps it for any of 14 others, light ones included):
- 🖤 **Black background** - `#000000`
- 💜 **Violet borders** - `#743AD5`
- 🔮 **Purple selections** - `#9400D3`
- 💗 **Magenta directories** - `#FF00FF`
- 🩵 **Cyan accents** - `#00FFFF`
- 💙 **Soft purple files** - `#7289DA`

---

## 🎵 Inspired By

- 🌅 FM-84 (the band, obviously)
- 🖥️ Midnight Commander
- 🎮 Total Commander
- 🌆 Synthwave aesthetics
- 📼 That retro terminal feel

---

## 📜 License

*Ride free through the neon grid.*

Released under the [MIT License](https://github.com/coltwillcox/fm84/blob/master/LICENSE) - use it, change it, ship it, keep the notice.

---

<p align="center">
  <strong>💜 FM84 💜</strong><br>
  <em>Where every file operation feels like a synth drop</em><br>
  <code>▀▄▀▄▀▄ v0.40.0 ▄▀▄▀▄▀</code>
</p>
