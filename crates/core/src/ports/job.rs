//! The PC port install job phase machine (design D5).
//!
//! ```text
//! Queued -> Resolving -> Downloading -> Verifying -> Extracting
//!        -> SuppliedFiles(slot i: Hashing | Extracting pkg | Copying)
//!        -> Finalizing -> Registering -> Completed
//!        -> Finalizing -> AwaitingExecutable -> Registering -> Completed
//!   any unfinished -> Cancelling -> Cancelled
//!   any unfinished -> Failed(reason)
//!   any unfinished -> Interrupted(reason)   // startup recovery, after cleanup
//! ```
//!
//! One job runs at a time; the rest queue. Every phase change goes through
//! [`PortJobPhase::transition`], which is how an out-of-order phase is caught in tests rather than
//! in the field.

use serde::{Deserialize, Serialize};

/// Where an install job is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PortJobPhase {
    /// Waiting behind the running job.
    Queued,
    /// Reading the entry, release and overlay into a [`crate::ports::plan::PortInstallPlan`].
    Resolving,
    /// Downloading the release asset.
    Downloading,
    /// Checking the download's SHA-256 and caching it.
    Verifying,
    /// Extracting the release archive, or copying a bare executable, into the staging folder.
    Extracting,
    /// Working on one supplied-file slot.
    SuppliedFiles { slot: String, step: SuppliedStep },
    /// Renaming the staging folder into place and writing the external-root copies.
    Finalizing,
    /// The files are in place, but none of the preferred executables exists. The install waits for
    /// the user to pick an executable inside the folder; it is not terminal and registers later.
    AwaitingExecutable,
    /// Inserting the DRM-free registration; the commit point.
    Registering,
    /// The registration is committed and the files are in place.
    Completed,
    /// Cancellation was requested; the worker is cleaning up.
    Cancelling,
    /// Cancelled jobs cleaned up after themselves and registered nothing.
    Cancelled,
    /// The job stopped with an error and cleaned up after itself.
    Failed { reason: String },
    /// Startup recovery found an unfinished job, removed its partial work and kept its downloads.
    Interrupted { reason: String },
}

/// The step of a supplied-file slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SuppliedStep {
    /// Hashing the user's file, or a folder's marker file.
    Hashing,
    /// Running the pinned extraction tool on console packages.
    ExtractingPackage,
    /// Copying the accepted file or folder into its destination.
    Copying,
}

impl PortJobPhase {
    /// True once the job can no longer change phase.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Failed { .. } | Self::Interrupted { .. }
        )
    }

    /// True while the job is still expected to do something, including while cancelling.
    pub fn is_unfinished(&self) -> bool {
        !self.is_terminal()
    }

    /// True when the job is queued and has not started.
    pub fn is_queued(&self) -> bool {
        matches!(self, Self::Queued)
    }

    /// A stable key for snapshots, logs and the UI. Phases that carry data keep the key of their
    /// variant; the data is rendered separately.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Resolving => "resolving",
            Self::Downloading => "downloading",
            Self::Verifying => "verifying",
            Self::Extracting => "extracting",
            Self::SuppliedFiles { .. } => "suppliedFiles",
            Self::Finalizing => "finalizing",
            Self::AwaitingExecutable => "awaitingExecutable",
            Self::Registering => "registering",
            Self::Completed => "completed",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Failed { .. } => "failed",
            Self::Interrupted { .. } => "interrupted",
        }
    }

    /// Whether the job may move to `next`.
    ///
    /// Forward steps follow the pipeline. Any unfinished phase may enter [`Self::Cancelling`],
    /// [`Self::Failed`] or [`Self::Interrupted`]; a terminal phase may not move at all.
    pub fn can_transition_to(&self, next: &Self) -> bool {
        if self.is_terminal() {
            return false;
        }
        match next {
            Self::Failed { .. } | Self::Interrupted { .. } => return true,
            Self::Cancelling => return !matches!(self, Self::Cancelling),
            _ => {}
        }
        matches!(
            (self, next),
            (Self::Queued, Self::Resolving)
                | (Self::Resolving, Self::Downloading)
                | (Self::Downloading, Self::Verifying)
                | (Self::Verifying, Self::Extracting)
                | (Self::Extracting, Self::SuppliedFiles { .. })
                | (Self::Extracting, Self::Finalizing)
                | (Self::SuppliedFiles { .. }, Self::SuppliedFiles { .. })
                | (Self::SuppliedFiles { .. }, Self::Finalizing)
                | (Self::Finalizing, Self::Registering)
                | (Self::Finalizing, Self::AwaitingExecutable)
                | (Self::AwaitingExecutable, Self::Registering)
                | (Self::Registering, Self::Completed)
                | (Self::Cancelling, Self::Cancelled)
        )
    }

    /// The next phase, or [`TransitionError`] when the move is not legal.
    pub fn transition(&self, next: Self) -> Result<Self, TransitionError> {
        if self.can_transition_to(&next) {
            Ok(next)
        } else {
            Err(TransitionError {
                from: self.clone(),
                to: next,
            })
        }
    }
}

impl std::fmt::Display for PortJobPhase {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SuppliedFiles { slot, step } => {
                write!(formatter, "{} ({slot}: {step})", self.label())
            }
            Self::Failed { reason } | Self::Interrupted { reason } => {
                write!(formatter, "{} ({reason})", self.label())
            }
            _ => formatter.write_str(self.label()),
        }
    }
}

impl std::fmt::Display for SuppliedStep {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Hashing => "hashing",
            Self::ExtractingPackage => "extractingPackage",
            Self::Copying => "copying",
        })
    }
}

/// An illegal phase change. It means the job runner lost track of its own state.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a port install job in phase {from} cannot move to {to}")]
pub struct TransitionError {
    pub from: PortJobPhase,
    pub to: PortJobPhase,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supplied(slot: &str, step: SuppliedStep) -> PortJobPhase {
        PortJobPhase::SuppliedFiles {
            slot: slot.into(),
            step,
        }
    }

    fn full_chain() -> Vec<PortJobPhase> {
        vec![
            PortJobPhase::Queued,
            PortJobPhase::Resolving,
            PortJobPhase::Downloading,
            PortJobPhase::Verifying,
            PortJobPhase::Extracting,
            supplied("game", SuppliedStep::Hashing),
            supplied("game", SuppliedStep::ExtractingPackage),
            supplied("game", SuppliedStep::Copying),
            supplied("extra", SuppliedStep::Hashing),
            supplied("extra", SuppliedStep::Copying),
            PortJobPhase::Finalizing,
            PortJobPhase::Registering,
            PortJobPhase::Completed,
        ]
    }

    /// The same pipeline when the preferred executable is missing and the user picks one.
    fn awaiting_chain() -> Vec<PortJobPhase> {
        vec![
            PortJobPhase::Queued,
            PortJobPhase::Resolving,
            PortJobPhase::Downloading,
            PortJobPhase::Verifying,
            PortJobPhase::Extracting,
            PortJobPhase::Finalizing,
            PortJobPhase::AwaitingExecutable,
            PortJobPhase::Registering,
            PortJobPhase::Completed,
        ]
    }

    #[test]
    fn the_whole_pipeline_is_legal_in_order() {
        for chain in [full_chain(), awaiting_chain()] {
            for pair in chain.windows(2) {
                assert!(
                    pair[0].can_transition_to(&pair[1]),
                    "{} -> {} should be legal",
                    pair[0],
                    pair[1]
                );
                assert_eq!(pair[0].transition(pair[1].clone()), Ok(pair[1].clone()));
            }
            assert!(chain.last().unwrap().is_terminal());
            assert!(
                chain[..chain.len() - 1]
                    .iter()
                    .all(PortJobPhase::is_unfinished)
            );
        }
    }

    #[test]
    fn awaiting_an_executable_is_not_terminal_and_only_registers_forwards() {
        let awaiting = PortJobPhase::AwaitingExecutable;
        assert!(!awaiting.is_terminal());
        assert!(awaiting.is_unfinished());
        assert!(awaiting.can_transition_to(&PortJobPhase::Registering));
        assert!(!awaiting.can_transition_to(&PortJobPhase::Completed));
        assert!(!awaiting.can_transition_to(&PortJobPhase::Finalizing));
        assert!(!PortJobPhase::Finalizing.can_transition_to(&PortJobPhase::Completed));
        // A late pick never goes back to waiting once registration started.
        assert!(!PortJobPhase::Registering.can_transition_to(&PortJobPhase::AwaitingExecutable));
    }

    #[test]
    fn steps_cannot_skip_phases_or_go_back() {
        let illegal = [
            (PortJobPhase::Queued, PortJobPhase::Downloading),
            (PortJobPhase::Downloading, PortJobPhase::Resolving),
            (PortJobPhase::Extracting, PortJobPhase::Registering),
            (PortJobPhase::Finalizing, PortJobPhase::Extracting),
            (PortJobPhase::Registering, PortJobPhase::Finalizing),
            (PortJobPhase::AwaitingExecutable, PortJobPhase::Finalizing),
            (PortJobPhase::Registering, PortJobPhase::AwaitingExecutable),
            (PortJobPhase::Cancelling, PortJobPhase::Completed),
            (PortJobPhase::Cancelling, PortJobPhase::Cancelling),
            (PortJobPhase::Completed, PortJobPhase::Resolving),
            (
                PortJobPhase::Cancelled,
                PortJobPhase::Failed { reason: "x".into() },
            ),
            (
                PortJobPhase::Failed { reason: "x".into() },
                PortJobPhase::Interrupted { reason: "y".into() },
            ),
        ];
        for (from, to) in illegal {
            assert!(
                !from.can_transition_to(&to),
                "{} -> {} should be illegal",
                from,
                to
            );
            assert!(from.transition(to).is_err());
        }
        let error = PortJobPhase::Queued
            .transition(PortJobPhase::Completed)
            .unwrap_err();
        assert_eq!(error.from, PortJobPhase::Queued);
        assert_eq!(error.to, PortJobPhase::Completed);
    }

    #[test]
    fn any_unfinished_phase_can_cancel_fail_or_interrupt() {
        let every_phase = |phase: &PortJobPhase| {
            assert!(
                phase.can_transition_to(&PortJobPhase::Cancelling),
                "{} cancelling",
                phase
            );
            assert!(
                phase.can_transition_to(&PortJobPhase::Failed {
                    reason: "boom".into()
                }),
                "{} failing",
                phase
            );
            assert!(
                phase.can_transition_to(&PortJobPhase::Interrupted {
                    reason: "quit".into()
                }),
                "{} interrupting",
                phase
            );
        };
        for phase in full_chain().into_iter().chain(awaiting_chain()) {
            if phase.is_terminal() {
                assert!(!phase.can_transition_to(&PortJobPhase::Cancelling));
                continue;
            }
            every_phase(&phase);
        }
    }

    #[test]
    fn cancelling_ends_only_as_cancelled_failed_or_interrupted() {
        let cancelling = PortJobPhase::Cancelling;
        assert!(cancelling.can_transition_to(&PortJobPhase::Cancelled));
        assert!(cancelling.can_transition_to(&PortJobPhase::Failed {
            reason: "cleanup failed".into()
        }));
        assert!(cancelling.can_transition_to(&PortJobPhase::Interrupted {
            reason: "quit during cleanup".into()
        }));
        assert!(!cancelling.can_transition_to(&PortJobPhase::Completed));
        assert!(!cancelling.is_terminal());
    }

    #[test]
    fn labels_are_stable_and_data_phases_render_their_data() {
        assert_eq!(PortJobPhase::Queued.label(), "queued");
        assert_eq!(PortJobPhase::Finalizing.label(), "finalizing");
        assert_eq!(
            PortJobPhase::AwaitingExecutable.label(),
            "awaitingExecutable"
        );
        assert_eq!(
            PortJobPhase::AwaitingExecutable.to_string(),
            "awaitingExecutable"
        );
        assert_eq!(
            supplied("game", SuppliedStep::Copying).label(),
            "suppliedFiles"
        );
        assert_eq!(
            supplied("game", SuppliedStep::Copying).to_string(),
            "suppliedFiles (game: copying)"
        );
        assert_eq!(
            PortJobPhase::Failed {
                reason: "disk full".into()
            }
            .to_string(),
            "failed (disk full)"
        );
        assert_eq!(
            SuppliedStep::ExtractingPackage.to_string(),
            "extractingPackage"
        );
    }
}
