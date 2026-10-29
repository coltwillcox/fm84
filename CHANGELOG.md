# 📼 Changelog

All notable changes to FM84 will be documented in this file.

*The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).*

---

## [0.33.1] - 2026-10-29

### 🐛 Fixed
- 🔼 **A file named `...`** was taken for the `..` row above it, whose name its stem matched: Enter on it went up a directory, and it could not be selected, renamed, copied, moved or deleted. The parent row is told apart by the name on disk
- 🪟 **Opening a file on Windows** went through `cmd /C start`, which read the path as a command line: a file named `x&calc` ran calc, and `%VAR%` in a name was expanded. Files are handed to the shell directly
- 🔗 **Space on a link to a directory** counted the size of everything it pointed at - all of `/`, or a slow mount, for a link to one. It is selected and left uncounted, as Alt+Shift+Enter already left links out

---

## [0.33.0] - 2026-10-25

### ✨ Added
- ┃ **Scroll bars in the Viewer and the Editor** - drawn into the frame's border, down the right for the lines and along the bottom for the columns, each only when there is more than fits, so the text keeps every column and row it had. A click on the track scrolls a page that way; the thumb can be dragged, and follows the pointer, off the bar too, until the button comes up. In the editor they move the view as the wheel does, leaving the cursor where it is
- 🎨 **Scroll bar style in F11** - Heavy `┃━` (the default, the border's own line drawn thicker), Double `║═`, Dashed `╏╍`, Block `█`, Shade `▓` or ASCII `#`, in the title's colour. Saved as `scroll_bar` in the config
- 🔀 **Switch git branches** - a click on a panel's branch lists the repository's local branches, the one checked out marked. Enter, or a click on one and a second on it, switches there with `git switch`, and both panels are read again. When git refuses - edits the switch would lose - its reason is shown and nothing changes. Git runs on a thread of its own: the list is waited for two seconds at most, and a switch that takes longer is taken in when it is done
- 🗂️ **Size every directory at once** - Alt+Shift+Enter, as in Total Commander, or Alt+Enter where the terminal cannot report Shift with Enter. Four are counted at a time, top to bottom, and the rest show `…` while they wait. A directory counted before is counted again, and shows its running count until it is done. Links to directories are left out, since one to / or a slow mount would walk all of it. Esc stops it, and so does leaving the directory, unless the other panel is in it

### 🐛 Fixed
- 🌈 **Stale lines in the editor** - pasting over a selection as many lines long, or undoing Replace all, left the lines after the first showing the text they had before, though the file held the new. Every line an edit rewrites is drawn again
- 📂 **A file vanishing from a panel** - a slow reread still on its way could land after a newer one and take back what it showed: a file a copy had just made disappeared, and stayed gone until something else changed in the directory
- 🈳 **Wide characters** - CJK text and emoji take two columns, and were counted as one: in the editor the cursor could go off the right edge and a click picked a character twice as far along; in the Viewer a click did the same, and Ctrl+F left a match after them off screen
- ⚖️ **The diff's headings** cut the last three characters off a file's name when its path had to be shortened
- 🔢 **Sizes just under a unit** - 1 048 575 bytes read `1024 KiB`; a size is rounded before its unit is chosen, so it reads `1 MiB`
- 🦀 **A long name of emoji** - copying or saving a file whose name was 63 four-byte characters failed with "File name too long": the temporary written beside it took 64 characters of the name, not 64 bytes

### 🛠️ Changed
- ⏩ **Space on a directory goes first** - sized ahead of any waiting from Alt+Shift+Enter, since it was asked for on its own

---

## [0.32.1] - 2026-10-24

### 🐛 Fixed
- ⌨️ **Alt+\* in kitty** - with kitty's keyboard protocol on, a chord with Shift in it came as the key unshifted: Alt+\* arrived as Alt+8, inverted nothing, and with eight tabs open went to the eighth. The shifted character is asked for as well, so it inverts the selection again
- 📜 **Jumping to a difference near the end of a diff** scrolled past the last row, leaving the bottom of the view empty, and the next ↑ jumped back. The last row now stays at the bottom
- 📜 **F1 after the window grew** - scrolled to its end and then given more room, it took several presses of ↑ before it moved. It keeps to what the room allows
- ▏ **The cursor behind a question** - over the editor, Save changes?, the file changed on disk and the large-file question left the terminal's cursor blinking in the text beside them. It shows in the editor only with nothing over it

### 🛠️ Changed
- 🗂️ **The source split into modules** - the six largest files, the biggest of them nearly 6,000 lines, are folders of files a concern each: `app`, `ui`, `input`, `fs_ops`, `viewer` and `options`, with their tests beside them by topic. Nothing changes in how fm84 behaves

---

## [0.32.0] - 2026-10-24

### ✨ Added
- 📜 **F1 scrolls** - the arrows, PageUp/PageDown, Home/End and the mouse wheel move through it when it is longer than the panels, and the corner says which lines show. It opens at the top
- 💡 **About in F1** - after the keys: the version, the description, what fm84 is built with, the copyright, the licence and the repository. The version, description, licence and address come from Cargo.toml, so they follow each release
- 🖱️ **The wheel in F11** - scrolls the options list as ↑/↓ do, except while a command is being typed into it

### 🛠️ Changed
- 📐 **One width for every popup** - 60% of the screen, as F1 and F7 always were. Copy, Move and Symlink, Find files, Alt+A, Bookmarks and the questions asked during a copy were 64-80%
- 📏 **Popups keep to the panels** - every popup is centred over the panels and no taller than they are, so the top bar, the path bar, the status bar and the key bar always show. F11 used to run over them on a short terminal; it scrolls within the panels instead
- 🗂️ **F1 in two parts** - Keys and About, each under a heading, as F11 groups its options

---

## [0.31.0] - 2026-10-24

### ✨ Added
- 🔗 **Symbolic links** - Alt+F5, or Ctrl+Shift+F5 as in Total Commander where the terminal lets it through (kitty keeps it for reloading its config), makes a link to what is chosen. It asks where, as F5 does: for one entry, file or directory, its name in the other panel, ready for another; for several, the other panel's directory, with a link in it for each under its own name. Each points at the absolute path of what it links to. Nothing already there is written over: an entry that cannot be linked is named, and the rest are still made. On Windows it takes Developer Mode or the right to make links
- ⚖️ **Compare directories** - Shift+F2, as in Total Commander, selects in each panel what differs from the other: an entry only it has, and of a file both have, the newer one, or both when the times agree and the sizes do not. Times within two seconds count as the same, as FAT keeps them no finer. Directories both have are not gone into. What was selected before is replaced, ready for F5 to copy across, and the status bar says how many differ
- 🪞 **Compare files** - `=` or Shift+F3 shows two files side by side: two selected in the panel, or the file under the cursor against the one of the same name in the other panel, or failing that the one under the cursor there. Between them, the marks `diff -y` uses - `|` changed, `<` only on the left, `>` only on the right - and the lines that differ shaded. F3 or N goes to the next difference and Shift+F3 or P to the previous; the arrows, the page keys and the wheel scroll both sides together. The diff is Myers', the one `diff` and git use, on lines, written for fm84 rather than brought in. Files that are the same, that differ only in line endings, or that are not text are only spoken of, in the status bar - for binaries, the first byte that differs
- ▏ **Cursor shape** - F11 sets the cursor in every field and in the editor to a Bar, a Block or an Underline, blinking or not, or to Classic, the reversed cell fm84 drew before, for a terminal that cannot be told a shape. Saved as `cursor` and `cursor_blink`; on exit the terminal gets its own shape back

### 🛠️ Changed
- ▏ **The terminal's own cursor** - fields and the editor show the terminal's cursor, a blinking bar unless F11 says otherwise, where they drew a reversed cell of their own. With the cursor at the end of what is typed, it sits straight after the last character: a directory being renamed reads `[doc]`, not `[doc ]`
- ⌨️ **Kitty's keyboard protocol** - asked for where the terminal has it (kitty, foot, WezTerm, Ghostty, Alacritty), and given back on exit, around the external editor and after a crash

### 🐛 Fixed
- 🎹 **Ctrl+F3, and Shift+F3 in the Viewer and the Editor, never arrived** - most terminals send F3 with a modifier in the form they report where their cursor is, and it was taken for that and dropped. With kitty's keyboard protocol they come through; elsewhere they still cannot, which is why Compare files has `=` as well

---

## [0.30.0] - 2026-10-23

### ✨ Added
- 🔐 **Change attributes** - Alt+A, as in Total Commander, changes the permissions, owner, group and modified time of what is selected, or of the entry under the cursor. Each field opens on what the entries have in common, or empty where they differ, and one left as it opened, or emptied, changes nothing. The mode is octal (`755`, `4755`) or as `chmod` takes it (`u+x,go-w`, `a=rX`, `+t`), so entries whose modes differ can each gain or lose a bit; with one entry, the `rwxr-xr-x` it comes to is shown as it is typed. Owner and group go by name or number, and the time as `2026-10-23 14:02:11`. Something typed that cannot be read is said so before anything changes, and the form stays to fix it; an entry that refuses is passed over and named, and the rest still change. Tab, or a click on a field, moves between them, a click putting the cursor where it lands. Links are followed, as `chmod` and `chown` follow them. On Windows only the time
- 🌿 **Git branch** - after a panel's path, the branch its directory is on when it is in a repository, or the commit when none is checked out. Read from the repository's own files - worktrees and submodules too - on the thread that reads the directory, so there is no git to run and nothing to wait for. It is the first to give way when the path needs the room
- 👤 **user@host** - who fm84 runs as and where, by the logo with the border between, reversed when it is root. Left off on a terminal too narrow for it

### 🐛 Fixed
- 🎹 **F3 Next showed over a picture**, and over the notice F4 raises on a binary file, where there is nothing to find. It shows only while there is text to search

---

## [0.29.0] - 2026-10-23

### ✨ Added
- 🔖 **Bookmarks** - Ctrl+D lists the directories kept to go back to. Enter, or 1 to 9, takes the active panel there; A or Insert keeps the panel's directory, after the one highlighted, and only highlights it when it is kept already; D or Delete forgets one, and Shift+Up and Shift+Down move it in the list. The panel's own directory is marked, and the list opens on it. One gone since is said to be, as Ctrl+L says, and the panel stays put. They are kept in `bookmarks` beside the config, a `bookmark = path` line each, read afresh each time so a hand edit shows
- 🔃 **Sorting from the panel** - Ctrl+F3 sorts by name, Ctrl+F4 by extension, Ctrl+F5 by date and Ctrl+F6 by size, as in Total Commander, and a click on a column heading does the same. The key they are sorted by already turns the order round; another starts ascending. The heading sorted by carries ↑ or ↓, and both panels follow, kept as F11 keeps it
- 🔁 **Swap the panels** - Ctrl+U, as in Midnight Commander: each takes its tabs, cursor, selection and history to the other side, and the focus goes with the one it was in
- 🪞 **Same directory in the other panel** - Alt+I sends the other panel to the active one's directory, its cursor on the same entry
- 📋 **Copy paths and names** - Ctrl+C puts the full paths of what is selected, or of the entry under the cursor, on the clipboard, a line each, and Alt+C the names alone; on `..`, the panel's own directory. It goes over OSC 52, and the built-in editor pastes it either way. The status bar says what was copied until the next key
- 🖱️ **A click on a panel's path edits it**, as Ctrl+L does, in that panel. A click anywhere else puts the field away, as Esc does, and is still the click it was
- 🎹 **The key bar answers clicks** - a click on a label along the bottom presses that key, F10 quitting as the key does

### 🛠️ Changed
- 🎹 **The key bar shows what the screen answers** - in the Viewer F3 and F10, in the Editor F2, F3, F4 and F10, rather than the panels' twelve everywhere. F8 is named Trash or Delete as F11 has it, and F7 is Mkdir, Shift+F4 being the one that makes files

### 🧪 Tests
- Tests never write the config, session or bookmarks files, so one that changes an option cannot touch the config of whoever runs them
- Sorting, bookmarks, swapping the panels, copying paths, and clicks on paths, headings and the key bar

---

## [0.28.0] - 2026-10-13

### ✨ Added
- 🧭 **Go to path** - Ctrl+L edits the active panel's path in place, in the line above it, and Enter goes there. Tab fills out a name as far as the entries agree, smart case, offering hidden ones only when they are shown or a dot asks for them; it reads on a thread of its own, so a mount that does not answer leaves Tab doing nothing rather than hanging. `~` is home, a path without a leading `/` starts from the panel, and `..` is worked out as `cd` does. A file is landed on, in the directory it is in. Somewhere that is not there is said to be, and the panel stays put, rather than climbing to the nearest parent as it does for a directory gone from under it
- ⏪ **Back and forward** - Alt+← and Alt+→ step through where the tab has been, as in a browser, landing on the entry the cursor was on. Each tab keeps its own, up to 50 steps back, and a new one starts with none. Every move counts - Enter, Backspace, Ctrl+←/→, a drive, Find files and Ctrl+L - but not a reread or a tab switched to, and a step is kept only once the panel gets there. A directory gone since is passed over, and one that cannot be read is dropped

### 🎨 Changed
- ✏️ **Every input field in the rename colours** - F5 and F6, F7 and Shift+F4, Alt+F7, the select, find, go to line and replace prompts, and a command typed in F11 were each drawn in colours of their own, some barely set apart from what was around them. They now all look like a name being renamed with F2, which every theme is checked to keep readable

---

## [0.27.0] - 2026-10-13

### ✨ Added
- 📖 **A man page** - `doc/fm84.1`: the command line, every key, the options, the files and environment fm84 reads, and its exit status. `man ./doc/fm84.1` reads it from a checkout, and the release archives carry it beside the binary. A test keeps its version in step with the program's
- 📦 **On crates.io** - `cargo install --locked fm84` builds and installs it, and the README says so

### 🧪 Tests
- Sizes, thousands, names and paths as they are drawn, find's matching and case folding, globs, and the Options dialog driven from the keys

---

## [0.26.2] - 2026-10-12

### 🐛 Fixed
- ⚙️ **The Options list went round** - Up on the first row jumped to the last, and Down on the last to the first. The highlight now stops at either end

---

## [0.26.1] - 2026-10-12

### 🐛 Fixed
- 🔭 **Find files could wait for good** - searching for text from `/` went into the system's own trees, and a file there - a USB-C port's state under `/sys` - never answered a read. `/proc`, `/sys`, `/dev` and `/run`, all but `/run/media`, are now left out unless the search starts in one
- 🔁 **Replace could go on for ever** - started partway through a file, with a replacement holding what it replaced (`let` to `/* let`), it went round the end a second time into the text it had put in. Replace all hung, and Y kept asking. It stops once round now, whatever is left
- 📜 **Wrapping a very long line slowed the viewer to a crawl** - every row laid out and searched the whole of its line, so a megabyte-long line wrapped onto fifty rows was gone through fifty times a frame: a tenth of a second for each key, a third with matches marked. Each line is now laid out once a frame, about 5 ms
- ⏱️ **Replace all in a highlighted file was slow** - each replacement highlighted its line again, so 60,000 took over a second. It now highlights once, at the end, in about a third of that
- 🔤 **Find files listed capitals first** - names were put in byte order. They now come as the panels list them, ignoring case unless F11 says otherwise
- 🧭 **F5, F6 and Alt+F7 used the directory a panel was leaving** - while it was still reading a slow one it was going to, they offered the old one. They now go by where it is going
- 🔗 **A move onto another name for the same file left it behind** - merging into a directory holding a hard link to the file being moved, the source stayed, and its directory would not go as not empty. The old name is taken away now, as `mv` does, without asking about writing over it

---

## [0.26.0] - 2026-10-12

### ✨ Added
- 🔭 **Find files** - Alt+F7, or Ctrl+F7 where the desktop takes Alt+F7, searches down from the active panel's directory by a name pattern as `+` takes it (`*.rs;*.toml`, `*/` for directories) and, if asked, for text in the files, lower case matching either case. It runs on a thread of its own: results arrive as they are found, with where it is looking, and Esc stops it. Directories each list straight after their parent, in name order; links to directories are not followed, hidden entries are passed over unless F11 shows them, files are read 64 KiB at a time, pipes and devices are never opened, and directories that cannot be read are counted rather than stopping it. Enter takes the panel to the result chosen, cursor on it, and the last search is offered again
- 🔁 **Replace in the editor** - Ctrl+R or F4 asks what to replace, offering the selection or the last thing looked for, and what with; then it goes from the cursor a match at a time - **Y** replace, **N** skip, **A** all the rest, **Esc** stop - round the end of the file and back to where it began, saying how many it replaced. It never looks inside the text it has just put in, so `a` to `aa` ends. Enter is not an answer, and Replace all is one step for Ctrl+Z
- 📜 **Wrap in the viewer** - W breaks long lines onto rows, after a space where there is one, and scrolling goes by rows, so a line longer than the screen can be read to its end. Finding, selecting and copying still go by whole lines - a copy gains no breaks, and a word split across rows is still found - while line numbers and Go to line count lines. The next file takes up W as the last was left

### 🛠️ Changed
- 🛑 **The editor asks before saving over changes made elsewhere** - a file written by something else since it was opened, as its time or size shows, is not saved over without a word: **O** overwrites, **R** reloads and drops the edits, **Esc** goes back to them. A save on the way out carries on to closing, or quitting, once answered. A file deleted in the meantime is said to be, and O saves it again

---

## [0.25.0] - 2026-10-12

### ✨ Added
- 🎯 **F5 and F6 ask where to** - the dialog has a field for the destination, filled in so Enter alone does what it always did: for a single file its path in the other panel, with the cursor at the end of the name to change it; for several entries, or a directory, the other panel's directory. What is typed is read as `cp` and `mv` read it - into a directory that is there, or one written with a `/` on the end, under each entry's own name; anything else is a new name for the one entry, in a directory that has to be there. A path that is not absolute starts from the panel you are in, so a bare name copies beside the original, and F6 with one renames it. `~` is home. Y and N now type into the field; Enter and Esc answer it
- ♻️ **Files already there are asked about one at a time** - as the copy or move reaches each, with both shown: size, date, and which is newer. **Y** overwrites it, **N** skips it, **A** overwrites it and every one after, **S** skips them all, **O** overwrites only those older than what would replace them, and **Esc** stops the job. Enter is not an answer, so a key pressed out of habit cannot write over a file. Directories are still merged, and the files inside them asked about. What was skipped stays where it was, on both sides, and the end says how many

### 🛠️ Changed
- ❓ **Ask, under F11's When destination exists, asks per file** - it used to ask once, before the job, for every name taken at the top; files in a directory being merged were written over without a word. Overwrite and Refuse are as they were

---

## [0.24.1] - 2026-10-10

### 🛠️ Changed
- 📦 **Smaller release binaries** - built with link-time optimisation, as one codegen unit and stripped of symbols, the binary comes down from 5.6 MB to 3.5 MB on Linux x86_64
- 🦀 **Rust 1.88 or newer to build** - said in the README and in `Cargo.toml`, where cargo checks it, rather than found out from a compile error. The crate now carries a description, keywords and categories, and leaves the screenshots out

---

## [0.24.0] - 2026-10-10

### ✨ Added
- ♻️ **F8 moves to the trash** - files and folders go to the system's trash, from where they can be put back. Shift+F8 (or Shift+Delete) deletes for good, and always asks first, whatever Confirm delete says. What the trash will not take - a read-only disk, one with nowhere to keep a trash - is asked about like any other failure, with D to delete it for good. **Delete to trash** under F11 turns it off

### 🛠️ Changed
- 🐌 **A dead network mount no longer freezes fm84** - reading a panel's directory, the once-a-second check for changes and free space, the drive list, and the detail lines and preview that follow the cursor all touched the disk on the thread that draws and reads keys, and on a mount that had stopped answering each one waited, with Esc and F10 unheard. They run on threads of their own now. On a disk that answers nothing looks different; on one that does not, the panel keeps showing what it did, the status bar says it is still reading, Esc stops waiting, and the other panel goes on working
- 🕰️ **Copies keep their modified times** - files and directories alike, and so does a move to another disk, which used to come out dated the moment it landed
- 🧾 **Popups wrap long lines** - an error naming a long path, or the names of a dozen files being copied, wrap at their spaces instead of running off the edge. Too many to fit are cut short with `…`, so the line saying which keys answer still shows
- 🖱️ **Only the left button clicks** - the right one moved the cursor as if it were the left
- 📅 **The detail line's date follows F11's date format**, ISO included

### 🐛 Fixed
- 💾 **F10 in the editor threw away unsaved edits** - it now asks Save, Discard or Cancel first, and leaves once answered
- 🔤 **A text file with a stray non-UTF-8 byte past its first 512 refused to open** - the viewer shows it, with a placeholder for the byte; the editor says it is not UTF-8 text, rather than opening it with the byte replaced for good
- 🧩 **A copy cut short left half a file under the real name** - each file is written beside its destination and renamed into place once whole, so a cancel, a failure or quitting mid-copy leaves nothing that passes for a finished copy
- ✅ **A mouse click cleared the selection in both panels** - clicking to make the other panel active lost a selection built by hand
- 🚧 **Keys reached the panel behind an error** - Space selected and letters searched in a panel the error covered. Esc or Enter puts it away, and nothing else gets past
- 📑 **Switching to a tab whose directory had become unreadable took over another tab** - the panel stayed put under the new tab's name. It is refused now, and the tabs stay as they were
- ↔️ **Lines past 65,535 columns scrolled back to the start** in the viewer and the editor - a minified file has them
- 🖼️ **The large-file question while stepping through pictures could only be answered with Esc**, which closed the viewer too
- 🪟 **Windows: a link to a directory would not delete**, directory symlinks and junctions alike
- ⚙️ **The config and session files could be left empty** by a crash or a full disk while they were written - they are written beside and renamed into place, like a save

---

## [0.23.0] - 2026-10-09

### ✨ Added
- 📑 **Tabs** - each panel keeps its own, in the row under its drives. Ctrl+T opens one beside the current tab in the same directory, Ctrl+W closes it (a panel keeps its last), Ctrl+PgUp and Ctrl+PgDn go round them, Alt+1 to Alt+8 go to that tab and Alt+9 to the last. Click a tab to show it, middle-click to close it. Each remembers its directory, cursor and selection, and with Remember directories on they all come back next time - a session from an earlier version still reads, as one tab a panel
- 🎯 **Select by pattern** - + and - select and deselect by a pattern such as `*.jpg;*.png`, with `*`, `?` and `[a-z]` as in the shell and lower case matching either case. Patterns pick files; one ending in `/` picks directories. * inverts the selection of files and Alt+* of directories too. Once a quick search is under way, - types into it as before
- ↔️ **Ctrl+Left and Ctrl+Right** open the directory under the cursor in the left or right panel, keeping the focus where it is; on a file, the directory it is in, on the file

### 🛠️ Changed
- 💿 **The drive strip scrolls when the drives do not all fit** - it was cut off at the edge of its half, leaving the drives past it out of reach of a click. It now shows as many as fit, with `<` and `>` at the ends while there are more that way, moved a page along by a click or the wheel, and follows the panel's drive and the one being chosen with Alt+F1 or Alt+F2

---

## [0.22.0] - 2026-10-02

### ✨ Added
- 🔎 **Find in the viewer and the editor** - Ctrl+F asks what to look for, offering the last thing looked for so Enter alone repeats it. F3 goes to the next match and Shift+F3 to the previous one, round the ends of the file, saying so when they wrap and when there is nothing to find. Every match on screen is underlined and the current one selected - ready for Ctrl+C in the viewer, and replaced by typing in the editor. A term all in lower case matches either case; one with a capital in it matches exactly. In hex view it searches the dump as it is drawn
- 🔢 **Go to line** - Ctrl+G and a number, in the viewer and the editor. Past the end goes to the last line
- ⌨️ **Command line** - `fm84 [LEFT [RIGHT]]` opens the panels in those directories, ahead of any remembered ones; `--help` shows the usage and where the config lives, and `--version` the version, both without needing a terminal. An unknown option or a directory that is not there is said in the shell, with exit status 2, before anything starts

### 🛠️ Changed
- ⎋ **Esc closes the viewer and the editor**, and F3 and F4 no longer do - F3 is the next match now, and closing on the key that opened it meant one press too many shut the file. The editor still asks about unsaved changes on the way out

---

## [0.21.0] - 2026-10-01

### ✨ Added
- 🧯 **A copy, move or delete asks when an entry fails** rather than stopping at it - a socket or a pipe in a tree being copied, a file that will not delete, a disk that fills. The job waits on **R** retry, **S** skip, **A** skip all or **Esc** abort, and says at the end how many were skipped. What is skipped stays where it was, and so does each directory holding it, so a delete keeps the path to it and a move keeps the source
- 🧵 **Directory sizes are counted in the background** - Space on a directory used to walk it before the interface could answer again, which on `/`, a large tree or a network mount meant seconds of nothing, with Esc unheard. The Size column now shows the running total with a `…` while it counts, several can count at once, and Esc stops them. Anything unreadable inside counts as nothing rather than failing the whole size, the way `du` carries on

### 🐛 Fixed
- ✅ **Selecting a file no longer selects another that reads the same** - selections were kept by the name as shown, and two names that are not valid UTF-8 can show alike. Picking one picked both, and F8 deleted both
- 🔤 **The rest of the lossy names are gone** - the external editor and the F9 terminal were handed the name as shown, opening a file that was not there; the cursor landed by it after a reread; remembered directories were saved by it and lost spaces at either end. F2 then Enter on an unchanged non-UTF-8 name renamed the file to its own lossy reading
- 💾 **Saving in the editor cannot destroy the file** - it was emptied the moment the save began, so a full disk or a crash partway left a truncated file. The new content goes beside it and replaces it whole once written. Through a symlink, the file it points at is saved; a read-only file is still refused
- 🔒 **A directory you cannot read no longer brings its error back every second** - entering `/root` moved the panel there while it went on listing the directory it came from, and the refresh found it changed each tick and raised the error again after every Esc. It is now refused, and the panel stays put
- 🚚 **A move merging into a directory renames each entry** rather than copying everything and then deleting the source - instant on one filesystem, and across them each source goes only once its copy is whole

### 🛠️ Changed
- 📜 **Released under the MIT License**, with the notice in every release archive

---

## [0.20.0] - 2026-09-29

### ✨ Added
- 🔢 **The preview shows a binary as a hexdump** rather than the words "Binary file" - the head of it in hex with the printable characters beside it, which is enough to tell a PNG from a zip or spot an ELF header without opening anything. The row is laid out to the pane: sixteen bytes where there is room, then eight, then eight without the offset, then four, so it still says something in half of an 80-column terminal. Built from the same row the viewer's hex mode uses

### 🐛 Fixed
- 🖼️ **Pictures are no longer stretched vertically** - the viewer worked out how many rows a picture came to by assuming every terminal's cells were exactly twice as tall as they were wide. Real ones are not: Victor Mono at 13pt in kitty measures 9x25, which drew every picture 39% too tall. The cell is now measured from the size the terminal reports in pixels beside its size in cells, and a 2:1 image comes out 2:1 on screen. A terminal that reports nothing, or something that is plainly not a cell, still gets the old assumption
- 🔤 **Files whose names are not valid UTF-8 can be used again** - a name is bytes on Unix, and one from an old archive, a foreign disk or a Windows-formatted stick often is not text this machine can read. Such a name was kept only as `to_string_lossy` left it, replacement characters and all, and every path built from it named a file that was not there: the entry listed with an empty Name column, and viewing, editing, entering, renaming, copying, moving and deleting all answered "No such file or directory". Entries now carry the name the filesystem gave, which every operation is built from, and the name shown falls back to a lossy reading of it rather than to nothing
- 🛡️ **A file name can no longer drive your terminal** - names went to the screen as they were, and a name can hold anything but `/` and NUL. One carrying an escape sequence recoloured the display and shifted every column after it; one holding a tab moved the cursor and let the row beneath bleed through. Control characters in a name are now drawn as dots, as the viewer has always drawn them in a file's contents, wherever a name appears: the rows, the detail lines, the preview, the titles, the path bar and every popup
- 🔄 **The preview and the detail lines follow a file that changes under the cursor** - both held what they gathered until the cursor moved to something else, so a file saved in the editor, written over by a copy or changed by another program went on showing its old size, timestamp and first lines while the panel beside them showed the new ones. A reread now gathers both again

---

## [0.19.0] - 2026-09-29

### ✨ Added
- 🔎 **Two lines of detail under the panels**, in the space the F-key labels leave empty - the whole name however long the column had to clip it, the exact byte count with its thousands marked off rather than a rounded "9 MiB", the timestamp down to the second, `owner:group`, the permission bits, and where a symlink points, which was shown nowhere before. Gathered when the cursor moves rather than while drawing, so an unresponsive mount cannot hold up a frame
- 📄 **Page Up and Page Down step through a folder's pictures** from inside the viewer, wrapping round at either end. Anything that is not a picture is stepped over, and what counts as one is settled by reading each file's header rather than by its name
- ⚡ **The pictures either side are read ahead** while one is open, so stepping through a folder does not stop to decode each one - a 27-megapixel photograph took about 50 ms to open and now arrives already decoded. The one just left is kept as it is. F11's new **Read pictures ahead** turns it off; on by default
- 🖱️ **Drag a picture about with the mouse** in the viewer - it follows the pointer the way a sheet of paper follows a finger, moving both ways at once, which the arrow keys cannot. Each drag is measured from where the press landed, so a long one cannot creep from rounding

### 🐛 Fixed
- 💿 **Removable drives mounted under `/run/media` now appear** in the drive strip. udisks2 puts a USB stick there, and the filter that keeps the kernel's own mounts out of the strip was throwing that whole path away with them
- ✏️ **F2 scrolls the rename field to keep the cursor in view** - typing or arrowing past the right edge of a long name left the cursor somewhere off the end of the box, so you were editing where you could not see

---

## [0.18.1] - 2026-09-29

### 🐛 Fixed
- 💾 **Overwriting a file no longer destroys it when the copy does not finish** - the copy wrote straight over the file it was replacing, so it was truncated the moment it began, and cancelling or running out of disk then removed what was left. A 11 MB file replaced by a 3 GB one and cancelled a quarter of a second in was simply gone, leaving neither the old nor the new. The bytes now go to a temporary beside it and a rename puts them in place at the end, so what is there is either the old file or the new one and never a half of either

### 🛠️ Changed
- 🏷️ **Release builds are pinned to fixed runner images** rather than the `-latest` labels, which move on their own - `ubuntu-latest` becomes Ubuntu 26 between 19 October and 19 November 2026. A Linux binary asks for the glibc of whatever built it, so an image moving underneath would quietly raise the minimum glibc for everyone downloading a release. The pins are what the labels point at today, so nothing about the builds changes

---

## [0.18.0] - 2026-09-29

### ✨ Added
- 🎨 **Coloured backgrounds behind image characters** - each cell now sits on its own colour rather than the terminal's, so the dark parts of a picture keep their colour instead of losing it through the gaps in the glyphs. Measured against the colour each cell ought to show, the error drops by about 80% on a photograph. The character is lifted off its own background - lighter on a dark cell, darker on a bright one - so it stays legible either way
- ⚙️ **Image backgrounds** under F11, on by default. Off sends less to the terminal, which is worth having over a slow link: with the theme painting its background the difference is about 16% of a frame, and about 56% when it is not, since nothing is sending a background per run in that case

---

## [0.17.0] - 2026-09-28

### ✨ Added
- ♻️ **Overwriting on copy and move** - a name already taken at the destination no longer stops the transfer outright. F5 and F6 ask first, naming what is in the way: yes replaces files and merges directories, replacing only what clashes inside them. F11's new **When destination exists** makes that Overwrite without asking, or Refuse as before
  - 🛡️ Refused whatever the setting: a file in place of a directory or the other way round, which would throw a tree away, and an item onto itself, which overwriting would empty before reading it
  - 🔗 A link at the destination is replaced, never written through into whatever it points at, and a read-only file in the way is replaced as `cp -f` would

### 🐛 Fixed
- 📏 **Saving in the editor updates the size the panels show** - rewriting a file leaves its directory's modification time alone, so the refresh that watches for changes never reread it, and the old size stayed until something else changed there

---

## [0.16.1] - 2026-09-28

### 🐛 Fixed
- 💥 **The editor no longer crashes after Ctrl+A and a delete** in a file longer than the screen - the scroll stayed at the end of what was now one line. The view also follows the cursor after a long paste, an undo or a redo, instead of leaving it off screen
- 🧊 **Pipes and devices no longer freeze the app** - with the preview on, passing the cursor over a named pipe hung fm84 for good, and F3 and F4 did the same; F3 on `/dev/zero` read until memory ran out. Only regular files are opened now, and copying a pipe or a device is refused rather than left stuck where Esc could not reach it
- 💾 **A failed save in the editor can be dismissed** - Esc went to the save prompt hidden behind the error, leaving discarding as the only way out. Esc or Enter now close the error and return to the edits, and answering the prompt with Y keeps the editor open when the save fails
- ✂️ **A copy that fails partway removes its partial file**, as a cancelled one already did, rather than leaving a truncated file that looks finished
- 📛 **Rename and create take a plain name** - `../x` or `sub/x` renamed or created in another directory
- 🔤 **A rename that only changes case** works on case-insensitive filesystems, where "readme" to "README" was refused as already existing
- 🚚 **A move falls back to copying only across filesystems** - error 17, taken for Windows' cross-device error, is "already exists" on Linux and macOS
- ↩️ **Keys that change nothing keep the redo history** - Backspace at the start, Delete at the end or Ctrl+X with nothing selected no longer add an empty undo step and clear redo
- 📂 **Directory sizes are forgotten after a copy, move or delete** instead of showing what a directory held before; a renamed directory keeps its size
- 🔐 **Copies keep their permissions**, so a script or a program stays executable - where the filesystem cannot hold them, as on exFAT, the copy still goes ahead

---

## [0.16.0] - 2026-09-28

### ✨ Added
- 🎨 **Eleven more themes** - Dracula, Monokai, Nord, Gruvbox Dark, Solarized Dark, Tokyo Night and Catppuccin Mocha, and four light ones: Solarized Light, Gruvbox Light, Catppuccin Latte and GitHub Light, each from its scheme's published palette
- 🖌️ **Themes paint their own background** - behind the panels, the popups and everything else, with a gutter to match in the viewer and the editor. F11's new **Background** row puts the terminal's back, for a transparent or custom one; a light theme needs its own
- 🌗 **Light themes turn the rest round** - syntax highlighting takes a light scheme (Solarized's own for Solarized, InspiredGitHub or base16-ocean light otherwise), extension colours darken, and pictures in the viewer draw dark ink on the light background instead of bright on dark
- 🧪 **Every theme is checked for contrast** - a test holds file names, directories, headings and the selected row's text to at least 3:1 against what they sit on, and every extension colour against every background

### 🛠️ Changed
- 🌌 **Synthwave, Outrun, Vaporwave and High contrast** now paint a background too - black for Synthwave and High contrast - where before they drew on the terminal's. Set **Background** to Terminal for the old look
- 💗 **Vaporwave's rename highlight darkened**, where the text on it was too faint; **extension colours** are a shade lighter on dark backgrounds, so the blues still read on Nord's
- 🖼️ **Transparent parts of a picture** are left blank, the viewer showing through, on light and dark backgrounds alike

---

## [0.15.0] - 2026-09-28

### ✨ Added
- ⚙️ **Options under F11** - what was an "Under construction" placeholder is now a list, grouped under Panels, Behaviour, Appearance and Viewer and editor, that you move through with the arrows and change with Enter, Space or Left/Right, saved as you go to `~/.config/fm84/config` (`$XDG_CONFIG_HOME`, or `%APPDATA%` on Windows). The file is plain `key = value` lines, and a line that makes no sense leaves that setting at its default rather than stopping fm84 from starting
  - 🙈 **Show hidden files** - dotfiles, or the hidden attribute on Windows
  - 🔃 **Sort by** Name, Extension, Size or Modified, ascending or descending, with the name settling ties. Extension ascending is the default, which is the order fm84 always used
  - 📁 **Directories first** - on, as always, or off to mix them in among the files
  - 🔠 **Case-sensitive sort** - off, as always, or on to put `README` before `apple`
  - 🧱 **Ext, Size, Modified and Attributes columns** - each can be turned off, leaving Name the room. With no Ext column, Name shows the whole file name, so `Cargo.lock` and `Cargo.toml` no longer read the same - which also covers a panel too narrow for it
  - 📅 **Date format** - `dd/mm/yy` as before, `yyyy-mm-dd`, or relative (`3 h ago`). Dates are now written out as they are drawn, so a relative one keeps up without a reload, and the Modified column sizes itself to the format
  - 📏 **Size units** - KiB (1024), as before, or kB (1000), everywhere a size appears
  - ✅ **Confirm copy and move** - off sends F5 and F6 straight to the transfer; the checks for an existing destination and for copying a directory into itself still run
  - 📝 **Editor (F4)** - an external editor, handed the terminal until it exits, after which both panels are reread. `{}` stands for the file, which otherwise goes last. Left empty, the built-in editor opens as before
  - 📍 **Remember directories** - reopen both panels where they were at the last quit, kept in `session` beside the config. A directory that has gone since opens its nearest surviving parent
  - 👁️ **Preview on startup** - start with F12's preview open
  - 🕐 **Clock** - 24-hour as before, 12-hour, or off
  - 🗑️ **Confirm delete** - off sends F8 straight to the delete
  - 🔣 **Icons** - Nerd Font, Nerd Font Mono, or Plain. Mono drops the blank cell each drive icon keeps for a wide glyph, which in a Mono font would push it off centre; Plain uses letters and `/` for a terminal with no Nerd Font at all
  - 💻 **Terminal (F9)** - a command of your own, split on spaces, with `{}` standing for the directory. Left empty, the terminal is picked as before
  - 🎨 **Theme** - Synthwave, as it always was, or Outrun (orange and hot pink), Vaporwave (pastels) or High contrast. It changes on the spot, behind the dialog
  - ⇥ **Tab width** - 2, 4 or 8, wherever tabs are expanded: the viewer, the editor, where a click lands in it, and the preview
  - 📊 **Line numbers** - off hides the gutter in the viewer and the editor, and clicks in the editor still land on the character under the pointer
  - 🌈 **Highlight files up to** 256 KiB, 512 KiB (the old fixed limit) or 2 MiB, or Off for plain text always
  - ❓ **Ask before opening over** 16, 64 (the old fixed limit) or 256 MiB
  - 🖼️ **Images open as** Fit, as before, or Fill
  - 📜 **The list scrolls** when the terminal is too short for all of it, keeping the highlighted row in view

---

## [0.14.2] - 2026-09-28

### 🛠️ Changed
- 💿 **The drive a panel is on sits in a block of colour** rather than brackets - drawn like the row under a panel's cursor, cyan on the inactive selection colour, turning to the active selection colour while that panel is choosing. The whole block is clickable, and every icon holds the same width open, so switching drives moves nothing but the block

### 🐛 Fixed
- 💿 **Drive icons sit centred** - Nerd Font glyphs draw two cells wide while counting as one, so each icon now keeps a blank cell after it to spill into rather than sitting half a cell to the right

---

## [0.14.1] - 2026-09-28

### ✨ Added
- ⌨️ **Home and End while typing a name** - in the rename box and the create box, which had arrows, Backspace and Delete but no way to reach either end in one go

### 🛠️ Changed
- 💿 **The drive a panel is on is bracketed** - `[ icon ]` rather than colour alone, so it reads at a glance and without relying on the palette. The brackets take room that every other icon holds open, so switching drives moves nothing but the brackets, and the whole bracketed cell is clickable
- 🖼️ **Screenshots sit in a grid** in the README rather than one above another

---

## [0.14.0] - 2026-09-25

### ✨ Added
- 🗑️ **Progress while deleting** - counted in entries removed rather than bytes, since deleting is about how many there are and not how big they are. Esc stops it, leaving whatever it had not reached; a large tree on a slow disk no longer holds the whole app still
- 🚪 **A way out of an operation that will not finish** - F10 during one offers to leave and a second F10 takes it. Cancelling is only noticed between entries, so a transfer stuck writing to a disk that has stopped answering would otherwise trap the app with no way out at all. The first press changes nothing about the operation itself: Esc is still what stops it

### 🐛 Fixed
- ♻️ **Copying a directory into itself** - the destination is created before the source is read, so the copy found it, descended into what it was writing, and went on until the path outgrew PATH_MAX. A 1 MiB directory became 1017 MiB across 1018 levels in about a second; large enough and it would have filled the disk. It is refused now, the way `cp` refuses it
- ⌨️ **Popups hiding the line that says which key to press** - placing each line with a deeper margin cost two rows apiece, and once a popup was shorter than that the last line was handed nowhere to draw and silently vanished. On a 24-row terminal that meant Delete asked "Delete 3 items?" with nothing on screen saying that Y confirms. Create needed 56 rows before its line appeared
- 👻 **Selections coming back on a file that only shares a name** - a selection is keyed by name so it survives a re-sort, but the name stayed behind when the file went, invisible until something else took it. That file was then selected without the user ever picking it, and since a selection is used *instead of* the cursor, one such name quietly redirected the next copy, move or delete

---

## [0.13.0] - 2026-09-25

### ✨ Added
- 📊 **Progress for copying and moving** - a bar with the current file, bytes done, total and transfer rate; Esc cancels and the half-written file is cleaned up, while everything already finished stays. The transfer runs off the interface thread, so a slow or stalled disk cannot freeze the display, and a copy that finishes at once never flashes a popup on the way past
- 🔍 **Zoom in the Viewer** - `+` and `-` scale a picture from a quarter of the fitted size up to four times it, in round steps shown on the status bar. Whatever is in the middle of the view stays in the middle, so zooming in on a detail keeps it in sight. `F` returns to a plain Fit or Fill
- 🖱️ **Clickable drive icons** - a click on the strip sends that panel to the mount and moves the focus there, alongside Alt+F1 and Alt+F2

### 🛠️ Changed
- ❓ **A picture is weighed by its pixels, not its file size** - the two part company entirely, and a 410 KB PNG that unpacks to 549 MiB now asks first. The dimensions are read from the header, without decoding a pixel
- 📐 **Fit fits at any shape** - a picture more than twice as tall as the Viewer per column of its width used to hand Fit something it could not show whole, needing ten screens of scrolling. It is squashed into the Viewer instead; at that ratio it is a single column of colour with no shape left to distort
- ⚡ **Filling an extreme aspect ratio stays quick** - a one-pixel-wide strip covering the pane meant a drawing 102,400 rows tall, half a second to build and 160 MB to hold, and again on every resize. The overflow is now held to a few screens each way

### 🐛 Fixed
- 🖱️ **The mouse no longer reaches what is behind a popup** - clicks and the scroll wheel moved the panels under every dialog, so a panel could be scrolled out from under the question being asked about it

---

## [0.12.1] - 2026-09-24

### 🛠️ Changed
- 🎨 **Lighter image frames** - colours are rounded to 32 levels per channel so neighbouring characters share one escape sequence, cutting a full-screen image from about 170 KB of terminal output to 110 KB; scrolling a filled image is quicker, most of all over SSH
- ⚡ **Dependencies are optimised in debug builds** - unoptimised, image decoding ran 10-30x slower, so `cargo run` took about half a second to open a photo; the first debug build takes longer, and release builds are unchanged

---

## [0.12.0] - 2026-09-24

### ✨ Added
- 🖼️ **Images in the Viewer** - PNG, JPEG, GIF, WebP and BMP drawn as ASCII art, each character coloured by its pixel; the format is detected from the content rather than the extension, and anything that fails to decode still opens in hex
- 🔁 **`X` cycles Image, Text and Hex** for images, and Text and Hex for everything else as before
- 📐 **Fit and fill** - `F` switches an image between fitting whole and centred in the Viewer, the default, and filling it with the overflow scrolled; redrawn whenever the terminal is resized
- 💿 **Drive switcher** - Alt+F1 and Alt+F2 (or Ctrl+F1/F2, since window managers often take Alt) pick a mount for the left or right panel from a strip of drive icons; arrows move, Enter jumps, Esc cancels
- 🔌 **Removable and optical mounts** are recognised on Linux and get their own icons
- 📄 **Shift+F4 creates an empty file**, refusing if anything already has that name
- ↪️ **Redo in the Editor** - Ctrl+Y or Ctrl+Shift+Z
- 🖱️ **Mouse selection** - drag to select in the Editor and the Viewer; Ctrl+C copies from the Viewer

### 🛠️ Changed
- 🚫 **Refuses to start without a terminal** - launched from a desktop entry, it now explains how to run it instead of failing with a bare `ENXIO`
- 🧹 **Switching the Viewer mode clears the selection**, which pointed at different content in the new mode

---

## [0.11.0] - 2026-09-23

### ✨ Added
- ✂️ **Selection in the Editor** - Shift with any cursor key extends a selection, Ctrl+A takes the lot; the selection is drawn over the syntax colours rather than replacing them
- 📋 **Cut, copy and paste** - Ctrl+X/C/V, plus the Ctrl+Insert, Shift+Insert and Shift+Delete aliases; typing, Backspace and Delete all replace a selection
- 🌐 **System clipboard** - copies are offered to the terminal with OSC 52 and pastes arrive through bracketed paste, so the clipboard works over SSH without linking a platform clipboard library
- ↩️ **Undo** - Ctrl+Z, 200 steps, one step per action rather than per internal operation, restoring the cursor along with the text

### 🛠️ Changed
- 🎨 **The cursor line keeps its syntax colours** - it was previously redrawn from the raw text
- 🛡️ **Control characters are neutralised in the Editor too**, closing the same escape-sequence hole the Viewer had

---

## [0.10.0] - 2026-09-23

### ✨ Added
- 💽 **Disk usage per panel** - a five-cell meter plus used/total for each panel's filesystem on the status line; read on the refresh tick, never in the render path, so an unresponsive network mount can't freeze the UI
- 🔑 **Attributes column** - permissions as `ls -l` writes them, setuid, setgid and sticky bits included, from metadata the listing already holds
- 🔢 **Hex view in the Viewer** - F3 on a binary shows a `hexdump -C` instead of refusing; `X` toggles hex for any file, which also makes a BOM or CRLF endings visible in a text file
- 👁️ **F12 Preview** - the head of the cursor file in the opposite panel, following the cursor; capped at 64 KiB so it stays cheap while arrowing through large files
- ⚙️ **F11 Options** - placeholder popup, under construction

### 🛠️ Changed
- 📐 **Columns drop by priority** - Name, Ext, Size, Modified, Attributes; Name is never dropped and now takes the leftover width instead of being squeezed to two characters at 80 columns
- 🔡 **F-key bar drops whole labels** when the terminal is too narrow, rather than clipping one mid-word
- 📊 **Status bars drop whole segments** for the same reason, instead of overrunning the closing corner

### 🐛 Fixed
- 🛡️ **Control characters are neutralised in the Viewer** - a text file containing escape sequences could clear the screen and erase the frame around it
- 🍎 **Disk usage on Apple platforms** - `statvfs` reports block counts through a 32-bit `fsblkcnt_t` there, truncating past about 16 TiB; the native `statfs` is used instead
- 🚪 **F4's binary notice** closes with F4, and F3 no longer closes it - each key now closes only what it opened

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
