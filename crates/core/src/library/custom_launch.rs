//! Typed plugin-owned launch recipes. Plugin-provided text is data; only host-resolved handles may
//! become executable command paths.

use crate::drm_free::{LaunchEnvironment, LaunchEnvironmentDelta};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_RECIPE_ARGUMENTS: usize = 128;
pub const MAX_RECIPE_ARGUMENT_BYTES: usize = 4096;
pub const MAX_RECIPE_TEXT_BYTES: usize = 128;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomLaunchRecipe {
    /// Opaque id minted by Core when the recipe is proposed.
    pub recipe_id: String,
    /// Changes whenever the stored recipe is replaced.
    pub revision: u64,
    pub game_id: u64,
    pub installation_id: String,
    /// Revision of the detected target this recipe was based on.
    pub base_target_revision: u64,
    /// Exactly one direct alternate executable handle or URI, unless a runtime wraps the default.
    pub executable_handle: String,
    pub uri: String,
    pub arguments: Vec<String>,
    pub working_directory_handle: String,
    pub runtime_handle: String,
    /// Stable selected-content identity when a runtime binding requires a content item.
    #[serde(default)]
    pub content_id: String,
    pub environment_set: LaunchEnvironment,
    pub environment_unset: BTreeSet<String>,
}

impl CustomLaunchRecipe {
    pub fn environment(&self) -> LaunchEnvironmentDelta {
        LaunchEnvironmentDelta {
            set: self.environment_set.clone(),
            unset: self.environment_unset.clone(),
        }
    }

    pub fn validate(&self) -> bool {
        let handle_ok = |value: &str| {
            value.len() <= MAX_RECIPE_TEXT_BYTES
                && !value.chars().any(|ch| ch.is_control() || ch == '\0')
        };
        let has_executable = !self.executable_handle.is_empty();
        let has_uri = !self.uri.is_empty();
        if !handle_ok(&self.recipe_id)
            || self.recipe_id.is_empty()
            || self.revision == 0
            || self.game_id == 0
            || !handle_ok(&self.installation_id)
            || self.installation_id.is_empty()
            || self.base_target_revision == 0
            || !handle_ok(&self.executable_handle)
            || !handle_ok(&self.working_directory_handle)
            || !handle_ok(&self.runtime_handle)
            || !handle_ok(&self.content_id)
            || self.arguments.len() > MAX_RECIPE_ARGUMENTS
            || self.arguments.iter().any(|argument| {
                argument.len() > MAX_RECIPE_ARGUMENT_BYTES || argument.chars().any(|ch| ch == '\0')
            })
            || !self.environment().validate()
            || (has_executable && has_uri)
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
        has_executable || !self.runtime_handle.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri_recipe() -> CustomLaunchRecipe {
        CustomLaunchRecipe {
            recipe_id: "recipe-1".into(),
            revision: 1,
            game_id: 620,
            installation_id: "install-1".into(),
            base_target_revision: 3,
            executable_handle: String::new(),
            uri: "steam://rungameid/620".into(),
            arguments: vec!["--custom".into()],
            working_directory_handle: "approved-cwd".into(),
            runtime_handle: String::new(),
            content_id: String::new(),
            environment_set: [("CUSTOM_VALUE".into(), "value".into())].into(),
            environment_unset: ["REMOVE_VALUE".into()].into(),
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
}

/// Result of a revision-checked recipe mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomLaunchMutation {
    Applied { revision: u64 },
    Conflict { current: u64 },
    Missing,
}
