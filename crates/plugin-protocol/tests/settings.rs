use gamenode_plugin_protocol::{WireError, capability as cap, proto, settings::*};
use prost::Message;
use std::collections::BTreeMap;

fn context() -> Context {
    Context {
        plugin_id: "fixture.settings".into(),
        provider_id: "fixture".into(),
        session_id: "session-1".into(),
        profile_epoch: 7,
        form_handle: "form-42".into(),
    }
}

fn form() -> Form {
    let field = |id: &str, kind, default| Field {
        id: id.into(),
        label: Text::plain(id),
        help: None,
        kind,
        default,
    };
    Form {
        schema_version: SCHEMA_VERSION,
        form_id: "preferences".into(),
        schema_revision: "schema-1".into(),
        configuration_revision: "revision-3".into(),
        title: Text {
            fallback: "Preferences".into(),
            translations: BTreeMap::from([("pt-BR".into(), "Preferências".into())]),
        },
        fields: vec![
            field("enabled", Kind::Boolean, Value::Boolean(true)),
            field(
                "prefix",
                Kind::Text {
                    min_length: 1,
                    max_length: 12,
                },
                Value::Text("Games".into()),
            ),
            field(
                "interval",
                Kind::Number {
                    min: 5.0,
                    max: 60.0,
                    step: 5.0,
                },
                Value::Number(10.0),
            ),
            field(
                "channel",
                Kind::Choice {
                    options: vec![
                        Choice {
                            id: "stable".into(),
                            label: Text::plain("Stable"),
                        },
                        Choice {
                            id: "preview".into(),
                            label: Text::plain("Preview"),
                        },
                    ],
                },
                Value::Choice("stable".into()),
            ),
        ],
    }
}

fn values(form: &Form) -> Values {
    form.fields
        .iter()
        .map(|field| (field.id.clone(), field.default.clone()))
        .collect()
}

#[test]
fn optional_settings_negotiation_does_not_expand_existing_grants() {
    assert!(!cap::ALL.contains(&cap::SETTINGS));
    assert_eq!(cap::ALL.len(), 13);
    assert_eq!(
        cap::negotiate(1, 3, &[cap::SETTINGS.into()], &[cap::SETTINGS]),
        Ok(3)
    );
    assert_eq!(
        cap::negotiate(1, 2, &[cap::SETTINGS.into()], &[cap::SETTINGS]),
        Err(cap::NegotiationError::Unsupported)
    );
    assert_eq!(
        cap::negotiate(1, 3, &[cap::SETTINGS.into()], cap::ALL),
        Err(cap::NegotiationError::Capability)
    );
    assert_eq!(
        cap::negotiate(1, 3, &["ui.script".into()], &["ui.script"]),
        Err(cap::NegotiationError::Capability)
    );
    assert_eq!(
        cap::negotiate(1, 1, &[cap::DISCOVERY.into()], &[cap::DISCOVERY]),
        Ok(1)
    );
    assert_eq!(
        cap::negotiate(1, 2, &[cap::CONTEXT.into()], cap::ALL),
        Ok(2)
    );
}

#[test]
fn settings_roundtrip_preserves_localization_and_all_four_types() {
    let form = form();
    form.validate_values(&values(&form)).unwrap();
    let wire: proto::SettingsForm = form.clone().into();
    assert_eq!(
        Form::try_from(proto::SettingsForm::decode(wire.encode_to_vec().as_slice()).unwrap())
            .unwrap(),
        form
    );
    let reply = Reply {
        context: context(),
        outcome: Outcome::Form {
            values: values(&form),
            form,
        },
    };
    let request = Request {
        context: context(),
        action: Action::DescribeRead,
    };
    reply.validate_for(&request).unwrap();
    let wire: proto::SettingsReply = reply.clone().into();
    assert_eq!(
        Reply::try_from(proto::SettingsReply::decode(wire.encode_to_vec().as_slice()).unwrap())
            .unwrap(),
        reply
    );
}

#[test]
fn typed_submit_outcomes_require_real_acknowledgement_and_exact_scope() {
    let request = Request {
        context: context(),
        action: Action::Submit {
            schema_revision: "schema-1".into(),
            configuration_revision: "revision-3".into(),
            values: values(&form()),
        },
    };
    let wire: proto::SettingsRequest = request.clone().into();
    assert_eq!(
        Request::try_from(proto::SettingsRequest::decode(wire.encode_to_vec().as_slice()).unwrap())
            .unwrap(),
        request
    );
    let outcomes = [
        Outcome::Applied {
            configuration_revision: "revision-4".into(),
            persistence: Persistence::Persisted,
        },
        Outcome::Applied {
            configuration_revision: "revision-4".into(),
            persistence: Persistence::SessionOnly,
        },
        Outcome::FieldErrors {
            errors: BTreeMap::from([("prefix".into(), Text::plain("Choose another prefix"))]),
        },
        Outcome::RevisionConflict {
            configuration_revision: "revision-5".into(),
        },
        Outcome::Failed {
            failure: Failure::PersistenceFailed,
        },
        Outcome::Failed {
            failure: Failure::Cancelled,
        },
        Outcome::Failed {
            failure: Failure::TimedOut,
        },
        Outcome::Failed {
            failure: Failure::Unavailable,
        },
    ];
    for outcome in outcomes {
        let reply = Reply {
            context: context(),
            outcome,
        };
        reply.validate_for(&request).unwrap();
        let wire: proto::SettingsReply = reply.clone().into();
        assert_eq!(Reply::try_from(wire).unwrap(), reply);
        let mut stale = reply.clone();
        stale.context.profile_epoch += 1;
        assert_eq!(stale.validate_for(&request), Err(WireError));
        stale.context = reply.context.clone();
        stale.context.session_id = "replacement-session".into();
        assert_eq!(stale.validate_for(&request), Err(WireError));
        stale.context = reply.context.clone();
        stale.context.plugin_id = "other.plugin".into();
        assert_eq!(stale.validate_for(&request), Err(WireError));
        stale.context = reply.context.clone();
        stale.context.form_handle = "newer-surface".into();
        assert_eq!(stale.validate_for(&request), Err(WireError));
    }
    let service = proto::ServiceContext {
        provider_id: "fixture".into(),
        session_id: "session-1".into(),
        profile_epoch: 7,
        ..Default::default()
    };
    assert!(request.matches_service(&service, "fixture.settings"));
    assert!(!request.matches_service(&service, "other.plugin"));
    assert!(!request.matches_service(
        &proto::ServiceContext {
            provider_id: "other".into(),
            ..service
        },
        "fixture.settings"
    ));
}

#[test]
fn schema_rejects_duplicate_ids_types_options_and_invalid_constraints() {
    let mut f = form();
    f.fields.push(f.fields[0].clone());
    assert!(f.validate().is_err());
    let mut f = form();
    f.fields[0].default = Value::Number(1.0);
    assert!(f.validate().is_err());
    let mut f = form();
    f.fields[1].kind = Kind::Text {
        min_length: 20,
        max_length: 10,
    };
    assert!(f.validate().is_err());
    let mut f = form();
    f.fields[1].kind = Kind::Text {
        min_length: 0,
        max_length: MAX_TEXT_LENGTH + 1,
    };
    assert!(f.validate().is_err());
    for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        let mut f = form();
        f.fields[2].kind = Kind::Number {
            min: 0.0,
            max: 60.0,
            step: bad,
        };
        assert!(f.validate().is_err());
    }
    let mut f = form();
    f.fields[2].default = Value::Number(11.0);
    assert!(f.validate().is_err());
    let mut f = form();
    f.fields[3].kind = Kind::Choice { options: vec![] };
    assert!(f.validate().is_err());
    let mut f = form();
    f.fields[3].kind = Kind::Choice {
        options: vec![
            Choice {
                id: "same".into(),
                label: Text::plain("One")
            };
            2
        ],
    };
    assert!(f.validate().is_err());
    let mut f = form();
    f.fields[3].default = Value::Choice("unknown".into());
    assert!(f.validate().is_err());
    let mut wire: proto::SettingsForm = form().into();
    wire.fields[0].kind = None;
    assert!(Form::try_from(wire).is_err());
    let mut wire: proto::SettingsForm = form().into();
    wire.fields[0].default_value = Some(proto::SettingsValue { value: None });
    assert!(Form::try_from(wire).is_err());
}

#[test]
fn submitted_values_cannot_add_omit_or_change_field_types() {
    let f = form();
    let mut v = values(&f);
    v.remove("enabled");
    assert!(f.validate_values(&v).is_err());
    let mut v = values(&f);
    v.insert("arbitrary_command".into(), Value::Text("run".into()));
    assert!(f.validate_values(&v).is_err());
    let mut v = values(&f);
    v.insert("enabled".into(), Value::Text("true".into()));
    assert!(f.validate_values(&v).is_err());
    let mut v = values(&f);
    v.insert("prefix".into(), Value::Text("line\nbreak".into()));
    assert!(f.validate_values(&v).is_err());
    let mut v = values(&f);
    v.insert("prefix".into(), Value::Text("😀".repeat(12)));
    f.validate_values(&v).unwrap();
    let mut v = values(&f);
    v.insert("interval".into(), Value::Number(65.0));
    assert!(f.validate_values(&v).is_err());
    let mut v = values(&f);
    v.insert("channel".into(), Value::Choice("injected".into()));
    assert!(f.validate_values(&v).is_err());
}

#[test]
fn executable_presentation_and_credential_fields_are_rejected() {
    for label in [
        "<script>alert(1)</script>",
        "javascript:open()",
        "body { color: red }",
        "x => call()",
        "@import 'x'",
        "url(https://evil.test)",
    ] {
        let mut f = form();
        f.fields[0].label = Text::plain(label);
        assert!(f.validate().is_err(), "{label}");
    }
    for name in [
        "password",
        "session_cookie",
        "secrets.value",
        "access_token",
        "credentials",
        "api-key.token",
    ] {
        let mut f = form();
        f.fields[0].id = name.into();
        assert!(f.validate().is_err());
    }
    let mut value = serde_json::to_value(form()).unwrap();
    value["fields"][0]["on_change"] = serde_json::json!("invoke('arbitrary')");
    assert!(serde_json::from_value::<Form>(value).is_err());
    let mut value = serde_json::to_value(form()).unwrap();
    value["fields"][0]["kind"] = serde_json::json!({"type":"password"});
    assert!(serde_json::from_value::<Form>(value).is_err());
    let mut value = serde_json::to_value(form()).unwrap();
    value["html"] = serde_json::json!("<iframe>");
    assert!(serde_json::from_value::<Form>(value).is_err());
    let value = Value::Text("never-log-submitted-input".into());
    assert!(!format!("{value:?}").contains("never-log"));
    let wire: proto::SettingsValue = value.into();
    assert!(!format!("{wire:?}").contains("never-log"));
}

#[test]
fn per_field_and_aggregate_payload_limits_are_enforced() {
    let mut f = form();
    f.fields = (0..=MAX_FIELDS)
        .map(|n| Field {
            id: format!("field-{n}"),
            ..f.fields[0].clone()
        })
        .collect();
    assert!(f.validate().is_err());
    let mut f = form();
    f.title = Text::plain("x".repeat(257));
    assert!(f.validate().is_err());
    let mut f = form();
    f.fields[0].label.translations = (0..=MAX_TRANSLATIONS)
        .map(|n| (format!("en-{n}"), "Label".into()))
        .collect();
    assert!(f.validate().is_err());
    let mut f = form();
    f.fields = (0..MAX_FIELDS)
        .map(|n| Field {
            id: format!("field-{n}"),
            help: Some(Text {
                fallback: "x".repeat(1024),
                translations: (0..MAX_TRANSLATIONS)
                    .map(|n| (format!("en-{n}"), "y".repeat(1024)))
                    .collect(),
            }),
            ..f.fields[0].clone()
        })
        .collect();
    assert!(proto::SettingsForm::from(f.clone()).encoded_len() > MAX_PAYLOAD_BYTES);
    assert!(f.validate().is_err());
    assert!(Form::try_from(proto::SettingsForm::from(f)).is_err());
    let mut reply: proto::SettingsReply = Reply {
        context: context(),
        outcome: Outcome::Applied {
            configuration_revision: "revision-4".into(),
            persistence: Persistence::Persisted,
        },
    }
    .into();
    if let Some(proto::settings_reply::Outcome::Applied(ref mut applied)) = reply.outcome {
        applied.persistence = 99;
    }
    assert!(Reply::try_from(reply).is_err());
}
