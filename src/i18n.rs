//! The words fm84 shows, in the language F11 asks for. English is the code
//! itself: each sentence is written out where it is used, inside `t!`, and
//! is the key a translation is looked up by - so the code reads as it did,
//! and a translator sees the real sentence. One with no translation yet is
//! shown in English.
//!
//! Every translation is built into the executable, from `lang/` in the
//! repository. Nothing is read from the disk: a copy beside the config would
//! outlive the fm84 it came with, and hide the sentences added since.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

/// A language fm84 can be shown in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Language {
    English,
    /// In the Latin alphabet.
    Serbian,
    Turkish,
    /// Bokmål.
    Norwegian,
    German,
    Italian,
    Greek,
    French,
    Slovenian,
    Croatian,
    Macedonian,
    Hungarian,
    Slovak,
    Czech,
    Polish,
    Swedish,
    Spanish,
    /// As Brazil writes it.
    Portuguese,
    Dutch,
    Danish,
    Irish,
    Romanian,
}

impl Language {
    /// In the order F11 steps through them: English, then the rest by their
    /// own names, as the alphabet has them - a Greek or Cyrillic one by how
    /// it is spelt in Latin letters.
    pub const ALL: [Language; 22] = [
        Language::English,
        Language::Czech,
        Language::Danish,
        Language::German,
        Language::Greek,
        Language::Spanish,
        Language::French,
        Language::Irish,
        Language::Croatian,
        Language::Italian,
        Language::Hungarian,
        Language::Macedonian,
        Language::Dutch,
        Language::Norwegian,
        Language::Polish,
        Language::Portuguese,
        Language::Romanian,
        Language::Slovak,
        Language::Slovenian,
        Language::Serbian,
        Language::Swedish,
        Language::Turkish,
    ];

    /// How the config file names it.
    pub fn key(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Serbian => "sr",
            Language::Turkish => "tr",
            Language::Norwegian => "nb",
            Language::German => "de",
            Language::Italian => "it",
            Language::Greek => "el",
            Language::French => "fr",
            Language::Slovenian => "sl",
            Language::Croatian => "hr",
            Language::Macedonian => "mk",
            Language::Hungarian => "hu",
            Language::Slovak => "sk",
            Language::Czech => "cs",
            Language::Polish => "pl",
            Language::Swedish => "sv",
            Language::Spanish => "es",
            Language::Portuguese => "pt",
            Language::Dutch => "nl",
            Language::Danish => "da",
            Language::Irish => "ga",
            Language::Romanian => "ro",
        }
    }

    /// Its own name for itself, which is never translated: someone looking
    /// for their language finds it written in it.
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Serbian => "Srpski (latinica)",
            Language::Turkish => "Türkçe",
            Language::Norwegian => "Norsk bokmål",
            Language::German => "Deutsch",
            Language::Italian => "Italiano",
            Language::Greek => "Ελληνικά",
            Language::French => "Français",
            Language::Slovenian => "Slovenščina",
            Language::Croatian => "Hrvatski",
            Language::Macedonian => "Македонски",
            Language::Hungarian => "Magyar",
            Language::Slovak => "Slovenčina",
            Language::Czech => "Čeština",
            Language::Polish => "Polski",
            Language::Swedish => "Svenska",
            Language::Spanish => "Español",
            Language::Portuguese => "Português (Brasil)",
            Language::Dutch => "Nederlands",
            Language::Danish => "Dansk",
            Language::Irish => "Gaeilge",
            Language::Romanian => "Română",
        }
    }

    pub fn from_key(key: &str) -> Option<Language> {
        Language::ALL.into_iter().find(|language| language.key() == key)
    }

    /// The catalog, as it is in `lang/`. None for English, which is the
    /// code.
    pub(crate) fn source(self) -> Option<&'static str> {
        match self {
            Language::English => None,
            Language::Serbian => Some(include_str!("../lang/sr.lang")),
            Language::Turkish => Some(include_str!("../lang/tr.lang")),
            Language::Norwegian => Some(include_str!("../lang/nb.lang")),
            Language::German => Some(include_str!("../lang/de.lang")),
            Language::Italian => Some(include_str!("../lang/it.lang")),
            Language::Greek => Some(include_str!("../lang/el.lang")),
            Language::French => Some(include_str!("../lang/fr.lang")),
            Language::Slovenian => Some(include_str!("../lang/sl.lang")),
            Language::Croatian => Some(include_str!("../lang/hr.lang")),
            Language::Macedonian => Some(include_str!("../lang/mk.lang")),
            Language::Hungarian => Some(include_str!("../lang/hu.lang")),
            Language::Slovak => Some(include_str!("../lang/sk.lang")),
            Language::Czech => Some(include_str!("../lang/cs.lang")),
            Language::Polish => Some(include_str!("../lang/pl.lang")),
            Language::Swedish => Some(include_str!("../lang/sv.lang")),
            Language::Spanish => Some(include_str!("../lang/es.lang")),
            Language::Portuguese => Some(include_str!("../lang/pt.lang")),
            Language::Dutch => Some(include_str!("../lang/nl.lang")),
            Language::Danish => Some(include_str!("../lang/da.lang")),
            Language::Irish => Some(include_str!("../lang/ga.lang")),
            Language::Romanian => Some(include_str!("../lang/ro.lang")),
        }
    }

    /// Which form a count takes, by the language's own rule.
    pub fn plural(self, count: u64) -> Form {
        match self {
            // A noun after a number stays as it is in Turkish - "5 dosya" -
            // but a sentence can still say one apart from the rest.
            Language::English | Language::Turkish | Language::Norwegian | Language::German | Language::Italian | Language::Greek | Language::Hungarian | Language::Swedish | Language::Spanish | Language::Dutch | Language::Danish => {
                if count == 1 {
                    Form::One
                } else {
                    Form::Other
                }
            }
            // Nothing is one too: "0 fichier", as "1 fichier".
            Language::French | Language::Portuguese => {
                if count <= 1 {
                    Form::One
                } else {
                    Form::Other
                }
            }
            // By the last two digits: 1, 101 one; 2, 102 two; 3, 4, 103
            // few; the rest, and 11 to 14, other.
            Language::Slovenian => match count % 100 {
                1 => Form::One,
                2 => Form::Two,
                3 | 4 => Form::Few,
                _ => Form::Other,
            },
            // Ending in 1 but not in 11 one - 1, 21, 101 - the rest other.
            Language::Macedonian => {
                if count % 10 == 1 && count % 100 != 11 {
                    Form::One
                } else {
                    Form::Other
                }
            }
            // 1 one; 2, 3 and 4 few; everything else, 21 too, other.
            Language::Slovak | Language::Czech => match count {
                1 => Form::One,
                2..=4 => Form::Few,
                _ => Form::Other,
            },
            // 1 alone one; 2-4, 22-24 few, but not 12-14; the rest, 21 too,
            // other.
            Language::Polish => match (count, count % 10, count % 100) {
                (1, _, _) => Form::One,
                (_, 2..=4, tens) if !(12..=14).contains(&tens) => Form::Few,
                _ => Form::Other,
            },
            // 1, 21, 101 one; 2-4, 22-24 few; the rest, and 11-14, other.
            Language::Serbian | Language::Croatian => match (count % 10, count % 100) {
                (1, tens) if tens != 11 => Form::One,
                (2..=4, tens) if !(12..=14).contains(&tens) => Form::Few,
                _ => Form::Other,
            },
            // 1 one; 0, and 2 to 19 by the last two digits, few - "5
            // fișiere"; the rest other, which takes "de" - "20 de fișiere",
            // "101 de fișiere".
            Language::Romanian => match (count, count % 100) {
                (1, _) => Form::One,
                (0, _) | (_, 2..=19) => Form::Few,
                _ => Form::Other,
            },
            // By the whole number, as the noun after it changes: 1 one, 2
            // two, 3-6 few ("3 chomhad"), 7-10 many ("7 gcomhad"), the rest
            // other ("20 comhad").
            Language::Irish => match count {
                1 => Form::One,
                2 => Form::Two,
                3..=6 => Form::Few,
                7..=10 => Form::Many,
                _ => Form::Other,
            },
        }
    }

    /// The language a locale names - `sr_RS.UTF-8`, `sr@latin`, `en-GB` -
    /// or None for one fm84 has no words for. `C` and `POSIX` are English.
    pub fn from_locale(locale: &str) -> Option<Language> {
        let code = locale.split(['_', '-', '.', '@']).next().unwrap_or("").to_ascii_lowercase();
        match code.as_str() {
            "c" | "posix" | "en" => Some(Language::English),
            "sr" => Some(Language::Serbian),
            "tr" => Some(Language::Turkish),
            // Bokmål, and the older code for it; Nynorsk is a language of
            // its own.
            "nb" | "no" => Some(Language::Norwegian),
            "de" => Some(Language::German),
            "it" => Some(Language::Italian),
            "el" => Some(Language::Greek),
            "fr" => Some(Language::French),
            "sl" => Some(Language::Slovenian),
            "hr" => Some(Language::Croatian),
            "mk" => Some(Language::Macedonian),
            "hu" => Some(Language::Hungarian),
            "sk" => Some(Language::Slovak),
            "cs" => Some(Language::Czech),
            "pl" => Some(Language::Polish),
            "sv" => Some(Language::Swedish),
            "es" => Some(Language::Spanish),
            "pt" => Some(Language::Portuguese),
            "nl" => Some(Language::Dutch),
            "da" => Some(Language::Danish),
            "ga" => Some(Language::Irish),
            "ro" => Some(Language::Romanian),
            _ => None,
        }
    }
}

/// The forms a sentence with a count in it can take.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Form {
    One,
    /// Of a language with a dual, as Slovenian has: two of a thing.
    Two,
    Few,
    /// Of a language that sets more counts apart than few, as Irish sets
    /// 7 to 10 apart from 3 to 6.
    Many,
    Other,
}

impl Form {
    fn from_key(key: &str) -> Option<Form> {
        match key {
            "one" => Some(Form::One),
            "two" => Some(Form::Two),
            "few" => Some(Form::Few),
            "many" => Some(Form::Many),
            "other" => Some(Form::Other),
            _ => None,
        }
    }
}

/// The language the environment asks for: the first of `LANGUAGE`'s list
/// that fm84 has, then `LC_ALL`, `LC_MESSAGES` and `LANG`, the first of
/// them set deciding. English when none says, or says one fm84 has no
/// words for.
pub fn detect() -> Language {
    let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].into_iter().find_map(var);
    // GNU's list of languages to try, which counts only where the locale is
    // not C - as gettext has it, where none set is C too.
    let plain_c = locale.as_deref().is_none_or(|locale| locale == "C" || locale == "POSIX" || locale.starts_with("C."));
    if !plain_c
        && let Some(list) = var("LANGUAGE")
        && let Some(language) = list.split(':').find_map(Language::from_locale)
    {
        return language;
    }
    locale.as_deref().and_then(Language::from_locale).unwrap_or(Language::English)
}

/// One language's translations: each English sentence, and for one with a
/// count in it each of its forms.
#[derive(Default, Debug)]
pub(crate) struct Catalog {
    pub plain: HashMap<String, String>,
    pub forms: HashMap<(String, Form), String>,
}

/// A line of a catalog that could not be read, and why.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParseError {
    pub line: usize,
    pub message: String,
}

/// A catalog's text, as `lang/` has it:
///
/// ```text
/// # A comment.
/// msgid  Cannot open file: {e}
/// msgstr Ne mogu da otvorim datoteku: {e}
///
/// msgid  Renamed {done} of {total}
/// msgstr[one]   Preimenovan {done} od {total}
/// msgstr[few]   Preimenovana {done} od {total}
/// msgstr[other] Preimenovano {done} od {total}
///
/// msgctxt job
/// msgid   Copy
/// msgstr  Kopiranje
/// ```
///
/// Each text runs to the end of its line, spaces around it trimmed; `\n`
/// is a line break in it and `\\` a backslash. A `msgctxt` before a
/// `msgid` sets the one word apart from the same word elsewhere, as `tc!`
/// asks for it.
pub(crate) fn parse(text: &str) -> Result<Catalog, ParseError> {
    let mut catalog = Catalog::default();
    let mut id: Option<String> = None;
    let mut context: Option<String> = None;
    let mut translated = false;
    let error = |line: usize, message: &str| ParseError { line: line + 1, message: message.to_string() };
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (keyword, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = unescape(rest.trim());
        if keyword == "msgctxt" {
            if id.is_some() && !translated {
                return Err(error(number, "a msgctxt after a msgid with no msgstr"));
            }
            if rest.is_empty() {
                return Err(error(number, "an empty msgctxt"));
            }
            context = Some(rest);
            id = None;
            continue;
        }
        if keyword == "msgid" {
            if id.is_some() && !translated {
                return Err(error(number, "a msgid with no msgstr before it"));
            }
            if rest.is_empty() {
                return Err(error(number, "an empty msgid"));
            }
            id = Some(match context.take() {
                Some(context) => keyed(&context, &rest),
                None => rest,
            });
            translated = false;
            continue;
        }
        if context.is_some() {
            return Err(error(number, "a msgctxt with no msgid after it"));
        }
        let Some(current) = id.clone() else {
            return Err(error(number, "a msgstr with no msgid before it"));
        };
        if rest.is_empty() {
            return Err(error(number, "an empty msgstr"));
        }
        if keyword == "msgstr" {
            if catalog.plain.insert(current, rest).is_some() {
                return Err(error(number, "translated twice"));
            }
        } else if let Some(form) = keyword.strip_prefix("msgstr[").and_then(|form| form.strip_suffix(']')) {
            let Some(form) = Form::from_key(form) else {
                return Err(error(number, "a form that is not one, two, few, many or other"));
            };
            if catalog.forms.insert((current, form), rest).is_some() {
                return Err(error(number, "translated twice"));
            }
        } else {
            return Err(error(number, "neither msgid nor msgstr"));
        }
        translated = true;
    }
    if id.is_some() && !translated {
        return Err(ParseError {
            line: text.lines().count(),
            message: "a msgid with no msgstr after it".to_string(),
        });
    }
    Ok(catalog)
}

/// How a word in a context is kept: the two joined as gettext joins them.
pub(crate) fn keyed(context: &str, english: &str) -> String {
    format!("{context}\u{4}{english}")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Each language's catalog, read the first time it is wanted. A shipped
/// catalog that does not read is a bug the tests catch; should one slip
/// through, that language is shown in English rather than not at all.
fn catalog(language: Language) -> Option<&'static Catalog> {
    static CATALOGS: [OnceLock<Option<Catalog>>; Language::ALL.len()] = [const { OnceLock::new() }; Language::ALL.len()];
    let index = Language::ALL.iter().position(|&known| known == language)?;
    CATALOGS[index].get_or_init(|| language.source().and_then(|source| parse(source).ok())).as_ref()
}

#[cfg(not(test))]
mod current {
    use super::Language;
    use std::sync::atomic::{AtomicU8, Ordering};

    static CURRENT: AtomicU8 = AtomicU8::new(0);

    pub fn get() -> Language {
        Language::ALL.get(CURRENT.load(Ordering::Relaxed) as usize).copied().unwrap_or(Language::English)
    }

    pub fn set(language: Language) {
        let index = Language::ALL.iter().position(|&known| known == language).unwrap_or(0);
        CURRENT.store(index as u8, Ordering::Relaxed);
    }
}

/// Under test, each thread's own: the tests run side by side, and one that
/// tries a language must not change the words another one checks.
#[cfg(test)]
mod current {
    use super::Language;
    use std::cell::Cell;

    thread_local! {
        static CURRENT: Cell<Language> = const { Cell::new(Language::English) };
    }

    pub fn get() -> Language {
        CURRENT.with(Cell::get)
    }

    pub fn set(language: Language) {
        CURRENT.with(|current| current.set(language));
    }
}

/// The language the words are shown in now.
pub fn language() -> Language {
    current::get()
}

/// Show the words in `language` from now on.
pub fn set_language(language: Language) {
    current::set(language);
}

/// What F11's Language says, made so: a language of its own, or None for
/// the one the environment asks for.
pub fn apply(choice: Option<Language>) {
    set_language(choice.unwrap_or_else(detect));
}

/// `english` in the language shown, or as it is when that has no words for
/// it.
pub fn tr(english: &'static str) -> &'static str {
    let language = language();
    if language == Language::English {
        return english;
    }
    catalog(language).and_then(|catalog| catalog.plain.get(english)).map_or(english, String::as_str)
}

/// `english` as it is meant in `context` - the same word can be two in
/// another language - or as it is.
pub fn tr_context(context: &'static str, english: &'static str) -> &'static str {
    let language = language();
    if language == Language::English {
        return english;
    }
    catalog(language).and_then(|catalog| catalog.plain.get(&keyed(context, english))).map_or(english, String::as_str)
}

/// The form of `one` - the English with a count of one, which is its key -
/// for `count`, or English's own when the language shown has none.
pub fn tr_count(one: &'static str, other: &'static str, count: u64) -> &'static str {
    let language = language();
    let english = if count == 1 { one } else { other };
    if language == Language::English {
        return english;
    }
    let Some(catalog) = catalog(language) else {
        return english;
    };
    let form = language.plural(count);
    catalog.forms.get(&(one.to_string(), form)).or_else(|| catalog.forms.get(&(one.to_string(), Form::Other))).map_or(english, String::as_str)
}

/// `template` with each `{name}` in it given its value. A name with no
/// value is left as it is written, as a brace with no name is.
pub fn fill(template: &str, values: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| values.iter().find(|(name, _)| *name == &after[..close]).map(|(_, value)| (close, value)));
        match value {
            Some((close, value)) => {
                use std::fmt::Write;
                let _ = write!(out, "{value}");
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A sentence in the language shown: `t!("Theme")` as it is, a `&'static
/// str`, and `t!("Cannot open {name}: {e}", name, e = error)` with its
/// places filled, a `String` - a bare name standing for the variable of
/// that name, as `format!` has it.
macro_rules! t {
    (@value $name:ident = $value:expr) => {
        $value
    };
    (@value $name:ident) => {
        $name
    };
    ($text:literal) => {
        $crate::i18n::tr($text)
    };
    ($text:literal, $($name:ident $(= $value:expr)?),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($text), &[$((stringify!($name), &t!(@value $name $(= $value)?) as &dyn std::fmt::Display)),+])
    };
}

/// A word that another language says differently in different places:
/// `tc!("job", "Copy")` is the name of a job, where the F5 label is
/// `t!("Copy")`.
macro_rules! tc {
    ($context:literal, $text:literal) => {
        $crate::i18n::tr_context($context, $text)
    };
}

/// A sentence with a count in it, in the form the language gives that
/// count: `tn!(count, "Renamed {count} file", "Renamed {count} files",
/// count)`. The English with a count of one is the key the forms are found
/// by.
macro_rules! tn {
    ($count:expr, $one:literal, $other:literal) => {
        $crate::i18n::tr_count($one, $other, ($count) as u64)
    };
    ($count:expr, $one:literal, $other:literal, $($name:ident $(= $value:expr)?),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr_count($one, $other, ($count) as u64), &[$((stringify!($name), &t!(@value $name $(= $value)?) as &dyn std::fmt::Display)),+])
    };
}

#[cfg(test)]
mod tests;
