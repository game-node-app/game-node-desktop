//! `JobsGet`, `JobsWatch` and `JobsCancel` of `ResourceHost` (task 7.1).
//!
//! A job is addressed by its opaque id alone and belongs to the caller's host-authenticated
//! plugin/publisher/profile ([`JobOwner`]); see `plugin_host::jobs` for the registry. These
//! calls have no permission of their own: after finding the job under the caller's own owner,
//! each one needs the permission of the operation that created it, checked again before every
//! event a watch delivers. An id that is unknown, someone else's, from an earlier profile or
//! forgotten is the same `NOT_FOUND`.

use std::sync::Arc;

use gamenode_plugin_protocol::proto::*;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::mutations::mutation_key;
use super::{ResourceApi, WATCH_QUEUE, WATCH_RECHECK};
use crate::plugin_host::jobs::{Fence, JobOwner, Submission, Work};
use crate::plugin_host::provider::ResourceCaller;

const MAX_JOB_ID_BYTES: usize = 128;

fn job_id(id: &str) -> Result<&str, Status> {
    if id.is_empty() || id.len() > MAX_JOB_ID_BYTES {
        return Err(Status::invalid_argument(
            "job_id is required: 1 to 128 bytes",
        ));
    }
    Ok(id)
}

impl ResourceApi {
    /// Authenticates the call, finds the job under the caller's own owner and checks the
    /// permission of the operation that created it. Returns the caller, the owner, the job id and
    /// that permission.
    fn job_access<'a>(
        &self,
        context: Option<&ServiceContext>,
        id: &'a str,
    ) -> Result<(ResourceCaller, JobOwner, &'a str, &'static str), Status> {
        let caller = self.providers.authorize_resource_session(context)?;
        let owner = JobOwner::of(&caller)?;
        let id = job_id(id)?;
        let permission = self.jobs.permission(&owner, id)?;
        self.providers.authorize_resource(context, permission)?;
        Ok((caller, owner, id, permission))
    }

    pub(super) async fn get_job(
        &self,
        request: Request<JobGetRequest>,
    ) -> Result<Response<JobReply>, Status> {
        let request = request.into_inner();
        let (_, owner, id, _) = self.job_access(request.context.as_ref(), &request.job_id)?;
        self.jobs.get(&owner, id).map(Response::new)
    }

    pub(super) async fn cancel_job(
        &self,
        request: Request<JobCancelRequest>,
    ) -> Result<Response<JobReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        // Cancelling is idempotent in itself (a second request finds the first one's result), so
        // no outcome is stored; the key is still checked for its shape like every mutation's.
        mutation_key(mutation)?;
        let (_, owner, id, _) = self.job_access(mutation.context.as_ref(), &request.job_id)?;
        self.jobs.cancel(&owner, id).await.map(Response::new)
    }

    pub(super) async fn watch_job(
        &self,
        request: Request<JobWatchRequest>,
    ) -> Result<Response<super::BoxStream<JobEvent>>, Status> {
        let request = request.into_inner();
        let context = request.context.clone();
        let (caller, owner, id, permission) = self.job_access(context.as_ref(), &request.job_id)?;
        let id = id.to_owned();
        let slot = self.watch_slot(&caller.session_id)?;
        let (mut pending, mut over, mut changed) =
            self.jobs
                .events_after(&owner, &id, request.after_sequence)?;
        let (tx, rx) = mpsc::channel(WATCH_QUEUE);
        let providers = self.providers.clone();
        let jobs = self.jobs.clone();
        let mut position = request.after_sequence;
        tokio::spawn(async move {
            let _slot = slot;
            // Every delivery rechecks the session, profile and the creating permission; losing
            // it ends the stream with the denial rather than going on telling the plugin.
            let check = || providers.authorize_resource(context.as_ref(), permission);
            let mut recheck = tokio::time::interval(WATCH_RECHECK);
            recheck.tick().await;
            loop {
                for event in pending.drain(..) {
                    if let Err(status) = check() {
                        let _ = tx.send(Err(status)).await;
                        return;
                    }
                    position = event.sequence;
                    if tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                }
                if over {
                    return;
                }
                tokio::select! {
                    _ = changed.changed() => {}
                    _ = recheck.tick() => {
                        if let Err(status) = check() {
                            let _ = tx.send(Err(status)).await;
                            return;
                        }
                    }
                    () = tx.closed() => return,
                }
                match jobs.events_after(&owner, &id, position) {
                    Ok((events, finished, _)) => {
                        pending = events;
                        over = finished;
                    }
                    Err(status) => {
                        let _ = tx.send(Err(status)).await;
                        return;
                    }
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    /// Starts `work` as a job of `caller` for the operation that needs `permission`, or answers
    /// with the job the same idempotency key started before (`true`: a replay, nothing started).
    ///
    /// The work's fence is the same check every mutation makes at its commit: the permission is
    /// still held and the grants have not changed since the call began.
    // Called by the operations that return jobs (7.2 and later); tests exercise it meanwhile.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn start_job(
        &self,
        caller: &ResourceCaller,
        context: ServiceContext,
        permission: &'static str,
        key: &str,
        hash: [u8; 32],
        work: Work,
    ) -> Result<(JobReply, bool), Status> {
        let owner = JobOwner::of(caller)?;
        let providers = self.providers.clone();
        let grant_revision = caller.grant_revision;
        let fence: Fence = Arc::new(move || {
            let current = providers.authorize_resource(Some(&context), permission)?;
            if current.grant_revision == grant_revision {
                Ok(())
            } else {
                Err(Status::failed_precondition(
                    "plugin permissions changed; the job's effect was not committed",
                ))
            }
        });
        self.jobs.submit(
            &owner,
            Submission {
                permission,
                key: key.to_owned(),
                hash,
                fence,
                profile: self.providers.profiles().subscribe(),
            },
            work,
        )
    }
}
