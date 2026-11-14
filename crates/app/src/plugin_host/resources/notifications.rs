//! `NotificationsPublish` of `ResourceHost`.
//!
//! A plugin with `notifications.publish` may ask Core to show a short plain-text notice. Core
//! attributes it to the authenticated plugin, never to a name in the request, bounds and checks the
//! text ([`crate::plugin_host::notifications`]), lets a plugin speak only a few times a minute
//! and hands the notice to the application, which renders it as plain text. A retried key is
//! answered from the first delivery, so a notice is shown once; a call that was refused for its rate
//! is not remembered, so it can be retried after the wait.

use gamenode_plugin_protocol::{capability as cap, proto::*};
use tonic::{Request, Response, Status};

use super::ResourceApi;
use super::mutations::{mutation_key, reply, request_hash};
use crate::plugin_host::notifications::{
    MAX_TRANSLATIONS, PluginNotice, Severity, Text, check_text,
};

impl ResourceApi {
    pub(super) async fn publish_notification(
        &self,
        request: Request<NotificationPublishRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation.context.clone().unwrap_or_default();
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::NOTIFICATIONS_PUBLISH)?;
        let key = mutation_key(mutation)?;
        if mutation.expected_revision != 0 {
            return Err(Status::invalid_argument(
                "notifications carry no revision; expected_revision must be 0",
            ));
        }
        let resolver = caller.namespace.as_ref().ok_or_else(|| {
            Status::failed_precondition("official providers use trusted host adapters")
        })?;
        let plugin_id = resolver.identity().plugin_id.clone();
        let owner = *resolver.storage_owner().key();

        let invalid = |refused: crate::plugin_host::notifications::Refused| {
            Status::invalid_argument(refused.to_string())
        };
        let severity = Severity::parse(&request.severity).ok_or_else(|| {
            Status::invalid_argument(
                "severity must be empty or one of \"info\", \"success\", \"warning\", \"error\"",
            )
        })?;
        let main = check_text(&request.locale, &request.title, &request.body).map_err(invalid)?;
        if request.translations.len() > MAX_TRANSLATIONS {
            return Err(Status::invalid_argument(format!(
                "at most {MAX_TRANSLATIONS} translations are allowed"
            )));
        }
        let mut texts = vec![main];
        for translation in &request.translations {
            if translation.locale.is_empty() {
                return Err(Status::invalid_argument("every translation needs a locale"));
            }
            let text = check_text(&translation.locale, &translation.title, &translation.body)
                .map_err(invalid)?;
            if texts
                .iter()
                .any(|known: &Text| known.locale.eq_ignore_ascii_case(&text.locale))
            {
                return Err(Status::invalid_argument(
                    "each locale may appear once, the main text's included",
                ));
            }
            texts.push(text);
        }

        let mut parts: Vec<&str> = vec!["notification", &request.severity];
        for text in &texts {
            parts.extend([
                text.locale.as_str(),
                text.title.as_str(),
                text.body.as_str(),
            ]);
        }
        let hash = request_hash(&parts);

        // Delivery is quick, so the whole decision runs under the mutation lock: a retry that
        // races its first call sees its outcome instead of delivering twice.
        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(original));
        }
        let sink = self
            .notifications
            .sink()
            .ok_or_else(|| Status::unavailable("notifications are not available"))?;
        // Permissions are checked again before the call is counted or delivered.
        self.guard(context, cap::NOTIFICATIONS_PUBLISH, caller.grant_revision)()?;
        if let Err(wait) = self.notifications.admit(owner) {
            let mut status = Status::resource_exhausted(format!(
                "too many notifications; retry after {} seconds",
                wait.as_secs().max(1)
            ));
            if let Ok(value) = wait.as_millis().to_string().parse() {
                status.metadata_mut().insert("retry-after-ms", value);
            }
            return Err(status);
        }
        sink.deliver(PluginNotice {
            plugin_id,
            severity,
            texts,
        });
        let answer = reply("", 0, MutationOutcome::Applied);
        self.mutations.remember(&caller, key, hash, &answer);
        Ok(Response::new(answer))
    }
}
