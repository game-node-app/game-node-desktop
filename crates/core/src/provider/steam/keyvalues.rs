//! Parser for Valve's text KeyValues format (`.vdf` / `.acf`).
//!
//! Tokens are quoted (`"key"`, with `\\`, `\"`, `\n`, `\t` escapes) or bare. A key is followed by
//! either a value token or a `{ ... }` block. `// comment` runs to the end of the line, and
//! `[$CONDITION]` tags after a value are ignored. A malformed file fails as a whole.

use std::fmt;

/// Deeper nesting than any real Steam file; guards the recursive parser against hostile input.
const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Block(Vec<(String, Value)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    UnterminatedString,
    UnexpectedClose,
    MissingValue(String),
    UnclosedBlock,
    TooDeep,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnterminatedString => f.write_str("unterminated quoted string"),
            Self::UnexpectedClose => f.write_str("unexpected `}`"),
            Self::MissingValue(key) => write!(f, "key `{key}` has no value"),
            Self::UnclosedBlock => f.write_str("a `{` block is never closed"),
            Self::TooDeep => f.write_str("blocks are nested too deeply"),
        }
    }
}

impl std::error::Error for ParseError {}

impl Value {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Block(_) => None,
        }
    }

    /// The first entry named `key`, ignoring ASCII case like Steam does. `None` for text values.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries()
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_text)
    }

    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Self::Block(entries) => entries,
            Self::Text(_) => &[],
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Text(String),
    Open,
    Close,
}

struct Lexer<'a> {
    rest: &'a str,
}

impl<'a> Lexer<'a> {
    fn skip_blank(&mut self) {
        loop {
            self.rest = self.rest.trim_start();
            if let Some(after) = self.rest.strip_prefix("//") {
                self.rest = after.split_once('\n').map_or("", |(_, tail)| tail);
            } else {
                break;
            }
        }
    }

    fn next(&mut self) -> Result<Option<Token>, ParseError> {
        loop {
            self.skip_blank();
            let Some(first) = self.rest.chars().next() else {
                return Ok(None);
            };
            match first {
                '{' => {
                    self.rest = &self.rest[1..];
                    return Ok(Some(Token::Open));
                }
                '}' => {
                    self.rest = &self.rest[1..];
                    return Ok(Some(Token::Close));
                }
                '"' => return self.quoted().map(|text| Some(Token::Text(text))),
                _ => {
                    let end = self
                        .rest
                        .find(|c: char| c.is_whitespace() || matches!(c, '{' | '}' | '"'))
                        .unwrap_or(self.rest.len());
                    let word = &self.rest[..end];
                    self.rest = &self.rest[end..];
                    // `[$WIN32]`-style conditional tags carry no data for us.
                    if word.starts_with("[$") || word.starts_with("[!$") {
                        continue;
                    }
                    return Ok(Some(Token::Text(word.to_owned())));
                }
            }
        }
    }

    fn quoted(&mut self) -> Result<String, ParseError> {
        let mut out = String::new();
        let mut chars = self.rest[1..].char_indices();
        while let Some((index, c)) = chars.next() {
            match c {
                '"' => {
                    self.rest = &self.rest[1 + index + 1..];
                    return Ok(out);
                }
                '\\' => match chars.next() {
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, 't')) => out.push('\t'),
                    Some((_, '\\')) => out.push('\\'),
                    Some((_, '"')) => out.push('"'),
                    // Unknown escapes keep the backslash, so an unescaped `D:\Games` survives.
                    Some((_, other)) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => return Err(ParseError::UnterminatedString),
                },
                other => out.push(other),
            }
        }
        Err(ParseError::UnterminatedString)
    }
}

/// Parses a whole file into its top-level entries.
pub fn parse(text: &str) -> Result<Vec<(String, Value)>, ParseError> {
    let mut lexer = Lexer {
        rest: text.trim_start_matches('\u{feff}'),
    };
    let entries = parse_block(&mut lexer, 0, true)?;
    Ok(entries)
}

fn parse_block(
    lexer: &mut Lexer<'_>,
    depth: usize,
    top_level: bool,
) -> Result<Vec<(String, Value)>, ParseError> {
    if depth > MAX_DEPTH {
        return Err(ParseError::TooDeep);
    }
    let mut entries = Vec::new();
    loop {
        let key = match lexer.next()? {
            None if top_level => return Ok(entries),
            None => return Err(ParseError::UnclosedBlock),
            Some(Token::Close) if top_level => return Err(ParseError::UnexpectedClose),
            Some(Token::Close) => return Ok(entries),
            Some(Token::Text(key)) => key,
            // A block with no key.
            Some(Token::Open) => return Err(ParseError::MissingValue(String::new())),
        };
        let value = match lexer.next()? {
            Some(Token::Text(text)) => Value::Text(text),
            Some(Token::Open) => Value::Block(parse_block(lexer, depth + 1, false)?),
            Some(Token::Close) | None => return Err(ParseError::MissingValue(key)),
        };
        entries.push((key, value));
    }
}

/// Parses a file whose top level is a single named block, such as `"AppState" { ... }`, and
/// returns that block. The name is matched case-insensitively.
pub fn parse_root(text: &str, name: &str) -> Result<Value, ParseError> {
    let entries = parse(text)?;
    entries
        .into_iter()
        .find(|(key, value)| key.eq_ignore_ascii_case(name) && matches!(value, Value::Block(_)))
        .map(|(_, value)| value)
        .ok_or_else(|| ParseError::MissingValue(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    fn block(entries: &[(&str, Value)]) -> Value {
        Value::Block(
            entries
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn parses_valid_documents() {
        let cases: Vec<(&str, Vec<(String, Value)>)> = vec![
            ("", vec![]),
            ("  \n\t ", vec![]),
            ("\"a\" \"b\"", vec![("a".into(), text("b"))]),
            ("a b", vec![("a".into(), text("b"))]),
            ("\"a\"\t\t\"b c\"", vec![("a".into(), text("b c"))]),
            ("\u{feff}\"a\" \"b\"", vec![("a".into(), text("b"))]),
            (
                "\"p\" { \"x\" \"1\" \"y\" { \"z\" \"2\" } }",
                vec![(
                    "p".into(),
                    block(&[("x", text("1")), ("y", block(&[("z", text("2"))]))]),
                )],
            ),
            ("\"p\"{}", vec![("p".into(), block(&[]))]),
            (
                "// header\n\"a\" \"1\" // trailing\n// more\n\"b\" \"2\"",
                vec![("a".into(), text("1")), ("b".into(), text("2"))],
            ),
            (
                "\"a\" \"1\" [$WIN32]\n\"b\" \"2\"",
                vec![("a".into(), text("1")), ("b".into(), text("2"))],
            ),
            ("\"a\" [$X360] \"1\"", vec![("a".into(), text("1"))]),
            (
                "\"path\" \"D:\\\\Games\\\\Steam\"",
                vec![("path".into(), text("D:\\Games\\Steam"))],
            ),
            (
                "\"a\" \"line\\nbreak\\t\\\"q\\\"\"",
                vec![("a".into(), text("line\nbreak\t\"q\""))],
            ),
            ("\"a\" \"D:\\Games\"", vec![("a".into(), text("D:\\Games"))]),
            ("\"名前\" \"ゲーム\"", vec![("名前".into(), text("ゲーム"))]),
            (
                "\"url\" \"http://x/y\"",
                vec![("url".into(), text("http://x/y"))],
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                parse(input).as_deref(),
                Ok(expected.as_slice()),
                "{input:?}"
            );
        }
    }

    #[test]
    fn rejects_malformed_documents() {
        let cases = [
            ("\"a", ParseError::UnterminatedString),
            ("\"a\" \"b", ParseError::UnterminatedString),
            ("\"a\" \"b\\", ParseError::UnterminatedString),
            ("}", ParseError::UnexpectedClose),
            ("\"a\" \"1\" }", ParseError::UnexpectedClose),
            ("\"a\"", ParseError::MissingValue("a".into())),
            ("\"a\" }", ParseError::MissingValue("a".into())),
            ("\"a\" {", ParseError::UnclosedBlock),
            ("\"a\" { \"b\" \"1\"", ParseError::UnclosedBlock),
            ("{ }", ParseError::MissingValue(String::new())),
        ];
        for (input, expected) in cases {
            assert_eq!(parse(input), Err(expected), "{input:?}");
        }
    }

    #[test]
    fn refuses_excessive_nesting_without_overflowing() {
        let deep = "\"a\" {".repeat(10_000);
        assert_eq!(parse(&deep), Err(ParseError::TooDeep));
    }

    #[test]
    fn lookups_ignore_key_case() {
        let root = parse_root(
            "\"appstate\" { \"AppID\" \"620\" \"Name\" \"Portal 2\" }",
            "AppState",
        )
        .unwrap();
        assert_eq!(root.text("appid"), Some("620"));
        assert_eq!(root.text("NAME"), Some("Portal 2"));
        assert_eq!(root.text("missing"), None);
        assert_eq!(root.get("appid").and_then(Value::as_text), Some("620"));
    }

    #[test]
    fn parse_root_needs_the_named_block() {
        assert!(parse_root("\"other\" { }", "AppState").is_err());
        assert!(parse_root("\"AppState\" \"text\"", "AppState").is_err());
        assert!(parse_root("garbage \"", "AppState").is_err());
    }
}
