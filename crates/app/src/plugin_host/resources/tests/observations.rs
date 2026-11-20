//! Public Computer and GamePresence RPC flows over the local authenticated host services.

use super::*;
use gamenode_plugin_protocol::prost_types::{Struct, Value, value::Kind};
use std::collections::BTreeMap;
use tonic::Code;

const HARDWARE_GRANTS: &[&str] = &[cap::COMPUTER_HARDWARE_PUBLISH, cap::COMPUTER_HARDWARE_READ];
const PRESENCE_GRANTS: &[&str] = &[cap::GAME_PRESENCE_PUBLISH, cap::GAME_PRESENCE_READ];

fn value_string(text: &str) -> Value {
    Value {
        kind: Some(Kind::StringValue(text.into())),
    }
}

fn value_number(number: f64) -> Value {
    Value {
        kind: Some(Kind::NumberValue(number)),
    }
}

fn object(fields: impl IntoIterator<Item = (String, Value)>) -> Value {
    Value {
        kind: Some(Kind::StructValue(Struct {
            fields: fields.into_iter().collect(),
        })),
    }
}

#[tokio::test]
async fn hardware_report_uses_host_context_preserves_unknown_states_and_rejects_unique_ids() {
    let fixture = Fixture::new(HARDWARE_GRANTS).await;
    let observed_at = crate::plugin_host::resources::unix_now().unwrap();
    let published = fixture
        .client()
        .computer_publish_hardware(HardwareReportRequest {
            mutation: Some(fixture.mutation("hardware-1", 0)),
            computer_id: String::new(),
            schema_id: "org.example.hardware.v2".into(),
            schema_version: 2,
            observations: Some(Struct {
                fields: BTreeMap::from([
                    (
                        "cpu".into(),
                        object([
                            ("logical_cores".into(), value_number(8.0)),
                            ("temperature_state".into(), value_string("unknown")),
                        ]),
                    ),
                    ("os_version".into(), value_string("FixtureOS 1")),
                ]),
            }),
            observed_at,
        })
        .await
        .unwrap()
        .into_inner();
    assert!(!published.report_id.is_empty());

    let mut computer = fixture.host.computer_client();
    let listed = computer
        .list(ComputerListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            computer_context_id: "active-host".into(),
            projection_id: "hardware.v1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.reports.len(), 1);
    let report = &listed.reports[0];
    assert_eq!(report.record_id, published.report_id);
    assert_eq!(report.computer_context_id, "active-host");
    assert_eq!(report.owner_plugin_id, "fixture.plugin");
    assert_eq!(report.schema_id, "org.example.hardware.v2");
    assert_eq!(report.schema_version, 2);
    let fields = &report.typed_observations.as_ref().unwrap().fields;
    let cpu = fields["cpu"].kind.as_ref().unwrap();
    let Kind::StructValue(cpu) = cpu else {
        panic!("CPU observations should preserve their typed structure");
    };
    assert_eq!(
        cpu.fields["logical_cores"].kind,
        Some(Kind::NumberValue(8.0))
    );
    assert_eq!(
        cpu.fields["temperature_state"].kind,
        Some(Kind::StringValue("unknown".into()))
    );
    assert!(!fields.contains_key("serial_number"));

    let fetched = computer
        .get(ComputerGetRequest {
            context: Some(fixture.context()),
            computer_record_id: published.report_id,
            projection_id: "hardware.v1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(fetched.report.unwrap().schema_version, 2);

    let mut watch = computer
        .watch(ComputerWatchRequest {
            context: Some(fixture.context()),
            computer_context_id: "active-host".into(),
            projection_id: "hardware.v1".into(),
            after_revision: listed.revision,
        })
        .await
        .unwrap()
        .into_inner();
    fixture
        .client()
        .computer_publish_hardware(HardwareReportRequest {
            mutation: Some(fixture.mutation("hardware-2", 0)),
            schema_id: "org.example.hardware.v2".into(),
            schema_version: 2,
            observations: Some(Struct {
                fields: BTreeMap::from([("gpu_state".into(), value_string("unknown"))]),
            }),
            observed_at,
            ..Default::default()
        })
        .await
        .unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, "published");
    assert!(event.revision > listed.revision);

    let denied = fixture
        .client()
        .computer_publish_hardware(HardwareReportRequest {
            mutation: Some(fixture.mutation("hardware-private-id", 0)),
            schema_id: "org.example.hardware.v2".into(),
            schema_version: 2,
            observations: Some(Struct {
                fields: BTreeMap::from([("serial_number".into(), value_string("secret"))]),
            }),
            observed_at,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn hardware_context_id_is_host_assigned_and_permissions_are_separate() {
    let fixture = Fixture::new(&[cap::COMPUTER_HARDWARE_PUBLISH]).await;
    let denied = fixture
        .client()
        .computer_publish_hardware(HardwareReportRequest {
            mutation: Some(fixture.mutation("hardware-id", 0)),
            computer_id: "machine-serial-123".into(),
            schema_id: "org.example.hardware.v1".into(),
            schema_version: 1,
            observations: Some(Struct {
                fields: BTreeMap::from([("cpu_name".into(), value_string("Fixture CPU"))]),
            }),
            observed_at: crate::plugin_host::resources::unix_now().unwrap(),
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::InvalidArgument);

    let mut computer = fixture.host.computer_client();
    let denied = computer
        .list(ComputerListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            computer_context_id: String::new(),
            projection_id: "hardware.v1".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
}

#[tokio::test]
async fn presence_replaces_stale_state_and_clear_reaches_authorized_watchers() {
    let fixture = Fixture::new(PRESENCE_GRANTS).await;
    let mut presence = fixture.host.game_presence_client();
    let list_request = || PresenceListRequest {
        page: Some(PageRequest {
            context: Some(fixture.context()),
            cursor: String::new(),
            page_size: 10,
            common_data_categories: None,
        }),
        game_id: "42".into(),
        session_id: "session-a".into(),
        projection_id: "presence.v1".into(),
    };
    // Merely opening a session or selecting a game must not publish online state.
    assert!(
        presence
            .list(list_request())
            .await
            .unwrap()
            .into_inner()
            .records
            .is_empty()
    );
    let publish = |key: &str, mode: &str| PresencePublishRequest {
        mutation: Some(fixture.mutation(key, 0)),
        game_id: "42".into(),
        session_id: "session-a".into(),
        details: Some(Struct {
            fields: BTreeMap::from([("mode".into(), value_string(mode))]),
        }),
        ttl_seconds: 60,
    };
    let first = fixture
        .client()
        .game_presence_publish(publish("presence-1", "ranked"))
        .await
        .unwrap()
        .into_inner();
    let listed = presence.list(list_request()).await.unwrap().into_inner();
    assert_eq!(listed.records.len(), 1);
    assert_eq!(listed.records[0].presence_id, first.report_id);
    assert_eq!(listed.records[0].owner_plugin_id, "fixture.plugin");
    assert_eq!(listed.records[0].game_id, "42");
    assert_eq!(listed.records[0].session_id, "session-a");
    assert_eq!(
        listed.records[0].approved_details.as_ref().unwrap().fields["mode"].kind,
        Some(Kind::StringValue("ranked".into()))
    );

    let mut watch = presence
        .watch(PresenceWatchRequest {
            context: Some(fixture.context()),
            game_id: "42".into(),
            session_id: "session-a".into(),
            projection_id: "presence.v1".into(),
            after_sequence: listed.revision,
        })
        .await
        .unwrap()
        .into_inner();
    let second = fixture
        .client()
        .game_presence_publish(publish("presence-2", "menu"))
        .await
        .unwrap()
        .into_inner();
    assert_ne!(first.report_id, second.report_id);
    let update = tokio::time::timeout(std::time::Duration::from_secs(2), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(update.event_type, "published");
    assert_eq!(update.record.unwrap().presence_id, second.report_id);

    let listed = presence.list(list_request()).await.unwrap().into_inner();
    assert_eq!(listed.records.len(), 1);
    assert_eq!(listed.records[0].presence_id, second.report_id);
    assert_eq!(
        listed.records[0].approved_details.as_ref().unwrap().fields["mode"].kind,
        Some(Kind::StringValue("menu".into()))
    );
    let mut watch = presence
        .watch(PresenceWatchRequest {
            context: Some(fixture.context()),
            game_id: "42".into(),
            session_id: "session-a".into(),
            projection_id: "presence.v1".into(),
            after_sequence: listed.revision,
        })
        .await
        .unwrap()
        .into_inner();
    fixture
        .client()
        .game_presence_clear(PresenceClearRequest {
            mutation: Some(fixture.mutation("presence-clear", 0)),
            game_id: "42".into(),
            session_id: "session-a".into(),
        })
        .await
        .unwrap();
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, "cleared");
    assert_eq!(event.record.unwrap().session_id, "session-a");

    assert!(
        presence
            .list(list_request())
            .await
            .unwrap()
            .into_inner()
            .records
            .is_empty()
    );
    let missing = presence
        .get(PresenceGetRequest {
            context: Some(fixture.context()),
            presence_id: second.report_id,
            projection_id: "presence.v1".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);
}

#[tokio::test]
async fn presence_expires_from_public_query_after_the_host_ttl() {
    let fixture = Fixture::new(PRESENCE_GRANTS).await;
    fixture
        .client()
        .game_presence_publish(PresencePublishRequest {
            mutation: Some(fixture.mutation("presence-expiry", 0)),
            game_id: "42".into(),
            session_id: "session-expiring".into(),
            details: Some(Struct {
                fields: BTreeMap::from([("state".into(), value_string("playing"))]),
            }),
            ttl_seconds: 1,
        })
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    let mut presence = fixture.host.game_presence_client();
    let listed = presence
        .list(PresenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            game_id: "42".into(),
            session_id: "session-expiring".into(),
            projection_id: "presence.v1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(listed.records.is_empty());
}

#[tokio::test]
async fn computer_reads_do_not_include_another_plugins_sensitive_observations_without_policy() {
    use crate::plugin_host::{
        grants::PluginIdentity, shared_data::Publication, sharing_policy::SharingClass,
    };

    let fixture = Fixture::new(&[cap::COMPUTER_HARDWARE_READ]).await;
    fixture
        .data_broker
        .publish(
            Publication {
                owner: PluginIdentity {
                    plugin_id: "source.plugin".into(),
                    publisher_fingerprint: "cd".repeat(32),
                    profile_id: "a".into(),
                },
                dataset_id: "computer.hardware.observations.v1".into(),
                category: None,
                class: SharingClass::Sensitive,
                schema_id: "org.example.hardware.v1".into(),
                schema_version: 1,
                fields: serde_json::Map::from_iter([(
                    "cpu_name".into(),
                    serde_json::json!("private source CPU"),
                )]),
                opaque_payload: Vec::new(),
                resource_id: "active-host/1".into(),
                observed_at: crate::plugin_host::resources::unix_now().unwrap(),
                sequence: 0,
                source_observation_id: String::new(),
                session_id: Some("source-session".into()),
                publisher: None,
                expires_at: None,
            },
            crate::plugin_host::resources::unix_now().unwrap(),
        )
        .unwrap();

    let mut computer = fixture.host.computer_client();
    let listed = computer
        .list(ComputerListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            computer_context_id: "active-host".into(),
            projection_id: "hardware.v1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(listed.reports.is_empty());
}

#[tokio::test]
async fn presence_requires_separate_publish_and_read_permissions_and_bounds_ttl() {
    let fixture = Fixture::new(&[cap::GAME_PRESENCE_PUBLISH]).await;
    let invalid_ttl = fixture
        .client()
        .game_presence_publish(PresencePublishRequest {
            mutation: Some(fixture.mutation("presence-ttl", 0)),
            game_id: "42".into(),
            session_id: "session-a".into(),
            details: Some(Struct {
                fields: BTreeMap::from([("state".into(), value_string("playing"))]),
            }),
            ttl_seconds: 301,
        })
        .await
        .unwrap_err();
    assert_eq!(invalid_ttl.code(), Code::InvalidArgument);

    let mut presence = fixture.host.game_presence_client();
    let denied = presence
        .list(PresenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            game_id: String::new(),
            session_id: String::new(),
            projection_id: "presence.v1".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
}

#[tokio::test]
async fn presence_reads_do_not_include_another_plugins_sensitive_records_without_policy() {
    use crate::plugin_host::{
        grants::PluginIdentity, shared_data::Publication, sharing_policy::SharingClass,
    };

    let fixture = Fixture::new(PRESENCE_GRANTS).await;
    let now = crate::plugin_host::resources::unix_now().unwrap();
    fixture
        .data_broker
        .publish(
            Publication {
                owner: PluginIdentity {
                    plugin_id: "source.plugin".into(),
                    publisher_fingerprint: "cd".repeat(32),
                    profile_id: "a".into(),
                },
                dataset_id: "game.presence.v1".into(),
                category: None,
                class: SharingClass::Sensitive,
                schema_id: "game.presence.v1".into(),
                schema_version: 1,
                fields: [("details.mode".into(), serde_json::json!("ranked"))]
                    .into_iter()
                    .collect(),
                opaque_payload: Vec::new(),
                resource_id: "42/session-a".into(),
                observed_at: now,
                sequence: 1,
                source_observation_id: "source-presence-1".into(),
                session_id: Some("source-session".into()),
                publisher: None,
                expires_at: Some(now + 60),
            },
            now,
        )
        .unwrap();

    let mut presence = fixture.host.game_presence_client();
    let listed = presence
        .list(PresenceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            game_id: "42".into(),
            session_id: "session-a".into(),
            projection_id: "presence.v1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(listed.records.is_empty());
}

#[tokio::test]
async fn presence_is_removed_when_the_publishing_session_closes() {
    let fixture = Fixture::new(PRESENCE_GRANTS).await;
    fixture
        .client()
        .game_presence_publish(PresencePublishRequest {
            mutation: Some(fixture.mutation("presence-session-close", 0)),
            game_id: "42".into(),
            session_id: "session-a".into(),
            details: Some(Struct {
                fields: BTreeMap::from([("mode".into(), value_string("ranked"))]),
            }),
            ttl_seconds: 300,
        })
        .await
        .unwrap();

    let context = fixture.context();
    let caller = fixture
        .api
        .authorize_resource_session(Some(&context))
        .unwrap();
    assert_eq!(fixture.data_broker.close_session(&caller.session_id), 1);

    let mut presence = fixture.host.game_presence_client();
    let listed = presence
        .list(PresenceListRequest {
            page: Some(PageRequest {
                context: Some(context),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            game_id: "42".into(),
            session_id: "session-a".into(),
            projection_id: "presence.v1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(listed.records.is_empty());
}
