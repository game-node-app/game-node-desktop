//! Convenience calls for the host action services of API 1.6: launch/install, backups,
//! host-catalog tools and ports, and the jobs they return.
//!
//! Each method sends exactly the request a hand-written raw `ResourceHost` call would and returns
//! the host's answer unchanged, so the two are interchangeable. The helpers add no authority:
//! they use the session's `resource_context`, and the host still checks every grant, scope,
//! revision, confirmation and job ownership.
//!
//! Action services answer with a job. The user may have to confirm the operation in Core first:
//! the job stays `QUEUED` with the message `awaiting confirmation` until the dialog closes.
//! [`Actions::job`], [`Actions::watch_job`] and [`Actions::cancel_job`] follow or withdraw it.
//! Revoking the operation's permission while the dialog is open makes the job fail without
//! committing anything; see `docs/plugin-development.md`.

use tonic::{Status, Streaming};

use crate::proto::resource_host_client::ResourceHostClient;
use crate::proto::*;
use crate::{MAX_MESSAGE_BYTES, SERVICE_TIMEOUT};

/// How many times a list restarts when its cursor goes stale because the host's observations
/// changed under the walk.
const LIST_RESTARTS: usize = 3;

fn timed<T>(message: T) -> tonic::Request<T> {
    let mut request = tonic::Request::new(message);
    request.set_timeout(SERVICE_TIMEOUT);
    request
}

/// Typed access to the action services, bound to one session's `resource_context`. Get it from
/// [`HostClient::actions`](crate::HostClient::actions). Cheap to clone.
#[derive(Clone)]
pub struct Actions {
    pub(crate) client: ResourceHostClient<tonic::transport::Channel>,
    pub(crate) context: ServiceContext,
}

impl Actions {
    pub(crate) fn new(channel: tonic::transport::Channel, context: ServiceContext) -> Self {
        Self {
            client: ResourceHostClient::new(channel)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES),
            context,
        }
    }

    fn mutation(&self, key: &str, expected_revision: u64) -> MutationContext {
        MutationContext {
            context: Some(self.context.clone()),
            operation_id: String::new(),
            idempotency_key: key.to_owned(),
            expected_revision,
        }
    }

    fn page(&self, page_size: u32, cursor: &str) -> PageRequest {
        PageRequest {
            context: Some(self.context.clone()),
            cursor: cursor.to_owned(),
            page_size,
            common_data_categories: None,
        }
    }

    // ---- Launch and install (`game.launch`, `games.install`) ----

    /// Asks the host to start an installed game through its detected target. `expected_revision`
    /// is the entry's `revision`; `installation_id` may be empty when the extension's
    /// host-selected target is the game. Leave `user_intent_id` empty to let Core ask the user;
    /// a host-issued intent from a Core action skips the dialog. The answer is a job.
    pub async fn launch_game(
        &self,
        game_id: &str,
        installation_id: &str,
        expected_revision: u64,
        user_intent_id: &str,
        idempotency_key: &str,
    ) -> Result<GameLaunchReply, Status> {
        self.client
            .clone()
            .games_request_launch(timed(GameLaunchRequest {
                context: Some(self.context.clone()),
                game_id: game_id.to_owned(),
                installation_id: installation_id.to_owned(),
                expected_revision,
                user_intent_id: user_intent_id.to_owned(),
                idempotency_key: idempotency_key.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Asks the host to open the provider's install flow for a game. The user confirms in Core
    /// unless a host-issued `user_intent_id` is given. The answer is a job.
    pub async fn install_game(
        &self,
        game_id: &str,
        installation_id: &str,
        user_intent_id: &str,
        idempotency_key: &str,
    ) -> Result<JobReply, Status> {
        self.client
            .clone()
            .games_request_install(timed(GameInstallRequest {
                context: Some(self.context.clone()),
                game_id: game_id.to_owned(),
                installation_id: installation_id.to_owned(),
                user_intent_id: user_intent_id.to_owned(),
                idempotency_key: idempotency_key.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    // ---- Jobs (`JobsGet`, `JobsWatch`, `JobsCancel`) ----

    /// One job of this plugin and profile. `NOT_FOUND` means it is not yours or it is gone.
    pub async fn job(&self, job_id: &str) -> Result<JobReply, Status> {
        self.client
            .clone()
            .jobs_get(timed(JobGetRequest {
                context: Some(self.context.clone()),
                job_id: job_id.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// The job's events in order, starting after `after_sequence` (0: from the beginning). The
    /// stream ends after the terminal state. No request timeout is applied, because a job may
    /// wait for the user's confirmation.
    pub async fn watch_job(
        &self,
        job_id: &str,
        after_sequence: u64,
    ) -> Result<Streaming<JobEvent>, Status> {
        self.client
            .clone()
            .jobs_watch(JobWatchRequest {
                context: Some(self.context.clone()),
                job_id: job_id.to_owned(),
                after_sequence,
            })
            .await
            .map(tonic::Response::into_inner)
    }

    /// Asks to stop a job. `CANCELLED` in the reply means it stopped; `RUNNING` means the work
    /// cannot stop halfway and will report what it finally did.
    pub async fn cancel_job(
        &self,
        job_id: &str,
        idempotency_key: &str,
    ) -> Result<JobReply, Status> {
        self.client
            .clone()
            .jobs_cancel(timed(JobCancelRequest {
                mutation: Some(self.mutation(idempotency_key, 0)),
                job_id: job_id.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    // ---- Backups (`backups.read`, `backups.create`, `backups.restore`) ----

    /// One page of backup history for an approved game and save profile.
    pub async fn backup_page(
        &self,
        game_id: &str,
        profile_id: &str,
        page_size: u32,
        cursor: &str,
    ) -> Result<BackupsListReply, Status> {
        self.client
            .clone()
            .backups_list(timed(BackupsListRequest {
                page: Some(self.page(page_size, cursor)),
                game_id: game_id.to_owned(),
                profile_id: profile_id.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Every retained backup of an approved game and save profile, following the cursors. A
    /// stale cursor restarts the walk, up to three times.
    pub async fn backups(
        &self,
        game_id: &str,
        profile_id: &str,
    ) -> Result<Vec<BackupRecord>, Status> {
        let mut restarts = 0;
        'again: loop {
            let mut backups = Vec::new();
            let mut cursor = String::new();
            loop {
                match self
                    .backup_page(
                        game_id,
                        profile_id,
                        crate::resources::MAX_PAGE_SIZE,
                        &cursor,
                    )
                    .await
                {
                    Ok(page) => {
                        backups.extend(page.backups);
                        if page.next_cursor.is_empty() {
                            return Ok(backups);
                        }
                        cursor = page.next_cursor;
                    }
                    Err(status)
                        if status.code() == tonic::Code::FailedPrecondition
                            && !cursor.is_empty()
                            && restarts < LIST_RESTARTS =>
                    {
                        restarts += 1;
                        continue 'again;
                    }
                    Err(status) => return Err(status),
                }
            }
        }
    }

    /// Requests a backup of an approved save profile. `expected_revision` is the profile's
    /// revision. The answer is a job; a restore confirmation is only for restore.
    pub async fn create_backup(
        &self,
        game_id: &str,
        profile_id: &str,
        reason: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<JobReply, Status> {
        self.client
            .clone()
            .backups_request_create(timed(BackupCreateRequest {
                mutation: Some(self.mutation(idempotency_key, expected_revision)),
                game_id: game_id.to_owned(),
                profile_id: profile_id.to_owned(),
                reason: reason.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Requests a restore of one retained backup. Core shows its own restore confirmation, so
    /// `user_intent_id` stays empty. The answer is a job.
    pub async fn restore_backup(
        &self,
        backup_id: &str,
        game_id: &str,
        profile_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<JobReply, Status> {
        self.client
            .clone()
            .backups_request_restore(timed(BackupRestoreRequest {
                mutation: Some(self.mutation(idempotency_key, expected_revision)),
                backup_id: backup_id.to_owned(),
                game_id: game_id.to_owned(),
                profile_id: profile_id.to_owned(),
                user_intent_id: String::new(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    // ---- Host-catalog tools (`tools.read`, `tools.apply`, `tools.remove`) ----

    /// One page of the host tool catalog. `game_id` scopes the catalog and its availability to
    /// one installed game; empty lists the built-in catalog.
    pub async fn tool_page(
        &self,
        game_id: &str,
        platform: &str,
        page_size: u32,
        cursor: &str,
    ) -> Result<ToolsListReply, Status> {
        self.client
            .clone()
            .tools_list(timed(ToolsListRequest {
                page: Some(self.page(page_size, cursor)),
                game_id: game_id.to_owned(),
                platform: platform.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Every tool in the catalog for `game_id` and `platform`, following the cursors. A stale
    /// cursor restarts the walk, up to three times.
    pub async fn tools(&self, game_id: &str, platform: &str) -> Result<Vec<ToolRecord>, Status> {
        let mut restarts = 0;
        'again: loop {
            let mut tools = Vec::new();
            let mut cursor = String::new();
            loop {
                match self
                    .tool_page(game_id, platform, crate::resources::MAX_PAGE_SIZE, &cursor)
                    .await
                {
                    Ok(page) => {
                        tools.extend(page.tools);
                        if page.next_cursor.is_empty() {
                            return Ok(tools);
                        }
                        cursor = page.next_cursor;
                    }
                    Err(status)
                        if status.code() == tonic::Code::FailedPrecondition
                            && !cursor.is_empty()
                            && restarts < LIST_RESTARTS =>
                    {
                        restarts += 1;
                        continue 'again;
                    }
                    Err(status) => return Err(status),
                }
            }
        }
    }

    /// Requests the host-catalog tool's apply/update on one installed game. `expected_revision`
    /// is the entry's `revision`; the host opens its own review. The answer is a job.
    pub async fn apply_tool(
        &self,
        tool_id: &str,
        game_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<JobReply, Status> {
        self.client
            .clone()
            .tools_request_apply(timed(ToolApplyRequest {
                mutation: Some(self.mutation(idempotency_key, expected_revision)),
                tool_id: tool_id.to_owned(),
                game_id: game_id.to_owned(),
                user_intent_id: String::new(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Requests removal of an installed host-catalog tool from one game. The answer is a job.
    pub async fn remove_tool(
        &self,
        tool_id: &str,
        game_id: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<JobReply, Status> {
        self.client
            .clone()
            .tools_request_remove(timed(ToolRemoveRequest {
                mutation: Some(self.mutation(idempotency_key, expected_revision)),
                tool_id: tool_id.to_owned(),
                game_id: game_id.to_owned(),
                user_intent_id: String::new(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    // ---- Host-catalog PC ports (`ports.read`, `ports.install`) ----

    /// One page of the host port catalog, optionally filtered by platform and a case-insensitive
    /// name query.
    pub async fn port_page(
        &self,
        platform: &str,
        query: &str,
        page_size: u32,
        cursor: &str,
    ) -> Result<PortsListReply, Status> {
        self.client
            .clone()
            .ports_list(timed(PortsListRequest {
                page: Some(self.page(page_size, cursor)),
                platform: platform.to_owned(),
                query: query.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Every host port matching the filter, following the cursors. A stale cursor restarts the
    /// walk, up to three times.
    pub async fn ports(&self, platform: &str, query: &str) -> Result<Vec<PortRecord>, Status> {
        let mut restarts = 0;
        'again: loop {
            let mut ports = Vec::new();
            let mut cursor = String::new();
            loop {
                match self
                    .port_page(platform, query, crate::resources::MAX_PAGE_SIZE, &cursor)
                    .await
                {
                    Ok(page) => {
                        ports.extend(page.ports);
                        if page.next_cursor.is_empty() {
                            return Ok(ports);
                        }
                        cursor = page.next_cursor;
                    }
                    Err(status)
                        if status.code() == tonic::Code::FailedPrecondition
                            && !cursor.is_empty()
                            && restarts < LIST_RESTARTS =>
                    {
                        restarts += 1;
                        continue 'again;
                    }
                    Err(status) => return Err(status),
                }
            }
        }
    }

    /// One host port by its catalog id.
    pub async fn port(&self, port_id: &str) -> Result<PortRecord, Status> {
        let reply = self
            .client
            .clone()
            .ports_detail(timed(PortDetailRequest {
                context: Some(self.context.clone()),
                port_id: port_id.to_owned(),
            }))
            .await?
            .into_inner();
        reply
            .port
            .ok_or_else(|| Status::not_found("port not found"))
    }

    /// Requests installation of a host-catalog port. `expected_revision` is the source game's
    /// entry revision, or 0 for a new catalog install; `supplied_file_handles` are optional
    /// approved Files candidates that need `files.read`. The host opens its own setup and review.
    pub async fn install_port(
        &self,
        port_id: &str,
        game_id: &str,
        supplied_file_handles: Vec<String>,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<JobReply, Status> {
        self.client
            .clone()
            .ports_request_install(timed(PortInstallRequest {
                mutation: Some(self.mutation(idempotency_key, expected_revision)),
                port_id: port_id.to_owned(),
                game_id: game_id.to_owned(),
                supplied_file_handles,
                user_intent_id: String::new(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }
}
