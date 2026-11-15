use std::collections::BTreeMap;

use super::*;
use crate::game_api::AccountEntry;
use crate::library::layout::{
    GroupBy, LibraryFilters, LibraryScope, PlaytimeThreshold, Sort, SortDirection, SortKey,
    ViewMode,
};
use crate::library::machine::tests::{account, loaded, providers, stored, stored_with};
use crate::library::machine::{Effect, Input, LibraryMachine};
use crate::library::membership::EntryStatus;
use crate::library::query::TitleSourceKind;
use crate::library::store::{AccountLibrary, AutoMatch, GameRecord, LibraryCache};
use crate::playtime::{PlaytimeSource, PlaytimeView};

fn filters(change: impl FnOnce(&mut LibraryFilters)) -> LibraryFilters {
    let mut filters = LibraryFilters::default();
    change(&mut filters);
    filters
}

fn view(id: i64, name: &str, payload: SavedViewPayload) -> SavedView {
    SavedView {
        id,
        name: name.to_owned(),
        payload,
        position: id,
        created_at: 1,
    }
}

fn machine_with_layout(
    layout: LibraryLayout,
    cache: LibraryCache,
) -> (LibraryMachine, Vec<Effect>) {
    let mut machine = LibraryMachine::new(providers(false), layout);
    let effects = machine.handle(Input::AccountChanged(Some(account("a"))));
    let op = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::LoadCache { op, .. } => Some(*op),
            _ => None,
        })
        .expect("a LoadCache effect");
    machine.set_now(1_000);
    let effects = machine.handle(Input::Loaded {
        op,
        result: Ok(cache),
    });
    (machine, effects)
}

fn persist_view(effects: &[Effect]) -> (u64, String, SavedView) {
    for effect in effects {
        if let Effect::PersistView {
            op,
            account_id,
            view,
        } = effect
        {
            return (*op, account_id.clone(), view.clone());
        }
    }
    panic!("a PersistView effect: {effects:?}");
}

fn persisted(account_id: &str, op: u64, id: i64) -> Input {
    Input::ViewPersisted {
        op,
        account_id: account_id.to_owned(),
        result: Ok(id),
    }
}

#[test]
fn saving_a_view_snapshots_the_effective_layout_and_uses_the_returned_id() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Alpha", true),
            stored_with("steam", "2", "Beta", false, true, false),
        ],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    machine.set_now(1_000);
    machine.set_playtime(BTreeMap::from([
        (
            "steam:1".to_owned(),
            PlaytimeView {
                seconds: Some(11 * 3_600),
                source: PlaytimeSource::Store,
                last_played_at: None,
            },
        ),
        (
            "steam:2".to_owned(),
            PlaytimeView {
                seconds: Some(11 * 3_600),
                source: PlaytimeSource::Store,
                last_played_at: None,
            },
        ),
    ]));
    assert!(!machine.can_save_view());

    machine.handle(Input::SetFilters(filters(|filters| {
        filters.stores = vec!["steam".to_owned()];
        filters.playtime = PlaytimeThreshold::AtLeast10h;
    })));
    machine.handle(Input::SetSort(Sort {
        key: SortKey::Playtime,
        direction: SortDirection::Descending,
    }));
    machine.handle(Input::SetGroup(GroupBy::Store));
    machine.handle(Input::SetView(ViewMode::List));
    assert!(machine.can_save_view());

    let effects = machine.handle(Input::SaveView {
        name: "  Steam 10 h+  ".to_owned(),
    });
    let (op, account_id, view) = persist_view(&effects);
    assert_eq!(account_id, "a");
    assert_eq!(
        view,
        SavedView {
            id: 0,
            name: "Steam 10 h+".to_owned(),
            payload: SavedViewPayload {
                scope: LibraryScope::All,
                filters: filters(|filters| {
                    filters.stores = vec!["steam".to_owned()];
                    filters.playtime = PlaytimeThreshold::AtLeast10h;
                }),
                sort: Sort {
                    key: SortKey::Playtime,
                    direction: SortDirection::Descending,
                },
                group: GroupBy::Store,
                view: ViewMode::List,
            },
            position: 0,
            created_at: 1_000,
        }
    );
    // Nothing is shown before the database answers.
    assert!(machine.saved_views.is_empty());
    assert!(machine.view().unwrap().saved_views.is_empty());

    assert!(machine.handle(persisted(&account_id, op, 41)).is_empty());
    let saved = &machine.saved_views[0];
    assert_eq!(saved.id, 41);
    assert_eq!(saved.name, "Steam 10 h+");
    assert_eq!(saved.position, 0);
    assert_eq!(
        machine.last_saved_view,
        Some(SavedViewNotice {
            id: 41,
            name: "Steam 10 h+".to_owned(),
            run: op,
        })
    );
    let compact = machine.view().unwrap().saved_views;
    assert_eq!(
        compact,
        [crate::library::SavedViewCount {
            id: 41,
            name: "Steam 10 h+".to_owned(),
            count: 2,
            active: false,
        }]
    );
    assert!(machine.view().unwrap().can_save_view);
}

#[test]
fn a_second_save_keeps_the_id_the_database_returned_not_a_local_maximum() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    machine.handle(Input::SetSort(Sort {
        key: SortKey::Title,
        direction: SortDirection::Descending,
    }));

    let effects = machine.handle(Input::SaveView {
        name: "First".to_owned(),
    });
    let (op, account_id, _) = persist_view(&effects);
    machine.handle(persisted(&account_id, op, 77));

    let effects = machine.handle(Input::SaveView {
        name: "Second".to_owned(),
    });
    let (op, account_id, _) = persist_view(&effects);
    machine.handle(persisted(&account_id, op, 12));

    let ids: Vec<i64> = machine.saved_views.iter().map(|view| view.id).collect();
    assert_eq!(ids, [77, 12], "the returned ids are kept verbatim");
    let positions: Vec<i64> = machine
        .saved_views
        .iter()
        .map(|view| view.position)
        .collect();
    assert_eq!(positions, [0, 1]);
}

#[test]
fn a_default_payload_cannot_be_saved() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    assert!(!machine.can_save_view());
    assert!(!machine.view().unwrap().can_save_view);
    assert!(
        machine
            .handle(Input::SaveView {
                name: "Everything".to_owned(),
            })
            .is_empty()
    );
    assert!(machine.saved_views.is_empty());
    assert_eq!(machine.view_error, None);

    // Search text is not part of a saved payload.
    machine.handle(Input::SetSearch("portal".to_owned()));
    assert!(!machine.can_save_view());

    machine.handle(Input::SetView(ViewMode::List));
    assert!(machine.can_save_view());
}

#[test]
fn names_are_trimmed_and_must_be_non_empty_and_unique_ignoring_case() {
    let cache = LibraryCache {
        saved_views: vec![view(5, "Steam 10 h+", SavedViewPayload::default())],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::SetSort(Sort {
        key: SortKey::Title,
        direction: SortDirection::Descending,
    }));

    assert!(
        machine
            .handle(Input::SaveView { name: "   ".into() })
            .is_empty()
    );
    assert_eq!(machine.view_error, Some(ViewError::Empty));
    assert_eq!(
        machine.view().unwrap().view_error,
        Some(ViewError::Empty),
        "the dialog reads the error from the view"
    );

    assert!(
        machine
            .handle(Input::SaveView {
                name: " steam 10 H+ ".into()
            })
            .is_empty()
    );
    assert_eq!(machine.view_error, Some(ViewError::Duplicate));

    let effects = machine.handle(Input::SaveView {
        name: "New view".into(),
    });
    assert!(matches!(effects.as_slice(), [Effect::PersistView { .. }]));
    assert_eq!(machine.view_error, None);
    let (op, account_id, _) = persist_view(&effects);
    machine.handle(persisted(&account_id, op, 6));

    // Renaming a view to a different capitalization of its own name is allowed.
    let effects = machine.handle(Input::RenameView {
        id: 5,
        name: "STEAM 10 H+".into(),
    });
    let (op, account_id, view) = persist_view(&effects);
    assert_eq!(view.name, "STEAM 10 H+");
    machine.handle(persisted(&account_id, op, 5));
    assert_eq!(machine.saved_views[0].name, "STEAM 10 H+");

    // Another view's name, ignoring case, is refused and leaves the name as it was.
    assert!(
        machine
            .handle(Input::RenameView {
                id: 5,
                name: "new VIEW".into(),
            })
            .is_empty()
    );
    assert_eq!(machine.view_error, Some(ViewError::Duplicate));
    assert_eq!(machine.saved_views[0].name, "STEAM 10 H+");
}

#[test]
fn applying_a_view_keeps_its_effective_scope_counts_and_title_while_highlighted() {
    let mut cache = LibraryCache {
        entries: vec![
            stored_with("steam", "1", "Installed Steam", true, true, false),
            stored_with("steam", "2", "Owned Steam", false, true, false),
            stored_with("epic", "3", "Installed Epic", true, true, false),
        ],
        ..LibraryCache::default()
    };
    cache.saved_views = vec![view(
        9,
        "Installed Steam",
        SavedViewPayload {
            scope: LibraryScope::Installed,
            filters: filters(|filters| filters.stores = vec!["steam".to_owned()]),
            sort: Sort {
                key: SortKey::Playtime,
                direction: SortDirection::Descending,
            },
            group: GroupBy::Store,
            view: ViewMode::List,
        },
    )];
    let (mut machine, _) = loaded(false, cache);

    let effects = machine.handle(Input::ApplyView { id: 9 });
    assert!(matches!(effects.as_slice(), [Effect::PersistLayout(_)]));
    assert_eq!(machine.layout.scope, LibraryScope::SavedView(9));
    assert_eq!(machine.effective_scope(), &LibraryScope::Installed);
    assert_eq!(machine.layout.filters.stores, ["steam"]);
    assert_eq!(
        machine.layout.sort,
        Sort {
            key: SortKey::Playtime,
            direction: SortDirection::Descending,
        }
    );
    assert_eq!(machine.layout.group, GroupBy::Store);
    assert_eq!(machine.layout.view, ViewMode::List);

    let rows = machine.visible_rows("");
    let keys: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
    assert_eq!(
        keys,
        ["steam:1"],
        "the payload scope joins the store facet while the view is highlighted"
    );
    let query = machine.query("");
    assert_eq!(
        query.title_source.unwrap(),
        crate::library::TitleSource {
            kind: TitleSourceKind::SavedView,
            id: "9".to_owned(),
        }
    );
    assert_eq!(query.filter_counts.match_states.unmatched, 1);
    assert!(machine.saved_view_counts()[0].active);

    // Pane sizes are not part of the payload and must not leave the view.
    machine.handle(Input::SetDetailWidth(420));
    assert_eq!(machine.layout.scope, LibraryScope::SavedView(9));
    assert!(machine.saved_view_counts()[0].active);

    // Search is not stored either.
    machine.handle(Input::SetSearch("installed".to_owned()));
    assert_eq!(machine.layout.scope, LibraryScope::SavedView(9));
    assert!(machine.saved_view_counts()[0].active);
    machine.handle(Input::SetSearch(String::new()));

    // Changing the sort leaves the view and keeps its payload scope.
    let effects = machine.handle(Input::SetSort(Sort {
        key: SortKey::Title,
        direction: SortDirection::Ascending,
    }));
    assert!(matches!(effects.as_slice(), [Effect::PersistLayout(_)]));
    assert_eq!(machine.layout.scope, LibraryScope::Installed);
    assert_eq!(
        machine.query("").title_source.unwrap(),
        crate::library::TitleSource {
            kind: TitleSourceKind::Scope,
            id: "installed".to_owned(),
        }
    );
    assert!(!machine.saved_view_counts()[0].active);
    assert_eq!(
        machine.saved_views[0].payload.scope,
        LibraryScope::Installed,
        "the saved view itself is unchanged"
    );
    assert_eq!(machine.layout.filters.stores, ["steam"]);

    // Filters, grouping and the view mode each end the highlight the same way.
    for change in [
        Input::SetFilters(filters(|filters| filters.stores = vec!["epic".to_owned()])),
        Input::SetGroup(GroupBy::None),
        Input::SetView(ViewMode::Grid),
    ] {
        machine.handle(Input::ApplyView { id: 9 });
        assert!(machine.saved_view_counts()[0].active);
        machine.handle(change);
        assert_eq!(
            machine.layout.scope,
            LibraryScope::Installed,
            "the payload's own scope stays"
        );
        assert!(!machine.saved_view_counts()[0].active);
    }
}

#[test]
fn saving_while_a_view_is_highlighted_snapshots_its_own_scope() {
    let mut cache = LibraryCache {
        entries: vec![stored("steam", "1", "Alpha", true)],
        ..LibraryCache::default()
    };
    cache.saved_views = vec![view(
        4,
        "Installed",
        SavedViewPayload {
            scope: LibraryScope::Installed,
            ..SavedViewPayload::default()
        },
    )];
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::ApplyView { id: 4 });
    assert_eq!(machine.effective_scope(), &LibraryScope::Installed);

    let effects = machine.handle(Input::SaveView {
        name: "Copy".to_owned(),
    });
    let (_, _, submitted) = persist_view(&effects);
    assert_eq!(
        submitted.payload.scope,
        LibraryScope::Installed,
        "a copy must never point at the saved-view marker"
    );
}

#[test]
fn selecting_a_known_saved_scope_through_set_scope_applies_its_payload() {
    let mut cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Alpha", true),
            stored_with("steam", "2", "Beta", false, true, false),
        ],
        ..LibraryCache::default()
    };
    cache.saved_views = vec![view(
        5,
        "Installed",
        SavedViewPayload {
            scope: LibraryScope::Installed,
            ..SavedViewPayload::default()
        },
    )];
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::SetScope(LibraryScope::SavedView(5)));
    assert_eq!(machine.layout.scope, LibraryScope::SavedView(5));
    assert_eq!(machine.effective_scope(), &LibraryScope::Installed);
    let rows = machine.visible_rows("");
    assert_eq!(rows.len(), 1);
}

#[test]
fn an_unknown_view_changes_nothing() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    let before = machine.layout.clone();
    assert!(machine.handle(Input::ApplyView { id: 404 }).is_empty());
    assert_eq!(machine.layout, before);
    assert_eq!(machine.saved_view_counts(), Vec::new());
}

#[test]
fn deleting_the_active_view_keeps_its_scope_filters_and_sort() {
    let mut cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Alpha", true),
            stored("steam", "2", "Beta", true),
        ],
        ..LibraryCache::default()
    };
    cache.saved_views = vec![
        view(
            1,
            "Steam by playtime",
            SavedViewPayload {
                scope: LibraryScope::Favorites,
                filters: filters(|filters| filters.stores = vec!["steam".to_owned()]),
                sort: Sort {
                    key: SortKey::Playtime,
                    direction: SortDirection::Descending,
                },
                group: GroupBy::Store,
                view: ViewMode::List,
            },
        ),
        view(2, "Other", SavedViewPayload::default()),
    ];
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::ApplyView { id: 1 });

    // Deleting a view that is not highlighted leaves the active one alone.
    let effects = machine.handle(Input::DeleteView { id: 2 });
    assert_eq!(effects.len(), 1);
    assert!(matches!(effects[0], Effect::DeleteViewEffect { id: 2, .. }));
    assert_eq!(machine.layout.scope, LibraryScope::SavedView(1));
    assert!(machine.saved_view_counts()[0].active);

    let effects = machine.handle(Input::DeleteView { id: 1 });
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::DeleteViewEffect {
            account_id,
            id: 1
        } if account_id == "a"
    )));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::PersistLayout(_)))
    );
    assert!(machine.saved_views.is_empty());
    assert_eq!(machine.layout.scope, LibraryScope::Favorites);
    assert_eq!(machine.layout.filters.stores, ["steam"]);
    assert_eq!(
        machine.layout.sort,
        Sort {
            key: SortKey::Playtime,
            direction: SortDirection::Descending,
        }
    );
    assert_eq!(machine.layout.group, GroupBy::Store);
    assert_eq!(machine.layout.view, ViewMode::List);
    assert_eq!(
        machine.query("").title_source.unwrap().kind,
        TitleSourceKind::Scope
    );
}

#[test]
fn hiding_a_row_takes_it_out_of_rows_counts_and_selection_without_touching_facts() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Alpha", true),
            stored_with("steam", "2", "Beta", false, true, false),
        ],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    machine.set_now(1_000);
    machine.favorites.insert(
        1,
        crate::library::favorites::FavoriteState {
            is_favorite: true,
            pending: false,
        },
    );
    machine.set_playtime(BTreeMap::from([(
        "steam:1".to_owned(),
        PlaytimeView {
            seconds: Some(60),
            source: PlaytimeSource::Store,
            last_played_at: Some(900),
        },
    )]));
    machine.set_running(BTreeMap::from([("steam:1".to_owned(), 1_000)]));
    machine.handle(Input::Select {
        key: "steam:1".to_owned(),
        mode: crate::library::SelectMode::Replace,
    });
    assert_eq!(machine.view().unwrap().selected, ["steam:1"]);

    let effects = machine.handle(Input::SetHidden {
        keys: vec!["steam:1".to_owned()],
        hidden: true,
    });
    assert_eq!(
        effects,
        [Effect::PersistHidden {
            account_id: "a".to_owned(),
            keys: vec!["steam:1".to_owned()],
            hidden: true,
            now: 1_000,
        }]
    );
    let rows = machine.visible_rows("");
    let keys: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
    assert_eq!(keys, ["steam:2"]);
    let query = machine.query("");
    assert_eq!(query.total, 1);
    assert_eq!(query.scope_counts.all, 1);
    assert_eq!(query.filter_counts.show_hidden, 1);
    assert!(machine.view().unwrap().selected.is_empty());
    assert_eq!(
        machine.last_hidden.as_ref().unwrap().keys,
        ["steam:1".to_owned()]
    );
    assert!(
        machine
            .favorite_state(1)
            .is_some_and(|favorite| favorite.is_favorite),
        "hiding never clears a favorite"
    );
    assert!(
        machine.entry("steam:1").unwrap().installed,
        "hiding never deletes or uninstalls the entry"
    );
    assert_eq!(
        machine.running_title().as_deref(),
        Some("Alpha"),
        "a hidden game is still tracked and reported as running"
    );
    // Groups and Select all see the same filtered rows as the plain view.
    machine.handle(Input::SetGroup(GroupBy::Store));
    let groups = machine.query("").groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].count, 1);
    machine.handle(Input::SelectAll);
    assert_eq!(machine.view().unwrap().selected, ["steam:2"]);
    machine.handle(Input::SetGroup(GroupBy::None));

    // Unhiding brings the row and its counts back.
    let effects = machine.handle(Input::SetHidden {
        keys: vec!["steam:1".to_owned()],
        hidden: false,
    });
    assert!(matches!(
        effects.as_slice(),
        [Effect::PersistHidden { hidden: false, .. }]
    ));
    assert_eq!(machine.visible_rows("").len(), 2);
    assert_eq!(machine.query("").total, 2);
    assert_eq!(machine.query("").filter_counts.show_hidden, 0);
}

#[test]
fn show_hidden_reveals_dimmed_rows_and_select_all_includes_them() {
    let cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Alpha", true),
            stored("steam", "2", "Beta", true),
        ],
        hidden: vec!["steam:2".to_owned()],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    assert_eq!(machine.visible_rows("").len(), 1);

    machine.handle(Input::SetFilters(filters(|filters| {
        filters.show_hidden = true
    })));
    let rows = machine.visible_rows("");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().find(|row| row.key == "steam:2").unwrap().hidden);
    let query = machine.query("");
    assert_eq!(query.total, 2);
    assert_eq!(query.scope_counts.all, 2);

    machine.handle(Input::SelectAll);
    let selected = machine.view().unwrap().selected;
    assert_eq!(selected.len(), 2);
    assert!(selected.contains(&"steam:2".to_owned()));
}

#[test]
fn saved_views_and_hidden_entries_belong_to_the_signed_in_account() {
    let mut cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Alpha", true),
            stored("steam", "2", "Beta", true),
        ],
        hidden: vec!["steam:2".to_owned()],
        ..LibraryCache::default()
    };
    cache.saved_views = vec![view(3, "Mine", SavedViewPayload::default())];
    let (mut machine, _) = loaded(false, cache);
    assert_eq!(machine.saved_views.len(), 1);
    assert_eq!(machine.visible_rows("").len(), 1);

    // Account b has no views and hid nothing.
    let effects = machine.handle(Input::AccountChanged(Some(account("b"))));
    let op = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::LoadCache { op, .. } => Some(*op),
            _ => None,
        })
        .expect("a LoadCache effect");
    assert!(machine.saved_views.is_empty());
    assert!(machine.hidden.is_empty());
    assert!(machine.last_saved_view.is_none());
    assert!(machine.last_hidden.is_none());

    machine.handle(Input::Loaded {
        op,
        result: Ok(LibraryCache {
            entries: vec![stored("steam", "1", "Alpha", true)],
            ..LibraryCache::default()
        }),
    });
    assert!(machine.saved_views.is_empty());
    assert!(machine.hidden.is_empty());
    assert_eq!(machine.visible_rows("").len(), 1);
}

#[test]
fn a_restart_restores_the_highlighted_views_scope_and_drops_an_unknown_marker() {
    let mut cache = LibraryCache {
        entries: vec![
            stored("steam", "1", "Alpha", true),
            stored("steam", "2", "Beta", true),
        ],
        auto_matches: vec![AutoMatch {
            category: 1,
            uid: "2".to_owned(),
            game_ids: vec![2],
            checked_at: 1,
        }],
        games: vec![GameRecord {
            game_id: 2,
            name: "Beta".to_owned(),
            release_year: Some(2020),
            cover_url: None,
            cover_file: None,
            cover_failed_at: None,
        }],
        ..LibraryCache::default()
    };
    cache.account_library = Some(AccountLibrary {
        entries: BTreeMap::from([(
            2,
            AccountEntry {
                id: "entry-2".to_owned(),
                game_id: 2,
                status: Some(EntryStatus::Playing),
                collection_ids: vec!["backlog".to_owned()],
            },
        )]),
        collections: Vec::new(),
        read_at: 5,
    });
    cache.saved_views = vec![view(
        7,
        "Backlog",
        SavedViewPayload {
            scope: LibraryScope::Collection("backlog".to_owned()),
            ..SavedViewPayload::default()
        },
    )];
    let layout = LibraryLayout {
        scope: LibraryScope::SavedView(7),
        ..LibraryLayout::default()
    };
    let (machine, effects) = machine_with_layout(layout, cache);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::PersistLayout(_))),
        "a matching payload needs no layout rewrite"
    );
    assert_eq!(machine.layout.scope, LibraryScope::SavedView(7));
    assert_eq!(
        machine.effective_scope(),
        &LibraryScope::Collection("backlog".to_owned())
    );
    let rows = machine.visible_rows("");
    let keys: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
    assert_eq!(keys, ["steam:2"]);
    assert!(machine.saved_view_counts()[0].active);

    // The same layout for an account without that view falls back to All games and persists it.
    let layout = LibraryLayout {
        scope: LibraryScope::SavedView(99),
        ..LibraryLayout::default()
    };
    let (machine, effects) = machine_with_layout(
        layout,
        LibraryCache {
            entries: vec![stored("steam", "1", "Alpha", true)],
            ..LibraryCache::default()
        },
    );
    assert_eq!(machine.layout.scope, LibraryScope::All);
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::PersistLayout(saved) if saved.scope == LibraryScope::All
        )),
        "the broken marker is replaced on disk: {effects:?}"
    );
    assert_eq!(
        machine.query("").title_source.unwrap().kind,
        TitleSourceKind::Scope
    );
    assert_eq!(machine.visible_rows("").len(), 1);
}

#[test]
fn a_failed_view_write_leaves_no_phantom_view_or_notice() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    machine.handle(Input::SetSort(Sort {
        key: SortKey::Title,
        direction: SortDirection::Descending,
    }));
    let effects = machine.handle(Input::SaveView {
        name: "Lost".to_owned(),
    });
    let (op, account_id, _) = persist_view(&effects);

    assert!(
        machine
            .handle(Input::ViewPersisted {
                op,
                account_id,
                result: Err(()),
            })
            .is_empty()
    );
    assert!(machine.saved_views.is_empty());
    assert!(machine.last_saved_view.is_none());
    assert!(machine.view().unwrap().saved_views.is_empty());
}

#[test]
fn a_failed_rename_keeps_the_previous_name() {
    let cache = LibraryCache {
        saved_views: vec![view(5, "Old name", SavedViewPayload::default())],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    let effects = machine.handle(Input::RenameView {
        id: 5,
        name: "New name".to_owned(),
    });
    let (op, account_id, _) = persist_view(&effects);
    // Nothing changes until the store confirms; the failure leaves the old name.
    assert_eq!(machine.saved_views[0].name, "Old name");
    machine.handle(Input::ViewPersisted {
        op,
        account_id,
        result: Err(()),
    });
    assert_eq!(machine.saved_views[0].name, "Old name");
    assert_eq!(machine.view().unwrap().saved_views[0].name, "Old name");
}

#[test]
fn a_stale_view_answer_is_ignored_after_the_account_changed() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    machine.handle(Input::SetSort(Sort {
        key: SortKey::Title,
        direction: SortDirection::Descending,
    }));
    let effects = machine.handle(Input::SaveView {
        name: "Mine".to_owned(),
    });
    let (op, account_id, _) = persist_view(&effects);

    let effects = machine.handle(Input::AccountChanged(Some(account("b"))));
    assert!(
        machine.view_write.is_none(),
        "another account starts with no attempt of the previous one"
    );
    let load_op = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::LoadCache { op, .. } => Some(*op),
            _ => None,
        })
        .expect("a LoadCache effect");
    machine.handle(Input::Loaded {
        op: load_op,
        result: Ok(LibraryCache::default()),
    });

    machine.handle(persisted(&account_id, op, 13));
    assert!(
        machine.saved_views.is_empty(),
        "account a's answer cannot appear under account b"
    );
    assert!(
        machine.view_write.is_none(),
        "a stale answer cannot resurrect the previous account's attempt"
    );
}

#[test]
fn a_view_payload_never_keeps_or_saves_a_saved_view_marker() {
    let recursive = SavedViewPayload {
        scope: LibraryScope::SavedView(3),
        ..SavedViewPayload::default()
    };
    assert_eq!(recursive.normalized().scope, LibraryScope::All);
    assert!(SavedViewPayload::default().is_default());
    assert!(
        !SavedViewPayload {
            scope: LibraryScope::Installed,
            ..SavedViewPayload::default()
        }
        .is_default()
    );
    assert!(
        !SavedViewPayload {
            view: ViewMode::List,
            ..SavedViewPayload::default()
        }
        .is_default()
    );
    assert!(
        !SavedViewPayload {
            filters: filters(|filters| filters.favorites_only = true),
            ..SavedViewPayload::default()
        }
        .is_default()
    );
    assert_eq!(
        serde_json::to_value(ViewError::Empty).unwrap(),
        serde_json::json!("empty")
    );
    assert_eq!(
        serde_json::to_value(ViewError::Duplicate).unwrap(),
        serde_json::json!("duplicate")
    );
}

#[test]
fn playlist_like_views_do_not_lose_unknown_collection_ids() {
    // An unknown collection id still matches nothing and stays removable as a facet chip.
    let mut cache = LibraryCache {
        entries: vec![stored("steam", "1", "Alpha", true)],
        ..LibraryCache::default()
    };
    cache.saved_views = vec![view(
        2,
        "Gone",
        SavedViewPayload {
            scope: LibraryScope::Collection("gone".to_owned()),
            filters: filters(|filters| filters.stores = vec!["missing".to_owned()]),
            ..SavedViewPayload::default()
        },
    )];
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::ApplyView { id: 2 });
    assert_eq!(
        machine.effective_scope(),
        &LibraryScope::Collection("gone".to_owned())
    );
    assert_eq!(machine.layout.filters.stores, ["missing"]);
    assert!(machine.visible_rows("").is_empty());
    assert_eq!(machine.query("").filter_counts.show_hidden, 0);
}

#[test]
fn prune_hidden_selection_drops_a_row_the_user_just_hid() {
    let cache = LibraryCache {
        entries: vec![stored("steam", "1", "Alpha", true)],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::Select {
        key: "steam:1".to_owned(),
        mode: crate::library::SelectMode::Replace,
    });
    assert_eq!(machine.view().unwrap().selected, ["steam:1"]);

    machine.handle(Input::SetHidden {
        keys: vec!["steam:1".to_owned()],
        hidden: true,
    });
    // Show hidden is off, so the row is no longer part of the visible order at all.
    assert!(machine.view().unwrap().selected.is_empty());

    // Turning Show hidden on does not bring the hidden row back into the selection.
    machine.handle(Input::SetFilters(filters(|filters| {
        filters.show_hidden = true
    })));
    assert_eq!(machine.visible_rows("").len(), 1);
    assert!(machine.view().unwrap().selected.is_empty());
}

#[test]
fn a_failed_write_ends_the_attempt_without_a_phantom_view_and_a_retry_succeeds() {
    let cache = LibraryCache {
        saved_views: vec![view(5, "Old name", SavedViewPayload::default())],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::SetSort(Sort {
        key: SortKey::Title,
        direction: SortDirection::Descending,
    }));

    let effects = machine.handle(Input::SaveView {
        name: "Mine".to_owned(),
    });
    let (op, account_id, _) = persist_view(&effects);
    assert_eq!(
        machine.view_write,
        Some(ViewWriteView {
            run: op,
            operation: ViewWriteOperation::Save,
            view_id: 0,
            name: "Mine".to_owned(),
            state: ViewWriteState::Pending,
            error: None,
            saved_view_id: None,
        }),
        "the dialog sees that its write is in flight"
    );

    // A storage failure ends the attempt with a retryable error and changes nothing else.
    machine.handle(Input::ViewPersisted {
        op,
        account_id: account_id.clone(),
        result: Err(()),
    });
    assert_eq!(
        machine.view_write,
        Some(ViewWriteView {
            run: op,
            operation: ViewWriteOperation::Save,
            view_id: 0,
            name: "Mine".to_owned(),
            state: ViewWriteState::Failed,
            error: Some(ViewWriteError::Failed),
            saved_view_id: None,
        })
    );
    assert_eq!(machine.saved_views.len(), 1, "no phantom view appears");
    assert_eq!(machine.saved_views[0].id, 5);
    assert!(machine.last_saved_view.is_none(), "no false success notice");
    assert_eq!(machine.view_error, None);

    // The retry is a new attempt with its own run and the real AUTOINCREMENT id.
    let effects = machine.handle(Input::SaveView {
        name: "Mine".to_owned(),
    });
    let (retry, account_id, _) = persist_view(&effects);
    assert!(retry > op, "each attempt advances the run");
    assert_eq!(
        machine.view_write.as_ref().unwrap().state,
        ViewWriteState::Pending
    );
    machine.handle(persisted(&account_id, retry, 21));
    let success = machine.view_write.clone().unwrap();
    assert_eq!(
        (success.run, success.state, success.saved_view_id),
        (retry, ViewWriteState::Succeeded, Some(21))
    );
    assert_eq!(machine.saved_views.len(), 2);
    assert_eq!(machine.saved_views[1].id, 21);
    assert_eq!(
        machine.last_saved_view,
        Some(SavedViewNotice {
            id: 21,
            name: "Mine".to_owned(),
            run: retry,
        })
    );

    // A failed rename keeps the previous name and reports its own operation and target.
    let effects = machine.handle(Input::RenameView {
        id: 5,
        name: "New name".to_owned(),
    });
    let (rename_op, account_id, _) = persist_view(&effects);
    machine.handle(Input::ViewPersisted {
        op: rename_op,
        account_id: account_id.clone(),
        result: Err(()),
    });
    let failed = machine.view_write.clone().unwrap();
    assert_eq!(
        (failed.run, failed.operation, failed.view_id),
        (rename_op, ViewWriteOperation::Rename, 5)
    );
    assert_eq!(failed.error, Some(ViewWriteError::Failed));
    assert_eq!(machine.saved_views[0].name, "Old name");

    let effects = machine.handle(Input::RenameView {
        id: 5,
        name: "New name".to_owned(),
    });
    let (retry, account_id, _) = persist_view(&effects);
    assert!(retry > rename_op);
    machine.handle(persisted(&account_id, retry, 5));
    assert_eq!(machine.saved_views[0].name, "New name");
    let success = machine.view_write.unwrap();
    assert_eq!(
        (success.run, success.operation, success.state),
        (retry, ViewWriteOperation::Rename, ViewWriteState::Succeeded)
    );
}

#[test]
fn a_repeated_validation_refusal_still_starts_a_new_attempt() {
    let cache = LibraryCache {
        saved_views: vec![view(5, "Steam 10 h+", SavedViewPayload::default())],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    machine.handle(Input::SetSort(Sort {
        key: SortKey::Title,
        direction: SortDirection::Descending,
    }));

    assert!(
        machine
            .handle(Input::SaveView {
                name: "  steam 10 H+ ".to_owned()
            })
            .is_empty()
    );
    let first = machine.view_write.clone().unwrap();
    assert_eq!(
        (first.operation, first.state, first.error.as_ref()),
        (
            ViewWriteOperation::Save,
            ViewWriteState::Failed,
            Some(&ViewWriteError::Duplicate)
        )
    );
    assert_eq!(first.name, "steam 10 H+");
    assert_eq!(machine.view_error, Some(ViewError::Duplicate));

    // Repeating the same refused name is a new run, so an attempt-scoped UI still ends.
    assert!(
        machine
            .handle(Input::SaveView {
                name: "STEAM 10 H+".to_owned()
            })
            .is_empty()
    );
    let second = machine.view_write.clone().unwrap();
    assert!(second.run > first.run);
    assert_eq!(second.error, Some(ViewWriteError::Duplicate));

    assert!(
        machine
            .handle(Input::SaveView {
                name: "   ".to_owned()
            })
            .is_empty()
    );
    assert_eq!(
        machine.view_write.as_ref().unwrap().error,
        Some(ViewWriteError::Empty)
    );

    // A valid attempt clears the validation error and starts pending under its own run.
    let effects = machine.handle(Input::SaveView {
        name: "Fresh".to_owned(),
    });
    let (op, _, _) = persist_view(&effects);
    assert_eq!(machine.view_error, None);
    let pending = machine.view_write.clone().unwrap();
    assert_eq!(
        (pending.run, pending.state, pending.error),
        (op, ViewWriteState::Pending, None)
    );
    assert_eq!(
        serde_json::to_value(&machine.view().unwrap().view_write).unwrap(),
        serde_json::json!({
            "run": op,
            "operation": "save",
            "viewId": 0,
            "name": "Fresh",
            "state": "pending",
            "error": null,
            "savedViewId": null,
        }),
        "the exact contract the dialog reads"
    );
}

#[test]
fn deleting_a_view_while_its_rename_is_pending_ends_the_attempt() {
    let cache = LibraryCache {
        saved_views: vec![view(5, "Old name", SavedViewPayload::default())],
        ..LibraryCache::default()
    };
    let (mut machine, _) = loaded(false, cache);
    let effects = machine.handle(Input::RenameView {
        id: 5,
        name: "New name".to_owned(),
    });
    let (op, account_id, _) = persist_view(&effects);

    machine.handle(Input::DeleteView { id: 5 });
    assert!(machine.saved_views.is_empty());
    let write = machine.view_write.clone().unwrap();
    assert_eq!(write.run, op);
    assert_eq!(write.state, ViewWriteState::Failed);
    assert_eq!(write.error, Some(ViewWriteError::Failed));

    // The late answer is stale: the view was deleted and the attempt already ended.
    machine.handle(Input::ViewPersisted {
        op,
        account_id,
        result: Ok(5),
    });
    assert!(machine.saved_views.is_empty());
    assert_eq!(
        machine.view_write.unwrap().state,
        ViewWriteState::Failed,
        "a discarded answer cannot turn the attempt into a success"
    );
}

#[test]
fn an_older_attempts_answer_never_replaces_a_newer_attempts_result() {
    let (mut machine, _) = loaded(false, LibraryCache::default());
    machine.handle(Input::SetSort(Sort {
        key: SortKey::Title,
        direction: SortDirection::Descending,
    }));
    let first = machine.handle(Input::SaveView {
        name: "First".to_owned(),
    });
    let (first_op, account_id, _) = persist_view(&first);
    let second = machine.handle(Input::SaveView {
        name: "Second".to_owned(),
    });
    let (second_op, _, _) = persist_view(&second);
    assert!(second_op > first_op);

    machine.handle(persisted(&account_id, second_op, 22));
    let success = machine.view_write.clone().unwrap();
    assert_eq!(
        (success.run, success.state, success.saved_view_id),
        (second_op, ViewWriteState::Succeeded, Some(22))
    );

    // The older write fails after it: it is still discarded from memory and never regresses the
    // newer attempt's visible result.
    machine.handle(Input::ViewPersisted {
        op: first_op,
        account_id,
        result: Err(()),
    });
    let write = machine.view_write.clone().unwrap();
    assert_eq!(
        (write.run, write.state, write.error),
        (second_op, ViewWriteState::Succeeded, None)
    );
    assert_eq!(
        machine
            .saved_views
            .iter()
            .map(|view| view.id)
            .collect::<Vec<_>>(),
        [22]
    );
}
