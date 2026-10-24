//! Syntax highlighting, for the viewer and the editor alike, a line at a time
//! so an edit re-parses from where it was made.

use super::*;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();

static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

static HIGHLIGHTERS: OnceLock<std::collections::HashMap<&'static str, Highlighter<'static>>> = OnceLock::new();

/// Parser and highlighter state as it stands *before* a given line.
pub type LineState = (ParseState, HighlightState);

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The highlighter for the palette showing, so a light theme gets dark code.
/// The theme cannot change while a file is open - F11 is not reachable from
/// the viewer or the editor - so the highlight state an open file carries was
/// always made by the highlighter it goes back to.
fn highlighter() -> &'static Highlighter<'static> {
    let highlighters = HIGHLIGHTERS.get_or_init(|| {
        let themes = THEME_SET.get_or_init(ThemeSet::load_defaults);
        crate::options::Theme::ALL
            .iter()
            .map(|&theme| crate::display::palette_of(theme).syntax_theme)
            .filter_map(|name| themes.themes.get(name).map(|theme| (name, Highlighter::new(theme))))
            .collect()
    });
    highlighters.get(crate::display::palette().syntax_theme).unwrap_or_else(|| highlighters.values().next().expect("syntect ships its default themes"))
}

pub fn detect_syntax(path: &Path) -> String {
    match path.extension().and_then(|s| s.to_str()) {
        Some("rs") => "Rust".to_string(),
        Some("py") => "Python".to_string(),
        Some("js") | Some("jsx") => "JavaScript".to_string(),
        Some("ts") | Some("tsx") => "TypeScript".to_string(),
        Some("json") => "JSON".to_string(),
        Some("toml") => "TOML".to_string(),
        Some("yaml") | Some("yml") => "YAML".to_string(),
        Some("md") => "Markdown".to_string(),
        Some("sh") | Some("bash") => "Shell".to_string(),
        Some("c") | Some("h") => "C".to_string(),
        Some("cpp") | Some("hpp") => "C++".to_string(),
        Some("html") => "HTML".to_string(),
        Some("css") => "CSS".to_string(),
        _ => "Plain Text".to_string(),
    }
}

/// Highlight a whole file, recording the parser state before each line.
pub fn highlight_all(content: &[String], extension: &str) -> (Vec<Vec<Span<'static>>>, Vec<LineState>) {
    let mut spans = Vec::new();
    let mut states = Vec::new();
    highlight_from(content, extension, 0, &mut spans, &mut states);
    (spans, states)
}

/// Re-highlight from `from` downward, resuming from the cached parser state
/// rather than re-parsing the file from the top, and stopping again as soon as
/// the state matches the cache - past that point the existing spans still hold.
/// Both vectors are edited in place, so an ordinary keystroke touches a couple
/// of entries no matter how long the file is.
pub fn highlight_from(content: &[String], extension: &str, from: usize, spans: &mut Vec<Vec<Span<'static>>>, states: &mut Vec<LineState>) {
    highlight_through(content, extension, from, from, spans, states);
}

/// highlight_from for an edit that changed every line from `from` to
/// `through` - Replace all, done in one go and highlighted once. The parse
/// may not stop on meeting the cache before `through`: below a line the
/// cache agrees with, the lines changed since still have to be read again.
pub fn highlight_through(content: &[String], extension: &str, from: usize, through: usize, spans: &mut Vec<Vec<Span<'static>>>, states: &mut Vec<LineState>) {
    let ps = syntax_set();
    let hlr = highlighter();
    let syntax = ps.find_syntax_by_extension(extension).unwrap_or_else(|| ps.find_syntax_plain_text());
    let fresh = || (ParseState::new(syntax), HighlightState::new(hlr, ScopeStack::new()));

    let mut start = from;
    // Lines added or removed shift the cache; splice it back into alignment so
    // index i again describes line i. The spliced slots get rewritten below.
    let delta = content.len() as isize - states.len() as isize;
    if !states.is_empty() && states.len() == spans.len() {
        start = from.min(states.len() - 1);
        let at = (start + 1).min(states.len());
        if delta > 0 {
            let filler = states[start].clone();
            for _ in 0..delta {
                states.insert(at, filler.clone());
                spans.insert(at, Vec::new());
            }
        } else {
            for _ in 0..-delta {
                if at < states.len() {
                    states.remove(at);
                    spans.remove(at);
                }
            }
        }
    }

    // Anything we could not reconcile: start over from the top.
    let resumable = states.len() == content.len() && spans.len() == content.len();
    if !resumable {
        states.clear();
        spans.clear();
        states.reserve(content.len());
        spans.reserve(content.len());
        start = 0;
    }

    let (mut parse, mut hl) = if resumable { states[start].clone() } else { fresh() };
    // The cache is only trustworthy below the spliced region.
    let min_converge = if resumable { (start + 1 + delta.max(0) as usize).max(through.saturating_add(1)) } else { usize::MAX };

    let mut index = start;
    while index < content.len() {
        if index >= min_converge
            && let Some((cached_parse, cached_hl)) = states.get(index)
            && *cached_parse == parse
            && *cached_hl == hl
        {
            break;
        }

        let entering = (parse.clone(), hl.clone());
        let line = &content[index];
        let ops = parse.parse_line(line, ps).unwrap_or_default();
        let line_spans: Vec<Span<'static>> = HighlightIterator::new(&mut hl, &ops, line, hlr)
            .map(|(style, text)| Span::styled(text.to_string(), ratatui::style::Style::default().fg(syntect_to_ratatui_color(style.foreground))))
            .collect();

        if index < states.len() {
            states[index] = entering;
            spans[index] = line_spans;
        } else {
            states.push(entering);
            spans.push(line_spans);
        }
        index += 1;
    }
}

fn syntect_to_ratatui_color(color: syntect::highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}
