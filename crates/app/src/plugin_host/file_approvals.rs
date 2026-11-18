//! Host-owned settings for file approvals and standing logical-library root approvals.
use std::path::PathBuf;
use std::sync::Arc;

use gamenode_platform::plugin_files;
use gamenode_platform::plugin_storage::{
    ORIGIN_DECLARED, ORIGIN_USER, PluginLibraryRoot, PluginLibraryRow, operation,
};
use serde::Serialize;
use tonic::Status;

use super::files::{
    APPROVAL_TIMEOUT, AffectedGame, FileAccess, RootApprovalBridge, RootApprover, RootRequest,
    RootScope,
};
use super::library::{CommitGuard, LibraryFeed};
use super::profile::PluginProfiles;
use super::storage::{Namespace, PluginStorage};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginFolderApproval {
    pub id: String,
    pub plugin_id: String,
    pub publisher_fingerprint: String,
    pub installation_name: String,
    pub kind: String,
    pub root: String,
    pub operations: Vec<String>,
    pub affected_games: Vec<String>,
    pub revoked: bool,
    pub declared_by_plugin: bool,
    pub logical_library: bool,
    pub library_id: Option<String>,
    pub watching: Option<bool>,
}

/// Reads and changes approvals for one authenticated plugin in the active account profile.
pub struct PluginFolderApprovals {
    storage: Arc<PluginStorage>,
    profiles: Arc<PluginProfiles>,
    files: Arc<FileAccess>,
    feed: Arc<LibraryFeed>,
    bridge: Arc<RootApprovalBridge>,
}

impl PluginFolderApprovals {
    pub fn new(
        storage: Arc<PluginStorage>,
        profiles: Arc<PluginProfiles>,
        files: Arc<FileAccess>,
        feed: Arc<LibraryFeed>,
        bridge: Arc<RootApprovalBridge>,
    ) -> Self {
        Self {
            storage,
            profiles,
            files,
            feed,
            bridge,
        }
    }

    fn identity(
        &self,
        plugin_id: &str,
        publisher: &str,
    ) -> Result<super::namespace::ExternalNamespaceResolver, String> {
        self.profiles
            .resolver(plugin_id, publisher, None)
            .map_err(|error| error.message().to_owned())
    }

    pub async fn list(
        &self,
        plugin_id: &str,
        publisher: &str,
    ) -> Result<Vec<PluginFolderApproval>, String> {
        let resolver = self.identity(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let (approvals, libraries) = self
            .storage
            .run_for_host(&resolver, epoch, |namespace| {
                Ok((namespace.file_approvals()?, namespace.plugin_libraries()?))
            })
            .await
            .map_err(|error| error.message().to_owned())?;
        let feed_names = self
            .feed
            .folders()
            .into_iter()
            .map(|entry| (entry.entry_key, entry.name))
            .collect::<std::collections::HashMap<_, _>>();
        let mut result = approvals
            .into_iter()
            .map(|approval| {
                let affected_games = approval
                    .affected
                    .iter()
                    .filter_map(|key| feed_names.get(key).cloned())
                    .collect();
                PluginFolderApproval {
                    id: approval.id,
                    plugin_id: plugin_id.to_owned(),
                    publisher_fingerprint: publisher.to_owned(),
                    installation_name: approval.installation_id,
                    kind: approval.kind,
                    root: approval.root,
                    operations: operation_names(approval.operations),
                    affected_games,
                    revoked: approval.revoked_at.is_some(),
                    declared_by_plugin: approval.origin == ORIGIN_DECLARED,
                    logical_library: false,
                    library_id: None,
                    watching: None,
                }
            })
            .collect::<Vec<_>>();
        for library in libraries.into_iter().filter(|row| !row.removed) {
            result.extend(library.roots.into_iter().map(|root| PluginFolderApproval {
                id: root.id,
                plugin_id: plugin_id.to_owned(),
                publisher_fingerprint: publisher.to_owned(),
                installation_name: library.name.clone(),
                kind: "library".to_owned(),
                root: root.path,
                operations: vec![
                    "read".to_owned(),
                    "plugin_discovery".to_owned(),
                    "core_executable_discovery".to_owned(),
                ],
                affected_games: Vec::new(),
                revoked: false,
                declared_by_plugin: root.declared,
                logical_library: true,
                library_id: Some(library.id.clone()),
                watching: Some(root.watching),
            }));
        }
        result.sort_by(|left, right| {
            left.installation_name
                .cmp(&right.installation_name)
                .then(left.kind.cmp(&right.kind))
        });
        Ok(result)
    }

    pub async fn revoke(&self, plugin_id: &str, publisher: &str, id: &str) -> Result<bool, String> {
        let resolver = self.identity(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let id = id.to_owned();
        let profiles = self.profiles.clone();
        self.storage
            .run_for_host(&resolver, epoch, move |namespace| {
                if namespace
                    .file_approvals()?
                    .iter()
                    .any(|approval| approval.id == id)
                {
                    return namespace.revoke_approval(&id);
                }
                let Some((mut library, _)) = find_logical_root(namespace, &id)? else {
                    return Ok(false);
                };
                let expected = library.revision;
                library.roots.retain(|root| root.id != id);
                let guard = profile_guard(profiles, epoch);
                namespace.put_plugin_library(library, expected, &guard)?;
                Ok(true)
            })
            .await
            .map_err(|error| error.message().to_owned())
    }

    /// Stops or resumes Core discovery for one standing logical-library root. File access still
    /// requires the independent Files grant and per-installation approval.
    pub async fn set_watching(
        &self,
        plugin_id: &str,
        publisher: &str,
        id: &str,
        watching: bool,
    ) -> Result<bool, String> {
        let resolver = self.identity(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let id = id.to_owned();
        let profiles = self.profiles.clone();
        self.storage
            .run_for_host(&resolver, epoch, move |namespace| {
                let Some((mut library, mut root)) = find_logical_root(namespace, &id)? else {
                    return Ok(false);
                };
                if root.watching == watching {
                    return Ok(false);
                }
                let expected = library.revision;
                root.watching = watching;
                if let Some(target) = library.roots.iter_mut().find(|target| target.id == id) {
                    *target = root;
                }
                let guard = profile_guard(profiles, epoch);
                namespace.put_plugin_library(library, expected, &guard)?;
                Ok(true)
            })
            .await
            .map_err(|error| error.message().to_owned())
    }

    /// Replaces the selected root of an existing per-installation file approval after a fresh
    /// user selection and confirmation. The persisted approval swap fences old handles.
    pub async fn change_root(
        &self,
        plugin_id: &str,
        publisher: &str,
        id: &str,
    ) -> Result<(), String> {
        let resolver = self.identity(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let _waiting = self
            .files
            .begin_approval(*resolver.storage_owner().key())
            .ok_or_else(|| "a folder approval is already pending for this plugin".to_owned())?;
        let approval_id = id.to_owned();
        let current = self
            .storage
            .run_for_host(&resolver, epoch, move |namespace| {
                namespace
                    .approval(&approval_id)?
                    .ok_or_else(|| Status::not_found("folder approval not found"))
            })
            .await
            .map_err(|error| error.message().to_owned())?;
        if current.revoked_at.is_some() {
            return Err("folder approval is revoked".into());
        }

        let operations = operation_names_static(current.operations);
        let request = RootRequest {
            plugin_id: plugin_id.to_owned(),
            publisher_fingerprint: publisher.to_owned(),
            installation_name: current.installation_id.clone(),
            kind: current.kind.clone(),
            operations: operations.clone(),
            previously_approved: Some(super::files::PreviousRootApproval {
                root: PathBuf::from(&current.root),
                operations: operations.clone(),
            }),
            suggested: None,
            declared: Vec::new(),
            preselected: Some(PathBuf::from(&current.root)),
        };
        let chosen = tokio::time::timeout(APPROVAL_TIMEOUT, self.bridge.choose_root(request))
            .await
            .map_err(|_| "folder selection timed out".to_owned())?
            .map_err(|error| error.message().to_owned())?
            .ok_or_else(|| "folder change cancelled".to_owned())?;

        let files = self.files.clone();
        let feed = self.feed.clone();
        let (root, overlapping, volume_token) = tokio::task::spawn_blocking(move || {
            let root = plugin_files::approve_root(&chosen, &files.reserved)
                .map_err(|_| Status::failed_precondition("folder cannot be approved"))?;
            let overlapping = files.overlapping(&feed, &root.path);
            let volume_token = gamenode_platform::volume::volume_info(&root.path)
                .map(|volume| volume.volume_identity_token)
                .filter(|token| !token.is_empty())
                .ok_or_else(|| Status::failed_precondition("volume identity is unavailable"))?;
            Ok::<_, Status>((root, overlapping, volume_token))
        })
        .await
        .map_err(|_| "folder validation failed".to_owned())?
        .map_err(|error| error.message().to_owned())?;
        let scope = RootScope {
            plugin_id: plugin_id.to_owned(),
            publisher_fingerprint: publisher.to_owned(),
            installation_name: current.installation_id.clone(),
            kind: current.kind.clone(),
            operations,
            root: root.path.clone(),
            declared_by_plugin: false,
            affected: overlapping
                .iter()
                .map(|entry| AffectedGame {
                    name: entry.name.clone(),
                    game_id: entry.game_id,
                })
                .collect(),
        };
        if !tokio::time::timeout(APPROVAL_TIMEOUT, self.bridge.confirm(scope))
            .await
            .map_err(|_| "folder approval timed out".to_owned())?
            .map_err(|error| error.message().to_owned())?
        {
            return Err("folder change declined".into());
        }
        if self.profiles.epoch() != epoch {
            return Err("account profile changed while approval was open".into());
        }

        let mut replacement = current;
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| "no source of randomness".to_owned())?;
        replacement.id = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        replacement.root = root
            .path
            .to_str()
            .ok_or("folder path is not valid Unicode")?
            .to_owned();
        replacement.root_volume = root.identity.volume;
        replacement.root_node = root.identity.node;
        replacement.volume_token = volume_token;
        replacement.affected = super::files::affected_keys(&overlapping);
        replacement.revoked_at = None;
        replacement.origin = ORIGIN_USER.to_owned();
        replacement.approved_at = 0;
        let profiles = self.profiles.clone();
        self.storage
            .run_for_host(&resolver, epoch, move |namespace| {
                let guard = profile_guard(profiles, epoch);
                namespace.save_approval(&replacement, &guard)
            })
            .await
            .map_err(|error| error.message().to_owned())
    }
}

fn find_logical_root(
    namespace: &mut Namespace<'_>,
    id: &str,
) -> Result<Option<(PluginLibraryRow, PluginLibraryRoot)>, Status> {
    for library in namespace
        .plugin_libraries()?
        .into_iter()
        .filter(|row| !row.removed)
    {
        if let Some(root) = library.roots.iter().find(|root| root.id == id).cloned() {
            return Ok(Some((library, root)));
        }
    }
    Ok(None)
}

fn profile_guard(profiles: Arc<PluginProfiles>, epoch: u64) -> CommitGuard {
    Box::new(move || {
        if profiles.epoch() == epoch {
            Ok(())
        } else {
            Err(Status::failed_precondition("account profile changed"))
        }
    })
}

fn operation_names(operations: u8) -> Vec<String> {
    operation_names_static(operations)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn operation_names_static(operations: u8) -> Vec<&'static str> {
    [
        (operation::READ, "read"),
        (operation::WRITE, "write"),
        (operation::DELETE, "delete"),
    ]
    .into_iter()
    .filter_map(|(bit, name)| (operations & bit != 0).then_some(name))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_platform::plugin_storage::FileApproval;

    #[tokio::test]
    async fn settings_list_can_revoke_file_scopes_and_manage_logical_root_monitoring() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Arc::new(PluginStorage::new(temp.path()));
        let profiles = Arc::new(PluginProfiles::default());
        profiles.update(Some("account"), true);
        let publisher = "ab".repeat(32);
        let resolver = profiles.resolver("org.example", &publisher, None).unwrap();
        storage
            .run_for_host(&resolver, profiles.epoch(), |namespace| {
                let guard: CommitGuard = Box::new(|| Ok(()));
                namespace.save_approval(
                    &FileApproval {
                        id: "file-approval".into(),
                        installation_id: "installation-1".into(),
                        game_id: "42".into(),
                        kind: "save".into(),
                        root: "/games/saves".into(),
                        volume_token: "volume".into(),
                        root_volume: 1,
                        root_node: 2,
                        operations: operation::READ,
                        affected: vec!["entry-1".into()],
                        approved_at: 100,
                        revoked_at: None,
                        origin: ORIGIN_USER.into(),
                    },
                    &guard,
                )?;
                namespace.put_plugin_library(
                    PluginLibraryRow {
                        id: "library-1".into(),
                        name: "Scanned games".into(),
                        description: "Fixture".into(),
                        revision: 0,
                        removed: false,
                        roots: vec![PluginLibraryRoot {
                            id: "root-1".into(),
                            name: "Games".into(),
                            path: "/games".into(),
                            volume: 1,
                            node: 3,
                            volume_token: "volume".into(),
                            declared: true,
                            watching: true,
                        }],
                    },
                    0,
                    &guard,
                )?;
                Ok(())
            })
            .await
            .unwrap();

        let bridge = Arc::new(RootApprovalBridge::new(Arc::new(|_| true)));
        let feed = LibraryFeed::new();
        let files = Arc::new(FileAccess::default());
        let approvals = PluginFolderApprovals::new(storage, profiles, files, feed, bridge);

        let listed = approvals.list("org.example", &publisher).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|item| item.id == "root-1"
            && item.logical_library
            && item.watching == Some(true)));
        assert!(
            listed
                .iter()
                .any(|item| item.id == "file-approval" && item.operations == ["read"])
        );

        assert!(
            approvals
                .set_watching("org.example", &publisher, "root-1", false)
                .await
                .unwrap()
        );
        assert!(
            approvals
                .revoke("org.example", &publisher, "root-1")
                .await
                .unwrap()
        );
        assert!(
            approvals
                .revoke("org.example", &publisher, "file-approval")
                .await
                .unwrap()
        );
        assert!(
            !approvals
                .revoke("org.example", &publisher, "file-approval")
                .await
                .unwrap()
        );

        let listed = approvals.list("org.example", &publisher).await.unwrap();
        let file = listed
            .iter()
            .find(|item| item.id == "file-approval")
            .unwrap();
        assert!(file.revoked);
        assert!(listed.iter().all(|item| item.id != "root-1"));
        assert!(
            approvals
                .list("org.other", &publisher)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
