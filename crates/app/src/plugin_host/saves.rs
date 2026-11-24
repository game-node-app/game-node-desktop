//! Save profiles contributed by plugins (task 13.1, design section 12).
//!
//! A save profile says where one runtime keeps a game's saves: a name, one to eight folders, rules
//! that narrow what counts as a save inside each folder, and an optional runtime and format. It
//! belongs to the plugin that made it and lives in that plugin's own store for the active account
//! profile, beside the folder approvals it points to.
//!
//! A profile is *configuration*. It carries no path and grants no access: each folder is a folder
//! the user already approved for the same plugin and installation (task 6.2), named by the
//! approval's id; the rules only narrow inside it, so a profile can never reach further than the
//! approvals do, and revoking an approval deactivates the folders that rest on it. Reading,
//! backing up and restoring stay separate operations with their own permissions.
//!
//! This module holds what needs no I/O: the bounds, the checks of what a plugin sends, how a
//! profile is stored, and the provisional port through which another plugin's profile can be
//! granted. The service is in `resources/saves.rs`.

use std::sync::{Arc, Mutex};

use gamenode_plugin_protocol::prost_types::{self, Struct, Value, value::Kind};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tonic::Status;

use super::notifications::is_plain_line;

/// Profiles one plugin may keep for one installation.
pub const MAX_PROFILES_PER_INSTALLATION: usize = 16;
/// Profiles one plugin may keep in all.
pub const MAX_PROFILES_PER_PLUGIN: usize = 256;
/// Folders in one profile.
pub const MAX_DIRECTORIES: usize = 8;
/// Rules for one folder.
pub const MAX_PATTERNS: usize = 16;
/// The longest rule, in bytes.
pub const MAX_PATTERN_BYTES: usize = 256;
/// The longest profile or folder name, in bytes.
pub const MAX_NAME_BYTES: usize = 120;
/// The longest runtime or format id, in bytes.
pub const MAX_ID_BYTES: usize = 128;
/// The most descriptive metadata, as JSON.
pub const MAX_METADATA_BYTES: usize = 8 * 1024;
/// How deep descriptive metadata may nest.
pub const MAX_METADATA_DEPTH: usize = 4;
/// How many keys and list items descriptive metadata may hold in all.
pub const MAX_METADATA_ITEMS: usize = 64;

/// What a foreign profile grant allows.
pub mod access {
    pub const READ: u8 = 1;
    pub const UPDATE: u8 = 2;
    pub const DELETE: u8 = 4;
}

fn refuse(message: impl Into<String>) -> Status {
    Status::invalid_argument(message.into())
}

/// A folder of a profile as stored: the approval it rests on and the directory below its root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
    pub approval_id: String,
    /// Plain names below the approval's root; empty means the root itself.
    pub path: Vec<String>,
    pub display_name: String,
    pub naming_patterns: Vec<String>,
}

pub fn directories_json(directories: &[Directory]) -> String {
    serde_json::to_string(directories).unwrap_or_else(|_| "[]".into())
}

pub fn directories_from_json(text: &str) -> Vec<Directory> {
    serde_json::from_str(text).unwrap_or_default()
}

/// A profile or folder name: trimmed, one line of plain text, 1 to 120 bytes.
pub fn check_name(label: &str, text: &str) -> Result<String, Status> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_NAME_BYTES || !is_plain_line(text) {
        return Err(refuse(format!(
            "{label} is required: one line of plain text, at most {MAX_NAME_BYTES} bytes"
        )));
    }
    Ok(text.to_owned())
}

/// A folder's display name, which may be empty.
pub fn check_display_name(text: &str) -> Result<String, Status> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    check_name("display_name", text)
}

/// A runtime or format id: letters, digits, `.`, `_` and `-`, at most 128 bytes, or empty.
pub fn check_identifier(label: &str, text: &str) -> Result<String, Status> {
    if text.len() > MAX_ID_BYTES
        || !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(refuse(format!(
            "{label} may hold letters, digits, '.', '_' and '-', at most {MAX_ID_BYTES} bytes"
        )));
    }
    Ok(text.to_owned())
}

/// A naming rule: a relative glob inside the folder, `/`-separated, `*` and `?` inside a name and
/// `**` as a whole name for any depth. A leading `!` makes it an exclusion.
pub fn check_pattern(text: &str) -> Result<String, Status> {
    let bad = || {
        refuse(format!(
            "a naming pattern is a relative path of at most {MAX_PATTERN_BYTES} bytes, optionally \
             starting with '!' to exclude, with '*', '?' and '**' as the only wildcards"
        ))
    };
    if text.len() > MAX_PATTERN_BYTES {
        return Err(bad());
    }
    let body = text.strip_prefix('!').unwrap_or(text);
    if body.is_empty() || body.starts_with('/') || body.ends_with('/') {
        return Err(bad());
    }
    for name in body.split('/') {
        let forbidden = |c: char| c.is_control() || matches!(c, '\\' | ':' | '[' | ']' | '{' | '}');
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.chars().any(forbidden)
            || (name.contains("**") && name != "**")
        {
            return Err(bad());
        }
    }
    Ok(text.to_owned())
}

fn segment_matches(pattern: &[char], name: &[char]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|skip| segment_matches(rest, &name[skip..])),
        Some(('?', rest)) => !name.is_empty() && segment_matches(rest, &name[1..]),
        Some((c, rest)) => name.first() == Some(c) && segment_matches(rest, &name[1..]),
    }
}

fn path_matches(pattern: &[&str], path: &[String]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| path_matches(rest, &path[skip..])),
        Some((segment, rest)) => path.split_first().is_some_and(|(name, below)| {
            let (segment, name): (Vec<char>, Vec<char>) =
                (segment.chars().collect(), name.chars().collect());
            segment_matches(&segment, &name) && path_matches(rest, below)
        }),
    }
}

/// Whether the file at `relative` (below the profile's folder) is one the folder's rules select.
/// A rule matches the whole relative path, from the folder, so `*.sav` means the files of the
/// folder itself and `**/*.sav` those at any depth. With no inclusion rule everything is
/// selected; an exclusion (`!rule`) always wins.
pub fn selected(patterns: &[String], relative: &[String]) -> bool {
    let matches = |pattern: &str| {
        let segments: Vec<&str> = pattern.split('/').collect();
        path_matches(&segments, relative)
    };
    let mut included = patterns.iter().all(|p| p.starts_with('!'));
    for pattern in patterns {
        match pattern.strip_prefix('!') {
            Some(exclusion) if matches(exclusion) => return false,
            Some(_) => {}
            None => included |= matches(pattern),
        }
    }
    included
}

/// Checks the rules of one folder.
pub fn check_patterns(patterns: &[String]) -> Result<Vec<String>, Status> {
    if patterns.len() > MAX_PATTERNS {
        return Err(refuse(format!(
            "a folder takes at most {MAX_PATTERNS} naming patterns"
        )));
    }
    let mut seen = std::collections::HashSet::new();
    patterns
        .iter()
        .map(|pattern| {
            let pattern = check_pattern(pattern)?;
            seen.insert(pattern.clone())
                .then_some(pattern)
                .ok_or_else(|| refuse("a naming pattern is listed twice"))
        })
        .collect()
}

fn json_of(value: &Value, depth: usize, items: &mut usize) -> Result<Json, Status> {
    let too_deep = || refuse("descriptive metadata nests too deeply");
    if depth > MAX_METADATA_DEPTH {
        return Err(too_deep());
    }
    Ok(match value.kind.as_ref() {
        None | Some(Kind::NullValue(_)) => Json::Null,
        Some(Kind::BoolValue(flag)) => Json::Bool(*flag),
        Some(Kind::StringValue(text)) => {
            if text.chars().any(char::is_control) {
                return Err(refuse("descriptive metadata holds a control character"));
            }
            Json::String(text.clone())
        }
        Some(Kind::NumberValue(number)) => serde_json::Number::from_f64(*number)
            .map(Json::Number)
            .ok_or_else(|| refuse("descriptive metadata holds a number that is not finite"))?,
        Some(Kind::StructValue(inner)) => Json::Object(object_of(inner, depth + 1, items)?),
        Some(Kind::ListValue(list)) => Json::Array(
            list.values
                .iter()
                .map(|value| {
                    *items += 1;
                    if *items > MAX_METADATA_ITEMS {
                        return Err(refuse("descriptive metadata holds too many items"));
                    }
                    json_of(value, depth + 1, items)
                })
                .collect::<Result<_, _>>()?,
        ),
    })
}

fn object_of(
    object: &Struct,
    depth: usize,
    items: &mut usize,
) -> Result<serde_json::Map<String, Json>, Status> {
    let mut map = serde_json::Map::new();
    for (key, value) in &object.fields {
        *items += 1;
        if *items > MAX_METADATA_ITEMS {
            return Err(refuse("descriptive metadata holds too many items"));
        }
        if key.is_empty() || key.len() > MAX_ID_BYTES || key.chars().any(char::is_control) {
            return Err(refuse(
                "a descriptive metadata key is 1 to 128 bytes of plain text",
            ));
        }
        map.insert(key.clone(), json_of(value, depth, items)?);
    }
    Ok(map)
}

/// Checks descriptive metadata (bounded depth, items and size) and returns it as stored JSON.
/// Structural validation of extension schemas is task 12.6; until then these bounds apply.
pub fn check_metadata(metadata: Option<&Struct>) -> Result<String, Status> {
    let Some(metadata) = metadata else {
        return Ok("{}".into());
    };
    let mut items = 0;
    let object = object_of(metadata, 1, &mut items)?;
    let text = serde_json::to_string(&Json::Object(object)).unwrap_or_else(|_| "{}".into());
    if text.len() > MAX_METADATA_BYTES {
        return Err(refuse(format!(
            "descriptive metadata is larger than {MAX_METADATA_BYTES} bytes"
        )));
    }
    Ok(text)
}

fn value_of(json: &Json) -> Value {
    Value {
        kind: Some(match json {
            Json::Null => Kind::NullValue(0),
            Json::Bool(flag) => Kind::BoolValue(*flag),
            Json::Number(number) => Kind::NumberValue(number.as_f64().unwrap_or_default()),
            Json::String(text) => Kind::StringValue(text.clone()),
            Json::Array(list) => Kind::ListValue(prost_types::ListValue {
                values: list.iter().map(value_of).collect(),
            }),
            Json::Object(map) => Kind::StructValue(struct_of(map)),
        }),
    }
}

fn struct_of(map: &serde_json::Map<String, Json>) -> Struct {
    Struct {
        fields: map
            .iter()
            .map(|(key, value)| (key.clone(), value_of(value)))
            .collect(),
    }
}

/// Descriptive metadata as it was stored, for a reply.
pub fn metadata_struct(text: &str) -> Option<Struct> {
    match serde_json::from_str::<Json>(text) {
        Ok(Json::Object(map)) if !map.is_empty() => Some(struct_of(&map)),
        _ => None,
    }
}

/// A new profile id. It is the host's: the request has no way to choose one.
pub fn new_profile_id() -> Result<String, Status> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| Status::internal("no source of randomness"))?;
    Ok(format!(
        "profile-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

/// Another plugin's profile that the user allowed this plugin to use, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignGrant {
    pub creator_plugin_id: String,
    pub creator_publisher: String,
    pub profile_id: String,
    /// A mix of [`access`] bits.
    pub operations: u8,
}

/// Where the grants for other plugins' profiles come from. **Provisional** (task 13.1): until the
/// shared data plane (section 12) and the permissions screen exist, the grants are whatever the
/// application connects here; with nothing connected no foreign profile can be reached.
pub trait ProfileGrants: Send + Sync {
    /// The profiles of other plugins the user allowed `plugin_id` of `publisher` to use.
    fn grants(&self, plugin_id: &str, publisher: &str) -> Vec<ForeignGrant>;
}

/// The profile grants the host holds. Shared by every plugin session.
#[derive(Default)]
pub struct SaveAccess {
    grants: Mutex<Option<Arc<dyn ProfileGrants>>>,
}

impl SaveAccess {
    /// Connects the source of foreign grants. Until it is set, a plugin reaches only its own
    /// profiles.
    pub fn set_grants(&self, grants: Arc<dyn ProfileGrants>) {
        *self.grants.lock().unwrap_or_else(|p| p.into_inner()) = Some(grants);
    }

    /// The grant that lets `plugin_id` of `publisher` do `operation` to the profile `profile_id`
    /// of another plugin, if any.
    pub fn grant_for(
        &self,
        plugin_id: &str,
        publisher: &str,
        profile_id: &str,
        operation: u8,
    ) -> Option<ForeignGrant> {
        let source = self
            .grants
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()?;
        source
            .grants(plugin_id, publisher)
            .into_iter()
            .find(|grant| {
                grant.profile_id == profile_id
                    && grant.operations & operation == operation
                    // A plugin's own profiles are never reached through a foreign grant.
                    && !(grant.creator_plugin_id == plugin_id
                        && grant.creator_publisher == publisher)
            })
    }
}

#[cfg(test)]
mod tests;
