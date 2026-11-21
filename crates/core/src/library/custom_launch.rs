//! Typed plugin-owned launch recipes. Plugin-provided text is data; only host-resolved handles may
//! become executable command paths.

use crate::drm_free::{LaunchEnvironment, LaunchEnvironmentDelta};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_RECIPE_ARGUMENTS: usize = 128;
pub const MAX_RECIPE_ARGUMENT_BYTES: usize = 4096;
pub const MAX_RECIPE_TEXT_BYTES: usize = 128;
pub const MAX_RECIPE_MOD_LAYERS: usize = 32;

/// A declared launch-time mod layer. The host cannot inspect mod files, so the identity and
/// version are plugin-provided text covered by the reviewed plan; the layer's argument and
/// environment contributions are host-validated and applied in declaration order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomLaunchModLayer {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub version: String,
    /// Changes whenever the plugin replaces this layer; a changed value requires new review.
    pub revision: u64,
    pub arguments: Vec<String>,
    pub environment_set: LaunchEnvironment,
    pub environment_unset: BTreeSet<String>,
}

impl CustomLaunchModLayer {
    pub fn environment(&self) -> LaunchEnvironmentDelta {
        LaunchEnvironmentDelta {
            set: self.environment_set.clone(),
            unset: self.environment_unset.clone(),
        }
    }

    pub fn validate(&self) -> bool {
        let text_ok = |value: &str| {
            value.len() <= MAX_RECIPE_TEXT_BYTES && !value.chars().any(char::is_control)
        };
        !self.id.is_empty()
            && text_ok(&self.id)
            && text_ok(&self.name)
            && text_ok(&self.version)
            && self.revision > 0
            && arguments_ok(&self.arguments)
            && self.environment().validate()
    }
}

fn arguments_ok(arguments: &[String]) -> bool {
    arguments.len() <= MAX_RECIPE_ARGUMENTS
        && arguments
            .iter()
            .all(|argument| argument.len() <= MAX_RECIPE_ARGUMENT_BYTES && !argument.contains('\0'))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomLaunchRecipe {
    /// Opaque id minted by Core when the recipe is proposed.
    pub recipe_id: String,
    /// Changes whenever the stored recipe is replaced.
    pub revision: u64,
    /// Package version that authored the recipe, copied from the authenticated caller.
    #[serde(default)]
    pub requester_package_version: String,
    pub game_id: u64,
    pub installation_id: String,
    /// Revision of the detected target this recipe was based on.
    pub base_target_revision: u64,
    /// Fingerprint of the provider's effective launch action when this recipe was proposed.
    #[serde(default)]
    pub base_launch_fingerprint: u64,
    /// Exactly one direct alternate executable handle or URI, unless a runtime wraps the default.
    pub executable_handle: String,
    /// Durable host-owned file reference captured from `executable_handle` when proposed.
    /// It contains an approval id and relative path, never an absolute path or session token.
    #[serde(default)]
    pub executable: Option<super::runtime::ApprovedFileReference>,
    pub uri: String,
    pub arguments: Vec<String>,
    pub working_directory_handle: String,
    /// Durable host-owned file reference captured from `working_directory_handle` when proposed.
    #[serde(default)]
    pub working_directory: Option<super::runtime::ApprovedFileReference>,
    pub runtime_handle: String,
    /// Stable selected-content identity when a runtime binding requires a content item.
    #[serde(default)]
    pub content_id: String,
    pub environment_set: LaunchEnvironment,
    pub environment_unset: BTreeSet<String>,
    /// Declared mod layers applied after the recipe arguments, in order. They are reviewed as
    /// components and contribute their arguments/environment changes to the effective plan.
    #[serde(default)]
    pub mod_layers: Vec<CustomLaunchModLayer>,
}

impl CustomLaunchRecipe {
    pub fn environment(&self) -> LaunchEnvironmentDelta {
        LaunchEnvironmentDelta {
            set: self.environment_set.clone(),
            unset: self.environment_unset.clone(),
        }
    }

    /// The exact argument vector after applying declared mod layers in order.
    pub fn effective_arguments(&self) -> Vec<String> {
        let mut arguments = self.arguments.clone();
        for layer in &self.mod_layers {
            arguments.extend(layer.arguments.iter().cloned());
        }
        arguments
    }

    /// The exact environment delta after applying declared mod layers in order. A later layer
    /// replaces an earlier `set` for the same key and removes a key it `unset`s.
    pub fn effective_environment(&self) -> LaunchEnvironmentDelta {
        let mut environment = self.environment();
        for layer in &self.mod_layers {
            let layer = layer.environment();
            for (key, value) in layer.set {
                environment.unset.remove(&key);
                environment.set.insert(key, value);
            }
            for key in layer.unset {
                environment.set.remove(&key);
                environment.unset.insert(key);
            }
        }
        environment
    }

    pub fn validate(&self) -> bool {
        let handle_ok = |value: &str| {
            value.len() <= MAX_RECIPE_TEXT_BYTES
                && !value.chars().any(|ch| ch.is_control() || ch == '\0')
        };
        let has_executable = !self.executable_handle.is_empty() || self.executable.is_some();
        let has_uri = !self.uri.is_empty();
        let effective_arguments = self.effective_arguments();
        let effective_environment = self.effective_environment();
        let parameter_overlay = !effective_arguments.is_empty()
            || !effective_environment.is_empty()
            || (!self.working_directory_handle.is_empty() || self.working_directory.is_some());
        let mut layer_ids = BTreeSet::new();
        if !handle_ok(&self.recipe_id)
            || self.recipe_id.is_empty()
            || self.revision == 0
            || self.requester_package_version.len() > 128
            || self.requester_package_version.chars().any(char::is_control)
            || self.game_id == 0
            || !handle_ok(&self.installation_id)
            || self.installation_id.is_empty()
            || self.base_target_revision == 0
            || self.base_launch_fingerprint == 0
            || !handle_ok(&self.executable_handle)
            || !handle_ok(&self.working_directory_handle)
            || !handle_ok(&self.runtime_handle)
            || !handle_ok(&self.content_id)
            || !arguments_ok(&self.arguments)
            || self.mod_layers.len() > MAX_RECIPE_MOD_LAYERS
            || self
                .mod_layers
                .iter()
                .any(|layer| !layer.validate() || !layer_ids.insert(layer.id.as_str()))
            || !arguments_ok(&effective_arguments)
            || !effective_environment.validate()
            || (!has_executable && !has_uri && self.runtime_handle.is_empty() && !parameter_overlay)
            || (has_executable && has_uri)
            || (!self.runtime_handle.is_empty() && has_executable)
            || (!self.executable_handle.is_empty() && self.executable.is_some())
            || (!self.working_directory_handle.is_empty() && self.working_directory.is_some())
            || self
                .executable
                .as_ref()
                .is_some_and(|reference| !reference.validate())
            || self
                .working_directory
                .as_ref()
                .is_some_and(|reference| !reference.validate())
        {
            return false;
        }
        if has_uri {
            let target = super::target::TargetSpec::Uri {
                uri: self.uri.clone(),
            };
            // Custom fields are retained on the recipe and passed to UriOpener::open_custom.
            // The platform adapter decides whether its handler can honor them and returns
            // Unsupported when it cannot. A runtime wrapper has no meaning for a URI target.
            return target.validate_shape() && self.runtime_handle.is_empty();
        }
        has_executable || !self.runtime_handle.is_empty() || parameter_overlay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri_recipe() -> CustomLaunchRecipe {
        CustomLaunchRecipe {
            recipe_id: "recipe-1".into(),
            revision: 1,
            requester_package_version: "1.0".into(),
            game_id: 620,
            installation_id: "install-1".into(),
            base_target_revision: 3,
            base_launch_fingerprint: 9,
            executable_handle: String::new(),
            executable: None,
            uri: "steam://rungameid/620".into(),
            arguments: vec!["--custom".into()],
            working_directory_handle: "approved-cwd".into(),
            working_directory: None,
            runtime_handle: String::new(),
            content_id: String::new(),
            environment_set: [("CUSTOM_VALUE".into(), "value".into())].into(),
            environment_unset: ["REMOVE_VALUE".into()].into(),
            mod_layers: Vec::new(),
        }
    }

    fn mod_layer() -> CustomLaunchModLayer {
        CustomLaunchModLayer {
            id: "texture-hd".into(),
            name: "HD texture layer".into(),
            version: "1.2".into(),
            revision: 3,
            arguments: vec!["--mod=texture-hd".into()],
            environment_set: [("MOD_LAYER".into(), "texture-hd".into())].into(),
            environment_unset: ["VANILLA".into()].into(),
        }
    }

    #[test]
    fn recipes_without_a_content_id_remain_compatible() {
        let mut value = serde_json::to_value(uri_recipe()).unwrap();
        value.as_object_mut().unwrap().remove("contentId");
        let recipe: CustomLaunchRecipe = serde_json::from_value(value).unwrap();
        assert!(recipe.content_id.is_empty());
    }

    #[test]
    fn durable_file_references_round_trip_without_session_handles() {
        let recipe = CustomLaunchRecipe {
            recipe_id: "recipe-1".into(),
            revision: 1,
            requester_package_version: "1.0".into(),
            game_id: 620,
            installation_id: "install-1".into(),
            base_target_revision: 3,
            base_launch_fingerprint: 9,
            executable_handle: String::new(),
            executable: Some(super::super::ApprovedFileReference {
                approval_id: "approval-1".into(),
                relative_path: vec!["bin".into(), "game.exe".into()],
            }),
            uri: String::new(),
            arguments: vec!["--custom".into()],
            working_directory_handle: String::new(),
            working_directory: Some(super::super::ApprovedFileReference {
                approval_id: "approval-1".into(),
                relative_path: vec!["bin".into()],
            }),
            runtime_handle: String::new(),
            content_id: String::new(),
            environment_set: Default::default(),
            environment_unset: Default::default(),
            mod_layers: Vec::new(),
        };
        assert!(recipe.validate());
        let serialized = serde_json::to_string(&recipe).unwrap();
        assert!(!serialized.contains("C:\\"));
        assert!(recipe.executable_handle.is_empty());
        assert!(recipe.working_directory_handle.is_empty());
        assert_eq!(
            serde_json::from_str::<CustomLaunchRecipe>(&serialized).unwrap(),
            recipe
        );
    }

    #[test]
    fn uri_customization_is_retained_for_the_uri_handler_to_accept_or_reject() {
        assert!(uri_recipe().validate());
    }

    #[test]
    fn uri_recipe_cannot_also_select_an_external_runtime() {
        let mut recipe = uri_recipe();
        recipe.runtime_handle = "runtime-handle".into();
        assert!(!recipe.validate());
    }

    #[test]
    fn uri_recipe_cannot_also_select_an_executable() {
        let mut recipe = uri_recipe();
        recipe.executable_handle = "executable-handle".into();
        assert!(!recipe.validate());
    }

    #[test]
    fn parameter_only_recipe_uses_the_detected_target_without_a_target_field() {
        let mut recipe = CustomLaunchRecipe {
            arguments: vec!["--windowed".into()],
            environment_set: [("GAME_PROFILE".into(), "fixture".into())].into(),
            ..CustomLaunchRecipe::default()
        };
        recipe.recipe_id = "recipe-1".into();
        recipe.revision = 1;
        recipe.game_id = 620;
        recipe.installation_id = "install-1".into();
        recipe.base_target_revision = 3;
        recipe.base_launch_fingerprint = 9;
        assert!(recipe.validate());

        recipe.arguments.clear();
        recipe.environment_set.clear();
        assert!(
            !recipe.validate(),
            "an empty overlay is not a customization"
        );
    }

    #[test]
    fn mod_layers_apply_in_order_and_bind_their_own_revision() {
        let mut recipe = CustomLaunchRecipe {
            arguments: vec!["--game".into()],
            environment_set: [("MODE".into(), "base".into()), ("DROP".into(), "1".into())].into(),
            mod_layers: vec![
                mod_layer(),
                CustomLaunchModLayer {
                    id: "second".into(),
                    name: String::new(),
                    version: "0.1".into(),
                    revision: 1,
                    arguments: vec!["--second".into()],
                    environment_set: [("MODE".into(), "layered".into())].into(),
                    environment_unset: ["DROP".into()].into(),
                },
            ],
            ..CustomLaunchRecipe::default()
        };
        recipe.recipe_id = "recipe-1".into();
        recipe.revision = 1;
        recipe.game_id = 620;
        recipe.installation_id = "install-1".into();
        recipe.base_target_revision = 3;
        recipe.base_launch_fingerprint = 9;
        assert!(recipe.validate());
        assert_eq!(
            recipe.effective_arguments(),
            ["--game", "--mod=texture-hd", "--second"]
        );
        let environment = recipe.effective_environment();
        assert_eq!(environment.set["MODE"], "layered");
        assert_eq!(environment.set["MOD_LAYER"], "texture-hd");
        assert!(!environment.set.contains_key("DROP"));
        assert!(environment.unset.contains("VANILLA"));
        assert!(environment.unset.contains("DROP"));
        assert!(environment.validate());

        let mut changed = recipe.clone();
        changed.mod_layers[0].revision += 1;
        assert!(changed.validate());
        assert_eq!(changed.effective_arguments(), recipe.effective_arguments());
    }

    #[test]
    fn mod_layer_validation_rejects_duplicates_and_target_conflicts() {
        let mut recipe = CustomLaunchRecipe {
            mod_layers: vec![mod_layer(), mod_layer()],
            ..CustomLaunchRecipe::default()
        };
        recipe.recipe_id = "recipe-1".into();
        recipe.revision = 1;
        recipe.game_id = 620;
        recipe.installation_id = "install-1".into();
        recipe.base_target_revision = 3;
        recipe.base_launch_fingerprint = 9;
        assert!(!recipe.validate());

        recipe.mod_layers = vec![mod_layer()];
        recipe.uri = "steam://rungameid/620".into();
        assert!(recipe.validate());
        recipe.runtime_handle = "runtime-handle".into();
        assert!(!recipe.validate());
    }

    #[test]
    fn recipes_without_mod_layers_remain_compatible() {
        let mut value = serde_json::to_value(uri_recipe()).unwrap();
        value.as_object_mut().unwrap().remove("modLayers");
        let recipe: CustomLaunchRecipe = serde_json::from_value(value).unwrap();
        assert!(recipe.mod_layers.is_empty());
    }
}

/// Result of a revision-checked recipe mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomLaunchMutation {
    Applied { revision: u64 },
    Conflict { current: u64 },
    Missing,
}

/// The user's active custom launch choice for one library entry. No row means the detected
/// default is selected. A stale row remains present so Play can ask for an explicit repair or
/// return to the detected default instead of silently switching routes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRecipeSelection {
    pub entry_key: String,
    pub owner_key: String,
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub recipe_id: String,
    pub recipe_revision: u64,
    pub base_target_revision: u64,
    /// The exact reviewed plan. Empty until the user approves a resolved plan.
    pub approved_plan_fingerprint: String,
}

/// Compare-and-set result when changing the user's active launch recipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchRecipeSelectionWrite {
    Applied,
    Conflict,
    RecipeMissing,
}
