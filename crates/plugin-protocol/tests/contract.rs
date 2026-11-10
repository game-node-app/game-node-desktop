use gamenode_plugin_protocol::{models, proto::*};
use prost::Message;

#[test]
fn typed_login_result_distinguishes_close_timeout_cancel_and_failure() {
    for outcome in [
        models::SignInResult::Connected,
        models::SignInResult::Cancelled,
        models::SignInResult::Closed,
        models::SignInResult::TimedOut,
        models::SignInResult::Failed,
        models::SignInResult::Superseded,
    ] {
        let wire = OperationResult {
            disposition: OperationDisposition::Completed as i32,
            login_outcome: Some(AccountLoginOutcome::from(outcome) as i32),
            ..Default::default()
        };
        let decoded = OperationResult::decode(wire.encode_to_vec().as_slice()).unwrap();
        let restored: models::SignInResult =
            AccountLoginOutcome::try_from(decoded.login_outcome.unwrap())
                .unwrap()
                .try_into()
                .unwrap();
        assert_eq!(restored, outcome);
    }
    assert!(models::SignInResult::try_from(AccountLoginOutcome::Unspecified).is_err());
    assert!(AccountLoginOutcome::try_from(99).is_err());
}

#[test]
fn operation_contract_distinguishes_empty_failure_interruption_and_stale_results() {
    let empty_discovery = OperationResult {
        disposition: OperationDisposition::Completed as i32,
        discovery: Some(DiscoveryResult {
            provider_id: "steam".into(),
            records: vec![],
        }),
        ..Default::default()
    };
    let failed_discovery = OperationResult {
        disposition: OperationDisposition::Failed as i32,
        failure: Some(PluginFailure {
            code: "discovery_unavailable".into(),
            message: "scanner could not read the library".into(),
        }),
        ..Default::default()
    };
    let interrupted = OperationResult {
        disposition: OperationDisposition::Cancelled as i32,
        ..Default::default()
    };
    let stale = OperationResult {
        disposition: OperationDisposition::Superseded as i32,
        ..Default::default()
    };

    let decode = |result: OperationResult| {
        OperationResult::decode(result.encode_to_vec().as_slice()).unwrap()
    };
    let empty = decode(empty_discovery);
    assert!(empty.discovery.unwrap().records.is_empty());
    let failed = decode(failed_discovery);
    assert_eq!(failed.disposition, OperationDisposition::Failed as i32);
    assert_eq!(failed.failure.unwrap().code, "discovery_unavailable");
    assert_eq!(
        decode(interrupted).disposition,
        OperationDisposition::Cancelled as i32
    );
    assert_eq!(
        decode(stale).disposition,
        OperationDisposition::Superseded as i32
    );
    assert!(ProviderOperation::try_from(999).is_err());
}

#[test]
fn login_policy_scopes_https_hosts_cookies_and_probe_origin() {
    use gamenode_plugin_protocol::login::LoginPolicy;
    let policy = LoginPolicy {
        start_url: "https://store.example.test/login".into(),
        cookie_urls: vec!["https://store.example.test/".into()],
        probe_origin: "https://store.example.test".into(),
        title: "Example".into(),
        allowed_hosts: vec!["example.test".into()],
        cookie_names: vec!["session".into()],
        probe_script: "null".into(),
        width: 600,
        height: 760,
        timeout_seconds: 600,
    };
    policy.validate().unwrap();
    for allowed in ["https://example.test/", "https://store.example.test/login"] {
        assert!(policy.allows(&url::Url::parse(allowed).unwrap()));
    }
    for denied in [
        "http://store.example.test/",
        "https://notexample.test/",
        "https://example.test.evil.test/",
        "https://user:password@store.example.test/",
    ] {
        assert!(!policy.allows(&url::Url::parse(denied).unwrap()));
    }
    assert!(
        LoginPolicy {
            cookie_urls: vec![],
            cookie_names: vec![],
            ..policy.clone()
        }
        .validate()
        .is_ok()
    );
    assert!(
        LoginPolicy {
            cookie_urls: vec![],
            ..policy.clone()
        }
        .validate()
        .is_err()
    );
    assert!(
        LoginPolicy {
            cookie_names: vec![],
            ..policy.clone()
        }
        .validate()
        .is_err()
    );
    assert!(
        LoginPolicy {
            probe_origin: "https://store.example.test/path".into(),
            ..policy.clone()
        }
        .validate()
        .is_err()
    );
    assert!(
        LoginPolicy {
            cookie_urls: vec!["https://other.test/".into()],
            ..policy.clone()
        }
        .validate()
        .is_err()
    );
    assert!(
        LoginPolicy {
            timeout_seconds: 601,
            ..policy
        }
        .validate()
        .is_err()
    );
}

#[test]
fn negotiation_preserves_discovery_clients_without_granting_account_access() {
    use gamenode_plugin_protocol::{API_MAJOR, capability::*};
    assert_eq!(
        negotiate(API_MAJOR, 1, &[DISCOVERY.into()], &[DISCOVERY]),
        Ok(1)
    );
    assert_eq!(
        negotiate(API_MAJOR, 99, &[CATALOG.into()], &[CATALOG]),
        Ok(gamenode_plugin_protocol::API_MINOR)
    );
    assert_eq!(
        negotiate(API_MAJOR, 1, &[CATALOG.into()], &[CATALOG]),
        Err(NegotiationError::Unsupported)
    );
    assert_eq!(
        negotiate(API_MAJOR, 2, &[SECRET_READ.into()], &[DISCOVERY]),
        Err(NegotiationError::Capability)
    );
    assert_eq!(
        negotiate(API_MAJOR + 1, 2, &[DISCOVERY.into()], &[DISCOVERY]),
        Err(NegotiationError::Version)
    );
    assert_eq!(
        negotiate(
            API_MAJOR,
            2,
            &[DISCOVERY.into(), DISCOVERY.into()],
            &[DISCOVERY]
        ),
        Err(NegotiationError::Capability)
    );
}

#[test]
fn achievements_require_the_new_api_minor() {
    use gamenode_plugin_protocol::{API_MAJOR, API_MINOR, capability::*};
    assert_eq!(
        negotiate(API_MAJOR, 3, &[ACHIEVEMENTS.into()], &[ACHIEVEMENTS]),
        Err(NegotiationError::Unsupported)
    );
    assert_eq!(
        negotiate(
            API_MAJOR,
            ACHIEVEMENTS_API_MINOR,
            &[ACHIEVEMENTS.into()],
            &[ACHIEVEMENTS]
        ),
        Ok(ACHIEVEMENTS_API_MINOR)
    );
    // Later additive minors keep achievements negotiable, and an older minor
    // still selects the version it declared.
    const { assert!(API_MINOR > ACHIEVEMENTS_API_MINOR) };
    assert_eq!(
        negotiate(API_MAJOR, 2, &[CATALOG.into()], &[CATALOG]),
        Ok(2)
    );
}

#[test]
fn achievement_outcomes_reject_unrequested_ids_and_oversized_batches() {
    use gamenode_plugin_protocol::achievements::{MAX_GAMES, parse_outcomes};
    let requested: std::collections::BTreeSet<String> = ["10".to_owned()].into();
    let outcome = AchievementOutcome {
        game_id: "11".into(),
        outcome: Some(achievement_outcome::Outcome::None(true)),
    };
    assert!(parse_outcomes(&[outcome], &requested, false).is_err());
    let oversized = (0..=MAX_GAMES)
        .map(|index| AchievementOutcome {
            game_id: index.to_string(),
            outcome: Some(achievement_outcome::Outcome::None(true)),
        })
        .collect::<Vec<_>>();
    assert!(parse_outcomes(&oversized, &requested, false).is_err());
}

#[test]
fn counter_last_played_round_trips_absent_and_present_values() {
    let absent = CounterObservation {
        id: "620".into(),
        seconds: 3_600,
        last_played: None,
    };
    let present = CounterObservation {
        id: "620".into(),
        seconds: 3_600,
        last_played: Some(1_700_000_000),
    };
    let decode = |observation: CounterObservation| -> CounterObservation {
        CounterObservation::decode(observation.encode_to_vec().as_slice()).unwrap()
    };
    assert_eq!(decode(absent.clone()), absent);
    assert_eq!(decode(absent).last_played, None);
    assert_eq!(decode(present).last_played, Some(1_700_000_000));
}

#[test]
fn catalog_games_carry_bounded_fields_and_reject_invalid_ones() {
    use gamenode_plugin_protocol::models::{
        CatalogRecord, MAX_CATALOG_FIELD_KEY, MAX_CATALOG_FIELD_VALUE, MAX_CATALOG_FIELDS,
    };
    let record = CatalogRecord::new("app", "Game")
        .with_field("namespace", "0584d2013f0149a791e7b9bad0eec102")
        .unwrap();
    assert_eq!(
        record.fields["namespace"],
        "0584d2013f0149a791e7b9bad0eec102"
    );
    let wire = CatalogGame::from(record.clone());
    assert_eq!(CatalogRecord::try_from(wire).unwrap(), record);
    let empty = CatalogGame {
        id: "app".into(),
        name: "Game".into(),
        fields: Default::default(),
    };
    let decoded = CatalogGame::decode(empty.encode_to_vec().as_slice()).unwrap();
    assert!(decoded.fields.is_empty());

    let valid = CatalogRecord::new("app", "Game");
    let mut full = valid.clone();
    for index in 0..MAX_CATALOG_FIELDS {
        full = full.with_field(format!("k{index}"), "v").unwrap();
    }
    assert!(
        full.clone().with_field("one-too-many", "v").is_err(),
        "the field count is bounded"
    );
    assert!(
        full.with_field("k0", "replacement").is_ok(),
        "replacing a key does not grow the map"
    );
    assert!(valid.clone().with_field("", "v").is_err());
    assert!(
        valid
            .clone()
            .with_field("k".repeat(MAX_CATALOG_FIELD_KEY + 1), "v")
            .is_err()
    );
    assert!(
        valid
            .clone()
            .with_field("k", "v".repeat(MAX_CATALOG_FIELD_VALUE + 1))
            .is_err()
    );
    assert!(valid.clone().with_field("k\n", "v").is_err());
    assert!(valid.clone().with_field("k", "v\u{7}").is_err());
    assert!(
        valid
            .clone()
            .with_field(
                "k".repeat(MAX_CATALOG_FIELD_KEY),
                "v".repeat(MAX_CATALOG_FIELD_VALUE)
            )
            .is_ok()
    );

    // The host also rejects fields that bypassed the builder.
    let mut smuggled = valid.clone();
    smuggled.fields.insert("k\n".into(), "v".into());
    assert!(CatalogRecord::try_from(CatalogGame::from(smuggled)).is_err());
    let mut oversized = valid;
    for index in 0..=MAX_CATALOG_FIELDS {
        oversized.fields.insert(format!("k{index}"), "v".into());
    }
    assert!(oversized.validate_fields().is_err());
}

#[test]
fn invalid_credential_keeps_wire_number_seven_and_reads_legacy_rows() {
    assert_eq!(CatalogFailure::InvalidCredential as i32, 7);
    assert_eq!(
        CatalogFailure::try_from(7).unwrap(),
        CatalogFailure::InvalidCredential
    );
    assert_eq!(
        models::CatalogError::try_from(CatalogFailure::InvalidCredential).unwrap(),
        models::CatalogError::InvalidCredential
    );
    assert_eq!(
        CatalogFailure::from(models::CatalogError::InvalidCredential) as i32,
        7
    );
    assert_eq!(
        models::CatalogError::InvalidCredential.as_str(),
        "invalidCredential"
    );
    // Rows written before the provider-neutral rename still load.
    assert_eq!(
        models::CatalogError::from_category("invalidCookies"),
        Some(models::CatalogError::InvalidCredential)
    );
}

#[test]
fn catalog_wire_distinguishes_empty_failure_and_unchanged() {
    let outcomes = [
        catalog_completion::Outcome::CompletedRecords(0),
        catalog_completion::Outcome::Failed(CatalogFailure::FamilyUnavailable as i32),
        catalog_completion::Outcome::Unchanged(true),
    ];
    for outcome in outcomes {
        let completion = CatalogCompletion {
            category: CatalogCategory::Family as i32,
            outcome: Some(outcome),
        };
        assert_eq!(
            CatalogCompletion::decode(completion.encode_to_vec().as_slice()).unwrap(),
            completion,
        );
    }
}

fn chunk(category: CatalogCategory, index: u32) -> CatalogChunk {
    CatalogChunk {
        category: category as i32,
        index,
        games: vec![CatalogGame {
            id: "620".into(),
            name: "Portal 2".into(),
            fields: Default::default(),
        }],
    }
}

#[test]
fn catalog_commits_only_complete_categories_and_isolates_family_failure() {
    use gamenode_plugin_protocol::catalog::CatalogTransfer;
    let mut transfer = CatalogTransfer::default();
    transfer.push(chunk(CatalogCategory::Owned, 0)).unwrap();
    let update = transfer
        .finish(
            vec![
                CatalogCompletion {
                    category: CatalogCategory::Owned as i32,
                    outcome: Some(catalog_completion::Outcome::CompletedRecords(1)),
                },
                CatalogCompletion {
                    category: CatalogCategory::Family as i32,
                    outcome: Some(catalog_completion::Outcome::Failed(
                        CatalogFailure::FamilyUnavailable as i32,
                    )),
                },
            ],
            100,
        )
        .unwrap();
    assert_eq!(update.owned.unwrap().unwrap()[0].id, "620");
    assert_eq!(
        update.family,
        Some(Err(models::CatalogError::FamilyUnavailable))
    );
    assert_eq!(
        CatalogTransfer::default()
            .finish(
                vec![CatalogCompletion {
                    category: CatalogCategory::Owned as i32,
                    outcome: Some(catalog_completion::Outcome::CompletedRecords(0))
                }],
                100
            )
            .unwrap()
            .owned,
        Some(Ok(vec![]))
    );
}

#[test]
fn partial_duplicate_out_of_order_and_oversized_catalogs_are_rejected() {
    use gamenode_plugin_protocol::catalog::{CHUNK_RECORDS, CatalogTransfer};
    let mut partial = CatalogTransfer::default();
    partial.push(chunk(CatalogCategory::Owned, 0)).unwrap();
    assert!(partial.finish(vec![], 100).is_err());
    let mut transfer = CatalogTransfer::default();
    assert!(transfer.push(chunk(CatalogCategory::Owned, 1)).is_err());
    transfer.push(chunk(CatalogCategory::Owned, 0)).unwrap();
    assert!(transfer.push(chunk(CatalogCategory::Owned, 0)).is_err());
    assert!(transfer.push(chunk(CatalogCategory::Owned, 1)).is_err());
    let mut oversized = chunk(CatalogCategory::Family, 0);
    oversized.games = vec![
        CatalogGame {
            id: "1".into(),
            name: "Game".into(),
            fields: Default::default(),
        };
        CHUNK_RECORDS + 1
    ];
    assert!(transfer.push(oversized).is_err());
    assert!(
        transfer
            .finish(
                vec![CatalogCompletion {
                    category: CatalogCategory::Owned as i32,
                    outcome: Some(catalog_completion::Outcome::CompletedRecords(2))
                }],
                100
            )
            .is_err()
    );
}

#[test]
fn commands_preserve_operation_and_account_profile_fences() {
    let command = ProviderCommand {
        context: Some(ServiceContext {
            session_id: "launch-1".into(),
            session_credential: vec![42; 32],
            provider_id: "example".into(),
            operation_id: "login-7".into(),
            account_epoch: 3,
            profile_epoch: 9,
        }),
        operation: ProviderOperation::CancelSignIn as i32,
        target_operation_id: "login-6".into(),
        settings: None,
        achievements: None,
    };
    assert_eq!(
        ProviderCommand::decode(command.encode_to_vec().as_slice()).unwrap(),
        command,
    );
}

#[test]
fn secret_bearing_wire_messages_are_redacted_including_nested_outcomes() {
    let secret = "secret-provider-session-123";
    let request = SecretRequest {
        key: "session".into(),
        operation: SecretOperation::Write as i32,
        value: Some(secret.as_bytes().to_vec()),
        ..Default::default()
    };
    let reply = SecretReply {
        storage: SecretStorage::SecretService as i32,
        outcome: Some(secret_reply::Outcome::Value(secret.as_bytes().to_vec())),
    };
    let login = LoginReply {
        handle: "window-1".into(),
        state: LoginState::Open as i32,
        cookies: vec![LoginCookie {
            name: "session".into(),
            value: secret.into(),
        }],
        page_result_json: Some(format!(r#"{{"token":"{secret}"}}"#)),
    };
    for debug in [
        format!("{request:?}"),
        format!("{reply:?}"),
        format!("{:?}", reply.outcome),
        format!("{login:?}"),
        format!("{:?}", login.cookies),
    ] {
        assert!(debug.contains("redacted"));
        assert!(!debug.contains(secret));
        assert!(!debug.contains("115, 101, 99, 114, 101, 116"));
    }
    let opaque = models::SessionCredential::new(secret).unwrap();
    assert!(!format!("{opaque:?}").contains(secret));
}
