//! The `Saves` service: save profiles contributed by plugins (task 13.1, design section 12).
//!
//! A profile is configuration kept in the contributing plugin's own store (see
//! `plugin_host::saves`). Its folders are folders the user already approved for the same plugin
//! and installation (task 6.2), named by the handles the plugin got; the rules only narrow inside
//! them. Nothing here reads a file, and no permission here authorizes reading, backing up or
//! restoring one.
//!
//! The grants are `game.saves.read`, `.create`, `.update` and `.delete`, independent of each
//! other. A plugin reaches another plugin's profile only through a provisional grant the host
//! holds for that profile and operation ([`SaveAccess`]); it never changes the folders of a
//! profile it did not create, because they are the other plugin's approved folders.

use gamenode_plugin_protocol::{capability as cap, proto::*};
use tonic::{Request, Response, Status};

use super::mutations::{game_id, mutation_key, reply, request_hash};
use super::{ResourceApi, Served};
use crate::plugin_host::files::FileAccess;
use crate::plugin_host::provider::ResourceCaller;
use crate::plugin_host::saves::{
    self as saves, Directory, ForeignGrant, MAX_DIRECTORIES, MAX_PROFILES_PER_INSTALLATION,
    MAX_PROFILES_PER_PLUGIN, access,
};
use crate::plugin_host::storage::{Namespace, ProfileWrite};
use gamenode_platform::plugin_storage::SaveProfileRow;

const DEFAULT_PAGE: usize = 50;
const MAX_PAGE: usize = 200;
const MAX_SELECTED: usize = 64;
const MAX_ID_BYTES: usize = 128;

pub const OWN_PROFILES: &str =
    "this plugin's own profiles; another plugin's only through a grant for that profile";

/// The `Saves` methods this build serves, for `DescribeApi`.
pub const SERVED: &[Served] = &[
    Served::new("ListProfiles", cap::GAME_SAVES_READ, true, OWN_PROFILES),
    Served::new("GetProfile", cap::GAME_SAVES_READ, false, OWN_PROFILES),
    Served::new("CreateProfile", cap::GAME_SAVES_CREATE, false, OWN_PROFILES),
    Served::new("UpdateProfile", cap::GAME_SAVES_UPDATE, false, OWN_PROFILES),
    Served::new("RemoveProfile", cap::GAME_SAVES_DELETE, false, OWN_PROFILES),
];

/// Fields an update may name in its mask. Identity, installation, game and provenance are not
/// among them: a profile that moved would be a different profile.
const PATCHABLE: &[&str] = &[
    "name",
    "directories",
    "runtime_id",
    "format_id",
    "format_version",
    "descriptive_metadata",
];

fn profile_id(text: &str) -> Result<&str, Status> {
    if text.is_empty() || text.len() > MAX_ID_BYTES || text.chars().any(char::is_control) {
        return Err(Status::invalid_argument(
            "profile_id is required: 1 to 128 bytes",
        ));
    }
    Ok(text)
}

fn not_found() -> Status {
    Status::not_found("save profile not found")
}

/// Whose a profile in a reply is.
struct Owner {
    plugin_id: String,
}

/// Resolves what a plugin sent as folders into stored folders. Runs on the namespace worker.
fn resolve_directories(
    files: &FileAccess,
    namespace: &mut Namespace<'_>,
    session: &str,
    installation_id: &str,
    directories: &[SaveDirectory],
) -> Result<Vec<Directory>, Status> {
    if directories.is_empty() || directories.len() > MAX_DIRECTORIES {
        return Err(Status::invalid_argument(format!(
            "a profile has 1 to {MAX_DIRECTORIES} directories"
        )));
    }
    let mut resolved: Vec<Directory> = Vec::new();
    for directory in directories {
        // A malformed handle, another session's, a revoked approval's and an unknown one are the
        // same answer, as for file access.
        let (approval_id, path) = files
            .open_handle(session, &directory.approved_resource_handle)
            .ok_or_else(|| Status::not_found("file handle not found"))?;
        let approval = namespace
            .approval(&approval_id)?
            .filter(|approval| approval.revoked_at.is_none())
            .ok_or_else(|| Status::not_found("file handle not found"))?;
        if approval.installation_id != installation_id {
            return Err(Status::permission_denied(
                "this folder was approved for another library entry",
            ));
        }
        if resolved
            .iter()
            .any(|seen| seen.approval_id == approval_id && seen.path == path)
        {
            return Err(Status::invalid_argument("a directory is listed twice"));
        }
        resolved.push(Directory {
            approval_id,
            path,
            display_name: saves::check_display_name(&directory.display_name)?,
            naming_patterns: saves::check_patterns(&directory.naming_patterns)?,
        });
    }
    Ok(resolved)
}

/// A stored profile as a plugin sees it. `handles` is the session whose folder handles to issue
/// (an own profile whose approvals still stand); a folder whose approval was revoked, or any
/// folder of another plugin's profile, comes with no handle: its folder is not reachable.
fn to_proto(
    row: &SaveProfileRow,
    owner: &Owner,
    handles: Option<(&FileAccess, &str, &mut Namespace<'_>)>,
) -> Result<SaveProfile, Status> {
    let mut directories = Vec::new();
    let mut handles = handles;
    for directory in saves::directories_from_json(&row.directories) {
        let handle = match handles.as_mut() {
            Some((files, session, namespace)) => {
                let active = namespace
                    .approval(&directory.approval_id)?
                    .is_some_and(|approval| {
                        approval.revoked_at.is_none()
                            && approval.installation_id == row.installation_id
                    });
                if active {
                    files.seal_handle(session, &directory.approval_id, &directory.path)
                } else {
                    String::new()
                }
            }
            None => String::new(),
        };
        directories.push(SaveDirectory {
            approved_resource_handle: handle,
            display_name: directory.display_name,
            naming_patterns: directory.naming_patterns,
        });
    }
    Ok(SaveProfile {
        profile_id: row.id.clone(),
        game_id: row.game_id.clone(),
        installation_id: row.installation_id.clone(),
        name: row.name.clone(),
        directories,
        runtime_id: row.runtime_id.clone(),
        format_id: row.format_id.clone(),
        format_version: row.format_version,
        descriptive_metadata: saves::metadata_struct(&row.metadata),
        creator_plugin_id: owner.plugin_id.clone(),
        revision: row.revision,
    })
}

fn no_categories(selector: Option<&CommonDataSelector>) -> Result<(), Status> {
    match selector {
        Some(selector) if !selector.categories.is_empty() => Err(Status::unimplemented(
            "common-data categories are not available for Saves",
        )),
        Some(selector) if selector.selected_record_ids.len() > MAX_SELECTED => {
            Err(Status::invalid_argument("too many selected records"))
        }
        _ => Ok(()),
    }
}

impl ResourceApi {
    /// The caller's plugin identity: the authenticated plugin and its pinned publisher.
    fn saves_identity(caller: &ResourceCaller) -> Result<(String, String), Status> {
        let identity = caller
            .namespace
            .as_ref()
            .ok_or_else(|| {
                Status::failed_precondition("official providers use trusted host adapters")
            })?
            .identity();
        Ok((
            identity.plugin_id.clone(),
            identity.publisher_fingerprint.clone(),
        ))
    }

    /// Another plugin's profile `id`, if this caller holds a grant for `operation` on it.
    fn foreign_grant(
        &self,
        caller: &ResourceCaller,
        id: &str,
        operation: u8,
    ) -> Result<Option<ForeignGrant>, Status> {
        let (plugin_id, publisher) = Self::saves_identity(caller)?;
        Ok(self.saves.grant_for(&plugin_id, &publisher, id, operation))
    }

    /// Runs `work` on the namespace of the plugin that created a profile this caller was granted.
    async fn in_creator<T: Send + 'static>(
        &self,
        grant: &ForeignGrant,
        work: impl FnOnce(&mut Namespace<'_>) -> Result<T, Status> + Send + 'static,
    ) -> Result<T, Status> {
        let profiles = self.providers.profiles();
        let resolver =
            profiles.resolver(&grant.creator_plugin_id, &grant.creator_publisher, None)?;
        self.storage
            .run_for_host(&resolver, profiles.epoch(), work)
            .await
    }

    pub(super) async fn list_save_profiles(
        &self,
        request: Request<SaveProfileListRequest>,
    ) -> Result<Response<SaveProfileListReply>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::GAME_SAVES_READ)?;
        no_categories(request.shared_records.as_ref())?;
        let wanted_game = game_id(&request.game_id)?;
        let size = match request.page_size as usize {
            0 => DEFAULT_PAGE,
            size => size.min(MAX_PAGE),
        };
        let (plugin_id, _) = Self::saves_identity(&caller)?;
        let (files, session) = (self.files.clone(), caller.session_id.clone());
        let (installation, game, after) = (
            request.installation_id.clone(),
            wanted_game.map(|id| id.to_string()),
            request.cursor.clone(),
        );
        let owner = Owner {
            plugin_id: plugin_id.clone(),
        };
        let (mut profiles, next, revision) = self
            .in_namespace(&caller, move |namespace| {
                let mut rows: Vec<_> = namespace
                    .save_profiles()?
                    .into_iter()
                    .filter(|row| installation.is_empty() || row.installation_id == installation)
                    .filter(|row| game.as_ref().is_none_or(|game| row.game_id == *game))
                    .filter(|row| {
                        after.is_empty() || format!("{}|{}", row.installation_id, row.id) > after
                    })
                    .collect();
                let more = rows.len() > size;
                rows.truncate(size);
                let next = if more {
                    rows.last()
                        .map(|row| format!("{}|{}", row.installation_id, row.id))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                let profiles = rows
                    .iter()
                    .map(|row| to_proto(row, &owner, Some((&files, &session, namespace))))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((profiles, next, namespace.revision()?))
            })
            .await?;

        // Profiles of other plugins that were granted and named: only on the first page.
        if request.cursor.is_empty()
            && let Some(selector) = &request.shared_records
        {
            for id in &selector.selected_record_ids {
                if profiles.iter().any(|profile| profile.profile_id == *id) {
                    continue;
                }
                let Some(grant) = self.foreign_grant(&caller, id, access::READ)? else {
                    continue;
                };
                if let Some(profile) = self.foreign_profile(&grant).await? {
                    let matches = (request.installation_id.is_empty()
                        || profile.installation_id == request.installation_id)
                        && wanted_game.is_none_or(|game| profile.game_id == game.to_string());
                    if matches {
                        profiles.push(profile);
                    }
                }
            }
        }
        // The grants are checked once more before the answer leaves.
        self.providers
            .authorize_resource(request.context.as_ref(), cap::GAME_SAVES_READ)?;
        Ok(Response::new(SaveProfileListReply {
            profiles,
            next_cursor: next,
            revision,
        }))
    }

    /// The profile a foreign grant names, as the grantee sees it (no folder handles).
    async fn foreign_profile(&self, grant: &ForeignGrant) -> Result<Option<SaveProfile>, Status> {
        let id = grant.profile_id.clone();
        let owner = Owner {
            plugin_id: grant.creator_plugin_id.clone(),
        };
        self.in_creator(grant, move |namespace| {
            namespace
                .save_profile(&id)?
                .map(|row| to_proto(&row, &owner, None))
                .transpose()
        })
        .await
    }

    pub(super) async fn get_save_profile(
        &self,
        request: Request<SaveProfileGetRequest>,
    ) -> Result<Response<SaveProfileReply>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::GAME_SAVES_READ)?;
        no_categories(request.shared_records.as_ref())?;
        let id = profile_id(&request.profile_id)?.to_owned();
        let wanted_game = game_id(&request.game_id)?.map(|id| id.to_string());
        let (plugin_id, _) = Self::saves_identity(&caller)?;
        let (files, session) = (self.files.clone(), caller.session_id.clone());
        let owner = Owner { plugin_id };
        let lookup = id.clone();
        let own = self
            .in_namespace(&caller, move |namespace| {
                let Some(row) = namespace.save_profile(&lookup)? else {
                    return Ok(None);
                };
                let profile = to_proto(&row, &owner, Some((&files, &session, namespace)))?;
                Ok(Some((profile, namespace.revision()?)))
            })
            .await?;
        let (profile, revision) = match own {
            Some(found) => found,
            None => {
                let grant = self
                    .foreign_grant(&caller, &id, access::READ)?
                    .ok_or_else(not_found)?;
                let profile = self.foreign_profile(&grant).await?.ok_or_else(not_found)?;
                let revision = profile.revision;
                (profile, revision)
            }
        };
        if wanted_game.is_some_and(|game| profile.game_id != game) {
            return Err(not_found());
        }
        self.providers
            .authorize_resource(request.context.as_ref(), cap::GAME_SAVES_READ)?;
        Ok(Response::new(SaveProfileReply {
            profile: Some(profile),
            revision,
        }))
    }

    pub(super) async fn create_save_profile(
        &self,
        request: Request<SaveProfileCreateRequest>,
    ) -> Result<Response<SaveProfileMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let caller = self
            .providers
            .authorize_resource(mutation.context.as_ref(), cap::GAME_SAVES_CREATE)?;
        let key = mutation_key(mutation)?;
        let profile = request
            .profile
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("profile is required"))?;
        if mutation.expected_revision != 0 {
            return Err(Status::invalid_argument(
                "expected_revision must be 0 to create a profile",
            ));
        }
        if !profile.profile_id.is_empty()
            || !profile.creator_plugin_id.is_empty()
            || profile.revision != 0
        {
            return Err(Status::invalid_argument(
                "profile_id, creator_plugin_id and revision are set by the host",
            ));
        }
        let name = saves::check_name("name", &profile.name)?;
        let runtime_id = saves::check_identifier("runtime_id", &profile.runtime_id)?;
        let format_id = saves::check_identifier("format_id", &profile.format_id)?;
        let metadata = saves::check_metadata(profile.descriptive_metadata.as_ref())?;
        if profile.installation_id.is_empty() || profile.installation_id.len() > MAX_ID_BYTES {
            return Err(Status::invalid_argument(
                "installation_id is required: 1 to 128 bytes",
            ));
        }
        if profile.directories.is_empty() || profile.directories.len() > MAX_DIRECTORIES {
            return Err(Status::invalid_argument(format!(
                "a profile has 1 to {MAX_DIRECTORIES} directories"
            )));
        }
        // The profile is for one of this plugin's own library entries.
        let (_, entry) = self.action_target(
            &caller,
            &mutation.context.clone().unwrap_or_default(),
            &profile.game_id,
            &profile.installation_id,
        )?;
        let game = entry.game_id.map(|id| id.to_string()).unwrap_or_default();
        let id = saves::new_profile_id()?;

        let mut parts: Vec<String> = vec![
            "save-profile-create".into(),
            profile.installation_id.clone(),
            profile.game_id.clone(),
            name.clone(),
            runtime_id.clone(),
            format_id.clone(),
            profile.format_version.to_string(),
            metadata.clone(),
        ];
        for directory in &profile.directories {
            parts.push(directory.approved_resource_handle.clone());
            parts.push(directory.display_name.clone());
            parts.push(directory.naming_patterns.join("\u{0}"));
        }
        let hash = request_hash(&parts.iter().map(String::as_str).collect::<Vec<_>>());

        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(from_stored(original)));
        }
        let context = mutation.context.clone().unwrap_or_default();
        let guard = self.guard(context, cap::GAME_SAVES_CREATE, caller.grant_revision);
        let (files, session) = (self.files.clone(), caller.session_id.clone());
        let directories = profile.directories.clone();
        let row = SaveProfileRow {
            id: id.clone(),
            installation_id: entry.installation_id.clone(),
            game_id: game,
            name,
            runtime_id,
            format_id,
            format_version: profile.format_version,
            metadata,
            directories: String::new(),
            revision: 0,
            created_at: 0,
        };
        let written = self
            .in_namespace(&caller, move |namespace| {
                let existing = namespace.save_profiles()?;
                if existing.len() >= MAX_PROFILES_PER_PLUGIN
                    || existing
                        .iter()
                        .filter(|row| row.installation_id == entry.installation_id)
                        .count()
                        >= MAX_PROFILES_PER_INSTALLATION
                {
                    return Err(Status::resource_exhausted(format!(
                        "a plugin keeps at most {MAX_PROFILES_PER_INSTALLATION} save profiles per \
                         game entry and {MAX_PROFILES_PER_PLUGIN} in all"
                    )));
                }
                let resolved = resolve_directories(
                    &files,
                    namespace,
                    &session,
                    &row.installation_id,
                    &directories,
                )?;
                let row = SaveProfileRow {
                    directories: saves::directories_json(&resolved),
                    created_at: namespace.now(),
                    ..row
                };
                namespace.put_save_profile(row, 0, &guard)
            })
            .await?;
        let answer = match written {
            ProfileWrite::Applied { revision } => SaveProfileMutationReply {
                profile_id: id,
                revision,
                outcome: MutationOutcome::Applied as i32,
            },
            ProfileWrite::NameTaken => {
                return Err(Status::already_exists(
                    "this game entry already has a profile with that name",
                ));
            }
            ProfileWrite::Conflict { .. } | ProfileWrite::NotFound => {
                return Err(Status::aborted(
                    "the profile could not be created; try again",
                ));
            }
        };
        self.mutations.remember(
            &caller,
            key,
            hash,
            &reply(
                &answer.profile_id,
                answer.revision,
                MutationOutcome::Applied,
            ),
        );
        Ok(Response::new(answer))
    }

    pub(super) async fn update_save_profile(
        &self,
        request: Request<SaveProfileUpdateRequest>,
    ) -> Result<Response<SaveProfileMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let caller = self
            .providers
            .authorize_resource(mutation.context.as_ref(), cap::GAME_SAVES_UPDATE)?;
        let key = mutation_key(mutation)?;
        let id = profile_id(&request.profile_id)?.to_owned();
        if mutation.expected_revision == 0 {
            return Err(Status::invalid_argument(
                "expected_revision is required to update a profile",
            ));
        }
        let patch = request
            .patch
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("patch is required"))?;
        let new = patch.profile.clone().unwrap_or_default();
        if patch.field_mask.is_empty() {
            return Err(Status::invalid_argument("patch.field_mask is empty"));
        }
        for path in &patch.field_mask {
            if !PATCHABLE.contains(&path.as_str()) {
                return Err(Status::invalid_argument(format!(
                    "{path:?} cannot be updated; name, directories, runtime_id, format_id, \
                     format_version and descriptive_metadata can"
                )));
            }
        }
        let touches = |field: &str| patch.field_mask.iter().any(|path| path == field);
        // What the patch sets is checked now, before anything is read or locked.
        let name = touches("name")
            .then(|| saves::check_name("name", &new.name))
            .transpose()?;
        let runtime_id = touches("runtime_id")
            .then(|| saves::check_identifier("runtime_id", &new.runtime_id))
            .transpose()?;
        let format_id = touches("format_id")
            .then(|| saves::check_identifier("format_id", &new.format_id))
            .transpose()?;
        let metadata = touches("descriptive_metadata")
            .then(|| saves::check_metadata(new.descriptive_metadata.as_ref()))
            .transpose()?;
        let directories_given = touches("directories");
        if directories_given
            && (new.directories.is_empty() || new.directories.len() > MAX_DIRECTORIES)
        {
            return Err(Status::invalid_argument(format!(
                "a profile has 1 to {MAX_DIRECTORIES} directories"
            )));
        }
        let mut parts: Vec<String> = vec![
            "save-profile-update".into(),
            id.clone(),
            mutation.expected_revision.to_string(),
            patch.field_mask.join(","),
            name.clone().unwrap_or_default(),
            runtime_id.clone().unwrap_or_default(),
            format_id.clone().unwrap_or_default(),
            new.format_version.to_string(),
            metadata.clone().unwrap_or_default(),
        ];
        for directory in &new.directories {
            parts.push(directory.approved_resource_handle.clone());
            parts.push(directory.display_name.clone());
            parts.push(directory.naming_patterns.join("\u{0}"));
        }
        let hash = request_hash(&parts.iter().map(String::as_str).collect::<Vec<_>>());

        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(from_stored(original)));
        }
        let context = mutation.context.clone().unwrap_or_default();
        let guard = self.guard(context, cap::GAME_SAVES_UPDATE, caller.grant_revision);
        let expected = mutation.expected_revision;
        let format_version = touches("format_version").then_some(new.format_version);
        let new_directories = new.directories.clone();
        let (files, session) = (self.files.clone(), caller.session_id.clone());

        // The plugin's own profile first; then, for another's, the grant for this operation.
        let lookup = id.clone();
        let own = self
            .in_namespace(&caller, move |namespace| {
                Ok(namespace.save_profile(&lookup)?.is_some())
            })
            .await?;
        let apply =
            move |row: SaveProfileRow, directories: Option<Vec<Directory>>| -> SaveProfileRow {
                SaveProfileRow {
                    name: name.unwrap_or(row.name),
                    runtime_id: runtime_id.unwrap_or(row.runtime_id),
                    format_id: format_id.unwrap_or(row.format_id),
                    format_version: format_version.unwrap_or(row.format_version),
                    metadata: metadata.unwrap_or(row.metadata),
                    directories: directories
                        .map(|directories| saves::directories_json(&directories))
                        .unwrap_or(row.directories),
                    ..row
                }
            };
        let written = if own {
            let lookup = id.clone();
            self.in_namespace(&caller, move |namespace| {
                let row = namespace.save_profile(&lookup)?.ok_or_else(not_found)?;
                let directories = if directories_given {
                    Some(resolve_directories(
                        &files,
                        namespace,
                        &session,
                        &row.installation_id,
                        &new_directories,
                    )?)
                } else {
                    None
                };
                namespace.put_save_profile(apply(row, directories), expected, &guard)
            })
            .await?
        } else {
            let grant = self
                .foreign_grant(&caller, &id, access::UPDATE)?
                .ok_or_else(not_found)?;
            if directories_given {
                // The folders are the creator's own approved folders; a grantee cannot widen,
                // narrow or replace them.
                return Err(Status::permission_denied(
                    "the directories of a profile can be changed only by the plugin that made it",
                ));
            }
            let lookup = id.clone();
            self.in_creator(&grant, move |namespace| {
                let row = namespace.save_profile(&lookup)?.ok_or_else(not_found)?;
                namespace.put_save_profile(apply(row, None), expected, &guard)
            })
            .await?
        };
        let answer = Self::profile_outcome(&id, written)?;
        // Only a change that was made is remembered: a stale revision may be retried.
        if answer_outcome(&answer) == MutationOutcome::Applied {
            self.mutations.remember(
                &caller,
                key,
                hash,
                &reply(
                    &answer.profile_id,
                    answer.revision,
                    MutationOutcome::Applied,
                ),
            );
        }
        Ok(Response::new(answer))
    }

    pub(super) async fn remove_save_profile(
        &self,
        request: Request<SaveProfileRemoveRequest>,
    ) -> Result<Response<SaveProfileMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let caller = self
            .providers
            .authorize_resource(mutation.context.as_ref(), cap::GAME_SAVES_DELETE)?;
        let key = mutation_key(mutation)?;
        let id = profile_id(&request.profile_id)?.to_owned();
        let wanted_game = game_id(&request.game_id)?.map(|id| id.to_string());
        if mutation.expected_revision == 0 {
            return Err(Status::invalid_argument(
                "expected_revision is required to remove a profile",
            ));
        }
        let hash = request_hash(&[
            "save-profile-remove",
            &id,
            &request.game_id,
            &mutation.expected_revision.to_string(),
        ]);

        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(from_stored(original)));
        }
        let context = mutation.context.clone().unwrap_or_default();
        let guard = self.guard(context, cap::GAME_SAVES_DELETE, caller.grant_revision);
        let expected = mutation.expected_revision;
        let (lookup, game) = (id.clone(), wanted_game.clone());
        let own = self
            .in_namespace(&caller, move |namespace| {
                Ok(namespace
                    .save_profile(&lookup)?
                    .filter(|row| game.as_ref().is_none_or(|game| row.game_id == *game))
                    .is_some())
            })
            .await?;
        // Removing a profile never touches a file: the folders and the archives stay where the
        // user and the backup history keep them.
        let written = if own {
            let lookup = id.clone();
            self.in_namespace(&caller, move |namespace| {
                namespace.remove_save_profile(&lookup, expected, &guard)
            })
            .await?
        } else {
            let grant = self
                .foreign_grant(&caller, &id, access::DELETE)?
                .ok_or_else(not_found)?;
            let (lookup, game) = (id.clone(), wanted_game);
            self.in_creator(&grant, move |namespace| {
                let matches = namespace
                    .save_profile(&lookup)?
                    .is_some_and(|row| game.as_ref().is_none_or(|game| row.game_id == *game));
                if !matches {
                    return Ok(ProfileWrite::NotFound);
                }
                namespace.remove_save_profile(&lookup, expected, &guard)
            })
            .await?
        };
        let answer = Self::profile_outcome(&id, written)?;
        // Only a change that was made is remembered: a stale revision may be retried.
        if answer_outcome(&answer) == MutationOutcome::Applied {
            self.mutations.remember(
                &caller,
                key,
                hash,
                &reply(
                    &answer.profile_id,
                    answer.revision,
                    MutationOutcome::Applied,
                ),
            );
        }
        Ok(Response::new(answer))
    }

    /// The reply for an update or removal, or the status for what stopped it. A stale revision
    /// is an outcome, not an error, so the plugin can read the profile and try again.
    fn profile_outcome(
        id: &str,
        written: ProfileWrite,
    ) -> Result<SaveProfileMutationReply, Status> {
        Ok(match written {
            ProfileWrite::Applied { revision } => SaveProfileMutationReply {
                profile_id: id.to_owned(),
                revision,
                outcome: MutationOutcome::Applied as i32,
            },
            ProfileWrite::Conflict { current } => SaveProfileMutationReply {
                profile_id: id.to_owned(),
                revision: current,
                outcome: MutationOutcome::RevisionConflict as i32,
            },
            ProfileWrite::NameTaken => {
                return Err(Status::already_exists(
                    "this game entry already has a profile with that name",
                ));
            }
            ProfileWrite::NotFound => return Err(not_found()),
        })
    }
}

/// The outcome a retry is answered with: what the first call stored, in the reply's own shape.
fn from_stored(stored: MutationReply) -> SaveProfileMutationReply {
    SaveProfileMutationReply {
        profile_id: stored.resource_id,
        revision: stored.revision,
        outcome: stored.outcome,
    }
}

fn answer_outcome(answer: &SaveProfileMutationReply) -> MutationOutcome {
    MutationOutcome::try_from(answer.outcome).unwrap_or(MutationOutcome::Applied)
}

#[tonic::async_trait]
impl saves_server::Saves for ResourceApi {
    async fn list_profiles(
        &self,
        request: Request<SaveProfileListRequest>,
    ) -> Result<Response<SaveProfileListReply>, Status> {
        self.list_save_profiles(request).await
    }

    async fn get_profile(
        &self,
        request: Request<SaveProfileGetRequest>,
    ) -> Result<Response<SaveProfileReply>, Status> {
        self.get_save_profile(request).await
    }

    async fn create_profile(
        &self,
        request: Request<SaveProfileCreateRequest>,
    ) -> Result<Response<SaveProfileMutationReply>, Status> {
        self.create_save_profile(request).await
    }

    async fn update_profile(
        &self,
        request: Request<SaveProfileUpdateRequest>,
    ) -> Result<Response<SaveProfileMutationReply>, Status> {
        self.update_save_profile(request).await
    }

    async fn remove_profile(
        &self,
        request: Request<SaveProfileRemoveRequest>,
    ) -> Result<Response<SaveProfileMutationReply>, Status> {
        self.remove_save_profile(request).await
    }
}
