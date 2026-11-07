use super::*;
fn session(account: &str, key: &str, start: i64, end: Option<i64>) -> Session {
    Session {
        id: "session".into(),
        account_id: account.into(),
        entry_key: key.into(),
        game_id: None,
        source: "steam".into(),
        platform_id: 6,
        started_at: start,
        checkpoint_at: start,
        ended_at: end,
    }
}
#[test]
fn selected_history_is_not_merged_and_other_accounts_are_excluded() {
    let totals = vec![Total {
        account_id: "a".into(),
        entry_key: "steam:620".into(),
        total_seconds: 45240,
        last_played_at: Some(1),
        accepted_game_id: None,
        accepted_seconds: None,
        rejected_seconds: None,
    }];
    let sessions = vec![
        session("a", "steam:620", 100, Some(160)),
        session("other", "steam:620", 0, Some(1000)),
        session("a", "steam:70", 0, Some(1000)),
    ];
    assert_eq!(
        view_for_entry(
            "a",
            "steam:620",
            PlaytimeSource::Store,
            &totals,
            &sessions,
            200
        )
        .seconds,
        Some(45240)
    );
    assert_eq!(
        view_for_entry(
            "a",
            "steam:620",
            PlaytimeSource::Internal,
            &totals,
            &sessions,
            200
        )
        .seconds,
        Some(60)
    );
    assert_eq!(
        view_for_entry(
            "other",
            "steam:620",
            PlaytimeSource::Store,
            &totals,
            &sessions,
            200
        )
        .seconds,
        None
    );
}
#[test]
fn active_session_advances_without_counting_its_checkpoint_twice() {
    let sessions = vec![
        session("a", "game", 10, Some(70)),
        session("a", "game", 100, None),
    ];
    assert_eq!(
        view_for_entry("a", "game", PlaytimeSource::Internal, &[], &sessions, 130).seconds,
        Some(90)
    );
    assert_eq!(
        view_for_entry("a", "game", PlaytimeSource::Internal, &[], &sessions, 190).seconds,
        Some(150)
    );
    assert_eq!(
        view_for_entry("a", "game", PlaytimeSource::Internal, &[], &sessions, 90).seconds,
        Some(60)
    );
}
#[test]
fn missing_store_data_is_unknown_and_empty_internal_history_is_zero() {
    assert_eq!(
        view_for_entry("a", "game", PlaytimeSource::Store, &[], &[], 0).seconds,
        None
    );
    assert_eq!(
        view_for_entry("a", "game", PlaytimeSource::Store, &[], &[], 0).last_played_at,
        None
    );
    assert_eq!(
        view_for_entry("a", "game", PlaytimeSource::Internal, &[], &[], 0).seconds,
        Some(0)
    );
    assert_eq!(
        view_for_entry("a", "game", PlaytimeSource::Internal, &[], &[], 0).last_played_at,
        None,
        "no session means no date, not an invented one"
    );
}

#[test]
fn last_played_comes_from_the_selected_source() {
    let totals = vec![Total {
        account_id: "a".into(),
        entry_key: "steam:620".into(),
        total_seconds: 100,
        last_played_at: Some(5_000),
        accepted_game_id: None,
        accepted_seconds: None,
        rejected_seconds: None,
    }];
    let sessions = vec![
        session("a", "steam:620", 100, Some(160)),
        session("a", "steam:620", 900, Some(1_000)),
    ];

    assert_eq!(
        view_for_entry(
            "a",
            "steam:620",
            PlaytimeSource::Store,
            &totals,
            &sessions,
            2_000
        )
        .last_played_at,
        Some(5_000)
    );
    assert_eq!(
        view_for_entry(
            "a",
            "steam:620",
            PlaytimeSource::Internal,
            &totals,
            &sessions,
            2_000
        )
        .last_played_at,
        Some(1_000)
    );
    // Another account's sessions never leak in.
    assert_eq!(
        view_for_entry(
            "b",
            "steam:620",
            PlaytimeSource::Internal,
            &totals,
            &sessions,
            2_000
        )
        .last_played_at,
        None
    );
}

#[test]
fn an_open_session_counts_as_played_until_now_for_last_played() {
    let sessions = vec![session("a", "game", 100, None)];
    let view = view_for_entry("a", "game", PlaytimeSource::Internal, &[], &sessions, 250);
    assert_eq!(view.seconds, Some(150));
    assert_eq!(view.last_played_at, Some(250));
    // The current session keeps advancing as time passes.
    let view = view_for_entry("a", "game", PlaytimeSource::Internal, &[], &sessions, 400);
    assert_eq!(view.last_played_at, Some(400));
}

#[test]
fn a_negative_store_total_is_unknown_including_its_date() {
    let totals = vec![Total {
        account_id: "a".into(),
        entry_key: "steam:1".into(),
        total_seconds: -5,
        last_played_at: Some(10),
        accepted_game_id: None,
        accepted_seconds: None,
        rejected_seconds: None,
    }];
    let view = view_for_entry("a", "steam:1", PlaytimeSource::Store, &totals, &[], 0);
    assert_eq!(view.seconds, None);
    assert_eq!(view.last_played_at, None);
}
