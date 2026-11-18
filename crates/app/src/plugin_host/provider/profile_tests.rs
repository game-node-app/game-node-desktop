//! Third-party providers run inside the active GameNode account profile; official providers
//! are bundled with GameNode and are not bound to it.
use super::*;
use crate::plugin_host::namespace::OwnerKind;
use crate::plugin_host::profile::ProfileTransition;

fn third_party_registration(provider_id: &str) -> Registration {
    Registration {
        provider_id: provider_id.into(),
        plugin_id: format!("{provider_id}.plugin"),
        kind: super::RegistrationKind::Provider,
        origin: RegistrationOrigin::ThirdParty {
            publisher_fingerprint: "cd".repeat(32),
        },
        capabilities: vec![cap::CONTEXT.into()],
        shared_catalog: false,
        playtime: Default::default(),
        services: Arc::new(Services::default()),
    }
}

fn official_registration(provider_id: &str) -> Registration {
    Registration {
        origin: RegistrationOrigin::Official,
        ..third_party_registration(provider_id)
    }
}

fn unavailable(result: Result<impl Sized, Status>) -> bool {
    result.err().map(|status| status.code()) == Some(tonic::Code::Unavailable)
}

async fn wait_until_disconnected(api: &ProviderApi, provider_id: &str) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while api.connected(provider_id) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("third-party session was not fenced");
}

#[tokio::test]
async fn third_party_processes_start_only_inside_an_account_profile() {
    let api = ProviderApi::default();
    api.register(third_party_registration("community")).unwrap();
    api.register(official_registration("bundled")).unwrap();

    assert!(unavailable(api.begin_launch("community")));
    assert!(api.begin_launch("bundled").is_ok());

    // The cached account of an unverified restore is enough to run.
    api.profiles().update(Some("account-a"), false);
    assert!(api.begin_launch("community").is_ok());

    api.profiles().update(None, false);
    assert!(unavailable(api.begin_launch("community")));
}

#[test]
fn pending_third_party_launch_is_rejected_after_profile_changes() {
    let api = ProviderApi::default();
    api.register(third_party_registration("community")).unwrap();
    api.profiles().update(Some("account-a"), true);
    let epoch = api.profiles().current().unwrap().epoch();
    let launch = api
        .begin_launch_for_profile("community", Some(epoch))
        .unwrap();
    let credential = launch
        .credential
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();

    api.profiles().update(Some("account-b"), true);
    let request = gamenode_plugin_protocol::proto::OpenSessionRequest {
        session_credential: credential,
        ..Default::default()
    };
    let error = api.open_session(&request).unwrap_err();
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    assert!(!api.owns_launch(&request.session_credential));
}

#[tokio::test]
async fn a_different_verified_account_fences_third_party_sessions_and_namespaces() {
    let mut h = Harness::third_party("account-a").await;
    let operation = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let (lease, _) = h
        .api
        .lease(Some(client.context().clone()), cap::CONTEXT)
        .unwrap();
    let previous = lease.namespace().unwrap();
    let owner = previous.storage_owner();

    assert_eq!(
        h.api.profiles().update(Some("account-b"), true),
        ProfileTransition::Fenced
    );
    // Issued contexts of the previous profile are stale before the session is even stopped.
    assert!(lease.check().is_err());
    assert!(lease.namespace().is_err());
    let next = h
        .api
        .profiles()
        .resolver("fixture.plugin", &"ab".repeat(32), Some("fixture"))
        .unwrap();
    assert!(!next.owns(&owner, OwnerKind::Storage));

    h.api.fence_third_party_sessions().await;
    assert!(!h.api.connected("fixture"));
    assert!(operation.await.unwrap().is_err());
    assert!(h.api.begin_launch("fixture").is_ok());
}

#[tokio::test]
async fn sign_out_makes_third_party_services_unavailable_and_stops_sessions() {
    let mut h = Harness::third_party("account-a").await;
    h.api
        .spawn_profile_fencing(&tokio::runtime::Handle::current());
    let operation = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let context = client.context().clone();
    assert!(h.api.lease(Some(context.clone()), cap::CONTEXT).is_ok());

    h.api.profiles().update(None, false);
    // No profile is a missing session, not a permission denial.
    assert!(unavailable(h.api.lease(Some(context), cap::CONTEXT)));
    assert!(unavailable(h.api.begin_launch("fixture")));
    wait_until_disconnected(&h.api, "fixture").await;
    assert!(operation.await.unwrap().is_err());
}

#[tokio::test]
async fn verifying_the_cached_account_keeps_third_party_sessions() {
    let mut h = Harness::third_party("account-a").await;
    h.api
        .spawn_profile_fencing(&tokio::runtime::Handle::current());
    let operation = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let (lease, _) = h
        .api
        .lease(Some(client.context().clone()), cap::CONTEXT)
        .unwrap();

    // The harness profile is verified; dropping back to unverified and verifying again is the
    // same account, so contexts stay valid and nothing is fenced.
    for verified in [false, true] {
        assert_eq!(
            h.api.profiles().update(Some("account-a"), verified),
            ProfileTransition::Unchanged
        );
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(h.api.connected("fixture"));
    lease.check().unwrap();
    lease.namespace().unwrap();
    client.finish(completed()).await.unwrap();
    operation.await.unwrap().unwrap();
}

#[tokio::test]
async fn official_providers_ignore_account_profile_changes() {
    let mut h = Harness::new().await;
    h.api
        .spawn_profile_fencing(&tokio::runtime::Handle::current());
    let operation = h.execute(ProviderOperation::Check);
    let client = h.next(ProviderOperation::Check).await;
    let (lease, _) = h
        .api
        .lease(Some(client.context().clone()), cap::CONTEXT)
        .unwrap();
    // Official providers use trusted host adapters, not a third-party namespace.
    assert_eq!(
        lease.namespace().unwrap_err().code(),
        tonic::Code::FailedPrecondition
    );

    h.api.profiles().update(Some("account-a"), true);
    h.api.profiles().update(None, false);
    h.api.fence_third_party_sessions().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(h.api.connected("fixture"));
    lease.check().unwrap();
    client.finish(completed()).await.unwrap();
    operation.await.unwrap().unwrap();
}
