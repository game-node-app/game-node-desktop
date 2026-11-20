//! Plugin-produced storage diagnostics. Core validates and routes reports but never runs a
//! benchmark or SMART utility. Reports are session-retained by the common data broker and every
//! cross-plugin read is projected by the existing sensitive-data policy.

use super::super::shared_data::{MAX_PAGE_SIZE, Publication, QueryRequest};
use super::super::sharing_policy::{Delivery, SharingClass};
use super::*;
use futures_util::Stream;
use std::collections::BTreeMap;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;
use tonic::{Request, Response, Status};

const BENCHMARK_DATASET: &str = "storage.diagnostics.benchmark.v1";
const SMART_DATASET: &str = "storage.diagnostics.smart.v1";
const MAX_DIAGNOSTIC_FIELDS: usize = 256;
const MAX_DIAGNOSTIC_TEXT: usize = 512;
const MAX_PAGE: usize = 200;
const WATCH_QUEUE: usize = 32;
const WATCH_RECHECK: Duration = Duration::from_secs(5);

pub const SERVED: &[Served] = &[
    Served::new(
        "List",
        cap::STORAGE_DIAGNOSTICS_READ,
        true,
        "sensitive report projection",
    ),
    Served::new(
        "Get",
        cap::STORAGE_DIAGNOSTICS_READ,
        false,
        "sensitive report projection",
    ),
    Served::new(
        "Watch",
        cap::STORAGE_DIAGNOSTICS_READ,
        false,
        "sensitive report projection",
    ),
];

type WatchStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

fn dataset(projection: &str) -> Result<(&'static str, &'static str), Status> {
    match projection {
        "benchmark.v1" => Ok((BENCHMARK_DATASET, "benchmark")),
        "smart.v1" => Ok((SMART_DATASET, "smart")),
        _ => Err(Status::invalid_argument("unknown diagnostics projection")),
    }
}

fn plain(value: &str, max: usize, required: bool) -> Result<(), Status> {
    if value.len() > max
        || (required && value.trim().is_empty())
        || value.chars().any(char::is_control)
    {
        return Err(Status::invalid_argument("invalid diagnostic text field"));
    }
    Ok(())
}

fn field_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
}

fn state_name(value: i32) -> Result<&'static str, Status> {
    match DiagnosticValueState::try_from(value) {
        Ok(DiagnosticValueState::Available) => Ok("available"),
        Ok(DiagnosticValueState::Unavailable) => Ok("unavailable"),
        Ok(DiagnosticValueState::Unsupported) => Ok("unsupported"),
        Ok(DiagnosticValueState::AccessDenied) => Ok("access_denied"),
        Ok(DiagnosticValueState::Error) => Ok("error"),
        _ => Err(Status::invalid_argument(
            "diagnostic value state is required",
        )),
    }
}

fn flatten_struct(
    prefix: &str,
    value: &prost_types::Struct,
    fields: &mut serde_json::Map<String, JsonValue>,
    depth: usize,
) -> Result<(), Status> {
    if depth > 8 {
        return Err(Status::invalid_argument(
            "diagnostic conditions are too deep",
        ));
    }
    for (key, value) in &value.fields {
        if !field_name(key) || fields.len() >= MAX_DIAGNOSTIC_FIELDS {
            return Err(Status::invalid_argument(
                "invalid diagnostic condition field",
            ));
        }
        let name = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value.kind.as_ref() {
            Some(prost_types::value::Kind::StructValue(nested)) => {
                flatten_struct(&name, nested, fields, depth + 1)?;
            }
            Some(prost_types::value::Kind::ListValue(_)) => {
                let one = BTreeMap::from([(key.clone(), value.clone())]);
                let mut converted = super::struct_to_json(&prost_types::Struct { fields: one })?;
                fields.insert(name, converted.remove(key).unwrap_or(JsonValue::Null));
            }
            _ => {
                let one = BTreeMap::from([(key.clone(), value.clone())]);
                let mut converted = super::struct_to_json(&prost_types::Struct { fields: one })?;
                fields.insert(name, converted.remove(key).unwrap_or(JsonValue::Null));
            }
        }
    }
    Ok(())
}

fn checked_time(observed_at: i64, now: i64) -> Result<(), Status> {
    if observed_at <= 0 || observed_at > now.saturating_add(300) {
        return Err(Status::invalid_argument(
            "diagnostic observation time is outside the supported range",
        ));
    }
    Ok(())
}

fn benchmark_fields(
    request: &BenchmarkReportRequest,
    now: i64,
) -> Result<serde_json::Map<String, JsonValue>, Status> {
    plain(&request.tool_name, MAX_DIAGNOSTIC_TEXT, true)?;
    plain(&request.tool_version, MAX_DIAGNOSTIC_TEXT, true)?;
    plain(&request.units, 128, true)?;
    checked_time(request.observed_at, now)?;
    if request.measurements.len() > MAX_DIAGNOSTIC_FIELDS
        || request.measurement_states.len() > MAX_DIAGNOSTIC_FIELDS
        || request.measurement_units.len() > MAX_DIAGNOSTIC_FIELDS
    {
        return Err(Status::resource_exhausted(
            "diagnostic field limit exceeded",
        ));
    }
    let mut fields = serde_json::Map::new();
    fields.insert(
        "tool_name".into(),
        JsonValue::String(request.tool_name.clone()),
    );
    fields.insert(
        "tool_version".into(),
        JsonValue::String(request.tool_version.clone()),
    );
    fields.insert("units".into(), JsonValue::String(request.units.clone()));
    for (key, value) in &request.measurements {
        if !field_name(key) || !value.is_finite() || *value < 0.0 {
            return Err(Status::invalid_argument("invalid benchmark measurement"));
        }
        let state = request
            .measurement_states
            .get(key)
            .copied()
            .unwrap_or(DiagnosticValueState::Available as i32);
        if state != DiagnosticValueState::Available as i32 {
            return Err(Status::invalid_argument(
                "available measurement has a non-available state",
            ));
        }
        let unit = request.measurement_units.get(key).unwrap_or(&request.units);
        plain(unit, 128, true)?;
        fields.insert(format!("measurements.{key}"), JsonValue::from(*value));
        fields.insert(
            format!("measurement_units.{key}"),
            JsonValue::String(unit.clone()),
        );
        fields.insert(
            format!("measurement_states.{key}"),
            JsonValue::String("available".into()),
        );
    }
    for (key, raw_state) in &request.measurement_states {
        if !field_name(key) {
            return Err(Status::invalid_argument("invalid measurement state field"));
        }
        if request.measurements.contains_key(key) {
            if *raw_state != DiagnosticValueState::Available as i32 {
                return Err(Status::invalid_argument(
                    "available measurement has a non-available state",
                ));
            }
            continue;
        }
        if *raw_state == DiagnosticValueState::Available as i32 {
            return Err(Status::invalid_argument(
                "available measurement state requires a value",
            ));
        }
        let state = state_name(*raw_state)?;
        fields.insert(
            format!("measurement_states.{key}"),
            JsonValue::String(state.into()),
        );
    }
    if request.measurement_units.keys().any(|key| {
        !field_name(key)
            || (!request.measurements.contains_key(key)
                && !request.measurement_states.contains_key(key))
    }) {
        return Err(Status::invalid_argument(
            "measurement units do not match a reported measurement",
        ));
    }
    if let Some(conditions) = request.conditions.as_ref() {
        flatten_struct("conditions", conditions, &mut fields, 0)?;
    }
    if fields.len() > MAX_DIAGNOSTIC_FIELDS {
        return Err(Status::resource_exhausted(
            "diagnostic field limit exceeded",
        ));
    }
    Ok(fields)
}

fn smart_fields(
    request: &SmartReportRequest,
    now: i64,
) -> Result<serde_json::Map<String, JsonValue>, Status> {
    plain(&request.device_model, MAX_DIAGNOSTIC_TEXT, false)?;
    plain(&request.firmware, MAX_DIAGNOSTIC_TEXT, false)?;
    checked_time(request.observed_at, now)?;
    if request.attributes.len() > MAX_DIAGNOSTIC_FIELDS
        || request.attribute_states.len() > MAX_DIAGNOSTIC_FIELDS
        || request.attribute_units.len() > MAX_DIAGNOSTIC_FIELDS
    {
        return Err(Status::resource_exhausted(
            "diagnostic field limit exceeded",
        ));
    }
    let mut fields = serde_json::Map::new();
    fields.insert(
        "device_model".into(),
        JsonValue::String(request.device_model.clone()),
    );
    fields.insert(
        "firmware".into(),
        JsonValue::String(request.firmware.clone()),
    );
    for (key, value) in &request.attributes {
        if !field_name(key)
            || value.len() > MAX_DIAGNOSTIC_TEXT
            || value.chars().any(char::is_control)
        {
            return Err(Status::invalid_argument("invalid SMART attribute"));
        }
        let state = request
            .attribute_states
            .get(key)
            .copied()
            .unwrap_or(DiagnosticValueState::Available as i32);
        if state != DiagnosticValueState::Available as i32 {
            return Err(Status::invalid_argument(
                "available attribute has a non-available state",
            ));
        }
        let unit = request
            .attribute_units
            .get(key)
            .map(String::as_str)
            .unwrap_or("");
        plain(unit, 128, false)?;
        fields.insert(
            format!("attributes.{key}"),
            JsonValue::String(value.clone()),
        );
        fields.insert(
            format!("attribute_units.{key}"),
            JsonValue::String(unit.into()),
        );
        fields.insert(
            format!("attribute_states.{key}"),
            JsonValue::String("available".into()),
        );
    }
    for (key, raw_state) in &request.attribute_states {
        if !field_name(key) {
            return Err(Status::invalid_argument(
                "invalid SMART attribute state field",
            ));
        }
        if request.attributes.contains_key(key) {
            if *raw_state != DiagnosticValueState::Available as i32 {
                return Err(Status::invalid_argument(
                    "available attribute has a non-available state",
                ));
            }
            continue;
        }
        if *raw_state == DiagnosticValueState::Available as i32 {
            return Err(Status::invalid_argument(
                "available attribute state requires a value",
            ));
        }
        fields.insert(
            format!("attribute_states.{key}"),
            JsonValue::String(state_name(*raw_state)?.into()),
        );
        if let Some(unit) = request.attribute_units.get(key) {
            plain(unit, 128, false)?;
            fields.insert(
                format!("attribute_units.{key}"),
                JsonValue::String(unit.clone()),
            );
        }
    }
    if request.attribute_units.keys().any(|key| {
        !field_name(key)
            || (!request.attributes.contains_key(key)
                && !request.attribute_states.contains_key(key))
    }) {
        return Err(Status::invalid_argument(
            "SMART attribute units do not match a reported attribute",
        ));
    }
    if let Some(conditions) = request.conditions.as_ref() {
        flatten_struct("conditions", conditions, &mut fields, 0)?;
    }
    if fields.len() > MAX_DIAGNOSTIC_FIELDS {
        return Err(Status::resource_exhausted(
            "diagnostic field limit exceeded",
        ));
    }
    Ok(fields)
}

fn report(record: &super::super::shared_data::ProjectedRecord, kind: &str) -> DiagnosticReport {
    let mut fields = record.fields().clone();
    let volume_identity_token = fields
        .remove("volume_identity_token")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    DiagnosticReport {
        report_id: record.record_id().to_owned(),
        report_kind: kind.to_owned(),
        owner_plugin_id: record.provenance().source.plugin_id.clone(),
        volume_identity_token,
        schema_id: record.schema().0.to_owned(),
        schema_version: record.schema().1,
        typed_measurements: Some(super::json_to_struct(&fields)),
        approved_extension: None,
        observed_at: record.observed_at(),
        revision: record.revision(),
    }
}

fn report_matches(
    record: &super::super::shared_data::ProjectedRecord,
    owner: &str,
    volume: &str,
) -> bool {
    (owner.is_empty() || record.provenance().source.plugin_id == owner)
        && (volume.is_empty() || record.resource_id() == volume)
}

fn broker_status(error: super::super::shared_data::BrokerError) -> Status {
    match error {
        super::super::shared_data::BrokerError::InvalidSelector => {
            Status::invalid_argument("invalid diagnostics query")
        }
        super::super::shared_data::BrokerError::StaleCursor
        | super::super::shared_data::BrokerError::StaleSubscription => {
            Status::failed_precondition("diagnostics changed; query again")
        }
        super::super::shared_data::BrokerError::AuthorizationChanged
        | super::super::shared_data::BrokerError::Expired => {
            Status::failed_precondition("diagnostics authorization changed")
        }
        super::super::shared_data::BrokerError::RecordTooLarge
        | super::super::shared_data::BrokerError::QuotaExceeded => {
            Status::resource_exhausted("diagnostics capacity exceeded")
        }
        _ => Status::invalid_argument("invalid diagnostics report"),
    }
}

impl ResourceApi {
    async fn volume_token(
        &self,
        caller: &ResourceCaller,
        handle: String,
    ) -> Result<String, Status> {
        if handle.is_empty() || handle.len() > 16 * 1024 {
            return Err(Status::invalid_argument("volume_handle is required"));
        }
        let files = self.files.clone();
        let feed = self.library.clone();
        let session = caller.session_id.clone();
        self.in_namespace(caller, move |namespace| {
            let opened = super::files::open_handle(&files, &feed, namespace, &session, &handle, 0)?;
            let info = gamenode_platform::volume::volume_info(Path::new(&opened.approval.root))
                .ok_or_else(|| Status::failed_precondition("volume identity is unavailable"))?;
            if info.volume_identity_token.is_empty() {
                return Err(Status::failed_precondition(
                    "volume identity is unavailable",
                ));
            }
            Ok(info.volume_identity_token)
        })
        .await
    }

    async fn publish_diagnostic(
        &self,
        mutation: Option<MutationContext>,
        volume_handle: String,
        projection: &'static str,
        fields: serde_json::Map<String, JsonValue>,
        observed_at: i64,
        hash_material: String,
    ) -> Result<Response<ReportMutationReply>, Status> {
        let mutation = mutation.ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_DIAGNOSTICS_PUBLISH)?;
        let owner = ResourceApi::data_identity(&caller)?;
        let (dataset_id, _) = dataset(projection)?;
        let key = super::mutations::mutation_key(&mutation)?.to_owned();
        let expected_revision = mutation.expected_revision.to_string();
        let hash = super::mutations::request_hash(&[
            "storage-diagnostics",
            projection,
            &volume_handle,
            &observed_at.to_string(),
            &expected_revision,
            &hash_material,
        ]);
        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, &key, hash)? {
            return Ok(Response::new(ReportMutationReply {
                report_id: original.resource_id,
                revision: original.revision,
                outcome: original.outcome,
            }));
        }
        let fresh = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_DIAGNOSTICS_PUBLISH)?;
        if fresh.grant_revision != caller.grant_revision {
            return Err(Status::failed_precondition(
                "permissions changed; retry the report",
            ));
        }
        let volume = self.volume_token(&caller, volume_handle).await?;
        let mut fields = fields;
        fields.insert(
            "volume_identity_token".into(),
            JsonValue::String(volume.clone()),
        );
        let receipt = self
            .data
            .publish(
                Publication {
                    owner,
                    dataset_id: dataset_id.into(),
                    category: None,
                    class: SharingClass::Sensitive,
                    schema_id: dataset_id.into(),
                    schema_version: 1,
                    fields,
                    opaque_payload: Vec::new(),
                    resource_id: volume,
                    observed_at,
                    sequence: 0,
                    source_observation_id: String::new(),
                    session_id: Some(caller.session_id.clone()),
                    // The broker's package provenance is reserved for sequenced source-observation
                    // streams. Diagnostics reports use the authenticated owner plus their typed
                    // tool/version fields; they do not claim an observation sequence.
                    publisher: None,
                    expires_at: None,
                },
                super::unix_now()?,
            )
            .map_err(broker_status)?;
        let outcome = if receipt.duplicate {
            MutationOutcome::AlreadyApplied
        } else {
            MutationOutcome::Applied
        };
        let stored = super::mutations::reply(&receipt.record_id, receipt.revision, outcome);
        self.mutations.remember(&caller, &key, hash, &stored);
        Ok(Response::new(ReportMutationReply {
            report_id: receipt.record_id,
            revision: receipt.revision,
            outcome: outcome as i32,
        }))
    }

    pub(super) async fn publish_benchmark(
        &self,
        request: Request<BenchmarkReportRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        let request = request.into_inner();
        let now = super::unix_now()?;
        let fields = benchmark_fields(&request, now)?;
        let material = serde_json::to_string(&fields)
            .map_err(|_| Status::invalid_argument("invalid benchmark report"))?;
        self.publish_diagnostic(
            request.mutation,
            request.volume_handle,
            "benchmark.v1",
            fields,
            request.observed_at,
            material,
        )
        .await
    }

    pub(super) async fn publish_smart(
        &self,
        request: Request<SmartReportRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        let request = request.into_inner();
        let now = super::unix_now()?;
        let fields = smart_fields(&request, now)?;
        let material = serde_json::to_string(&fields)
            .map_err(|_| Status::invalid_argument("invalid SMART report"))?;
        self.publish_diagnostic(
            request.mutation,
            request.volume_handle,
            "smart.v1",
            fields,
            request.observed_at,
            material,
        )
        .await
    }

    async fn list_diagnostics(
        &self,
        request: Request<DiagnosticListRequest>,
    ) -> Result<Response<DiagnosticListReply>, Status> {
        let request = request.into_inner();
        let page = request
            .page
            .ok_or_else(|| Status::invalid_argument("page is required"))?;
        let context = page
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_DIAGNOSTICS_READ)?;
        let identity = ResourceApi::data_identity(&caller)?;
        if request.volume_identity_token.len() > 512 || request.owner_plugin_id.len() > 128 {
            return Err(Status::invalid_argument("invalid diagnostics selector"));
        }
        let (dataset_id, kind) = dataset(&request.projection_id)?;
        let cursor = self.take_data_cursor(&page.cursor, &caller.session_id)?;
        let size = if page.page_size == 0 {
            DEFAULT_PAGE_SIZE
        } else {
            (page.page_size as usize).min(MAX_PAGE)
        };
        let result = self
            .data
            .query_data(
                QueryRequest {
                    caller: identity,
                    dataset_id: dataset_id.into(),
                    record_id: None,
                    category: None,
                    selector: None,
                    delivery: Delivery::History,
                    page_size: size.min(MAX_PAGE_SIZE),
                    cursor,
                },
                self.shared_authorizer.as_ref(),
                super::unix_now()?,
            )
            .map_err(broker_status)?;
        let reports = result
            .records
            .iter()
            .filter(|record| {
                report_matches(
                    record,
                    &request.owner_plugin_id,
                    &request.volume_identity_token,
                )
            })
            .map(|record| report(record, kind))
            .collect();
        let next_cursor = match result.next_cursor {
            Some(cursor) => self.issue_data_cursor(&caller.session_id, cursor)?,
            None => String::new(),
        };
        Ok(Response::new(DiagnosticListReply {
            reports,
            next_cursor,
            revision: result.revision,
        }))
    }

    async fn get_diagnostic(
        &self,
        request: Request<DiagnosticGetRequest>,
    ) -> Result<Response<DiagnosticGetReply>, Status> {
        let request = request.into_inner();
        if request.report_id.is_empty() || request.report_id.len() > 256 {
            return Err(Status::invalid_argument("report_id is required"));
        }
        let context = request
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_DIAGNOSTICS_READ)?;
        let identity = ResourceApi::data_identity(&caller)?;
        let (dataset_id, kind) = dataset(&request.projection_id)?;
        let result = self
            .data
            .query_data(
                QueryRequest {
                    caller: identity,
                    dataset_id: dataset_id.into(),
                    record_id: Some(request.report_id.clone()),
                    category: None,
                    selector: None,
                    delivery: Delivery::Query,
                    page_size: 1,
                    cursor: None,
                },
                self.shared_authorizer.as_ref(),
                super::unix_now()?,
            )
            .map_err(broker_status)?;
        let record = result
            .records
            .iter()
            .find(|record| record.record_id() == request.report_id)
            .ok_or_else(|| Status::not_found("diagnostic report not found"))?;
        Ok(Response::new(DiagnosticGetReply {
            report: Some(report(record, kind)),
        }))
    }

    async fn watch_diagnostics(
        &self,
        request: Request<DiagnosticWatchRequest>,
    ) -> Result<Response<WatchStream<DiagnosticEvent>>, Status> {
        let request = request.into_inner();
        let context = request
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        if request.owner_plugin_id.len() > 128 || request.volume_identity_token.len() > 512 {
            return Err(Status::invalid_argument("invalid diagnostics selector"));
        }
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_DIAGNOSTICS_READ)?;
        let identity = ResourceApi::data_identity(&caller)?;
        let (dataset_id, kind) = dataset(&request.projection_id)?;
        let subscription = self
            .data
            .subscribe(
                QueryRequest {
                    caller: identity,
                    dataset_id: dataset_id.into(),
                    record_id: None,
                    category: None,
                    selector: None,
                    delivery: Delivery::Live,
                    page_size: DEFAULT_PAGE_SIZE,
                    cursor: None,
                },
                request.after_revision,
                WATCH_QUEUE,
                self.shared_authorizer.clone(),
            )
            .map_err(broker_status)?;
        let slot = self.watch_slot(&caller.session_id)?;
        let providers = self.providers.clone();
        let grant_revision = caller.grant_revision;
        let owner = request.owner_plugin_id;
        let volume = request.volume_identity_token;
        let stream = futures_util::stream::unfold(
            Some((subscription, slot, providers, context, owner, volume)),
            move |state| async move {
                let (subscription, slot, providers, context, owner, volume) = state?;
                loop {
                    let received = tokio::time::timeout(
                        WATCH_RECHECK,
                        subscription.recv(super::unix_now().unwrap_or(0)),
                    )
                    .await;
                    match providers
                        .authorize_resource(Some(&context), cap::STORAGE_DIAGNOSTICS_READ)
                    {
                        Ok(active) if active.grant_revision == grant_revision => {}
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
                    let event = match received {
                        Ok(Ok(Some(record))) => record,
                        Ok(Ok(None)) | Err(_) => continue,
                        Ok(Err(SubscriptionEnd::Revoked)) => {
                            return Some((
                                Err(Status::failed_precondition(
                                    "diagnostics sharing changed; watch again",
                                )),
                                None,
                            ));
                        }
                        Ok(Err(SubscriptionEnd::FellBehind)) => {
                            return Some((
                                Err(Status::resource_exhausted(
                                    "diagnostics watch fell behind; list again",
                                )),
                                None,
                            ));
                        }
                    };
                    if !report_matches(&event, &owner, &volume) {
                        continue;
                    }
                    let value = DiagnosticEvent {
                        revision: event.revision(),
                        event_type: "published".into(),
                        report: Some(report(&event, kind)),
                    };
                    return Some((
                        Ok(value),
                        Some((subscription, slot, providers, context, owner, volume)),
                    ));
                }
            },
        );
        Ok(Response::new(Box::pin(stream)))
    }
}

#[tonic::async_trait]
impl storage_diagnostics_server::StorageDiagnostics for ResourceApi {
    type WatchStream = WatchStream<DiagnosticEvent>;
    async fn list(
        &self,
        request: Request<DiagnosticListRequest>,
    ) -> Result<Response<DiagnosticListReply>, Status> {
        self.list_diagnostics(request).await
    }
    async fn get(
        &self,
        request: Request<DiagnosticGetRequest>,
    ) -> Result<Response<DiagnosticGetReply>, Status> {
        self.get_diagnostic(request).await
    }
    async fn watch(
        &self,
        request: Request<DiagnosticWatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        self.watch_diagnostics(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::sharing_policy::{AccessDecision, AccessRequest, Authorizer};
    use std::collections::HashMap;

    struct ToolNameOnly;

    impl Authorizer for ToolNameOnly {
        fn revision(&self, _recipient: &crate::plugin_host::grants::PluginIdentity) -> u64 {
            3
        }

        fn decide(&self, request: &AccessRequest<'_>) -> AccessDecision {
            let allowed =
                request.dataset_id == BENCHMARK_DATASET && request.fields.contains("tool_name");
            AccessDecision {
                allowed,
                fields: if allowed {
                    ["tool_name".to_owned()].into_iter().collect()
                } else {
                    Default::default()
                },
                history: allowed && request.delivery == Delivery::History,
                live: allowed && request.delivery == Delivery::Live,
                revision: 3,
            }
        }
    }

    fn benchmark() -> BenchmarkReportRequest {
        BenchmarkReportRequest {
            tool_name: "FixtureBench".into(),
            tool_version: "2.1".into(),
            units: "bytes_per_second".into(),
            measurements: HashMap::from([("sequential_read".into(), 10_000.0)]),
            measurement_units: HashMap::new(),
            measurement_states: HashMap::from([
                (
                    "random_read_iops".into(),
                    DiagnosticValueState::Unsupported as i32,
                ),
                (
                    "sequential_read".into(),
                    DiagnosticValueState::Available as i32,
                ),
            ]),
            observed_at: 1_000_000,
            ..Default::default()
        }
    }

    #[test]
    fn benchmark_ingestion_keeps_units_conditions_and_explicit_unavailable_state() {
        let mut value = benchmark();
        value.conditions = Some(prost_types::Struct {
            fields: BTreeMap::from([(
                "test_size_bytes".into(),
                prost_types::Value {
                    kind: Some(prost_types::value::Kind::NumberValue(4096.0)),
                },
            )]),
        });
        let fields = benchmark_fields(&value, 1_000_001).unwrap();
        assert_eq!(fields["tool_name"], "FixtureBench");
        assert_eq!(
            fields["measurement_units.sequential_read"],
            "bytes_per_second"
        );
        assert_eq!(fields["measurement_states.random_read_iops"], "unsupported");
        assert_eq!(fields["conditions.test_size_bytes"], 4096.0);
        assert!(!fields.contains_key("measurements.random_read_iops"));
    }

    #[test]
    fn benchmark_rejects_non_finite_values_and_conflicting_availability() {
        let mut value = benchmark();
        value.measurements.insert("latency".into(), f64::NAN);
        assert_eq!(
            benchmark_fields(&value, 1_000_001).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );

        let mut value = benchmark();
        value
            .measurement_states
            .insert("sequential_read".into(), DiagnosticValueState::Error as i32);
        assert_eq!(
            benchmark_fields(&value, 1_000_001).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );

        let mut value = benchmark();
        value.measurement_states.insert(
            "missing_value".into(),
            DiagnosticValueState::Available as i32,
        );
        assert_eq!(
            benchmark_fields(&value, 1_000_001).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn smart_report_retains_per_attribute_units_and_access_denied_state() {
        let value = SmartReportRequest {
            device_model: "Example SSD".into(),
            firmware: "1.0".into(),
            attributes: HashMap::from([("temperature_celsius".into(), "42".into())]),
            attribute_units: HashMap::from([("temperature_celsius".into(), "celsius".into())]),
            attribute_states: HashMap::from([(
                "wear_level".into(),
                DiagnosticValueState::AccessDenied as i32,
            )]),
            observed_at: 1_000_000,
            ..Default::default()
        };
        let fields = smart_fields(&value, 1_000_001).unwrap();
        assert_eq!(fields["attributes.temperature_celsius"], "42");
        assert_eq!(fields["attribute_units.temperature_celsius"], "celsius");
        assert_eq!(fields["attribute_states.wear_level"], "access_denied");
    }

    #[test]
    fn a_replacement_volume_has_a_distinct_report_binding() {
        let mut first = benchmark();
        first.observed_at = 1_000_000;
        let fields = benchmark_fields(&first, 1_000_001).unwrap();
        let owner = super::super::super::grants::PluginIdentity {
            plugin_id: "fixture.plugin".into(),
            publisher_fingerprint: "ab".repeat(32),
            profile_id: "profile-a".into(),
        };
        let broker = super::super::super::shared_data::DataBroker::new();
        let publish = |volume: &str, observed_at| Publication {
            owner: owner.clone(),
            dataset_id: BENCHMARK_DATASET.into(),
            category: None,
            class: SharingClass::Sensitive,
            schema_id: BENCHMARK_DATASET.into(),
            schema_version: 1,
            fields: fields.clone(),
            opaque_payload: Vec::new(),
            resource_id: volume.into(),
            observed_at,
            sequence: 0,
            source_observation_id: String::new(),
            session_id: Some("session".into()),
            publisher: None,
            expires_at: None,
        };
        broker
            .publish(publish("volume-old", 1_000_000), 1_000_001)
            .unwrap();
        broker
            .publish(publish("volume-replacement", 1_000_001), 1_000_001)
            .unwrap();
        let result = broker
            .query_data(
                QueryRequest {
                    caller: owner.clone(),
                    dataset_id: BENCHMARK_DATASET.into(),
                    record_id: None,
                    category: None,
                    selector: None,
                    delivery: Delivery::History,
                    page_size: 10,
                    cursor: None,
                },
                &super::super::super::resources::DenySharedData,
                1_000_001,
            )
            .unwrap();
        let replacement = result
            .records
            .iter()
            .filter(|record| report_matches(record, "fixture.plugin", "volume-replacement"))
            .count();
        assert_eq!(replacement, 1);
        assert!(
            result
                .records
                .iter()
                .filter(|record| report_matches(record, "fixture.plugin", "volume-replacement"))
                .all(|record| record.provenance().source == owner)
        );
        assert_eq!(
            result
                .records
                .iter()
                .filter(|record| report_matches(record, "fixture.plugin", "volume-old"))
                .count(),
            1
        );
    }

    #[test]
    fn sensitive_report_read_returns_only_the_approved_projection() {
        let owner = super::super::super::grants::PluginIdentity {
            plugin_id: "producer.plugin".into(),
            publisher_fingerprint: "aa".repeat(32),
            profile_id: "profile-a".into(),
        };
        let recipient = super::super::super::grants::PluginIdentity {
            plugin_id: "consumer.plugin".into(),
            publisher_fingerprint: "bb".repeat(32),
            profile_id: "profile-a".into(),
        };
        let mut fields = serde_json::Map::new();
        fields.insert("tool_name".into(), JsonValue::String("FixtureBench".into()));
        fields.insert(
            "measurements.sequential_read".into(),
            JsonValue::from(1000.0),
        );
        let broker = super::super::super::shared_data::DataBroker::new();
        broker
            .publish(
                Publication {
                    owner,
                    dataset_id: BENCHMARK_DATASET.into(),
                    category: None,
                    class: SharingClass::Sensitive,
                    schema_id: BENCHMARK_DATASET.into(),
                    schema_version: 1,
                    fields,
                    opaque_payload: Vec::new(),
                    resource_id: "volume-token".into(),
                    observed_at: 1_000_000,
                    sequence: 0,
                    source_observation_id: String::new(),
                    session_id: Some("producer-session".into()),
                    publisher: None,
                    expires_at: None,
                },
                1_000_001,
            )
            .unwrap();
        let result = broker
            .query_data(
                QueryRequest {
                    caller: recipient,
                    dataset_id: BENCHMARK_DATASET.into(),
                    record_id: None,
                    category: None,
                    selector: None,
                    delivery: Delivery::History,
                    page_size: 10,
                    cursor: None,
                },
                &ToolNameOnly,
                1_000_001,
            )
            .unwrap();
        assert_eq!(result.records.len(), 1);
        assert_eq!(
            result.records[0]
                .fields()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["tool_name"]
        );
    }
}
