//! `FilesRequestAccess.declared_locations` over the real local socket (task 6.7): a plugin may
//! suggest where a game's saves or prefix usually are, and the suggestion is only ever shown to
//! the user, labelled as the plugin's.

use gamenode_plugin_sdk::declared_location as declare;

use super::*;
use crate::plugin_host::files::DeclaredFolder;

const FILES: &[&str] = &[cap::FILES_READ, cap::FILES_WRITE, cap::FILES_DELETE];

fn text(path: &std::path::Path) -> String {
    path.to_str().unwrap().to_owned()
}

impl Fixture {
    async fn ask_with(
        &self,
        installation: &str,
        operations: &[&str],
        kinds: &[&str],
        declared: Vec<DeclaredLocation>,
    ) -> Result<FileAccessReply, Status> {
        self.client()
            .files_request_access(FilesRequestAccessRequest {
                mutation: Some(self.mutation("access", 0)),
                game_id: String::new(),
                installation_id: installation.into(),
                requested_operations: operations.iter().map(|op| (*op).into()).collect(),
                resource_kinds: kinds.iter().map(|kind| (*kind).into()).collect(),
                declared_locations: declared,
            })
            .await
            .map(Response::into_inner)
    }

    /// How the approval behind `handle` was made.
    async fn origin_of(&self, handle: &str) -> String {
        let id = self
            .files
            .open_handle(&self.context().session_id, handle)
            .unwrap()
            .0;
        let caller = self.caller();
        self.storage
            .run(
                &caller,
                |_| true,
                move |namespace| Ok(namespace.approval(&id)?.unwrap().origin),
            )
            .await
            .unwrap()
    }
}

/// A library with one entry in `<base>/games/one`, and a folder `<base>/<name>` per name given.
fn setup(fixture: &Fixture, folders: &[&str]) -> Vec<std::path::PathBuf> {
    let game = fixture.game_dir("one", &[("a.txt", b"a")]);
    fixture.library_in(&[("fixture:1", Some(7), "One", &game)]);
    folders
        .iter()
        .map(|name| {
            let dir = fixture.base().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        })
        .collect()
}

#[tokio::test]
async fn a_declaration_alone_grants_nothing_and_leaves_no_trace() {
    let fixture = Fixture::new(FILES).await;
    let [saves] = setup(&fixture, &["saves"]).try_into().unwrap();
    let declared = || vec![declare("save", &text(&saves), "Saves")];

    // No dialog in this build: nothing is approved.
    assert_eq!(
        fixture
            .ask_with("id-fixture:1", &["read"], &["save"], declared())
            .await
            .unwrap_err()
            .code(),
        Code::FailedPrecondition
    );
    // The user declines: nothing is approved, and the next request asks again.
    let user = fixture.user_approving(&saves);
    user.consent.store(false, Ordering::SeqCst);
    assert_eq!(
        fixture
            .ask_with("id-fixture:1", &["read"], &["save"], declared())
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(user.asked(), 1);
    // The folder was only ever shown to the user as a suggestion; declining left no approval.
    let asked = user.asked.lock().unwrap()[0].clone();
    assert_eq!(asked.declared.len(), 1);
    let [scope] = user.confirmed.lock().unwrap().clone().try_into().unwrap();
    assert!(scope.declared_by_plugin);
    let caller = fixture.caller();
    let stored = fixture
        .storage
        .run(
            &caller,
            |_| true,
            |namespace| namespace.approval_for("id-fixture:1", "save"),
        )
        .await
        .unwrap();
    assert_eq!(stored, None);
    user.consent.store(true, Ordering::SeqCst);
    fixture
        .ask_with("id-fixture:1", &["read"], &["save"], declared())
        .await
        .unwrap();
    assert_eq!(user.asked(), 2);
}

#[tokio::test]
async fn suggestions_reach_the_user_in_the_plugins_order_labelled_with_the_first_existing_chosen() {
    let fixture = Fixture::new(FILES).await;
    let [first, second] = setup(&fixture, &["first", "second"]).try_into().unwrap();
    let missing = fixture.base().join("not-there");
    let user = fixture.user_approving(&second);
    fixture
        .ask_with(
            "id-fixture:1",
            &["read"],
            &["save"],
            vec![
                declare("save", &text(&missing), "  Saves on the old path  "),
                declare("save", &text(&first), "Saves"),
                declare("save", &text(&second), ""),
                // The same folder again, written another way, and one of the other kind.
                declare("save", &format!("{}/./", text(&first)), "again"),
                declare("prefix", &text(&second), "Prefix"),
            ],
        )
        .await
        .unwrap();
    let asked = user.asked.lock().unwrap()[0].clone();
    assert_eq!(
        asked.declared,
        [
            DeclaredFolder {
                path: missing,
                description: "Saves on the old path".into(),
                exists: false
            },
            DeclaredFolder {
                path: first.clone(),
                description: "Saves".into(),
                exists: true
            },
            DeclaredFolder {
                path: second,
                description: String::new(),
                exists: true
            },
        ]
    );
    // Not the first listed, but the first that exists, and the host has no suggestion of its own.
    assert_eq!(asked.preselected, Some(first));
    assert_eq!(asked.suggested, None);
    assert_eq!(asked.kind, "save");
}

#[tokio::test]
async fn the_host_suggestion_for_an_installation_stays_first() {
    let fixture = Fixture::new(FILES).await;
    setup(&fixture, &[]);
    let game = fixture.base().join("games").join("one");
    let user = fixture.user_approving(&game);
    fixture
        .ask_with("id-fixture:1", &["read"], &[], vec![])
        .await
        .unwrap();
    let asked = user.asked.lock().unwrap()[0].clone();
    assert_eq!(asked.suggested.as_deref(), Some(game.as_path()));
    assert_eq!(asked.preselected, Some(game));
    assert!(asked.declared.is_empty());
}

#[tokio::test]
async fn what_cannot_apply_here_or_is_private_to_the_host_is_dropped_without_an_error() {
    let fixture = Fixture::new(FILES).await;
    let [saves] = setup(&fixture, &["saves"]).try_into().unwrap();
    let user = fixture.user_approving(&saves);
    let reply = fixture
        .ask_with(
            "id-fixture:1",
            &["read"],
            &["save"],
            vec![
                declare("save", "{appdata}/Portal", "Windows"),
                declare("save", "{localappdata}/Portal", "Windows"),
                declare(
                    "save",
                    "C:\\Users\\ana\\Saved Games\\Portal",
                    "Windows drive",
                ),
                declare("save", "\\\\nas\\games\\portal", "Windows share"),
                // The host's own data folder: never offered, never an error.
                declare("save", &text(&fixture.plugin_data), "Private"),
                declare(
                    "save",
                    &format!("{}/plugins", text(&fixture.plugin_data)),
                    "Private",
                ),
                declare("save", &text(&saves), "Saves"),
            ],
        )
        .await
        .unwrap();
    assert_eq!(reply.handles.len(), 1);
    let asked = user.asked.lock().unwrap()[0].clone();
    assert_eq!(
        asked
            .declared
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>(),
        [saves]
    );
    // `{xdg_*}`, `{home}` and `{documents}` apply here; whether the folder exists is the user's
    // dialog's concern, and none of them is an error.
    fixture
        .ask_with(
            "id-fixture:1",
            &["read"],
            &["prefix"],
            vec![
                declare("prefix", "{xdg_data}/Portal", "Linux data"),
                declare("prefix", "{xdg_config}/Portal", "Linux config"),
                declare("prefix", "{home}/Portal", "Home"),
                declare("prefix", "{documents}/Portal", "Documents"),
            ],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn a_malformed_declaration_is_refused_before_the_user_is_asked() {
    let fixture = Fixture::new(FILES).await;
    let [saves] = setup(&fixture, &["saves"]).try_into().unwrap();
    let user = fixture.user_approving(&saves);
    let good = declare("save", &text(&saves), "Saves");
    let nine: Vec<_> = (0..9)
        .map(|n| declare("save", &format!("/opt/games/{n}"), ""))
        .collect();
    let long = format!("/{}", "a".repeat(1100));
    let cases: Vec<(&str, Vec<DeclaredLocation>)> = vec![
        ("unknown placeholder", vec![declare("save", "{nope}/x", "")]),
        ("relative path", vec![declare("save", "saves/slot1", "")]),
        ("tilde", vec![declare("save", "~/saves", "")]),
        ("dot dot", vec![declare("save", "{home}/../etc", "")]),
        ("dot dot literal", vec![declare("save", "/opt/../etc", "")]),
        ("empty", vec![declare("save", "", "")]),
        ("too long", vec![declare("save", &long, "")]),
        ("control", vec![declare("save", "/opt/a\u{7}b", "")]),
        (
            "installation kind",
            vec![declare("installation", &text(&saves), "")],
        ),
        ("unknown kind", vec![declare("backup", &text(&saves), "")]),
        ("empty kind", vec![declare("", &text(&saves), "")]),
        ("nine for one kind", nine),
        (
            "long description",
            vec![declare("save", &text(&saves), &"x".repeat(121))],
        ),
        (
            "multi-line description",
            vec![declare("save", &text(&saves), "a\nb")],
        ),
        (
            "control description",
            vec![declare("save", &text(&saves), "a\u{7}")],
        ),
        (
            "bidi description",
            vec![declare("save", &text(&saves), "a\u{202e}b")],
        ),
        (
            "a bad one among good ones",
            vec![good.clone(), declare("save", "x", "")],
        ),
    ];
    for (name, declared) in cases {
        let status = fixture
            .ask_with("id-fixture:1", &["read"], &["save"], declared)
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::InvalidArgument, "{name}");
        // The message never repeats the location the plugin sent back as a path of ours.
        assert!(
            !status.message().contains("/home"),
            "{name}: {}",
            status.message()
        );
    }
    assert_eq!(user.asked(), 0);

    // Eight for each kind is the limit, not nine in total.
    let eight: Vec<_> = (0..8)
        .map(|n| declare("save", &format!("/opt/games/{n}"), ""))
        .chain((0..8).map(|n| declare("prefix", &format!("/opt/prefix/{n}"), "")))
        .collect();
    fixture
        .ask_with("id-fixture:1", &["read"], &["save"], eight)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_approval_records_whether_the_plugin_suggested_the_folder() {
    let fixture = Fixture::new(FILES).await;
    let [suggested, other] = setup(&fixture, &["suggested", "other"]).try_into().unwrap();
    let user = fixture.user_approving(&suggested);

    // The user takes the plugin's suggestion; the confirmation still names the library games.
    let taken = fixture
        .ask_with(
            "id-fixture:1",
            &["read"],
            &["save"],
            vec![declare("save", &text(&suggested), "Saves")],
        )
        .await
        .unwrap();
    assert_eq!(
        fixture.origin_of(&taken.handles[0].handle).await,
        "declared"
    );
    assert!(user.confirmed.lock().unwrap()[0].declared_by_plugin);

    // The user picks another folder for the prefix, though the plugin suggested the first.
    *user.root.lock().unwrap() = Some(other.clone());
    let chosen = fixture
        .ask_with(
            "id-fixture:1",
            &["read"],
            &["prefix"],
            vec![declare("prefix", &text(&suggested), "Prefix")],
        )
        .await
        .unwrap();
    assert_eq!(fixture.origin_of(&chosen.handles[0].handle).await, "user");
    assert!(!user.confirmed.lock().unwrap()[1].declared_by_plugin);

    // A suggestion that is a link to the folder the user chose is the same folder.
    let link = fixture.base().join("link");
    std::os::unix::fs::symlink(&other, &link).unwrap();
    *user.root.lock().unwrap() = Some(link.clone());
    let reply = fixture
        .ask_with(
            "id-fixture:1",
            &["read"],
            &["save", "prefix"],
            vec![declare("save", &text(&other), "Other")],
        )
        .await
        .unwrap();
    // The `save` approval already stands and is reused; `prefix` had no suggestion at all.
    assert_eq!(
        fixture.origin_of(&reply.handles[0].handle).await,
        "declared"
    );
    assert_eq!(fixture.origin_of(&reply.handles[1].handle).await, "user");

    // Nothing but the kind and writability tells the plugin how it went.
    assert_eq!(
        reply
            .handles
            .iter()
            .map(|h| (h.resource_kind.as_str(), h.writable))
            .collect::<Vec<_>>(),
        [("save", false), ("prefix", false)]
    );
}

#[tokio::test]
async fn the_plugin_cannot_tell_which_suggestions_exist_or_were_shown() {
    let describe = |reply: &FileAccessReply| -> Vec<(String, String, bool)> {
        reply
            .handles
            .iter()
            .map(|h| (h.display_name.clone(), h.resource_kind.clone(), h.writable))
            .collect()
    };
    let mut answers = Vec::new();
    for suggestions in [0, 1, 2] {
        let fixture = Fixture::new(FILES).await;
        let [saves] = setup(&fixture, &["saves"]).try_into().unwrap();
        fixture.user_approving(&saves);
        let declared = match suggestions {
            0 => vec![],
            1 => vec![declare("save", &text(&saves), "Saves")],
            _ => vec![
                declare("save", "/definitely/not/there", "Nothing"),
                declare("save", "{appdata}/X", "Windows"),
            ],
        };
        answers.push(describe(
            &fixture
                .ask_with("id-fixture:1", &["read"], &["save"], declared)
                .await
                .unwrap(),
        ));
    }
    assert_eq!(answers[0], answers[1]);
    assert_eq!(answers[1], answers[2]);
}

#[tokio::test]
async fn a_read_only_approval_gives_no_write_and_widening_asks_the_user_again() {
    let fixture = Fixture::new(FILES).await;
    let [saves] = setup(&fixture, &["saves"]).try_into().unwrap();
    std::fs::write(saves.join("slot.sav"), b"s").unwrap();
    let user = fixture.user_approving(&saves);
    let declared = || vec![declare("save", &text(&saves), "Saves")];

    let read_only = fixture
        .ask_with("id-fixture:1", &["read"], &["save"], declared())
        .await
        .unwrap();
    let root = &read_only.handles[0];
    assert!(!root.writable);
    assert_eq!(user.asked(), 1);
    let denied = fixture
        .write_to(&root.handle, "new.sav", b"x", 0, true, "w")
        .await
        .unwrap_err();
    assert_eq!(denied.code(), Code::PermissionDenied);
    assert!(!saves.join("new.sav").exists());

    // The same request again reuses the approval and the declared list is not looked at again.
    fixture
        .ask_with(
            "id-fixture:1",
            &["read"],
            &["save"],
            vec![declare("save", "/somewhere/else", "Other")],
        )
        .await
        .unwrap();
    assert_eq!(user.asked(), 1);
    // ... but a list that is not well formed is refused even then.
    assert_eq!(
        fixture
            .ask_with(
                "id-fixture:1",
                &["read"],
                &["save"],
                vec![declare("save", "relative", "")]
            )
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );

    // Widening to write asks the user again, with the suggestions shown again.
    let widened = fixture
        .ask_with("id-fixture:1", &["read", "write"], &["save"], declared())
        .await
        .unwrap();
    assert_eq!(user.asked(), 2);
    assert_eq!(user.asked.lock().unwrap()[1].declared.len(), 1);
    assert!(widened.handles[0].writable);
}

#[tokio::test]
async fn the_sdk_sends_the_declaration_the_raw_call_sends() {
    let fixture = Fixture::new(FILES).await;
    let [saves] = setup(&fixture, &["saves"]).try_into().unwrap();
    let user = fixture.user_approving(&saves);
    let sdk = fixture.host.resources().unwrap();
    let handles = sdk
        .files_request_access_with(
            "id-fixture:1",
            &["read"],
            &["save"],
            &[declare("save", &text(&saves), "Saves")],
            "k",
        )
        .await
        .unwrap();
    assert_eq!(handles.len(), 1);
    let asked = user.asked.lock().unwrap()[0].clone();
    assert_eq!(asked.declared.len(), 1);
    assert_eq!(asked.declared[0].description, "Saves");
    // Without a declaration the old call still works and sends none.
    sdk.files_request_access("id-fixture:1", &["read"], &["prefix"], "k2")
        .await
        .unwrap();
    assert!(user.asked.lock().unwrap()[1].declared.is_empty());
}
