use std::time::Duration;

use gamenode_core::SessionCredential;
use gamenode_core::game_api::{
    ApiError, Collection, CoverError, ExternalMatch, FavoriteError, GameApi, GameSummary,
    LibraryEntry, MAX_COVER_BYTES,
};
use gamenode_core::library::membership::EntryStatus;
use httpmock::HttpMockRequest;
use httpmock::prelude::*;
use serde_json::{Value, json};
use url::Url;

use crate::GameNodeClient;

fn client(server: &MockServer) -> GameNodeClient {
    GameNodeClient::with_timeout(
        Url::parse(&server.base_url()).unwrap(),
        Duration::from_secs(2),
    )
    .unwrap()
}

fn cred() -> SessionCredential {
    SessionCredential::new("tok").unwrap()
}

fn ids(range: std::ops::Range<u32>) -> Vec<String> {
    range.map(|n| format!("g{n:03}")).collect()
}

fn uids_of(request: &HttpMockRequest) -> Vec<String> {
    request
        .query_params()
        .into_iter()
        .filter(|(key, _)| key == "uids")
        .map(|(_, value)| value)
        .collect()
}

fn row(uid: &str, game_id: u64, name: &str) -> Value {
    json!({
        "id": game_id * 10,
        "uid": uid,
        "category": 1,
        "name": "store name",
        "year": 1999,
        "gameId": game_id,
        "game": {
            "id": game_id,
            "name": name,
            "firstReleaseDate": "2011-04-18T00:00:00.000Z",
            "cover": { "url": format!("//images.igdb.com/igdb/image/upload/t_thumb/c{game_id}.jpg") }
        }
    })
}

fn page(rows: Vec<Value>, has_next_page: bool) -> Value {
    json!({
        "data": rows,
        "pagination": { "totalItems": 0, "totalPages": 1, "hasNextPage": has_next_page }
    })
}

fn summary(id: u64, name: &str) -> GameSummary {
    GameSummary {
        id,
        name: name.to_owned(),
        release_year: Some(2011),
        cover_url: Some(format!(
            "//images.igdb.com/igdb/image/upload/t_thumb/c{id}.jpg"
        )),
    }
}

#[tokio::test]
async fn a_lookup_of_120_ids_is_split_into_requests_of_at_most_50() {
    let server = MockServer::start_async().await;
    // Each batch holds sorted ids, so it starts with g000, g050 or g100.
    let batches = [
        ("g000", 1, "Zero"),
        ("g050", 2, "Fifty"),
        ("g100", 3, "Hundred"),
    ];
    let mocks: Vec<_> = batches
        .iter()
        .map(|(first, game_id, name)| {
            let first = (*first).to_owned();
            server.mock(move |when, then| {
                when.method(GET)
                    .path("/v1/game/external")
                    .header("authorization", "Bearer tok")
                    .query_param("category", "1")
                    .query_param("limit", "100")
                    .query_param("offset", "0")
                    .is_true(move |request| uids_of(request).first() == Some(&first));
                then.status(200).json_body(page(
                    vec![row(batches_uid(*game_id), *game_id, name)],
                    false,
                ));
            })
        })
        .collect();
    let oversized = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/game/external")
            .is_true(|request| uids_of(request).len() > 50);
        then.status(400);
    });

    let found = client(&server)
        .lookup_external(&cred(), 1, &ids(0..120))
        .await
        .unwrap();

    for mock in &mocks {
        mock.assert_calls_async(1).await;
    }
    oversized.assert_calls_async(0).await;
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].game, summary(1, "Zero"));
}

fn batches_uid(game_id: u64) -> &'static str {
    match game_id {
        1 => "g000",
        2 => "g050",
        _ => "g100",
    }
}

#[tokio::test]
async fn pages_are_followed_while_the_server_reports_a_next_page() {
    let server = MockServer::start_async().await;
    let first = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/game/external")
            .query_param("offset", "0");
        then.status(200)
            .json_body(page(vec![row("620", 1, "Portal 2")], true));
    });
    let second = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/game/external")
            .query_param("offset", "100");
        then.status(200)
            .json_body(page(vec![row("570", 2, "Dota 2")], false));
    });

    let found = client(&server)
        .lookup_external(&cred(), 1, &["620".to_owned(), "570".to_owned()])
        .await
        .unwrap();

    first.assert_calls_async(1).await;
    second.assert_calls_async(1).await;
    let games: Vec<u64> = found.iter().map(|m| m.game.id).collect();
    assert_eq!(games, [1, 2]);
}

#[tokio::test]
async fn one_id_with_two_games_returns_both_once() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path("/v1/game/external");
        then.status(200).json_body(page(
            vec![
                row("620", 1, "Portal 2"),
                row("620", 2, "Portal 2: Remaster"),
                // The same pair on another page or a repeated row is not listed twice.
                row("620", 1, "Portal 2"),
                // An id that was not asked for is ignored.
                row("999", 9, "Stranger"),
            ],
            false,
        ));
    });

    let found = client(&server)
        .lookup_external(&cred(), 1, &["620".to_owned()])
        .await
        .unwrap();

    assert_eq!(
        found,
        [
            ExternalMatch {
                uid: "620".to_owned(),
                game: summary(1, "Portal 2")
            },
            ExternalMatch {
                uid: "620".to_owned(),
                game: summary(2, "Portal 2: Remaster")
            },
        ]
    );
}

#[tokio::test]
async fn unknown_ids_are_simply_absent_and_odd_rows_are_tolerated() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path("/v1/game/external");
        then.status(200).json_body(page(
            vec![
                // A 64-bit id as text, no game details, release year from the store row.
                json!({ "uid": "1", "gameId": "42", "year": 2003 }),
            ],
            false,
        ));
    });

    let api = client(&server);
    let found = api
        .lookup_external(&cred(), 1, &["1".to_owned(), "2".to_owned()])
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].game.id, 42);
    assert_eq!(found[0].game.release_year, Some(2003));
    assert_eq!(found[0].game.cover_url, None);

    // Nothing to ask about means no request at all.
    assert_eq!(api.lookup_external(&cred(), 1, &[]).await, Ok(vec![]));
    assert_eq!(
        api.lookup_external(&cred(), 1, &[String::new()]).await,
        Ok(vec![])
    );
}

#[tokio::test]
async fn a_401_is_a_rejected_session_and_other_failures_are_plain_failures() {
    let server = MockServer::start_async().await;
    let api = client(&server);
    let uids = ["620".to_owned()];
    for (status, expected) in [
        (401, ApiError::Rejected),
        (403, ApiError::Failed),
        (429, ApiError::Failed),
        (500, ApiError::Failed),
        (502, ApiError::Failed),
    ] {
        let mock = server.mock(|when, then| {
            when.method(GET).path("/v1/game/external");
            then.status(status);
        });
        assert_eq!(
            api.lookup_external(&cred(), 1, &uids).await,
            Err(expected),
            "{status}"
        );
        mock.delete_async().await;
    }

    // An answer that is not the expected shape is a failure too.
    server.mock(|when, then| {
        when.method(GET).path("/v1/game/external");
        then.status(200).json_body(json!({ "data": "garbage" }));
    });
    assert_eq!(
        api.lookup_external(&cred(), 1, &uids).await,
        Err(ApiError::Failed)
    );
}

#[tokio::test]
async fn one_failing_request_fails_the_whole_lookup() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET)
            .path("/v1/game/external")
            .is_true(|request| uids_of(request).first().map(String::as_str) == Some("g000"));
        then.status(200)
            .json_body(page(vec![row("g000", 1, "Zero")], false));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/v1/game/external")
            .is_true(|request| uids_of(request).first().map(String::as_str) == Some("g050"));
        then.status(500);
    });

    // The first batch succeeded, but its rows are not returned on their own.
    assert_eq!(
        client(&server)
            .lookup_external(&cred(), 1, &ids(0..60))
            .await,
        Err(ApiError::Failed)
    );
}

#[tokio::test]
async fn a_slow_server_times_out_as_a_failure() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path("/v1/game/external");
        then.status(200)
            .delay(Duration::from_secs(3))
            .json_body(page(vec![], false));
    });
    let api = GameNodeClient::with_timeout(
        Url::parse(&server.base_url()).unwrap(),
        Duration::from_millis(300),
    )
    .unwrap();
    assert_eq!(
        api.lookup_external(&cred(), 1, &["620".to_owned()]).await,
        Err(ApiError::Failed)
    );
}

#[tokio::test]
async fn a_server_that_always_has_a_next_page_is_stopped() {
    let server = MockServer::start_async().await;
    let mock = server.mock(|when, then| {
        when.method(GET).path("/v1/game/external");
        then.status(200)
            .json_body(page(vec![row("620", 1, "Portal 2")], true));
    });
    let found = client(&server)
        .lookup_external(&cred(), 1, &["620".to_owned()])
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert!(mock.calls_async().await <= 50);
}

fn search_item(id: u64, name: &str, date: Option<&str>, cover: Option<&str>) -> Value {
    let mut item = json!({
        "id": id, "name": name, "slug": name.to_lowercase(), "category": 0, "status": 0,
        "createdAt": "2020-01-01T00:00:00.000Z", "updatedAt": "2020-01-01T00:00:00.000Z",
        "source": "manticore"
    });
    if let Some(date) = date {
        item["firstReleaseDate"] = json!(date);
    }
    if let Some(cover) = cover {
        item["coverUrl"] = json!(cover);
    }
    item
}

fn search_page(items: Vec<Value>) -> Value {
    json!({ "data": { "items": items }, "pagination": { "totalItems": 0, "totalPages": 1, "hasNextPage": false } })
}

#[tokio::test]
async fn search_sends_the_query_and_returns_title_year_and_cover() {
    let server = MockServer::start_async().await;
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/search/games")
            .header("authorization", "Bearer tok")
            .json_body(json!({ "query": "portal", "limit": 20, "page": 1 }));
        then.status(200).json_body(search_page(vec![
            search_item(
                7,
                "Portal 2",
                Some("2011-04-18T00:00:00.000Z"),
                Some("//images.igdb.com/igdb/image/upload/t_thumb/c7.jpg"),
            ),
            search_item(8, "Portal", None, None),
        ]));
    });

    // Surrounding spaces are not part of the query.
    let found = client(&server)
        .search_games(&cred(), "  portal ")
        .await
        .unwrap();

    mock.assert_async().await;
    assert_eq!(found[0], summary(7, "Portal 2"));
    assert_eq!(
        found[1],
        GameSummary {
            id: 8,
            name: "Portal".to_owned(),
            release_year: None,
            cover_url: None
        }
    );
}

#[tokio::test]
async fn a_search_with_no_results_is_an_empty_list_and_a_blank_query_sends_nothing() {
    let server = MockServer::start_async().await;
    let mock = server.mock(|when, then| {
        when.method(POST).path("/v1/search/games");
        then.status(200).json_body(search_page(vec![]));
    });
    let api = client(&server);
    assert_eq!(api.search_games(&cred(), "zzzz").await, Ok(vec![]));
    mock.assert_calls_async(1).await;

    assert_eq!(api.search_games(&cred(), "   ").await, Ok(vec![]));
    mock.assert_calls_async(1).await;
}

#[tokio::test]
async fn search_failures_are_classified() {
    let server = MockServer::start_async().await;
    let api = client(&server);
    for (status, expected) in [
        (401, ApiError::Rejected),
        (400, ApiError::Failed),
        (500, ApiError::Failed),
        (503, ApiError::Failed),
    ] {
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/search/games");
            then.status(status);
        });
        assert_eq!(
            api.search_games(&cred(), "portal").await,
            Err(expected),
            "{status}"
        );
        mock.delete_async().await;
    }

    server.mock(|when, then| {
        when.method(POST).path("/v1/search/games");
        then.status(200).json_body(json!({ "unexpected": true }));
    });
    assert_eq!(
        api.search_games(&cred(), "portal").await,
        Err(ApiError::Failed)
    );
}

#[tokio::test]
async fn a_slow_search_times_out_as_a_failure() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(POST).path("/v1/search/games");
        then.status(200)
            .delay(Duration::from_secs(3))
            .json_body(search_page(vec![]));
    });
    let api = GameNodeClient::with_timeout(
        Url::parse(&server.base_url()).unwrap(),
        Duration::from_millis(300),
    )
    .unwrap();
    assert_eq!(
        api.search_games(&cred(), "portal").await,
        Err(ApiError::Failed)
    );
}

const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00,
];

fn cover_url(server: &MockServer) -> Url {
    Url::parse(&server.url("/igdb/image/upload/t_cover_big/co1wyy.jpg")).unwrap()
}

#[tokio::test]
async fn a_cover_is_downloaded_without_any_credential() {
    let server = MockServer::start();
    let image = server.mock(|when, then| {
        when.method(GET)
            .path("/igdb/image/upload/t_cover_big/co1wyy.jpg")
            .is_true(|request: &HttpMockRequest| {
                !request
                    .headers()
                    .iter()
                    .any(|(name, _)| name.as_str().eq_ignore_ascii_case("authorization"))
            });
        then.status(200).body(JPEG);
    });

    let bytes = client(&server)
        .download_image(&cover_url(&server))
        .await
        .unwrap();
    assert_eq!(bytes, JPEG);
    image.assert();
}

#[tokio::test]
async fn a_cover_over_the_cap_is_refused_whether_or_not_the_length_is_announced() {
    let server = MockServer::start();
    let mut big = JPEG.to_vec();
    big.resize(MAX_COVER_BYTES + 1, 0);
    server.mock(|when, then| {
        when.method(GET)
            .path("/igdb/image/upload/t_cover_big/co1wyy.jpg");
        then.status(200).body(big);
    });
    assert_eq!(
        client(&server).download_image(&cover_url(&server)).await,
        Err(CoverError::TooLarge)
    );

    // Exactly the cap is fine.
    let server = MockServer::start();
    let mut exact = JPEG.to_vec();
    exact.resize(MAX_COVER_BYTES, 0);
    server.mock(|when, then| {
        when.method(GET)
            .path("/igdb/image/upload/t_cover_big/co1wyy.jpg");
        then.status(200).body(exact);
    });
    let got = client(&server)
        .download_image(&cover_url(&server))
        .await
        .unwrap();
    assert_eq!(got.len(), MAX_COVER_BYTES);
}

#[tokio::test]
async fn a_cover_that_is_not_a_jpeg_or_not_a_200_is_a_failure() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/igdb/image/upload/t_cover_big/co1wyy.jpg");
        then.status(200).body("<html>captive portal</html>");
    });
    assert_eq!(
        client(&server).download_image(&cover_url(&server)).await,
        Err(CoverError::NotAnImage)
    );

    for status in [404, 500, 302] {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET)
                .path("/igdb/image/upload/t_cover_big/co1wyy.jpg");
            then.status(status)
                .header("location", "http://elsewhere.invalid/x.jpg")
                .body(JPEG);
        });
        assert_eq!(
            client(&server).download_image(&cover_url(&server)).await,
            Err(CoverError::Failed),
            "{status}"
        );
    }
}

#[tokio::test]
async fn a_cover_that_times_out_is_a_failure() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET)
            .path("/igdb/image/upload/t_cover_big/co1wyy.jpg");
        then.status(200).body(JPEG).delay(Duration::from_secs(4));
    });
    assert_eq!(
        client(&server).download_image(&cover_url(&server)).await,
        Err(CoverError::Failed)
    );
}

#[tokio::test]
async fn collections_are_listed_with_their_default_status() {
    let server = MockServer::start_async().await;
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/collections/library/user-1")
            .header("authorization", "Bearer tok");
        then.status(200).json_body(json!([
            { "id": "c1", "name": "Backlog", "isPublic": false, "defaultEntryStatus": "planned" },
            { "id": "c2", "name": "Favorites", "isPublic": true, "defaultEntryStatus": null },
            { "id": "c3", "name": "Odd", "defaultEntryStatus": "from-the-future" },
        ]));
    });

    let found = client(&server)
        .list_collections(&cred(), "user-1")
        .await
        .unwrap();

    mock.assert_async().await;
    let collection = |id: &str, name: &str, default_status| Collection {
        id: id.to_owned(),
        name: name.to_owned(),
        default_status,
    };
    assert_eq!(
        found,
        [
            collection("c1", "Backlog", Some(EntryStatus::Planned)),
            collection("c2", "Favorites", None),
            // A status this version does not know is not shown as one.
            collection("c3", "Odd", None),
        ]
    );
}

#[tokio::test]
async fn a_user_id_that_is_not_a_plain_id_is_never_sent() {
    let server = MockServer::start_async().await;
    let any = server.mock(|when, then| {
        when.any_request();
        then.status(200).json_body(json!([]));
    });
    for bad in ["", "../x", "a/b", "a?b", "a b", &"x".repeat(200)] {
        assert_eq!(
            client(&server).list_collections(&cred(), bad).await,
            Err(ApiError::Failed),
            "{bad:?}"
        );
    }
    any.assert_calls_async(0).await;
}

#[tokio::test]
async fn collection_list_errors_are_classified() {
    for (status, expected) in [
        (401, ApiError::Rejected),
        (403, ApiError::Failed),
        (500, ApiError::Failed),
    ] {
        let server = MockServer::start_async().await;
        server.mock(|when, then| {
            when.method(GET).path("/v1/collections/library/u");
            then.status(status);
        });
        assert_eq!(
            client(&server).list_collections(&cred(), "u").await,
            Err(expected),
            "{status}"
        );
    }
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path("/v1/collections/library/u");
        then.status(200).body("not json");
    });
    assert_eq!(
        client(&server).list_collections(&cred(), "u").await,
        Err(ApiError::Failed)
    );
}

#[tokio::test]
async fn an_entry_reports_its_status_and_a_404_means_absent() {
    let server = MockServer::start_async().await;
    let present = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/collections-entries/game/7")
            .header("authorization", "Bearer tok");
        then.status(200)
            .json_body(json!({ "id": "e1", "status": "finished", "isFavorite": false }));
    });
    let absent = server.mock(|when, then| {
        when.method(GET).path("/v1/collections-entries/game/8");
        then.status(404)
            .json_body(json!({ "message": "Collection entry not found." }));
    });

    let api = client(&server);
    assert_eq!(
        api.library_entry(&cred(), 7).await,
        Ok(Some(LibraryEntry {
            status: Some(EntryStatus::Finished),
            ..Default::default()
        }))
    );
    assert_eq!(api.library_entry(&cred(), 8).await, Ok(None));
    present.assert_async().await;
    absent.assert_async().await;
}

#[tokio::test]
async fn an_entry_with_an_unknown_status_is_still_an_entry() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET).path("/v1/collections-entries/game/7");
        then.status(200)
            .json_body(json!({ "status": "from-the-future" }));
    });
    assert_eq!(
        client(&server).library_entry(&cred(), 7).await,
        Ok(Some(LibraryEntry {
            status: None,
            ..Default::default()
        }))
    );
}

#[tokio::test]
async fn an_entry_check_that_cannot_be_read_is_a_failure_not_absent() {
    for (status, body, expected) in [
        (401, "", ApiError::Rejected),
        (500, "", ApiError::Failed),
        (400, "", ApiError::Failed),
        (200, "not json", ApiError::Failed),
        (200, r#"{"id":"e1"}"#, ApiError::Failed),
    ] {
        let server = MockServer::start_async().await;
        server.mock(|when, then| {
            when.method(GET).path("/v1/collections-entries/game/7");
            then.status(status).body(body);
        });
        assert_eq!(
            client(&server).library_entry(&cred(), 7).await,
            Err(expected),
            "{status} {body}"
        );
    }
}

#[tokio::test]
async fn an_entry_is_created_for_pc_and_not_finished() {
    let server = MockServer::start_async().await;
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/collections-entries")
            .header("authorization", "Bearer tok")
            .json_body(json!({
                "collectionIds": ["c1", "c2"],
                "gameId": 7,
                "platformIds": [6],
                "status": "playing",
                "finishedAt": null,
            }));
        then.status(201).json_body(json!({ "id": "e1" }));
    });

    client(&server)
        .create_entry(
            &cred(),
            7,
            EntryStatus::Playing,
            &["c1".to_owned(), "c2".to_owned()],
        )
        .await
        .unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn an_entry_can_be_created_without_collections() {
    let server = MockServer::start_async().await;
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/collections-entries")
            .json_body(json!({
                "collectionIds": [],
                "gameId": 9,
                "platformIds": [6],
                "status": "dropped",
                "finishedAt": null,
            }));
        then.status(201);
    });
    client(&server)
        .create_entry(&cred(), 9, EntryStatus::Dropped, &[])
        .await
        .unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn a_refused_create_is_an_error_and_is_not_retried() {
    for (status, expected) in [
        (401, ApiError::Rejected),
        (400, ApiError::Failed),
        (403, ApiError::Failed),
        (500, ApiError::Failed),
    ] {
        let server = MockServer::start_async().await;
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/collections-entries");
            then.status(status);
        });
        assert_eq!(
            client(&server)
                .create_entry(&cred(), 7, EntryStatus::Planned, &[])
                .await,
            Err(expected),
            "{status}"
        );
        mock.assert_calls_async(1).await;
    }
}

#[tokio::test]
async fn a_create_that_times_out_fails() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(POST).path("/v1/collections-entries");
        then.status(201).delay(Duration::from_secs(5));
    });
    assert_eq!(
        client(&server)
            .create_entry(&cred(), 7, EntryStatus::Planned, &[])
            .await,
        Err(ApiError::Failed)
    );
}

#[test]
fn statuses_use_the_wire_names() {
    for status in EntryStatus::ALL {
        assert_eq!(EntryStatus::from_api_name(status.api_name()), Some(status));
    }
    assert_eq!(EntryStatus::from_api_name("Playing"), None);
    assert_eq!(EntryStatus::ALL.len(), 5);
}

#[tokio::test]
async fn editing_preserves_platforms_and_completion_date() {
    let server = MockServer::start_async().await;
    let read = server.mock(|when, then| {
        when.method(GET).path("/v1/collections-entries/game/7");
        then.status(200).json_body(json!({
            "status": "finished", "collections": [{"id": "old"}],
            "ownedPlatforms": [{"id": 6}, {"id": 48}],
            "finishedAt": "2026-01-01T00:00:00Z"
        }));
    });
    let save = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/collections-entries")
            .header("authorization", "Bearer tok")
            .json_body(json!({
                "gameId": 7, "status": "finished", "collectionIds": ["new"],
                "platformIds": [6, 48], "finishedAt": "2026-01-01T00:00:00Z"
            }));
        then.status(201);
    });
    let api = client(&server);
    let entry = api.library_entry(&cred(), 7).await.unwrap().unwrap();
    assert_eq!(entry.collection_ids, ["old"]);
    assert_eq!(
        api.update_entry(&cred(), 7, EntryStatus::Finished, &["new".into()], &entry)
            .await,
        Ok(())
    );
    read.assert_async().await;
    save.assert_async().await;
}

#[tokio::test]
async fn edit_errors_are_reported_and_missing_platforms_send_nothing() {
    for (status, expected) in [(401, ApiError::Rejected), (500, ApiError::Failed)] {
        let server = MockServer::start_async().await;
        let save = server.mock(|when, then| {
            when.method(POST).path("/v1/collections-entries");
            then.status(status);
        });
        let api = client(&server);
        assert_eq!(
            api.update_entry(
                &cred(),
                7,
                EntryStatus::Planned,
                &[],
                &LibraryEntry::default()
            )
            .await,
            Err(ApiError::Failed)
        );
        assert_eq!(save.calls_async().await, 0);
        let entry = LibraryEntry {
            platform_ids: vec![6],
            ..Default::default()
        };
        assert_eq!(
            api.update_entry(&cred(), 7, EntryStatus::Planned, &[], &entry)
                .await,
            Err(expected)
        );
        save.assert_async().await;
    }
}

#[tokio::test]
async fn search_retains_unknown_truncated_and_complete_page_evidence() {
    for (count, metadata, complete) in [
        (1, json!({}), None),
        (1, json!({"hasNextPage":true}), Some(false)),
        (1, json!({"hasNextPage":false}), Some(true)),
        (1, json!({"total":1}), Some(true)),
        (1, json!({"total":2}), Some(false)),
        (20, json!({"hasNextPage":false,"total":20}), Some(false)),
    ] {
        let server = MockServer::start_async().await;
        let mut data = metadata;
        data["items"] = Value::Array(
            (0..count)
                .map(|id| search_item(id, "Celeste", None, None))
                .collect(),
        );
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/search/games");
            then.status(200).json_body(json!({"data":data}));
        });
        let page = client(&server)
            .search_page(&cred(), "Celeste")
            .await
            .unwrap();
        assert_eq!(page.complete, complete);
        assert_eq!(page.games.len(), count as usize);
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn favorites_are_read_across_every_page() {
    let server = MockServer::start_async().await;
    let first = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/collections-entries/library/user-1/favorites")
            .query_param("offset", "0")
            .query_param("limit", "100")
            .header("authorization", "Bearer tok");
        then.status(200).json_body(json!({
            "data": [{ "gameId": 7 }, { "gameId": "8" }],
            "pagination": { "hasNextPage": true }
        }));
    });
    let second = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/collections-entries/library/user-1/favorites")
            .query_param("offset", "100")
            .query_param("limit", "100");
        then.status(200).json_body(json!({
            "data": [{ "gameId": 9 }],
            "pagination": { "hasNextPage": false }
        }));
    });

    let found = client(&server)
        .list_favorites(&cred(), "user-1")
        .await
        .unwrap();
    assert_eq!(found, [7, 8, 9]);
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn a_failed_favorite_page_is_never_treated_as_a_complete_list() {
    let server = MockServer::start_async().await;
    server.mock(|when, then| {
        when.method(GET)
            .path("/v1/collections-entries/library/u/favorites")
            .query_param("offset", "0");
        then.status(200).json_body(json!({
            "data": [{ "gameId": 7 }],
            "pagination": { "hasNextPage": true }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/v1/collections-entries/library/u/favorites")
            .query_param("offset", "100");
        then.status(500);
    });
    assert_eq!(
        client(&server).list_favorites(&cred(), "u").await,
        Err(ApiError::Failed)
    );
}

#[tokio::test]
async fn favorite_list_errors_are_classified_and_ids_are_checked() {
    let server = MockServer::start_async().await;
    let unauthorized = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/collections-entries/library/u/favorites");
        then.status(401);
    });
    assert_eq!(
        client(&server).list_favorites(&cred(), "u").await,
        Err(ApiError::Rejected)
    );
    unauthorized.delete_async().await;

    let any = server.mock(|when, then| {
        when.any_request();
        then.status(200).json_body(json!([]));
    });
    for bad in ["", "../x", "a/b", "a?b", "a b"] {
        assert_eq!(
            client(&server).list_favorites(&cred(), bad).await,
            Err(ApiError::Failed),
            "{bad:?}"
        );
    }
    any.assert_calls_async(0).await;
}

#[tokio::test]
async fn setting_a_favorite_sends_the_flag_and_classifies_the_answer() {
    let server = MockServer::start_async().await;
    let sent = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/collections-entries/game/7/favorite")
            .header("authorization", "Bearer tok")
            .json_body(json!({ "isFavorite": true }));
        then.status(204);
    });
    client(&server)
        .set_favorite(&cred(), 7, true)
        .await
        .unwrap();
    sent.assert_async().await;
}

#[tokio::test]
async fn a_missing_entry_is_not_in_library_and_other_errors_are_transient() {
    let server = MockServer::start_async().await;
    for (status, expected) in [
        (404, FavoriteError::NotInLibrary),
        (401, FavoriteError::Rejected),
        (403, FavoriteError::Failed),
        (500, FavoriteError::Failed),
    ] {
        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/collections-entries/game/7/favorite");
            then.status(status);
        });
        assert_eq!(
            client(&server).set_favorite(&cred(), 7, false).await,
            Err(expected),
            "{status}"
        );
        mock.delete_async().await;
    }
}
