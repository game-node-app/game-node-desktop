//! Engine tests for the GameNode website links (D14).

use gamenode_core::AuthView;

use crate::engine::{GameNodePage, GameNodePageError};

use super::{FakeApi, FakeCreds, harness};

/// These tests use the production website address; keep another test's environment from leaking
/// into them. The async mutex keeps the guard valid across the awaits these tests make.
static ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn lock_web_url() -> tokio::sync::MutexGuard<'static, ()> {
    let guard = ENV.lock().await;
    unsafe {
        std::env::remove_var(gamenode_api::WEB_URL_ENV);
    }
    guard
}

fn signed_in(snapshot: &gamenode_core::Snapshot) -> bool {
    matches!(snapshot.auth, AuthView::SignedIn { .. })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verified_session_opens_both_game_and_library_pages() {
    let _guard = lock_web_url().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    h.engine.start();
    h.until("signed in", signed_in).await;

    h.engine
        .open_gamenode_page(GameNodePage::Game { id: 42 })
        .await
        .expect("the game page opens");
    h.engine
        .open_gamenode_page(GameNodePage::Library)
        .await
        .expect("the library page opens");

    let opened = h.browser.opened.lock().unwrap();
    assert_eq!(
        opened.iter().map(|url| url.as_str()).collect::<Vec<_>>(),
        [
            "https://gamenode.app/game/42",
            "https://gamenode.app/library/u1"
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_library_page_needs_a_session_but_a_game_page_does_not() {
    let _guard = lock_web_url().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(None));
    h.engine.start();
    h.until("signed out", |snapshot| {
        snapshot.auth == AuthView::SignedOut
    })
    .await;

    assert_eq!(
        h.engine.open_gamenode_page(GameNodePage::Library).await,
        Err(GameNodePageError::NoSession)
    );
    h.engine
        .open_gamenode_page(GameNodePage::Game { id: 7 })
        .await
        .expect("a public game page still opens");
    assert_eq!(
        h.browser
            .opened
            .lock()
            .unwrap()
            .iter()
            .map(|url| url.as_str())
            .collect::<Vec<_>>(),
        ["https://gamenode.app/game/7"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_browser_failure_is_a_typed_error() {
    let _guard = lock_web_url().await;
    let mut h = harness(FakeApi::new(), FakeCreds::with(Some("stored")));
    h.engine.start();
    h.until("signed in", signed_in).await;
    *h.browser.fail.lock().unwrap() = true;

    assert_eq!(
        h.engine.open_gamenode_page(GameNodePage::Library).await,
        Err(GameNodePageError::Browser)
    );
}
