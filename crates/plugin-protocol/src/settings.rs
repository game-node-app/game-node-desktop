//! Optional, non-secret declarative settings (API 1.3, `ui.settings`).
//!
//! Hosts validate before rendering and again before dispatching a submission.
//! Plugins validate semantics and acknowledge application/persistence themselves.
//! This closed schema has no HTML, script, CSS, callback, password or IPC field.

use crate::{WireError, proto};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_FIELDS: usize = 32;
pub const MAX_OPTIONS: usize = 32;
pub const MAX_TRANSLATIONS: usize = 8;
pub const MAX_TEXT_LENGTH: u32 = 1024;
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub plugin_id: String,
    pub provider_id: String,
    pub session_id: String,
    pub profile_epoch: u64,
    pub form_handle: String,
}

impl Context {
    pub fn validate(&self) -> Result<(), WireError> {
        if !id(&self.plugin_id)
            || !id(&self.provider_id)
            || !id(&self.session_id)
            || !id(&self.form_handle)
            || self.profile_epoch == 0
        {
            return Err(WireError);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub fallback: String,
    #[serde(default)]
    pub translations: BTreeMap<String, String>,
}

impl Text {
    pub fn plain(value: impl Into<String>) -> Self {
        Self {
            fallback: value.into(),
            translations: BTreeMap::new(),
        }
    }

    fn validate(&self, max: usize) -> Result<(), WireError> {
        if !presentation(&self.fallback, max)
            || self.translations.len() > MAX_TRANSLATIONS
            || self.translations.iter().any(|(locale, value)| {
                locale.is_empty()
                    || locale.len() > 35
                    || !locale
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    || !presentation(value, max)
            })
        {
            return Err(WireError);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    Boolean(bool),
    Text(String),
    Number(f64),
    Choice(String),
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Configuration is non-secret by contract, but diagnostics never need
        // submitted values, including values from a malformed plugin.
        f.write_str("SettingsValue(<redacted>)")
    }
}

pub type Values = BTreeMap<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Kind {
    Boolean,
    Text { min_length: u32, max_length: u32 },
    Number { min: f64, max: f64, step: f64 },
    Choice { options: Vec<Choice> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub id: String,
    pub label: Text,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: String,
    pub label: Text,
    pub help: Option<Text>,
    pub kind: Kind,
    pub default: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    pub schema_version: u32,
    pub form_id: String,
    pub schema_revision: String,
    pub configuration_revision: String,
    pub title: Text,
    pub fields: Vec<Field>,
}

impl Form {
    pub fn validate(&self) -> Result<(), WireError> {
        if self.schema_version != SCHEMA_VERSION
            || !id(&self.form_id)
            || !revision(&self.schema_revision)
            || !revision(&self.configuration_revision)
            || self.fields.is_empty()
            || self.fields.len() > MAX_FIELDS
        {
            return Err(WireError);
        }
        self.title.validate(256)?;
        let mut seen = BTreeSet::new();
        for field in &self.fields {
            if !id(&field.id) || secret_id(&field.id) || !seen.insert(&field.id) {
                return Err(WireError);
            }
            field.label.validate(256)?;
            if let Some(help) = &field.help {
                help.validate(1024)?;
            }
            match &field.kind {
                Kind::Boolean => {}
                Kind::Text {
                    min_length,
                    max_length,
                } => {
                    if min_length > max_length || *max_length > MAX_TEXT_LENGTH {
                        return Err(WireError);
                    }
                }
                Kind::Number { min, max, step } => {
                    if !min.is_finite()
                        || !max.is_finite()
                        || !step.is_finite()
                        || min > max
                        || min.abs().max(max.abs()) > 1e12
                        || *step <= 0.0
                        || (max - min) / step > 1e9
                    {
                        return Err(WireError);
                    }
                }
                Kind::Choice { options } => {
                    if options.is_empty() || options.len() > MAX_OPTIONS {
                        return Err(WireError);
                    }
                    let mut seen = BTreeSet::new();
                    for option in options {
                        if !id(&option.id) || !seen.insert(&option.id) {
                            return Err(WireError);
                        }
                        option.label.validate(256)?;
                    }
                }
            }
            field.validate_value(&field.default)?;
        }
        bounded(&proto::SettingsForm::from(self.clone()))
    }

    /// Complete values are required. Unknown/missing fields never get silently
    /// applied, and current values must satisfy the same schema as defaults.
    pub fn validate_values(&self, values: &Values) -> Result<(), WireError> {
        self.validate()?;
        validate_values(values)?;
        if values.len() != self.fields.len() {
            return Err(WireError);
        }
        for field in &self.fields {
            field.validate_value(values.get(&field.id).ok_or(WireError)?)?;
        }
        Ok(())
    }
}

impl Field {
    fn validate_value(&self, value: &Value) -> Result<(), WireError> {
        match (&self.kind, value) {
            (Kind::Boolean, Value::Boolean(_)) => Ok(()),
            (
                Kind::Text {
                    min_length,
                    max_length,
                },
                Value::Text(value),
            ) => {
                let length = value.chars().count();
                if length < *min_length as usize
                    || length > *max_length as usize
                    || value.chars().any(char::is_control)
                {
                    Err(WireError)
                } else {
                    Ok(())
                }
            }
            (Kind::Number { min, max, step }, Value::Number(value)) => {
                let steps = (value - min) / step;
                if !value.is_finite()
                    || value < min
                    || value > max
                    || (steps - steps.round()).abs() > f64::EPSILON * 64.0 * steps.abs().max(1.0)
                {
                    Err(WireError)
                } else {
                    Ok(())
                }
            }
            (Kind::Choice { options }, Value::Choice(value))
                if options.iter().any(|option| option.id == *value) =>
            {
                Ok(())
            }
            _ => Err(WireError),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    DescribeRead,
    Submit {
        schema_revision: String,
        configuration_revision: String,
        values: Values,
    },
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub context: Context,
    pub action: Action,
}

impl Request {
    pub fn validate(&self) -> Result<(), WireError> {
        self.context.validate()?;
        if let Action::Submit {
            schema_revision,
            configuration_revision,
            values,
        } = &self.action
        {
            if !revision(schema_revision) || !revision(configuration_revision) {
                return Err(WireError);
            }
            validate_values(values)?;
        }
        bounded(&proto::SettingsRequest::from(self.clone()))
    }

    pub fn operation(&self) -> proto::ProviderOperation {
        match self.action {
            Action::DescribeRead => proto::ProviderOperation::SettingsDescribeRead,
            Action::Submit { .. } => proto::ProviderOperation::SettingsSubmit,
            Action::Cancel => proto::ProviderOperation::SettingsCancel,
        }
    }

    /// The host-issued settings scope must agree with its authenticated stream
    /// scope. Plugin identity is separately checked against the registration.
    pub fn matches_service(&self, service: &proto::ServiceContext, plugin_id: &str) -> bool {
        self.context.plugin_id == plugin_id
            && self.context.provider_id == service.provider_id
            && self.context.session_id == service.session_id
            && self.context.profile_epoch == service.profile_epoch
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Persistence {
    Persisted,
    SessionOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    Denied,
    Unavailable,
    TimedOut,
    Cancelled,
    Stale,
    Invalid,
    PersistenceFailed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Form {
        form: Form,
        values: Values,
    },
    Applied {
        configuration_revision: String,
        persistence: Persistence,
    },
    FieldErrors {
        errors: BTreeMap<String, Text>,
    },
    RevisionConflict {
        configuration_revision: String,
    },
    Failed {
        failure: Failure,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub context: Context,
    pub outcome: Outcome,
}

impl Reply {
    pub fn validate(&self) -> Result<(), WireError> {
        self.context.validate()?;
        match &self.outcome {
            Outcome::Form { form, values } => form.validate_values(values)?,
            Outcome::Applied {
                configuration_revision,
                ..
            }
            | Outcome::RevisionConflict {
                configuration_revision,
            } => {
                if !revision(configuration_revision) {
                    return Err(WireError);
                }
            }
            Outcome::FieldErrors { errors } => {
                if errors.is_empty() || errors.len() > MAX_FIELDS {
                    return Err(WireError);
                }
                for (field, message) in errors {
                    if !id(field) || secret_id(field) {
                        return Err(WireError);
                    }
                    message.validate(256)?;
                }
            }
            Outcome::Failed { .. } => {}
        }
        bounded(&proto::SettingsReply::from(self.clone()))
    }

    /// Check correlation before accepting a late response. A form handle alone
    /// does not authorize another plugin, profile or process session.
    pub fn validate_for(&self, request: &Request) -> Result<(), WireError> {
        self.validate()?;
        request.validate()?;
        if self.context != request.context {
            return Err(WireError);
        }
        if let (Action::Submit { values, .. }, Outcome::FieldErrors { errors }) =
            (&request.action, &self.outcome)
            && errors.keys().any(|field| !values.contains_key(field))
        {
            return Err(WireError);
        }
        let allowed = matches!(
            (&request.action, &self.outcome),
            (_, Outcome::Failed { .. })
                | (Action::DescribeRead, Outcome::Form { .. })
                | (
                    Action::Submit { .. },
                    Outcome::Applied { .. }
                        | Outcome::FieldErrors { .. }
                        | Outcome::RevisionConflict { .. },
                )
        );
        if !allowed {
            return Err(WireError);
        }
        Ok(())
    }
}

fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn revision(value: &str) -> bool {
    id(value)
}

fn secret_id(value: &str) -> bool {
    value
        .to_ascii_lowercase()
        .split(['_', '-', '.'])
        .any(|part| {
            matches!(
                part,
                "password"
                    | "passwd"
                    | "secret"
                    | "secrets"
                    | "token"
                    | "cookie"
                    | "cookies"
                    | "credential"
                    | "credentials"
                    | "authorization"
                    | "apikey"
            )
        })
}

fn presentation(value: &str, max: usize) -> bool {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return false;
    }
    // Plain text is rendered as text. Reject executable presentation rather
    // than attempting HTML/script/CSS sanitization or executing it.
    let lower = value.to_ascii_lowercase();
    !value.contains(['<', '>', '{', '}'])
        && ![
            "javascript:",
            "data:",
            "function(",
            "function (",
            "=>",
            "@import",
            "url(",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn validate_values(values: &Values) -> Result<(), WireError> {
    if values.len() > MAX_FIELDS {
        return Err(WireError);
    }
    for (field, value) in values {
        if !id(field) || secret_id(field) {
            return Err(WireError);
        }
        match value {
            Value::Text(text)
                if text.len() > MAX_TEXT_LENGTH as usize * 4
                    || text.chars().count() > MAX_TEXT_LENGTH as usize
                    || text.chars().any(char::is_control) =>
            {
                return Err(WireError);
            }
            Value::Choice(choice) if !id(choice) => return Err(WireError),
            Value::Number(number) if !number.is_finite() || number.abs() > 1e12 => {
                return Err(WireError);
            }
            _ => {}
        }
    }
    Ok(())
}

fn bounded(message: &impl Message) -> Result<(), WireError> {
    if message.encoded_len() > MAX_PAYLOAD_BYTES {
        Err(WireError)
    } else {
        Ok(())
    }
}

mod wire;
