//! Host-owned resolution of an authenticated request into the plan that user approval covers.

use crate::plugin_host::library::{CustomLaunchDispatch, Projected};
use crate::plugin_host::provider::ResourceCaller;
use gamenode_core::library::{
    CustomLaunchRecipe, LaunchPlan, LaunchPlanComponent, LaunchPlanResource, LaunchPlanTarget,
};
use tonic::Status;

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
