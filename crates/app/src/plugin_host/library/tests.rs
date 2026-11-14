use super::*;

fn source(key: &str, game_id: Option<u64>, name: &str) -> LibrarySource {
    LibrarySource {
        entry_key: key.to_owned(),
        provider_id: key.split(':').next().unwrap().to_owned(),
        game_id,
        name: name.to_owned(),
        install_state: InstallState::Installed,
        display_name: None,
        metadata_revision: 0,
    }
}

fn own(provider: &str) -> Scope {
    Scope {
        own_provider: provider.to_owned(),
        read_all: false,
        provider_filter: None,
    }
}

fn all() -> Scope {
    Scope {
        own_provider: "steam".to_owned(),
        read_all: true,
        provider_filter: None,
    }
}

/// Syncs `sources` and answers every id request with `id-<key>`.
fn feed_with(sources: Vec<LibrarySource>) -> Arc<LibraryFeed> {
    let feed = LibraryFeed::new();
    resync(&feed, sources);
    feed
}

fn resync(feed: &LibraryFeed, sources: Vec<LibrarySource>) {
    let missing = feed.sync(sources);
    let ids = missing
        .iter()
        .map(|key| (key.clone(), format!("id-{key}")))
        .collect();
    feed.assign_ids(&missing, ids);
}

fn binding(scope: Scope) -> CursorBinding {
    CursorBinding {
        route: "library",
        session_id: "s1".into(),
        profile_epoch: 1,
        grant_revision: 1,
        scope,
        search: None,
    }
}

#[test]
fn entries_appear_only_once_their_installation_id_is_durable() {
    let feed = LibraryFeed::new();
    let missing = feed.sync(vec![source("steam:620", Some(7), "Portal 2")]);
    assert_eq!(missing, ["steam:620"]);
    assert!(feed.page(&all(), None, None, 10).1.is_empty());
    // A second sync while the request is outstanding does not ask again.
    assert!(
        feed.sync(vec![source("steam:620", Some(7), "Portal 2")])
            .is_empty()
    );
    feed.assign_ids(&missing, vec![("steam:620".into(), "abc".into())]);
    let (revision, page, more) = feed.page(&all(), None, None, 10);
    assert_eq!(revision, 1);
    assert!(!more);
    assert_eq!(page[0].installation_id, "abc");
    assert_eq!(page[0].game_id, Some(7));
}

#[test]
fn a_failed_id_request_keeps_the_entry_hidden_and_backs_off() {
    let feed = LibraryFeed::new();
    let missing = feed.sync(vec![source("steam:620", None, "Portal 2")]);
    feed.ids_failed(&missing);
    assert!(feed.page(&all(), None, None, 10).1.is_empty());
    assert!(
        feed.sync(vec![source("steam:620", None, "Portal 2 GOTY")])
            .is_empty(),
        "no retry storm right after a failure"
    );
}

#[test]
fn unmatched_entries_have_no_game_and_multiple_installations_share_one() {
    let feed = feed_with(vec![
        source("steam:570", None, "Dota 2"),
        source("steam:620", Some(7), "Portal 2"),
        source("gog:1", Some(7), "Portal 2"),
    ]);
    let (_, page, _) = feed.page(&all(), None, None, 10);
    let protos: Vec<_> = page.iter().map(Projected::to_proto).collect();
    let dota = protos.iter().find(|e| e.name == "Dota 2").unwrap();
    assert_eq!(dota.game_id, "");
    assert_eq!(dota.installation_id, "id-steam:570");
    let portal: Vec<_> = protos.iter().filter(|e| e.game_id == "7").collect();
    assert_eq!(portal.len(), 2);
    assert_ne!(portal[0].installation_id, portal[1].installation_id);
    assert_eq!(feed.by_game(&all(), 7).1.len(), 2);
    assert_eq!(feed.by_game(&own("gog"), 7).1.len(), 1);
}

#[test]
fn a_rescan_or_match_change_keeps_the_installation_id_and_moves_the_game() {
    let feed = feed_with(vec![source("steam:620", None, "Portal 2")]);
    let before = feed.get(&all(), "id-steam:620").1.unwrap();
    resync(&feed, vec![source("steam:620", Some(7), "Portal 2")]);
    let (revision, after) = feed.get(&all(), "id-steam:620");
    let after = after.unwrap();
    assert_eq!(after.installation_id, before.installation_id);
    assert_eq!(after.game_id, Some(7));
    assert_eq!(after.revision, revision);
    assert!(after.revision > before.revision);
    // An identical rescan changes nothing.
    resync(&feed, vec![source("steam:620", Some(7), "Portal 2")]);
    assert_eq!(feed.revision(), revision);
}

#[test]
fn own_provider_reads_never_disclose_another_providers_entries() {
    let feed = feed_with(vec![
        source("steam:620", Some(7), "Portal 2"),
        source("gog:1", Some(8), "Witcher"),
    ]);
    let (_, page, _) = feed.page(&own("gog"), None, None, 10);
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].provider_id, "gog");
    // Filtering to a foreign provider without the whole-library grant yields nothing.
    let foreign = Scope {
        provider_filter: Some("steam".into()),
        ..own("gog")
    };
    assert!(feed.page(&foreign, None, None, 10).1.is_empty());
    assert!(feed.get(&own("gog"), "id-steam:620").1.is_none());
    assert!(feed.by_game(&own("gog"), 7).1.is_empty());
    // With it, the filter selects that provider.
    let granted = Scope {
        read_all: true,
        ..foreign
    };
    assert_eq!(feed.page(&granted, None, None, 10).1.len(), 1);
}

#[test]
fn pages_are_ordered_bounded_and_searchable() {
    let feed = feed_with(
        (0..5)
            .map(|i| source(&format!("steam:{i}"), None, &format!("Game {i}")))
            .collect(),
    );
    let (_, first, more) = feed.page(&all(), None, None, 2);
    assert!(more);
    let (_, second, _) = feed.page(&all(), None, Some(&first[1].installation_id), 2);
    assert!(first[1].installation_id < second[0].installation_id);
    let (_, found, _) = feed.page(&all(), Some("game 3"), None, 10);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Game 3");
}

#[test]
fn cursors_are_bound_to_their_query_and_expire_with_the_library_revision() {
    let feed = feed_with(vec![source("steam:1", None, "A")]);
    let bound = binding(all());
    let cursor = feed.issue_cursor(&bound, feed.revision(), "id-steam:1");
    assert_eq!(
        feed.open_cursor(&cursor, &bound),
        Ok(("id-steam:1".to_owned(), 1))
    );
    // Another scope, search, session, profile or grant revision rejects it.
    for other in [
        binding(own("steam")),
        CursorBinding {
            search: Some("a".into()),
            ..binding(all())
        },
        CursorBinding {
            session_id: "s2".into(),
            ..binding(all())
        },
        CursorBinding {
            profile_epoch: 2,
            ..binding(all())
        },
        CursorBinding {
            grant_revision: 2,
            ..binding(all())
        },
    ] {
        assert_eq!(feed.open_cursor(&cursor, &other), Err(CursorError::Stale));
    }
    // A tampered cursor or one from another process is not a cursor at all.
    let mut tampered = cursor.clone();
    tampered.replace_range(0..2, "00");
    assert_eq!(
        feed.open_cursor(&tampered, &bound),
        Err(CursorError::Malformed)
    );
    assert_eq!(
        LibraryFeed::new().open_cursor(&cursor, &bound),
        Err(CursorError::Malformed)
    );
    assert_eq!(
        feed.open_cursor("not a cursor", &bound),
        Err(CursorError::Malformed)
    );
    // The cursor reports the revision it was issued for; the service rejects it once the
    // library has moved on.
    resync(&feed, vec![source("steam:1", None, "B")]);
    assert_eq!(feed.open_cursor(&cursor, &bound).unwrap().1, 1);
    assert_eq!(feed.revision(), 2);
}

#[test]
fn watchers_resume_from_retained_history_or_must_resync() {
    let feed = feed_with(vec![source("steam:1", None, "A")]);
    // Zero starts at the current revision with no backlog.
    let (current, backlog, _rx) = feed.subscribe(0).unwrap();
    assert_eq!((current, backlog.len()), (1, 0));
    resync(&feed, vec![source("steam:1", Some(3), "A")]);
    resync(&feed, vec![]);
    let (_, backlog, _rx) = feed.subscribe(1).unwrap();
    assert_eq!(
        backlog
            .iter()
            .map(|c| (c.revision, c.kind))
            .collect::<Vec<_>>(),
        [(2, ChangeKind::Upserted), (3, ChangeKind::Removed)]
    );
    assert_eq!(backlog[1].entry.installation_id, "id-steam:1");
    assert_eq!(feed.subscribe(9).unwrap_err(), ResumeError::Future);
    // Push the first events out of the bounded history.
    for i in 0..HISTORY_EVENTS as u64 + 4 {
        resync(&feed, vec![source("steam:1", Some(i), "A")]);
    }
    assert_eq!(feed.subscribe(1).unwrap_err(), ResumeError::Resync);
    assert!(feed.subscribe(feed.revision() - 1).is_ok());
}

#[tokio::test]
async fn live_events_follow_the_backlog_without_gaps() {
    let feed = feed_with(vec![source("steam:1", None, "A")]);
    let (_, _, mut rx) = feed.subscribe(1).unwrap();
    resync(&feed, vec![source("steam:1", None, "B")]);
    let change = rx.recv().await.unwrap();
    assert_eq!(change.revision, 2);
    assert_eq!(change.entry.name, "B");
}

#[test]
fn hmac_matches_the_rfc_4231_vector_shape() {
    // RFC 4231 test case 2 uses a short key; a 32-byte key padded with zeros is equivalent to
    // the 4-byte key "Jefe" padded to the block, so the published digest applies.
    let mut key = [0u8; 32];
    key[..4].copy_from_slice(b"Jefe");
    assert_eq!(
        super::super::hex(&hmac_sha256(&key, b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}
