use super::*;

#[test]
fn only_exact_https_origins_count_and_they_are_normalized() {
    for (text, expected) in [
        ("https://api.example.com", Some("https://api.example.com")),
        ("https://API.Example.com/", Some("https://api.example.com")),
        (
            "https://api.example.com:443",
            Some("https://api.example.com"),
        ),
        (
            "https://api.example.com:8443/",
            Some("https://api.example.com:8443"),
        ),
        (
            "https://bücher.example",
            Some("https://xn--bcher-kva.example"),
        ),
        ("https://api.example.com/v1", None),
        ("https://api.example.com/?x=1", None),
        ("https://api.example.com/#f", None),
        ("https://user@api.example.com", None),
        ("http://api.example.com", None),
        ("api.example.com", None),
        ("*.example.com", None),
        ("https://*.example.com", None),
        ("", None),
    ] {
        assert_eq!(canonical_origin(text).as_deref(), expected, "{text:?}");
    }
}

#[test]
fn a_plugin_has_a_small_allowance_and_so_have_all_plugins() {
    let access = NetworkAccess::default();
    let mut held = Vec::new();
    for _ in 0..PER_PLUGIN_FETCHES {
        held.push(access.permits([1; 32]).unwrap());
    }
    assert_eq!(
        access.permits([1; 32]).err().map(|s| s.code()),
        Some(tonic::Code::ResourceExhausted)
    );
    // Another plugin is unaffected, until the global allowance is used up.
    for owner in 2..=(GLOBAL_FETCHES / PER_PLUGIN_FETCHES) as u8 {
        for _ in 0..PER_PLUGIN_FETCHES {
            held.push(access.permits([owner; 32]).unwrap());
        }
    }
    assert_eq!(held.len(), GLOBAL_FETCHES);
    assert_eq!(
        access.permits([99; 32]).err().map(|s| s.code()),
        Some(tonic::Code::ResourceExhausted)
    );
    // Finishing a fetch frees both slots.
    held.pop();
    assert!(access.permits([99; 32]).is_ok());
}
