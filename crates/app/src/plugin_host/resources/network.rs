//! `NetworkFetch` of `ResourceHost`.
//!
//! A plugin asks the host to fetch an https URL. It needs `network.fetch` *and* an approved
//! origin: the URL's exact origin (scheme, host, port) must be one the user approved for this
//! plugin, and nothing is reachable by default. The origin is checked again for every redirect
//! hop, so an unapproved or since-revoked origin is never contacted. The host adds no
//! credentials of its own, never connects to loopback, private, link-local or similar addresses
//! (the checks are in [`gamenode_platform::plugin_network`]), bounds the response and the whole
//! fetch, and answers a refusal without saying where a name resolved or where a redirect led.

use std::collections::HashMap;
use std::sync::Arc;

use gamenode_platform::plugin_network::{FetchError, Request as Fetch};
use gamenode_plugin_protocol::{capability as cap, proto::*};
use tonic::{Request, Response, Status};

use super::ResourceApi;
use crate::plugin_host::network::AllowOrigin;
use crate::plugin_host::provider::ResourceCaller;

fn fetch_status(error: FetchError, denied: impl FnOnce() -> Status) -> Status {
    match error {
        FetchError::Denied => denied(),
        FetchError::TooLarge => Status::resource_exhausted(
            "the response is larger than max_response_bytes (at most 8 MiB); nothing was truncated",
        ),
        FetchError::Timeout => Status::deadline_exceeded("the fetch took longer than timeout_ms"),
        FetchError::TooManyRedirects => Status::failed_precondition("too many redirects"),
        FetchError::Unreachable => Status::unavailable("the destination could not be reached"),
        FetchError::BadResponse => Status::unavailable("the destination sent an invalid redirect"),
    }
}

impl ResourceApi {
    pub(super) async fn fetch_url(
        &self,
        request: Request<NetworkFetchRequest>,
    ) -> Result<Response<NetworkFetchReply>, Status> {
        let NetworkFetchRequest {
            context,
            url,
            method,
            headers,
            body,
            max_response_bytes,
            timeout_ms,
        } = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(context.as_ref(), cap::NETWORK_FETCH)?;
        let resolver = caller.namespace.as_ref().ok_or_else(|| {
            Status::failed_precondition("official providers use trusted host adapters")
        })?;
        let identity = resolver.identity().clone();
        let owner = *resolver.storage_owner().key();
        let fetcher = self
            .network
            .fetcher()
            .ok_or_else(|| Status::unavailable("network fetch is not available"))?;

        let headers: Vec<(&str, &str)> = headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let fetch = Fetch::build(&url, &method, headers, body, max_response_bytes, timeout_ms)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        // The origin the plugin may reach is the user's to name, and exact.
        let origin = fetch
            .origin()
            .ok_or_else(|| Status::invalid_argument("url must be an https URL"))?;
        if !self.network.approved_origins(&identity).contains(&origin) {
            return Err(self
                .providers
                .deny_resource_scope(&caller, cap::NETWORK_FETCH));
        }
        let _slots = self.network.permits(owner)?;

        // Asked before the first request and before every redirect: revoking the grant or the
        // origin stops a fetch that is already running.
        let allow: AllowOrigin = {
            let providers = self.providers.clone();
            let network = self.network.clone();
            let context = context.clone().unwrap_or_default();
            Arc::new(move |origin: &str| {
                providers
                    .authorize_resource(Some(&context), cap::NETWORK_FETCH)
                    .is_ok()
                    && network.approved_origins(&identity).contains(origin)
            })
        };
        let response = fetcher
            .fetch(fetch, allow)
            .await
            .map_err(|error| fetch_status(error, || self.deny_scope(&caller)))?;
        // The bytes were fetched for a call that may since have lost its permission.
        self.providers
            .authorize_resource(context.as_ref(), cap::NETWORK_FETCH)?;
        Ok(Response::new(NetworkFetchReply {
            status_code: u32::from(response.status),
            headers: response.headers.into_iter().collect::<HashMap<_, _>>(),
            body: response.body,
            final_origin: response.final_origin,
        }))
    }

    fn deny_scope(&self, caller: &ResourceCaller) -> Status {
        self.providers
            .deny_resource_scope(caller, cap::NETWORK_FETCH)
    }
}
