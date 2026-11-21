//! End-to-end GamePerformance coverage over the authenticated local plugin services.

use super::*;
use std::time::Duration;
use tonic::Code;

const BATCH_SIZE: usize = 1000;

const GRANTS: &[&str] = &[
    cap::GAME_LAUNCH,
    cap::GAME_PERFORMANCE_PUBLISH,
    cap::GAME_PERFORMANCE_READ,
];

fn sample(sequence: u64, observed_at: i64, fps: f64) -> PerformanceSample {
    PerformanceSample {
        observed_at,
        frames_per_second: fps,
        cpu_percent: 42.0,
        memory_bytes: 512 * 1024 * 1024,
        sequence,
        sampling_window_ms: 0,
        fps_method: PerformanceFpsMethod::Instantaneous as i32,
        fps_statistic: PerformanceFpsStatistic::Instantaneous as i32,
        quality: PerformanceMeasurementQuality::Measured as i32,
        ..Default::default()
    }
}

fn request(
    fixture: &Fixture,
    key: &str,
    expected_revision: u64,
    samples: Vec<PerformanceSample>,
) -> PerformanceSamplesRequest {
    PerformanceSamplesRequest {
        mutation: Some(fixture.mutation(key, expected_revision)),
        series_id: "series-primary".into(),
        game_id: "5".into(),
        samples,
        runtime_id: "runtime-main".into(),
        runtime_recipe_revision: "rev-2".into(),
        source: "overlay-frame-counter".into(),
        tool_name: "Fixture collector".into(),
        tool_version: "1.2".into(),
        sample_interval_ms: 100,
        cpu_scope: PerformanceCpuScope::Process as i32,
        cpu_normalization: PerformanceCpuNormalization::WholeMachinePercent as i32,
        memory_kind: PerformanceMemoryKind::WorkingSetBytes as i32,
        memory_source: "linux-proc-status".into(),
        fps_unit: "frames/second".into(),
        cpu_unit: "percent".into(),
        memory_unit: "bytes".into(),
        ..Default::default()
    }
}

async fn fixture() -> Fixture {
    Fixture::start_with_target(
        GRANTS,
        true,
        None,
        Some(TargetGameContext {
            game_id: "5".into(),
            installation_id: "steam:five".into(),
            profile_id: String::new(),
            revision: "library-rev-1".into(),
        }),
    )
    .await
}

#[tokio::test]
async fn performance_batches_are_retained_queryable_and_aggregated_with_context() {
    let fixture = fixture().await;
    let now = crate::plugin_host::resources::unix_now().unwrap() * 1000;
    let published = fixture
        .client()
        .game_performance_publish_samples(request(
            &fixture,
            "performance-batch-1",
            0,
            vec![sample(1, now - 200, 59.0), sample(2, now - 100, 61.0)],
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(published.accepted_samples, 2);
    assert!(published.report_id.starts_with("performance-"));

    let mut performance = fixture.host.game_performance_client();
    let listed = performance
        .list(PerformanceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 20,
                common_data_categories: None,
            }),
            game_id: "5".into(),
            computer_context_id: "active-host".into(),
            projection_id: "performance.v1".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.series.len(), 1);
    let series = &listed.series[0];
    assert_eq!(series.series_id, published.report_id);
    assert_eq!(series.installation_id, "steam:five");
    assert_eq!(series.computer_context_id, "active-host");
    assert_eq!(series.runtime_id, "runtime-main");
    assert_eq!(series.source, "overlay-frame-counter");
    assert_eq!(series.fps_unit, "frames/second");
    assert_eq!(series.cpu_unit, "percent");
    assert_eq!(series.memory_unit, "bytes");
    assert_eq!(series.samples.len(), 2);

    let queried = performance
        .query_series(PerformanceQuerySeriesRequest {
            context: Some(fixture.context()),
            series_id: series.series_id.clone(),
            from_time: now - 300,
            to_time: now,
            resolution_ms: 300,
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(queried.samples.len(), 1);
    assert_eq!(queried.coverage, "downsampled");
    assert_eq!(queried.dropped_samples, 0);

    let aggregate = performance
        .aggregate(PerformanceAggregateRequest {
            context: Some(fixture.context()),
            series_id: series.series_id.clone(),
            from_time: now - 300,
            to_time: now,
            metric: "fps".into(),
            aggregation: "average".into(),
            common_data_categories: None,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(aggregate.value, 60.0);
    assert_eq!(aggregate.unit, "frames/second");
    assert_eq!(aggregate.sample_count, 2);
    assert_eq!(aggregate.coverage, "complete");
    let cpu = performance
        .aggregate(PerformanceAggregateRequest {
            context: Some(fixture.context()),
            series_id: series.series_id.clone(),
            from_time: now - 300,
            to_time: now,
            metric: "cpu_percent".into(),
            aggregation: "average".into(),
            common_data_categories: None,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(cpu.value, 42.0);
    assert_eq!(cpu.unit, "percent");
    let memory = performance
        .aggregate(PerformanceAggregateRequest {
            context: Some(fixture.context()),
            series_id: series.series_id.clone(),
            from_time: now - 300,
            to_time: now,
            metric: "memory_bytes".into(),
            aggregation: "max".into(),
            common_data_categories: None,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(memory.value, (512 * 1024 * 1024) as f64);
    assert_eq!(memory.unit, "bytes");
    let percentile = performance
        .aggregate(PerformanceAggregateRequest {
            context: Some(fixture.context()),
            series_id: series.series_id.clone(),
            from_time: now - 300,
            to_time: now,
            metric: "fps".into(),
            aggregation: "p95".into(),
            common_data_categories: None,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(percentile.value, 61.0);

    let mut watch = performance
        .watch(PerformanceWatchRequest {
            context: Some(fixture.context()),
            game_id: "5".into(),
            computer_context_id: "active-host".into(),
            projection_id: "performance.v1".into(),
            after_sequence: listed.revision,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();

    let ended = fixture
        .client()
        .game_performance_end_series(PerformanceEndSeriesRequest {
            mutation: Some(fixture.mutation("performance-end", series.revision)),
            series_id: "series-primary".into(),
            ended_at: now,
            expected_last_sequence: 2,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(ended.outcome, MutationOutcome::Applied as i32);
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), watch.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, "ended");
    let after_end = fixture
        .client()
        .game_performance_publish_samples(request(
            &fixture,
            "performance-after-end",
            ended.revision,
            vec![sample(3, now + 100, 60.0)],
        ))
        .await
        .unwrap_err();
    assert_eq!(after_end.code(), Code::FailedPrecondition);
}

#[tokio::test]
async fn performance_rejects_out_of_order_and_reports_batch_backpressure() {
    let fixture = fixture().await;
    let now = crate::plugin_host::resources::unix_now().unwrap() * 1000;
    let first = request(
        &fixture,
        "performance-first",
        0,
        vec![sample(5, now - 100, 60.0)],
    );
    let first_reply = fixture
        .client()
        .game_performance_publish_samples(first.clone())
        .await
        .unwrap()
        .into_inner();
    let mut repeated_request = first;
    repeated_request.mutation.as_mut().unwrap().idempotency_key = "performance-first-retry".into();
    let repeated = fixture
        .client()
        .game_performance_publish_samples(repeated_request)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(repeated.outcome, MutationOutcome::AlreadyApplied as i32);
    assert_eq!(repeated.revision, first_reply.revision);
    let mut performance = fixture.host.game_performance_client();
    let listed = performance
        .list(PerformanceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            projection_id: "performance.v1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.series[0].coverage, "gapped");
    assert_eq!(listed.series[0].dropped_samples, 4);

    let stale = request(
        &fixture,
        "performance-stale",
        first_reply.revision,
        vec![sample(4, now, 60.0)],
    );
    assert_eq!(
        fixture
            .client()
            .game_performance_publish_samples(stale)
            .await
            .unwrap_err()
            .code(),
        Code::Aborted
    );

    let pressure = request(
        &fixture,
        "performance-pressure",
        first_reply.revision,
        vec![sample(6, now, 60.0)],
    );
    let pressure = fixture
        .client()
        .game_performance_publish_samples(pressure)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(pressure.outcome, MutationOutcome::Backpressure as i32);
    assert_eq!(pressure.dropped_samples, 1);
    assert!(pressure.retry_after_ms > 0);
}

#[tokio::test]
async fn performance_reports_reject_invalid_units_normalization_and_future_samples() {
    let fixture = fixture().await;
    let now = crate::plugin_host::resources::unix_now().unwrap() * 1000;
    let mut wrong_unit = request(&fixture, "performance-unit", 0, vec![sample(1, now, 60.0)]);
    wrong_unit.memory_unit = "percent".into();
    assert_eq!(
        fixture
            .client()
            .game_performance_publish_samples(wrong_unit)
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );

    let mut wrong_normalization =
        request(&fixture, "performance-cpu", 0, vec![sample(1, now, 60.0)]);
    wrong_normalization.cpu_normalization = PerformanceCpuNormalization::WholeMachinePercent as i32;
    wrong_normalization.samples[0].cpu_percent = 101.0;
    assert_eq!(
        fixture
            .client()
            .game_performance_publish_samples(wrong_normalization)
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );

    let mut future = request(
        &fixture,
        "performance-future",
        0,
        vec![sample(1, now + 400_000, 60.0)],
    );
    future.samples[0].observed_at = now + 400_000;
    assert_eq!(
        fixture
            .client()
            .game_performance_publish_samples(future)
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );

    let mut excessive = request(
        &fixture,
        "performance-count",
        0,
        vec![sample(1, now, 60.0); 1001],
    );
    for (index, item) in excessive.samples.iter_mut().enumerate() {
        item.sequence = index as u64 + 1;
        item.observed_at = now + index as i64 * 100;
    }
    assert_eq!(
        fixture
            .client()
            .game_performance_publish_samples(excessive)
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
}

#[tokio::test]
async fn performance_accepts_per_core_cpu_and_frame_time_percentile_semantics() {
    let fixture = fixture().await;
    let now = crate::plugin_host::resources::unix_now().unwrap() * 1000;
    let mut report = request(
        &fixture,
        "performance-per-core",
        0,
        vec![sample(1, now, 120.0)],
    );
    report.cpu_scope = PerformanceCpuScope::Process as i32;
    report.cpu_normalization = PerformanceCpuNormalization::PerCorePercent as i32;
    report.samples[0].cpu_percent = 240.0;
    report.samples[0].fps_method = PerformanceFpsMethod::FrameTimeDerived as i32;
    report.samples[0].fps_statistic = PerformanceFpsStatistic::Percentile as i32;
    report.samples[0].sampling_window_ms = 100;
    report.samples[0].fps_percentile = Some(95.0);
    let reply = fixture
        .client()
        .game_performance_publish_samples(report)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(reply.accepted_samples, 1);
}

#[tokio::test]
async fn common_performance_history_requires_an_explicit_selector_and_current_grant() {
    use crate::plugin_host::{
        grants::PluginIdentity, shared_data::Publication, sharing_policy::SharingClass,
    };

    let fixture = fixture().await;
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
                dataset_id: "game.performance.history.v1".into(),
                category: Some(CommonDataCategory::GamePerformanceHistory),
                class: SharingClass::Common,
                schema_id: "game.performance.history.v1".into(),
                schema_version: 1,
                fields: [
                    ("game_id".into(), serde_json::json!("5")),
                    ("installation_id".into(), serde_json::json!("steam:five")),
                    (
                        "computer_context_id".into(),
                        serde_json::json!("active-host"),
                    ),
                    ("session_id".into(), serde_json::json!("source-session")),
                    (
                        "producer_plugin_id".into(),
                        serde_json::json!("source.plugin"),
                    ),
                    ("runtime_id".into(), serde_json::json!("runtime-main")),
                    ("runtime_recipe_revision".into(), serde_json::json!("")),
                    ("collector_source".into(), serde_json::json!("fixture")),
                    ("fps_unit".into(), serde_json::json!("frames/second")),
                    ("cpu_unit".into(), serde_json::json!("percent")),
                    ("memory_unit".into(), serde_json::json!("bytes")),
                    ("tool_name".into(), serde_json::json!("fixture")),
                    ("tool_version".into(), serde_json::json!("1")),
                    ("sample_interval_ms".into(), serde_json::json!(100)),
                    (
                        "cpu_scope".into(),
                        serde_json::json!(PerformanceCpuScope::Process as i32),
                    ),
                    (
                        "cpu_normalization".into(),
                        serde_json::json!(PerformanceCpuNormalization::WholeMachinePercent as i32),
                    ),
                    (
                        "memory_kind".into(),
                        serde_json::json!(PerformanceMemoryKind::WorkingSetBytes as i32),
                    ),
                    ("memory_source".into(), serde_json::json!("fixture")),
                    (
                        "samples".into(),
                        serde_json::Value::String(
                            serde_json::to_string(&serde_json::json!([{
                                "observed_at": now * 1000,
                                "frames_per_second": 60.0,
                                "cpu_percent": 40.0,
                                "memory_bytes": 4096,
                                "sequence": 1,
                                "sampling_window_ms": 0,
                                "fps_method": PerformanceFpsMethod::Instantaneous as i32,
                                "fps_statistic": PerformanceFpsStatistic::Instantaneous as i32,
                                "fps_percentile": null,
                                "frames_per_second_min": null,
                                "frames_per_second_max": null,
                                "quality": PerformanceMeasurementQuality::Measured as i32
                            }]))
                            .unwrap(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
                opaque_payload: Vec::new(),
                resource_id: "performance-source-series".into(),
                observed_at: now,
                sequence: 1,
                source_observation_id: "source-observation-1".into(),
                session_id: Some("source-session".into()),
                publisher: None,
                expires_at: None,
            },
            now,
        )
        .unwrap();

    let mut performance = fixture.host.game_performance_client();
    let private = performance
        .list(PerformanceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: None,
            }),
            projection_id: "performance.v1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert!(private.series.is_empty());

    let selected_but_not_granted = performance
        .list(PerformanceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 10,
                common_data_categories: Some(CommonDataSelector {
                    categories: vec![CommonDataCategory::GamePerformanceHistory as i32],
                    selected_record_ids: Vec::new(),
                }),
            }),
            projection_id: "performance.v1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert!(selected_but_not_granted.series.is_empty());
}

#[tokio::test]
async fn performance_history_flows_to_a_consumer_with_an_explicit_category_grant() {
    use crate::plugin_host::sharing_policy::ActivationReadGrant;

    let fixture = fixture().await;
    let consumer =
        super::data::consumer_session_with_grants(&fixture, &[cap::GAME_PERFORMANCE_READ]).await;
    let consumer_context = consumer.host.resource_context().unwrap().clone();
    let recipient = super::data::authenticated_identity(&fixture, &consumer_context);
    fixture
        .shared_policies
        .set_activation_grants(
            &recipient,
            ActivationReadGrant::new(
                [CommonDataCategory::GamePerformanceHistory],
                std::iter::empty(),
            )
            .unwrap(),
        )
        .unwrap();

    let now = crate::plugin_host::resources::unix_now().unwrap() * 1000;
    let published = fixture
        .client()
        .game_performance_publish_samples(request(
            &fixture,
            "producer-performance",
            0,
            vec![sample(1, now - 100, 59.0)],
        ))
        .await
        .unwrap()
        .into_inner();

    let mut performance = consumer.host.game_performance_client();
    let list = |selector| PerformanceListRequest {
        page: Some(PageRequest {
            context: Some(consumer_context.clone()),
            page_size: 10,
            common_data_categories: selector,
            ..Default::default()
        }),
        game_id: "5".into(),
        computer_context_id: "active-host".into(),
        projection_id: "performance.v1".into(),
    };
    assert!(
        performance
            .list(list(None))
            .await
            .unwrap()
            .into_inner()
            .series
            .is_empty()
    );
    let listed = performance
        .list(list(Some(CommonDataSelector {
            categories: vec![CommonDataCategory::GamePerformanceHistory as i32],
            selected_record_ids: Vec::new(),
        })))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.series.len(), 1);
    assert_eq!(listed.series[0].series_id, published.report_id);
    assert_eq!(listed.series[0].samples.len(), 1);
    assert_eq!(listed.series[0].samples[0].frames_per_second, 59.0);
}

#[tokio::test]
async fn performance_series_quota_backpressures_without_losing_accepted_samples() {
    let fixture = fixture().await;
    let now = crate::plugin_host::resources::unix_now().unwrap() * 1000;
    let mut revision = 0;
    let mut next_sequence = 1;
    let mut accepted = 0;
    let mut saw_backpressure = false;
    for batch in 0..8 {
        if batch > 0 {
            tokio::time::sleep(Duration::from_millis(1001)).await;
        }
        let samples = (0..BATCH_SIZE)
            .map(|offset| {
                let sequence = next_sequence + offset as u64;
                let observed_at = now - 600_000 + (sequence as i64 - 1) * 100;
                sample(sequence, observed_at, 60.0)
            })
            .collect();
        let report = request(
            &fixture,
            &format!("performance-quota-{batch}"),
            revision,
            samples,
        );
        let validation = super::super::performance::validate_request(&report, now + 1);
        assert!(
            validation.is_ok(),
            "batch {batch} validation failed: {validation:?}"
        );
        let reply = fixture
            .client()
            .game_performance_publish_samples(report)
            .await
            .unwrap_or_else(|error| panic!("quota batch {batch} failed: {error:?}"))
            .into_inner();
        if reply.outcome == MutationOutcome::Backpressure as i32 {
            saw_backpressure = true;
            assert_eq!(reply.accepted_samples, 0);
            assert_eq!(reply.dropped_samples, BATCH_SIZE as u32);
            break;
        }
        accepted += reply.accepted_samples as usize;
        revision = reply.revision;
        next_sequence += BATCH_SIZE as u64;
    }
    assert!(
        saw_backpressure,
        "a growing series should hit the 1 MiB retained-record quota"
    );
    assert!(accepted >= 1000);

    let mut performance = fixture.host.game_performance_client();
    let listed = performance
        .list(PerformanceListRequest {
            page: Some(PageRequest {
                context: Some(fixture.context()),
                cursor: String::new(),
                page_size: 1,
                common_data_categories: None,
            }),
            projection_id: "performance.v1".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(listed.series.len(), 1);
    assert_eq!(listed.series[0].samples.len(), accepted);
}
