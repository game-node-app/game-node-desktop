//! `NotificationsPublish` over the real local socket (task 6.4).

use std::sync::Mutex as StdMutex;
use std::sync::atomic::Ordering;

use gamenode_plugin_protocol::prost::Message;

use super::*;
use crate::plugin_host::notifications::{
    GLOBAL_PER_WINDOW, NotificationSink, PER_PLUGIN_PER_WINDOW, PluginNotice, Severity, WINDOW,
};

const NOTICES: &[&str] = &[cap::NOTIFICATIONS_PUBLISH];

/// What the application would be told to show.
#[derive(Default)]
pub(super) struct Collected(StdMutex<Vec<PluginNotice>>);

impl NotificationSink for Collected {
    fn deliver(&self, notice: PluginNotice) {
        self.0.lock().unwrap().push(notice);
    }
}

impl Collected {
    pub(super) fn all(&self) -> Vec<PluginNotice> {
        self.0.lock().unwrap().clone()
    }
}

impl Fixture {
    async fn notify(
        &self,
        idempotency: &str,
        title: &str,
        body: &str,
        severity: &str,
    ) -> Result<MutationReply, Status> {
        self.notify_full(idempotency, title, body, "", severity, vec![])
            .await
    }

    pub(super) async fn notify_full(
        &self,
        idempotency: &str,
        title: &str,
        body: &str,
        locale: &str,
        severity: &str,
        translations: Vec<(&str, &str, &str)>,
    ) -> Result<MutationReply, Status> {
        self.client()
            .notifications_publish(NotificationPublishRequest {
                mutation: Some(self.mutation(idempotency, 0)),
                title: title.into(),
                body: body.into(),
                locale: locale.into(),
                severity: severity.into(),
                translations: translations
                    .into_iter()
                    .map(|(locale, title, body)| NotificationTranslation {
                        locale: locale.into(),
                        title: title.into(),
                        body: body.into(),
                    })
                    .collect(),
            })
            .await
            .map(Response::into_inner)
    }

    pub(super) fn advance_notice_clock(&self, millis: u64) {
        self.notice_clock.fetch_add(millis, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn a_notification_is_delivered_attributed_to_the_authenticated_plugin() {
    let fixture = Fixture::new(NOTICES).await;
    let reply = fixture
        .notify_full(
            "n1",
            "  Backup finished ",
            "3 files saved.\nNo errors.",
            "en",
            "success",
            vec![
                ("pt-BR", "Backup concluído", "3 arquivos salvos."),
                ("de", "Sicherung fertig", ""),
            ],
        )
        .await
        .unwrap();
    assert_eq!(reply.outcome, MutationOutcome::Applied as i32);

    let [notice] = fixture.notices.all().try_into().unwrap();
    // The attribution is the authenticated plugin id; the request has no field for it.
    assert_eq!(notice.plugin_id, "fixture.plugin");
    assert_eq!(notice.severity, Severity::Success);
    let texts: Vec<_> = notice
        .texts
        .iter()
        .map(|text| {
            (
                text.locale.as_str(),
                text.title.as_str(),
                text.body.as_str(),
            )
        })
        .collect();
    assert_eq!(
        texts,
        [
            ("en", "Backup finished", "3 files saved.\nNo errors."),
            ("pt-BR", "Backup concluído", "3 arquivos salvos."),
            ("de", "Sicherung fertig", ""),
        ]
    );
}

#[tokio::test]
async fn severity_defaults_to_info_and_unknown_words_are_refused() {
    let fixture = Fixture::new(NOTICES).await;
    fixture.notify("a", "hello", "", "").await.unwrap();
    for (index, word) in ["info", "success", "warning", "error"]
        .into_iter()
        .enumerate()
    {
        fixture
            .notify(&format!("s{index}"), "hello", &index.to_string(), word)
            .await
            .unwrap();
    }
    let severities: Vec<_> = fixture.notices.all().iter().map(|n| n.severity).collect();
    assert_eq!(
        severities,
        [
            Severity::Info,
            Severity::Info,
            Severity::Success,
            Severity::Warning,
            Severity::Error
        ]
    );
    for bad in ["INFO", "critical", "danger", "warn"] {
        let status = fixture.notify("bad", "hello", "x", bad).await.unwrap_err();
        assert_eq!(status.code(), Code::InvalidArgument, "{bad}");
    }
}

#[tokio::test]
async fn the_grant_is_needed_and_a_revoked_one_stops_it_without_using_the_allowance() {
    let none = Fixture::new(&[]).await;
    let status = none.notify("k", "hello", "", "").await.unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(
        DenialDetail::decode(status.details())
            .unwrap()
            .permission_id,
        cap::NOTIFICATIONS_PUBLISH
    );
    assert!(none.notices.all().is_empty());

    let fixture = Fixture::new(NOTICES).await;
    fixture.notify("first", "hello", "", "").await.unwrap();
    fixture
        .api
        .revoke_session_permission("fixture", cap::NOTIFICATIONS_PUBLISH)
        .await;
    for index in 0..10 {
        let status = fixture
            .notify(&format!("late-{index}"), "hello", "", "")
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::PermissionDenied);
    }
    assert_eq!(fixture.notices.all().len(), 1);
}

#[tokio::test]
async fn bad_text_is_refused_and_nothing_is_shown() {
    let fixture = Fixture::new(NOTICES).await;
    let long_title = "t".repeat(121);
    let long_body = "b".repeat(513);
    let many_lines = ["x"; 9].join("\n");
    let cases: Vec<(&str, &str, &str)> = vec![
        ("", "body", "en"),
        ("   ", "", "en"),
        (&long_title, "", "en"),
        ("two\nlines", "", "en"),
        ("tab\there", "", "en"),
        ("esc\u{1b}[31mred", "", "en"),
        ("exe \u{202e}fdp.exe", "", "en"),
        ("zero\u{200b}width", "", "en"),
        ("ok", &long_body, "en"),
        ("ok", &many_lines, "en"),
        ("ok", "bell\u{7}", "en"),
        ("ok", "rlo \u{202e}x", "en"),
        ("ok", "", "en_US"),
        ("ok", "", "../x"),
    ];
    for (title, body, locale) in cases {
        let status = fixture
            .notify_full("k", title, body, locale, "", vec![])
            .await
            .unwrap_err();
        assert_eq!(
            status.code(),
            Code::InvalidArgument,
            "{title:?} {body:?} {locale:?}"
        );
    }
    assert!(fixture.notices.all().is_empty());
}

#[tokio::test]
async fn translations_are_bounded_checked_and_distinct() {
    let fixture = Fixture::new(NOTICES).await;
    let nine: Vec<(String, String, String)> = (0..9)
        .map(|i| (format!("l{i}"), "t".into(), String::new()))
        .collect();
    let nine: Vec<(&str, &str, &str)> = nine
        .iter()
        .map(|(l, t, b)| (l.as_str(), t.as_str(), b.as_str()))
        .collect();
    let eight = nine[..8].to_vec();
    assert_eq!(
        fixture
            .notify_full("n", "t", "", "en", "", nine)
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    fixture
        .notify_full("ok", "t", "", "en", "", eight)
        .await
        .unwrap();

    for translations in [
        vec![("", "t", "")],
        vec![("fr", "", "")],
        vec![("fr", "t", ""), ("FR", "t2", "")],
        vec![("en", "t", "")],
        vec![("EN", "t", "")],
        vec![("fr", "line\nbreak", "")],
        vec![("fr", "t", "x\u{202e}y")],
        vec![("fr_FR", "t", "")],
    ] {
        let status = fixture
            .notify_full("bad", "t", "", "en", "", translations.clone())
            .await
            .unwrap_err();
        assert_eq!(status.code(), Code::InvalidArgument, "{translations:?}");
    }
    assert_eq!(
        fixture.notices.all().len(),
        1,
        "only the valid one was shown"
    );
}

#[tokio::test]
async fn markup_links_and_script_are_passed_on_untouched_as_text() {
    let fixture = Fixture::new(NOTICES).await;
    let title = "<script>alert(1)</script> <img src=x onerror=alert(2)>";
    let body =
        "[click](javascript:alert(3)) <a href=\"https://evil.test\">pay</a> https://evil.test/x";
    fixture.notify("k", title, body, "").await.unwrap();
    let [notice] = fixture.notices.all().try_into().unwrap();
    // The host never interprets the text; showing it as plain text is the application's side.
    assert_eq!(
        (
            notice.texts[0].title.as_str(),
            notice.texts[0].body.as_str()
        ),
        (title, body)
    );
}

#[tokio::test]
async fn a_plugin_that_notifies_too_often_is_told_when_to_retry_and_may_then() {
    let fixture = Fixture::new(NOTICES).await;
    for index in 0..PER_PLUGIN_PER_WINDOW {
        fixture.advance_notice_clock(1000);
        fixture
            .notify(&format!("n{index}"), "hello", &index.to_string(), "")
            .await
            .unwrap();
    }
    assert_eq!(fixture.notices.all().len(), PER_PLUGIN_PER_WINDOW);

    let status = fixture
        .notify("over", "hello", "over", "")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::ResourceExhausted);
    let retry: u64 = status
        .metadata()
        .get("retry-after-ms")
        .expect("the wait is in the metadata")
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        retry,
        WINDOW.as_millis() as u64 - 4000,
        "until the oldest leaves the window"
    );
    assert!(
        status.message().contains("retry after 56 seconds"),
        "{}",
        status.message()
    );
    assert_eq!(
        fixture.notices.all().len(),
        PER_PLUGIN_PER_WINDOW,
        "nothing more was shown"
    );

    // Waiting as told makes room; the refused call was not remembered, so its key is usable.
    fixture.advance_notice_clock(retry);
    fixture.notify("over", "hello", "over", "").await.unwrap();
    assert_eq!(fixture.notices.all().len(), PER_PLUGIN_PER_WINDOW + 1);
}

#[tokio::test]
async fn a_retried_notification_is_shown_once_and_a_reused_key_is_refused() {
    let fixture = Fixture::new(NOTICES).await;
    let first = fixture
        .notify("same", "hello", "body", "info")
        .await
        .unwrap();
    let retry = fixture
        .notify("same", "hello", "body", "info")
        .await
        .unwrap();
    assert_eq!(first, retry);
    assert_eq!(fixture.notices.all().len(), 1);
    // Retries do not use the allowance either.
    for _ in 0..20 {
        fixture
            .notify("same", "hello", "body", "info")
            .await
            .unwrap();
    }
    fixture.notify("fresh", "another", "", "").await.unwrap();

    let status = fixture
        .notify("same", "different", "body", "info")
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert_eq!(fixture.notices.all().len(), 2);
}

#[tokio::test]
async fn publishing_is_unavailable_without_a_sink_and_revisions_are_not_used() {
    let fixture = Fixture::new(NOTICES).await;
    let status = fixture
        .client()
        .notifications_publish(NotificationPublishRequest {
            mutation: Some(fixture.mutation("k", 4)),
            title: "hello".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
    let status = fixture
        .client()
        .notifications_publish(NotificationPublishRequest {
            mutation: Some(MutationContext {
                idempotency_key: String::new(),
                ..fixture.mutation("k", 0)
            }),
            title: "hello".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(
        status.code(),
        Code::InvalidArgument,
        "an idempotency key is required"
    );

    // A host with nothing connected cannot show anything.
    let service = ResourceApi::new(
        fixture.api.clone(),
        fixture.feed.clone(),
        fixture.storage.clone(),
        fixture.files.clone(),
        fixture.network.clone(),
        Arc::new(Notifications::default()),
        Jobs::default(),
        Arc::default(),
        Arc::default(),
        Arc::default(),
        Arc::default(),
        None,
    );
    let status = service
        .notifications_publish(Request::new(NotificationPublishRequest {
            mutation: Some(fixture.mutation("k", 0)),
            title: "hello".into(),
            ..Default::default()
        }))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
}

#[test]
fn the_limits_are_the_documented_ones() {
    assert_eq!((PER_PLUGIN_PER_WINDOW, GLOBAL_PER_WINDOW), (5, 20));
    assert_eq!(WINDOW, Duration::from_secs(60));
}

#[tokio::test]
async fn the_service_is_described() {
    let fixture = Fixture::new(NOTICES).await;
    let described = fixture.host.describe_api().await.unwrap();
    assert!(described.methods.iter().any(|method| {
        method.method == "NotificationsPublish"
            && method
                .permissions
                .iter()
                .any(|p| p == cap::NOTIFICATIONS_PUBLISH)
    }));
}
