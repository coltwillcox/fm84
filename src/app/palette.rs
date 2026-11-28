//! Ctrl+Shift+P, the command palette: every action of the panels with its
//! key, narrowed as words are typed, and Enter runs the one chosen - by
//! pressing its key, so it does just what the key does and nothing here
//! can fall out of step with it.
//!
//! Each is said as F1 says it, key first, in the language shown; a line of
//! F1 that tells of two keys at once is said here as two.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// One action: how it is shown, and the key that does it.
#[derive(Clone, Copy, Debug)]
pub struct Command {
    pub text: &'static str,
    pub key: KeyEvent,
}

/// The palette open: what is typed, the commands it lets through - by their
/// place in `all` - and the one the cursor is on among them.
pub struct PaletteView {
    pub input: TextInput,
    all: Vec<Command>,
    pub shown: Vec<usize>,
    pub index: usize,
}

impl PaletteView {
    pub fn new() -> Self {
        let all = commands();
        let shown = (0..all.len()).collect();
        PaletteView { input: TextInput::new(), all, shown, index: 0 }
    }

    /// The commands shown, in order.
    pub fn shown(&self) -> impl Iterator<Item = &Command> {
        self.shown.iter().map(|&at| &self.all[at])
    }

    /// Narrow to the commands with every word typed somewhere in them, in
    /// any order, smart case as Find is: all in lower case matches either.
    /// The cursor goes back to the first.
    fn narrow(&mut self) {
        let words: Vec<&str> = self.input.text.split_whitespace().collect();
        self.shown = (0..self.all.len()).filter(|&at| command_matches(self.all[at].text, &words)).collect();
        self.index = 0;
    }
}

/// Whether `text` has each of `words` in it, smart case.
pub fn command_matches(text: &str, words: &[&str]) -> bool {
    let fold = !words.iter().any(|word| word.chars().any(char::is_uppercase));
    let folded = |text: &str| -> String { if fold { text.chars().map(crate::find::fold_char).collect() } else { text.to_string() } };
    let text = folded(text);
    words.iter().all(|word| text.contains(&folded(word)))
}

impl AppState {
    pub fn open_palette(&mut self) {
        self.dialog = Some(Dialog::Palette(Box::new(PaletteView::new())));
    }

    /// The field changed: the list narrowed to it.
    pub fn palette_typed(&mut self) {
        if let Some(Dialog::Palette(view)) = &mut self.dialog {
            view.narrow();
        }
    }

    /// Up or Down by `delta`, held to the list's ends.
    pub fn move_palette_cursor(&mut self, delta: isize) {
        if let Some(Dialog::Palette(view)) = &mut self.dialog {
            let last = view.shown.len().saturating_sub(1);
            view.index = view.index.saturating_add_signed(delta).min(last);
        }
    }

    /// Enter: the palette put away, and the key of the command chosen given
    /// back to be pressed. None, and the palette left open, with nothing
    /// shown to choose.
    pub fn take_palette_key(&mut self) -> Option<KeyEvent> {
        let Some(Dialog::Palette(view)) = &self.dialog else {
            return None;
        };
        let key = view.shown().nth(view.index)?.key;
        self.dialog = None;
        Some(key)
    }
}

/// Every action the palette offers, in F1's order.
pub fn commands() -> Vec<Command> {
    let with = |code: KeyCode, modifiers: KeyModifiers| KeyEvent::new(code, modifiers);
    let plain = |code: KeyCode| with(code, KeyModifiers::NONE);
    let shift = |code: KeyCode| with(code, KeyModifiers::SHIFT);
    let alt = |code: KeyCode| with(code, KeyModifiers::ALT);
    let ctrl = |code: KeyCode| with(code, KeyModifiers::CONTROL);
    let alt_char = |c: char| alt(KeyCode::Char(c));
    let ctrl_char = |c: char| ctrl(KeyCode::Char(c));
    let command = |text: &'static str, key: KeyEvent| Command { text, key };
    vec![
        command(t!("F1 - This help"), plain(KeyCode::F(1))),
        command(t!("F2 - Rename folder/file"), plain(KeyCode::F(2))),
        command(t!("Shift+F2 compare directories"), shift(KeyCode::F(2))),
        command(t!("F3 - View file (X text/hex/image)"), plain(KeyCode::F(3))),
        command(t!("Shift+F3 (or =) compare two files"), shift(KeyCode::F(3))),
        command(t!("F4 - Edit file (Ctrl+S/F2 save)"), plain(KeyCode::F(4))),
        command(t!("F5 - Copy (to the other panel, or where typed)"), plain(KeyCode::F(5))),
        command(t!("Alt+F5 (or Ctrl+Shift+F5) link, Tab hard"), alt(KeyCode::F(5))),
        command(t!("F6 - Move (to the other panel, or where typed)"), plain(KeyCode::F(6))),
        command(t!("F7 - Create directory"), plain(KeyCode::F(7))),
        command(t!("Alt+F7 (or Ctrl+F7) find files"), alt(KeyCode::F(7))),
        command(t!("Shift+F4 - Create file"), shift(KeyCode::F(4))),
        command(t!("F8 - Move to trash"), plain(KeyCode::F(8))),
        command(t!("Shift+F8 delete permanently"), shift(KeyCode::F(8))),
        command(t!("Alt+T - The trash as a folder"), alt_char('t')),
        command(t!("Ctrl+Z - Undo the last rename, copy, move or trash"), ctrl_char('z')),
        command(t!("Alt+A - Permissions, owner, time"), alt_char('a')),
        command(t!("Alt+P - Pack: Tab format, level, password"), alt_char('p')),
        command(t!("Alt+F9 - Extract a whole archive"), alt(KeyCode::F(9))),
        command(t!("Alt+L - Lock with a password, or unlock"), alt_char('l')),
        command(t!("Alt+H - Checksums (Tab algorithm); check a file"), alt_char('h')),
        command(t!("Alt+S - Synchronize the two directories"), alt_char('s')),
        command(t!("Alt+D - Find duplicate files"), alt_char('d')),
        command(t!("Alt+R - Dossier: the directory summed up"), alt_char('r')),
        command(t!("Alt+Shift+G - Ghost hunt: untouched for years"), alt_char('G')),
        command(t!("Alt+Q - Share with a phone, by a QR code"), alt_char('q')),
        command(t!("Alt+Shift+Q takes files a phone sends"), alt_char('Q')),
        command(t!("Alt+V - The clipboard with a phone, both ways"), alt_char('v')),
        command(t!("Alt+W - A phone as a remote; S in it stops"), alt_char('w')),
        command(t!("Alt+N - Send to another fm84 (null modem)"), alt_char('n')),
        command(t!("Alt+Shift+N listens for one to send here"), alt_char('N')),
        command(t!("Alt+G - Git: stage, unstage, discard, compare"), alt_char('g')),
        command(t!("Alt+B - Onto the mixtape; Alt+Shift+B lists them"), alt_char('b')),
        command(t!("Alt+Shift+B - The mixtapes"), alt_char('B')),
        command(t!("F9 - Open terminal"), plain(KeyCode::F(9))),
        command(t!("Alt+M - User menu (E edits it)"), alt_char('m')),
        command(t!("F10 - Quit"), plain(KeyCode::F(10))),
        command(t!("Shift+F10 quit, cd84 staying put"), shift(KeyCode::F(10))),
        command(t!("Alt+F10 directory tree, Enter sends the other"), alt(KeyCode::F(10))),
        command(t!("F11 - Options (Alt+F1/F2 drives)"), plain(KeyCode::F(11))),
        command(t!("F12 - Preview in other panel"), plain(KeyCode::F(12))),
        command(t!("Shift+Enter (or Ctrl+O) open as the system does"), ctrl_char('o')),
        command(t!("Alt+O open with... (E per extension)"), alt_char('o')),
        command(t!("Ctrl+U - Swap the panels"), ctrl_char('u')),
        command(t!("Alt+I - The same directory in the other panel"), alt_char('i')),
        command(t!("Ctrl+L - Go to path (or click it)"), ctrl_char('l')),
        command(t!("Alt+Down every directory visited"), alt(KeyCode::Down)),
        command(t!("Alt+J jump by a few letters, most used first"), alt_char('j')),
        command(t!("Ctrl+D - Bookmarks"), ctrl_char('d')),
        command(t!("Ctrl+T - New tab"), ctrl_char('t')),
        command(t!("Ctrl+W - Close tab"), ctrl_char('w')),
        command(t!("Alt+Shift+Enter size and map every directory"), with(KeyCode::Enter, KeyModifiers::ALT | KeyModifiers::SHIFT)),
        command(t!("Alt+U the space map as blocks, Enter goes in"), alt_char('u')),
        command(t!("+ - Select by pattern"), plain(KeyCode::Char('+'))),
        command(t!("- - Deselect by pattern"), plain(KeyCode::Char('-'))),
        command(t!("* - Invert the selection"), plain(KeyCode::Char('*'))),
        command(t!("Alt+* invert with directories"), alt_char('*')),
        command(t!("Ctrl+S - Filter the panel by pattern"), ctrl_char('s')),
        command(t!("Ctrl+B - Every file below, flat"), ctrl_char('b')),
        command(t!("Ctrl+E - Edit the names as text"), ctrl_char('e')),
        command(t!("Ctrl+C - Copy the paths"), ctrl_char('c')),
        command(t!("Alt+C - Copy the names"), alt_char('c')),
        command(t!("Ctrl+F3 - Sort by name"), ctrl(KeyCode::F(3))),
        command(t!("Ctrl+F4 - Sort by extension"), ctrl(KeyCode::F(4))),
        command(t!("Ctrl+F5 - Sort by date"), ctrl(KeyCode::F(5))),
        command(t!("Ctrl+F6 - Sort by size"), ctrl(KeyCode::F(6))),
        command(t!("Ctrl+R - Reload both panels"), ctrl_char('r')),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every word, in any order, anywhere; lower case matches either case,
    /// a capital only itself.
    #[test]
    fn words_narrow_in_any_order_smart_case() {
        let text = "Alt+D - Find duplicate files";
        assert!(command_matches(text, &[]));
        assert!(command_matches(text, &["dupl"]));
        assert!(command_matches(text, &["files", "find"]));
        assert!(command_matches(text, &["alt+d"]));
        assert!(command_matches(text, &["Find"]));
        assert!(!command_matches(text, &["FIND"]));
        assert!(!command_matches(text, &["find", "tabs"]));
    }

    /// The key each command presses as its words name it - so the palette
    /// can never say one key and press another.
    #[test]
    fn each_command_names_the_key_it_presses() {
        let name = |key: KeyEvent| {
            let mut name = String::new();
            let shifted = matches!(key.code, KeyCode::Char(c) if c.is_ascii_uppercase());
            for (modifier, said) in [(KeyModifiers::CONTROL, "Ctrl+"), (KeyModifiers::ALT, "Alt+")] {
                if key.modifiers.contains(modifier) {
                    name.push_str(said);
                }
            }
            if key.modifiers.contains(KeyModifiers::SHIFT) || shifted {
                name.push_str("Shift+");
            }
            name.push_str(&match key.code {
                KeyCode::F(n) => format!("F{n}"),
                KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
                KeyCode::Enter => "Enter".to_string(),
                KeyCode::Down => "Down".to_string(),
                other => panic!("no name for {other:?}"),
            });
            name
        };
        let all = commands();
        for command in &all {
            assert!(command.text.contains(&name(command.key)), "{:?} presses {}", command.text, name(command.key));
        }
        // No key offered twice.
        for (at, command) in all.iter().enumerate() {
            assert!(all[..at].iter().all(|before| before.key != command.key), "{:?} twice", command.key);
        }
    }
}
