//! Session-scoped published data RPCs. Common projections and watches remain unavailable until
//! their separate sharing-policy and revocation adapters are wired.

use gamenode_plugin_protocol::{capability as cap, common_data, proto::*};
use tonic::{Request, Response, Status};

use super::ResourceApi;
use crate::plugin_host::shared_data::{PublishOutcome, Publisher};

impl ResourceApi {
    pub(super) async fn publish_data(
        &self,
        request: Request<DataPublishRequest>,
    ) -> Result<Response<DataMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .as_ref()
            .ok_or_else(|| Status::unauthenticated("resource context required"))?
            .clone();
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::DATA_PUBLISH)?;
        if mutation.idempotency_key.trim().is_empty() || mutation.idempotency_key.len() > 128 {
            return Err(Status::invalid_argument(
                "idempotency_key must contain 1 to 128 bytes",
            ));
        }
        match StorageDurability::try_from(request.durability) {
            Ok(StorageDurability::SessionOnly) => {}
            Ok(StorageDurability::Persistent) => {
                self.providers
                    .authorize_resource(Some(&context), cap::STORAGE_PERSIST)?;
                return Err(Status::unimplemented(
                    "persistent data publication is unavailable; no durable write was made",
                ));
            }
            _ => return Err(Status::invalid_argument("durability is required")),
        }
        let namespace = caller.namespace.as_ref().ok_or_else(|| {
            Status::permission_denied("published data requires an authenticated third-party plugin")
        })?;
        let identity = namespace.identity();
        let value = request.value;
        if value.is_some() == !request.opaque_payload.is_empty() {
            return Err(Status::invalid_argument(
                "provide exactly one of value or opaque_payload",
            ));
        }
        let record = DataRecord {
            dataset_id: request.dataset_id,
            schema_id: request.schema_id,
            schema_version: request.schema_version,
            value,
            opaque_payload: request.opaque_payload,
            resource_id: request.resource_id,
            observed_at: request.observed_at,
            sequence: request.sequence,
            source_observation_id: request.source_observation_id,
            ..Default::default()
        };
        let publisher = Publisher {
            profile_id: identity.profile_id.clone(),
            profile_epoch: caller.profile_epoch,
            plugin_id: identity.plugin_id.clone(),
            publisher_fingerprint: identity.publisher_fingerprint.clone(),
            package_version: caller.package_version,
            declared_developer: caller.declared_developer,
            origin: caller.origin,
            package_digest: caller.package_digest,
            session_id: caller.session_id,
        };
        let providers = self.providers.clone();
        let store = self.shared_data.clone();
        let ttl = request.ttl_seconds;
        let published = tokio::task::spawn_blocking(move || {
            store.publish(&publisher, record, ttl, |session| {
                providers.session_is_open(session)
            })
        })
        .await
        .map_err(|_| Status::internal("published data worker failed"))??;
        let (record, outcome) = match published {
            PublishOutcome::Applied(record) => (record, MutationOutcome::Applied),
            PublishOutcome::Duplicate(record) => (record, MutationOutcome::AlreadyApplied),
            PublishOutcome::Conflict { current_sequence } => {
                return Ok(Response::new(DataMutationReply {
                    revision: current_sequence,
                    outcome: MutationOutcome::RevisionConflict as i32,
                    ..Default::default()
                }));
            }
        };
        self.providers
            .authorize_resource(Some(&context), cap::DATA_PUBLISH)?;
        Ok(Response::new(DataMutationReply {
            record_id: record.record_id,
            revision: record.revision,
            expires_at: record.expires_at,
            outcome: outcome as i32,
        }))
    }

    pub(super) async fn get_data(
        &self,
        request: Request<DataGetRequest>,
    ) -> Result<Response<DataGetReply>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::DATA_PUBLISH)?;
        ensure_private_selector(
            &self.providers,
            request.context.as_ref(),
            request.common_data_categories.as_ref(),
        )?;
        let namespace = caller.namespace.as_ref().ok_or_else(|| {
            Status::permission_denied("published data requires an authenticated third-party plugin")
        })?;
        let profile_id = namespace.identity().profile_id.clone();
        let plugin_id = namespace.identity().plugin_id.clone();
        let store = self.shared_data.clone();
        let providers = self.providers.clone();
        let record = tokio::task::spawn_blocking(move || {
            store.get_private(
                &profile_id,
                caller.profile_epoch,
                &plugin_id,
                &request.dataset_id,
                &request.record_id,
                |session| providers.session_is_open(session),
            )
        })
        .await
        .map_err(|_| Status::internal("published data worker failed"))?;
        let context = request.context;
        self.providers
            .authorize_resource(context.as_ref(), cap::DATA_PUBLISH)?;
        Ok(Response::new(DataGetReply { record }))
    }

    pub(super) async fn list_data(
        &self,
        request: Request<DataListRequest>,
    ) -> Result<Response<DataListReply>, Status> {
        let request = request.into_inner();
        let page = request
            .page
            .ok_or_else(|| Status::invalid_argument("page is required"))?;
        let caller = self
            .providers
            .authorize_resource(page.context.as_ref(), cap::DATA_PUBLISH)?;
        ensure_private_selector(
            &self.providers,
            page.context.as_ref(),
            request.common_data_categories.as_ref(),
        )?;
        if !request.filter.is_empty() || !page.cursor.is_empty() {
            return Err(Status::invalid_argument(
                "filter and cursor are not supported by this session-only API",
            ));
        }
        let namespace = caller.namespace.as_ref().ok_or_else(|| {
            Status::permission_denied("published data requires an authenticated third-party plugin")
        })?;
        let profile_id = namespace.identity().profile_id.clone();
        let plugin_id = namespace.identity().plugin_id.clone();
        let dataset_id = request.dataset_id;
        let store = self.shared_data.clone();
        let providers = self.providers.clone();
        let records = tokio::task::spawn_blocking(move || {
            store.list_private(
                &profile_id,
                caller.profile_epoch,
                &plugin_id,
                &dataset_id,
                |session| providers.session_is_open(session),
            )
        })
        .await
        .map_err(|_| Status::internal("published data worker failed"))?;
        self.providers
            .authorize_resource(page.context.as_ref(), cap::DATA_PUBLISH)?;
        Ok(Response::new(DataListReply {
            records,
            next_cursor: String::new(),
        }))
    }
}

fn ensure_private_selector(
    providers: &crate::plugin_host::provider::ProviderApi,
    context: Option<&ServiceContext>,
    selector: Option<&CommonDataSelector>,
) -> Result<(), Status> {
    let categories = common_data::categories(selector)
        .map_err(|_| Status::invalid_argument("invalid common-data category selector"))?;
    let ids = common_data::selected_record_ids(selector)
        .map_err(|_| Status::invalid_argument("invalid selected-record selector"))?;
    if categories.is_empty() && ids.is_empty() {
        return Ok(());
    }
    for category in categories {
        let permission = common_data::permission(category)
            .ok_or_else(|| Status::invalid_argument("unsupported common-data category"))?;
        providers.authorize_resource(context, permission)?;
    }
    Err(Status::unimplemented(
        "common-data projections and selected-record grants are not available",
    ))
}
