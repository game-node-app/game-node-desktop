//! Achievement scheduler, cache and command behavior at the engine boundary.

use super::*;

/// Achievement tests share the test provider runtime's concurrency counter.
static ACHIEVEMENT_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

use gamenode_core::achievements::{
    AchievementFilter, AchievementGameState, AchievementRecord, AchievementSummaryView,
};
use gamenode_core::library::{LibraryCache, ProviderScanState, StoredEntry};
use gamenode_plugin_protocol::models;

fn entry(store_id: &str) -> StoredEntry {
    StoredEntry {
        key: format!("steam:{store_id}"),
        provider: "steam".to_owned(),
        store_id: store_id.to_owned(),
        name: Some(format!("Game {store_id}")),
        install_dir: format!("Game {store_id}"),
        fields: Default::default(),
        installed: true,
        owned: false,
        family_shared: false,
        first_seen: 1,
        last_seen: 1,
    }
}

fn cache(ids: &[&str]) -> LibraryCache {
    LibraryCache {
        entries: ids.iter().map(|id| entry(id)).collect(),
        scans: vec![ProviderScanState {
            provider: "steam".to_owned(),
            last_success_at: Some(1),
            ..Default::default()
        }],
        ..LibraryCache::default()
    }
}

fn store_account() -> gamenode_core::library::StoreAccount {
    gamenode_core::library::StoreAccount {
        provider: "steam".to_owned(),
        account_id: "A".to_owned(),
        account_name: Some("Ana".to_owned()),
        last_verified_at: Some(1),
    }
}

fn hold_worker(h: &Harness) {
    h.engine
        .shared
        .achievement_queues
        .lock()
        .unwrap()
        .entry("steam".to_owned())
        .or_default()
        .running = true;
}

fn pending(h: &Harness) -> Vec<(String, u8)> {
    h.engine
        .shared
        .achievement_queues
        .lock()
        .unwrap()
        .get("steam")
        .map(|queue| {
            queue
                .pending
                .iter()
                .map(|job| (job.entry_id.clone(), job.priority))
                .collect()
        })
        .unwrap_or_default()
}

fn record(
    id: &str,
    unlocked: bool,
    at: Option<i64>,
    hidden: bool,
    position: i64,
) -> AchievementRecord {
    AchievementRecord {
        id: id.into(),
        name: format!("Name {id}"),
        description: format!("Description {id}"),
        hidden,
        icon_url: String::new(),
        icon_gray_url: String::new(),
        unlocked,
        unlocked_at: at,
        position,
    }
}

#[test]
fn trusted_icon_urls_become_asset_routes_and_untrusted_ones_stay_empty() {
    let url = "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/620/a.jpg";
    let hash = gamenode_platform::achievement_icons::achievement_icon_hash(url);
    assert_eq!(
        crate::engine::achievements::icon_asset(url),
        format!("gamenode-asset://localhost/achievement/{hash}"),
        "the host returns the canonical route; the UI maps its base to the webview form"
    );
    assert_eq!(
        crate::engine::achievements::icon_asset(
            "http://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/620/a.jpg"
        ),
        "",
        "a URL that may not be fetched keeps the placeholder"
    );
}

#[test]
fn a_selected_job_is_not_batched_behind_background_imports() {
    use crate::engine::achievements::{Job, Queue, batch_size};

    let now = Instant::now();
    let job = |entry_id: &str, priority: u8, sequence: u64| Job {
        entry_id: entry_id.to_owned(),
        priority,
        due: now,
        sequence,
    };
    let mut queue = Queue {
        pending: vec![job("background", 3, 0), job("selected", 1, 1)],
        ..Default::default()
    };
    queue
        .pending
        .sort_by_key(|job| (job.priority, job.due, job.sequence));
    assert_eq!(
        batch_size(&queue, false),
        1,
        "the selected game runs alone instead of waiting for background games"
    );

    queue.pending.clear();
    for index in 0..12 {
        queue
            .pending
            .push(job(&format!("background-{index}"), 3, index));
    }
    assert_eq!(batch_size(&queue, false), 5);
    assert_eq!(
        batch_size(&queue, true),
        0,
        "a rate-limit pause runs no background work"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn import_skips_fresh_games_and_selected_games_jump_the_queue() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = cache(&["620", "70", "240"]);
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.achievements
        .save_ready(
            "steam",
            "A",
            "620",
            "en",
            &[record("one", true, Some(10), false, 0)],
            100,
        )
        .unwrap();
    hold_worker(&h);
    h.engine.start();
    h.until("the cached library", |snapshot| {
        snapshot
            .library
            .as_ref()
            .is_some_and(|library| library.rows.len() == 3)
    })
    .await;
    let wait = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !pending(&h).is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    wait.expect("the import request reaches the held queue");
    let queued = pending(&h);
    assert!(
        queued.iter().all(|(entry_id, _)| entry_id != "620"),
        "a fresh game is not re-read after a restart: {queued:?}"
    );
    assert!(
        queued.iter().any(|(entry_id, _)| entry_id == "70"),
        "queued={queued:?} saved={}",
        h.achievements.load_summaries("steam", "A").unwrap().len()
    );

    h.engine
        .refresh_selected_achievements("steam:70".to_owned());
    let queued = pending(&h);
    assert_eq!(
        queued
            .iter()
            .find(|(entry_id, _)| entry_id == "70")
            .unwrap()
            .1,
        1,
        "selection is priority 1 even when import queued it first"
    );

    // A stored language that differs from the current locale makes a selected game stale at once.
    let wait = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if h.engine
                .lock()
                .achievement_summaries
                .contains_key(&("steam".to_owned(), "620".to_owned()))
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    wait.expect("the cached summary is loaded");
    let summary = h
        .engine
        .lock()
        .achievement_summaries
        .get(&("steam".to_owned(), "620".to_owned()))
        .cloned()
        .unwrap();
    assert_eq!(summary.language, "en");
    h.engine.lock().achievement_language = "pt-BR".to_owned();
    h.engine
        .refresh_selected_achievements("steam:620".to_owned());
    assert!(pending(&h).iter().any(|(entry_id, _)| entry_id == "620"));
}

#[tokio::test(start_paused = true)]
async fn a_game_exit_refresh_waits_for_the_settling_interval() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = cache(&["620"]);
    h.store.store_accounts.lock().unwrap().push(store_account());
    hold_worker(&h);
    h.engine.schedule_exited_game_achievements("steam:620");
    tokio::task::yield_now().await;
    assert!(
        pending(&h).is_empty(),
        "nothing is requested during the settling interval"
    );
    tokio::time::advance(Duration::from_secs(
        gamenode_core::achievements::GAME_EXIT_SETTLING_SECS as u64,
    ))
    .await;
    tokio::task::yield_now().await;
    assert_eq!(pending(&h), vec![("620".to_owned(), 2)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_list_is_ordered_masked_and_needs_no_gamenode_request() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = cache(&["620"]);
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.achievements
        .save_ready(
            "steam",
            "A",
            "620",
            "en",
            &[
                record("locked-secret", false, None, true, 0),
                record("old", true, Some(100), false, 1),
                record("new", true, Some(300), false, 2),
            ],
            100,
        )
        .unwrap();
    let before = h.api.lookups.lock().unwrap().len();
    let view = h
        .engine
        .achievement_list("steam:620".to_owned(), AchievementFilter::All)
        .await
        .unwrap()
        .unwrap();
    assert!(view.connected);
    assert_eq!(view.state, Some(AchievementGameState::Ready));
    assert_eq!(
        view.achievements
            .iter()
            .map(|achievement| achievement.id.as_str())
            .collect::<Vec<_>>(),
        ["new", "old", "locked-secret"]
    );
    assert_eq!(view.achievements[2].name, "");
    assert_eq!(view.achievements[2].description, "");
    assert!(view.achievements[2].hidden);
    assert_eq!(
        h.api.lookups.lock().unwrap().len(),
        before,
        "the list never reaches GameNode"
    );

    let unlocked = h
        .engine
        .achievement_list("steam:620".to_owned(), AchievementFilter::Unlocked)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unlocked.achievements.len(), 2);

    let none = h
        .engine
        .achievement_list("steam:240".to_owned(), AchievementFilter::All)
        .await
        .unwrap();
    assert!(none.is_none(), "a game never read has no cached row");

    let unsupported = h
        .engine
        .achievement_list("manual:1".to_owned(), AchievementFilter::All)
        .await
        .unwrap();
    assert!(unsupported.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_detail_snapshot_carries_only_the_compact_summary() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = cache(&["620"]);
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.engine.start();
    h.until("the cached library", |snapshot| {
        snapshot
            .library
            .as_ref()
            .is_some_and(|library| library.rows.len() == 1)
    })
    .await;
    h.engine
        .library_input(gamenode_core::library::Input::Select {
            key: "steam:620".to_owned(),
            mode: gamenode_core::library::SelectMode::Replace,
        });
    let snapshot = h
        .until("a ready achievement summary", |snapshot| {
            snapshot
                .library
                .as_ref()
                .and_then(|library| library.detail.as_ref())
                .and_then(|detail| detail.achievements.as_ref())
                .is_some_and(|summary| summary.state == AchievementGameState::Ready)
        })
        .await;
    let detail = snapshot.library.unwrap().detail.unwrap();
    assert!(detail.achievement_backend);
    let summary: AchievementSummaryView = detail.achievements.unwrap();
    assert_eq!(summary.unlocked, 1);
    assert_eq!(summary.total, 1);
    assert_eq!(summary.latest_unlock.as_ref().unwrap().id, "ACH_WIN");
    assert_eq!(
        summary.latest_unlock.as_ref().unwrap().unlocked_at,
        Some(1_700_000_000)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limits_pause_the_worker_expiry_stops_it_and_cached_data_survives() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = cache(&["620"]);
    h.store.store_accounts.lock().unwrap().push(store_account());
    hold_worker(&h);
    h.achievements
        .save_ready(
            "steam",
            "A",
            "620",
            "en",
            &[record("one", true, Some(10), false, 0)],
            100,
        )
        .unwrap();
    {
        let game = h
            .achievements
            .load_game("steam", "A", "620")
            .unwrap()
            .unwrap();
        h.engine.lock().achievement_summaries.insert(
            ("steam".to_owned(), "620".to_owned()),
            gamenode_core::achievements::summarize(&game),
        );
    }
    h.engine
        .save_achievement_outcomes(
            "steam",
            "A",
            vec![models::AchievementOutcome::Failed {
                game_id: "620".into(),
                error: models::CatalogError::RateLimited,
            }],
            "en",
            200,
        )
        .await;
    {
        let queues = h.engine.shared.achievement_queues.lock().unwrap();
        let queue = queues.get("steam").unwrap();
        assert!(
            queue.paused_until.is_some(),
            "a rate limit pauses the worker"
        );
    }
    let cached = h
        .engine
        .lock()
        .achievement_summaries
        .get(&("steam".to_owned(), "620".to_owned()))
        .cloned()
        .unwrap();
    assert_eq!(cached.unlocked, 1, "cached data survives a failed read");
    assert_eq!(cached.total, 1);
    assert_eq!(
        cached.failure,
        Some(gamenode_core::library::CatalogError::RateLimited)
    );

    h.engine
        .save_achievement_outcomes(
            "steam",
            "A",
            vec![models::AchievementOutcome::Failed {
                game_id: "620".into(),
                error: models::CatalogError::Expired,
            }],
            "en",
            300,
        )
        .await;
    assert!(
        h.engine
            .shared
            .achievement_queues
            .lock()
            .unwrap()
            .get("steam")
            .unwrap()
            .stopped,
        "an expired session stops background reads"
    );
    let shown = format!("{:?}", h.engine.snapshot());
    assert!(!shown.contains("token"));
    assert!(!shown.contains("cookie"));

    h.engine.resume_achievements("steam");
    assert!(
        !h.engine
            .shared
            .achievement_queues
            .lock()
            .unwrap()
            .get("steam")
            .unwrap()
            .stopped,
        "reconnecting resumes the worker"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_and_replacement_delete_but_expiry_and_unreachability_keep_data() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    use crate::plugin_host::services::AccountPersistence;

    let h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.achievements
        .save_ready(
            "steam",
            "A",
            "620",
            "en",
            &[record("one", true, Some(10), false, 0)],
            100,
        )
        .unwrap();
    let accounts = crate::engine::providers::EngineAccounts(Arc::downgrade(&h.engine.shared));

    // Expiry and unreachability only update the account row; every save without ownership clear
    // leaves the achievement cache alone.
    let model_account = |id: &str, name: &str| models::StoreAccount {
        provider: "steam".to_owned(),
        account_id: id.to_owned(),
        account_name: Some(name.to_owned()),
        last_verified_at: Some(1),
    };
    accounts
        .save(model_account("A", "Ana"), false)
        .expect("an unchanged account saves");
    assert!(
        h.achievements
            .load_game("steam", "A", "620")
            .unwrap()
            .is_some()
    );

    // A different Steam account replaces the row: the previous account's cache is deleted first.
    accounts
        .save(model_account("B", "Bia"), true)
        .expect("a replacement account saves");
    assert!(
        h.achievements
            .load_game("steam", "A", "620")
            .unwrap()
            .is_none(),
        "replacement deletes the previous account's achievements"
    );
    {
        let mut accounts = h.store.store_accounts.lock().unwrap();
        accounts.clear();
        accounts.push(store_account());
    }

    // Explicit sign-out deletes the connected account's achievements.
    h.achievements
        .save_ready(
            "steam",
            "A",
            "620",
            "en",
            &[record("one", true, Some(10), false, 0)],
            100,
        )
        .unwrap();
    accounts
        .clear("steam")
        .expect("sign-out clears the account");
    assert!(
        h.achievements
            .load_game("steam", "A", "620")
            .unwrap()
            .is_none(),
        "sign-out deletes the account's achievements"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn achievement_batches_are_dispatched_one_at_a_time() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    let ids = (0..25)
        .map(|index| format!("{}", 1000 + index))
        .collect::<Vec<_>>();
    let refs = ids.iter().map(String::as_str).collect::<Vec<_>>();
    *h.store.cache.lock().unwrap() = cache(&refs);
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.engine.start();
    const LIMIT: usize = 25;
    let wait = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let saved = h.achievements.load_summaries("steam", "A").unwrap().len();
            if saved >= LIMIT {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if wait.is_err() {
        let saved = h.achievements.load_summaries("steam", "A").unwrap().len();
        let queue = h
            .engine
            .shared
            .achievement_queues
            .lock()
            .unwrap()
            .get("steam")
            .map(|queue| (queue.pending.len(), queue.running, queue.stopped));
        panic!("every achievement batch completes; saved={saved} queue={queue:?}");
    }
    assert_eq!(
        super::provider_runtime::ACHIEVEMENT_MAX_ACTIVE.load(Ordering::SeqCst),
        1,
        "a single provider never has two achievement batches in flight"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_with_gamenode_does_not_send_achievement_data() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = cache(&["620"]);
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.engine.start();
    h.until("the cached library", |snapshot| {
        snapshot
            .library
            .as_ref()
            .is_some_and(|library| !library.rows.is_empty())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    h.engine.set_sync(true);
    let before = (
        h.api.lookups.lock().unwrap().len(),
        h.api.covers_fetched.lock().unwrap().len(),
        *h.api.favorite_reads.lock().unwrap(),
        h.api.favorite_pushes.lock().unwrap().len(),
    );
    h.engine.request_import_achievements();
    let wait = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let saved = h.achievements.load_summaries("steam", "A").unwrap();
            if saved.iter().any(|summary| summary.entry_id == "620") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    wait.expect("the selected refresh completes");
    assert_eq!(
        (
            h.api.lookups.lock().unwrap().len(),
            h.api.covers_fetched.lock().unwrap().len(),
            *h.api.favorite_reads.lock().unwrap(),
            h.api.favorite_pushes.lock().unwrap().len(),
        ),
        before,
        "an achievement refresh never reaches the GameNode client"
    );
}

fn owned_entry(store_id: &str) -> StoredEntry {
    StoredEntry {
        installed: false,
        owned: true,
        ..entry(store_id)
    }
}

async fn wait_worker_idle(h: &Harness) {
    let wait = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let running = h
                .engine
                .shared
                .achievement_queues
                .lock()
                .unwrap()
                .get("steam")
                .is_some_and(|queue| queue.running);
            if !running {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    wait.expect("the achievement worker becomes idle");
}

#[tokio::test(flavor = "multi_thread")]
async fn repeated_library_input_on_a_failed_selected_game_queues_one_request() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = cache(&["620"]);
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.engine.start();
    h.until("the cached library", |snapshot| {
        snapshot
            .library
            .as_ref()
            .is_some_and(|library| !library.rows.is_empty())
    })
    .await;
    h.engine
        .library_input(gamenode_core::library::Input::Select {
            key: "steam:620".to_owned(),
            mode: gamenode_core::library::SelectMode::Replace,
        });
    let wait = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if h.achievements
                .load_summaries("steam", "A")
                .unwrap()
                .iter()
                .any(|summary| summary.entry_id == "620")
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    wait.expect("the initial selected refresh finishes");
    wait_worker_idle(&h).await;

    h.achievements.delete_account("steam", "A").unwrap();
    h.engine
        .lock()
        .achievement_summaries
        .remove(&("steam".to_owned(), "620".to_owned()));
    h.engine
        .save_achievement_outcomes(
            "steam",
            "A",
            vec![models::AchievementOutcome::Failed {
                game_id: "620".into(),
                error: models::CatalogError::Privacy,
            }],
            "en",
            crate::engine::unix_now() - gamenode_core::achievements::SELECTED_STALE_SECS,
        )
        .await;
    {
        let mut queues = h.engine.shared.achievement_queues.lock().unwrap();
        let queue = queues.get_mut("steam").unwrap();
        queue.pending.clear();
        queue.running = false;
        // No worker may drain the queue while the test counts requests.
        queue.stopped = true;
    }
    for _ in 0..5 {
        h.engine
            .library_input(gamenode_core::library::Input::Select {
                key: "steam:620".to_owned(),
                mode: gamenode_core::library::SelectMode::Replace,
            });
    }
    let queued = pending(&h);
    assert_eq!(
        queued
            .iter()
            .filter(|(entry_id, _)| entry_id == "620")
            .count(),
        1,
        "a failed selected game is not requested once per library input: {queued:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_execute_is_requeued_and_reads_the_same_game_after_recovery() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = cache(&["620"]);
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.engine.start();
    h.until("the cached library", |snapshot| {
        snapshot
            .library
            .as_ref()
            .is_some_and(|library| !library.rows.is_empty())
    })
    .await;
    wait_worker_idle(&h).await;

    h.achievements.delete_account("steam", "A").unwrap();
    h.engine
        .lock()
        .achievement_summaries
        .remove(&("steam".to_owned(), "620".to_owned()));
    super::provider_runtime::ACHIEVEMENT_FAIL_NEXT.store(true, Ordering::SeqCst);
    h.engine
        .refresh_selected_achievements("steam:620".to_owned());

    let wait = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let saved = h.achievements.load_summaries("steam", "A").unwrap();
            if saved.iter().any(|summary| summary.entry_id == "620") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    wait.expect("the requeued batch succeeds after the transient failure");
    assert!(
        !super::provider_runtime::ACHIEVEMENT_FAIL_NEXT.load(Ordering::SeqCst),
        "the injected failure was consumed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rate_limit_pause_still_runs_selected_work_but_holds_import_work() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    *h.store.cache.lock().unwrap() = cache(&["620", "70"]);
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.engine.start();
    h.until("the cached library", |snapshot| {
        snapshot
            .library
            .as_ref()
            .is_some_and(|library| library.rows.len() == 2)
    })
    .await;
    wait_worker_idle(&h).await;
    h.achievements.delete_account("steam", "A").unwrap();
    {
        let mut state = h.engine.lock();
        state.achievement_summaries.clear();
    }
    {
        let mut queues = h.engine.shared.achievement_queues.lock().unwrap();
        let queue = queues.get_mut("steam").unwrap();
        queue.pending.clear();
        queue.running = false;
        queue.paused_until = Some(Instant::now() + Duration::from_secs(60));
        queue.next_sequence = 1;
        queue.pending.push(crate::engine::achievements::Job {
            entry_id: "70".to_owned(),
            priority: 3,
            due: Instant::now(),
            sequence: 0,
        });
    }
    h.engine
        .refresh_selected_achievements("steam:620".to_owned());
    let wait = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let saved = h.achievements.load_summaries("steam", "A").unwrap();
            if saved.iter().any(|summary| summary.entry_id == "620") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    wait.expect("the selected game runs while the queue is paused");
    let saved = h.achievements.load_summaries("steam", "A").unwrap();
    assert!(
        saved.iter().all(|summary| summary.entry_id != "70"),
        "background import work waits out the rate-limit pause"
    );
    let queues = h.engine.shared.achievement_queues.lock().unwrap();
    let queue = queues.get("steam").unwrap();
    assert!(
        queue
            .paused_until
            .is_some_and(|until| until > Instant::now())
    );
    assert_eq!(
        queue
            .pending
            .iter()
            .filter(|job| job.entry_id == "70")
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn results_from_a_signed_out_or_replaced_account_write_nothing() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    h.achievements
        .save_ready(
            "steam",
            "A",
            "620",
            "en",
            &[record("one", true, Some(10), false, 0)],
            100,
        )
        .unwrap();
    h.store.store_accounts.lock().unwrap().clear();
    h.achievements.delete_account("steam", "A").unwrap();
    h.engine
        .save_achievement_outcomes(
            "steam",
            "A",
            vec![models::AchievementOutcome::Ready {
                game_id: "620".into(),
                achievements: vec![models::Achievement {
                    id: "late".into(),
                    name: "Late".into(),
                    description: "Late".into(),
                    hidden: false,
                    icon_url: String::new(),
                    icon_gray_url: String::new(),
                    unlocked: true,
                    unlocked_at: Some(200),
                }],
            }],
            "en",
            200,
        )
        .await;
    assert!(
        h.achievements
            .load_game("steam", "A", "620")
            .unwrap()
            .is_none(),
        "a result after sign-out is discarded"
    );
    assert!(
        h.engine.lock().achievement_summaries.is_empty(),
        "a discarded result does not update the snapshot summary"
    );

    h.store
        .store_accounts
        .lock()
        .unwrap()
        .push(gamenode_core::library::StoreAccount {
            provider: "steam".to_owned(),
            account_id: "B".to_owned(),
            account_name: Some("Bia".to_owned()),
            last_verified_at: Some(2),
        });
    h.engine
        .save_achievement_outcomes(
            "steam",
            "A",
            vec![models::AchievementOutcome::Ready {
                game_id: "70".into(),
                achievements: vec![models::Achievement {
                    id: "old".into(),
                    name: "Old".into(),
                    description: "Old".into(),
                    hidden: false,
                    icon_url: String::new(),
                    icon_gray_url: String::new(),
                    unlocked: true,
                    unlocked_at: Some(200),
                }],
            }],
            "en",
            200,
        )
        .await;
    assert!(
        h.achievements
            .load_game("steam", "A", "70")
            .unwrap()
            .is_none(),
        "a result from the replaced account is discarded"
    );
    assert!(
        h.achievements
            .load_game("steam", "B", "70")
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn clearing_an_account_keeps_the_running_worker_entry() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    hold_worker(&h);
    {
        let mut queues = h.engine.shared.achievement_queues.lock().unwrap();
        let queue = queues.get_mut("steam").unwrap();
        queue.pending.push(crate::engine::achievements::Job {
            entry_id: "620".to_owned(),
            priority: 3,
            due: Instant::now(),
            sequence: 0,
        });
        queue.running = true;
    }
    h.engine.clear_achievement_memory("steam");
    let queues = h.engine.shared.achievement_queues.lock().unwrap();
    let queue = queues.get("steam").unwrap();
    assert!(queue.running, "a second worker cannot start for this queue");
    assert!(queue.pending.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_import_reads_owned_games_that_are_not_installed() {
    let _guard = ACHIEVEMENT_TEST_LOCK.lock().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    let mut library = cache(&[]);
    library.entries.push(owned_entry("70"));
    *h.store.cache.lock().unwrap() = library;
    h.store.store_accounts.lock().unwrap().push(store_account());
    h.engine.start();
    h.until("the owned library", |snapshot| {
        snapshot
            .library
            .as_ref()
            .is_some_and(|library| library.rows.len() == 1)
    })
    .await;
    let wait = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let saved = h.achievements.load_summaries("steam", "A").unwrap();
            if saved.iter().any(|summary| summary.entry_id == "70") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    wait.expect("an owned, not-installed game is imported");
}
