//! Durable scheduler transitions. The host persists before executing and supplies fresh observations.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    Running,
    Stopped,
    Uncertain,
}
pub fn automatic_job(
    id: String,
    installation: &Registration,
    profile: &SaveProfile,
    settings: &BackupSettings,
    period: String,
    last_exit: i64,
) -> Option<BackupJob> {
    if installation.removed
        || !settings.automatic
        || profile.automatic == Some(false)
        || !matches!(
            profile.support,
            SaveSupport::Supported | SaveSupport::NoSaves
        )
    {
        return None;
    }
    Some(BackupJob {
        id,
        installation: installation.id.clone(),
        profile: profile.id.clone(),
        revision: profile.revision,
        play_period: Some(period),
        destination: settings.destination.clone(),
        state: JobState::Queued,
        attempts: 0,
        ready_at: last_exit.saturating_add(13),
        backup_id: None,
        error: None,
    })
}
pub fn ready(
    job: &BackupJob,
    profile: &SaveProfile,
    settings: &BackupSettings,
    observation: Observation,
    now: i64,
) -> bool {
    job.state == JobState::Queued
        && job.profile == profile.id
        && job.revision == profile.revision
        && now >= job.ready_at
        && observation == Observation::Stopped
        && (job.play_period.is_none() || (settings.automatic && profile.automatic != Some(false)))
}
pub fn failed(job: &mut BackupJob, error: String, now: i64) {
    job.error = Some(error);
    if let Some(delay) = [30, 120, 600].get(job.attempts as usize) {
        job.ready_at = now + delay;
        job.attempts += 1;
        job.state = JobState::Queued;
    } else {
        job.state = JobState::Failed;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retries_are_bounded_and_uncertainty_never_authorizes_work() {
        let mut job = BackupJob {
            id: "j".into(),
            installation: "i".into(),
            profile: "p".into(),
            revision: 1,
            play_period: Some("period".into()),
            destination: "local".into(),
            state: JobState::Running,
            attempts: 0,
            ready_at: 0,
            backup_id: None,
            error: None,
        };
        for (now, expected) in [(1, 31), (32, 152), (153, 753)] {
            failed(&mut job, "offline".into(), now);
            assert_eq!(job.ready_at, expected);
            assert_eq!(job.state, JobState::Queued);
        }
        failed(&mut job, "offline".into(), 754);
        assert_eq!(job.state, JobState::Failed);
    }
}
