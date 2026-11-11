//! The `colors` file: a colour of the user's own for files by extension, in
//! place of the one hashed from it.

use super::*;
use ratatui::style::{Color, Modifier};

/// One line of the `colors` file.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorRule {
    /// Without the dot, as written; `*` is every file.
    pub extensions: Vec<String>,
    /// On a dark background, or None to keep the hashed colour.
    pub dark: Option<Color>,
    /// On a light one: the second colour, or the first when there is one.
    pub light: Option<Color>,
    pub modifiers: Modifier,
}

impl ColorRule {
    /// Whether the line is for a file named `name`: one ending in one of its
    /// extensions, any case, or named just that - so `makefile` is Makefile.
    pub fn applies(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.extensions.iter().any(|extension| {
            let extension = extension.to_lowercase();
            extension == "*" || name == extension || name.strip_suffix(extension.as_str()).is_some_and(|start| start.ends_with('.'))
        })
    }
}

/// What a new `colors` file says.
const COLORS_TEMPLATE: &str = "\
# The colours fm84 draws files in, by extension, as
#   extensions = colour
# The first line for a file is the one used; a file with none keeps the
# colour fm84 picks for its extension. Extensions are separated by spaces,
# any case, without the dot - tar.gz works too, and a whole name such as
# makefile - and * is every file. A colour is #rrggbb, #rgb, a number from
# 0 to 255 or a name: black red green yellow blue magenta cyan white gray,
# and bright-red and the rest. Two, as dark / light, are for dark themes
# and light ones. bold italic underline dim can go with it, or alone.
#
# rs toml = #dea584
# jpg jpeg png gif webp = magenta
# tar.gz tgz zip 7z = bold bright-red
# md = #7aa2f7 / #1a4b95
# log = dim
";

/// The `colors` file's lines, in order.
pub fn load_colors() -> Vec<ColorRule> {
    config_path("colors").and_then(|path| fs::read_to_string(path).ok()).map(|text| parse_colors(&text)).unwrap_or_default()
}

/// A line with no extension, a word that is no colour or style, or nothing
/// to give the file, is passed over.
pub fn parse_colors(text: &str) -> Vec<ColorRule> {
    key_values(text)
        .filter_map(|(extensions, value)| {
            let extensions: Vec<String> = extensions.split([' ', '\t', ',']).map(|extension| extension.trim_start_matches('.')).filter(|extension| !extension.is_empty()).map(String::from).collect();
            let mut modifiers = Modifier::empty();
            let mut colors = Vec::new();
            for part in value.split('/') {
                let mut color = None;
                for word in part.split_whitespace() {
                    match word.to_lowercase().as_str() {
                        "bold" => modifiers |= Modifier::BOLD,
                        "italic" => modifiers |= Modifier::ITALIC,
                        "underline" | "underlined" => modifiers |= Modifier::UNDERLINED,
                        "dim" => modifiers |= Modifier::DIM,
                        _ if color.is_none() => color = Some(parse_color(word)?),
                        _ => return None,
                    }
                }
                colors.push(color);
            }
            if extensions.is_empty() || colors.len() > 2 {
                return None;
            }
            let dark = colors[0];
            let light = colors.get(1).copied().flatten().or(dark);
            (dark.is_some() || light.is_some() || !modifiers.is_empty()).then_some(ColorRule { extensions, dark, light, modifiers })
        })
        .collect()
}

/// `#rrggbb`, `#rgb`, 0 to 255 or a name, as ratatui reads them - but not
/// `reset`, which would leave the file in whatever the terminal has.
fn parse_color(word: &str) -> Option<Color> {
    let expanded;
    let word = match word.strip_prefix('#') {
        Some(short) if short.len() == 3 && short.chars().all(|digit| digit.is_ascii_hexdigit()) => {
            expanded = format!("#{}", short.chars().flat_map(|digit| [digit, digit]).collect::<String>());
            &expanded
        }
        _ => word,
    };
    word.parse::<Color>().ok().filter(|&color| color != Color::Reset)
}

/// The `colors` file, written with examples if there is none yet.
pub fn colors_file() -> Option<PathBuf> {
    let path = config_path("colors")?;
    if !path.exists() {
        let _ = write_config_file("colors", COLORS_TEMPLATE);
    }
    Some(path)
}
