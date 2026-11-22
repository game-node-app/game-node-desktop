//! Host-owned settings for approved runtime executables.
use std::sync::Arc;

use serde::Serialize;

use super::profile::PluginProfiles;
use super::storage::PluginStorage;

/// One active runtime executable approval as the settings list shows it. The volume and node
/// identity and the digest stay in the host.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntimeApproval {
    /// The owned runtime this approval belongs to.
    pub runtime_id: String,
    /// The canonical executable location the user approved.
    pub path: String,
    /// Unix seconds.
    pub approved_at: i64,
}

/// Reads and revokes runtime executable approvals for one plugin in the active account profile.
pub struct PluginRuntimeApprovals {
    storage: Arc<PluginStorage>,
    profiles: Arc<PluginProfiles>,
}

impl PluginRuntimeApprovals {
    pub fn new(storage: Arc<PluginStorage>, profiles: Arc<PluginProfiles>) -> Self {
        Self { storage, profiles }
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

    /// Lists the active approvals of this plugin, publisher and account profile. A revoked
    /// approval is never shown; the next Play asks for a fresh program instead.
    pub async fn list(
        &self,
        plugin_id: &str,
        publisher: &str,
    ) -> Result<Vec<PluginRuntimeApproval>, String> {
        let resolver = self.identity(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let approvals = self
            .storage
            .run_for_host(&resolver, epoch, |namespace| namespace.runtime_approvals())
            .await
            .map_err(|error| error.message().to_owned())?;
        Ok(approvals
            .into_iter()
            .filter(|approval| approval.revoked_at.is_none())
            .map(|approval| PluginRuntimeApproval {
                runtime_id: approval.runtime_id,
                path: approval.path,
                approved_at: approval.approved_at,
            })
            .collect())
    }

    /// Revokes one active approval of this plugin, publisher and account profile. A runtime
    /// another identity owns is never touched. Returns whether this call revoked it.
    pub async fn revoke(
        &self,
        plugin_id: &str,
        publisher: &str,
        runtime_id: &str,
    ) -> Result<bool, String> {
        let resolver = self.identity(plugin_id, publisher)?;
        let epoch = self.profiles.epoch();
        let runtime_id = runtime_id.to_owned();
        self.storage
            .run_for_host(&resolver, epoch, move |namespace| {
                let Some(approval) = namespace.runtime_approval_for(&runtime_id)? else {
                    return Ok(false);
                };
                if approval.revoked_at.is_some() {
                    return Ok(false);
                }
                namespace.revoke_runtime_approval(&runtime_id)
            })
            .await
            .map_err(|error| error.message().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::library::CommitGuard;
    use crate::plugin_host::runtime_access::validate_approval;
    use gamenode_platform::plugin_files::Reserved;
    use gamenode_platform::plugin_storage::RuntimeApproval;

    fn approval(runtime_id: &str, path: &str, approved_at: i64) -> RuntimeApproval {
        RuntimeApproval {
            id: format!("{runtime_id}-approval"),
            runtime_id: runtime_id.into(),
            path: path.into(),
            volume: 1,
            node: 2,
            size: 3,
            digest: "ab".repeat(32),
            approved_at,
            revoked_at: None,
        }
    }

    async fn save(
        storage: &PluginStorage,
        profiles: &PluginProfiles,
        plugin_id: &str,
        publisher: &str,
        approval: &RuntimeApproval,
    ) {
        let resolver = profiles.resolver(plugin_id, publisher, None).unwrap();
        let guard: CommitGuard = Box::new(|| Ok(()));
        let approval = approval.clone();
        storage
            .run_for_host(&resolver, profiles.epoch(), move |namespace| {
                namespace.save_runtime_approval(&approval, &guard)
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn lists_only_this_plugins_active_approvals_and_revokes_once() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Arc::new(PluginStorage::new(temp.path()));
        let profiles = Arc::new(PluginProfiles::default());
        profiles.update(Some("account"), true);
        let publisher = "ab".repeat(32);
        let extension = "dev.richmont.gamenode.dolphin-emu-launch";

        let active = approval("runtime-1", "/usr/bin/dolphin-emu", 100);
        let mut revoked = approval("runtime-2", "/usr/bin/old-dolphin", 50);
        save(&storage, &profiles, extension, &publisher, &active).await;
        revoked.revoked_at = Some(200);
        save(&storage, &profiles, extension, &publisher, &revoked).await;
        let resolver = profiles.resolver(extension, &publisher, None).unwrap();

        let service = PluginRuntimeApprovals::new(storage.clone(), profiles.clone());
        let listed = service.list(extension, &publisher).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].runtime_id, "runtime-1");
        assert_eq!(listed[0].path, "/usr/bin/dolphin-emu");
        assert_eq!(listed[0].approved_at, 100);

        // Another plugin, publisher or profile sees nothing of this namespace.
        assert!(
            service
                .list("dev.richmont.gamenode.dolphin-emu", &publisher)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            service
                .list(extension, &"cd".repeat(32))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !service
                .revoke(extension, &"cd".repeat(32), "runtime-1")
                .await
                .unwrap()
        );
        assert!(
            !service
                .revoke("dev.richmont.gamenode.dolphin-emu", &publisher, "runtime-1")
                .await
                .unwrap()
        );

        assert!(
            service
                .revoke(extension, &publisher, "runtime-1")
                .await
                .unwrap()
        );
        assert!(
            !service
                .revoke(extension, &publisher, "runtime-1")
                .await
                .unwrap()
        );
        assert!(
            service
                .list(extension, &publisher)
                .await
                .unwrap()
                .is_empty()
        );

        // The runtime record no longer launches through the revoked approval.
        let stored = storage
            .run_for_host(&resolver, profiles.epoch(), |namespace| {
                namespace.runtime_approval_for("runtime-1")
            })
            .await
            .unwrap()
            .unwrap();
        assert!(stored.revoked_at.is_some());
        assert!(validate_approval(&stored, &Reserved::default()).is_err());

        // A profile switch moves the service to the new profile's empty namespace.
        profiles.update(Some("other-account"), true);
        assert!(
            service
                .list(extension, &publisher)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !service
                .revoke(extension, &publisher, "runtime-1")
                .await
                .unwrap()
        );
    }
}
