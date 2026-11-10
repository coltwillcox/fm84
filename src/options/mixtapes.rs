//! Alt+B's mixtapes: baskets with names, kept in the `mixtapes` file beside
//! the config so they are there next time - "Release files", "Tax 2026".
//! One is playing, the one Alt+B adds to.

use super::*;

/// One tape: its name, and what is on it, as paths and whether each is a
/// directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tape {
    pub name: String,
    pub items: Vec<(PathBuf, bool)>,
}

/// Every tape, in the order they are listed, and which is playing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mixtapes {
    pub tapes: Vec<Tape>,
    pub playing: usize,
}

impl Mixtapes {
    /// What the `mixtapes` file holds; none for a file not there.
    pub fn load() -> Self {
        let text = config_path("mixtapes").and_then(|path| fs::read_to_string(path).ok()).unwrap_or_default();
        Self::parse(&text)
    }

    /// The file's tapes: a `tape` line starts one, and the `file` and `dir`
    /// lines after it are what is on it. `playing` names the one playing;
    /// none that is there, and it is the first.
    pub fn parse(text: &str) -> Self {
        let mut mixtapes = Mixtapes::default();
        let mut playing = None;
        for (key, value) in key_values(text) {
            match key {
                "tape" if !value.is_empty() => mixtapes.tapes.push(Tape { name: value.to_string(), items: Vec::new() }),
                "playing" => playing = Some(value.to_string()),
                "file" | "dir" if !value.is_empty() => {
                    if let Some(tape) = mixtapes.tapes.last_mut() {
                        tape.items.push((unescape_path(value), key == "dir"));
                    }
                }
                _ => {}
            }
        }
        mixtapes.playing = playing.and_then(|name| mixtapes.tapes.iter().position(|tape| tape.name == name)).unwrap_or(0);
        mixtapes
    }

    pub fn serialize(&self) -> String {
        let mut text = String::from("# fm84's mixtapes, from Alt+B. A tape line starts each; the file and dir\n# lines after it are what is on it.\n");
        if let Some(tape) = self.playing() {
            text.push_str(&format!("playing = {}\n", tape.name));
        }
        for tape in &self.tapes {
            text.push_str(&format!("\ntape = {}\n", tape.name));
            for (path, is_dir) in &tape.items {
                text.push_str(&format!("{} = {}\n", if *is_dir { "dir" } else { "file" }, escape_path(path)));
            }
        }
        text
    }

    pub fn save(&self) -> io::Result<()> {
        write_config_file("mixtapes", &self.serialize())
    }

    pub fn playing(&self) -> Option<&Tape> {
        self.tapes.get(self.playing)
    }

    pub fn playing_mut(&mut self) -> Option<&mut Tape> {
        self.tapes.get_mut(self.playing)
    }

    /// Whether a tape other than `except` is called `name` already.
    pub fn is_taken(&self, name: &str, except: Option<usize>) -> bool {
        self.tapes.iter().enumerate().any(|(index, tape)| Some(index) != except && tape.name == name)
    }

    /// A name for a new tape that no other has: `base`, then `base 2` and on.
    pub fn fresh_name(&self, base: &str) -> String {
        (1..).map(|n| if n == 1 { base.to_string() } else { format!("{base} {n}") }).find(|name| !self.is_taken(name, None)).unwrap_or_default()
    }
}
