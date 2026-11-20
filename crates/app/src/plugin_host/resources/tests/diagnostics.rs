//! Public StorageDiagnostics RPC flow and permission fences.

use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

const GRANTS: &[&str] = &[
    cap::STORAGE_DIAGNOSTICS_PUBLISH,
    cap::STORAGE_DIAGNOSTICS_READ,
    cap::FILES_READ,
];

#[tokio::test]
async fn benchmark_report_is_host_volume_bound_and_retrievable_over_public_services() {
    let fixture = Fixture::new(GRANTS).await;
    let installation = fixture.game_dir("installed", &[]);
    fixture.library_in(&[("fixture:1", Some(42), "Example game", &installation)]);
    fixture.user_approving(&installation);
    let handle = fixture
        .request_access("id-fixture:1", &["read"], &[])
        .await
        .unwrap()
        .handles
        .remove(0)
        .handle;
    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let published = fixture
        .client()
        .storage_diagnostics_publish_benchmark(BenchmarkReportRequest {
            mutation: Some(fixture.mutation("benchmark-report", 0)),
            volume_handle: handle,
            tool_name: "FixtureBench".into(),
            tool_version: "2.1".into(),
            measurements: std::collections::HashMap::from([("sequential_read".into(), 12_000.0)]),
            units: "bytes_per_second".into(),
            observed_at,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert!(!published.report_id.is_empty());

    let mut diagnostics = fixture.host.storage_diagnostics_client();
    let listed = diagnostics
        .list(DiagnosticListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            owner_plugin_id: "fixture.plugin".into(),
            volume_identity_token: String::new(),
            projection_id: "benchmark.v1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.reports.len(), 1);
    let report = &listed.reports[0];
    assert_eq!(report.report_id, published.report_id);
    assert_eq!(report.report_kind, "benchmark");
    assert_eq!(report.owner_plugin_id, "fixture.plugin");
    assert!(!report.volume_identity_token.is_empty());
    assert_eq!(report.schema_id, "storage.diagnostics.benchmark.v1");
    let fields = report.typed_measurements.as_ref().unwrap();
    assert!(fields.fields.contains_key("measurements.sequential_read"));
    assert!(!fields.fields.contains_key("volume_identity_token"));

    let fetched = diagnostics
        .get(DiagnosticGetRequest {
            context: Some(fixture.context()),
            report_id: published.report_id,
            projection_id: "benchmark.v1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(fetched.report.unwrap().report_kind, "benchmark");
}

#[tokio::test]
async fn diagnostics_publish_and_read_require_separate_permissions() {
    let fixture = Fixture::new(&[cap::FILES_READ]).await;
    let status = fixture
        .client()
        .storage_diagnostics_publish_benchmark(BenchmarkReportRequest {
            mutation: Some(fixture.mutation("denied-publish", 0)),
            volume_handle: "not-resolved".into(),
            tool_name: "FixtureBench".into(),
            tool_version: "1".into(),
            units: "bytes_per_second".into(),
            measurements: std::collections::HashMap::from([("read".into(), 1.0)]),
            observed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), tonic::Code::PermissionDenied);

    let mut diagnostics = fixture.host.storage_diagnostics_client();
    let status = diagnostics
        .list(DiagnosticListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            owner_plugin_id: String::new(),
            volume_identity_token: String::new(),
            projection_id: "benchmark.v1".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
}

#[tokio::test]
async fn benchmark_report_flows_from_producer_to_consumer_with_reviewed_projection() {
    use crate::plugin_host::sharing_policy::{SharingClass, SourceRecipientPolicy};

    let fixture = Fixture::new(&[
        cap::STORAGE_DIAGNOSTICS_PUBLISH,
        cap::STORAGE_DIAGNOSTICS_READ,
        cap::FILES_READ,
    ])
    .await;
    let consumer =
        super::data::consumer_session_with_grants(&fixture, &[cap::STORAGE_DIAGNOSTICS_READ]).await;
    let consumer_context = consumer.host.resource_context().unwrap().clone();
    let source = super::data::authenticated_identity(&fixture, &fixture.context());
    let recipient = super::data::authenticated_identity(&fixture, &consumer_context);
    fixture
        .shared_policies
        .set_source_recipient_policy(
            SourceRecipientPolicy::new(
                source,
                recipient,
                SharingClass::Sensitive,
                "storage.diagnostics.benchmark.v1".into(),
                ["measurements.sequential_read".into()],
                true,
                false,
            )
            .unwrap(),
        )
        .unwrap();

    let installation = fixture.game_dir("installed", &[]);
    fixture.library_in(&[("fixture:1", Some(42), "Example game", &installation)]);
    fixture.user_approving(&installation);
    let handle = fixture
        .request_access("id-fixture:1", &["read"], &[])
        .await
        .unwrap()
        .handles
        .remove(0)
        .handle;
    let published = fixture
        .client()
        .storage_diagnostics_publish_benchmark(BenchmarkReportRequest {
            mutation: Some(fixture.mutation("producer-benchmark", 0)),
            volume_handle: handle,
            tool_name: "FixtureBench".into(),
            tool_version: "2.1".into(),
            measurements: std::collections::HashMap::from([
                ("sequential_read".into(), 12_000.0),
                ("random_read".into(), 3_000.0),
            ]),
            units: "bytes_per_second".into(),
            observed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();

    let mut diagnostics = consumer.host.storage_diagnostics_client();
    let listed = diagnostics
        .list(DiagnosticListRequest {
            page: Some(PageRequest {
                context: Some(consumer_context.clone()),
                page_size: 10,
                ..Default::default()
            }),
            projection_id: "benchmark.v1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.reports.len(), 1);
    let report = &listed.reports[0];
    assert_eq!(report.report_id, published.report_id);
    let fields = &report.typed_measurements.as_ref().unwrap().fields;
    assert!(fields.contains_key("measurements.sequential_read"));
    assert!(!fields.contains_key("measurements.random_read"));
    assert!(!fields.contains_key("volume_identity_token"));
}
