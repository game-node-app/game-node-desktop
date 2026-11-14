use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

fn text(title: &str, body: &str) -> Result<Text, Refused> {
    check_text("en", title, body)
}

#[test]
fn plain_text_is_trimmed_and_kept_exactly() {
    let accepted = check_text(
        "pt-BR",
        "  Backup concluído  ",
        "  Três arquivos.\nSem erros. ",
    )
    .unwrap();
    assert_eq!(accepted.locale, "pt-BR");
    assert_eq!(accepted.title, "Backup concluído");
    assert_eq!(accepted.body, "Três arquivos.\nSem erros.");
    // Markup, links and script are just characters: the host does not interpret them.
    let hostile = text(
        "<script>alert(1)</script>",
        "[click](javascript:alert(1)) <a href=\"http://evil.test\">x</a> https://evil.test",
    )
    .unwrap();
    assert_eq!(hostile.title, "<script>alert(1)</script>");
    assert!(hostile.body.contains("<a href="));
    assert!(
        text("Emoji 👨‍👩‍👧 and ZWNJ می‌خواهم", "").is_ok(),
        "ordinary joiners are text"
    );
}

#[test]
fn titles_and_bodies_are_bounded() {
    assert!(text("", "body").is_err());
    assert!(text("   ", "body").is_err());
    assert!(text(&"t".repeat(MAX_TITLE_BYTES), "").is_ok());
    assert!(text(&"t".repeat(MAX_TITLE_BYTES + 1), "").is_err());
    assert!(text("t", &"b".repeat(MAX_BODY_BYTES)).is_ok());
    assert!(text("t", &"b".repeat(MAX_BODY_BYTES + 1)).is_err());
    // Bytes, not characters.
    assert!(text(&"ã".repeat(MAX_TITLE_BYTES / 2 + 1), "").is_err());
    let lines = |n: usize| vec!["x"; n].join("\n");
    assert!(text("t", &lines(MAX_BODY_LINES)).is_ok());
    assert!(text("t", &lines(MAX_BODY_LINES + 1)).is_err());
}

#[test]
fn text_that_could_disguise_itself_is_refused() {
    for bad in [
        "line\nbreak",
        "tab\there",
        "carriage\rreturn",
        "nul\0",
        "esc\u{1b}[31m",
        "c1\u{85}x",
        "del\u{7f}",
        "rlo \u{202e}txt.exe",
        "lri \u{2066}x",
        "alm \u{061c}x",
        "lrm \u{200e}x",
        "zero\u{200b}width",
        "word\u{2060}joiner",
        "bom\u{feff}x",
        "sep\u{2028}x",
        "para\u{2029}x",
        "tag \u{e0041}x",
        "interlinear \u{fff9}x",
    ] {
        assert!(text(bad, "").is_err(), "title {bad:?}");
        assert!(
            text("ok", bad).is_err() || bad.contains('\n'),
            "body {bad:?}"
        );
    }
    // A body may have line breaks, and nothing else of that kind.
    assert!(text("ok", "one\ntwo").is_ok());
    assert!(text("ok", "one\r\ntwo").is_err());
}

#[test]
fn locales_are_language_tags() {
    for good in [
        "",
        "en",
        "pt-BR",
        "zh-Hant-TW",
        "sr-Latn-RS",
        "en-US-x-private",
    ] {
        assert!(check_text(good, "t", "").is_ok(), "{good:?}");
    }
    for bad in [
        "-en",
        "en-",
        "en--US",
        "en_US",
        "en US",
        "en;q=0.9",
        "../x",
        "é",
        &"a".repeat(MAX_LOCALE_BYTES + 1),
    ] {
        assert!(check_text(bad, "t", "").is_err(), "{bad:?}");
    }
}

#[test]
fn severities_are_four_words_and_empty_means_info() {
    assert_eq!(Severity::parse(""), Some(Severity::Info));
    assert_eq!(Severity::parse("info"), Some(Severity::Info));
    assert_eq!(Severity::parse("success"), Some(Severity::Success));
    assert_eq!(Severity::parse("warning"), Some(Severity::Warning));
    assert_eq!(Severity::parse("error"), Some(Severity::Error));
    for bad in ["INFO", "critical", "fatal", "warn", "danger", " info"] {
        assert_eq!(Severity::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn a_notice_reaches_the_application_in_a_stable_camel_case_shape() {
    let notice = PluginNotice {
        plugin_id: "org.example.plugin".into(),
        severity: Severity::Warning,
        texts: vec![Text {
            locale: "en".into(),
            title: "T".into(),
            body: "B".into(),
        }],
    };
    assert_eq!(
        serde_json::to_value(&notice).unwrap(),
        serde_json::json!({
            "pluginId": "org.example.plugin",
            "severity": "warning",
            "texts": [{ "locale": "en", "title": "T", "body": "B" }],
        })
    );
}

/// A clock the test moves by hand.
fn hub() -> (Notifications, Arc<AtomicU64>) {
    let base = Instant::now();
    let offset = Arc::new(AtomicU64::new(0));
    let moved = offset.clone();
    let hub = Notifications::default()
        .with_clock(move || base + Duration::from_millis(moved.load(Ordering::SeqCst)));
    (hub, offset)
}

#[test]
fn a_plugin_may_publish_a_few_notifications_a_minute() {
    let (hub, clock) = hub();
    for second in 0..PER_PLUGIN_PER_WINDOW as u64 {
        clock.store(second * 1000, Ordering::SeqCst);
        assert!(hub.admit([1; 32]).is_ok());
    }
    // The next one has to wait until the oldest leaves the window.
    clock.store(10_000, Ordering::SeqCst);
    assert_eq!(hub.admit([1; 32]), Err(Duration::from_secs(50)));
    // A refusal counts nothing: the wait does not grow by asking again.
    assert_eq!(hub.admit([1; 32]), Err(Duration::from_secs(50)));
    clock.store(59_999, Ordering::SeqCst);
    assert_eq!(
        hub.admit([1; 32]),
        Err(Duration::from_secs(1)),
        "never less than a second"
    );
    clock.store(60_000, Ordering::SeqCst);
    assert!(hub.admit([1; 32]).is_ok(), "the oldest has left the window");
    assert!(hub.admit([1; 32]).is_err(), "and its place is taken again");
}

#[test]
fn another_plugin_has_its_own_allowance_until_the_shared_one_is_used() {
    let (hub, _) = hub();
    for _ in 0..PER_PLUGIN_PER_WINDOW {
        hub.admit([1; 32]).unwrap();
    }
    assert!(hub.admit([1; 32]).is_err());
    for owner in 2..=(GLOBAL_PER_WINDOW / PER_PLUGIN_PER_WINDOW) as u8 {
        for _ in 0..PER_PLUGIN_PER_WINDOW {
            assert!(hub.admit([owner; 32]).is_ok(), "plugin {owner}");
        }
    }
    // Every plugin together has spoken GLOBAL_PER_WINDOW times: a plugin that has not is held too.
    let wait = hub.admit([99; 32]).unwrap_err();
    assert!(wait <= WINDOW && wait >= Duration::from_secs(1));
}

#[test]
fn the_window_forgets_idle_plugins() {
    let (hub, clock) = hub();
    hub.admit([1; 32]).unwrap();
    clock.store(WINDOW.as_millis() as u64 + 1, Ordering::SeqCst);
    hub.admit([2; 32]).unwrap();
    assert_eq!(hub.windows.lock().unwrap().per_plugin.len(), 1);
}
