//! Host catalog and user-reviewed tool operations. Plugins never choose installer inputs.
use super::mutations::{game_id, mutation_key, request_hash};
use super::*;
use crate::plugin_host::jobs::{JobOwner, Outcome, work};
use crate::plugin_host::library::{InstallState, Projected};
use crate::plugin_host::provider::ResourceCaller;

impl ResourceApi {
    pub(super) fn tool_target(
        &self,
        caller: &ResourceCaller,
        context: &ServiceContext,
        game: &str,
    ) -> Result<(String, Projected), Status> {
        let game_id =
            game_id(game)?.ok_or_else(|| Status::invalid_argument("game_id is required"))?;
        let installation = if context.session_kind() == SessionKind::Extension {
            context
                .target_game
                .as_ref()
                .ok_or_else(|| Status::not_found("game not found"))?
                .installation_id
                .clone()
        } else {
            let scope = Scope {
                own_provider: caller.provider_id.clone(),
                read_all: false,
                provider_filter: None,
            };
            let (_, entries) = self.library.by_game(&scope, game_id);
            let entries = entries
                .into_iter()
                .filter(|e| e.install_state == InstallState::Installed)
                .collect::<Vec<_>>();
            if entries.len() != 1 {
                return Err(Status::failed_precondition(
                    "one installed game target is required",
                ));
            }
            entries[0].installation_id.clone()
        };
        let target = self.action_target(caller, context, game, &installation)?;
        if target.1.install_state != InstallState::Installed {
            return Err(Status::failed_precondition("game is not installed"));
        }
        Ok(target)
    }

    pub(super) async fn list_tools(
        &self,
        request: Request<ToolsListRequest>,
    ) -> Result<Response<ToolsListReply>, Status> {
        let request = request.into_inner();
        let page = request.page.unwrap_or_default();
        let context = page.context.clone().unwrap_or_default();
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::TOOLS_READ)?;
        super::no_common_data(page.common_data_categories.as_ref())?;
        if !matches!(request.platform.as_str(), "" | "windows" | "linux") {
            return Err(Status::invalid_argument("unknown platform"));
        }
        let revision = self.library.revision();
        let host = self.library.host();
        let records = if request.game_id.is_empty() {
            gamenode_core::tools::builtin_tools()
                .into_iter()
                .map(|d| ToolRecord {
                    tool_id: d.id.as_str().into(),
                    name: d.name,
                    version: String::new(),
                    availability: if host.as_ref().is_some_and(|h| h.tools_supported()) {
                        "available"
                    } else {
                        "unavailable"
                    }
                    .into(),
                })
                .collect::<Vec<_>>()
        } else {
            let (key, _) = self.tool_target(&caller, &context, &request.game_id)?;
            host.as_ref()
                .ok_or_else(|| Status::unavailable("tools unavailable"))?
                .tools_list(key)
                .await?
        };
        let records = if !request.platform.is_empty() && request.platform != "windows" {
            vec![]
        } else {
            records
        };
        let binding = request_hash(&[
            "tools",
            &request.game_id,
            &request.platform,
            &caller.grant_revision.to_string(),
            &self.library.revision().to_string(),
            &format!("{records:?}"),
        ]);
        let binding = format!("tools:{binding:?}");
        let after = if page.cursor.is_empty() {
            0
        } else {
            self.files
                .open_cursor(&caller.session_id, &binding, &page.cursor)
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|n| *n <= records.len())
                .ok_or_else(|| Status::failed_precondition("tool catalog changed; list again"))?
        };
        let count = if page.page_size == 0 {
            DEFAULT_PAGE_SIZE
        } else {
            (page.page_size as usize).min(MAX_PAGE_SIZE)
        };
        let end = (after + count).min(records.len());
        let next_cursor = if end < records.len() {
            self.files
                .seal_cursor(&caller.session_id, &binding, &end.to_string())
        } else {
            String::new()
        };
        (self.guard(context, cap::TOOLS_READ, caller.grant_revision))()?;
        if self.library.revision() != revision {
            return Err(Status::failed_precondition(
                "tool target changed; list again",
            ));
        }
        Ok(Response::new(ToolsListReply {
            tools: records[after..end].to_vec(),
            next_cursor,
        }))
    }

    pub(super) async fn request_tool_action(
        &self,
        mutation: Option<MutationContext>,
        tool: String,
        game: String,
        intent: String,
        remove: bool,
    ) -> Result<Response<JobReply>, Status> {
        let mutation = mutation.ok_or_else(|| Status::invalid_argument("mutation required"))?;
        let context = mutation.context.clone().unwrap_or_default();
        let permission = if remove {
            cap::TOOLS_REMOVE
        } else {
            cap::TOOLS_APPLY
        };
        let caller = self
            .providers
            .authorize_resource(Some(&context), permission)?;
        let tool = gamenode_core::tools::find_tool(&tool)
            .ok_or_else(|| Status::not_found("tool not found"))?
            .id
            .as_str()
            .to_owned();
        if !intent.is_empty() {
            return Err(Status::failed_precondition(
                "tools require review in the Core tools window",
            ));
        }
        let (entry_key, entry) = self.tool_target(&caller, &context, &game)?;
        if entry.revision != mutation.expected_revision {
            return Err(Status::aborted("game target changed; read it again"));
        }
        let host = self
            .library
            .host()
            .filter(|h| h.tools_supported())
            .ok_or_else(|| Status::unavailable("tools are unavailable on this platform"))?;
        let key = mutation_key(&mutation)?.to_owned();
        let hash = request_hash(&[
            permission,
            &game,
            &tool,
            &mutation.expected_revision.to_string(),
        ]);
        let owner = JobOwner::of(&caller)?;
        let _serial = self.mutations.lock.lock().await;
        if let Some(job) = self.jobs.replay(&owner, &key, hash)? {
            return Ok(Response::new(job));
        }
        let identity = caller
            .namespace
            .as_ref()
            .ok_or_else(|| Status::unavailable("plugin identity unavailable"))?
            .identity();
        let review = gamenode_core::tools::ToolPluginRequest {
            plugin_id: identity.plugin_id.clone(),
            publisher_fingerprint: identity.publisher_fingerprint.clone(),
            remove,
        };
        let api = self.clone();
        let c = caller.clone();
        let cx = context.clone();
        let run = work(move |job| async move {
            if let Err(e) = job.start() {
                return Outcome::stopped(&e);
            }
            let checkpoint = job.checkpoint();
            let target_key = entry_key.clone();
            let check = Arc::new(move || {
                checkpoint()?;
                let (current_key, current) = api.tool_target(&c, &cx, &game)?;
                if current_key != entry_key || current.revision != entry.revision {
                    return Err(Status::failed_precondition("tool target changed"));
                }
                Ok(())
            });
            match host.request_tool(target_key, tool, review, check).await {
                Ok(()) => Outcome::succeeded("tool_complete", "tool operation completed"),
                Err(e) => Outcome::stopped(&e),
            }
        });
        let (job, _) = self.start_job(
            &caller,
            context,
            permission,
            &key,
            hash,
            Some("review in Core tools"),
            run,
        )?;
        Ok(Response::new(job))
    }
}
