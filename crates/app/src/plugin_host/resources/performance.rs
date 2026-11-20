//! Bounded plugin-produced game-performance history. The host validates and retains samples but
//! starts no collector and does not infer process ownership from reported metrics.

use super::super::shared_data::{MAX_PAGE_SIZE, Publication, PublisherMetadata, QueryRequest};
use super::super::sharing_policy::{Delivery, SharingClass};
use super::*;
use futures_util::Stream;
use gamenode_plugin_protocol::prost::Message;
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use sha2::{Digest, Sha256};
use std::pin::Pin;
use std::time::Duration;
use tonic::{Request, Response, Status};

const DATASET: &str = "game.performance.history.v1";
const PROJECTION: &str = "performance.v1";
const HOST_COMPUTER_CONTEXT: &str = "active-host";
const MAX_SERIES_ID: usize = 128;
const MAX_TEXT: usize = 256;
const MAX_BATCH_SAMPLES: usize = 1000;
const MAX_BATCH_BYTES: usize = 1024 * 1024;
const MAX_SERIES_BYTES: usize = 1024 * 1024;
const MIN_SAMPLE_INTERVAL_MS: u32 = 100;
const MAX_SAMPLE_INTERVAL_MS: u32 = 60_000;
const MAX_SAMPLE_WINDOW_MS: u32 = 600_000;
const MAX_FPS: f64 = 10_000.0;
const MAX_PAGE: usize = 200;
const WATCH_QUEUE: usize = 32;
const WATCH_RECHECK: Duration = Duration::from_secs(5);
const BATCH_INTERVAL: Duration = Duration::from_secs(1);
const CURSOR_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_CURSORS: usize = 4096;

type WatchStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

pub(super) struct CursorEntry {
    session_id: String,
    owner: super::super::grants::PluginIdentity,
    scope_hash: [u8; 32],
    broker_revision: u64,
    policy_revision: u64,
    after_sequence: u64,
    expires_at: Instant,
}

pub const SERVED: &[Served] = &[
    Served::new(
        "List",
        cap::GAME_PERFORMANCE_READ,
        true,
        "own session-retained series or explicitly selected common category/records",
    ),
    Served::new(
        "Get",
        cap::GAME_PERFORMANCE_READ,
        false,
        "own series or explicitly selected common category/record",
    ),
    Served::new(
        "Watch",
        cap::GAME_PERFORMANCE_READ,
        false,
        "own session-retained series or explicitly selected common category/records",
    ),
    Served::new(
        "QuerySeries",
        cap::GAME_PERFORMANCE_READ,
        true,
        "retained samples projected by current sharing policy",
    ),
    Served::new(
        "Aggregate",
        cap::GAME_PERFORMANCE_READ,
        false,
        "aggregates only available retained samples",
    ),
];

fn broker_status(error: BrokerError) -> Status {
    match error {
        BrokerError::StaleCursor | BrokerError::StaleSubscription => {
            Status::failed_precondition("performance cursor is stale; query again")
        }
        BrokerError::AuthorizationChanged | BrokerError::Expired => {
            Status::failed_precondition("performance authorization changed")
        }
        BrokerError::RecordTooLarge | BrokerError::QuotaExceeded => {
            Status::resource_exhausted("performance retention capacity is exhausted")
        }
        BrokerError::SequenceConflict(_) => {
            Status::aborted("performance sample sequence is duplicate or out of order")
        }
        _ => Status::invalid_argument("invalid performance request"),
    }
}

fn validate_projection(value: &str) -> Result<(), Status> {
    if value == PROJECTION {
        Ok(())
    } else {
        Err(Status::invalid_argument("unknown performance projection"))
    }
}

fn valid_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
}

fn valid_text(value: &str, required: bool) -> bool {
    value.len() <= MAX_TEXT
        && (!required || !value.trim().is_empty())
        && !value.chars().any(char::is_control)
}

fn series_resource_id(owner: &super::super::grants::PluginIdentity, series_id: &str) -> String {
    let mut digest = Sha256::new();
    for part in [
        owner.plugin_id.as_bytes(),
        owner.publisher_fingerprint.as_bytes(),
        owner.profile_id.as_bytes(),
        series_id.as_bytes(),
    ] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    let mut token = String::from("performance-");
    for byte in digest.finalize() {
        use std::fmt::Write as _;
        let _ = write!(token, "{byte:02x}");
    }
    token
}

fn owner_key(owner: &super::super::grants::PluginIdentity) -> String {
    format!(
        "{}:{}:{}",
        owner.plugin_id, owner.publisher_fingerprint, owner.profile_id
    )
}

fn unix_now_millis() -> Result<i64, Status> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Status::internal("system clock is before the Unix epoch"))?
        .as_millis()
        .try_into()
        .map_err(|_| Status::internal("system clock exceeds the supported range"))
}

fn sample_json(sample: &PerformanceSample) -> JsonValue {
    json!({
        "observed_at": sample.observed_at,
        "frames_per_second": sample.frames_per_second,
        "cpu_percent": sample.cpu_percent,
        "memory_bytes": sample.memory_bytes,
        "sequence": sample.sequence,
        "sampling_window_ms": sample.sampling_window_ms,
        "fps_method": sample.fps_method,
        "fps_statistic": sample.fps_statistic,
        "fps_percentile": sample.fps_percentile,
        "frames_per_second_min": sample.frames_per_second_min,
        "frames_per_second_max": sample.frames_per_second_max,
        "quality": sample.quality,
    })
}

fn number(value: &JsonValue) -> Result<f64, Status> {
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .ok_or_else(|| Status::failed_precondition("stored performance sample is invalid"))
}

fn unsigned(value: &JsonValue) -> Result<u64, Status> {
    value
        .as_u64()
        .ok_or_else(|| Status::failed_precondition("stored performance sample is invalid"))
}

fn integer(value: &JsonValue) -> Result<i64, Status> {
    value
        .as_i64()
        .ok_or_else(|| Status::failed_precondition("stored performance sample is invalid"))
}

fn proto_sample(value: &JsonValue) -> Result<PerformanceSample, Status> {
    let field = |name: &str| {
        value
            .get(name)
            .ok_or_else(|| Status::failed_precondition("stored performance sample is incomplete"))
    };
    Ok(PerformanceSample {
        observed_at: integer(field("observed_at")?)?,
        frames_per_second: number(field("frames_per_second")?)?,
        cpu_percent: number(field("cpu_percent")?)?,
        memory_bytes: unsigned(field("memory_bytes")?)?,
        sequence: unsigned(field("sequence")?)?,
        sampling_window_ms: unsigned(field("sampling_window_ms")?)? as u32,
        fps_method: integer(field("fps_method")?)? as i32,
        fps_statistic: integer(field("fps_statistic")?)? as i32,
        fps_percentile: field("fps_percentile")?.as_f64(),
        frames_per_second_min: field("frames_per_second_min")?.as_f64(),
        frames_per_second_max: field("frames_per_second_max")?.as_f64(),
        quality: integer(field("quality")?)? as i32,
    })
}

fn sample_list(fields: &JsonMap<String, JsonValue>) -> Result<Vec<PerformanceSample>, Status> {
    let encoded = fields
        .get("samples")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Status::failed_precondition("stored performance series is invalid"))?;
    let decoded: JsonValue = serde_json::from_str(encoded)
        .map_err(|_| Status::failed_precondition("stored performance series is invalid"))?;
    decoded
        .as_array()
        .ok_or_else(|| Status::failed_precondition("stored performance series is invalid"))?
        .iter()
        .map(proto_sample)
        .collect()
}

fn required_string(fields: &JsonMap<String, JsonValue>, name: &str) -> Result<String, Status> {
    fields
        .get(name)
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Status::failed_precondition("stored performance series is invalid"))
}

fn required_u32(fields: &JsonMap<String, JsonValue>, name: &str) -> Result<u32, Status> {
    fields
        .get(name)
        .and_then(JsonValue::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| Status::failed_precondition("stored performance series is invalid"))
}

fn required_i32(fields: &JsonMap<String, JsonValue>, name: &str) -> Result<i32, Status> {
    fields
        .get(name)
        .and_then(JsonValue::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| Status::failed_precondition("stored performance series is invalid"))
}

fn missing_samples(samples: &[PerformanceSample]) -> u64 {
    samples
        .first()
        .map_or(0, |sample| sample.sequence.saturating_sub(1))
        .saturating_add(
            samples
                .windows(2)
                .map(|pair| {
                    pair[1]
                        .sequence
                        .saturating_sub(pair[0].sequence.saturating_add(1))
                })
                .sum::<u64>(),
        )
}

fn record_fields(record: &ProjectedRecord) -> Result<&JsonMap<String, JsonValue>, Status> {
    if record.schema() != (DATASET, 1) {
        return Err(Status::failed_precondition(
            "stored performance schema is invalid",
        ));
    }
    Ok(record.fields())
}

fn series(record: &ProjectedRecord) -> Result<PerformanceSeries, Status> {
    let fields = record_fields(record)?;
    let samples = sample_list(fields)?;
    let ended_at = fields
        .get("ended_at")
        .and_then(JsonValue::as_i64)
        .unwrap_or(0);
    let started_at = samples.first().map_or(0, |sample| sample.observed_at);
    let dropped_samples = missing_samples(&samples);
    let source_identity = &record.provenance().source;
    let owner_plugin_id = source_identity.plugin_id.clone();
    let provenance = record.provenance();
    let provenance_json = serde_json::to_string(&json!({
        "producer_plugin_id": owner_plugin_id,
        "publisher_fingerprint": source_identity.publisher_fingerprint,
        "package_version": provenance.package_version,
    }))
    .unwrap_or_default();
    Ok(PerformanceSeries {
        series_id: record.resource_id().to_owned(),
        record_id: record.record_id().to_owned(),
        game_id: required_string(fields, "game_id")?,
        installation_id: required_string(fields, "installation_id")?,
        computer_context_id: required_string(fields, "computer_context_id")?,
        session_id: required_string(fields, "session_id")?,
        owner_plugin_id: source_identity.plugin_id.clone(),
        samples,
        revision: record.revision(),
        runtime_id: required_string(fields, "runtime_id")?,
        runtime_recipe_revision: required_string(fields, "runtime_recipe_revision")?,
        source: required_string(fields, "collector_source")?,
        fps_unit: required_string(fields, "fps_unit")?,
        cpu_unit: required_string(fields, "cpu_unit")?,
        memory_unit: required_string(fields, "memory_unit")?,
        tool_name: required_string(fields, "tool_name")?,
        tool_version: required_string(fields, "tool_version")?,
        sample_interval_ms: required_u32(fields, "sample_interval_ms")?,
        cpu_scope: required_i32(fields, "cpu_scope")?,
        cpu_normalization: required_i32(fields, "cpu_normalization")?,
        memory_kind: required_i32(fields, "memory_kind")?,
        memory_source: required_string(fields, "memory_source")?,
        started_at,
        ended_at,
        dropped_samples,
        coverage: if dropped_samples == 0 {
            "complete"
        } else {
            "gapped"
        }
        .into(),
        provenance: provenance_json,
    })
}

fn query_request(
    caller: super::super::grants::PluginIdentity,
    selector: Option<CommonDataSelector>,
    delivery: Delivery,
    cursor: Option<QueryCursor>,
    page_size: usize,
) -> QueryRequest {
    QueryRequest {
        caller,
        dataset_id: DATASET.into(),
        record_id: None,
        category: Some(CommonDataCategory::GamePerformanceHistory),
        selector,
        delivery,
        page_size: page_size.min(MAX_PAGE_SIZE),
        cursor,
    }
}

fn cursor_scope(parts: &[&str], selector: Option<&CommonDataSelector>) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    if let Some(selector) = selector {
        digest.update(selector.encode_to_vec());
    }
    digest.finalize().into()
}

fn performance_fields(
    request: &PerformanceSamplesRequest,
    caller: &ResourceCaller,
    owner: &super::super::grants::PluginIdentity,
    token: &str,
    samples: &[PerformanceSample],
    ended_at: Option<i64>,
    batch_hash: &str,
) -> JsonMap<String, JsonValue> {
    let target = request
        .mutation
        .as_ref()
        .and_then(|mutation| mutation.context.as_ref())
        .and_then(|context| context.target_game.as_ref());
    let mut fields = JsonMap::from_iter([
        ("series_id".into(), JsonValue::String(token.into())),
        ("game_id".into(), JsonValue::String(request.game_id.clone())),
        (
            "installation_id".into(),
            JsonValue::String(
                target.map_or_else(String::new, |target| target.installation_id.clone()),
            ),
        ),
        (
            "computer_context_id".into(),
            JsonValue::String(HOST_COMPUTER_CONTEXT.into()),
        ),
        (
            "session_id".into(),
            JsonValue::String(caller.session_id.clone()),
        ),
        (
            "producer_plugin_id".into(),
            JsonValue::String(owner.plugin_id.clone()),
        ),
        (
            "runtime_id".into(),
            JsonValue::String(request.runtime_id.clone()),
        ),
        (
            "runtime_recipe_revision".into(),
            JsonValue::String(request.runtime_recipe_revision.clone()),
        ),
        (
            "collector_source".into(),
            JsonValue::String(request.source.clone()),
        ),
        (
            "tool_name".into(),
            JsonValue::String(request.tool_name.clone()),
        ),
        (
            "tool_version".into(),
            JsonValue::String(request.tool_version.clone()),
        ),
        (
            "sample_interval_ms".into(),
            JsonValue::Number(request.sample_interval_ms.into()),
        ),
        (
            "cpu_scope".into(),
            JsonValue::Number(request.cpu_scope.into()),
        ),
        (
            "cpu_normalization".into(),
            JsonValue::Number(request.cpu_normalization.into()),
        ),
        (
            "memory_kind".into(),
            JsonValue::Number(request.memory_kind.into()),
        ),
        (
            "memory_source".into(),
            JsonValue::String(request.memory_source.clone()),
        ),
        (
            "fps_unit".into(),
            JsonValue::String(request.fps_unit.clone()),
        ),
        (
            "cpu_unit".into(),
            JsonValue::String(request.cpu_unit.clone()),
        ),
        (
            "memory_unit".into(),
            JsonValue::String(request.memory_unit.clone()),
        ),
        (
            "samples".into(),
            JsonValue::String(
                serde_json::to_string(&samples.iter().map(sample_json).collect::<Vec<_>>())
                    .unwrap_or_default(),
            ),
        ),
        (
            "last_batch_hash".into(),
            JsonValue::String(batch_hash.into()),
        ),
    ]);
    if let Some(ended_at) = ended_at {
        fields.insert("ended_at".into(), JsonValue::Number(ended_at.into()));
    }
    fields
}

impl ResourceApi {
    pub(super) async fn publish_performance_samples(
        &self,
        request: Request<PerformanceSamplesRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_PERFORMANCE_PUBLISH)?;
        let owner = Self::data_identity(&caller)?;
        let now = unix_now_millis()?;
        let broker_now = unix_now()?;
        let (batch_hash, samples, first_at) = validate_request(&request, now)?;
        let token = series_resource_id(&owner, &request.series_id);
        let key = super::mutations::mutation_key(mutation)?.to_owned();
        let hash = super::mutations::request_hash(&[
            "game-performance-publish",
            &token,
            &batch_hash,
            &mutation.expected_revision.to_string(),
        ]);
        let _serial = self.mutations.lock.lock().await;
        if let Some(reply) = self.mutations.replay(&caller, &key, hash)? {
            return Ok(Response::new(ReportMutationReply {
                report_id: reply.resource_id,
                revision: reply.revision,
                outcome: reply.outcome,
                accepted_samples: samples.len() as u32,
                retained_from: first_at,
                retained_to: samples.last().map_or(first_at, |sample| sample.observed_at),
                ..Default::default()
            }));
        }
        let prior = self
            .data
            .owner_resource(&owner, DATASET, &token, broker_now);
        if let Some(prior) = prior.as_ref() {
            if prior
                .fields
                .get("last_batch_hash")
                .and_then(JsonValue::as_str)
                == Some(batch_hash.as_str())
            {
                let reply = super::mutations::reply(
                    &token,
                    prior.revision,
                    MutationOutcome::AlreadyApplied,
                );
                self.mutations.remember(&caller, &key, hash, &reply);
                return Ok(Response::new(ReportMutationReply {
                    report_id: token,
                    revision: prior.revision,
                    outcome: MutationOutcome::AlreadyApplied as i32,
                    accepted_samples: samples.len() as u32,
                    retained_from: first_at,
                    retained_to: samples.last().map_or(first_at, |sample| sample.observed_at),
                    ..Default::default()
                }));
            }
            if prior.fields.contains_key("ended_at") {
                return Err(Status::failed_precondition("performance series has ended"));
            }
            let expected = performance_fields(&request, &caller, &owner, &token, &[], None, "");
            if expected.iter().any(|(field, value)| {
                field != "samples"
                    && field != "last_batch_hash"
                    && prior.fields.get(field) != Some(value)
            }) {
                return Err(Status::failed_precondition(
                    "performance series metadata cannot change",
                ));
            }
            let old = sample_list(&prior.fields)?;
            let last_sequence = old.last().map_or(0, |sample| sample.sequence);
            let last_time = old.last().map_or(0, |sample| sample.observed_at);
            if samples
                .first()
                .is_none_or(|sample| sample.sequence <= last_sequence)
            {
                return Err(Status::aborted(
                    "sample sequence is duplicate or out of order",
                ));
            }
            if samples.first().is_some_and(|sample| {
                sample.observed_at.saturating_sub(last_time) < i64::from(request.sample_interval_ms)
            }) {
                return Err(Status::invalid_argument(
                    "sample rate exceeds the series interval",
                ));
            }
            if mutation.expected_revision != prior.revision {
                return Err(Status::aborted("performance series revision changed"));
            }
        } else if mutation.expected_revision != 0 {
            return Err(Status::aborted(
                "performance series does not exist at expected revision",
            ));
        }
        let throttle_key = owner_key(&owner);
        {
            let mut times = self
                .performance_batch_times
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            times.retain(|_, at| at.elapsed() < Duration::from_secs(60));
            if times
                .get(&throttle_key)
                .is_some_and(|at| at.elapsed() < BATCH_INTERVAL)
            {
                return Ok(Response::new(backpressure_reply(
                    token,
                    samples.len(),
                    1000,
                    first_at,
                    samples.last().map_or(first_at, |sample| sample.observed_at),
                )));
            }
            times.insert(throttle_key, Instant::now());
        }
        let mut all_samples = prior
            .as_ref()
            .map(|record| sample_list(&record.fields))
            .transpose()?
            .unwrap_or_default();
        all_samples.extend(samples.iter().cloned());
        if all_samples.len() > 10_000
            || serde_json::to_vec(&all_samples.iter().map(sample_json).collect::<Vec<_>>())
                .map_or(true, |bytes| bytes.len() > MAX_SERIES_BYTES)
        {
            return Ok(Response::new(backpressure_reply(
                token,
                samples.len(),
                1000,
                first_at,
                samples.last().map_or(first_at, |sample| sample.observed_at),
            )));
        }
        let (fresh, _session_commit) = self
            .providers
            .authorize_resource_commit(context, cap::GAME_PERFORMANCE_PUBLISH)
            .await?;
        if fresh.grant_revision != caller.grant_revision || Self::data_identity(&fresh)? != owner {
            return Err(Status::failed_precondition(
                "plugin identity or permissions changed; retry the performance batch",
            ));
        }
        let mut full_request = request.clone();
        let fields = performance_fields(
            &full_request,
            &caller,
            &owner,
            &token,
            &all_samples,
            None,
            &batch_hash,
        );
        let retained_from = all_samples
            .first()
            .map_or(first_at, |sample| sample.observed_at);
        let retained_to = all_samples
            .last()
            .map_or(first_at, |sample| sample.observed_at);
        full_request.samples = all_samples;
        let fields = fields.into_iter().collect();
        let receipt = match self.data.publish_replacing(
            Publication {
                owner,
                dataset_id: DATASET.into(),
                category: Some(CommonDataCategory::GamePerformanceHistory),
                class: SharingClass::Common,
                schema_id: DATASET.into(),
                schema_version: 1,
                fields,
                opaque_payload: Vec::new(),
                resource_id: token.clone(),
                observed_at: retained_to,
                sequence: full_request
                    .samples
                    .last()
                    .map_or(0, |sample| sample.sequence),
                source_observation_id: batch_hash,
                session_id: Some(caller.session_id.clone()),
                publisher: Some(PublisherMetadata {
                    declared_developer: caller.declared_developer.clone(),
                    origin: caller.origin.clone(),
                    package_version: caller.package_version.clone(),
                    package_digest: caller.package_digest.clone(),
                }),
                expires_at: None,
            },
            broker_now,
        ) {
            Ok(receipt) => receipt,
            Err(BrokerError::RecordTooLarge | BrokerError::QuotaExceeded) => {
                return Ok(Response::new(backpressure_reply(
                    token,
                    samples.len(),
                    1000,
                    first_at,
                    samples.last().map_or(first_at, |sample| sample.observed_at),
                )));
            }
            Err(error) => return Err(broker_status(error)),
        };
        let outcome = if receipt.duplicate {
            MutationOutcome::AlreadyApplied
        } else {
            MutationOutcome::Applied
        };
        let reply = super::mutations::reply(&token, receipt.revision, outcome);
        self.mutations.remember(&caller, &key, hash, &reply);
        Ok(Response::new(ReportMutationReply {
            report_id: token,
            revision: receipt.revision,
            outcome: outcome as i32,
            accepted_samples: samples.len() as u32,
            retained_from,
            retained_to,
            ..Default::default()
        }))
    }

    pub(super) async fn end_performance_series(
        &self,
        request: Request<PerformanceEndSeriesRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_PERFORMANCE_PUBLISH)?;
        let owner = Self::data_identity(&caller)?;
        if !valid_identifier(&request.series_id, MAX_SERIES_ID)
            || request.ended_at <= 0
            || request.ended_at > unix_now_millis()?.saturating_add(300_000)
        {
            return Err(Status::invalid_argument("invalid series end"));
        }
        let token = series_resource_id(&owner, &request.series_id);
        let key = super::mutations::mutation_key(mutation)?.to_owned();
        let hash = super::mutations::request_hash(&[
            "game-performance-end",
            &token,
            &request.ended_at.to_string(),
            &request.expected_last_sequence.to_string(),
            &mutation.expected_revision.to_string(),
        ]);
        let _serial = self.mutations.lock.lock().await;
        if let Some(reply) = self.mutations.replay(&caller, &key, hash)? {
            return Ok(Response::new(reply));
        }
        let broker_now = unix_now()?;
        let prior = self
            .data
            .owner_resource(&owner, DATASET, &token, broker_now)
            .ok_or_else(|| Status::not_found("performance series not found"))?;
        let samples = sample_list(&prior.fields)?;
        let last_sequence = samples.last().map_or(0, |sample| sample.sequence);
        if request.expected_last_sequence != last_sequence {
            return Err(Status::aborted("last sample sequence changed"));
        }
        if let Some(ended) = prior.fields.get("ended_at").and_then(JsonValue::as_i64) {
            if ended == request.ended_at {
                let reply = MutationReply {
                    resource_id: token,
                    revision: prior.revision,
                    outcome: MutationOutcome::AlreadyApplied as i32,
                };
                self.mutations.remember(&caller, &key, hash, &reply);
                return Ok(Response::new(reply));
            }
            return Err(Status::failed_precondition(
                "performance series already ended",
            ));
        }
        if request.ended_at < samples.last().map_or(0, |sample| sample.observed_at) {
            return Err(Status::invalid_argument(
                "series end precedes the last sample",
            ));
        }
        if prior.revision != mutation.expected_revision {
            return Err(Status::aborted("performance series revision changed"));
        }
        let (fresh, _session_commit) = self
            .providers
            .authorize_resource_commit(context, cap::GAME_PERFORMANCE_PUBLISH)
            .await?;
        if fresh.grant_revision != caller.grant_revision {
            return Err(Status::failed_precondition(
                "permissions changed; retry the series end",
            ));
        }
        let mut fields = prior.fields;
        fields.insert(
            "ended_at".into(),
            JsonValue::Number(request.ended_at.into()),
        );
        let receipt = self
            .data
            .publish_replacing(
                Publication {
                    owner: owner.clone(),
                    dataset_id: DATASET.into(),
                    category: Some(CommonDataCategory::GamePerformanceHistory),
                    class: SharingClass::Common,
                    schema_id: DATASET.into(),
                    schema_version: 1,
                    fields,
                    opaque_payload: Vec::new(),
                    resource_id: token.clone(),
                    observed_at: request.ended_at,
                    sequence: last_sequence.saturating_add(1),
                    source_observation_id: format!("end:{}", request.ended_at),
                    session_id: Some(caller.session_id.clone()),
                    publisher: Some(PublisherMetadata {
                        declared_developer: caller.declared_developer.clone(),
                        origin: caller.origin.clone(),
                        package_version: caller.package_version.clone(),
                        package_digest: caller.package_digest.clone(),
                    }),
                    expires_at: None,
                },
                broker_now,
            )
            .map_err(broker_status)?;
        let reply = MutationReply {
            resource_id: token,
            revision: receipt.revision,
            outcome: MutationOutcome::Applied as i32,
        };
        self.mutations.remember(&caller, &key, hash, &reply);
        Ok(Response::new(reply))
    }

    async fn list_performance(
        &self,
        request: Request<PerformanceListRequest>,
    ) -> Result<Response<PerformanceListReply>, Status> {
        let request = request.into_inner();
        let page = request
            .page
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("page is required"))?;
        let context = page
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_PERFORMANCE_READ)?;
        validate_projection(&request.projection_id)?;
        if !request.game_id.is_empty() && !valid_identifier(&request.game_id, MAX_SERIES_ID) {
            return Err(Status::invalid_argument("invalid game_id"));
        }
        if !request.computer_context_id.is_empty()
            && request.computer_context_id != HOST_COMPUTER_CONTEXT
        {
            return Err(Status::not_found("computer context not found"));
        }
        let identity = Self::data_identity(&caller)?;
        let scope = cursor_scope(
            &[
                &request.game_id,
                &request.computer_context_id,
                &request.projection_id,
            ],
            None,
        );
        let cursor = self.take_scoped_data_cursor(&page.cursor, &caller.session_id, Some(scope))?;
        let result = self
            .data
            .query_data(
                query_request(
                    identity,
                    page.common_data_categories.clone(),
                    Delivery::History,
                    cursor,
                    if page.page_size == 0 {
                        MAX_PAGE
                    } else {
                        (page.page_size as usize).min(MAX_PAGE)
                    },
                ),
                self.shared_authorizer.as_ref(),
                unix_now()?,
            )
            .map_err(broker_status)?;
        let mut records = Vec::new();
        for record in &result.records {
            let value = series(record)?;
            if (request.game_id.is_empty() || value.game_id == request.game_id)
                && (request.computer_context_id.is_empty()
                    || value.computer_context_id == request.computer_context_id)
            {
                records.push(value);
            }
        }
        let next_cursor = match result.next_cursor {
            Some(cursor) => {
                self.issue_scoped_data_cursor(&caller.session_id, cursor, Some(scope))?
            }
            None => String::new(),
        };
        Ok(Response::new(PerformanceListReply {
            series: records,
            next_cursor,
            revision: result.revision,
        }))
    }

    async fn get_performance(
        &self,
        request: Request<PerformanceGetRequest>,
    ) -> Result<Response<PerformanceGetReply>, Status> {
        let request = request.into_inner();
        if request.series_id.is_empty() || request.series_id.len() > 256 {
            return Err(Status::invalid_argument("series_id is required"));
        }
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_PERFORMANCE_READ)?;
        validate_projection(&request.projection_id)?;
        let result = self
            .data
            .query_data_resource(
                query_request(
                    Self::data_identity(&caller)?,
                    request.common_data_categories,
                    Delivery::Query,
                    None,
                    1,
                ),
                &request.series_id,
                self.shared_authorizer.as_ref(),
                unix_now()?,
            )
            .map_err(broker_status)?;
        let record = result
            .records
            .first()
            .ok_or_else(|| Status::not_found("performance series not found"))?;
        Ok(Response::new(PerformanceGetReply {
            series: Some(series(record)?),
        }))
    }

    async fn query_performance_series(
        &self,
        request: Request<PerformanceQuerySeriesRequest>,
    ) -> Result<Response<PerformanceQuerySeriesReply>, Status> {
        let request = request.into_inner();
        if request.series_id.is_empty()
            || request.series_id.len() > 256
            || request.from_time < 0
            || request.to_time < 0
            || (request.to_time != 0 && request.from_time > request.to_time)
            || request.resolution_ms > MAX_SAMPLE_WINDOW_MS
        {
            return Err(Status::invalid_argument("invalid performance query range"));
        }
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_PERFORMANCE_READ)?;
        let projection = format!(
            "{}:{}:{}:{}",
            request.series_id, request.from_time, request.to_time, request.resolution_ms
        );
        let scope = cursor_scope(&[&projection], request.common_data_categories.as_ref());
        let page = request.page.as_ref();
        let after = if let Some(page) = page {
            self.take_performance_cursor(&page.cursor, &caller, scope)?
        } else {
            0
        };
        let result = self
            .data
            .query_data_resource(
                query_request(
                    Self::data_identity(&caller)?,
                    request.common_data_categories.clone(),
                    Delivery::Query,
                    None,
                    1,
                ),
                &request.series_id,
                self.shared_authorizer.as_ref(),
                unix_now()?,
            )
            .map_err(broker_status)?;
        let record = result
            .records
            .first()
            .ok_or_else(|| Status::not_found("performance series not found"))?;
        let value = series(record)?;
        let page_size = page
            .filter(|page| page.page_size > 0)
            .map_or(MAX_PAGE, |page| (page.page_size as usize).min(MAX_PAGE));
        let retained = value
            .samples
            .iter()
            .filter(|sample| {
                sample.observed_at >= request.from_time
                    && (request.to_time == 0 || sample.observed_at <= request.to_time)
            })
            .cloned()
            .collect::<Vec<_>>();
        let dropped = missing_samples(&retained);
        let downsampled = request.resolution_ms > 0 && retained.len() > 1;
        let resolved = if request.resolution_ms == 0 {
            retained
        } else {
            let mut buckets = std::collections::BTreeMap::new();
            for sample in retained {
                let bucket = sample.observed_at.saturating_sub(request.from_time)
                    / i64::from(request.resolution_ms);
                buckets.entry(bucket).or_insert(sample);
            }
            buckets.into_values().collect::<Vec<_>>()
        };
        let raw = resolved
            .iter()
            .filter(|sample| sample.sequence > after)
            .take(page_size)
            .cloned()
            .collect::<Vec<_>>();
        let next_after = raw.last().map(|sample| sample.sequence);
        let has_more =
            next_after.is_some_and(|last| resolved.iter().any(|sample| sample.sequence > last));
        let next_cursor = if has_more {
            self.issue_performance_cursor(&caller, scope, next_after.unwrap(), result.revision)?
        } else {
            String::new()
        };
        let samples = raw;
        Ok(Response::new(PerformanceQuerySeriesReply {
            samples,
            next_cursor,
            coverage: if dropped > 0 {
                "gapped"
            } else if downsampled {
                "downsampled"
            } else {
                "complete"
            }
            .into(),
            dropped_samples: dropped,
        }))
    }

    async fn aggregate_performance(
        &self,
        request: Request<PerformanceAggregateRequest>,
    ) -> Result<Response<PerformanceAggregateReply>, Status> {
        let request = request.into_inner();
        if request.series_id.is_empty()
            || request.from_time < 0
            || request.to_time < 0
            || (request.to_time != 0 && request.from_time > request.to_time)
        {
            return Err(Status::invalid_argument(
                "invalid performance aggregate range",
            ));
        }
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_PERFORMANCE_READ)?;
        let result = self
            .data
            .query_data_resource(
                query_request(
                    Self::data_identity(&caller)?,
                    request.common_data_categories,
                    Delivery::Query,
                    None,
                    1,
                ),
                &request.series_id,
                self.shared_authorizer.as_ref(),
                unix_now()?,
            )
            .map_err(broker_status)?;
        let record = result
            .records
            .first()
            .ok_or_else(|| Status::not_found("performance series not found"))?;
        let value = series(record)?;
        let values = value
            .samples
            .iter()
            .filter(|sample| {
                sample.observed_at >= request.from_time
                    && (request.to_time == 0 || sample.observed_at <= request.to_time)
            })
            .map(|sample| match request.metric.as_str() {
                "fps" => Ok(sample.frames_per_second),
                "cpu_percent" => Ok(sample.cpu_percent),
                "memory_bytes" => Ok(sample.memory_bytes as f64),
                _ => Err(Status::invalid_argument(
                    "metric must be fps, cpu_percent, or memory_bytes",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if values.is_empty() {
            return Err(Status::not_found(
                "no retained samples in the requested range",
            ));
        }
        let aggregate = match request.aggregation.as_str() {
            "average" => values.iter().sum::<f64>() / values.len() as f64,
            "min" => values.iter().copied().fold(f64::INFINITY, f64::min),
            "max" => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            value if value.starts_with('p') => {
                let percentile = value[1..]
                    .parse::<u32>()
                    .ok()
                    .filter(|value| *value <= 100)
                    .ok_or_else(|| {
                        Status::invalid_argument("percentile aggregation must be p0 through p100")
                    })?;
                let mut ordered = values.clone();
                ordered.sort_by(f64::total_cmp);
                let index = if percentile == 0 {
                    0
                } else {
                    ((percentile as usize * ordered.len()).div_ceil(100)).saturating_sub(1)
                };
                ordered[index]
            }
            _ => {
                return Err(Status::invalid_argument(
                    "aggregation must be average, min, max, or p0 through p100",
                ));
            }
        };
        Ok(Response::new(PerformanceAggregateReply {
            metric: request.metric.clone(),
            aggregation: request.aggregation,
            value: aggregate,
            unit: match request.metric.as_str() {
                "fps" => "frames/second",
                "cpu_percent" => "percent",
                _ => "bytes",
            }
            .into(),
            sample_count: values.len() as u64,
            coverage: value.coverage,
            provenance: value.provenance,
        }))
    }

    async fn watch_performance(
        &self,
        request: Request<PerformanceWatchRequest>,
    ) -> Result<Response<WatchStream<PerformanceEvent>>, Status> {
        let request = request.into_inner();
        let context = request
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::GAME_PERFORMANCE_READ)?;
        validate_projection(&request.projection_id)?;
        if !request.game_id.is_empty() && !valid_identifier(&request.game_id, MAX_SERIES_ID) {
            return Err(Status::invalid_argument("invalid game_id"));
        }
        if !request.computer_context_id.is_empty()
            && request.computer_context_id != HOST_COMPUTER_CONTEXT
        {
            return Err(Status::not_found("computer context not found"));
        }
        let subscription = self
            .data
            .subscribe(
                query_request(
                    Self::data_identity(&caller)?,
                    request.common_data_categories,
                    Delivery::Live,
                    None,
                    MAX_PAGE,
                ),
                request.after_sequence,
                WATCH_QUEUE,
                self.shared_authorizer.clone(),
            )
            .map_err(broker_status)?;
        let slot = self.watch_slot(&caller.session_id)?;
        let providers = self.providers.clone();
        let expected_grant_revision = caller.grant_revision;
        let game_filter = request.game_id;
        let computer_filter = request.computer_context_id;
        let stream = futures_util::stream::unfold(
            Some((
                subscription,
                slot,
                providers,
                context,
                expected_grant_revision,
            )),
            move |state| {
                let game_filter = game_filter.clone();
                let computer_filter = computer_filter.clone();
                async move {
                    let (subscription, slot, providers, context, expected_grant_revision) = state?;
                    loop {
                        let received = tokio::time::timeout(
                            WATCH_RECHECK,
                            subscription.recv(unix_now().unwrap_or(0)),
                        )
                        .await;
                        match providers
                            .authorize_resource(Some(&context), cap::GAME_PERFORMANCE_READ)
                        {
                            Ok(caller) if caller.grant_revision == expected_grant_revision => {}
                            Ok(_) => {
                                return Some((
                                    Err(Status::failed_precondition(
                                        "permissions changed; watch again",
                                    )),
                                    None,
                                ));
                            }
                            Err(error) => return Some((Err(error), None)),
                        }
                        let record = match received {
                            Ok(Ok(Some(record))) => record,
                            Ok(Ok(None)) | Err(_) => continue,
                            Ok(Err(SubscriptionEnd::Revoked)) => {
                                return Some((
                                    Err(Status::failed_precondition(
                                        "sharing authorization changed; watch again",
                                    )),
                                    None,
                                ));
                            }
                            Ok(Err(SubscriptionEnd::FellBehind)) => {
                                return Some((
                                    Err(Status::resource_exhausted(
                                        "performance watch fell behind; list again",
                                    )),
                                    None,
                                ));
                            }
                        };
                        let value = match series(&record) {
                            Ok(value) => value,
                            Err(error) => return Some((Err(error), None)),
                        };
                        if (!game_filter.is_empty() && value.game_id != game_filter)
                            || (!computer_filter.is_empty()
                                && value.computer_context_id != computer_filter)
                        {
                            continue;
                        }
                        let event = PerformanceEvent {
                            sequence: record.revision(),
                            event_type: if value.ended_at > 0 {
                                "ended"
                            } else {
                                "published"
                            }
                            .into(),
                            series: Some(value),
                        };
                        return Some((
                            Ok(event),
                            Some((
                                subscription,
                                slot,
                                providers,
                                context,
                                expected_grant_revision,
                            )),
                        ));
                    }
                }
            },
        );
        Ok(Response::new(Box::pin(stream)))
    }

    fn take_performance_cursor(
        &self,
        token: &str,
        caller: &ResourceCaller,
        scope: [u8; 32],
    ) -> Result<u64, Status> {
        if token.is_empty() {
            return Ok(0);
        }
        let mut cursors = self
            .performance_cursors
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        cursors.retain(|_, value| value.expires_at > Instant::now());
        let entry = cursors
            .get(token)
            .ok_or_else(|| Status::failed_precondition("performance cursor is stale"))?;
        if entry.session_id != caller.session_id
            || entry.scope_hash != scope
            || entry.owner != Self::data_identity(caller)?
            || entry.broker_revision != self.data.revision()
            || entry.policy_revision
                != self
                    .shared_authorizer
                    .revision(&Self::data_identity(caller)?)
        {
            return Err(Status::failed_precondition(
                "performance cursor is stale; query again",
            ));
        }
        Ok(entry.after_sequence)
    }

    fn issue_performance_cursor(
        &self,
        caller: &ResourceCaller,
        scope: [u8; 32],
        after_sequence: u64,
        broker_revision: u64,
    ) -> Result<String, Status> {
        let owner = Self::data_identity(caller)?;
        let mut cursors = self
            .performance_cursors
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        cursors.retain(|_, value| value.expires_at > Instant::now());
        if cursors.len() >= MAX_CURSORS {
            if let Some(key) = cursors
                .iter()
                .min_by_key(|(_, value)| value.expires_at)
                .map(|(key, _)| key.clone())
            {
                cursors.remove(&key);
            }
        }
        let mut bytes = [0u8; 24];
        getrandom::fill(&mut bytes)
            .map_err(|_| Status::internal("cursor randomness unavailable"))?;
        let token = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        cursors.insert(
            token.clone(),
            CursorEntry {
                session_id: caller.session_id.clone(),
                owner: owner.clone(),
                scope_hash: scope,
                broker_revision,
                policy_revision: self.shared_authorizer.revision(&owner),
                after_sequence,
                expires_at: Instant::now() + CURSOR_TTL,
            },
        );
        Ok(token)
    }
}

#[tonic::async_trait]
impl game_performance_server::GamePerformance for ResourceApi {
    type WatchStream = WatchStream<PerformanceEvent>;
    async fn list(
        &self,
        request: Request<PerformanceListRequest>,
    ) -> Result<Response<PerformanceListReply>, Status> {
        self.list_performance(request).await
    }
    async fn get(
        &self,
        request: Request<PerformanceGetRequest>,
    ) -> Result<Response<PerformanceGetReply>, Status> {
        self.get_performance(request).await
    }
    async fn watch(
        &self,
        request: Request<PerformanceWatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        self.watch_performance(request).await
    }
    async fn query_series(
        &self,
        request: Request<PerformanceQuerySeriesRequest>,
    ) -> Result<Response<PerformanceQuerySeriesReply>, Status> {
        self.query_performance_series(request).await
    }
    async fn aggregate(
        &self,
        request: Request<PerformanceAggregateRequest>,
    ) -> Result<Response<PerformanceAggregateReply>, Status> {
        self.aggregate_performance(request).await
    }
}

pub(super) fn validate_request(
    request: &PerformanceSamplesRequest,
    now: i64,
) -> Result<(String, Vec<PerformanceSample>, i64), Status> {
    if !valid_identifier(&request.series_id, MAX_SERIES_ID)
        || !valid_identifier(&request.game_id, MAX_SERIES_ID)
        || !request.computer_id.is_empty()
        || (!request.runtime_id.is_empty() && !valid_identifier(&request.runtime_id, MAX_SERIES_ID))
        || !valid_text(&request.runtime_recipe_revision, false)
        || !valid_text(&request.source, true)
        || !valid_text(&request.tool_name, true)
        || !valid_text(&request.tool_version, true)
        || !valid_text(&request.memory_source, true)
        || request.fps_unit != "frames/second"
        || request.cpu_unit != "percent"
        || request.memory_unit != "bytes"
        || !(MIN_SAMPLE_INTERVAL_MS..=MAX_SAMPLE_INTERVAL_MS).contains(&request.sample_interval_ms)
        || request.samples.is_empty()
        || request.samples.len() > MAX_BATCH_SAMPLES
        || request.encoded_len() > MAX_BATCH_BYTES
    {
        return Err(Status::invalid_argument(
            "invalid performance series or batch",
        ));
    }
    if PerformanceCpuScope::try_from(request.cpu_scope)
        .ok()
        .is_none_or(|scope| scope == PerformanceCpuScope::Unspecified)
        || PerformanceCpuNormalization::try_from(request.cpu_normalization)
            .ok()
            .is_none_or(|value| value == PerformanceCpuNormalization::Unspecified)
        || PerformanceMemoryKind::try_from(request.memory_kind)
            .ok()
            .is_none_or(|value| value == PerformanceMemoryKind::Unspecified)
    {
        return Err(Status::invalid_argument(
            "performance units and scope are required",
        ));
    }
    let context = request
        .mutation
        .as_ref()
        .and_then(|mutation| mutation.context.as_ref())
        .ok_or_else(|| Status::invalid_argument("performance mutation context is required"))?;
    let target = context
        .target_game
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("performance samples need a selected game"))?;
    if target.game_id != request.game_id {
        return Err(Status::permission_denied(
            "performance game does not match the host-selected game",
        ));
    }
    if target.installation_id.is_empty() || target.installation_id.len() > MAX_SERIES_ID {
        return Err(Status::failed_precondition(
            "performance samples need a selected installation",
        ));
    }
    let mut previous_sequence = 0;
    let mut previous_time = 0;
    let mut first_time = 0;
    let mut samples = Vec::with_capacity(request.samples.len());
    for sample in &request.samples {
        if sample.sequence == 0
            || sample.sequence <= previous_sequence
            || sample.observed_at <= 0
            || sample.observed_at > now.saturating_add(300_000)
            || (previous_time > 0
                && sample.observed_at.saturating_sub(previous_time)
                    < i64::from(request.sample_interval_ms))
            || !sample.frames_per_second.is_finite()
            || !(0.0..=MAX_FPS).contains(&sample.frames_per_second)
            || !sample.cpu_percent.is_finite()
            || sample.cpu_percent < 0.0
            || (request.cpu_normalization
                == PerformanceCpuNormalization::WholeMachinePercent as i32
                && sample.cpu_percent > 100.0)
            || (request.cpu_normalization == PerformanceCpuNormalization::PerCorePercent as i32
                && sample.cpu_percent > MAX_FPS)
            || sample.sampling_window_ms > MAX_SAMPLE_WINDOW_MS
            || (sample.fps_method != PerformanceFpsMethod::Instantaneous as i32
                && sample.sampling_window_ms < request.sample_interval_ms)
            || (sample.fps_method != PerformanceFpsMethod::Instantaneous as i32
                && sample.sampling_window_ms == 0)
            || (sample.fps_method == PerformanceFpsMethod::Instantaneous as i32
                && sample.sampling_window_ms != 0)
            || !matches!(
                PerformanceFpsMethod::try_from(sample.fps_method),
                Ok(PerformanceFpsMethod::Instantaneous)
                    | Ok(PerformanceFpsMethod::FrameCountWindow)
                    | Ok(PerformanceFpsMethod::FrameTimeDerived)
            )
            || !matches!(
                PerformanceFpsStatistic::try_from(sample.fps_statistic),
                Ok(PerformanceFpsStatistic::Instantaneous)
                    | Ok(PerformanceFpsStatistic::Average)
                    | Ok(PerformanceFpsStatistic::Minimum)
                    | Ok(PerformanceFpsStatistic::Maximum)
                    | Ok(PerformanceFpsStatistic::Percentile)
            )
            || !matches!(
                PerformanceMeasurementQuality::try_from(sample.quality),
                Ok(PerformanceMeasurementQuality::Measured)
                    | Ok(PerformanceMeasurementQuality::Estimated)
            )
        {
            return Err(Status::invalid_argument("invalid performance sample"));
        }
        if sample.fps_method == PerformanceFpsMethod::Instantaneous as i32
            && sample.fps_statistic != PerformanceFpsStatistic::Instantaneous as i32
        {
            return Err(Status::invalid_argument(
                "instantaneous FPS must use instantaneous statistic",
            ));
        }
        match sample.fps_statistic {
            value if value == PerformanceFpsStatistic::Percentile as i32 => {
                if sample.fps_percentile.is_none_or(|percentile| {
                    !percentile.is_finite() || !(0.0..=100.0).contains(&percentile)
                }) {
                    return Err(Status::invalid_argument(
                        "FPS percentile must be between 0 and 100",
                    ));
                }
            }
            _ if sample.fps_percentile.is_some() => {
                return Err(Status::invalid_argument(
                    "FPS percentile is only valid for a percentile statistic",
                ));
            }
            _ => {}
        }
        if sample.frames_per_second_min.is_some_and(|value| {
            !value.is_finite() || !(0.0..=sample.frames_per_second).contains(&value)
        }) || sample.frames_per_second_max.is_some_and(|value| {
            !value.is_finite() || !(sample.frames_per_second..=MAX_FPS).contains(&value)
        }) {
            return Err(Status::invalid_argument("FPS range is inconsistent"));
        }
        if (sample.fps_statistic == PerformanceFpsStatistic::Minimum as i32
            && sample
                .frames_per_second_min
                .is_some_and(|value| value != sample.frames_per_second))
            || (sample.fps_statistic == PerformanceFpsStatistic::Maximum as i32
                && sample
                    .frames_per_second_max
                    .is_some_and(|value| value != sample.frames_per_second))
        {
            return Err(Status::invalid_argument(
                "FPS statistic conflicts with its value",
            ));
        }
        if first_time == 0 {
            first_time = sample.observed_at;
        }
        previous_sequence = sample.sequence;
        previous_time = sample.observed_at;
        samples.push(sample.clone());
    }
    let mut canonical = request.clone();
    canonical.mutation = None;
    let digest = Sha256::digest(canonical.encode_to_vec());
    let batch_hash = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok((batch_hash, samples, first_time))
}

fn backpressure_reply(
    report_id: String,
    dropped: usize,
    retry_after_ms: u32,
    retained_from: i64,
    retained_to: i64,
) -> ReportMutationReply {
    ReportMutationReply {
        report_id,
        revision: 0,
        outcome: MutationOutcome::Backpressure as i32,
        accepted_samples: 0,
        dropped_samples: dropped.min(u32::MAX as usize) as u32,
        retry_after_ms,
        retained_from,
        retained_to,
    }
}
