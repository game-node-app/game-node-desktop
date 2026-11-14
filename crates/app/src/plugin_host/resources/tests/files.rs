//! `FilesRequestAccess`/`List`/`Read`/`Write`/`Delete` over the real local socket (task 6.2),
//! with a scripted stand-in for the user's approval dialog.

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;

use super::*;
use crate::plugin_host::files::{AffectedGame, RootApprover, RootRequest, RootScope};
use crate::plugin_host::provider::ResourceCaller;

const FILES: &[&str] = &[cap::FILES_READ, cap::FILES_WRITE, cap::FILES_DELETE];
const ALL: &[&str] = &["read", "write", "delete"];
const FILES_AND_STORAGE: &[&str] = &[
    cap::FILES_READ,
    cap::FILES_WRITE,
    cap::FILES_DELETE,
    cap::STORAGE_READ,
    cap::STORAGE_WRITE,
    cap::STORAGE_PERSIST,
];

/// The user: picks `root`, and consents or not.
#[derive(Default)]
pub(super) struct FakeApprover {
    pub(super) root: StdMutex<Option<PathBuf>>,
    pub(super) consent: AtomicBool,
    asked: StdMutex<Vec<RootRequest>>,
    confirmed: StdMutex<Vec<RootScope>>,
    gate: StdMutex<Option<Arc<tokio::sync::Notify>>>,
}

#[tonic::async_trait]
impl RootApprover for FakeApprover {
    async fn choose_root(&self, request: RootRequest) -> Result<Option<PathBuf>, Status> {
        self.asked.lock().unwrap().push(request);
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.notified().await;
        }
        Ok(self.root.lock().unwrap().clone())
    }

    async fn confirm(&self, scope: RootScope) -> Result<bool, Status> {
        self.confirmed.lock().unwrap().push(scope);
        Ok(self.consent.load(Ordering::SeqCst))
    }
}

impl FakeApprover {
    pub(super) fn asked(&self) -> usize {
        self.asked.lock().unwrap().len()
    }
}

fn names(reply: &FilesListReply) -> Vec<&str> {
    reply
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect()
}

impl Fixture {
    /// A user who approves `root`.
    pub(super) fn user_approving(&self, root: &Path) -> Arc<FakeApprover> {
        let approver = Arc::new(FakeApprover::default());
        *approver.root.lock().unwrap() = Some(root.to_path_buf());
        approver.consent.store(true, Ordering::SeqCst);
        self.files.set_approver(approver.clone());
        approver
    }

    /// The plugin as the resource services see it, for operations the UI performs.
    pub(super) fn caller(&self) -> ResourceCaller {
        self.api
            .authorize_resource(Some(&self.context()), cap::FILES_READ)
            .unwrap()
    }

    pub(super) fn base(&self) -> PathBuf {
        self._temp.path().to_path_buf()
    }

    /// `<temp>/games/<name>` holding `files`.
    pub(super) fn game_dir(&self, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = self.base().join("games").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, contents) in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        dir
    }

    /// Library rows `(entry key, game, name, install folder)`; each gets the id `id-<key>`.
    pub(super) fn library_in(&self, rows: &[(&str, Option<u64>, &str, &Path)]) {
        let missing = self.feed.sync(
            rows.iter()
                .map(|(key, game_id, name, dir)| LibrarySource {
                    entry_key: (*key).into(),
                    provider_id: key.split(':').next().unwrap().into(),
                    game_id: *game_id,
                    name: (*name).into(),
                    install_state: InstallState::Installed,
                    display_name: None,
                    metadata_revision: 0,
                    artwork_revision: 0,
                    install_dir: dir.to_string_lossy().into_owned(),
                })
                .collect(),
        );
        let ids = missing
            .iter()
            .map(|key| (key.clone(), format!("id-{key}")))
            .collect();
        self.feed.assign_ids(&missing, ids);
    }

    pub(super) async fn request_access(
        &self,
        installation: &str,
        operations: &[&str],
        kinds: &[&str],
    ) -> Result<FileAccessReply, Status> {
        self.client()
            .files_request_access(FilesRequestAccessRequest {
                mutation: Some(self.mutation("access", 0)),
                game_id: String::new(),
                installation_id: installation.into(),
                requested_operations: operations.iter().map(|op| (*op).into()).collect(),
                resource_kinds: kinds.iter().map(|kind| (*kind).into()).collect(),
            })
            .await
            .map(Response::into_inner)
    }

    /// The root handle of an approved `installation` folder.
    pub(super) async fn root_handle(&self, installation: &str, operations: &[&str]) -> String {
        self.request_access(installation, operations, &[])
            .await
            .unwrap()
            .handles
            .remove(0)
            .handle
    }

    pub(super) async fn ls(&self, handle: &str) -> Result<FilesListReply, Status> {
        self.ls_page(handle, 0, "").await
    }

    async fn ls_page(
        &self,
        handle: &str,
        page_size: u32,
        cursor: &str,
    ) -> Result<FilesListReply, Status> {
        self.client()
            .files_list(FilesListRequest {
                context: Some(self.context()),
                directory_handle: handle.into(),
                cursor: cursor.into(),
                page_size,
            })
            .await
            .map(Response::into_inner)
    }

    pub(super) async fn cat(
        &self,
        handle: &str,
        offset: u64,
        max_bytes: u64,
    ) -> Result<FileReadReply, Status> {
        self.client()
            .files_read(FilesReadRequest {
                context: Some(self.context()),
                file_handle: handle.into(),
                offset,
                max_bytes,
            })
            .await
            .map(Response::into_inner)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn write_to(
        &self,
        handle: &str,
        name: &str,
        contents: &[u8],
        offset: u64,
        truncate: bool,
        idempotency: &str,
    ) -> Result<MutationReply, Status> {
        self.client()
            .files_write(FilesWriteRequest {
                mutation: Some(self.mutation(idempotency, 0)),
                file_handle: handle.into(),
                offset,
                contents: contents.to_vec(),
                truncate,
                name: name.into(),
            })
            .await
            .map(Response::into_inner)
    }

    pub(super) async fn rm(
        &self,
        handle: &str,
        idempotency: &str,
    ) -> Result<MutationReply, Status> {
        self.client()
            .files_delete(FilesDeleteRequest {
                mutation: Some(self.mutation(idempotency, 0)),
                file_handle: handle.into(),
            })
            .await
            .map(Response::into_inner)
    }

    /// The handle of the entry called `name` in a listing.
    pub(super) async fn child(&self, directory: &str, name: &str) -> String {
        self.ls(directory)
            .await
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("no entry {name}"))
            .handle
    }
}

fn code(result: Result<impl std::fmt::Debug, Status>) -> Code {
    result.unwrap_err().code()
}

#[tokio::test]
async fn an_approved_folder_is_listed_read_written_and_deleted() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir(
        "portal",
        &[("readme.txt", b"hello world"), ("saves/slot1.sav", b"s1")],
    );
    fixture.library_in(&[("fixture:1", Some(7), "Portal", &dir)]);
    let user = fixture.user_approving(&dir);

    let reply = fixture
        .request_access("id-fixture:1", ALL, &[])
        .await
        .unwrap();
    let [root] = reply.handles.as_slice() else {
        panic!("one handle expected")
    };
    assert_eq!(root.resource_kind, "installation");
    assert!(root.writable);
    assert_eq!(root.display_name, "Portal (installation)");
    // The user was shown the folder the library knows and the full scope.
    {
        let asked = user.asked.lock().unwrap();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].plugin_id, "fixture.plugin");
        assert_eq!(asked[0].installation_name, "Portal");
        assert_eq!(asked[0].operations, ["read", "write", "delete"]);
        assert_eq!(asked[0].suggested.as_deref(), Some(dir.as_path()));
        let scopes = user.confirmed.lock().unwrap();
        assert_eq!(scopes[0].root, std::fs::canonicalize(&dir).unwrap());
        assert_eq!(
            scopes[0].affected,
            [AffectedGame {
                name: "Portal".into(),
                game_id: Some(7)
            }]
        );
    }

    let listing = fixture.ls(&root.handle).await.unwrap();
    assert_eq!(names(&listing), ["readme.txt", "saves"]);
    assert!(listing.entries[1].directory);
    assert_eq!(listing.entries[0].size_bytes, 11);

    let readme = fixture.child(&root.handle, "readme.txt").await;
    let read = fixture.cat(&readme, 6, 0).await.unwrap();
    assert_eq!(
        (read.contents.as_slice(), read.eof, read.next_offset),
        (&b"world"[..], true, 11)
    );

    let saves = fixture.child(&root.handle, "saves").await;
    assert_eq!(names(&fixture.ls(&saves).await.unwrap()), ["slot1.sav"]);

    // Create a file, read it back through the returned handle, overwrite it, then delete it.
    let created = fixture
        .write_to(&saves, "slot2.sav", b"new save", 0, false, "w1")
        .await
        .unwrap();
    assert_eq!(created.outcome, MutationOutcome::Applied as i32);
    assert_eq!(
        std::fs::read(dir.join("saves/slot2.sav")).unwrap(),
        b"new save"
    );
    assert_eq!(
        fixture
            .cat(&created.resource_id, 0, 0)
            .await
            .unwrap()
            .contents,
        b"new save"
    );
    fixture
        .write_to(&created.resource_id, "", b"X", 0, true, "w2")
        .await
        .unwrap();
    assert_eq!(std::fs::read(dir.join("saves/slot2.sav")).unwrap(), b"X");
    fixture.rm(&created.resource_id, "d1").await.unwrap();
    assert!(!dir.join("saves/slot2.sav").exists());
    assert_eq!(
        code(fixture.cat(&created.resource_id, 0, 0).await),
        Code::NotFound
    );
}

#[tokio::test]
async fn nothing_is_reachable_without_the_grants_and_each_operation_needs_its_own() {
    let none = Fixture::new(&[]).await;
    assert_eq!(
        code(none.request_access("id-x", &["read"], &[]).await),
        Code::PermissionDenied
    );
    assert_eq!(code(none.cat("h", 0, 0).await), Code::PermissionDenied);
    assert_eq!(code(none.ls("h").await), Code::PermissionDenied);
    assert_eq!(
        code(none.write_to("h", "", b"x", 0, false, "k").await),
        Code::PermissionDenied
    );
    assert_eq!(code(none.rm("h", "k").await), Code::PermissionDenied);

    // Asking for write needs files.write, whatever else is granted.
    let reader = Fixture::new(&[cap::FILES_READ]).await;
    let dir = reader.game_dir("g", &[("a.txt", b"a")]);
    reader.library_in(&[("fixture:1", None, "G", &dir)]);
    let user = reader.user_approving(&dir);
    let status = reader
        .request_access("id-fixture:1", &["read", "write"], &[])
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(
        user.asked(),
        0,
        "the user is not asked for what the plugin may not even request"
    );

    // With only read, an approval for read works and mutations stay denied by the grants.
    let root = reader.root_handle("id-fixture:1", &["read"]).await;
    let file = reader.child(&root, "a.txt").await;
    assert_eq!(reader.cat(&file, 0, 0).await.unwrap().contents, b"a");
    assert_eq!(
        code(reader.write_to(&file, "", b"x", 0, false, "w").await),
        Code::PermissionDenied
    );
    assert_eq!(code(reader.rm(&file, "d").await), Code::PermissionDenied);
    assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"a");
}

#[tokio::test]
async fn write_and_delete_are_independent_in_the_grants_and_in_the_approval() {
    // Granted all three, but the user approved only read and write.
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture
        .root_handle("id-fixture:1", &["read", "write"])
        .await;
    let file = fixture.child(&root, "a.txt").await;
    fixture
        .write_to(&file, "", b"b", 0, false, "w")
        .await
        .unwrap();
    let status = fixture.rm(&file, "d").await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert!(
        status.message().contains("not approved for delete"),
        "{}",
        status.message()
    );
    assert!(dir.join("a.txt").exists());

    // Approved for delete too, but the plugin's delete grant was revoked.
    let wider = fixture.root_handle("id-fixture:1", ALL).await;
    let file = fixture.child(&wider, "a.txt").await;
    fixture
        .api
        .revoke_session_permission("fixture", cap::FILES_DELETE)
        .await;
    assert_eq!(code(fixture.rm(&file, "d2").await), Code::PermissionDenied);
    assert!(dir.join("a.txt").exists());
    // Write still works: the grants are independent.
    fixture
        .write_to(&file, "", b"c", 0, false, "w2")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_declined_or_impossible_approval_gives_no_handle_and_is_not_remembered() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);

    // No dialog connected in this build.
    assert_eq!(
        code(fixture.request_access("id-fixture:1", &["read"], &[]).await),
        Code::FailedPrecondition
    );

    let user = fixture.user_approving(&dir);
    *user.root.lock().unwrap() = None;
    assert_eq!(
        code(fixture.request_access("id-fixture:1", &["read"], &[]).await),
        Code::PermissionDenied
    );
    *user.root.lock().unwrap() = Some(dir.clone());
    user.consent.store(false, Ordering::SeqCst);
    assert_eq!(
        code(fixture.request_access("id-fixture:1", &["read"], &[]).await),
        Code::PermissionDenied
    );
    // A folder that is not there.
    *user.root.lock().unwrap() = Some(fixture.base().join("missing"));
    user.consent.store(true, Ordering::SeqCst);
    assert_eq!(
        code(fixture.request_access("id-fixture:1", &["read"], &[]).await),
        Code::FailedPrecondition
    );
    let asked = user.asked();

    // None of that left an approval behind: the next request asks again, and works.
    *user.root.lock().unwrap() = Some(dir.clone());
    fixture
        .request_access("id-fixture:1", &["read"], &[])
        .await
        .unwrap();
    assert_eq!(user.asked(), asked + 1);
}

#[tokio::test]
async fn an_approval_is_remembered_and_reused_but_a_wider_request_asks_again() {
    let first = Fixture::new(FILES).await;
    let dir = first.game_dir("g", &[("a.txt", b"a")]);
    first.library_in(&[("fixture:1", None, "G", &dir)]);
    let user = first.user_approving(&dir);
    first
        .request_access("id-fixture:1", &["read"], &[])
        .await
        .unwrap();
    assert_eq!(user.asked(), 1);

    // Asking again for the same or less does not bother the user.
    first
        .request_access("id-fixture:1", &["read"], &[])
        .await
        .unwrap();
    assert_eq!(user.asked(), 1);
    // Wanting write as well is a wider scope.
    first
        .request_access("id-fixture:1", &["read", "write"], &[])
        .await
        .unwrap();
    assert_eq!(user.asked(), 2);
    first
        .request_access("id-fixture:1", &["write"], &[])
        .await
        .unwrap();
    assert_eq!(user.asked(), 2);

    // After a restart of the host the approval is still there: it belongs to the profile.
    let second = Fixture::restarted(FILES, first.plugin_data.clone()).await;
    second.library_in(&[("fixture:1", None, "G", &dir)]);
    let nobody = Arc::new(FakeApprover::default());
    second.files.set_approver(nobody.clone());
    let root = second.root_handle("id-fixture:1", &["read", "write"]).await;
    assert_eq!(names(&second.ls(&root).await.unwrap()), ["a.txt"]);
    assert_eq!(nobody.asked(), 0);
}

#[tokio::test]
async fn handles_belong_to_the_session_that_received_them() {
    let mut fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    let user = fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", ALL).await;
    let file = fixture.child(&root, "a.txt").await;

    fixture.reconnect().await;
    // The old session's handles mean nothing now, however they are used.
    assert_eq!(code(fixture.ls(&root).await), Code::NotFound);
    assert_eq!(code(fixture.cat(&file, 0, 0).await), Code::NotFound);
    assert_eq!(code(fixture.rm(&file, "d").await), Code::NotFound);
    // The approval is kept, so new handles come without asking.
    let fresh = fixture.root_handle("id-fixture:1", ALL).await;
    assert_ne!(fresh, root);
    assert_eq!(names(&fixture.ls(&fresh).await.unwrap()), ["a.txt"]);
    assert_eq!(user.asked(), 1);
    assert!(dir.join("a.txt").exists());
}

#[tokio::test]
async fn forged_edited_and_empty_handles_open_nothing() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", ALL).await;
    let file = fixture.child(&root, "a.txt").await;

    let mut flipped = file.clone().into_bytes();
    flipped[20] = if flipped[20] == b'0' { b'1' } else { b'0' };
    for bad in [
        String::new(),
        "x".into(),
        "../../etc/passwd".into(),
        "/etc/passwd".into(),
        file[..file.len() - 2].to_owned(),
        String::from_utf8(flipped).unwrap(),
        "0".repeat(200),
    ] {
        let expected = if bad.is_empty() {
            Code::InvalidArgument
        } else {
            Code::NotFound
        };
        assert_eq!(code(fixture.cat(&bad, 0, 0).await), expected, "{bad:?}");
        assert_eq!(code(fixture.ls(&bad).await), expected, "{bad:?}");
        assert_eq!(code(fixture.rm(&bad, "d").await), expected, "{bad:?}");
    }
    // A cursor is not a handle and a handle is not a cursor.
    assert_eq!(
        code(fixture.ls_page(&root, 1, &file).await),
        Code::InvalidArgument
    );
    assert!(dir.join("a.txt").exists());
}

#[tokio::test]
async fn a_sibling_games_folder_is_out_of_reach() {
    let fixture = Fixture::new(FILES).await;
    let a = fixture.game_dir("a", &[("a.sav", b"a")]);
    let b = fixture.game_dir("b", &[("b.sav", b"b")]);
    fixture.library_in(&[("fixture:1", None, "A", &a), ("fixture:2", None, "B", &b)]);
    let user = fixture.user_approving(&a);

    let root_a = fixture.root_handle("id-fixture:1", ALL).await;
    // A's folder shows only A's files and nothing links to the sibling.
    assert_eq!(names(&fixture.ls(&root_a).await.unwrap()), ["a.sav"]);
    // Approving A did not approve B: B asks the user on its own, and is shown B.
    *user.root.lock().unwrap() = Some(b.clone());
    let root_b = fixture.root_handle("id-fixture:2", &["read"]).await;
    assert_eq!(names(&fixture.ls(&root_b).await.unwrap()), ["b.sav"]);
    assert_eq!(user.asked(), 2);
    assert_eq!(
        user.asked.lock().unwrap()[1].suggested.as_deref(),
        Some(b.as_path())
    );
    // B's read-only approval does not let A's handle write to B or B's handle write at all.
    let b_file = fixture.child(&root_b, "b.sav").await;
    assert_eq!(
        code(fixture.write_to(&b_file, "", b"x", 0, false, "w").await),
        Code::PermissionDenied
    );
    assert_eq!(std::fs::read(b.join("b.sav")).unwrap(), b"b");
}

#[tokio::test]
async fn another_providers_or_unknown_installations_cannot_be_approved() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[]);
    fixture.library_in(&[
        ("fixture:1", Some(5), "G", &dir),
        ("steam:9", Some(6), "Other", &dir),
    ]);
    let user = fixture.user_approving(&dir);
    assert_eq!(
        code(fixture.request_access("id-steam:9", &["read"], &[]).await),
        Code::NotFound
    );
    assert_eq!(
        code(fixture.request_access("id-nope", &["read"], &[]).await),
        Code::NotFound
    );
    // A game named for another match does not select the installation either.
    let status = fixture
        .client()
        .files_request_access(FilesRequestAccessRequest {
            mutation: Some(fixture.mutation("k", 0)),
            game_id: "99".into(),
            installation_id: "id-fixture:1".into(),
            requested_operations: vec!["read".into()],
            resource_kinds: vec![],
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::NotFound);
    assert_eq!(user.asked(), 0);
}

#[tokio::test]
async fn a_shared_folder_needs_the_whole_scope_confirmed_and_holds_only_while_it_is_complete() {
    let fixture = Fixture::new(FILES).await;
    let shared = fixture.base().join("games");
    let a = fixture.game_dir("a", &[("a.sav", b"a")]);
    let b = fixture.game_dir("b", &[("b.sav", b"b")]);
    fixture.library_in(&[
        ("fixture:1", Some(1), "A", &a),
        ("fixture:2", Some(2), "B", &b),
    ]);
    // The user approves the parent that holds both games for A.
    let user = fixture.user_approving(&shared);
    let root = fixture.root_handle("id-fixture:1", &["read"]).await;
    let scope = user.confirmed.lock().unwrap()[0].clone();
    assert_eq!(
        scope.affected,
        [
            AffectedGame {
                name: "A".into(),
                game_id: Some(1)
            },
            AffectedGame {
                name: "B".into(),
                game_id: Some(2)
            },
        ],
        "the sibling is named in the confirmation"
    );
    assert_eq!(names(&fixture.ls(&root).await.unwrap()), ["a", "b"]);

    // A game installed there later was never confirmed: the approval stops covering it.
    let c = fixture.game_dir("c", &[]);
    fixture.library_in(&[
        ("fixture:1", Some(1), "A", &a),
        ("fixture:2", Some(2), "B", &b),
        ("fixture:3", Some(3), "C", &c),
    ]);
    assert_eq!(code(fixture.ls(&root).await), Code::FailedPrecondition);
    // Asking again shows the new, complete scope; confirming it restores access.
    let asked = user.asked();
    let again = fixture.root_handle("id-fixture:1", &["read"]).await;
    assert_eq!(user.asked(), asked + 1);
    assert_eq!(
        user.confirmed
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .affected
            .len(),
        3
    );
    assert_eq!(names(&fixture.ls(&again).await.unwrap()), ["a", "b", "c"]);
}

#[tokio::test]
async fn a_game_leaving_a_shared_folder_also_needs_a_new_approval() {
    let fixture = Fixture::new(FILES).await;
    let a = fixture.game_dir("a", &[]);
    let b = fixture.game_dir("b", &[]);
    fixture.library_in(&[("fixture:1", None, "A", &a), ("fixture:2", None, "B", &b)]);
    fixture.user_approving(&fixture.base().join("games"));
    let root = fixture.root_handle("id-fixture:1", &["read"]).await;
    fixture.library_in(&[("fixture:1", None, "A", &a)]);
    assert_eq!(code(fixture.ls(&root).await), Code::FailedPrecondition);
}

#[tokio::test]
async fn the_hosts_own_folders_are_excluded_even_under_an_approved_parent() {
    let fixture = Fixture::new(FILES_AND_STORAGE).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    // A durable value makes the plugin's private store exist inside the reserved folder.
    fixture
        .store("k", b"v", StorageDurability::Persistent, 0)
        .await
        .unwrap();
    assert!(fixture.stored_files() > 0);

    // The user approves the whole base folder, which contains the reserved plugin-data folder.
    let user = fixture.user_approving(&fixture.base());
    let root = fixture.root_handle("id-fixture:1", ALL).await;
    let listing = names(&fixture.ls(&root).await.unwrap()).join(",");
    assert!(listing.contains("games"), "{listing}");
    assert!(!listing.contains("plugin-data"), "{listing}");
    // Nor can a folder inside it, or it itself, be approved.
    for reserved in [
        fixture.plugin_data.clone(),
        fixture.plugin_data.join("anything"),
    ] {
        std::fs::create_dir_all(&reserved).unwrap();
        *user.root.lock().unwrap() = Some(reserved);
        let status = fixture
            .request_access("id-fixture:1", &["read"], &["save"])
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::FailedPrecondition);
    }
    assert!(fixture.stored_files() > 0, "the private store is untouched");
}

#[tokio::test]
async fn a_directory_replaced_by_a_link_after_listing_is_refused() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("saves/slot1.sav", b"mine")]);
    let outside = fixture.base().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("slot1.sav"), b"secret").unwrap();
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", ALL).await;
    let saves = fixture.child(&root, "saves").await;
    let slot = fixture.child(&saves, "slot1.sav").await;
    assert_eq!(fixture.cat(&slot, 0, 0).await.unwrap().contents, b"mine");

    std::fs::remove_dir_all(dir.join("saves")).unwrap();
    symlink(&outside, dir.join("saves")).unwrap();

    for status in [
        fixture.cat(&slot, 0, 0).await.map(|_| ()),
        fixture.ls(&saves).await.map(|_| ()),
        fixture.rm(&slot, "d").await.map(|_| ()),
        fixture
            .write_to(&slot, "", b"pwn", 0, true, "w")
            .await
            .map(|_| ()),
        fixture
            .write_to(&saves, "new.sav", b"pwn", 0, false, "w2")
            .await
            .map(|_| ()),
    ] {
        assert_eq!(status.unwrap_err().code(), Code::PermissionDenied);
    }
    assert_eq!(std::fs::read(outside.join("slot1.sav")).unwrap(), b"secret");
    assert!(!outside.join("new.sav").exists());
}

#[tokio::test]
async fn a_root_that_was_replaced_is_no_longer_the_approved_folder() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", ALL).await;
    assert!(fixture.ls(&root).await.is_ok());

    std::fs::rename(&dir, fixture.base().join("moved")).unwrap();
    assert_eq!(code(fixture.ls(&root).await), Code::FailedPrecondition);
    // Another folder put in its place is not the one the user approved.
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("impostor.txt"), b"x").unwrap();
    assert_eq!(code(fixture.ls(&root).await), Code::FailedPrecondition);
}

#[tokio::test]
async fn a_revoked_approval_or_grant_stops_its_handles_at_once() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", ALL).await;
    let file = fixture.child(&root, "a.txt").await;

    // The user revokes this approval (the settings UI does this through the same store).
    let id = {
        let mut found = None;
        let session = fixture.context().session_id;
        found.replace(fixture.files.open_handle(&session, &root).unwrap().0);
        found.unwrap()
    };
    let caller = fixture.caller();
    let revoked = fixture
        .storage
        .run(
            &caller,
            |_| true,
            move |namespace| namespace.revoke_approval(&id),
        )
        .await
        .unwrap();
    assert!(revoked);
    assert_eq!(code(fixture.ls(&root).await), Code::NotFound);
    assert_eq!(code(fixture.cat(&file, 0, 0).await), Code::NotFound);
    assert_eq!(code(fixture.rm(&file, "d").await), Code::NotFound);
    assert!(dir.join("a.txt").exists());

    // Approving again makes a fresh approval, with its own handles.
    let fresh = fixture.root_handle("id-fixture:1", ALL).await;
    assert_eq!(names(&fixture.ls(&fresh).await.unwrap()), ["a.txt"]);
    // And a revoked grant stops them even though the approval stands.
    fixture
        .api
        .revoke_session_permission("fixture", cap::FILES_READ)
        .await;
    assert_eq!(code(fixture.ls(&fresh).await), Code::PermissionDenied);
}

#[tokio::test]
async fn reads_are_bounded_and_resume_where_they_stopped() {
    let fixture = Fixture::new(FILES).await;
    let big = vec![7u8; 5 * 1024 * 1024];
    let dir = fixture.game_dir("g", &[("big.bin", &big), ("small.txt", b"0123456789")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", &["read"]).await;

    let file = fixture.child(&root, "big.bin").await;
    // A huge budget is capped, and the default is smaller still.
    let capped = fixture.cat(&file, 0, u64::MAX).await.unwrap();
    assert_eq!(
        (capped.contents.len(), capped.eof),
        (4 * 1024 * 1024, false)
    );
    assert_eq!(
        fixture.cat(&file, 0, 0).await.unwrap().contents.len(),
        1024 * 1024
    );
    let rest = fixture
        .cat(&file, capped.next_offset, u64::MAX)
        .await
        .unwrap();
    assert_eq!((rest.contents.len(), rest.eof), (1024 * 1024, true));

    let small = fixture.child(&root, "small.txt").await;
    let mut offset = 0;
    let mut text = Vec::new();
    loop {
        let part = fixture.cat(&small, offset, 4).await.unwrap();
        text.extend_from_slice(&part.contents);
        offset = part.next_offset;
        if part.eof {
            break;
        }
    }
    assert_eq!(text, b"0123456789");
    assert!(fixture.cat(&small, 100, 4).await.unwrap().eof);
}

#[tokio::test]
async fn listings_page_with_cursors_bound_to_the_directory() {
    let fixture = Fixture::new(FILES).await;
    let files: Vec<(String, &[u8])> = (0..5).map(|i| (format!("f{i}"), &b"x"[..])).collect();
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, c)| (n.as_str(), *c)).collect();
    let dir = fixture.game_dir("g", &refs);
    std::fs::create_dir_all(dir.join("other")).unwrap();
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", &["read"]).await;

    let first = fixture.ls_page(&root, 2, "").await.unwrap();
    assert_eq!(names(&first), ["f0", "f1"]);
    let second = fixture.ls_page(&root, 2, &first.next_cursor).await.unwrap();
    assert_eq!(names(&second), ["f2", "f3"]);
    let third = fixture
        .ls_page(&root, 2, &second.next_cursor)
        .await
        .unwrap();
    assert_eq!(names(&third), ["f4", "other"]);
    assert!(third.next_cursor.is_empty());
    // A cursor of one directory means nothing for another, or when edited.
    let other = fixture.child(&root, "other").await;
    assert_eq!(
        code(fixture.ls_page(&other, 2, &first.next_cursor).await),
        Code::InvalidArgument
    );
    assert_eq!(
        code(
            fixture
                .ls_page(&root, 2, &format!("{}00", first.next_cursor))
                .await
        ),
        Code::InvalidArgument
    );
}

#[tokio::test]
async fn writes_are_validated_and_never_leave_the_folder_or_leave_holes() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"hello")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture
        .root_handle("id-fixture:1", &["read", "write"])
        .await;
    let file = fixture.child(&root, "a.txt").await;

    for bad in ["..", "../x", "a/b", "a\\b", "con", "x:y", "tail.", ""] {
        let result = fixture.write_to(&root, bad, b"x", 0, false, "w").await;
        if bad.is_empty() {
            // No name: the directory handle is not a file.
            assert_eq!(code(result), Code::FailedPrecondition);
        } else {
            assert_eq!(code(result), Code::InvalidArgument, "{bad:?}");
        }
    }
    assert_eq!(
        code(fixture.write_to(&file, "", b"x", 6, false, "w1").await),
        Code::OutOfRange
    );
    assert_eq!(
        code(fixture.write_to(&root, "new", b"x", 1, false, "w2").await),
        Code::OutOfRange
    );
    let too_big = vec![0u8; 4 * 1024 * 1024 + 1];
    assert_eq!(
        code(fixture.write_to(&file, "", &too_big, 0, false, "w3").await),
        Code::InvalidArgument
    );
    // A file handle is not a directory to create inside.
    assert_eq!(
        code(fixture.write_to(&file, "child", b"x", 0, false, "w4").await),
        Code::FailedPrecondition
    );
    // Files carry no revision.
    let status = fixture
        .client()
        .files_write(FilesWriteRequest {
            mutation: Some(fixture.mutation("w5", 3)),
            file_handle: file.clone(),
            offset: 0,
            contents: b"x".to_vec(),
            truncate: false,
            name: String::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"hello");
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        1,
        "nothing was created"
    );
}

#[tokio::test]
async fn retried_writes_and_deletes_return_the_original_outcome_and_a_reused_key_is_refused() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    fixture.user_approving(&dir);
    let root = fixture.root_handle("id-fixture:1", ALL).await;
    let file = fixture.child(&root, "a.txt").await;

    // An append retried under its key is applied once.
    let first = fixture
        .write_to(&file, "", b"line;", 0, false, "append")
        .await
        .unwrap();
    let retry = fixture
        .write_to(&file, "", b"line;", 0, false, "append")
        .await
        .unwrap();
    assert_eq!(first, retry);
    assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"line;");
    // The key is for that request only.
    assert_eq!(
        code(
            fixture
                .write_to(&file, "", b"other", 0, false, "append")
                .await
        ),
        Code::FailedPrecondition
    );
    assert_eq!(
        code(fixture.rm(&file, "append").await),
        Code::FailedPrecondition
    );

    let deleted = fixture.rm(&file, "remove").await.unwrap();
    let again = fixture.rm(&file, "remove").await.unwrap();
    assert_eq!(deleted, again, "the retry is answered, not NOT_FOUND");
    // A new delete of the same file is a different request and the file is gone.
    assert_eq!(code(fixture.rm(&file, "remove-2").await), Code::NotFound);
}

#[tokio::test]
async fn requests_are_validated() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    let user = fixture.user_approving(&dir);
    for (operations, kinds) in [
        (vec![], vec![]),
        (vec!["execute"], vec![]),
        (vec!["READ"], vec![]),
        (vec!["read"], vec!["everything"]),
    ] {
        assert_eq!(
            code(
                fixture
                    .request_access("id-fixture:1", &operations, &kinds)
                    .await
            ),
            Code::InvalidArgument,
            "{operations:?} {kinds:?}"
        );
    }
    assert_eq!(
        code(fixture.request_access("", &["read"], &[]).await),
        Code::InvalidArgument
    );
    assert_eq!(
        code(
            fixture
                .request_access(&"i".repeat(200), &["read"], &[])
                .await
        ),
        Code::InvalidArgument
    );
    assert_eq!(user.asked(), 0);
}

#[tokio::test]
async fn each_kind_is_approved_separately_and_only_installations_have_a_suggestion() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"a")]);
    let saves = fixture.base().join("elsewhere");
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(saves.join("save.dat"), b"s").unwrap();
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    let user = fixture.user_approving(&saves);

    let reply = fixture
        .request_access("id-fixture:1", &["read"], &["save", "prefix", "save"])
        .await
        .unwrap();
    assert_eq!(
        reply
            .handles
            .iter()
            .map(|h| h.resource_kind.as_str())
            .collect::<Vec<_>>(),
        ["save", "prefix"]
    );
    assert!(!reply.handles[0].writable);
    assert_eq!(user.asked(), 2);
    assert!(
        user.asked
            .lock()
            .unwrap()
            .iter()
            .all(|ask| ask.suggested.is_none())
    );
    for handle in &reply.handles {
        assert_eq!(
            names(&fixture.ls(&handle.handle).await.unwrap()),
            ["save.dat"]
        );
    }
}

#[tokio::test]
async fn one_approval_waits_for_the_user_at_a_time() {
    let fixture = Fixture::new(FILES_AND_STORAGE).await;
    let dir = fixture.game_dir("g", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    let user = fixture.user_approving(&dir);
    let gate = Arc::new(tokio::sync::Notify::new());
    *user.gate.lock().unwrap() = Some(gate.clone());

    let waiting = {
        let fixture_client = fixture.client();
        let request = FilesRequestAccessRequest {
            mutation: Some(fixture.mutation("first", 0)),
            game_id: String::new(),
            installation_id: "id-fixture:1".into(),
            requested_operations: vec!["read".into()],
            resource_kinds: vec![],
        };
        tokio::spawn(async move {
            let mut client = fixture_client;
            client.files_request_access(request).await
        })
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while user.asked() == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the dialog never opened"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The user is already deciding: a second request does not open a second dialog.
    assert_eq!(
        code(fixture.request_access("id-fixture:1", &["read"], &[]).await),
        Code::Aborted
    );
    assert_eq!(user.asked(), 1);
    // Storage of the same plugin still answers while the user decides.
    assert!(!fixture.load("k").await.unwrap().found);

    gate.notify_one();
    let reply = waiting.await.unwrap().unwrap();
    assert_eq!(reply.into_inner().handles.len(), 1);
}

#[tokio::test]
async fn nothing_in_any_answer_is_a_path() {
    let fixture = Fixture::new(FILES).await;
    let dir = fixture.game_dir("secret-place", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", None, "G", &dir)]);
    let user = fixture.user_approving(&dir);
    let base = fixture.base().to_string_lossy().into_owned();

    let mut shown = Vec::new();
    let reply = fixture
        .request_access("id-fixture:1", ALL, &[])
        .await
        .unwrap();
    shown.push(format!("{reply:?}"));
    let root = reply.handles[0].handle.clone();
    shown.push(format!("{:?}", fixture.ls(&root).await.unwrap()));
    let file = fixture.child(&root, "a.txt").await;
    shown.push(format!("{:?}", fixture.cat(&file, 0, 0).await.unwrap()));
    shown.push(format!(
        "{:?}",
        fixture
            .write_to(&root, "w.txt", b"x", 0, false, "w")
            .await
            .unwrap()
    ));
    for failure in [
        fixture.cat("junk", 0, 0).await.map(|_| ()).unwrap_err(),
        fixture.rm(&root, "d").await.map(|_| ()).unwrap_err(),
        fixture
            .write_to(&root, "..", b"x", 0, false, "w2")
            .await
            .map(|_| ())
            .unwrap_err(),
    ] {
        shown.push(format!("{} {:?}", failure.message(), failure.code()));
    }
    // Moving the root makes an error; it names no path either.
    std::fs::rename(&dir, fixture.base().join("moved-away")).unwrap();
    shown.push(fixture.ls(&root).await.unwrap_err().message().to_owned());
    for text in &shown {
        assert!(
            !text.contains(&base) && !text.contains("secret-place"),
            "{text}"
        );
    }
    assert!(user.asked() > 0);
}
