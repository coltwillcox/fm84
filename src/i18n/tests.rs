use super::*;
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

#[test]
fn serbian_counts_take_one_few_or_other() {
    let form = |count| Language::Serbian.plural(count);
    for one in [1, 21, 31, 101, 1001] {
        assert_eq!(form(one), Form::One, "{one}");
    }
    for few in [2, 3, 4, 22, 34, 102] {
        assert_eq!(form(few), Form::Few, "{few}");
    }
    for other in [0, 5, 10, 11, 12, 13, 14, 15, 20, 111, 112, 114] {
        assert_eq!(form(other), Form::Other, "{other}");
    }
    assert_eq!(Language::English.plural(1), Form::One);
    assert_eq!(Language::English.plural(0), Form::Other);
    assert_eq!(Language::Turkish.plural(1), Form::One);
    assert_eq!(Language::Turkish.plural(21), Form::Other);
    // Slovenian has a dual, by the last two digits.
    let slovenian = |count| Language::Slovenian.plural(count);
    assert_eq!(
        [1, 2, 3, 4, 5, 11, 12, 101, 102, 103, 111].map(slovenian),
        [Form::One, Form::Two, Form::Few, Form::Few, Form::Other, Form::Other, Form::Other, Form::One, Form::Two, Form::Few, Form::Other]
    );
    // Macedonian: ending in 1, but not in 11.
    let macedonian = |count| Language::Macedonian.plural(count);
    assert_eq!([1, 2, 11, 21, 101, 111].map(macedonian), [Form::One, Form::Other, Form::Other, Form::One, Form::One, Form::Other]);
    // Slovak: 2 to 4 few, by the whole number - 22 is other.
    let slovak = |count| Language::Slovak.plural(count);
    assert_eq!([1, 2, 4, 5, 21, 22].map(slovak), [Form::One, Form::Few, Form::Few, Form::Other, Form::Other, Form::Other]);
    // Polish: 1 alone one; 22 few, but 12 and 21 other.
    let polish = |count| Language::Polish.plural(count);
    assert_eq!([1, 2, 4, 5, 12, 21, 22, 112, 122].map(polish), [Form::One, Form::Few, Form::Few, Form::Other, Form::Other, Form::Other, Form::Few, Form::Other, Form::Few]);
    // Irish by the whole number: 7 to 10 apart from 3 to 6, and 11 on as
    // other.
    let irish = |count| Language::Irish.plural(count);
    assert_eq!(
        [0, 1, 2, 3, 6, 7, 10, 11, 21, 102].map(irish),
        [Form::Other, Form::One, Form::Two, Form::Few, Form::Few, Form::Many, Form::Many, Form::Other, Form::Other, Form::Other]
    );
    // French counts nothing as one.
    assert_eq!(Language::French.plural(0), Form::One);
    assert_eq!(Language::French.plural(1), Form::One);
    assert_eq!(Language::French.plural(2), Form::Other);
}

#[test]
fn a_locale_names_its_language() {
    assert_eq!(Language::from_locale("sr_RS.UTF-8"), Some(Language::Serbian));
    assert_eq!(Language::from_locale("sr@latin"), Some(Language::Serbian));
    assert_eq!(Language::from_locale("en-GB"), Some(Language::English));
    assert_eq!(Language::from_locale("C.UTF-8"), Some(Language::English));
    assert_eq!(Language::from_locale("POSIX"), Some(Language::English));
    assert_eq!(Language::from_locale("tr_TR.UTF-8"), Some(Language::Turkish));
    assert_eq!(Language::from_locale("nb_NO.UTF-8"), Some(Language::Norwegian));
    assert_eq!(Language::from_locale("no_NO"), Some(Language::Norwegian));
    assert_eq!(Language::from_locale("nn_NO.UTF-8"), None);
    assert_eq!(Language::from_locale("de_DE.UTF-8"), Some(Language::German));
    assert_eq!(Language::from_locale("de_AT"), Some(Language::German));
    assert_eq!(Language::from_locale("it_IT.UTF-8"), Some(Language::Italian));
    assert_eq!(Language::from_locale("el_GR.UTF-8"), Some(Language::Greek));
    assert_eq!(Language::from_locale("fr_FR.UTF-8"), Some(Language::French));
    assert_eq!(Language::from_locale("fr_CA"), Some(Language::French));
    assert_eq!(Language::from_locale("sl_SI.UTF-8"), Some(Language::Slovenian));
    assert_eq!(Language::from_locale("hr_HR.UTF-8"), Some(Language::Croatian));
    assert_eq!(Language::from_locale("mk_MK.UTF-8"), Some(Language::Macedonian));
    assert_eq!(Language::from_locale("hu_HU.UTF-8"), Some(Language::Hungarian));
    assert_eq!(Language::from_locale("sk_SK.UTF-8"), Some(Language::Slovak));
    assert_eq!(Language::from_locale("cs_CZ.UTF-8"), Some(Language::Czech));
    assert_eq!(Language::from_locale("pl_PL.UTF-8"), Some(Language::Polish));
    assert_eq!(Language::from_locale("sv_SE.UTF-8"), Some(Language::Swedish));
    assert_eq!(Language::from_locale("es_ES.UTF-8"), Some(Language::Spanish));
    assert_eq!(Language::from_locale("pt_BR.UTF-8"), Some(Language::Portuguese));
    assert_eq!(Language::from_locale("nl_NL.UTF-8"), Some(Language::Dutch));
    assert_eq!(Language::from_locale("da_DK.UTF-8"), Some(Language::Danish));
    assert_eq!(Language::from_locale("ga_IE.UTF-8"), Some(Language::Irish));
    assert_eq!(Language::from_locale("ja_JP.UTF-8"), None);
    for language in Language::ALL {
        assert_eq!(Language::from_key(language.key()), Some(language));
    }
}

#[test]
fn a_catalog_reads_and_says_what_is_wrong_with_one_that_does_not() {
    let catalog = parse("# comment\nmsgid  Open {name}\nmsgstr Otvori {name}\n\nmsgid Two\\nlines\nmsgstr Dva\\nreda\n\nmsgid {count} file\nmsgstr[one] {count} datoteka\nmsgstr[few] {count} datoteke\n").unwrap();
    assert_eq!(catalog.plain["Open {name}"], "Otvori {name}");
    assert_eq!(catalog.plain["Two\nlines"], "Dva\nreda");
    assert_eq!(catalog.forms[&("{count} file".to_string(), Form::Few)], "{count} datoteke");
    let catalog = parse("msgid {count} file\nmsgstr[many] {count} gcomhad\n").unwrap();
    assert_eq!(catalog.forms[&("{count} file".to_string(), Form::Many)], "{count} gcomhad");
    let catalog = parse("msgid Copy\nmsgstr Kopiraj\nmsgctxt job\nmsgid Copy\nmsgstr Kopiranje\n").unwrap();
    assert_eq!(catalog.plain["Copy"], "Kopiraj");
    assert_eq!(catalog.plain[&keyed("job", "Copy")], "Kopiranje");
    assert_eq!(parse("msgctxt job\nmsgstr Kopiranje").err().map(|error| error.line), Some(2));
    for (wrong, line) in [
        ("msgstr Alone", 1),
        ("msgid One\nmsgid Two\nmsgstr Dva", 2),
        ("msgid One", 1),
        ("msgid One\nmsgstr Jedan\nmsgstr Opet", 3),
        ("msgid One\nmsgstr[three] Tri", 2),
        ("msgid One\nmsgtext Jedan", 2),
        ("msgid One\nmsgstr", 2),
    ] {
        assert_eq!(parse(wrong).err().map(|error| error.line), Some(line), "{wrong}");
    }
}

#[test]
fn places_are_filled_by_name() {
    assert_eq!(fill("{a} and {b}, {a}", &[("a", &1), ("b", &"two")]), "1 and two, 1");
    assert_eq!(fill("{missing} {", &[("a", &1)]), "{missing} {");
    let name = "x.txt";
    assert_eq!(t!("Cannot open {name}: {e}", name, e = "gone"), "Cannot open x.txt: gone");
    assert_eq!(tn!(1, "{count} file", "{count} files", count = 1), "1 file");
    assert_eq!(tn!(3, "{count} file", "{count} files", count = 3), "3 files");
}

#[test]
fn the_language_shown_is_the_one_set() {
    let english = "Cannot save options: {e}";
    set_language(Language::Serbian);
    let shown = tr(english);
    set_language(Language::English);
    assert_ne!(shown, english, "no Serbian for a sentence the catalog has");
    assert_eq!(tr(english), english);
    // What a catalog has no words for is shown in English.
    set_language(Language::Serbian);
    assert_eq!(tr("not a sentence fm84 has"), "not a sentence fm84 has");
    set_language(Language::English);
}

/// A sentence from the code: its English, and for one with a count the
/// English for many too.
struct Sentence {
    english: String,
    other: Option<String>,
    at: String,
}

/// Every `t!` and `tn!` in the code, by the literals in it.
fn sentences() -> Vec<Sentence> {
    let mut found = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut dirs = vec![root];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") && !path.starts_with(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/i18n")) && !path.ends_with("src/i18n.rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                found.extend(sentences_in(&text, &path.display().to_string()));
            }
        }
    }
    found
}

fn sentences_in(text: &str, file: &str) -> Vec<Sentence> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    for (start, _) in text.match_indices("!(") {
        let counted = start >= 2 && &text[start - 2..start] == "tn";
        let contextual = start >= 2 && &text[start - 2..start] == "tc";
        let name_start = if counted || contextual { start - 2 } else { start.saturating_sub(1) };
        if !(counted || contextual || (start >= 1 && &text[start - 1..start] == "t")) {
            continue;
        }
        if name_start > 0 && (bytes[name_start - 1].is_ascii_alphanumeric() || bytes[name_start - 1] == b'_') {
            continue;
        }
        let line = text[..start].lines().count();
        let at = format!("{file}:{line}");
        let mut rest = &text[start + 2..];
        if counted {
            // Past the count, to the first literal.
            let Some(quote) = rest.find('"') else { continue };
            rest = &rest[quote..];
        }
        let rest = rest.trim_start();
        assert!(!rest.starts_with('r'), "{at}: a raw string in t!, which the check cannot read");
        let Some((english, after)) = literal(rest) else {
            panic!("{at}: t! takes a string literal first");
        };
        if contextual {
            let after = after.trim_start().strip_prefix(',').unwrap_or_else(|| panic!("{at}: tc! takes two literals")).trim_start();
            let text = literal(after).unwrap_or_else(|| panic!("{at}: tc! takes two literals")).0;
            found.push(Sentence {
                english: keyed(&english, &text),
                other: None,
                at,
            });
            continue;
        }
        let other = if counted {
            let after = after.trim_start().strip_prefix(',').unwrap_or_else(|| panic!("{at}: tn! takes two literals")).trim_start();
            Some(literal(after).unwrap_or_else(|| panic!("{at}: tn! takes two literals")).0)
        } else {
            None
        };
        found.push(Sentence { english, other, at });
    }
    found
}

/// The string literal `text` starts with, as the program has it, and what
/// follows it.
fn literal(text: &str) -> Option<(String, &str)> {
    let mut chars = text.strip_prefix('"')?.char_indices();
    let mut out = String::new();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((out, &text[at + 2..])),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '0' => out.push('\0'),
                'u' => {
                    let mut hex = String::new();
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                        hex.push(c);
                    }
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                // A line carried on: the break and the indent after it go.
                '\n' => {
                    for (_, c) in chars.clone() {
                        if !c.is_whitespace() {
                            break;
                        }
                        chars.next();
                    }
                }
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            c => out.push(c),
        }
    }
    None
}

/// The `{name}` places in a sentence.
fn places(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if after[..close].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && close > 0 => {
                found.insert(after[..close].to_string());
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    found
}

/// The languages that have words for everything, and are to keep it so:
/// one of these missing a sentence fails the test. Any other shipped one
/// shows what it lacks in English.
const COMPLETE: &[Language] = &[
    Language::Serbian,
    Language::Turkish,
    Language::Norwegian,
    Language::German,
    Language::Italian,
    Language::Greek,
    Language::French,
    Language::Slovenian,
    Language::Croatian,
    Language::Macedonian,
    Language::Hungarian,
    Language::Slovak,
    Language::Czech,
    Language::Polish,
    Language::Swedish,
    Language::Spanish,
    Language::Portuguese,
    Language::Dutch,
    Language::Danish,
    Language::Irish,
];

#[test]
fn the_scan_finds_each_kind_of_sentence() {
    let code = "let a = t!(\"Plain\");\nprint!(\"not one\");\nlet b = t!(\"With {name}\", name);\nlet c = tn!(items.len(), \"{count} file\", \"{count} files\", count);\nlet d = t!(\"Carried \\\n        on\");\nlet e = t!(\"A \\\"quote\\\" \\u{2192}\");\nlet f = tc!(\"job\", \"Copy\");\n";
    let found = sentences_in(code, "x.rs");
    let english: Vec<(&str, Option<&str>)> = found.iter().map(|sentence| (sentence.english.as_str(), sentence.other.as_deref())).collect();
    assert_eq!(
        english,
        [
            ("Plain", None),
            ("With {name}", None),
            ("{count} file", Some("{count} files")),
            ("Carried on", None),
            ("A \"quote\" \u{2192}", None),
            ("job\u{4}Copy", None)
        ]
    );
    assert_eq!(found[2].at, "x.rs:4");
}

/// Every shipped catalog reads; has words for every sentence in the code,
/// in every form a count takes; fills the same places the English does;
/// and has nothing for a sentence the code no longer has.
#[test]
fn every_catalog_is_whole_and_up_to_date() {
    let sentences = sentences();
    let mut problems = Vec::new();
    for sentence in &sentences {
        let english = &sentence.english;
        if english.trim() != english {
            problems.push(format!("{}: spaces around {english:?}, which a catalog cannot hold - put them outside t!", sentence.at));
        }
        if let Some(other) = &sentence.other
            && places(english) != places(other)
        {
            problems.push(format!("{}: the two forms of {english:?} fill different places", sentence.at));
        }
    }
    let plain: HashSet<&str> = sentences.iter().filter(|sentence| sentence.other.is_none()).map(|sentence| sentence.english.as_str()).collect();
    let counted: HashSet<&str> = sentences.iter().filter(|sentence| sentence.other.is_some()).map(|sentence| sentence.english.as_str()).collect();
    for language in Language::ALL {
        let Some(source) = language.source() else { continue };
        let catalog = match parse(source) {
            Ok(catalog) => catalog,
            Err(error) => {
                problems.push(format!("lang/{}.lang:{}: {}", language.key(), error.line, error.message));
                continue;
            }
        };
        let whole = COMPLETE.contains(&language);
        // Every form the language's own rule gives a count.
        let mut forms: Vec<Form> = Vec::new();
        for count in 0..1000 {
            let form = language.plural(count);
            if !forms.contains(&form) {
                forms.push(form);
            }
        }
        for sentence in &sentences {
            let english = &sentence.english;
            let mut wanted = places(english);
            if let Some(other) = &sentence.other {
                wanted.extend(places(other));
                for &form in &forms {
                    match catalog.forms.get(&(english.clone(), form)) {
                        None if whole => problems.push(format!("{}: no {form:?} form in {} for {english:?}", sentence.at, language.key())),
                        None => {}
                        Some(translated) if places(translated) != wanted => problems.push(format!("{}: {} fills {:?} for {english:?}, not {wanted:?}", sentence.at, language.key(), places(translated))),
                        Some(_) => {}
                    }
                }
            } else {
                match catalog.plain.get(english) {
                    None if whole => problems.push(format!("{}: nothing in {} for {english:?}", sentence.at, language.key())),
                    None => {}
                    Some(translated) if places(translated) != wanted => problems.push(format!("{}: {} fills {:?} for {english:?}, not {wanted:?}", sentence.at, language.key(), places(translated))),
                    Some(_) => {}
                }
            }
        }
        for english in catalog.plain.keys() {
            if !plain.contains(english.as_str()) {
                problems.push(format!("lang/{}.lang: {english:?} is no longer in the code{}", language.key(), if counted.contains(english.as_str()) { " without a count" } else { "" }));
            }
        }
        for (english, _) in catalog.forms.keys() {
            if !counted.contains(english.as_str()) {
                problems.push(format!("lang/{}.lang: {english:?} is no longer in the code with a count", language.key()));
            }
        }
    }
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), problems.join("\n"));
}

/// How a language's own name sorts: in Latin letters, without the marks
/// over them, so Čeština sits among the C's and Ελληνικά among the E's.
fn sort_name(language: Language) -> String {
    let label = match language.label() {
        "Ελληνικά" => "Ellinika",
        "Македонски" => "Makedonski",
        "Български" => "Balgarski",
        label => label,
    };
    label
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ă' | 'å' | 'ä' | 'ã' => 'a',
            'č' | 'ç' | 'Č' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'î' | 'ı' => 'i',
            'ó' | 'ô' | 'ö' | 'ø' | 'õ' => 'o',
            'š' | 'ș' | 'ş' => 's',
            'ț' | 'ţ' => 't',
            'ú' | 'ü' => 'u',
            'ž' => 'z',
            'ñ' => 'n',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

#[test]
fn english_comes_first_then_the_rest_by_their_own_names() {
    assert_eq!(Language::ALL[0], Language::English);
    let names: Vec<String> = Language::ALL[1..].iter().map(|&language| sort_name(language)).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}
