//! Practical recipes for the host action services: launch/install, backups, tools and ports.
//!
//! These are plain functions over [`Actions`] (from `HostClient::actions()`); copy what you need.
//! They are compiled with the SDK and run against the desktop's fixture host in its service
//! tests, so they stay true to what the host does. Run `cargo doc` for the rules each call
//! follows; the permissions document explains every status code.
//!
//! An action answers with a job. Core may ask the user first, and the job stays `QUEUED` with
//! the message `awaiting confirmation` while the dialog is open. [`wait_for_job`] follows it to
//! a terminal state. A decline, a timeout or a revoked permission ends it `FAILED` without
//! committing anything; [`cancel`] withdraws a request that still waits.
#![allow(dead_code)]

use gamenode_plugin_sdk::{Actions, Code, Status, idempotency_key, proto::JobState};
use tokio_stream::StreamExt;

/// The terminal state of a job and the host's stable code for it. Show `message` to people, but
/// branch on `state` and `result_code`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    pub state: JobState,
    pub result_code: String,
    pub message: String,
}

/// Whether a job can no longer change.
pub fn is_terminal(state: JobState) -> bool {
    matches!(
        state,
        JobState::Succeeded | JobState::Failed | JobState::Cancelled
    )
}

/// Follows a job to its terminal state. A watch that ends early falls back to one `JobsGet`, so a
/// lost event does not hide the outcome. `PERMISSION_DENIED` means the operation's permission was
/// revoked: the job committed nothing, and the user has to approve the access again.
pub async fn wait_for_job(actions: &Actions, job_id: &str) -> Result<Finished, Status> {
    let mut events = actions.watch_job(job_id, 0).await?;
    while let Some(event) = events.next().await {
        let event = event?;
        if is_terminal(event.state()) {
            return Ok(Finished {
                state: event.state(),
                result_code: event.result_code,
                message: event.message,
            });
        }
    }
    let reply = actions.job(job_id).await?;
    if is_terminal(reply.state()) {
        Ok(Finished {
            state: reply.state(),
            result_code: reply.result_code,
            message: reply.message,
        })
    } else {
        Err(Status::unavailable(
            "the job watch ended before a terminal state",
        ))
    }
}

/// Withdraws a request that still waits for the user. A job that already runs may answer
/// `RUNNING` and report what it finally did; do not assume the effect was undone.
pub async fn cancel(actions: &Actions, job_id: &str) -> Result<Finished, Status> {
    let reply = actions.cancel_job(job_id, &idempotency_key()).await?;
    Ok(Finished {
        state: reply.state(),
        result_code: reply.result_code,
        message: reply.message,
    })
}

fn job_started(reply: gamenode_plugin_sdk::proto::JobReply) -> Result<String, Status> {
    if reply.job_id.is_empty() {
        Err(Status::failed_precondition("the host returned no job"))
    } else {
        Ok(reply.job_id)
    }
}

/// Starts an installed game through its detected target and waits for the result. Core asks the
/// user unless this call redeems a host-issued intent. `expected_revision` is the entry's
/// `revision`; a changed target is refused rather than silently replaced.
pub async fn launch(
    actions: &Actions,
    installation_id: &str,
    expected_revision: u64,
) -> Result<Finished, Status> {
    let reply = actions
        .launch_game(
            "",
            installation_id,
            expected_revision,
            "",
            &idempotency_key(),
        )
        .await?;
    if reply.job_id.is_empty() {
        return Err(Status::failed_precondition(
            if reply.refusal_code.is_empty() {
                "the launch was refused".to_owned()
            } else {
                reply.refusal_code
            },
        ));
    }
    wait_for_job(actions, &reply.job_id).await
}

/// Opens the provider's install flow for a game and waits for the result. The user confirms the
/// install in Core; a plugin cannot choose installer inputs.
pub async fn install(
    actions: &Actions,
    game_id: &str,
    installation_id: &str,
) -> Result<Finished, Status> {
    let job = actions
        .install_game(game_id, installation_id, "", &idempotency_key())
        .await?;
    wait_for_job(actions, &job_started(job)?).await
}

/// Applies or updates a host-catalog tool on one installed game and waits for the result.
pub async fn apply_tool(
    actions: &Actions,
    tool_id: &str,
    game_id: &str,
    expected_revision: u64,
) -> Result<Finished, Status> {
    let job = actions
        .apply_tool(tool_id, game_id, expected_revision, &idempotency_key())
        .await?;
    wait_for_job(actions, &job_started(job)?).await
}

/// Removes a host-catalog tool from one installed game and waits for the result.
pub async fn remove_tool(
    actions: &Actions,
    tool_id: &str,
    game_id: &str,
    expected_revision: u64,
) -> Result<Finished, Status> {
    let job = actions
        .remove_tool(tool_id, game_id, expected_revision, &idempotency_key())
        .await?;
    wait_for_job(actions, &job_started(job)?).await
}

/// Installs a host-catalog port and waits for the result. The user chooses the destination,
/// sources and risk acknowledgements in Core's setup window.
pub async fn install_port(
    actions: &Actions,
    port_id: &str,
    game_id: &str,
    supplied_file_handles: Vec<String>,
) -> Result<Finished, Status> {
    let job = actions
        .install_port(
            port_id,
            game_id,
            supplied_file_handles,
            0,
            &idempotency_key(),
        )
        .await?;
    wait_for_job(actions, &job_started(job)?).await
}

/// Backs up an approved save profile and waits for the result.
pub async fn create_backup(
    actions: &Actions,
    game_id: &str,
    profile_id: &str,
    expected_revision: u64,
) -> Result<Finished, Status> {
    let job = actions
        .create_backup(
            game_id,
            profile_id,
            "Requested by the plugin",
            expected_revision,
            &idempotency_key(),
        )
        .await?;
    wait_for_job(actions, &job_started(job)?).await
}

/// Restores one retained backup. Core asks the user to confirm the exact targets before anything
/// is written; the plugin cannot remember or reuse that consent.
pub async fn restore_backup(
    actions: &Actions,
    backup_id: &str,
    game_id: &str,
    profile_id: &str,
    expected_revision: u64,
) -> Result<Finished, Status> {
    let job = actions
        .restore_backup(
            backup_id,
            game_id,
            profile_id,
            expected_revision,
            &idempotency_key(),
        )
        .await?;
    wait_for_job(actions, &job_started(job)?).await
}

/// A small summary of a failed job for a user-facing message. `message` is untrusted text from
/// the host and must be rendered as plain text.
pub fn describe(finished: &Finished) -> Option<String> {
    if finished.state == JobState::Failed {
        Some(if finished.message.is_empty() {
            format!("The operation failed ({})", finished.result_code)
        } else {
            finished.message.clone()
        })
    } else {
        None
    }
}

/// Whether a status means the plugin should ask the user to approve access again instead of
/// retrying: the permission was revoked or was never granted.
pub fn needs_new_approval(status: &Status) -> bool {
    status.code() == Code::PermissionDenied
}

fn main() {}
