use super::*;

impl From<Context> for proto::SettingsContext {
    fn from(v: Context) -> Self {
        Self {
            plugin_id: v.plugin_id,
            provider_id: v.provider_id,
            session_id: v.session_id,
            profile_epoch: v.profile_epoch,
            form_handle: v.form_handle,
        }
    }
}
impl From<proto::SettingsContext> for Context {
    fn from(v: proto::SettingsContext) -> Self {
        Self {
            plugin_id: v.plugin_id,
            provider_id: v.provider_id,
            session_id: v.session_id,
            profile_epoch: v.profile_epoch,
            form_handle: v.form_handle,
        }
    }
}
impl From<Text> for proto::SettingsText {
    fn from(v: Text) -> Self {
        Self {
            fallback: v.fallback,
            translations: v.translations.into_iter().collect(),
        }
    }
}
impl From<proto::SettingsText> for Text {
    fn from(v: proto::SettingsText) -> Self {
        Self {
            fallback: v.fallback,
            translations: v.translations.into_iter().collect(),
        }
    }
}
impl From<Value> for proto::SettingsValue {
    fn from(v: Value) -> Self {
        use proto::settings_value::Value as P;
        Self {
            value: Some(match v {
                Value::Boolean(v) => P::Boolean(v),
                Value::Text(v) => P::Text(v),
                Value::Number(v) => P::Number(v),
                Value::Choice(v) => P::Choice(v),
            }),
        }
    }
}
impl TryFrom<proto::SettingsValue> for Value {
    type Error = WireError;
    fn try_from(v: proto::SettingsValue) -> Result<Self, WireError> {
        use proto::settings_value::Value as P;
        Ok(match v.value.ok_or(WireError)? {
            P::Boolean(v) => Value::Boolean(v),
            P::Text(v) => Value::Text(v),
            P::Number(v) => Value::Number(v),
            P::Choice(v) => Value::Choice(v),
        })
    }
}
impl From<Field> for proto::SettingsField {
    fn from(v: Field) -> Self {
        use proto::settings_field::Kind as P;
        Self {
            id: v.id,
            label: Some(v.label.into()),
            help: v.help.map(Into::into),
            default_value: Some(v.default.into()),
            kind: Some(match v.kind {
                Kind::Boolean => P::Boolean(true),
                Kind::Text {
                    min_length,
                    max_length,
                } => P::Text(proto::SettingsTextField {
                    min_length,
                    max_length,
                }),
                Kind::Number { min, max, step } => {
                    P::Number(proto::SettingsNumberField { min, max, step })
                }
                Kind::Choice { options } => P::Choice(proto::SettingsChoiceField {
                    options: options
                        .into_iter()
                        .map(|v| proto::SettingsChoiceOption {
                            id: v.id,
                            label: Some(v.label.into()),
                        })
                        .collect(),
                }),
            }),
        }
    }
}
impl TryFrom<proto::SettingsField> for Field {
    type Error = WireError;
    fn try_from(v: proto::SettingsField) -> Result<Self, WireError> {
        use proto::settings_field::Kind as P;
        Ok(Self {
            id: v.id,
            label: v.label.ok_or(WireError)?.into(),
            help: v.help.map(Into::into),
            default: v.default_value.ok_or(WireError)?.try_into()?,
            kind: match v.kind.ok_or(WireError)? {
                P::Boolean(true) => Kind::Boolean,
                P::Boolean(false) => return Err(WireError),
                P::Text(v) => Kind::Text {
                    min_length: v.min_length,
                    max_length: v.max_length,
                },
                P::Number(v) => Kind::Number {
                    min: v.min,
                    max: v.max,
                    step: v.step,
                },
                P::Choice(v) => Kind::Choice {
                    options: v
                        .options
                        .into_iter()
                        .map(|v| {
                            Ok(Choice {
                                id: v.id,
                                label: v.label.ok_or(WireError)?.into(),
                            })
                        })
                        .collect::<Result<_, WireError>>()?,
                },
            },
        })
    }
}
impl From<Form> for proto::SettingsForm {
    fn from(v: Form) -> Self {
        Self {
            schema_version: v.schema_version,
            form_id: v.form_id,
            schema_revision: v.schema_revision,
            configuration_revision: v.configuration_revision,
            title: Some(v.title.into()),
            fields: v.fields.into_iter().map(Into::into).collect(),
        }
    }
}
impl TryFrom<proto::SettingsForm> for Form {
    type Error = WireError;
    fn try_from(v: proto::SettingsForm) -> Result<Self, WireError> {
        bounded(&v)?;
        let form = Self {
            schema_version: v.schema_version,
            form_id: v.form_id,
            schema_revision: v.schema_revision,
            configuration_revision: v.configuration_revision,
            title: v.title.ok_or(WireError)?.into(),
            fields: v
                .fields
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
        };
        form.validate()?;
        Ok(form)
    }
}
impl From<Request> for proto::SettingsRequest {
    fn from(v: Request) -> Self {
        use proto::settings_request::Action as P;
        Self {
            context: Some(v.context.into()),
            action: Some(match v.action {
                Action::DescribeRead => P::DescribeRead(true),
                Action::Cancel => P::Cancel(true),
                Action::Submit {
                    schema_revision,
                    configuration_revision,
                    values,
                } => P::Submit(proto::SettingsSubmission {
                    schema_revision,
                    configuration_revision,
                    values: values.into_iter().map(|(k, v)| (k, v.into())).collect(),
                }),
            }),
        }
    }
}
impl TryFrom<proto::SettingsRequest> for Request {
    type Error = WireError;
    fn try_from(v: proto::SettingsRequest) -> Result<Self, WireError> {
        use proto::settings_request::Action as P;
        bounded(&v)?;
        let request = Self {
            context: v.context.ok_or(WireError)?.into(),
            action: match v.action.ok_or(WireError)? {
                P::DescribeRead(true) => Action::DescribeRead,
                P::Cancel(true) => Action::Cancel,
                P::DescribeRead(false) | P::Cancel(false) => return Err(WireError),
                P::Submit(v) => Action::Submit {
                    schema_revision: v.schema_revision,
                    configuration_revision: v.configuration_revision,
                    values: v
                        .values
                        .into_iter()
                        .map(|(k, v)| Ok((k, v.try_into()?)))
                        .collect::<Result<_, WireError>>()?,
                },
            },
        };
        request.validate()?;
        Ok(request)
    }
}

impl From<Persistence> for proto::SettingsPersistence {
    fn from(v: Persistence) -> Self {
        match v {
            Persistence::Persisted => Self::Persisted,
            Persistence::SessionOnly => Self::SessionOnly,
        }
    }
}
impl TryFrom<i32> for Persistence {
    type Error = WireError;
    fn try_from(v: i32) -> Result<Self, WireError> {
        match proto::SettingsPersistence::try_from(v).map_err(|_| WireError)? {
            proto::SettingsPersistence::Persisted => Ok(Self::Persisted),
            proto::SettingsPersistence::SessionOnly => Ok(Self::SessionOnly),
            _ => Err(WireError),
        }
    }
}
impl From<Failure> for proto::SettingsFailure {
    fn from(v: Failure) -> Self {
        match v {
            Failure::Denied => Self::Denied,
            Failure::Unavailable => Self::Unavailable,
            Failure::TimedOut => Self::TimedOut,
            Failure::Cancelled => Self::Cancelled,
            Failure::Stale => Self::Stale,
            Failure::Invalid => Self::Invalid,
            Failure::PersistenceFailed => Self::PersistenceFailed,
        }
    }
}
impl TryFrom<i32> for Failure {
    type Error = WireError;
    fn try_from(v: i32) -> Result<Self, WireError> {
        use proto::SettingsFailure as P;
        match P::try_from(v).map_err(|_| WireError)? {
            P::Denied => Ok(Self::Denied),
            P::Unavailable => Ok(Self::Unavailable),
            P::TimedOut => Ok(Self::TimedOut),
            P::Cancelled => Ok(Self::Cancelled),
            P::Stale => Ok(Self::Stale),
            P::Invalid => Ok(Self::Invalid),
            P::PersistenceFailed => Ok(Self::PersistenceFailed),
            _ => Err(WireError),
        }
    }
}
impl From<Reply> for proto::SettingsReply {
    fn from(v: Reply) -> Self {
        use proto::settings_reply::Outcome as P;
        Self {
            context: Some(v.context.into()),
            outcome: Some(match v.outcome {
                Outcome::Form { form, values } => P::Form(proto::SettingsView {
                    form: Some(form.into()),
                    values: values.into_iter().map(|(k, v)| (k, v.into())).collect(),
                }),
                Outcome::Applied {
                    configuration_revision,
                    persistence,
                } => P::Applied(proto::SettingsApplied {
                    configuration_revision,
                    persistence: proto::SettingsPersistence::from(persistence) as i32,
                }),
                Outcome::FieldErrors { errors } => P::FieldErrors(proto::SettingsFieldErrors {
                    errors: errors.into_iter().map(|(k, v)| (k, v.into())).collect(),
                }),
                Outcome::RevisionConflict {
                    configuration_revision,
                } => P::RevisionConflict(configuration_revision),
                Outcome::Failed { failure } => {
                    P::Failed(proto::SettingsFailure::from(failure) as i32)
                }
            }),
        }
    }
}
impl TryFrom<proto::SettingsReply> for Reply {
    type Error = WireError;
    fn try_from(v: proto::SettingsReply) -> Result<Self, WireError> {
        use proto::settings_reply::Outcome as P;
        bounded(&v)?;
        let reply = Self {
            context: v.context.ok_or(WireError)?.into(),
            outcome: match v.outcome.ok_or(WireError)? {
                P::Form(v) => Outcome::Form {
                    form: v.form.ok_or(WireError)?.try_into()?,
                    values: v
                        .values
                        .into_iter()
                        .map(|(k, v)| Ok((k, v.try_into()?)))
                        .collect::<Result<_, WireError>>()?,
                },
                P::Applied(v) => Outcome::Applied {
                    configuration_revision: v.configuration_revision,
                    persistence: v.persistence.try_into()?,
                },
                P::FieldErrors(v) => Outcome::FieldErrors {
                    errors: v.errors.into_iter().map(|(k, v)| (k, v.into())).collect(),
                },
                P::RevisionConflict(configuration_revision) => Outcome::RevisionConflict {
                    configuration_revision,
                },
                P::Failed(failure) => Outcome::Failed {
                    failure: failure.try_into()?,
                },
            },
        };
        reply.validate()?;
        Ok(reply)
    }
}
