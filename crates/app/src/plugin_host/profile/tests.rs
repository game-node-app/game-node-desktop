use super::*;
use crate::plugin_host::grants::{
    EffectiveGrant, GrantScope, PermissionDecision, PermissionRequest,
};

fn profiles_with(account: Option<&str>, verified: bool) -> PluginProfiles {
    let profiles = PluginProfiles::default();
    profiles.update(account, verified);
    profiles
}

#[test]
fn no_account_session_issues_no_profile_and_reports_unavailable() {
    let profiles = PluginProfiles::default();
    assert_eq!(profiles.current(), None);
    assert!(!profiles.may_start_third_party());
    let error = profiles.require().unwrap_err();
    assert_eq!(error.code(), tonic::Code::Unavailable);
    let error = profiles
        .resolver("org.example.plugin", "fingerprint", None)
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::Unavailable);

    // An empty account id is not an account.
    assert_eq!(
        profiles.update(Some("  "), true),
        ProfileTransition::Unchanged
    );
    assert!(profiles.current().is_none());
}

#[test]
fn profile_id_is_derived_from_the_account_without_exposing_it() {
    let profiles = profiles_with(Some("user-123"), true);
    let profile = profiles.current().unwrap();
    assert_eq!(profile.profile_id(), profile_id_for_account("user-123"));
    assert_ne!(profile.profile_id(), "user-123");
    assert!(!profile.profile_id().contains("user-123"));
    assert_eq!(profile.profile_id().len(), 32);
    assert!(profile.profile_id().chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(
        profile_id_for_account("user-123"),
        profile_id_for_account("user-124")
    );
}

#[test]
fn unverified_restore_uses_the_cached_account_and_verification_keeps_its_contexts() {
    let profiles = PluginProfiles::default();
    assert_eq!(
        profiles.update(Some("cached"), false),
        ProfileTransition::Fenced
    );
    let restored = profiles.current().unwrap();
    assert!(!restored.verified());
    assert!(profiles.may_start_third_party());

    // Verification of the same account is not a profile change.
    assert_eq!(
        profiles.update(Some("cached"), true),
        ProfileTransition::Unchanged
    );
    let verified = profiles.current().unwrap();
    assert!(verified.verified());
    assert_eq!(verified.profile_id(), restored.profile_id());
    assert_eq!(verified.epoch(), restored.epoch());
    assert!(profiles.is_current(restored.epoch()));
}

#[test]
fn sign_out_rejection_and_a_different_verified_account_fence_previous_contexts() {
    let profiles = profiles_with(Some("cached"), false);
    let cached = profiles.current().unwrap();
    let resolver = profiles
        .resolver("org.example.plugin", "fingerprint", Some("example"))
        .unwrap();

    // Verification found a different account.
    assert_eq!(
        profiles.update(Some("other"), true),
        ProfileTransition::Fenced
    );
    let other = profiles.current().unwrap();
    assert_ne!(other.profile_id(), cached.profile_id());
    assert!(!profiles.is_current(cached.epoch()));
    assert!(!resolver.is_current_profile_epoch(profiles.epoch()));

    // Sign-out (or a rejected session) removes the profile and fences again.
    assert_eq!(profiles.update(None, false), ProfileTransition::Fenced);
    assert!(profiles.current().is_none());
    assert!(!profiles.is_current(other.epoch()));
    assert!(!profiles.may_start_third_party());

    // Signing back in with the first account restores its profile id under a fresh epoch, so a
    // context from before sign-out still cannot act.
    profiles.update(Some("cached"), true);
    let again = profiles.current().unwrap();
    assert_eq!(again.profile_id(), cached.profile_id());
    assert!(again.epoch() > other.epoch());
    assert!(!profiles.is_current(cached.epoch()));
}

#[test]
fn namespace_owners_differ_between_accounts_for_the_same_plugin() {
    let profiles = profiles_with(Some("first"), true);
    let first = profiles
        .resolver("org.example.plugin", "fingerprint", Some("example"))
        .unwrap();
    profiles.update(Some("second"), true);
    let second = profiles
        .resolver("org.example.plugin", "fingerprint", Some("example"))
        .unwrap();
    assert_ne!(first.storage_owner(), second.storage_owner());
    assert_ne!(
        first.secret_owner().unwrap(),
        second.secret_owner().unwrap()
    );
}

#[tokio::test]
async fn subscribers_observe_each_profile_change() {
    let profiles = PluginProfiles::default();
    let mut changes = profiles.subscribe();
    profiles.update(Some("account"), false);
    changes.changed().await.unwrap();
    assert!(changes.borrow_and_update().is_some());
    profiles.update(None, false);
    changes.changed().await.unwrap();
    assert!(changes.borrow_and_update().is_none());
}

fn grant_request() -> PermissionRequest {
    PermissionRequest {
        permission_id: "storage.persist".into(),
        required: false,
        allowed_scopes: [GrantScope::OwnPrivateNamespace].into(),
    }
}

fn identity(profile: &ProfileContext) -> PluginIdentity {
    PluginIdentity {
        plugin_id: "org.example.plugin".into(),
        publisher_fingerprint: "fingerprint".into(),
        profile_id: profile.profile_id().into(),
    }
}

#[test]
fn active_profile_exposes_only_its_authenticated_identities_and_activation() {
    let temp = tempfile::tempdir().unwrap();
    let layout = PluginStateLayout::new(temp.path().join("plugins"));
    let profiles = profiles_with(Some("account"), true);
    profiles.set_state_layout(layout.clone());
    let current = profiles.current().unwrap();
    let identity = identity(&current);
    layout
        .open_grants(&current)
        .unwrap()
        .replace_grants(identity.clone(), &[grant_request()], &[])
        .unwrap();

    assert_eq!(profiles.identities().unwrap(), [identity.clone()]);
    assert!(!profiles.is_enabled(&identity.plugin_id).unwrap());
    layout
        .open_activation(&current)
        .unwrap()
        .set_enabled(&identity.plugin_id, true)
        .unwrap();
    assert!(profiles.is_enabled(&identity.plugin_id).unwrap());

    profiles.update(Some("another-account"), true);
    assert!(profiles.identities().unwrap().is_empty());
    assert!(!profiles.is_enabled(&identity.plugin_id).unwrap());
}

#[test]
fn another_account_sees_packages_but_not_grants_or_activation() {
    let temp = tempfile::tempdir().unwrap();
    let layout = PluginStateLayout::new(temp.path().join("plugins"));
    let profiles = profiles_with(Some("first"), true);
    let first = profiles.current().unwrap();

    // OS-user state does not depend on the account.
    let packages = layout.packages_dir();
    let trust = layout.trust_dir();

    let mut grants = layout.open_grants(&first).unwrap();
    grants
        .replace_grants(
            identity(&first),
            &[grant_request()],
            &[PermissionDecision {
                permission_id: "storage.persist".into(),
                scope: GrantScope::OwnPrivateNamespace,
                granted: true,
            }],
        )
        .unwrap();
    let mut activation = layout.open_activation(&first).unwrap();
    activation.set_enabled("org.example.plugin", true).unwrap();

    profiles.update(Some("second"), true);
    let second = profiles.current().unwrap();
    assert_eq!(layout.packages_dir(), packages);
    assert_eq!(layout.trust_dir(), trust);
    assert_ne!(
        layout.account_dir(&first).unwrap(),
        layout.account_dir(&second).unwrap()
    );

    let grants = layout.open_grants(&second).unwrap();
    assert!(
        grants
            .effective_grants(&identity(&second))
            .iter()
            .all(|grant| grant.state != crate::plugin_host::grants::PermissionState::Granted)
    );
    assert_eq!(grants.revision(&identity(&first)), 0);
    let activation = layout.open_activation(&second).unwrap();
    assert!(!activation.is_enabled("org.example.plugin"));
    assert_eq!(activation.enabled().count(), 0);

    // The first account keeps its state for its next sign-in.
    profiles.update(None, false);
    profiles.update(Some("first"), true);
    let first_again = profiles.current().unwrap();
    let grants = layout.open_grants(&first_again).unwrap();
    let effective: Vec<EffectiveGrant> = grants.effective_grants(&identity(&first_again));
    assert!(effective.iter().any(|grant| {
        grant.permission_id == "storage.persist"
            && grant.state == crate::plugin_host::grants::PermissionState::Granted
    }));
    assert!(
        layout
            .open_activation(&first_again)
            .unwrap()
            .is_enabled("org.example.plugin")
    );
}

#[test]
fn removing_plugin_clears_every_accounts_grants_and_activation_but_keeps_private_data() {
    let temp = tempfile::tempdir().unwrap();
    let layout = PluginStateLayout::new(temp.path().join("plugins"));
    let profiles = profiles_with(Some("first"), true);
    let first = profiles.current().unwrap();
    let plugin_id = "org.example.plugin";

    let mut grants = layout.open_grants(&first).unwrap();
    grants
        .replace_grants(
            identity(&first),
            &[grant_request()],
            &[PermissionDecision {
                permission_id: "storage.persist".into(),
                scope: GrantScope::OwnPrivateNamespace,
                granted: true,
            }],
        )
        .unwrap();
    layout
        .open_activation(&first)
        .unwrap()
        .set_enabled(plugin_id, true)
        .unwrap();
    let private_data = layout
        .account_dir(&first)
        .unwrap()
        .join("plugin-data")
        .join(plugin_id);
    std::fs::create_dir_all(&private_data).unwrap();
    std::fs::write(private_data.join("keep.json"), "user data").unwrap();

    profiles.update(Some("second"), true);
    let second = profiles.current().unwrap();
    let mut grants = layout.open_grants(&second).unwrap();
    grants
        .replace_grants(
            identity(&second),
            &[grant_request()],
            &[PermissionDecision {
                permission_id: "storage.persist".into(),
                scope: GrantScope::OwnPrivateNamespace,
                granted: true,
            }],
        )
        .unwrap();
    layout
        .open_activation(&second)
        .unwrap()
        .set_enabled(plugin_id, true)
        .unwrap();

    layout.remove_plugin_account_state(plugin_id).unwrap();

    for profile in [&first, &second] {
        assert!(
            !layout
                .open_activation(profile)
                .unwrap()
                .is_enabled(plugin_id)
        );
        assert!(
            layout
                .open_grants(profile)
                .unwrap()
                .effective_grants(&identity(profile))
                .is_empty()
        );
    }
    assert_eq!(
        std::fs::read_to_string(private_data.join("keep.json")).unwrap(),
        "user data"
    );
}

#[cfg(unix)]
#[test]
fn account_state_is_private_to_the_os_user() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let layout = PluginStateLayout::new(temp.path().join("plugins"));
    let profile = profiles_with(Some("account"), true).current().unwrap();
    let mut activation = layout.open_activation(&profile).unwrap();
    activation.set_enabled("org.example.plugin", true).unwrap();
    let dir = layout.account_dir(&profile).unwrap();
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(dir.join(ACTIVATION_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn playtime_accounting_preference_is_persistent_and_profile_scoped() {
    let temp = tempfile::tempdir().unwrap();
    let layout = PluginStateLayout::new(temp.path().join("plugins"));
    let profiles = PluginProfiles::default();
    profiles.set_state_layout(layout.clone());
    profiles.update(Some("first-account"), true);

    let first_profile = profiles.current().unwrap();
    layout
        .open_activation(&first_profile)
        .unwrap()
        .set_enabled("org.example.reader", true)
        .unwrap();

    let first_profile = profiles.current().unwrap();
    let mut activation = layout.open_activation(&first_profile).unwrap();
    activation.set_enabled("org.example.reader", true).unwrap();

    assert!(
        profiles
            .is_playtime_accounting_enabled("org.example.reader")
            .unwrap()
    );
    profiles
        .set_playtime_accounting_enabled("org.example.reader", false)
        .unwrap();
    assert!(
        !profiles
            .is_playtime_accounting_enabled("org.example.reader")
            .unwrap()
    );
    assert!(
        layout
            .open_activation(&first_profile)
            .unwrap()
            .is_enabled("org.example.reader")
    );

    let restarted = PluginProfiles::default();
    restarted.set_state_layout(layout.clone());
    restarted.update(Some("first-account"), true);
    assert!(
        !restarted
            .is_playtime_accounting_enabled("org.example.reader")
            .unwrap()
    );
    assert!(
        layout
            .open_activation(&first_profile)
            .unwrap()
            .is_enabled("org.example.reader")
    );

    let restarted = PluginProfiles::default();
    restarted.set_state_layout(layout.clone());
    restarted.update(Some("first-account"), true);
    assert!(
        !restarted
            .is_playtime_accounting_enabled("org.example.reader")
            .unwrap()
    );

    profiles.update(Some("second-account"), true);
    assert!(
        profiles
            .is_playtime_accounting_enabled("org.example.reader")
            .unwrap()
    );
    profiles.update(Some("first-account"), true);
    assert!(
        !profiles
            .is_playtime_accounting_enabled("org.example.reader")
            .unwrap()
    );
}

#[test]
fn invalid_plugin_ids_do_not_change_activation() {
    let temp = tempfile::tempdir().unwrap();
    let layout = PluginStateLayout::new(temp.path().join("plugins"));
    let profile = profiles_with(Some("account"), true).current().unwrap();
    let mut activation = layout.open_activation(&profile).unwrap();
    for invalid in ["", "  ", "bad\nid"] {
        assert!(activation.set_enabled(invalid, true).is_err());
    }
    assert_eq!(activation.enabled().count(), 0);
}
