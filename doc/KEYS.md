# 🎹 FM84 - Keys

Every key, by where it works. [USAGE.md](USAGE.md) says what each does in full; F1 lists them inside fm84.

## 🗂️ Panels

| Key | Action |
|-----|--------|
| `↑` `↓` `←` `→` | Navigate |
| `Home` / `End` | Jump to first / last item |
| `PageUp` / `PageDown` | Page navigation |
| `Tab` | Switch panels |
| `Enter` | Open directory, go into an archive, or open a file |
| `Shift+Enter` / `Ctrl+O` | Open as the system would, or with the program set for the extension (an archive too) |
| `Alt+O` | Open with: pick a program (`E` edits your own) |
| `Backspace` | Go to parent directory |
| `Ctrl+←` / `Ctrl+→` | Open the directory under the cursor in the left / right panel |
| `Ctrl+U` | Swap the panels |
| `Alt+I` | Same directory in the other panel |
| `Ctrl+L` | Go to a path (`Tab` completes a name; or click the path) |
| `Alt+←` / `Alt+→` | Back / forward through where the tab has been |
| `Alt+↓` | Every directory visited, across the tabs (type to narrow, `Enter` go) |
| `Ctrl+D` | Bookmarks (`A` add, `D` remove, `Enter` or `1`…`9` go) |
| `Alt+F1` / `Alt+F2` | Choose a drive for the left / right panel (Ctrl works too, or click an icon) |
| `Ctrl+T` / `Ctrl+W` | New tab / close tab |
| `Ctrl+PgUp` / `Ctrl+PgDn` | Previous / next tab |
| `Alt+1`…`Alt+9` | Go to tab 1…8, or the last |
| `[a-z0-9]` | Quick search |
| `Ctrl+S` | Filter the panel by a pattern, as it is typed (`Esc` puts back the one there was) |
| `Ctrl+B` | Flat view: every file under the directory in one list; again to go back |
| `Esc` | Clear search / close dialogs, the Viewer and the Editor / stop directory sizing / stop waiting on a directory that has not answered |
| `Ctrl+R` | Reload both panels |
| `Ctrl+F3` / `Ctrl+F4` / `Ctrl+F5` / `Ctrl+F6` | Sort by name / extension / date / size, again to reverse (or click a heading) |

## ✅ Selection

| Key | Action |
|-----|--------|
| `Space` / `Insert` | Select/deselect file |
| `+` / `-` | Select / deselect by pattern |
| `*` | Invert the selection of files |
| `Alt+*` | Invert the selection of files and directories |
| `Alt+Shift+Enter` / `Alt+Enter` | Size every directory in the panel |
| `Ctrl+C` / `Alt+C` | Copy full paths / names to the clipboard |

## 📝 File operations

| Key | Action |
|-----|--------|
| `F1` | Help |
| `F2` | Rename; with a selection, rename them all by a pattern |
| `Shift+F2` | Compare directories: select what differs in each panel |
| `F3` | View file |
| `F4` | Edit file (built-in, or the editor set under F11) |
| `Shift+F4` | Create empty file |
| `F5` | Copy (to the other panel, or wherever the dialog is told) |
| `F6` | Move (to the other panel, or wherever the dialog is told) |
| `Alt+F5` / `Ctrl+Shift+F5` | Make a symbolic link (in the other panel, or wherever the dialog is told; `Tab` for a hard link) |
| `F7` | Create directory |
| `F8` / `Delete` | Move to trash (selected items or cursor item) |
| `Shift+F8` / `Shift+Delete` | Delete permanently |
| `F9` | Open terminal (the one set under F11, if any) |
| `F10` | Quit (twice during an operation; asks first about unsaved edits) |
| `Shift+F10` | Quit, leaving `cd84` in the directory it started from |
| `F11` | Options - panels, behaviour, appearance, viewer and editor |
| `F12` | Preview cursor file in other panel |
| `=` / `Shift+F3` | Compare two files side by side |
| `Alt+A` | Change permissions, owner, group and modified time |
| `Alt+F7` / `Ctrl+F7` | Find files by name, content or regex, size and date (`F` on the results feeds them to the panel) |
| `Alt+S` | Synchronize the panels' directories (`→` `←` copy, `Del` delete, `Space` leave, `>` `<` mirror, `R` reset, `Enter` run) |
| `Alt+F9` / `Ctrl+F9` | Extract the whole of an archive |
| `Alt+P` | Pack what is chosen into an archive (`Tab` changes the format) |
| `Alt+H` | Checksums of what is chosen (`Tab` / `Shift+Tab` the algorithm, `C` copies, `Enter` writes), or check a checksum file |
| `Alt+M` | User menu: run commands of your own (`E` edits them) |

## ⏳ While a job runs

| Key | Action |
|-----|--------|
| `Esc` | Cancel a running copy, move or delete |
| `Y` / `N` / `K` | Overwrite / skip / keep both, when a file is already there |
| `A` / `S` / `B` / `O` | Overwrite all / skip all / keep both all / overwrite all older |
| `R` / `S` / `A` | Retry / skip / skip all, when an entry fails |
| `D` | Delete permanently, when the trash will not take an entry |

## 🎨 Viewer

| Key | Action |
|-----|--------|
| `X` | Toggle hex view; cycle Image, Text, Hex for images |
| `F` | Fit or fill an image |
| `+` / `-` | Zoom an image in or out |
| `W` | Wrap long lines, or scroll them sideways |
| `PageUp` / `PageDown` | Previous / next picture in the folder |
| `Drag` | Move a picture about |
| `Ctrl+C` | Copy selection |

## ✍️ Editor

| Key | Action |
|-----|--------|
| `Shift`+arrows | Select text |
| `Ctrl+A` | Select all |
| `Ctrl+X` / `Ctrl+C` / `Ctrl+V` | Cut / copy / paste |
| `Ctrl+Z` | Undo |
| `Ctrl+Y` / `Ctrl+Shift+Z` | Redo |
| `F2` / `Ctrl+S` | Save |
| `Ctrl+R` / `F4` | Replace (`Y` replace, `N` skip, `A` replace all) |
| `Tab` / `Shift+Tab` | Indent / unindent the selected lines |
| `Ctrl+/` / `Ctrl+_` / `Ctrl+7` | Comment or uncomment the selected lines |
| `O` / `R` | Overwrite / reload, when the file changed on disk since it was opened |

## 🔎 Viewer and Editor

| Key | Action |
|-----|--------|
| `Ctrl+F` | Find |
| `F3` / `Shift+F3` | Next / previous match |
| `Alt+R` | Text or regular expression, in the Find and Replace prompts |
| `Ctrl+G` | Go to line |

## ⚖️ Compare files

| Key | Action |
|-----|--------|
| `F3` / `N` | Next difference |
| `Shift+F3` / `P` | Previous difference |

## 🖱️ Mouse

| Action | Effect |
|--------|--------|
| Scroll | Scroll content (panels, Viewer, Editor) |
| Click / drag a scroll bar | A page that way / follow the pointer (Viewer, Editor) |
| Double-click | Open directories and archives, or open files as Enter does |
| Click a column heading | Sort by it, again to reverse |
| Click the path | Edit it in place, as Ctrl+L |
| Click a drive icon | Send the panel to that drive |
| Click the git branch | List the repository's branches, to switch to another |
| Click an F-key | Press it |
