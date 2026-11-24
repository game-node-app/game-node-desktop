//! Producer/consumer exchanges for performance history, approved locations, save profiles and a
//! shared runtime with several content identities.
//!
//! These are plain functions over the generated clients returned by [`HostClient`]; copy what you
//! need. They are compiled with the SDK and run against the desktop's fixture host in its service
//! tests, so they stay true to what the host does.
//!
//! What the host interprets and what it does not:
//!
//! - The host selects the game, installation and local `active-host` computer context from the
//!   authenticated session. A request that names another game is refused; the caller never picks
//!   its own computer identity.
//! - Units, sample sequence, FPS method/statistic/quality, CPU scope/normalization and memory kind
//!   are validated and retained as given. A source or tool name is descriptive metadata: the host
//!   does not start a collector or verify that one ran.
//! - Location inspection returns opaque handles and volume tokens for an approved folder. No reply
//!   contains a filesystem path, so a plugin never learns where the folder is.
//! - Save profiles are owner-only. A common-category selector on the save-profile routes is an
//!   `UNIMPLEMENTED` answer rather than a narrowed projection.
//! - A shared runtime projects its executable handle as empty, and content availability carries
//!   safe content/runtime identities only.
//!
//! Cross-plugin reads need an explicit selector on every query plus a current host grant. Without
//! the selector the answer is private-only; with a revoked grant the call is `PERMISSION_DENIED`.
#![allow(dead_code)]

use std::time::Duration;

use gamenode_plugin_sdk::{
    HostClient, Status,
    proto::{
        CommonDataCategory, CommonDataSelector, DomainListRequest, LibraryGetReply,
        LibraryGetRequest, LocationInspectReply, LocationInspectRequest, LocationSizeReply,
        LocationSizeRequest, MutationContext, PageRequest, PerformanceAggregateReply,
        PerformanceAggregateRequest, PerformanceCpuNormalization, PerformanceCpuScope,
        PerformanceEndSeriesRequest, PerformanceFpsMethod, PerformanceFpsStatistic,
        PerformanceListReply, PerformanceListRequest, PerformanceMeasurementQuality,
        PerformanceMemoryKind, PerformanceQuerySeriesReply, PerformanceQuerySeriesRequest,
        PerformanceSample, PerformanceSamplesRequest, ReportMutationReply, RuntimesListReply,
        SaveProfileGetRequest, SaveProfileListReply, SaveProfileListRequest, SaveProfileReply,
    },
};

/// The one computer context the host binds to an authenticated session.
pub const LOCAL_COMPUTER: &str = "active-host";
/// A host-defined projection for retained performance series.
pub const PERFORMANCE_PROJECTION: &str = "performance.v1";
/// Requested location dimensions; the host answers the ones it supports.
pub const LOCATION_DIMENSIONS: [&str; 3] = ["availability", "volume", "capacity"];

pub fn category_selector(category: CommonDataCategory) -> CommonDataSelector {
    CommonDataSelector {
        categories: vec![category as i32],
        selected_record_ids: Vec::new(),
    }
}

pub fn performance_selector() -> CommonDataSelector {
    category_selector(CommonDataCategory::GamePerformanceHistory)
}

/// One actual observation as an example game overlay would collect it. Nothing here is captured
/// by GameNode: the values are the publisher's claim with explicit units and semantics.
pub fn sample(sequence: u64, observed_at: i64, fps: f64) -> PerformanceSample {
    PerformanceSample {
        observed_at,
        frames_per_second: fps,
        cpu_percent: 34.5,
        memory_bytes: 512 * 1024 * 1024,
        sequence,
        sampling_window_ms: 0,
        fps_method: PerformanceFpsMethod::Instantaneous as i32,
        fps_statistic: PerformanceFpsStatistic::Instantaneous as i32,
        quality: PerformanceMeasurementQuality::Measured as i32,
        ..Default::default()
    }
}

/// A complete batch for one selected game. `expected_revision` is the series revision returned by
/// the previous batch (0 for the first). The host binds the game, installation, computer context
/// and producer; `game_id` here only has to match the selected target.
pub fn performance_batch(
    context: &gamenode_plugin_sdk::proto::ServiceContext,
    key: &str,
    expected_revision: u64,
    series_id: &str,
    game_id: &str,
    samples: Vec<PerformanceSample>,
) -> PerformanceSamplesRequest {
    PerformanceSamplesRequest {
        mutation: Some(MutationContext {
            context: Some(context.clone()),
            operation_id: String::new(),
            idempotency_key: key.into(),
            expected_revision,
        }),
        series_id: series_id.into(),
        game_id: game_id.into(),
        samples,
        runtime_id: "runtime:example".into(),
        runtime_recipe_revision: "recipe-2".into(),
        source: "overlay-frame-counter".into(),
        tool_name: "Example overlay".into(),
        tool_version: "2.0".into(),
        sample_interval_ms: 100,
        cpu_scope: PerformanceCpuScope::Process as i32,
        cpu_normalization: PerformanceCpuNormalization::WholeMachinePercent as i32,
        memory_kind: PerformanceMemoryKind::WorkingSetBytes as i32,
        memory_source: "example-overlay".into(),
        fps_unit: "frames/second".into(),
        cpu_unit: "percent".into(),
        memory_unit: "bytes".into(),
        ..Default::default()
    }
}

pub async fn publish_performance(
    host: &HostClient,
    key: &str,
    expected_revision: u64,
    series_id: &str,
    game_id: &str,
    samples: Vec<PerformanceSample>,
) -> Result<ReportMutationReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.resource_client()
        .game_performance_publish_samples(performance_batch(
            context,
            key,
            expected_revision,
            series_id,
            game_id,
            samples,
        ))
        .await
        .map(|reply| reply.into_inner())
}

/// Lists retained series in the caller's selected game scope. Without `selector` the common-data
/// path returns only the caller's own series; another plugin's history needs the explicit
/// performance-history category (or authorized record IDs) and a current grant.
pub async fn list_performance(
    host: &HostClient,
    game_id: &str,
    selector: Option<CommonDataSelector>,
    cursor: &str,
    page_size: u32,
) -> Result<PerformanceListReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.game_performance_client()
        .list(PerformanceListRequest {
            page: Some(PageRequest {
                context: Some(context.clone()),
                cursor: cursor.into(),
                page_size,
                common_data_categories: selector,
            }),
            game_id: game_id.into(),
            computer_context_id: LOCAL_COMPUTER.into(),
            projection_id: PERFORMANCE_PROJECTION.into(),
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Reads actual retained samples in a window. `resolution_ms = 0` keeps every sample; a larger
/// value selects actual observations only and never interpolates new points.
pub async fn query_performance(
    host: &HostClient,
    series_id: &str,
    from_time: i64,
    to_time: i64,
    resolution_ms: u32,
    selector: Option<CommonDataSelector>,
) -> Result<PerformanceQuerySeriesReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.game_performance_client()
        .query_series(PerformanceQuerySeriesRequest {
            context: Some(context.clone()),
            series_id: series_id.into(),
            from_time,
            to_time,
            resolution_ms,
            page: Some(PageRequest {
                context: Some(context.clone()),
                page_size: 100,
                ..Default::default()
            }),
            common_data_categories: selector,
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Aggregates one metric over actual samples. `metric` is `fps`, `cpu_percent` or `memory_bytes`;
/// `aggregation` is `average`, `min`, `max` or a nearest-rank `p0`–`p100`.
pub async fn aggregate_performance(
    host: &HostClient,
    series_id: &str,
    metric: &str,
    aggregation: &str,
    selector: Option<CommonDataSelector>,
) -> Result<PerformanceAggregateReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    let now = 0;
    host.game_performance_client()
        .aggregate(PerformanceAggregateRequest {
            context: Some(context.clone()),
            series_id: series_id.into(),
            from_time: now,
            to_time: 0,
            metric: metric.into(),
            aggregation: aggregation.into(),
            common_data_categories: selector,
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Ends a series when the game stops. The host keeps the retained samples readable but refuses a
/// later batch with `FAILED_PRECONDITION`, so a stopped collector cannot quietly reopen history.
pub async fn end_performance(
    host: &HostClient,
    series_id: &str,
    expected_revision: u64,
    expected_last_sequence: u64,
    ended_at: i64,
) -> Result<gamenode_plugin_sdk::proto::MutationReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.resource_client()
        .game_performance_end_series(PerformanceEndSeriesRequest {
            mutation: Some(MutationContext {
                context: Some(context.clone()),
                operation_id: String::new(),
                idempotency_key: gamenode_plugin_sdk::idempotency_key(),
                expected_revision,
            }),
            series_id: series_id.into(),
            ended_at,
            expected_last_sequence,
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Inspects an approved folder or runtime executable by its opaque handle. The answer describes
/// the approved scope with an opaque volume token; no field carries the folder's path.
pub async fn inspect_location(
    host: &HostClient,
    approved_resource_handle: &str,
) -> Result<LocationInspectReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.locations_client()
        .inspect(LocationInspectRequest {
            context: Some(context.clone()),
            approved_resource_handle: approved_resource_handle.into(),
            requested_dimensions: LOCATION_DIMENSIONS.iter().map(|d| (*d).into()).collect(),
            max_entries: 100,
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Measures the approved scope by metadata only. A missing or unreadable folder is reported through
/// `availability`, never as zero bytes.
pub async fn measure_location(
    host: &HostClient,
    approved_resource_handle: &str,
) -> Result<LocationSizeReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.locations_client()
        .get_size(LocationSizeRequest {
            context: Some(context.clone()),
            approved_resource_handle: approved_resource_handle.into(),
            max_entries: 0,
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Lists save profiles for one of the caller's own games. `selector` is accepted on the wire, but
/// this build answers a common-category selector with `UNIMPLEMENTED`: save profiles stay
/// owner-only, and a plugin does not get another plugin's folders through a shared selector.
pub async fn list_save_profiles(
    host: &HostClient,
    installation_id: &str,
    selector: Option<CommonDataSelector>,
) -> Result<SaveProfileListReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.saves_client()
        .list_profiles(SaveProfileListRequest {
            context: Some(context.clone()),
            game_id: String::new(),
            installation_id: installation_id.into(),
            cursor: String::new(),
            page_size: 20,
            shared_records: selector,
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Reads one of the caller's own profiles. The approved folder is reported as a session-bound
/// handle; when the approval is gone the handle is empty and the profile keeps its metadata.
pub async fn read_save_profile(
    host: &HostClient,
    game_id: &str,
    profile_id: &str,
) -> Result<SaveProfileReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.saves_client()
        .get_profile(SaveProfileGetRequest {
            context: Some(context.clone()),
            game_id: game_id.into(),
            profile_id: profile_id.into(),
            shared_records: None,
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Lists runtime records. With the explicit runtime-catalog category, the result includes other
/// plugins' runtimes as a shared projection: no executable handle, validated extension namespaces
/// and host-recorded producer provenance. Without a current grant the call is `PERMISSION_DENIED`.
pub async fn list_shared_runtimes(host: &HostClient) -> Result<RuntimesListReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.resource_client()
        .runtimes_list(DomainListRequest {
            page: Some(PageRequest {
                context: Some(context.clone()),
                page_size: 50,
                common_data_categories: Some(category_selector(CommonDataCategory::RuntimeCatalog)),
                ..Default::default()
            }),
            kind: "runtime".into(),
            owner_filter: String::new(),
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Reads one installation's safe content/runtime availability. Several content identities can
/// point at one shared runtime; each row carries content and runtime IDs, a display name and a
/// publisher, never a path, handle or payload.
pub async fn content_availability(
    host: &HostClient,
    installation_id: &str,
) -> Result<LibraryGetReply, Status> {
    let context = host
        .resource_context()
        .ok_or_else(|| Status::failed_precondition("the session has no resource context"))?;
    host.resource_client()
        .library_get(LibraryGetRequest {
            context: Some(context.clone()),
            game_id: String::new(),
            installation_id: installation_id.into(),
            common_data_categories: Some(category_selector(
                CommonDataCategory::ContentAvailability,
            )),
        })
        .await
        .map(|reply| reply.into_inner())
}

/// Retries a call that failed only because the transport was briefly unavailable.
pub async fn send_with_retry<T, F, Fut>(mut call: F) -> Result<T, Status>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, Status>>,
{
    let mut last = Status::unavailable("nothing was sent");
    for attempt in 0..3 {
        match call().await {
            Err(status)
                if matches!(
                    status.code(),
                    gamenode_plugin_sdk::Code::Unavailable
                        | gamenode_plugin_sdk::Code::DeadlineExceeded
                ) =>
            {
                last = status;
                tokio::time::sleep(Duration::from_millis(200 << attempt)).await;
            }
            done => return done,
        }
    }
    Err(last)
}

fn main() {}
