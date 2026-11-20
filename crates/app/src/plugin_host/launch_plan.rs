//! Host-owned resolution of an authenticated request into the plan that user approval covers.

use crate::plugin_host::library::{CustomLaunchDispatch, Projected};
use crate::plugin_host::provider::ResourceCaller;
use gamenode_core::library::{
    CustomLaunchRecipe, LaunchPlan, LaunchPlanComponent, LaunchPlanResource, LaunchPlanTarget,
};
use tonic::Status;

/// Plain, host-resolved data shown by the shared launch-plan review. Environment values are
/// included for exact review and remain masked in the UI until the user explicitly reveals them.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlanReview {
    pub source_id: String,
    pub requester_plugin_id: String,
    pub requester_publisher: String,
    pub requester_package_version: String,
    pub target: String,
    pub components: Vec<LaunchPlanReviewComponent>,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub environment_set: std::collections::BTreeMap<String, String>,
    pub environment_unset: Vec<String>,
    pub differences: Vec<String>,
    pub fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlanReviewComponent {
    pub kind: String,
    pub id: String,
    pub owner_plugin_id: String,
    pub owner_publisher: String,
    pub version: String,
    pub revision: u64,
}

pub fn review(plan: &LaunchPlan, fingerprint: String) -> LaunchPlanReview {
    let target = match &plan.target {
        LaunchPlanTarget::Detected { fingerprint } => {
            format!("Detected target (revision {fingerprint})")
        }
        LaunchPlanTarget::Uri { uri, handler } => handler.as_ref().map_or_else(
            || uri.clone(),
            |handler| format!("{uri} (handler: {handler})"),
        ),
        LaunchPlanTarget::Executable {
            path,
            file_identity,
        } => format!("{path} (file identity: {file_identity})"),
    };
    let mut differences = Vec::new();
    if !plan.recipe_id.is_empty() {
        differences.push("recipe-selected".into());
    }
    if !matches!(&plan.target, LaunchPlanTarget::Detected { .. }) {
        differences.push("custom-target".into());
    }
    if !plan.components.is_empty() {
        differences.push("known-components".into());
    }
    if !plan.arguments.is_empty() {
        differences.push("custom-arguments".into());
    }
    if !plan.working_directory.is_empty() {
        differences.push("custom-working-directory".into());
    }
    if !plan.environment.is_empty() {
        differences.push("environment-changes".into());
    }
    LaunchPlanReview {
        source_id: plan.source_id.clone(),
        requester_plugin_id: plan.requester_plugin_id.clone(),
        requester_publisher: plan.requester_publisher.clone(),
        requester_package_version: plan.requester_package_version.clone(),
        target,
        components: plan
            .components
            .iter()
            .map(|component| LaunchPlanReviewComponent {
                kind: component.kind.clone(),
                id: component.id.clone(),
                owner_plugin_id: component.owner_plugin_id.clone(),
                owner_publisher: component.owner_publisher.clone(),
                version: component.version.clone(),
                revision: component.revision,
            })
            .collect(),
        arguments: plan.arguments.clone(),
        working_directory: plan.working_directory.clone(),
        environment_set: plan.environment.set.clone(),
        environment_unset: plan.environment.unset.iter().cloned().collect(),
        differences,
        fingerprint,
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use gamenode_core::drm_free::LaunchEnvironmentDelta;

    #[test]
    fn executable_wrapper_plan_keeps_original_game_installation_and_source() {
        use crate::plugin_host::{
            grants::PluginIdentity,
            library::{CustomLaunchDispatch, InstallState},
            namespace::ExternalNamespaceResolver,
        };

        let identity = PluginIdentity {
            plugin_id: "org.example.wrapper".into(),
            publisher_fingerprint: "ab".repeat(32),
            profile_id: "account-1".into(),
        };
        let namespace = ExternalNamespaceResolver::new(identity, None, 1).unwrap();
        let caller = ResourceCaller {
            session_id: "session".into(),
            provider_id: String::new(),
            declared_developer: String::new(),
            origin: String::new(),
            package_digest: vec![],
            package_version: "1.0.0".into(),
            grants: Default::default(),
            grant_revision: 1,
            profile_epoch: 1,
            namespace: Some(namespace),
        };
        let entry = Projected {
            installation_id: "manual:original-installation".into(),
            provider_id: "official-store".into(),
            game_id: Some(42),
            name: "Original game".into(),
            install_state: InstallState::Installed,
            display_name: None,
            metadata_revision: 0,
            artwork_revision: 0,
            revision: 7,
        };
        let dispatch =
            CustomLaunchDispatch::Executable(gamenode_core::drm_free::CustomLaunchCommand {
                executable: "/approved/proton".into(),
                file_identity: "linux:1:2:3:4:5:6:7:8".into(),
                arguments: vec!["run".into(), "Z:/Games/original.exe".into()],
                working_directory: "/approved/prefix".into(),
                environment: LaunchEnvironmentDelta {
                    set: [("PROTON_LOG".into(), "1".into())].into(),
                    unset: Default::default(),
                },
            });

        let plan = resolve(&caller, &entry, 9, None, Some(&dispatch), &[]).unwrap();
        assert_eq!(plan.game_id, "42");
        assert_eq!(plan.installation_id, "manual:original-installation");
        assert_eq!(plan.source_id, "official-store");
        assert_eq!(plan.arguments, ["run", "Z:/Games/original.exe"]);
        assert_eq!(plan.working_directory, "/approved/prefix");
        assert_eq!(plan.environment.set["PROTON_LOG"], "1");
    }

    #[test]
    fn review_preserves_provenance_component_order_literals_and_environment_changes() {
        let plan = LaunchPlan {
            version: gamenode_core::library::LAUNCH_PLAN_VERSION,
            game_id: "620".into(),
            installation_id: "install-1".into(),
            profile_id: "profile-1".into(),
            source_id: "steam".into(),
            requester_plugin_id: "org.example.launcher".into(),
            requester_publisher: "ab".repeat(32),
            requester_package_version: "2.1.0".into(),
            target_revision: 4,
            recipe_id: "recipe-1".into(),
            recipe_revision: 3,
            grant_revision: 7,
            target: LaunchPlanTarget::Executable {
                path: "C:\\Games\\Portal 2\\Portal2.exe".into(),
                file_identity: "volume:file-id".into(),
            },
            components: vec![
                LaunchPlanComponent {
                    kind: "runtime".into(),
                    id: "proton".into(),
                    owner_plugin_id: "org.example.runtime".into(),
                    owner_publisher: "cd".repeat(32),
                    version: "9.0".into(),
                    revision: 2,
                },
                LaunchPlanComponent {
                    kind: "mod-layer".into(),
                    id: "compat-layer".into(),
                    owner_plugin_id: "org.example.mods".into(),
                    owner_publisher: "ef".repeat(32),
                    version: "1.4".into(),
                    revision: 5,
                },
            ],
            resources: vec![],
            arguments: vec!["--profile".into(), "a profile & literal".into()],
            working_directory: "C:\\Games\\Portal 2".into(),
            environment: LaunchEnvironmentDelta {
                set: [("GAME_MODE".into(), "custom".into())].into(),
                unset: ["OLD_MODE".into()].into(),
            },
        };
        let fingerprint = format!("v1:{}", "aa".repeat(32));
        let review = review(&plan, fingerprint.clone());
        assert_eq!(review.source_id, "steam");
        assert_eq!(review.requester_package_version, "2.1.0");
        assert_eq!(review.arguments, plan.arguments);
        assert_eq!(review.working_directory, plan.working_directory);
        assert_eq!(review.environment_set["GAME_MODE"], "custom");
        assert_eq!(review.environment_unset, ["OLD_MODE"]);
        assert_eq!(
            review
                .components
                .iter()
                .map(|part| part.id.as_str())
                .collect::<Vec<_>>(),
            ["proton", "compat-layer"]
        );
        assert_eq!(review.fingerprint, fingerprint);
        assert!(
            review
                .differences
                .iter()
                .any(|line| line == "custom-target")
        );
    }
}

pub(super) fn resolve(
    caller: &ResourceCaller,
    entry: &Projected,
    target_revision: u64,
    recipe: Option<&CustomLaunchRecipe>,
    dispatch: Option<&CustomLaunchDispatch>,
    resources: &[LaunchPlanResource],
) -> Result<LaunchPlan, Status> {
    let identity = caller
        .namespace
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("launch plans require a plugin namespace"))?
        .identity();
    let (target, arguments, working_directory, environment) = match dispatch {
        None => (
            LaunchPlanTarget::Detected {
                fingerprint: target_revision,
            },
            Vec::new(),
            String::new(),
            Default::default(),
        ),
        Some(CustomLaunchDispatch::Uri {
            uri,
            arguments,
            working_directory,
            environment,
        }) => (
            LaunchPlanTarget::Uri {
                uri: uri.as_str().to_owned(),
                handler: None,
            },
            arguments.clone(),
            working_directory.clone(),
            environment.clone(),
        ),
        Some(CustomLaunchDispatch::Executable(command)) => (
            LaunchPlanTarget::Executable {
                path: command.executable.clone(),
                file_identity: command.file_identity.clone(),
            },
            command.arguments.clone(),
            command.working_directory.clone(),
            command.environment.clone(),
        ),
        Some(CustomLaunchDispatch::Runtime {
            command,
            runtime_id,
            runtime_version,
            runtime_revision,
            content_id,
            binding_revision,
        }) => {
            let mut components = vec![LaunchPlanComponent {
                kind: "runtime".into(),
                id: runtime_id.clone(),
                owner_plugin_id: identity.plugin_id.clone(),
                owner_publisher: identity.publisher_fingerprint.clone(),
                version: runtime_version.clone(),
                revision: *runtime_revision,
            }];
            if !content_id.is_empty() {
                components.push(LaunchPlanComponent {
                    kind: "content".into(),
                    id: content_id.clone(),
                    owner_plugin_id: identity.plugin_id.clone(),
                    owner_publisher: identity.publisher_fingerprint.clone(),
                    version: String::new(),
                    revision: binding_revision.unwrap_or_default(),
                });
            }
            let mut resources = resources.to_vec();
            resources.push(LaunchPlanResource {
                id: format!("runtime:{runtime_id}"),
                revision: *runtime_revision,
            });
            if let Some(revision) = binding_revision {
                resources.push(LaunchPlanResource {
                    id: format!("content-binding:{content_id}"),
                    revision: *revision,
                });
            }
            return make(
                caller,
                identity,
                entry,
                target_revision,
                recipe,
                LaunchPlanTarget::Executable {
                    path: command.executable.clone(),
                    file_identity: command.file_identity.clone(),
                },
                command.arguments.clone(),
                command.working_directory.clone(),
                command.environment.clone(),
                components,
                resources,
            );
        }
    };
    let components = Vec::new();
    make(
        caller,
        identity,
        entry,
        target_revision,
        recipe,
        target,
        arguments,
        working_directory,
        environment,
        components,
        resources.to_vec(),
    )
}

#[allow(clippy::too_many_arguments)]
fn make(
    caller: &ResourceCaller,
    identity: &crate::plugin_host::grants::PluginIdentity,
    entry: &Projected,
    target_revision: u64,
    recipe: Option<&CustomLaunchRecipe>,
    target: LaunchPlanTarget,
    arguments: Vec<String>,
    working_directory: String,
    environment: gamenode_core::drm_free::LaunchEnvironmentDelta,
    components: Vec<LaunchPlanComponent>,
    mut resources: Vec<LaunchPlanResource>,
) -> Result<LaunchPlan, Status> {
    if let Some(recipe) = recipe {
        resources.push(LaunchPlanResource {
            id: format!("recipe:{}", recipe.recipe_id),
            revision: recipe.revision,
        });
        resources.push(LaunchPlanResource {
            id: format!("base-target:{}", entry.provider_id),
            revision: recipe.base_target_revision,
        });
    }
    let plan = LaunchPlan {
        version: gamenode_core::library::LAUNCH_PLAN_VERSION,
        game_id: entry.game_id.map(|id| id.to_string()).unwrap_or_default(),
        installation_id: entry.installation_id.clone(),
        profile_id: identity.profile_id.clone(),
        source_id: entry.provider_id.clone(),
        requester_plugin_id: identity.plugin_id.clone(),
        requester_publisher: identity.publisher_fingerprint.clone(),
        requester_package_version: caller.package_version.clone(),
        target_revision,
        recipe_id: recipe
            .map(|recipe| recipe.recipe_id.clone())
            .unwrap_or_default(),
        recipe_revision: recipe.map(|recipe| recipe.revision).unwrap_or_default(),
        grant_revision: caller.grant_revision,
        target,
        components,
        resources,
        arguments,
        working_directory,
        environment,
    };
    if !plan.validate() {
        return Err(Status::failed_precondition("the launch plan is invalid"));
    }
    Ok(plan)
}
