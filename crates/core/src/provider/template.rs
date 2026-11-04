//! Launch and install URI templates such as `steam://rungameid/{id}`.
//!
//! Only `{name}` placeholders with `[a-z_]+` names are allowed, and the scheme must be literal.
//! Filled values are percent-encoded so they cannot add path segments, queries, fragments or
//! userinfo, and values with control characters are refused. The filled URI's scheme is checked
//! against the template's literal scheme before it is returned.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use url::Url;

use super::record::Record;

/// Everything except the RFC 3986 unreserved characters is encoded.
const ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    #[error("the template is empty")]
    Empty,
    #[error("the template is not a URI: it needs a literal scheme followed by `://`")]
    NotAUri,
    #[error("the scheme `{0}` is not a valid URI scheme")]
    BadScheme(String),
    #[error("a placeholder appears in the scheme")]
    PlaceholderInScheme,
    #[error("malformed placeholder in `{0}`: use {{name}} with lowercase letters and `_`")]
    MalformedPlaceholder(String),
    #[error("the template does not produce a valid URI")]
    InvalidUri,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FillError {
    /// The record has no such field, so the action is unavailable for it.
    #[error("the game has no `{0}` field")]
    MissingField(String),
    /// The field value contains a control character.
    #[error("the `{0}` field contains a control character")]
    ControlCharacter(String),
    #[error("the filled URI does not keep the template's scheme")]
    SchemeChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Literal(String),
    Field(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    scheme: String,
    parts: Vec<Part>,
}

impl Template {
    pub fn parse(text: &str) -> Result<Self, TemplateError> {
        if text.is_empty() {
            return Err(TemplateError::Empty);
        }
        let Some(split) = text.find("://") else {
            return Err(TemplateError::NotAUri);
        };
        let scheme = &text[..split];
        if scheme.contains(['{', '}']) {
            return Err(TemplateError::PlaceholderInScheme);
        }
        let mut chars = scheme.chars();
        let valid_scheme = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if !valid_scheme {
            return Err(TemplateError::BadScheme(scheme.to_owned()));
        }

        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut rest = text;
        while let Some(open) = rest.find(['{', '}']) {
            literal.push_str(&rest[..open]);
            if rest.as_bytes()[open] == b'}' {
                return Err(TemplateError::MalformedPlaceholder(text.to_owned()));
            }
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else {
                return Err(TemplateError::MalformedPlaceholder(text.to_owned()));
            };
            let name = &after[..close];
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                return Err(TemplateError::MalformedPlaceholder(text.to_owned()));
            }
            if !literal.is_empty() {
                parts.push(Part::Literal(std::mem::take(&mut literal)));
            }
            parts.push(Part::Field(name.to_owned()));
            rest = &after[close + 1..];
        }
        literal.push_str(rest);
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }

        let template = Self {
            scheme: scheme.to_ascii_lowercase(),
            parts,
        };
        // The template must be a URI once filled; probe with a harmless value.
        let probe = template.render(|_| Some("0".to_owned()));
        match probe.as_deref().map(Url::parse) {
            Some(Ok(url)) if url.scheme() == template.scheme => Ok(template),
            _ => Err(TemplateError::InvalidUri),
        }
    }

    /// The template's literal, lowercase scheme.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// The names of the fields the template needs.
    pub fn fields(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter_map(|part| match part {
            Part::Field(name) => Some(name.as_str()),
            Part::Literal(_) => None,
        })
    }

    fn render(&self, mut value: impl FnMut(&str) -> Option<String>) -> Option<String> {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Literal(text) => out.push_str(text),
                Part::Field(name) => out.push_str(&value(name)?),
            }
        }
        Some(out)
    }

    /// Whether every field the template needs exists on the record.
    pub fn applies_to(&self, record: &Record) -> bool {
        self.fields().all(|name| record.field(name).is_some())
    }

    /// Fills the template from the record's fields.
    pub fn fill(&self, record: &Record) -> Result<Url, FillError> {
        let mut failure = None;
        let text = self.render(|name| match record.field(name) {
            None => {
                failure.get_or_insert(FillError::MissingField(name.to_owned()));
                None
            }
            Some(value) if value.chars().any(char::is_control) => {
                failure.get_or_insert(FillError::ControlCharacter(name.to_owned()));
                None
            }
            Some(value) => Some(utf8_percent_encode(value, ENCODE).to_string()),
        });
        let Some(text) = text else {
            return Err(failure.unwrap_or(FillError::SchemeChanged));
        };
        let url = Url::parse(&text).map_err(|_| FillError::SchemeChanged)?;
        if url.scheme() != self.scheme {
            return Err(FillError::SchemeChanged);
        }
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str) -> Record {
        Record {
            id: id.to_owned(),
            name: Some("Portal".to_owned()),
            install_dir: "/games/Portal".to_owned(),
            fields: Default::default(),
        }
    }

    #[test]
    fn fills_the_steam_launch_uri() {
        let template = Template::parse("steam://rungameid/{id}").unwrap();
        assert_eq!(template.scheme(), "steam");
        assert_eq!(
            template.fill(&record("620")).unwrap().as_str(),
            "steam://rungameid/620"
        );
    }

    #[test]
    fn encodes_reserved_characters_so_structure_cannot_change() {
        let template = Template::parse("steam://rungameid/{id}").unwrap();
        for (value, expected) in [
            ("a/b", "steam://rungameid/a%2Fb"),
            ("a?b", "steam://rungameid/a%3Fb"),
            ("a#b", "steam://rungameid/a%23b"),
            ("100%", "steam://rungameid/100%25"),
            ("a b", "steam://rungameid/a%20b"),
            ("x@evil", "steam://rungameid/x%40evil"),
            (
                "javascript:alert(1)",
                "steam://rungameid/javascript%3Aalert%281%29",
            ),
            ("ü", "steam://rungameid/%C3%BC"),
        ] {
            let url = template.fill(&record(value)).unwrap();
            assert_eq!(url.as_str(), expected, "{value}");
            assert_eq!(url.scheme(), "steam");
            assert_eq!(url.query(), None);
            assert_eq!(url.fragment(), None);
            assert_eq!(url.path_segments().unwrap().count(), 1);
        }
    }

    #[test]
    fn control_characters_are_rejected() {
        let template = Template::parse("steam://rungameid/{id}").unwrap();
        for value in ["a\nb", "a\tb", "a\0b", "\r", "a\u{7f}"] {
            assert_eq!(
                template.fill(&record(value)),
                Err(FillError::ControlCharacter("id".to_owned())),
                "{value:?}"
            );
        }
    }

    #[test]
    fn a_missing_field_makes_the_action_unavailable() {
        let template = Template::parse("store://{id}/{edition}").unwrap();
        let game = record("1");
        assert!(!template.applies_to(&game));
        assert_eq!(
            template.fill(&game),
            Err(FillError::MissingField("edition".to_owned()))
        );

        let mut with_edition = record("1");
        with_edition
            .fields
            .insert("edition".to_owned(), "deluxe".to_owned());
        assert!(template.applies_to(&with_edition));
        assert_eq!(
            template.fill(&with_edition).unwrap().as_str(),
            "store://1/deluxe"
        );
    }

    #[test]
    fn parse_rejects_bad_templates() {
        let cases = [
            ("", TemplateError::Empty),
            ("rungameid/{id}", TemplateError::NotAUri),
            ("{id}://x", TemplateError::PlaceholderInScheme),
            ("st{id}am://x", TemplateError::PlaceholderInScheme),
            ("1steam://x", TemplateError::BadScheme("1steam".to_owned())),
            ("://x", TemplateError::BadScheme(String::new())),
            (
                "steam://run/{id",
                TemplateError::MalformedPlaceholder("steam://run/{id".to_owned()),
            ),
            (
                "steam://run/{}",
                TemplateError::MalformedPlaceholder("steam://run/{}".to_owned()),
            ),
            (
                "steam://run/{Id}",
                TemplateError::MalformedPlaceholder("steam://run/{Id}".to_owned()),
            ),
            (
                "steam://run/id}",
                TemplateError::MalformedPlaceholder("steam://run/id}".to_owned()),
            ),
            (
                "steam://run/{id1}",
                TemplateError::MalformedPlaceholder("steam://run/{id1}".to_owned()),
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(Template::parse(text), Err(expected), "{text:?}");
        }
    }

    #[test]
    fn scheme_is_compared_case_insensitively() {
        let template = Template::parse("Steam://run/{id}").unwrap();
        assert_eq!(template.scheme(), "steam");
        assert_eq!(
            template.fill(&record("7")).unwrap().as_str(),
            "steam://run/7"
        );
    }
}
