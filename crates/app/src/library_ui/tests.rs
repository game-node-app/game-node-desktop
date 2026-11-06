use gamenode_core::library::{
    AddOutcome, Availability, Direction, EntryStatus, Input as LibInput, LaunchError, LaunchKind,
    LaunchState, LaunchStatus, MatchSource, MatchStatus, Membership, Reason, SearchFailure,
    SelectMode, SidebarSelection, SortDirection, SortKey, ViewMode,
};

use super::*;

#[test]
fn statuses_round_trip_and_unknown_is_not_pickable() {
    for status in [
        EntryStatus::Playing,
        EntryStatus::Finished,
        EntryStatus::Planned,
        EntryStatus::Dropped,
        EntryStatus::Ongoing,
    ] {
        assert_eq!(entry_status(status_kind(status)), Some(status));
    }
    assert_eq!(entry_status(StatusKind::Unknown), None);
}

#[test]
fn availability_maps_to_a_reason_or_available() {
    assert_eq!(availability(Availability::Available), Why::Available);
    assert_eq!(
        availability(Availability::Disabled(Reason::MultipleSelected)),
        Why::MultipleSelected
    );
    assert_eq!(
        availability(Availability::Disabled(Reason::AlreadyInLibrary(Some(
            EntryStatus::Planned
        )))),
        Why::AlreadyInLibrary
    );
}

#[test]
fn match_status_maps_kind_and_source() {
    let chosen = MatchStatus::Matched {
        game_id: 7,
        source: MatchSource::Chosen,
    };
    let automatic = MatchStatus::Matched {
        game_id: 7,
        source: MatchSource::Automatic,
    };
    assert_eq!(match_kind(&chosen), MatchKind::Matched);
    assert_eq!(source_kind(&chosen), SourceKind::Chosen);
    assert_eq!(source_kind(&automatic), SourceKind::Automatic);
    assert_eq!(match_kind(&MatchStatus::NoGame), MatchKind::NoGame);
    assert_eq!(match_kind(&MatchStatus::Unmatched), MatchKind::Unmatched);
    assert_eq!(source_kind(&MatchStatus::Unmatched), SourceKind::None);
}

#[test]
fn membership_maps_to_standing_with_status() {
    assert_eq!(
        standing(Membership::InLibrary(Some(EntryStatus::Finished))),
        (Standing::InLibrary, StatusKind::Finished)
    );
    assert_eq!(
        standing(Membership::InLibrary(None)),
        (Standing::InLibrary, StatusKind::Unknown)
    );
    assert_eq!(standing(Membership::Checking).0, Standing::Checking);
}

fn launch(kind: LaunchKind, state: LaunchState) -> LaunchStatus {
    LaunchStatus {
        entry_key: "steam:1".into(),
        kind,
        state,
    }
}

#[test]
fn launch_note_shows_only_for_the_selected_game() {
    let opening = launch(LaunchKind::Play, LaunchState::Opening);
    assert_eq!(
        launch_note(Some(&opening), Some("steam:1")),
        LaunchNote::OpeningPlay
    );
    assert_eq!(
        launch_note(Some(&opening), Some("steam:2")),
        LaunchNote::None
    );
    assert_eq!(launch_note(Some(&opening), None), LaunchNote::None);
    assert_eq!(launch_note(None, Some("steam:1")), LaunchNote::None);
    let failed = launch(
        LaunchKind::Install,
        LaunchState::Failed(LaunchError::NoHandler),
    );
    assert_eq!(
        launch_note(Some(&failed), Some("steam:1")),
        LaunchNote::NoHandler
    );
    let install = launch(LaunchKind::Install, LaunchState::Opening);
    assert_eq!(
        launch_note(Some(&install), Some("steam:1")),
        LaunchNote::OpeningInstall
    );
}

#[test]
fn search_failures_carry_the_blocking_reason() {
    assert_eq!(search_error(None), (SearchError::None, None));
    assert_eq!(
        search_error(Some(SearchFailure::Blocked(Reason::Offline))),
        (SearchError::Blocked, Some(Why::Offline))
    );
    assert_eq!(
        search_error(Some(SearchFailure::Failed)).0,
        SearchError::Failed
    );
    assert_eq!(
        search_error(Some(SearchFailure::Rejected)).0,
        SearchError::Rejected
    );
}

#[test]
fn add_outcomes_map_to_kind_and_status() {
    assert_eq!(
        outcome(&AddOutcome::Added(EntryStatus::Planned)),
        (Outcome::Added, StatusKind::Planned)
    );
    assert_eq!(
        outcome(&AddOutcome::AlreadyPresent(None)),
        (Outcome::AlreadyPresent, StatusKind::Unknown)
    );
    assert_eq!(outcome(&AddOutcome::SkippedUnmatched).0, Outcome::Skipped);
    assert_eq!(
        outcome(&AddOutcome::Failed(
            gamenode_core::game_api::ApiError::Failed
        ))
        .0,
        Outcome::Failed
    );
}

#[test]
fn clicks_pick_the_selection_mode() {
    let mode = |ctrl, shift| match select_input("k", ctrl, shift) {
        LibInput::Select { mode, .. } => mode,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(mode(false, false), SelectMode::Replace);
    assert_eq!(mode(true, false), SelectMode::Toggle);
    assert_eq!(mode(false, true), SelectMode::Range);
    assert_eq!(mode(true, true), SelectMode::Range);
}

#[test]
fn arrow_codes_map_to_directions() {
    let codes: Vec<_> = (0..6)
        .map(|code| match move_input(code, true) {
            Some(LibInput::Move { direction, extend }) => {
                assert!(extend);
                direction
            }
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        codes,
        [
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
            Direction::Home,
            Direction::End
        ]
    );
    assert!(move_input(6, false).is_none());
    assert!(move_input(-1, false).is_none());
}

#[test]
fn filter_sort_view_and_sizes() {
    assert!(matches!(
        filter_input(""),
        LibInput::SetFilter(SidebarSelection::All)
    ));
    assert!(matches!(
        filter_input("steam"),
        LibInput::SetFilter(SidebarSelection::Provider(ref id)) if id == "steam"
    ));
    match sort_input(SortKind::Provider, true) {
        LibInput::SetSort(sort) => {
            assert_eq!(sort.key, SortKey::Provider);
            assert_eq!(sort.direction, SortDirection::Descending);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(matches!(
        view_input(ViewKind::List),
        LibInput::SetView(ViewMode::List)
    ));
    assert!(matches!(
        width_input(300, LibInput::SetSidebarWidth),
        Some(LibInput::SetSidebarWidth(300))
    ));
    assert!(width_input(-5, LibInput::SetSidebarWidth).is_none());
    assert!(matches!(columns_input(0), LibInput::SetColumns(1)));
    assert!(matches!(columns_input(4), LibInput::SetColumns(4)));
}

#[test]
fn only_numeric_game_ids_can_be_picked() {
    assert!(matches!(
        pick_input("42", 9),
        Some(LibInput::PickMatch {
            game_id: 42,
            now: 9
        })
    ));
    assert!(pick_input("abc", 9).is_none());
    assert!(pick_input("", 9).is_none());
}

#[test]
fn cover_names_cannot_leave_the_covers_directory() {
    let dir = std::env::temp_dir();
    assert!(load_cover(&dir, "../x.jpg").is_none());
    assert!(load_cover(&dir, "a/b.jpg").is_none());
    assert!(load_cover(&dir, ".hidden").is_none());
    assert!(load_cover(&dir, "missing.jpg").is_none());
}

mod render {
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use gamenode_core::library::{
        Access, Actions, AddDialogView, AddItem, AddSummary, AddTarget, CollectionOption,
        CollectionsView, DetailView, EmptyState, LibraryLayout, LibraryView, MatchDialogView,
        MatchOption, ProviderCount, Row,
    };
    use gamenode_core::{AuthView, Settings, Snapshot, StorageMethod};
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::platform::{Platform, PlatformError, WindowAdapter};
    use slint::{ComponentHandle, Model};

    use super::super::*;

    struct Headless(Rc<MinimalSoftwareWindow>);

    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
            Ok(self.0.clone())
        }
        fn duration_since_start(&self) -> Duration {
            Duration::ZERO
        }
    }

    fn row(key: &str, title: &str, status: MatchStatus) -> Arc<Row> {
        Arc::new(Row {
            playtime: Default::default(),
            running: false,
            key: key.into(),
            title: title.into(),
            detected_name: title.into(),
            matched_name: None,
            provider: "steam".into(),
            provider_name: "Steam".into(),
            status,
            cover_file: None,
        })
    }

    fn view(rows: Vec<Arc<Row>>) -> LibraryView {
        LibraryView {
            generation: 1,
            layout: LibraryLayout::default(),
            search: String::new(),
            access: Access::Verified,
            total: rows.len(),
            providers: vec![ProviderCount {
                provider: "steam".into(),
                name: "Steam".into(),
                count: rows.len(),
            }],
            rows: rows.into(),
            scanning: false,
            failed_providers: vec![],
            selected: vec![],
            focused: None,
            actions: Actions {
                selected: 0,
                play: Availability::Disabled(Reason::NothingSelected),
                install: Availability::Disabled(Reason::NothingSelected),
                match_game: Availability::Disabled(Reason::NothingSelected),
                add_to_gamenode: Availability::Disabled(Reason::NothingSelected),
                change_match: false,
            },
            detail: None,
            empty: None,
            match_dialog: None,
            add_dialog: None,
        }
    }

    fn snapshot(library: Option<LibraryView>, launch: Option<LaunchStatus>) -> Snapshot {
        Snapshot {
            local_games: Default::default(),
            playtime_error: false,
            tracking: Default::default(),
            sync: Default::default(),
            playtime_source: Default::default(),
            auth: AuthView::SignedOut,
            notice: None,
            storage: StorageMethod::PrivateFile,
            settings: Settings::default(),
            library,
            launch,
            tools: Default::default(),
        }
    }

    fn option(id: u64, name: &str, current: bool) -> MatchOption {
        MatchOption {
            game_id: id,
            name: name.into(),
            release_year: Some(2011),
            cover_url: None,
            cover_file: None,
            current,
        }
    }

    /// One test, because Slint allows a single platform per process.
    #[test]
    fn snapshots_are_copied_into_the_screens() {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
        let ui = AppWindow::new().unwrap();
        let state = ui.global::<LibraryState>();
        let dir = tempfile::tempdir().unwrap();
        let mut renderer = Renderer::new(dir.path().to_owned(), &ui);
        renderer.attach(&ui);

        // Disk-backed covers are unavailable on the calling thread and decoded by the worker.
        std::fs::write(
            dir.path().join("async.png"),
            include_bytes!("../../ui/icons/gamenode.png"),
        )
        .unwrap();
        assert!(renderer.cover(Some("async.png")).is_none());
        let (file, decoded) = renderer
            .cover_results
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        assert_eq!(file, "async.png");
        assert!(decoded.is_some());

        // No library: the screen is off and nothing is listed.
        renderer.render(&ui, &snapshot(None, None));
        assert!(!state.get_active());
        assert_eq!(state.get_items().row_count(), 0);

        // Rows, selection, focus, filter, sort, view and pane sizes.
        let mut rows = vec![
            row(
                "steam:1",
                "Portal 2",
                MatchStatus::Matched {
                    game_id: 7,
                    source: MatchSource::Chosen,
                },
            ),
            row("steam:2", "Valheim", MatchStatus::NoGame),
            row("steam:3", "Mystery", MatchStatus::Unmatched),
        ];
        Arc::make_mut(&mut rows[0]).playtime = gamenode_core::playtime::PlaytimeView {
            seconds: Some(45240),
            source: gamenode_core::playtime::PlaytimeSource::Store,
        };
        let mut v = view(rows.clone());
        v.selected = vec!["steam:2".into()];
        v.focused = Some("steam:2".into());
        v.layout.view = ViewMode::List;
        v.layout.sidebar = SidebarSelection::Provider("steam".into());
        v.layout.sort.direction = SortDirection::Descending;
        v.layout.sidebar_width = 300;
        v.layout.detail_width = 400;
        v.layout.detail_open = false;
        v.scanning = true;
        v.providers = vec![
            ProviderCount {
                provider: "steam".into(),
                name: "Steam".into(),
                count: 3,
            },
            ProviderCount {
                provider: "gog".into(),
                name: "GOG".into(),
                count: 0,
            },
        ];
        v.failed_providers = vec!["Steam".into()];
        v.access = Access::Unverified;
        renderer.render(&ui, &snapshot(Some(v), None));
        assert!(state.get_active());
        let items = state.get_items();
        assert_eq!(items.row_count(), 3);
        assert_eq!(items.row_data(0).unwrap().title, "Portal 2");
        assert!(items.row_data(0).unwrap().playtime_known);
        assert_eq!(items.row_data(0).unwrap().playtime_hours, 12);
        assert_eq!(items.row_data(0).unwrap().playtime_minutes, 34);
        assert!(!items.row_data(1).unwrap().playtime_known);
        assert_eq!(items.row_data(0).unwrap().kind, MatchKind::Matched);
        assert_eq!(items.row_data(1).unwrap().kind, MatchKind::NoGame);
        assert!(items.row_data(1).unwrap().selected);
        assert!(items.row_data(1).unwrap().focused);
        assert!(!items.row_data(0).unwrap().selected);
        assert_eq!(state.get_focused_index(), 1);
        assert_eq!(state.get_view(), ViewKind::List);
        assert!(!state.get_all_selected());
        assert!(state.get_providers().row_data(0).unwrap().selected);
        assert!(
            state.get_providers().row_data(0).unwrap().failed,
            "Steam should be marked failed"
        );
        assert!(
            !state.get_providers().row_data(1).unwrap().failed,
            "GOG should not be marked failed"
        );
        assert!(state.get_descending());
        assert_eq!(state.get_sidebar_width(), 300);
        assert_eq!(state.get_detail_width(), 400);
        assert!(!state.get_detail_open());
        assert!(state.get_scanning());
        assert_eq!(state.get_failed_stores(), "Steam");
        assert_eq!(state.get_access(), AccessKind::Unverified);

        // Titles are shown exactly as received, whatever characters they hold.
        let odd = "Ünïcode {} <b>% 100 日本語";
        renderer.render(
            &ui,
            &snapshot(
                Some(view(vec![row("steam:9", odd, MatchStatus::Unmatched)])),
                None,
            ),
        );
        assert_eq!(state.get_items().row_data(0).unwrap().title, odd);

        // The detail pane and the launch note for the selected game only.
        let mut v = view(rows.clone());
        v.selected = vec!["steam:1".into()];
        v.actions = Actions {
            selected: 1,
            play: Availability::Available,
            install: Availability::Disabled(Reason::NoInstallRule),
            match_game: Availability::Available,
            add_to_gamenode: Availability::Available,
            change_match: true,
        };
        v.detail = Some(DetailView {
            running: false,
            row: rows[0].clone(),
            install_dir: "C:\\Steam\\Portal 2".into(),
            release_year: Some(2011),
            artwork: None,
            membership: Membership::InLibrary(Some(EntryStatus::Finished)),
        });
        let launch = LaunchStatus {
            entry_key: "steam:1".into(),
            kind: LaunchKind::Play,
            state: LaunchState::Failed(LaunchError::NoHandler),
        };
        renderer.render(&ui, &snapshot(Some(v.clone()), Some(launch)));
        assert!(state.get_detail_valid());
        assert_eq!(state.get_detail_title(), "Portal 2");
        assert!(state.get_detail_playtime_known());
        assert_eq!(state.get_detail_playtime_hours(), 12);
        assert_eq!(state.get_detail_playtime_minutes(), 34);
        assert!(state.get_detail_playtime_from_store());
        assert_eq!(state.get_detail_year(), "2011");
        assert_eq!(state.get_detail_dir(), "C:\\Steam\\Portal 2");
        assert_eq!(state.get_detail_source(), SourceKind::Chosen);
        assert_eq!(state.get_detail_standing(), Standing::InLibrary);
        assert_eq!(state.get_detail_standing_status(), StatusKind::Finished);
        assert_eq!(state.get_play_why(), Why::Available);
        assert_eq!(state.get_install_why(), Why::NoInstallRule);
        assert_eq!(state.get_add_why(), Why::Available);
        assert!(state.get_edit_existing());
        assert!(state.get_change_match());
        assert_eq!(state.get_launch_note(), LaunchNote::NoHandler);
        // A valid portrait must never be substituted while artwork is pending or unavailable.
        let cover =
            Image::load_from_data(include_bytes!("../../ui/icons/gamenode.png"), None).unwrap();
        renderer.covers.insert("7.jpg".into(), Some(cover));
        Arc::make_mut(&mut v.detail.as_mut().unwrap().row).cover_file = Some("7.jpg".into());
        renderer.render(&ui, &snapshot(Some(v.clone()), None));
        assert!(!state.get_detail_has_cover());
        assert_eq!(state.get_detail_cover().size().width, 0);

        let capture = |name: &str| {
            if let Ok(dir) = std::env::var("GAMENODE_ARTWORK_RENDER_DIR") {
                use std::io::Write;
                ui.set_signed_in(true);
                ui.window().set_size(slint::LogicalSize::new(1200., 800.));
                ui.show().unwrap();
                ui.window().request_redraw();
                let mut pixels = vec![slint::Rgb8Pixel::default(); 1200 * 800];
                window.draw_if_needed(|renderer| {
                    renderer.render(&mut pixels, 1200);
                });
                let mut file =
                    std::fs::File::create(std::path::Path::new(&dir).join(name)).unwrap();
                write!(file, "P6\n1200 800\n255\n").unwrap();
                for pixel in pixels {
                    file.write_all(&[pixel.r, pixel.g, pixel.b]).unwrap();
                }
            }
        };
        capture("artwork-pending.ppm");
        v.detail.as_mut().unwrap().artwork = Some(Arc::from(
            include_bytes!("../../ui/icons/gamenode.png").as_slice(),
        ));
        renderer.render(&ui, &snapshot(Some(v.clone()), None));
        assert!(
            !state.get_detail_has_cover(),
            "selection renders before artwork decoding"
        );
        assert_eq!(state.get_detail_title(), "Portal 2");
        renderer.artwork.wait(&ui);
        assert!(state.get_detail_has_cover());
        assert!(state.get_detail_cover().size().width > 0);
        capture("artwork-loaded.ppm");
        v.detail.as_mut().unwrap().artwork = None;
        renderer.render(&ui, &snapshot(Some(v.clone()), None));
        assert!(
            !state.get_detail_has_cover(),
            "previous artwork must be cleared"
        );
        // A late decode for a game we left must not reappear in the empty details pane.
        let bytes = Arc::from(include_bytes!("../../ui/icons/gamenode.png").as_slice());
        assert!(renderer.artwork.get(&ui, 99, &bytes).is_none());
        renderer.render(&ui, &snapshot(Some(v.clone()), None));
        renderer.artwork.wait(&ui);
        assert!(!state.get_detail_has_cover());
        // Both games remain decoded after switching, so returning needs no worker round-trip.
        assert!(renderer.artwork.get(&ui, 99, &bytes).is_some());
        assert!(renderer.artwork.get(&ui, 100, &bytes).is_none());
        renderer.artwork.wait(&ui);
        assert!(renderer.artwork.get(&ui, 99, &bytes).is_some());

        let other = LaunchStatus {
            entry_key: "steam:2".into(),
            kind: LaunchKind::Play,
            state: LaunchState::Opening,
        };
        renderer.render(&ui, &snapshot(Some(v), Some(other)));
        assert_eq!(state.get_launch_note(), LaunchNote::None);

        // Empty states.
        let mut v = view(vec![]);
        v.empty = Some(EmptyState::NoGames {
            stores: vec!["Steam".into(), "GOG".into()],
        });
        renderer.render(&ui, &snapshot(Some(v), None));
        assert_eq!(state.get_empty(), EmptyKind::NoGames);
        assert_eq!(state.get_stores(), "Steam, GOG");
        let mut v = view(vec![]);
        v.empty = Some(EmptyState::NothingMatches);
        renderer.render(&ui, &snapshot(Some(v), None));
        assert_eq!(state.get_empty(), EmptyKind::NothingMatches);

        // The match dialog: the query is pushed when it opens, then the text box owns it.
        let dialog = |query: &str, searching: bool| MatchDialogView {
            entry_key: "steam:1".into(),
            detected_name: "Portal 2".into(),
            provider_name: "Steam".into(),
            query: query.into(),
            searching,
            candidates: vec![option(7, "Portal 2", true)],
            results: vec![option(8, "Portal", false)],
            no_results: false,
            error: Some(SearchFailure::Blocked(Reason::Offline)),
            search_blocked: None,
            current: MatchStatus::Unmatched,
            can_clear: true,
        };
        let mut v = view(rows.clone());
        v.match_dialog = Some(dialog("Portal 2", false));
        renderer.render(&ui, &snapshot(Some(v), None));
        assert!(state.get_match_open());
        assert_eq!(state.get_match_query(), "Portal 2");
        assert_eq!(state.get_match_candidates().row_data(0).unwrap().id, "7");
        assert!(state.get_match_candidates().row_data(0).unwrap().current);
        assert_eq!(
            state.get_match_results().row_data(0).unwrap().name,
            "Portal"
        );
        assert_eq!(state.get_match_error(), SearchError::Blocked);
        assert_eq!(state.get_match_blocked(), Why::Offline);
        assert!(state.get_match_can_clear());
        state.set_match_query("Portal 3 being typed".into());
        let mut v = view(rows.clone());
        v.match_dialog = Some(dialog("Portal 2", true));
        renderer.render(&ui, &snapshot(Some(v), None));
        assert!(state.get_match_searching());
        assert_eq!(
            state.get_match_query(),
            "Portal 3 being typed",
            "typing must not be overwritten"
        );
        renderer.render(&ui, &snapshot(Some(view(rows.clone())), None));
        assert!(!state.get_match_open());

        // The add dialog: choosing, then the summary with its counts.
        let mut v = view(rows.clone());
        v.add_dialog = Some(AddDialogView {
            editing_existing: true,
            targets: vec![
                AddTarget {
                    entry_key: "steam:1".into(),
                    title: "Portal 2".into(),
                    matched: true,
                },
                AddTarget {
                    entry_key: "steam:3".into(),
                    title: "Mystery".into(),
                    matched: false,
                },
            ],
            status: EntryStatus::Planned,
            applied_status: EntryStatus::Playing,
            applied_from: Some("Now playing".into()),
            collections: CollectionsView::Loaded(vec![CollectionOption {
                id: "c1".into(),
                name: "Backlog".into(),
                default_status: Some(EntryStatus::Planned),
                picked: true,
            }]),
            progress: None,
            error: None,
            can_confirm: true,
            blocked: None,
            summary: None,
        });
        renderer.render(&ui, &snapshot(Some(v.clone()), None));
        assert!(state.get_add_open());
        assert!(state.get_add_editing());
        assert_eq!(state.get_add_targets().row_count(), 2);
        assert!(!state.get_add_targets().row_data(1).unwrap().matched);
        assert_eq!(state.get_add_status(), StatusKind::Planned);
        assert_eq!(state.get_add_applied_status(), StatusKind::Playing);
        assert_eq!(state.get_add_applied_from(), "Now playing");
        assert_eq!(state.get_add_collections_kind(), CollectionsKind::Loaded);
        let collection = state.get_add_collections().row_data(0).unwrap();
        assert_eq!(
            (
                collection.name.as_str(),
                collection.picked,
                collection.forces
            ),
            ("Backlog", true, StatusKind::Planned)
        );
        assert!(state.get_add_can_confirm());
        assert!(!state.get_add_has_summary());

        let dialog = v.add_dialog.as_mut().unwrap();
        dialog.progress = Some(gamenode_core::library::AddProgress { done: 1, total: 2 });
        renderer.render(&ui, &snapshot(Some(v.clone()), None));
        assert!(state.get_add_running());
        assert_eq!((state.get_add_done(), state.get_add_total()), (1, 2));

        let dialog = v.add_dialog.as_mut().unwrap();
        dialog.summary = Some(AddSummary {
            items: vec![
                AddItem {
                    entry_key: "steam:1".into(),
                    title: "Portal 2".into(),
                    outcome: AddOutcome::Added(EntryStatus::Planned),
                },
                AddItem {
                    entry_key: "steam:2".into(),
                    title: "Valheim".into(),
                    outcome: AddOutcome::AlreadyPresent(None),
                },
                AddItem {
                    entry_key: "steam:3".into(),
                    title: "Mystery".into(),
                    outcome: AddOutcome::SkippedUnmatched,
                },
                AddItem {
                    entry_key: "steam:4".into(),
                    title: "Other".into(),
                    outcome: AddOutcome::Failed(gamenode_core::game_api::ApiError::Failed),
                },
            ],
        });
        renderer.render(&ui, &snapshot(Some(v), None));
        assert!(state.get_add_has_summary());
        assert!(!state.get_add_running());
        assert_eq!(
            [
                state.get_add_summary_added(),
                state.get_add_summary_present(),
                state.get_add_summary_skipped(),
                state.get_add_summary_failed()
            ],
            [1, 1, 1, 1]
        );
        assert_eq!(state.get_add_summary().row_count(), 4);
        renderer.render(&ui, &snapshot(Some(view(rows)), None));
        assert!(!state.get_add_open());

        // Signing out empties the screen again.
        renderer.render(&ui, &snapshot(None, None));
        assert!(!state.get_active());
        assert_eq!(state.get_items().row_count(), 0);
    }
}

#[test]
fn discovery_rows_only_change_when_visible_data_changes() {
    let row = crate::DiscoveryRow {
        fingerprint: "game".into(),
        title: "Game".into(),
        targets: ModelRc::new(VecModel::from(vec!["game.exe".into()])),
        ..Default::default()
    };
    let model = ModelRc::new(VecModel::from(vec![row.clone()]));
    assert!(same_discovery_rows(&model, std::slice::from_ref(&row)));
    let mut updated = row;
    updated.picked = true;
    assert!(!same_discovery_rows(&model, &[updated]));
}

#[test]
fn format_bytes_renders_human_readable_units() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1023), "1023 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(48_120), "47.0 KB");
    assert_eq!(format_bytes(1_048_576), "1.0 MB");
    assert_eq!(format_bytes(1_572_864), "1.5 MB");
    assert_eq!(format_bytes(1_073_741_824), "1.00 GB");
    assert_eq!(format_bytes(2_684_354_560), "2.50 GB");
}

#[test]
fn profile_save_paths_returns_resolved_files_or_extracts_definition_patterns() {
    use gamenode_core::drm_free::{SaveProfile, SaveSupport};

    let resolved = SaveProfile {
        id: "p1".into(),
        installation: "inst1".into(),
        revision: 1,
        title: "Celeste".into(),
        manifest_hash: "h".into(),
        definition: "files:\n  <base>/Saves: {}\n".into(),
        files: vec![
            r"D:\Games\Celeste\Saves\0.celeste".into(),
            r"D:\Games\Celeste\Saves\settings.celeste".into(),
        ],
        registry: vec![],
        support: SaveSupport::Supported,
        automatic: None,
    };
    assert_eq!(
        profile_save_paths(&resolved),
        vec![
            r"D:\Games\Celeste\Saves\0.celeste".to_owned(),
            r"D:\Games\Celeste\Saves\settings.celeste".to_owned(),
        ]
    );

    let fallback_yaml = SaveProfile {
        files: vec![],
        support: SaveSupport::NoSaves,
        definition:
            "files:\n  <base>/Saves:\n    when:\n      - os: windows\n  <winAppData>/Celeste: {}\n"
                .into(),
        ..resolved.clone()
    };
    assert_eq!(
        profile_save_paths(&fallback_yaml),
        vec!["<base>/Saves".to_owned(), "<winAppData>/Celeste".to_owned()]
    );

    let empty = SaveProfile {
        files: vec![],
        definition: String::new(),
        ..resolved
    };
    assert!(profile_save_paths(&empty).is_empty());
}

#[test]
fn cached_library_covers_have_bounded_pixel_memory() {
    let directory = tempfile::tempdir().unwrap();
    image::RgbaImage::new(1200, 1800)
        .save(directory.path().join("1.png"))
        .unwrap();
    let pixels = super::decode_cover(directory.path(), "1.png").unwrap();
    assert_eq!((pixels.width(), pixels.height()), (320, 480));
    assert!(pixels.as_bytes().len() <= 320 * 480 * 4);
}
