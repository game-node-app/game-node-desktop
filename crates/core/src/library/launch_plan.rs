//! Immutable host-resolved launch plans and their versioned approval fingerprints.
//!
//! Fingerprints use an explicit field encoding. They never depend on protobuf/JSON serialization
//! order and include the literal effective target, argv, working directory, environment delta,
//! requester/package identity, ordered components, resource revisions and current grant revision.

use crate::drm_free::LaunchEnvironmentDelta;
use sha2::{Digest, Sha256};
pub const LAUNCH_PLAN_VERSION: u32 = 1;
const FINGERPRINT_DOMAIN: &[u8] = b"gamenode.launch-plan.approval.v1\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlan {
    pub version: u32,
    pub game_id: String,
    pub installation_id: String,
    pub profile_id: String,
    /// Store/provider identity or `manual`; never a plugin supplied display label.
    pub source_id: String,
    pub requester_plugin_id: String,
    pub requester_publisher: String,
    pub requester_package_version: String,
    pub target_revision: u64,
    pub recipe_id: String,
    pub recipe_revision: u64,
    pub grant_revision: u64,
    pub target: LaunchPlanTarget,
    /// Execution order is meaningful and therefore preserved in the canonical form.
    pub components: Vec<LaunchPlanComponent>,
    /// Resource identities/revisions are sorted by resource id in the canonical form.
    pub resources: Vec<LaunchPlanResource>,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub environment: LaunchEnvironmentDelta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchPlanTarget {
    Detected {
        fingerprint: u64,
    },
    Uri {
        uri: String,
        handler: Option<String>,
    },
    Executable {
        path: String,
        file_identity: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlanComponent {
    pub kind: String,
    pub id: String,
    pub owner_plugin_id: String,
    pub owner_publisher: String,
    pub version: String,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchPlanResource {
    pub id: String,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LaunchPlanError {
    #[error("the launch plan is invalid")]
    Invalid,
}

impl LaunchPlan {
    pub fn validate(&self) -> bool {
        let text = |value: &str, max: usize| {
            !value.is_empty()
                && value.len() <= max
                && !value.chars().any(|ch| ch == '\0' || ch.is_control())
        };
        self.version == LAUNCH_PLAN_VERSION
            && self.game_id.len() <= 128
            && !self.game_id.chars().any(char::is_control)
            && text(&self.installation_id, 128)
            && text(&self.profile_id, 256)
            && text(&self.source_id, 256)
            && text(&self.requester_plugin_id, 256)
            && text(&self.requester_publisher, 256)
            && text(&self.requester_package_version, 128)
            && match &self.target {
                LaunchPlanTarget::Detected { .. } => true,
                LaunchPlanTarget::Uri { uri, handler } => {
                    text(uri, 4096) && handler.as_ref().is_none_or(|handler| text(handler, 1024))
                }
                LaunchPlanTarget::Executable {
                    path,
                    file_identity,
                } => text(path, 4096) && text(file_identity, 1024),
            }
            && self.components.len() <= 128
            && self.components.iter().all(|component| {
                text(&component.kind, 128)
                    && text(&component.id, 512)
                    && text(&component.owner_plugin_id, 256)
                    && text(&component.owner_publisher, 256)
                    && component.version.len() <= 256
                    && !component.version.chars().any(char::is_control)
            })
            && self.resources.len() <= 128
            && self
                .resources
                .iter()
                .all(|resource| text(&resource.id, 512) && resource.revision > 0)
            && self.arguments.len() <= 256
            && self
                .arguments
                .iter()
                .all(|argument| argument.len() <= 16 * 1024 && !argument.contains('\0'))
            && self.working_directory.len() <= 4096
            && !self.working_directory.contains('\0')
            && self.environment.validate()
    }

    /// A lowercase SHA-256 digest with the canonical encoding version in its prefix.
    pub fn fingerprint(&self) -> Result<String, LaunchPlanError> {
        if !self.validate() {
            return Err(LaunchPlanError::Invalid);
        }
        let mut hash = Sha256::new();
        hash.update(FINGERPRINT_DOMAIN);
        number(&mut hash, self.version as u64);
        for field in [
            self.game_id.as_str(),
            self.installation_id.as_str(),
            self.profile_id.as_str(),
            self.source_id.as_str(),
            self.requester_plugin_id.as_str(),
            self.requester_publisher.as_str(),
            self.requester_package_version.as_str(),
        ] {
            part(&mut hash, field.as_bytes());
        }
        number(&mut hash, self.target_revision);
        part(&mut hash, self.recipe_id.as_bytes());
        number(&mut hash, self.recipe_revision);
        number(&mut hash, self.grant_revision);
        match &self.target {
            LaunchPlanTarget::Detected { fingerprint } => {
                part(&mut hash, b"detected");
                number(&mut hash, *fingerprint);
            }
            LaunchPlanTarget::Uri { uri, handler } => {
                part(&mut hash, b"uri");
                part(&mut hash, uri.as_bytes());
                optional(&mut hash, handler.as_deref());
            }
            LaunchPlanTarget::Executable {
                path,
                file_identity,
            } => {
                part(&mut hash, b"executable");
                part(&mut hash, path.as_bytes());
                part(&mut hash, file_identity.as_bytes());
            }
        }
        number(&mut hash, self.components.len() as u64);
        for component in &self.components {
            for field in [
                component.kind.as_str(),
                component.id.as_str(),
                component.owner_plugin_id.as_str(),
                component.owner_publisher.as_str(),
                component.version.as_str(),
            ] {
                part(&mut hash, field.as_bytes());
            }
            number(&mut hash, component.revision);
        }
        let mut resources = self.resources.iter().collect::<Vec<_>>();
        resources.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.revision.cmp(&right.revision))
        });
        number(&mut hash, resources.len() as u64);
        for resource in resources {
            part(&mut hash, resource.id.as_bytes());
            number(&mut hash, resource.revision);
        }
        number(&mut hash, self.arguments.len() as u64);
        for argument in &self.arguments {
            part(&mut hash, argument.as_bytes());
        }
        part(&mut hash, self.working_directory.as_bytes());
        number(&mut hash, self.environment.set.len() as u64);
        for (key, value) in &self.environment.set {
            part(&mut hash, key.as_bytes());
            part(&mut hash, value.as_bytes());
        }
        number(&mut hash, self.environment.unset.len() as u64);
        for key in &self.environment.unset {
            part(&mut hash, key.as_bytes());
        }
        let digest = hash.finalize();
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(format!("v{}:{hex}", self.version))
    }
}

fn part(hash: &mut Sha256, bytes: &[u8]) {
    number(hash, bytes.len() as u64);
    hash.update(bytes);
}

fn number(hash: &mut Sha256, value: u64) {
    hash.update(value.to_be_bytes());
}

fn optional(hash: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hash.update([1]);
            part(hash, value.as_bytes());
        }
        None => hash.update([0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn plan() -> LaunchPlan {
        LaunchPlan {
            version: LAUNCH_PLAN_VERSION,
            game_id: "42".into(),
            installation_id: "steam-install".into(),
            profile_id: "profile-a".into(),
            source_id: "steam".into(),
            requester_plugin_id: "org.example.runtime".into(),
            requester_publisher: "publisher-a".into(),
            requester_package_version: "1.2.3".into(),
            target_revision: 7,
            recipe_id: "recipe-a".into(),
            recipe_revision: 3,
            grant_revision: 8,
            target: LaunchPlanTarget::Executable {
                path: "/games/example/wrapper".into(),
                file_identity: "volume:inode".into(),
            },
            components: vec![LaunchPlanComponent {
                kind: "runtime".into(),
                id: "runtime-a".into(),
                owner_plugin_id: "org.example.runtime".into(),
                owner_publisher: "publisher-a".into(),
                version: "4.0".into(),
                revision: 2,
            }],
            resources: vec![LaunchPlanResource {
                id: "approval-a".into(),
                revision: 1,
            }],
            arguments: vec!["--profile".into(), "Two Words".into()],
            working_directory: "/games/example".into(),
            environment: LaunchEnvironmentDelta {
                set: BTreeMap::from([("MODE".into(), "performance".into())]),
                unset: BTreeSet::from(["REMOVE_ME".into()]),
            },
        }
    }

    #[test]
    fn fingerprint_is_stable_and_versioned() {
        let fingerprint = plan().fingerprint().unwrap();
        assert!(fingerprint.starts_with("v1:"));
        assert_eq!(plan().fingerprint().unwrap(), fingerprint);
    }

    #[test]
    fn every_approval_sensitive_field_changes_the_fingerprint() {
        let original = plan().fingerprint().unwrap();
        let mut variants = Vec::new();
        let mut changed = plan();
        changed.arguments.reverse();
        variants.push(changed);
        let mut changed = plan();
        changed
            .environment
            .set
            .insert("MODE".into(), "quiet".into());
        variants.push(changed);
        let mut changed = plan();
        changed.components[0].revision += 1;
        variants.push(changed);
        let mut changed = plan();
        changed.resources[0].revision += 1;
        variants.push(changed);
        let mut changed = plan();
        changed.grant_revision += 1;
        variants.push(changed);
        let mut changed = plan();
        changed.requester_package_version.push_str("+build2");
        variants.push(changed);
        let mut changed = plan();
        changed.target = LaunchPlanTarget::Uri {
            uri: "steam://rungameid/42".into(),
            handler: None,
        };
        variants.push(changed);
        assert!(
            variants
                .iter()
                .all(|variant| variant.fingerprint().unwrap() != original)
        );
    }

    #[test]
    fn resource_input_order_does_not_change_the_fingerprint() {
        let mut first = plan();
        first.resources.push(LaunchPlanResource {
            id: "approval-b".into(),
            revision: 9,
        });
        let mut second = first.clone();
        second.resources.reverse();
        assert_eq!(first.fingerprint().unwrap(), second.fingerprint().unwrap());
    }
}
