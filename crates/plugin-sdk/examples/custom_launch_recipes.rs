//! Typed recipe lifecycle for `game.custom_launch`.
//!
//! This API is separate from standard `game.launch`: a missing custom-launch grant returns
//! `PERMISSION_DENIED`, and the SDK never retries through the standard launch route. Handles are
//! opaque values returned by approved host resources, not filesystem paths supplied by a plugin.
#![allow(dead_code)]

use gamenode_plugin_sdk::{
    CustomLaunchRecipe, Resources, Status, idempotency_key, proto::CustomLaunchReply,
};

/// Propose a custom executable recipe with literal argv, a host-issued runtime handle and an
/// explicit environment delta. The host validates and reviews the recipe before it can run.
#[allow(clippy::too_many_arguments)] // Example mirrors the deliberate positional custom-launch API.
pub async fn propose_runtime_recipe(
    resources: &Resources,
    game_id: &str,
    installation_id: &str,
    content_id: &str,
    game_revision: u64,
    executable_handle: &str,
    working_directory_handle: &str,
    runtime_handle: &str,
) -> Result<CustomLaunchReply, Status> {
    let recipe = CustomLaunchRecipe {
        game_id: game_id.into(),
        installation_id: installation_id.into(),
        content_id: content_id.into(),
        expected_revision: game_revision,
        executable_handle: executable_handle.into(),
        working_directory_handle: working_directory_handle.into(),
        runtime_handle: runtime_handle.into(),
        argv: vec!["--game".into(), "620".into()],
        environment_set: [("GAME_MODE".into(), "compatibility".into())].into(),
        environment_unset: vec!["UNSAFE_INHERITED_SETTING".into()],
        ..Default::default()
    };
    resources
        .propose_custom_launch(&recipe, "", &idempotency_key())
        .await
}

/// Execute only the exact host-approved recipe revision. The request has no argv, executable,
/// working-directory, runtime or environment fields to replace the plan that was reviewed.
pub async fn request_approved_recipe(
    resources: &Resources,
    game_id: &str,
    installation_id: &str,
    game_revision: u64,
    recipe_id: &str,
    recipe_revision: u64,
    user_intent_id: &str,
) -> Result<CustomLaunchReply, Status> {
    resources
        .request_custom_launch(
            game_id,
            installation_id,
            game_revision,
            recipe_id,
            recipe_revision,
            user_intent_id,
            &idempotency_key(),
        )
        .await
}

/// Remove an owned recipe with the revision returned by the proposal/read operation.
pub async fn remove_recipe(
    resources: &Resources,
    game_id: &str,
    recipe_id: &str,
    recipe_revision: u64,
) -> Result<(), Status> {
    resources
        .remove_custom_launch(game_id, recipe_id, recipe_revision, &idempotency_key())
        .await?;
    Ok(())
}

fn main() {}
