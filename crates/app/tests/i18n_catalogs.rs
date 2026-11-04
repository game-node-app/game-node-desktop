//! Every sentence the UI translates must exist in both catalogs, with the same placeholders.
//!
//! The Slint sources are the truth: this reads each `@tr(...)` call, so a string added to a
//! screen without a catalog entry fails here instead of silently showing English.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

type Key = (String, String);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Reads a double-quoted Slint string starting at `at`; returns it and the index after it.
fn literal(source: &[char], mut at: usize) -> (String, usize) {
    assert_eq!(source[at], '"');
    at += 1;
    let mut value = String::new();
    while source[at] != '"' {
        if source[at] == '\\' {
            value.push(match source[at + 1] {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
            at += 2;
        } else {
            value.push(source[at]);
            at += 1;
        }
    }
    (value, at + 1)
}

fn skip_space(source: &[char], mut at: usize) -> usize {
    while source[at].is_whitespace() {
        at += 1;
    }
    at
}

/// Every `(context, message)` a `.slint` file translates. The default context is the component
/// the call sits in; every call in this app is in `AppWindow` unless it names a context.
fn translated(source: &str) -> Vec<Key> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at + 4 < chars.len() {
        if chars[at..at + 4] != ['@', 't', 'r', '('] {
            at += 1;
            continue;
        }
        let start = skip_space(&chars, at + 4);
        at += 4;
        // `@tr()` written in a comment has no string after it.
        if chars[start] != '"' {
            continue;
        }
        let (first, after) = literal(&chars, start);
        let next = skip_space(&chars, after);
        if chars[next..].starts_with(&['=', '>']) {
            let message = skip_space(&chars, next + 2);
            let (message, _) = literal(&chars, message);
            found.push((first, message));
        } else {
            found.push(("AppWindow".to_owned(), first));
        }
    }
    found
}

fn slint_messages() -> BTreeSet<Key> {
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
    let mut keys = BTreeSet::new();
    for entry in std::fs::read_dir(ui).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "slint") {
            keys.extend(translated(&std::fs::read_to_string(path).unwrap()));
        }
    }
    keys
}

fn field(entry: &str, name: &str) -> Option<String> {
    let prefix = format!("{name} ");
    entry
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|value| serde_json::from_str::<String>(value).expect("a quoted PO string"))
}

/// `(context, msgid) -> msgstr` for one locale.
fn catalog(locale: &str) -> BTreeMap<Key, String> {
    let path = root().join(format!("i18n/{locale}/LC_MESSAGES/gamenode-app.po"));
    let text = std::fs::read_to_string(path).unwrap();
    let mut entries = BTreeMap::new();
    for entry in text.split("\n\n") {
        let (Some(context), Some(id), Some(string)) =
            (field(entry, "msgctxt"), field(entry, "msgid"), field(entry, "msgstr"))
        else {
            continue;
        };
        let duplicate = entries.insert((context.clone(), id.clone()), string);
        assert!(duplicate.is_none(), "{locale}: {context}/{id:?} appears twice");
    }
    entries
}

fn placeholders(text: &str) -> usize {
    text.matches("{}").count()
}

#[test]
fn the_screens_translate_the_library_sentences() {
    let messages = slint_messages();
    assert!(
        messages.iter().filter(|(context, _)| context == "Library").count() > 80,
        "the extractor should see the library screens' sentences"
    );
}

#[test]
fn both_catalogs_cover_every_translated_sentence() {
    let messages = slint_messages();
    for locale in ["en", "pt_BR"] {
        let catalog = catalog(locale);
        let missing: Vec<_> = messages.iter().filter(|key| !catalog.contains_key(*key)).collect();
        assert!(missing.is_empty(), "{locale} is missing {missing:#?}");
    }
}

#[test]
fn translations_are_filled_and_keep_their_placeholders() {
    let english = catalog("en");
    let portuguese = catalog("pt_BR");
    for ((context, id), translation) in &portuguese {
        assert!(!translation.is_empty(), "pt_BR {context}/{id:?} is empty");
        assert_eq!(
            placeholders(id),
            placeholders(translation),
            "pt_BR {context}/{id:?} changes the placeholders"
        );
    }
    let english_keys: BTreeSet<&Key> = english.keys().collect();
    let portuguese_keys: BTreeSet<&Key> = portuguese.keys().collect();
    let only_in_one: Vec<_> = english_keys.symmetric_difference(&portuguese_keys).collect();
    assert!(only_in_one.is_empty(), "catalogs differ: {only_in_one:#?}");
}
