//! What a plugin may put in a save profile (task 13.1), without a plugin session. The service is
//! tested over the real socket in `resources/tests/saves.rs`.

use std::collections::BTreeMap;

use super::*;

fn string(text: &str) -> Value {
    Value {
        kind: Some(Kind::StringValue(text.into())),
    }
}

fn object(fields: &[(&str, Value)]) -> Value {
    Value {
        kind: Some(Kind::StructValue(Struct {
            fields: fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect::<BTreeMap<_, _>>(),
        })),
    }
}

#[test]
fn names_are_one_line_of_plain_text() {
    assert_eq!(
        check_name("name", "  Dolphin card A ").unwrap(),
        "Dolphin card A"
    );
    for bad in [
        "",
        "   ",
        "line\nbreak",
        "bell\u{7}",
        "evil\u{202E}gnp",
        &"x".repeat(121),
    ] {
        assert_eq!(
            check_name("name", bad).unwrap_err().code(),
            tonic::Code::InvalidArgument,
            "{bad:?}"
        );
    }
    assert_eq!(check_display_name("  ").unwrap(), "");
    assert!(check_display_name("a\nb").is_err());
}

#[test]
fn runtime_and_format_ids_are_plain_identifiers() {
    assert_eq!(
        check_identifier("runtime_id", "org.dolphin-emu_5").unwrap(),
        "org.dolphin-emu_5"
    );
    assert_eq!(check_identifier("format_id", "").unwrap(), "");
    for bad in ["has space", "slash/inside", "ünï", &"x".repeat(129)] {
        assert!(check_identifier("runtime_id", bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_naming_pattern_is_a_relative_glob_that_can_exclude() {
    for good in [
        "*.sav",
        "GC/USA/*.gci",
        "**",
        "saves/**/slot?.sav",
        "!cache/**",
        "!*.tmp",
        "Mario Kart (USA).srm",
    ] {
        assert_eq!(check_pattern(good).unwrap(), good, "{good}");
    }
    for bad in [
        "",
        "!",
        "/etc/passwd",
        "C:/Users/x",
        "..",
        "a/../b",
        "./a",
        "a//b",
        "a/",
        "a\\b",
        "a**b",
        "[ab].sav",
        "{a,b}.sav",
        "tab\there",
        &"x".repeat(257),
    ] {
        assert_eq!(
            check_pattern(bad).unwrap_err().code(),
            tonic::Code::InvalidArgument,
            "{bad:?}"
        );
    }
}

#[test]
fn a_folder_takes_a_bounded_list_of_distinct_patterns() {
    let many: Vec<String> = (0..MAX_PATTERNS).map(|n| format!("*.s{n}")).collect();
    assert_eq!(check_patterns(&many).unwrap().len(), MAX_PATTERNS);
    let too_many: Vec<String> = (0..=MAX_PATTERNS).map(|n| format!("*.s{n}")).collect();
    assert!(check_patterns(&too_many).is_err());
    assert!(check_patterns(&["*.sav".into(), "*.sav".into()]).is_err());
    assert!(check_patterns(&[]).unwrap().is_empty());
}

#[test]
fn descriptive_metadata_is_bounded_and_round_trips() {
    assert_eq!(check_metadata(None).unwrap(), "{}");
    let metadata = Struct {
        fields: BTreeMap::from([
            ("region".to_owned(), string("USA")),
            (
                "core".to_owned(),
                object(&[
                    ("name", string("dolphin")),
                    (
                        "version",
                        Value {
                            kind: Some(Kind::NumberValue(5.0)),
                        },
                    ),
                ]),
            ),
        ]),
    };
    let stored = check_metadata(Some(&metadata)).unwrap();
    assert_eq!(metadata_struct(&stored).unwrap(), metadata);
    assert_eq!(metadata_struct("{}"), None);
    assert_eq!(metadata_struct("not json"), None);

    // Too deep.
    let mut nested = string("leaf");
    for _ in 0..MAX_METADATA_DEPTH + 1 {
        nested = object(&[("k", nested)]);
    }
    let Some(Kind::StructValue(deep)) = nested.kind else {
        unreachable!()
    };
    assert!(check_metadata(Some(&deep)).is_err());
    // Too many items.
    let wide = Struct {
        fields: (0..=MAX_METADATA_ITEMS)
            .map(|n| (format!("k{n}"), string("v")))
            .collect(),
    };
    assert!(check_metadata(Some(&wide)).is_err());
    // Too large, a bad key, a control character, a number that is not finite.
    let big = Struct {
        fields: BTreeMap::from([("k".to_owned(), string(&"x".repeat(MAX_METADATA_BYTES)))]),
    };
    assert!(check_metadata(Some(&big)).is_err());
    for fields in [
        BTreeMap::from([(String::new(), string("v"))]),
        BTreeMap::from([("k".to_owned(), string("a\u{7}b"))]),
        BTreeMap::from([(
            "k".to_owned(),
            Value {
                kind: Some(Kind::NumberValue(f64::NAN)),
            },
        )]),
    ] {
        assert!(check_metadata(Some(&Struct { fields })).is_err());
    }
}

#[test]
fn directories_survive_storage_and_garbage_is_no_directories() {
    let directories = vec![Directory {
        approval_id: "a1".into(),
        path: vec!["GC".into(), "USA".into()],
        display_name: "Card A".into(),
        naming_patterns: vec!["*.gci".into(), "!cache/**".into()],
    }];
    assert_eq!(
        directories_from_json(&directories_json(&directories)),
        directories
    );
    assert!(directories_from_json("garbage").is_empty());
}

struct Fixed(Vec<ForeignGrant>);

impl ProfileGrants for Fixed {
    fn grants(&self, plugin_id: &str, publisher: &str) -> Vec<ForeignGrant> {
        assert_eq!((plugin_id, publisher), ("org.reader", "pub-r"));
        self.0.clone()
    }
}

fn grant(profile: &str, operations: u8) -> ForeignGrant {
    ForeignGrant {
        creator_plugin_id: "org.maker".into(),
        creator_publisher: "pub-m".into(),
        profile_id: profile.into(),
        operations,
    }
}

#[test]
fn a_foreign_grant_covers_one_profile_and_only_the_operations_given() {
    let access = SaveAccess::default();
    // Nothing connected: nothing reachable.
    assert!(
        access
            .grant_for("org.reader", "pub-r", "p1", access::READ)
            .is_none()
    );

    access.set_grants(Arc::new(Fixed(vec![grant(
        "p1",
        access::READ | access::UPDATE,
    )])));
    assert!(
        access
            .grant_for("org.reader", "pub-r", "p1", access::READ)
            .is_some()
    );
    assert!(
        access
            .grant_for("org.reader", "pub-r", "p1", access::UPDATE)
            .is_some()
    );
    assert!(
        access
            .grant_for("org.reader", "pub-r", "p1", access::DELETE)
            .is_none()
    );
    assert!(
        access
            .grant_for("org.reader", "pub-r", "p1", access::READ | access::DELETE)
            .is_none()
    );
    assert!(
        access
            .grant_for("org.reader", "pub-r", "p2", access::READ)
            .is_none()
    );
}

#[test]
fn a_grant_naming_the_plugin_itself_is_not_a_foreign_grant() {
    struct Own;
    impl ProfileGrants for Own {
        fn grants(&self, plugin_id: &str, publisher: &str) -> Vec<ForeignGrant> {
            vec![ForeignGrant {
                creator_plugin_id: plugin_id.into(),
                creator_publisher: publisher.into(),
                profile_id: "p1".into(),
                operations: access::READ,
            }]
        }
    }
    let access = SaveAccess::default();
    access.set_grants(Arc::new(Own));
    assert!(
        access
            .grant_for("org.reader", "pub-r", "p1", access::READ)
            .is_none()
    );
}

#[test]
fn profile_ids_are_host_made_and_never_repeat() {
    let first = new_profile_id().unwrap();
    assert!(first.starts_with("profile-") && first.len() == 40);
    assert_ne!(first, new_profile_id().unwrap());
}
