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
| `Alt+J` | Jump to a directory gone to before, most used lately first (type a few letters, `Enter` go) |
| `Ctrl+D` | Bookmarks (`A` add, `D` remove, `Enter` or `1`…`9` go) |
| `Alt+F1` / `Alt+F2` | Choose a drive for the left / right panel (Ctrl works too, or click an icon) |
| `Ctrl+T` / `Ctrl+W` | New tab / close tab |
| `Ctrl+PgUp` / `Ctrl+PgDn` | Previous / next tab |
| `Alt+1`…`Alt+9` | Go to tab 1…8, or the last |
| `[a-z0-9]` | Quick search |
| `Ctrl+S` | Filter the panel by a pattern, as it is typed (`Esc` puts back the one there was) |
| `Ctrl+B` | Flat view: every file under the directory in one list; again to go back |
| `Ctrl+E` | The names as text in the editor: edit a line to rename, delete it to delete, duplicate it to copy; saving shows the plan (`Enter` runs it, `Esc` back to the lines) |
| `Ctrl+Z` | Undo the last rename, copy, move or move to the trash |
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
| `Alt+Shift+Enter` / `Alt+Enter` | Size every directory in the panel, and map the space each entry takes |
| `Alt+U` | The space map as a screen of blocks by size |
| `Ctrl+C` / `Alt+C` | Copy full paths / names to the clipboard |
| `Alt+B` | Put what is chosen on the mixtape playing; on a tape shown, take it off |
| `Alt+Shift+B` | List the mixtapes; with one shown, go back |

## 📝 File operations

| Key | Action |
|-----|--------|
| `F1` | Help |
| `F2` | Rename; with a selection, rename them all by a pattern |
| `Shift+F2` | Compare directories: select what differs in each panel |
| `F3` | View file |
| `F4` | Edit file (built-in, or the editor set under F11) |
| `Shift+F4` | Create empty file |
| `F5` | Copy (to the other panel, or wherever the dialog is told - into an archive too); on a `.001`, join the pieces back |
| `F6` | Move (to the other panel, or wherever the dialog is told - into an archive too) |
| `Alt+F5` / `Ctrl+Shift+F5` | Make a symbolic link (in the other panel, or wherever the dialog is told; `Tab` for a hard link) |
| `F7` | Create directory (inside an archive too) |
| `F8` / `Delete` | Move to trash (selected items or cursor item); inside an archive, delete, asked first |
| `Shift+F8` / `Shift+Delete` | Delete permanently |
| `F9` | Open terminal (the one set under F11, if any) |
| `F10` | Quit (twice during an operation; asks first about unsaved edits) |
| `Shift+F10` | Quit, leaving `cd84` in the directory it started from |
| `Alt+F10` | The panel as its drive's directory tree; again to go back |
| `F11` | Options - panels, behaviour, appearance, viewer and editor |
| `F12` | Preview cursor file in other panel |
| `=` / `Shift+F3` | Compare two files side by side |
| `Alt+A` | Change permissions, owner, group and modified time |
| `Alt+F7` / `Ctrl+F7` | Find files by name, content or regex, size and date (`F` on the results feeds them to the panel) |
| `Alt+S` | Synchronize the panels' directories (`→` `←` copy, `Del` delete, `Space` leave, `>` `<` mirror, `R` reset, `Enter` run) |
| `Alt+F9` / `Ctrl+F9` | Extract the whole of an archive |
| `Alt+P` | Pack what is chosen into an archive (`Tab` changes the format; `↑` `↓` the level, one archive for each, delete what went in, leave out, password, and `Space` / `←` `→` change them) |
| `Alt+H` | Checksums of what is chosen (`Tab` / `Shift+Tab` the algorithm, `C` copies, `Enter` writes), or check a checksum file |
| `Alt+M` | User menu: run commands of your own (`E` edits them) |
| `Alt+D` | Find duplicate files, fed to the panel with all but one of each selected |
| `Alt+R` | Dossier: the directory summed up (`↑` `↓` scroll, `Esc` close) |
| `Alt+Shift+G` | Ghost hunt: the files nobody has touched in two years, fed to the panel faded |
| `Alt+T` | The trash as a folder; again to go back |
| `Alt+G` | Git on what is chosen: `S` stage, `U` unstage, `D` discard changes (asks first), `C` compare with HEAD, `R` open a revision as a folder |

## 📼 Mixtapes (Alt+Shift+B)

| Key | Action |
|-----|--------|
| `Enter` / `1`…`9` | Show the tape in the panel, all of it selected, and play it |
| `Space` | Play the tape, for Alt+B to add to |
| `N` / `Insert` | A new tape (type its name, `Enter`) |
| `R` / `F2` | Rename the tape |
| `D` / `Delete` | Remove the tape - never its files |
| `Esc` / `Alt+Shift+B` | Close |

## 🗑️ The trash (Alt+T)

| Key | Action |
|-----|--------|
| `F3` / `F5` | View / copy out, as from an archive |
| `F6` / `Enter` on a file | Put back where it came from |
| `F8` | Delete for good, asked first |
| `Alt+T` | Back where the panel was |

## 🌳 Directory tree (Alt+F10)

| Key | Action |
|-----|--------|
| `↑` `↓`, `PageUp` / `PageDown`, `Home` / `End` | Move |
| `→` / `+` | Open the directory, or go into it when it is open |
| `←` / `-` | Close it, or go up to the one it is in |
| `*` | Open everything below (a job; `Esc` stops it) |
| `Enter` | Send the other panel there |
| Typing | Go to the first directory on show starting with it (`↑` `↓` the others, `Esc` stop) |
| `F5` / `F6` / `F7` / `F8` | Copy / move to the other panel, make a directory inside, delete |
| `Esc` / `Alt+F10` | Back to the files, in the directory under the cursor |

## 🧱 Space map (Alt+U)

| Key | Action |
|-----|--------|
| `↑` `↓` `←` `→` | To the block that way |
| `Enter` | Into the directory, mapped in its turn; on a file, back to the panel on it |
| `Backspace` | Up a directory |
| `Esc` | Back to the panel, its cursor on the block chosen |
| Click | Choose a block; on the one chosen, `Enter` |

## 🌿 Opening a revision (Alt+G, R)

| Key | Action |
|-----|--------|
| Typing | Narrow the branches, tags, stashes and commits to those whose name or subject has each word |
| `↑` / `↓`, `PageUp` / `PageDown` | Choose |
| `Enter` | Open the one chosen as a read-only folder; with nothing listed, the revision as typed |
| `Esc` | Close |

## ⚙️ Options (F11)

| Key | Action |
|-----|--------|
| `↑` / `↓` | Move |
| `PageUp` / `PageDown` | A page up / down |
| `Home` / `End` | The first / the last option |
| `Enter` / `Space` / `→` | Change the option, or start typing into it |
| `←` | Change it the other way |
| `E` | On Info, edit the commands; on Theme, the file colours |
| `Esc` / `F11` | Close |

## ⏳ While a job runs

| Key | Action |
|-----|--------|
| `Esc` | Cancel a running copy, move or delete |
| `Y` / `N` / `K` | Overwrite / skip / keep both, when a file is already there |
| `A` / `S` / `B` / `O` | Overwrite all / skip all / keep both all / overwrite all older |
| `R` / `S` / `A` | Retry / skip / skip all, when an entry fails |
| `R` / `C` | Check again / go on anyway, when the target has not the room |
| `P` / `S` | Split into pieces / skip, a file too big for where it goes in one |
| `D` | Delete permanently, when the trash will not take an entry |

## 🎨 Viewer

| Key | Action |
|-----|--------|
| `X` | Toggle hex view; cycle Image, Text, Hex for images |
| `F` | Fit or fill an image; on text, follow the file as it grows (scrolling up stops it) |
| `+` / `-` | Zoom an image in or out |
| `W` | Wrap long lines, or scroll them sideways |
| `PageUp` / `PageDown` | Previous / next picture in the folder |
| `Alt+Left` / `Alt+Right` | The file as the commit before / after had it; past the newest, as it is now |
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
| Click / drag the minimap | The rows there in the middle of the view / follow the pointer (Viewer, Editor, Compare) |
| Double-click | Open directories and archives, or open files as Enter does |
| Click a column heading | Sort by it, again to reverse |
| Click the path | Edit it in place, as Ctrl+L |
| Click a drive icon | Send the panel to that drive |
| Click the git branch | List the repository's branches, to switch to another |
| Click an F-key | Press it |
| Click a tree's mark | Open or close that directory (double-click a name: `Enter`) |
