//! Finite host catalog, scoped supplied resources and the user's existing guided setup.
use super::mutations::{game_id, mutation_key, request_hash};
use super::*;
use crate::plugin_host::jobs::{JobOwner, Outcome, work};
use crate::plugin_host::provider::ResourceCaller;
use gamenode_core::ports::view::{PortPluginRequest, PortPluginSource};
use gamenode_platform::plugin_storage::operation;

fn port_id(id: &str) -> Result<(), Status> {
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        return Err(Status::invalid_argument(
            "a host catalog port id is required",
        ));
    }
    Ok(())
}
impl ResourceApi {
    pub(super) async fn list_ports(
        &self,
        request: Request<PortsListRequest>,
    ) -> Result<Response<PortsListReply>, Status> {
        let r = request.into_inner();
        let page = r.page.unwrap_or_default();
        let context = page.context.unwrap_or_default();
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::PORTS_READ)?;
        no_common_data(page.common_data_categories.as_ref())?;
        if !matches!(r.platform.as_str(), "" | "windows" | "linux")
            || r.query.len() > 256
            || r.query.chars().any(char::is_control)
        {
            return Err(Status::invalid_argument("invalid ports filter"));
        }
        let host = self
            .library
            .host()
            .ok_or_else(|| Status::unavailable("ports catalog unavailable"))?;
        let mut records = tokio::time::timeout(Duration::from_secs(30), host.ports_catalog(None))
            .await
            .map_err(|_| Status::deadline_exceeded("ports catalog deadline exceeded"))??;
        if records.len() > 10000 {
            return Err(Status::resource_exhausted(
                "port catalog exceeds the record limit",
            ));
        }
        let platform = r.platform.clone();
        let query_text = r.query.clone();
        let grant_revision = caller.grant_revision;
        let (records, binding) = tokio::task::spawn_blocking(move || {
            let query = query_text.to_lowercase();
            records.retain(|p| {
                (platform.is_empty() || p.platform == platform)
                    && (query.is_empty() || p.name.to_lowercase().contains(&query))
            });
            records.sort_by(|a, b| a.port_id.cmp(&b.port_id));
            let binding = format!(
                "ports:{:?}",
                request_hash(&[
                    &platform,
                    &query_text,
                    &grant_revision.to_string(),
                    &format!("{records:?}")
                ])
            );
            (records, binding)
        })
        .await
        .map_err(|_| Status::unavailable("port catalog worker unavailable"))?;
        let after = if page.cursor.is_empty() {
            0
        } else {
            self.files
                .open_cursor(&caller.session_id, &binding, &page.cursor)
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|n| *n <= records.len())
                .ok_or_else(|| Status::failed_precondition("port catalog changed; list again"))?
        };
        let size = if page.page_size == 0 {
            DEFAULT_PAGE_SIZE
        } else {
            (page.page_size as usize).min(MAX_PAGE_SIZE)
        };
        let end = (after + size).min(records.len());
        let next_cursor = if end < records.len() {
            self.files
                .seal_cursor(&caller.session_id, &binding, &end.to_string())
        } else {
            String::new()
        };
        (self.guard(context, cap::PORTS_READ, caller.grant_revision))()?;
        Ok(Response::new(PortsListReply {
            ports: records[after..end].to_vec(),
            next_cursor,
        }))
    }
    pub(super) async fn detail_port(
        &self,
        request: Request<PortDetailRequest>,
    ) -> Result<Response<PortDetailReply>, Status> {
        let r = request.into_inner();
        let context = r.context.unwrap_or_default();
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::PORTS_READ)?;
        port_id(&r.port_id)?;
        let host = self
            .library
            .host()
            .ok_or_else(|| Status::unavailable("ports catalog unavailable"))?;
        let records = tokio::time::timeout(
            Duration::from_secs(30),
            host.ports_catalog(Some(r.port_id.clone())),
        )
        .await
        .map_err(|_| Status::deadline_exceeded("port detail deadline exceeded"))??;
        let record = records
            .into_iter()
            .find(|p| p.port_id == r.port_id)
            .ok_or_else(|| Status::not_found("port not found"))?;
        (self.guard(context, cap::PORTS_READ, caller.grant_revision))()?;
        Ok(Response::new(PortDetailReply { port: Some(record) }))
    }
    fn resolve_port_sources(
        &self,
        caller: &ResourceCaller,
        handles: &[String],
    ) -> Result<Vec<PortPluginSource>, Status> {
        self.storage.on_backup_worker(caller, |namespace| {
            handles
                .iter()
                .map(|handle| {
                    let opened = super::files::open_handle(
                        &self.files,
                        &self.library,
                        namespace,
                        &caller.session_id,
                        handle,
                        operation::READ,
                    )?;
                    let entry = opened
                        .confined
                        .stat(&opened.path)
                        .map_err(super::files::file_status)?;
                    let path = opened.path.iter().fold(
                        std::path::PathBuf::from(&opened.approval.root),
                        |p, part| p.join(part),
                    );
                    Ok(PortPluginSource {
                        path: path.to_string_lossy().into_owned(),
                        directory: entry.directory,
                    })
                })
                .collect()
        })
    }
    pub(super) async fn install_port(
        &self,
        request: Request<PortInstallRequest>,
    ) -> Result<Response<JobReply>, Status> {
        let r = request.into_inner();
        let mutation = r
            .mutation
            .ok_or_else(|| Status::invalid_argument("mutation required"))?;
        let context = mutation.context.clone().unwrap_or_default();
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::PORTS_INSTALL)?;
        port_id(&r.port_id)?;
        if !r.user_intent_id.is_empty() {
            return Err(Status::failed_precondition(
                "port installation requires Core guided setup",
            ));
        }
        if r.supplied_file_handles.len() > 16
            || r.supplied_file_handles
                .iter()
                .any(|s| s.is_empty() || s.len() > 16384)
        {
            return Err(Status::invalid_argument(
                "at most 16 scoped source handles are accepted",
            ));
        }
        let target = if game_id(&r.game_id)?.is_some() {
            Some(self.tool_target(&caller, &context, &r.game_id)?)
        } else {
            None
        };
        if target
            .as_ref()
            .is_some_and(|(_, e)| e.revision != mutation.expected_revision)
            || (target.is_none() && mutation.expected_revision != 0)
        {
            return Err(Status::aborted("port source target revision changed"));
        }
        let host = self
            .library
            .host()
            .filter(|h| h.ports_supported())
            .ok_or_else(|| Status::unavailable("port installation unavailable on this platform"))?;
        let key = mutation_key(&mutation)?.to_owned();
        let hash = request_hash(&[
            cap::PORTS_INSTALL,
            &r.port_id,
            &r.game_id,
            &format!("{:?}", r.supplied_file_handles),
            &mutation.expected_revision.to_string(),
        ]);
        let owner = JobOwner::of(&caller)?;
        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.jobs.replay(&owner, &key, hash)? {
            return Ok(Response::new(original));
        }
        if !r.supplied_file_handles.is_empty() {
            self.providers
                .authorize_resource(Some(&context), cap::FILES_READ)?;
        }
        let api = self.clone();
        let c = caller.clone();
        let handles = r.supplied_file_handles.clone();
        let sources = if handles.is_empty() {
            vec![]
        } else {
            tokio::task::spawn_blocking(move || api.resolve_port_sources(&c, &handles))
                .await
                .map_err(|_| Status::unavailable("source scope worker unavailable"))??
        };
        let identity = caller
            .namespace
            .as_ref()
            .ok_or_else(|| Status::unavailable("plugin identity unavailable"))?
            .identity();
        let review = PortPluginRequest {
            id: String::new(),
            catalog_id: r.port_id.clone(),
            plugin_id: identity.plugin_id.clone(),
            publisher_fingerprint: identity.publisher_fingerprint.clone(),
            sources: sources.clone(),
        };
        let api = self.clone();
        let c = caller.clone();
        let cx = context.clone();
        let run = work(move |job| async move {
            if let Err(e) = job.start() {
                return Outcome::stopped(&e);
            }
            let checkpoint = job.checkpoint();
            let check = Arc::new(move || {
                checkpoint()?;
                if let Some((key, entry)) = &target
                    && api.library.revision_of(key) != Some(entry.revision)
                {
                    return Err(Status::failed_precondition("port source game changed"));
                }
                if !r.supplied_file_handles.is_empty() {
                    api.providers
                        .authorize_resource(Some(&cx), cap::FILES_READ)?;
                    if api.resolve_port_sources(&c, &r.supplied_file_handles)? != sources {
                        return Err(Status::failed_precondition("port sources changed"));
                    }
                }
                Ok(())
            });
            match host.request_port(r.port_id, review, check).await {
                Ok(()) => Outcome::succeeded("port_complete", "port installation completed"),
                Err(e) => Outcome::stopped(&e),
            }
        });
        let (job, _) = self.start_job(
            &caller,
            context,
            cap::PORTS_INSTALL,
            &key,
            hash,
            Some("review in Core port setup"),
            run,
        )?;
        Ok(Response::new(job))
    }
}
