//! Host-validated hardware observations and expiring game-presence state.
//!
//! Both families are sensitive data. Publication is authenticated to the current plugin session;
//! cross-plugin reads use the shared-data broker's exact dataset/field policy. Neither route
//! starts a collector, publishes online status, nor makes a process/backup decision.

use super::super::shared_data::{MAX_PAGE_SIZE, Publication, PublisherMetadata, QueryRequest};
use super::super::sharing_policy::{Delivery, SharingClass};
use super::*;
use futures_util::Stream;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tonic::{Request, Response, Status};

const COMPUTER_DATASET: &str = "computer.hardware.observations.v1";
const PRESENCE_DATASET: &str = "game.presence.v1";
const COMPUTER_PROJECTION: &str = "hardware.v1";
const PRESENCE_PROJECTION: &str = "presence.v1";
const PRESENCE_CLEAR_SCHEMA: &str = "game.presence.v1.clear";
const HOST_COMPUTER_CONTEXT: &str = "active-host";
const MAX_TEXT: usize = 512;
const MAX_FIELDS: usize = 256;
const MAX_STRUCT_DEPTH: usize = 8;
const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_PAGE: usize = 200;
const MAX_PRESENCE_TTL_SECONDS: u64 = 300;
const CLEAR_TOMBSTONE_TTL_SECONDS: u64 = 600;
const WATCH_QUEUE: usize = 32;
const WATCH_RECHECK: Duration = Duration::from_secs(5);

static NEXT_OBSERVATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

type WatchStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

pub const COMPUTER_SERVED: &[Served] = &[
    Served::new(
        "List",
        cap::COMPUTER_HARDWARE_READ,
        true,
        "own reports or exact sensitive source/recipient field grants",
    ),
    Served::new(
        "Get",
        cap::COMPUTER_HARDWARE_READ,
        false,
        "own report or exact sensitive source/recipient field grants",
    ),
    Served::new(
        "Watch",
        cap::COMPUTER_HARDWARE_READ,
        false,
        "own reports or exact sensitive source/recipient live field grants",
    ),
];

pub const PRESENCE_SERVED: &[Served] = &[
    Served::new(
        "List",
        cap::GAME_PRESENCE_READ,
        true,
        "own presence or exact sensitive source/recipient field grants",
    ),
    Served::new(
        "Get",
        cap::GAME_PRESENCE_READ,
        false,
        "own presence or exact sensitive source/recipient field grants",
    ),
    Served::new(
        "Watch",
        cap::GAME_PRESENCE_READ,
        false,
        "own presence or exact sensitive source/recipient live field grants",
    ),
];

fn broker_status(error: BrokerError) -> Status {
    match error {
        BrokerError::StaleCursor | BrokerError::StaleSubscription => {
            Status::failed_precondition("observation cursor is stale; query again")
        }
        BrokerError::AuthorizationChanged | BrokerError::Expired => {
            Status::failed_precondition("observation authorization changed")
        }
        BrokerError::RecordTooLarge | BrokerError::QuotaExceeded => {
            Status::resource_exhausted("observation capacity exceeded")
        }
        BrokerError::SequenceConflict(_) => {
            Status::aborted("observation sequence is stale; publish again")
        }
        _ => Status::invalid_argument("invalid observation request"),
    }
}

fn projection(value: &str, expected: &'static str) -> Result<(), Status> {
    if value == expected {
        Ok(())
    } else {
        Err(Status::invalid_argument("unknown observation projection"))
    }
}

fn next_sequence() -> Result<u64, Status> {
    NEXT_OBSERVATION_SEQUENCE
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map(|previous| previous + 1)
        .map_err(|_| Status::resource_exhausted("observation sequence is exhausted"))
}

fn checked_time(observed_at: i64, now: i64) -> Result<(), Status> {
    if observed_at <= 0 || observed_at > now.saturating_add(300) {
        Err(Status::invalid_argument(
            "observation time is outside the supported range",
        ))
    } else {
        Ok(())
    }
}

fn safe_field_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && !value.starts_with('_')
}

fn private_hardware_identifier(value: &str) -> bool {
    let normalized = value
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    matches!(
        normalized.as_slice(),
        b"serial"
            | b"serialnumber"
            | b"uuid"
            | b"machineid"
            | b"hardwareid"
            | b"deviceid"
            | b"mac"
            | b"macaddress"
            | b"assettag"
            | b"servicetag"
            | b"uniqueid"
            | b"hostid"
    )
}

fn safe_value(value: &JsonValue, depth: usize, items: &mut usize) -> Result<(), Status> {
    if depth > MAX_STRUCT_DEPTH {
        return Err(Status::invalid_argument("observation fields are too deep"));
    }
    *items += 1;
    if *items > MAX_FIELDS {
        return Err(Status::resource_exhausted(
            "observation field limit exceeded",
        ));
    }
    match value {
        JsonValue::Object(fields) => {
            for (key, value) in fields {
                if !safe_field_name(key) || private_hardware_identifier(key) {
                    return Err(Status::invalid_argument(
                        "observation contains an unsafe field name",
                    ));
                }
                safe_value(value, depth + 1, items)?;
            }
        }
        JsonValue::Array(values) => {
            if values.len() > MAX_FIELDS {
                return Err(Status::resource_exhausted("observation array is too large"));
            }
            for value in values {
                safe_value(value, depth + 1, items)?;
            }
        }
        JsonValue::String(text) if text.len() > MAX_TEXT || text.chars().any(char::is_control) => {
            return Err(Status::invalid_argument("observation text is invalid"));
        }
        JsonValue::Number(number) if number.as_f64().is_none_or(|value| !value.is_finite()) => {
            return Err(Status::invalid_argument("observation number is invalid"));
        }
        _ => {}
    }
    Ok(())
}

fn flatten(prefix: &str, value: &JsonValue, output: &mut JsonMap<String, JsonValue>) {
    if let JsonValue::Object(fields) = value {
        for (key, value) in fields {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            flatten(&path, value, output);
        }
    } else {
        output.insert(prefix.to_owned(), value.clone());
    }
}

fn checked_struct(
    value: Option<&prost_types::Struct>,
    root: &str,
) -> Result<JsonMap<String, JsonValue>, Status> {
    let value = value.ok_or_else(|| Status::invalid_argument("observations are required"))?;
    let json = JsonValue::Object(super::struct_to_json(value)?);
    let mut items = 0;
    safe_value(&json, 0, &mut items)?;
    let encoded = serde_json::to_vec(&json)
        .map_err(|_| Status::invalid_argument("observations are invalid"))?;
    if encoded.len() > MAX_RECORD_BYTES {
        return Err(Status::resource_exhausted(
            "observation payload is too large",
        ));
    }
    let mut flattened = JsonMap::new();
    flatten(root, &json, &mut flattened);
    Ok(flattened)
}

fn nested_fields(fields: &JsonMap<String, JsonValue>, prefix: &str) -> JsonMap<String, JsonValue> {
    fn insert_path(root: &mut JsonMap<String, JsonValue>, parts: &[&str], value: JsonValue) {
        if let Some((head, tail)) = parts.split_first() {
            if tail.is_empty() {
                root.insert((*head).to_owned(), value);
            } else {
                let child = root
                    .entry((*head).to_owned())
                    .or_insert_with(|| JsonValue::Object(JsonMap::new()));
                if let JsonValue::Object(child) = child {
                    insert_path(child, tail, value);
                }
            }
        }
    }
    let mut result = JsonMap::new();
    for (path, value) in fields {
        let Some(suffix) = path.strip_prefix(prefix) else {
            continue;
        };
        let Some(suffix) = suffix.strip_prefix('.') else {
            continue;
        };
        let parts = suffix.split('.').collect::<Vec<_>>();
        insert_path(&mut result, &parts, value.clone());
    }
    result
}

fn plugin_id(record: &ProjectedRecord) -> String {
    record.provenance().source.plugin_id.clone()
}

fn computer_report(record: &ProjectedRecord) -> ComputerReport {
    let fields = record.fields();
    ComputerReport {
        record_id: record.record_id().to_owned(),
        computer_context_id: HOST_COMPUTER_CONTEXT.into(),
        owner_plugin_id: plugin_id(record),
        schema_id: record.schema().0.to_owned(),
        schema_version: record.schema().1,
        typed_observations: Some(super::json_to_struct(&nested_fields(
            fields,
            "observations",
        ))),
        approved_extension: Some(prost_types::Struct::default()),
        observed_at: record.observed_at(),
        revision: record.revision(),
    }
}

fn presence_record(record: &ProjectedRecord) -> PresenceRecord {
    let fields = record.fields();
    let (game_id, session_id) = record
        .resource_id()
        .split_once('/')
        .map(|(game_id, session_id)| (game_id.to_owned(), session_id.to_owned()))
        .unwrap_or_default();
    PresenceRecord {
        presence_id: record.record_id().to_owned(),
        game_id,
        session_id,
        owner_plugin_id: plugin_id(record),
        sequence: record.revision(),
        observed_at: record.observed_at(),
        expires_at: record.expires_at().unwrap_or_default(),
        approved_details: Some(super::json_to_struct(&nested_fields(fields, "details"))),
        revision: record.revision(),
    }
}

fn matches_presence(record: &ProjectedRecord, game_id: &str, session_id: &str) -> bool {
    let Some((record_game_id, record_session_id)) = record.resource_id().split_once('/') else {
        return false;
    };
    (game_id.is_empty() || record_game_id == game_id)
        && (session_id.is_empty() || record_session_id == session_id)
}

fn is_cleared(record: &ProjectedRecord) -> bool {
    record.schema().0 == PRESENCE_CLEAR_SCHEMA
}

fn publication_metadata(caller: &ResourceCaller) -> PublisherMetadata {
    PublisherMetadata {
        declared_developer: caller.declared_developer.clone(),
        origin: caller.origin.clone(),
        package_version: caller.package_version.clone(),
        package_digest: caller.package_digest.clone(),
    }
}

fn observation_game_id(text: &str) -> Result<String, Status> {
    if text.len() > 20 || text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Status::invalid_argument(
            "game_id must be a GameNode game id",
        ));
    }
    let parsed = text
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| Status::invalid_argument("game_id must be a GameNode game id"))?;
    Ok(parsed.to_string())
}

fn presence_session_id(text: &str) -> Result<(), Status> {
    if text.is_empty()
        || text.len() > 128
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(Status::invalid_argument("session_id is invalid"));
    }
    Ok(())
}

fn presence_resource(game_id: &str, session_id: &str) -> String {
    format!("{game_id}/{session_id}")
}

impl ResourceApi {
    pub(super) async fn publish_hardware(
        &self,
        request: Request<HardwareReportRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::COMPUTER_HARDWARE_PUBLISH)?;
        let owner = Self::data_identity(&caller)?;
        if !request.computer_id.is_empty() {
            return Err(Status::invalid_argument(
                "computer context is host assigned; omit computer_id",
            ));
        }
        if request.schema_id.is_empty()
            || request.schema_id.len() > 256
            || !request
                .schema_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || request.schema_version == 0
        {
            return Err(Status::invalid_argument("invalid hardware schema"));
        }
        let now = super::unix_now()?;
        checked_time(request.observed_at, now)?;
        let fields = checked_struct(request.observations.as_ref(), "observations")?;
        if fields.is_empty() {
            return Err(Status::invalid_argument("observations are empty"));
        }
        let material = serde_json::to_string(&fields)
            .map_err(|_| Status::invalid_argument("observations are invalid"))?;
        let key = mutation_key(mutation)?.to_owned();
        let hash = mutations::request_hash(&[
            "computer-hardware",
            &request.schema_id,
            &request.schema_version.to_string(),
            &request.observed_at.to_string(),
            &mutation.expected_revision.to_string(),
            &material,
        ]);
        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, &key, hash)? {
            return Ok(Response::new(ReportMutationReply {
                report_id: original.resource_id,
                revision: original.revision,
                outcome: original.outcome,
                ..Default::default()
            }));
        }
        let fresh = self
            .providers
            .authorize_resource(Some(&context), cap::COMPUTER_HARDWARE_PUBLISH)?;
        if fresh.grant_revision != caller.grant_revision {
            return Err(Status::failed_precondition(
                "permissions changed; retry the hardware report",
            ));
        }
        let receipt = self
            .data
            .publish(
                Publication {
                    owner,
                    dataset_id: COMPUTER_DATASET.into(),
                    category: None,
                    class: SharingClass::Sensitive,
                    schema_id: request.schema_id,
                    schema_version: request.schema_version,
                    fields,
                    opaque_payload: Vec::new(),
                    resource_id: format!("{HOST_COMPUTER_CONTEXT}/{}", caller.profile_epoch),
                    observed_at: request.observed_at,
                    sequence: 0,
                    source_observation_id: String::new(),
                    session_id: Some(caller.session_id.clone()),
                    publisher: None,
                    expires_at: None,
                },
                now,
            )
            .map_err(broker_status)?;
        let outcome = if receipt.duplicate {
            MutationOutcome::AlreadyApplied
        } else {
            MutationOutcome::Applied
        };
        self.mutations.remember(
            &caller,
            &key,
            hash,
            &mutations::reply(&receipt.record_id, receipt.revision, outcome),
        );
        Ok(Response::new(ReportMutationReply {
            report_id: receipt.record_id,
            revision: receipt.revision,
            outcome: outcome as i32,
            ..Default::default()
        }))
    }

    pub(super) async fn publish_presence(
        &self,
        request: Request<PresencePublishRequest>,
    ) -> Result<Response<ReportMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::GAME_PRESENCE_PUBLISH)?;
        let owner = Self::data_identity(&caller)?;
        let game_id = observation_game_id(&request.game_id)?;
        presence_session_id(&request.session_id)?;
        if request.ttl_seconds == 0 || request.ttl_seconds > MAX_PRESENCE_TTL_SECONDS {
            return Err(Status::invalid_argument(
                "presence TTL must be 1 to 300 seconds",
            ));
        }
        let now = super::unix_now()?;
        let fields = checked_struct(request.details.as_ref(), "details")?;
        let material = serde_json::to_string(&fields)
            .map_err(|_| Status::invalid_argument("presence details are invalid"))?;
        let key = mutation_key(mutation)?.to_owned();
        let hash = mutations::request_hash(&[
            "game-presence-publish",
            &game_id,
            &request.session_id,
            &request.ttl_seconds.to_string(),
            &mutation.expected_revision.to_string(),
            &material,
        ]);
        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, &key, hash)? {
            return Ok(Response::new(ReportMutationReply {
                report_id: original.resource_id,
                revision: original.revision,
                outcome: original.outcome,
                ..Default::default()
            }));
        }
        let fresh = self
            .providers
            .authorize_resource(Some(&context), cap::GAME_PRESENCE_PUBLISH)?;
        if fresh.grant_revision != caller.grant_revision {
            return Err(Status::failed_precondition(
                "permissions changed; retry the presence report",
            ));
        }
        let receipt = self
            .data
            .publish_replacing(
                Publication {
                    owner,
                    dataset_id: PRESENCE_DATASET.into(),
                    category: None,
                    class: SharingClass::Sensitive,
                    schema_id: PRESENCE_DATASET.into(),
                    schema_version: 1,
                    fields,
                    opaque_payload: Vec::new(),
                    resource_id: presence_resource(&game_id, &request.session_id),
                    observed_at: now,
                    sequence: next_sequence()?,
                    source_observation_id: key.clone(),
                    session_id: Some(caller.session_id.clone()),
                    publisher: Some(publication_metadata(&caller)),
                    expires_at: Some(now.saturating_add(request.ttl_seconds as i64)),
                },
                now,
            )
            .map_err(broker_status)?;
        let outcome = if receipt.duplicate {
            MutationOutcome::AlreadyApplied
        } else {
            MutationOutcome::Applied
        };
        self.mutations.remember(
            &caller,
            &key,
            hash,
            &mutations::reply(&receipt.record_id, receipt.revision, outcome),
        );
        Ok(Response::new(ReportMutationReply {
            report_id: receipt.record_id,
            revision: receipt.revision,
            outcome: outcome as i32,
            ..Default::default()
        }))
    }

    pub(super) async fn clear_presence(
        &self,
        request: Request<PresenceClearRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::GAME_PRESENCE_PUBLISH)?;
        let owner = Self::data_identity(&caller)?;
        let game_id = observation_game_id(&request.game_id)?;
        presence_session_id(&request.session_id)?;
        let now = super::unix_now()?;
        let key = mutation_key(mutation)?.to_owned();
        let hash = mutations::request_hash(&[
            "game-presence-clear",
            &game_id,
            &request.session_id,
            &mutation.expected_revision.to_string(),
        ]);
        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, &key, hash)? {
            return Ok(Response::new(MutationReply {
                resource_id: original.resource_id,
                revision: original.revision,
                outcome: original.outcome,
            }));
        }
        let fresh = self
            .providers
            .authorize_resource(Some(&context), cap::GAME_PRESENCE_PUBLISH)?;
        if fresh.grant_revision != caller.grant_revision {
            return Err(Status::failed_precondition(
                "permissions changed; retry the presence clear",
            ));
        }
        let current = self
            .data
            .query_data(
                QueryRequest {
                    caller: owner.clone(),
                    dataset_id: PRESENCE_DATASET.into(),
                    record_id: None,
                    category: None,
                    selector: None,
                    delivery: Delivery::History,
                    page_size: MAX_PAGE_SIZE,
                    cursor: None,
                },
                self.shared_authorizer.as_ref(),
                now,
            )
            .map_err(broker_status)?;
        let resource_id = presence_resource(&game_id, &request.session_id);
        let mut fields = current
            .records
            .iter()
            .find(|record| record.resource_id() == resource_id)
            .map(|record| {
                record
                    .fields()
                    .keys()
                    .filter(|field| field.starts_with("details."))
                    .map(|field| (field.clone(), JsonValue::Null))
                    .collect::<JsonMap<_, _>>()
            })
            .unwrap_or_default();
        if fields.is_empty() {
            // A host-owned clear marker is exposed only if the recipient explicitly selects it.
            fields.insert("details.cleared".into(), JsonValue::Bool(true));
        }
        let receipt = self
            .data
            .publish_replacing(
                Publication {
                    owner,
                    dataset_id: PRESENCE_DATASET.into(),
                    category: None,
                    class: SharingClass::Sensitive,
                    schema_id: PRESENCE_CLEAR_SCHEMA.into(),
                    schema_version: 1,
                    fields,
                    opaque_payload: Vec::new(),
                    resource_id,
                    observed_at: now,
                    sequence: next_sequence()?,
                    source_observation_id: key.clone(),
                    session_id: Some(caller.session_id.clone()),
                    publisher: Some(publication_metadata(&caller)),
                    expires_at: Some(now.saturating_add(CLEAR_TOMBSTONE_TTL_SECONDS as i64)),
                },
                now,
            )
            .map_err(broker_status)?;
        let outcome = if receipt.duplicate {
            MutationOutcome::AlreadyApplied
        } else {
            MutationOutcome::Applied
        };
        self.mutations.remember(
            &caller,
            &key,
            hash,
            &mutations::reply(&receipt.record_id, receipt.revision, outcome),
        );
        Ok(Response::new(MutationReply {
            resource_id: receipt.record_id,
            revision: receipt.revision,
            outcome: outcome as i32,
        }))
    }

    async fn list_computers(
        &self,
        request: Request<ComputerListRequest>,
    ) -> Result<Response<ComputerListReply>, Status> {
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
            .authorize_resource(Some(&context), cap::COMPUTER_HARDWARE_READ)?;
        let identity = Self::data_identity(&caller)?;
        projection(&request.projection_id, COMPUTER_PROJECTION)?;
        if !request.computer_context_id.is_empty()
            && request.computer_context_id != HOST_COMPUTER_CONTEXT
        {
            return Err(Status::not_found("computer context not found"));
        }
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
                    dataset_id: COMPUTER_DATASET.into(),
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
        let reports = result.records.iter().map(computer_report).collect();
        let next_cursor = match result.next_cursor {
            Some(cursor) => self.issue_data_cursor(&caller.session_id, cursor)?,
            None => String::new(),
        };
        Ok(Response::new(ComputerListReply {
            reports,
            next_cursor,
            revision: result.revision,
        }))
    }

    async fn get_computer(
        &self,
        request: Request<ComputerGetRequest>,
    ) -> Result<Response<ComputerGetReply>, Status> {
        let request = request.into_inner();
        if request.computer_record_id.is_empty() || request.computer_record_id.len() > 256 {
            return Err(Status::invalid_argument("computer_record_id is required"));
        }
        projection(&request.projection_id, COMPUTER_PROJECTION)?;
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::COMPUTER_HARDWARE_READ)?;
        let result = self
            .data
            .query_data(
                QueryRequest {
                    caller: Self::data_identity(&caller)?,
                    dataset_id: COMPUTER_DATASET.into(),
                    record_id: Some(request.computer_record_id.clone()),
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
            .find(|record| record.record_id() == request.computer_record_id)
            .ok_or_else(|| Status::not_found("computer report not found"))?;
        Ok(Response::new(ComputerGetReply {
            report: Some(computer_report(record)),
        }))
    }

    async fn watch_computers(
        &self,
        request: Request<ComputerWatchRequest>,
    ) -> Result<Response<WatchStream<ComputerEvent>>, Status> {
        let request = request.into_inner();
        if !request.computer_context_id.is_empty()
            && request.computer_context_id != HOST_COMPUTER_CONTEXT
        {
            return Err(Status::not_found("computer context not found"));
        }
        projection(&request.projection_id, COMPUTER_PROJECTION)?;
        let context = request
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::COMPUTER_HARDWARE_READ)?;
        let subscription = self
            .data
            .subscribe(
                QueryRequest {
                    caller: Self::data_identity(&caller)?,
                    dataset_id: COMPUTER_DATASET.into(),
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
        let context = request.context;
        let stream = futures_util::stream::unfold(
            Some((subscription, slot, providers, context)),
            move |state| async move {
                let (subscription, slot, providers, context) = state?;
                loop {
                    let received = tokio::time::timeout(
                        WATCH_RECHECK,
                        subscription.recv(super::unix_now().unwrap_or(0)),
                    )
                    .await;
                    match providers
                        .authorize_resource(context.as_ref(), cap::COMPUTER_HARDWARE_READ)
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
                                    "hardware sharing changed; watch again",
                                )),
                                None,
                            ));
                        }
                        Ok(Err(SubscriptionEnd::FellBehind)) => {
                            return Some((
                                Err(Status::resource_exhausted(
                                    "hardware watch fell behind; list again",
                                )),
                                None,
                            ));
                        }
                    };
                    let value = ComputerEvent {
                        revision: event.revision(),
                        event_type: "published".into(),
                        report: Some(computer_report(&event)),
                    };
                    return Some((Ok(value), Some((subscription, slot, providers, context))));
                }
            },
        );
        Ok(Response::new(Box::pin(stream)))
    }

    async fn list_presence(
        &self,
        request: Request<PresenceListRequest>,
    ) -> Result<Response<PresenceListReply>, Status> {
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
            .authorize_resource(Some(&context), cap::GAME_PRESENCE_READ)?;
        let identity = Self::data_identity(&caller)?;
        projection(&request.projection_id, PRESENCE_PROJECTION)?;
        let game = if request.game_id.is_empty() {
            String::new()
        } else {
            observation_game_id(&request.game_id)?
        };
        if !request.session_id.is_empty() {
            presence_session_id(&request.session_id)?;
        }
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
                    dataset_id: PRESENCE_DATASET.into(),
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
        let records = result
            .records
            .iter()
            .filter(|record| {
                !is_cleared(record) && matches_presence(record, &game, &request.session_id)
            })
            .map(presence_record)
            .collect();
        let next_cursor = match result.next_cursor {
            Some(cursor) => self.issue_data_cursor(&caller.session_id, cursor)?,
            None => String::new(),
        };
        Ok(Response::new(PresenceListReply {
            records,
            next_cursor,
            revision: result.revision,
        }))
    }

    async fn get_presence(
        &self,
        request: Request<PresenceGetRequest>,
    ) -> Result<Response<PresenceGetReply>, Status> {
        let request = request.into_inner();
        if request.presence_id.is_empty() || request.presence_id.len() > 256 {
            return Err(Status::invalid_argument("presence_id is required"));
        }
        projection(&request.projection_id, PRESENCE_PROJECTION)?;
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(context), cap::GAME_PRESENCE_READ)?;
        let result = self
            .data
            .query_data(
                QueryRequest {
                    caller: Self::data_identity(&caller)?,
                    dataset_id: PRESENCE_DATASET.into(),
                    record_id: Some(request.presence_id.clone()),
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
            .find(|record| record.record_id() == request.presence_id && !is_cleared(record))
            .ok_or_else(|| Status::not_found("presence record not found"))?;
        Ok(Response::new(PresenceGetReply {
            record: Some(presence_record(record)),
        }))
    }

    async fn watch_presence(
        &self,
        request: Request<PresenceWatchRequest>,
    ) -> Result<Response<WatchStream<PresenceEvent>>, Status> {
        let request = request.into_inner();
        projection(&request.projection_id, PRESENCE_PROJECTION)?;
        let game = if request.game_id.is_empty() {
            String::new()
        } else {
            observation_game_id(&request.game_id)?
        };
        if !request.session_id.is_empty() {
            presence_session_id(&request.session_id)?;
        }
        let context = request
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::GAME_PRESENCE_READ)?;
        let subscription = self
            .data
            .subscribe(
                QueryRequest {
                    caller: Self::data_identity(&caller)?,
                    dataset_id: PRESENCE_DATASET.into(),
                    record_id: None,
                    category: None,
                    selector: None,
                    delivery: Delivery::Live,
                    page_size: DEFAULT_PAGE_SIZE,
                    cursor: None,
                },
                request.after_sequence,
                WATCH_QUEUE,
                self.shared_authorizer.clone(),
            )
            .map_err(broker_status)?;
        let slot = self.watch_slot(&caller.session_id)?;
        let providers = self.providers.clone();
        let grant_revision = caller.grant_revision;
        let context = request.context;
        let session_id = request.session_id;
        let stream = futures_util::stream::unfold(
            Some((subscription, slot, providers, context, game, session_id)),
            move |state| async move {
                let (subscription, slot, providers, context, game, session_id) = state?;
                loop {
                    let received = tokio::time::timeout(
                        WATCH_RECHECK,
                        subscription.recv(super::unix_now().unwrap_or(0)),
                    )
                    .await;
                    match providers.authorize_resource(context.as_ref(), cap::GAME_PRESENCE_READ) {
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
                                    "presence sharing changed; watch again",
                                )),
                                None,
                            ));
                        }
                        Ok(Err(SubscriptionEnd::FellBehind)) => {
                            return Some((
                                Err(Status::resource_exhausted(
                                    "presence watch fell behind; list again",
                                )),
                                None,
                            ));
                        }
                    };
                    if !matches_presence(&event, &game, &session_id) {
                        continue;
                    }
                    let cleared = is_cleared(&event);
                    let value = PresenceEvent {
                        sequence: event.revision(),
                        event_type: if cleared { "cleared" } else { "published" }.into(),
                        record: Some(presence_record(&event)),
                    };
                    return Some((
                        Ok(value),
                        Some((subscription, slot, providers, context, game, session_id)),
                    ));
                }
            },
        );
        Ok(Response::new(Box::pin(stream)))
    }
}

#[tonic::async_trait]
impl computer_server::Computer for ResourceApi {
    type WatchStream = WatchStream<ComputerEvent>;

    async fn list(
        &self,
        request: Request<ComputerListRequest>,
    ) -> Result<Response<ComputerListReply>, Status> {
        self.list_computers(request).await
    }

    async fn get(
        &self,
        request: Request<ComputerGetRequest>,
    ) -> Result<Response<ComputerGetReply>, Status> {
        self.get_computer(request).await
    }

    async fn watch(
        &self,
        request: Request<ComputerWatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        self.watch_computers(request).await
    }
}

#[tonic::async_trait]
impl game_presence_server::GamePresence for ResourceApi {
    type WatchStream = WatchStream<PresenceEvent>;

    async fn list(
        &self,
        request: Request<PresenceListRequest>,
    ) -> Result<Response<PresenceListReply>, Status> {
        self.list_presence(request).await
    }

    async fn get(
        &self,
        request: Request<PresenceGetRequest>,
    ) -> Result<Response<PresenceGetReply>, Status> {
        self.get_presence(request).await
    }

    async fn watch(
        &self,
        request: Request<PresenceWatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        self.watch_presence(request).await
    }
}
