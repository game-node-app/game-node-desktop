//! `ArtworkSet` with an approved file-handle reference (task 6.6): a picture the plugin may read
//! becomes the cover of its own entry, and nothing else can, over the real local socket.

use super::*;
use gamenode_platform::plugin_artwork::MAX_INPUT_BYTES;

const GRANTS: &[&str] = &[
    cap::ARTWORK_WRITE,
    cap::ARTWORK_READ,
    cap::FILES_READ,
    cap::FILES_WRITE,
];

impl Fixture {
    async fn set_cover_from(
        &self,
        key: &str,
        installation_id: &str,
        handle: &str,
        media_type: &str,
    ) -> Result<MutationReply, Status> {
        self.set_cover_with(ArtworkSetRequest {
            mutation: Some(self.mutation(key, 0)),
            installation_id: installation_id.into(),
            file_handle: handle.into(),
            media_type: media_type.into(),
            ..ArtworkSetRequest::default()
        })
        .await
    }
}

/// A fixture with two own entries, each in its own folder, the first one approved for reading.
async fn ready(
    files: &[(&str, &[u8])],
) -> (
    Fixture,
    std::sync::Arc<FakeHost>,
    std::path::PathBuf,
    String,
) {
    let fixture = Fixture::new(GRANTS).await;
    let one = fixture.game_dir("one", files);
    let two = fixture.game_dir("two", &[("other.png", &png(40, 40))]);
    fixture.library_in(&[
        ("fixture:1", Some(7), "One", &one),
        ("fixture:2", Some(8), "Two", &two),
    ]);
    let host = fixture.with_host();
    fixture.user_approving(&one);
    let root = fixture
        .root_handle("id-fixture:1", &["read", "write"])
        .await;
    (fixture, host, one, root)
}

fn code(result: Result<impl std::fmt::Debug, Status>) -> Code {
    result.unwrap_err().code()
}

#[tokio::test]
async fn a_picture_from_an_approved_file_becomes_the_cover_like_inline_bytes() {
    let picture = png(900, 1200);
    let (fixture, host, _dir, root) = ready(&[("cover.png", &picture)]).await;
    let file = fixture.child(&root, "cover.png").await;

    let applied = fixture
        .set_cover_from("k1", "id-fixture:1", &file, "image/png")
        .await
        .unwrap();
    assert_eq!(
        (
            applied.resource_id.as_str(),
            applied.revision,
            applied.outcome
        ),
        ("id-fixture:1", 1, MutationOutcome::Applied as i32)
    );
    // The host stored its own picture, shrunk and re-encoded, exactly as for inline bytes.
    let stored = host.covers.lock().unwrap()["fixture:1"].0.clone().unwrap();
    assert_eq!((stored.width, stored.height), (600, 800));
    assert_eq!(stored.media_type, "image/jpeg");
    assert_eq!(
        fixture
            .read_cover("id-fixture:1", 0)
            .await
            .unwrap()
            .revision,
        1
    );

    // Inline bytes of the same picture are the same picture: nothing changes.
    let inline = fixture
        .set_cover("k2", 1, "id-fixture:1", picture, "image/png")
        .await
        .unwrap();
    assert_eq!(inline.outcome, MutationOutcome::AlreadyApplied as i32);
}

#[tokio::test]
async fn a_retried_request_is_answered_before_the_file_is_read_again() {
    let (fixture, host, dir, root) = ready(&[("cover.png", &png(300, 450))]).await;
    let file = fixture.child(&root, "cover.png").await;
    let first = fixture
        .set_cover_from("k1", "id-fixture:1", &file, "image/png")
        .await
        .unwrap();
    // The file is gone; the retry is a replay and neither reads nor writes anything.
    std::fs::remove_file(dir.join("cover.png")).unwrap();
    let again = fixture
        .set_cover_from("k1", "id-fixture:1", &file, "image/png")
        .await
        .unwrap();
    assert_eq!(again, first);
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    // The same key for another declared type is another request.
    assert_eq!(
        code(
            fixture
                .set_cover_from("k1", "id-fixture:1", &file, "image/jpeg")
                .await
        ),
        Code::FailedPrecondition
    );
    // A new key finds the file missing, and says so without a path.
    let missing = fixture
        .set_cover_from("k2", "id-fixture:1", &file, "image/png")
        .await
        .unwrap_err();
    assert_eq!(missing.code(), Code::NotFound);
    assert!(!missing.message().contains("cover.png"));
}

#[tokio::test]
async fn the_request_must_choose_exactly_one_source_and_name_its_type() {
    let (fixture, host, _dir, root) = ready(&[("cover.png", &png(300, 450))]).await;
    let file = fixture.child(&root, "cover.png").await;
    let both = fixture
        .set_cover_with(ArtworkSetRequest {
            mutation: Some(fixture.mutation("k", 0)),
            installation_id: "id-fixture:1".into(),
            encoded_image: png(10, 10),
            file_handle: file.clone(),
            media_type: "image/png".into(),
            ..ArtworkSetRequest::default()
        })
        .await;
    assert_eq!(code(both), Code::InvalidArgument);
    assert_eq!(
        code(fixture.set_cover_from("k", "id-fixture:1", &file, "").await),
        Code::InvalidArgument
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn reading_a_file_needs_files_read_besides_artwork_write() {
    // Only the write side of both: a handle exists, but reading the file is not granted.
    let fixture = Fixture::new(&[cap::ARTWORK_WRITE, cap::FILES_WRITE]).await;
    let dir = fixture.game_dir("one", &[("cover.png", &png(300, 450))]);
    fixture.library_in(&[("fixture:1", Some(7), "One", &dir)]);
    let host = fixture.with_host();
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", &["write"]).await;
    let created = fixture
        .write_to(&root, "new.png", &png(300, 450), 0, true, "w")
        .await
        .unwrap();
    let denied = fixture
        .set_cover_from("k", "id-fixture:1", &created.resource_id, "image/png")
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);

    // And files.read alone does not allow changing a cover.
    let fixture = Fixture::new(&[cap::FILES_READ]).await;
    let dir = fixture.game_dir("one", &[("cover.png", &png(300, 450))]);
    fixture.library_in(&[("fixture:1", Some(7), "One", &dir)]);
    let host = fixture.with_host();
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", &["read"]).await;
    let file = fixture.child(&root, "cover.png").await;
    assert_eq!(
        code(
            fixture
                .set_cover_from("k", "id-fixture:1", &file, "image/png")
                .await
        ),
        Code::PermissionDenied
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_handle_of_another_entry_or_session_or_a_made_up_reference_changes_nothing() {
    let (mut fixture, host, _dir, root) = ready(&[("cover.png", &png(300, 450))]).await;
    let file = fixture.child(&root, "cover.png").await;

    // A handle from the approval of another entry's folder (the plugin's own, but not for this
    // entry) is refused for this entry.
    let two = fixture.base().join("games").join("two");
    let user = fixture.user_approving(&two);
    let other_root = fixture.root_handle("id-fixture:2", &["read"]).await;
    drop(user);
    let other = fixture.child(&other_root, "other.png").await;
    assert_eq!(
        code(
            fixture
                .set_cover_from("k1", "id-fixture:1", &other, "image/png")
                .await
        ),
        Code::PermissionDenied
    );
    // It works for the entry it was approved for.
    assert!(
        fixture
            .set_cover_from("k1", "id-fixture:2", &other, "image/png")
            .await
            .is_ok()
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);

    // A path, a URL, a forged or edited handle and a directory are all refused; a handle is the
    // only reference there is, so none of them is opened as a file or fetched.
    let mut edited = file.clone();
    edited.pop();
    edited.push('A');
    for (n, reference) in [
        "/etc/passwd",
        "../cover.png",
        "cover.png",
        "https://example.com/c.png",
        "file:///etc/passwd",
        "forged",
        edited.as_str(),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            code(
                fixture
                    .set_cover_from(&format!("bad{n}"), "id-fixture:1", reference, "image/png")
                    .await
            ),
            Code::NotFound,
            "{reference}"
        );
    }
    assert_eq!(
        code(
            fixture
                .set_cover_from("dir", "id-fixture:1", &root, "image/png")
                .await
        ),
        Code::FailedPrecondition
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    assert!(!host.covers.lock().unwrap().contains_key("fixture:1"));

    // After the session ends, the handles of the old one open nothing.
    fixture.reconnect().await;
    assert_eq!(
        code(
            fixture
                .set_cover_from("late", "id-fixture:1", &file, "image/png")
                .await
        ),
        Code::NotFound
    );
    assert!(!host.covers.lock().unwrap().contains_key("fixture:1"));
}

#[tokio::test]
async fn a_revoked_approval_or_grant_stops_the_handle_and_a_foreign_entry_is_never_read() {
    let (fixture, host, _dir, root) = ready(&[("cover.png", &png(300, 450))]).await;
    let file = fixture.child(&root, "cover.png").await;

    // An entry that is not the caller's is NOT_FOUND and its handle is not even opened.
    assert_eq!(
        code(
            fixture
                .set_cover_from("foreign", "id-steam:620", &file, "image/png")
                .await
        ),
        Code::NotFound
    );

    // The user revokes the approval: the handle opens nothing.
    let id = fixture
        .files
        .open_handle(&fixture.context().session_id, &root)
        .unwrap()
        .0;
    let caller = fixture.caller();
    fixture
        .storage
        .run(
            &caller,
            |_| true,
            move |namespace| namespace.revoke_approval(&id),
        )
        .await
        .unwrap();
    assert_eq!(
        code(
            fixture
                .set_cover_from("revoked", "id-fixture:1", &file, "image/png")
                .await
        ),
        Code::NotFound
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);

    // A revoked grant stops it even though a new approval stands.
    let fresh = fixture.root_handle("id-fixture:1", &["read"]).await;
    let file = fixture.child(&fresh, "cover.png").await;
    fixture
        .api
        .revoke_session_permission("fixture", cap::FILES_READ)
        .await;
    assert_eq!(
        code(
            fixture
                .set_cover_from("grant", "id-fixture:1", &file, "image/png")
                .await
        ),
        Code::PermissionDenied
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn losing_files_read_before_the_commit_writes_nothing() {
    let (fixture, host, _dir, root) = ready(&[("cover.png", &png(300, 450))]).await;
    let file = fixture.child(&root, "cover.png").await;
    *host.revoke_before_commit.lock().unwrap() = Some((fixture.api.clone(), cap::FILES_READ));
    assert_eq!(
        code(
            fixture
                .set_cover_from("k", "id-fixture:1", &file, "image/png")
                .await
        ),
        Code::PermissionDenied
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    assert!(host.covers.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_file_over_the_byte_limit_or_not_a_picture_is_refused_before_it_is_decoded_or_stored() {
    let big = vec![0u8; MAX_INPUT_BYTES + 1];
    let exact = {
        // A real picture padded to exactly the limit is still too far from a valid PNG to decode,
        // so it proves the bound is "more than the limit", not "the limit".
        let mut bytes = png(300, 450);
        bytes.resize(MAX_INPUT_BYTES, 0);
        bytes
    };
    let (fixture, host, _dir, root) = ready(&[
        ("big.png", &big),
        ("exact.png", &exact),
        ("notes.png", b"this is not a picture"),
        ("empty.png", b""),
    ])
    .await;

    let over = fixture.child(&root, "big.png").await;
    let status = fixture
        .set_cover_from("big", "id-fixture:1", &over, "image/png")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(status.message().contains("larger"), "{}", status.message());

    // The inline path says the same thing about the same size.
    let inline = fixture
        .set_cover("inline", 0, "id-fixture:1", big, "image/png")
        .await
        .unwrap_err();
    assert_eq!(inline.code(), Code::InvalidArgument);
    assert_eq!(inline.message(), status.message());

    // At the limit the file is read and decoded; trailing zeros after a PNG are not a reason
    // to refuse it as too large.
    let at = fixture.child(&root, "exact.png").await;
    let result = fixture
        .set_cover_from("exact", "id-fixture:1", &at, "image/png")
        .await;
    if let Err(status) = &result {
        assert!(!status.message().contains("larger"), "{}", status.message());
    }

    let writes = host.writes.load(Ordering::SeqCst);
    for (key, name) in [("text", "notes.png"), ("empty", "empty.png")] {
        let file = fixture.child(&root, name).await;
        assert_eq!(
            code(
                fixture
                    .set_cover_from(key, "id-fixture:1", &file, "image/png")
                    .await
            ),
            Code::InvalidArgument,
            "{name}"
        );
    }
    // A type that does not match the bytes is refused like inline bytes.
    let png_file = {
        let (fixture2, _h, _d, root2) = ready(&[("c.png", &png(100, 150))]).await;
        let file = fixture2.child(&root2, "c.png").await;
        code(
            fixture2
                .set_cover_from("mismatch", "id-fixture:1", &file, "image/jpeg")
                .await,
        )
    };
    assert_eq!(png_file, Code::InvalidArgument);
    assert_eq!(host.writes.load(Ordering::SeqCst), writes);
}

#[tokio::test]
async fn the_sdk_helper_sends_what_the_raw_call_sends() {
    let (fixture, host, _dir, root) = ready(&[("cover.png", &png(600, 900))]).await;
    let sdk = fixture.host.resources().unwrap();
    let file = fixture.child(&root, "cover.png").await;
    let key = gamenode_plugin_sdk::idempotency_key();
    let written = sdk
        .set_cover_from_file("id-fixture:1", 0, &key, &file, "image/png")
        .await
        .unwrap();
    assert_eq!(written.outcome, gamenode_plugin_sdk::Outcome::Applied);
    // The same request by hand under the SDK's key is a replay: the host only answers that way
    // for an identical request.
    let by_hand = fixture
        .set_cover_from(&key, "id-fixture:1", &file, "image/png")
        .await
        .unwrap();
    assert_eq!(
        gamenode_plugin_sdk::Written::try_from(by_hand).unwrap(),
        written
    );
    assert_eq!(host.writes.load(Ordering::SeqCst), 1);
    // Refusals are the host's own.
    let status = sdk
        .set_cover_from_file("id-fixture:1", 1, "k2", "/etc/passwd", "image/png")
        .await
        .unwrap_err();
    assert_eq!(status.code(), gamenode_plugin_sdk::Code::NotFound);
}
