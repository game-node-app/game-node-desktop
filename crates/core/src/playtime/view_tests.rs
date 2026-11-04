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
        view_for_entry("a", "game", PlaytimeSource::Internal, &[], &[], 0).seconds,
        Some(0)
    );
}
