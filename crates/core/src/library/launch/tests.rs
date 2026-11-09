use super::*;
use crate::library::machine::tests::{providers, record};
use crate::provider::Platform;
use crate::provider::registry;
use std::sync::Mutex;
use url::Url;

#[derive(Default)]
struct FakeOpener {
    opened: Mutex<Vec<String>>,
    outcome: Mutex<Option<UriError>>,
}

impl UriOpener for FakeOpener {
    fn open(&self, uri: &Url) -> Result<(), UriError> {
        if let Some(error) = *self.outcome.lock().unwrap() {
            return Err(error);
        }
        self.opened.lock().unwrap().push(uri.as_str().to_owned());
        Ok(())
    }
}

fn steam() -> Provider {
    providers(false).into_iter().next().unwrap()
}

#[test]
fn play_opens_the_launch_uri_and_install_the_install_uri() {
    let opener = FakeOpener::default();
    let portal = record("620", "Portal 2", "/g/Portal 2");
    open_action(&opener, &steam(), &portal, LaunchKind::Play).unwrap();
    open_action(&opener, &steam(), &portal, LaunchKind::Install).unwrap();
    assert_eq!(
        *opener.opened.lock().unwrap(),
        ["steam://rungameid/620", "steam://nav/games/details/620"]
    );
}

#[test]
fn ids_are_encoded_so_they_cannot_change_the_uri() {
    let opener = FakeOpener::default();
    let hostile = record("620/../x?y=1#z", "Hostile", "/g");
    // Either refused or encoded; never opened with extra structure.
    let _ = open_action(&opener, &steam(), &hostile, LaunchKind::Play);
    for uri in opener.opened.lock().unwrap().iter() {
        let url = Url::parse(uri).unwrap();
        assert_eq!(url.scheme(), "steam");
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
    }
}

#[test]
fn a_missing_handler_is_reported_as_such() {
    let opener = FakeOpener::default();
    *opener.outcome.lock().unwrap() = Some(UriError::NoHandler);
    let portal = record("620", "Portal 2", "/g");
    assert_eq!(
        open_action(&opener, &steam(), &portal, LaunchKind::Play),
        Err(LaunchError::NoHandler)
    );
    *opener.outcome.lock().unwrap() = Some(UriError::Failed);
    assert_eq!(
        open_action(&opener, &steam(), &portal, LaunchKind::Install),
        Err(LaunchError::Failed)
    );
}

#[test]
fn a_provider_without_the_action_opens_nothing() {
    let manifest = r#"
id = "plain"
name = "Plain"
platforms = ["windows", "linux"]
source = "plain"
scan = { native = "manual" }
"#;
    let loaded = registry::load(&[("plain.toml", manifest)], Platform::Linux);
    let plain = loaded.providers.into_iter().next().unwrap();
    let opener = FakeOpener::default();
    let portal = record("620", "Portal 2", "/g");
    for kind in [LaunchKind::Play, LaunchKind::Install] {
        assert_eq!(
            open_action(&opener, &plain, &portal, kind),
            Err(LaunchError::Unsupported)
        );
    }
    assert!(opener.opened.lock().unwrap().is_empty());
}

#[test]
fn a_record_without_the_needed_field_opens_nothing() {
    let manifest = r#"
id = "custom"
name = "Custom"
platforms = ["windows", "linux"]
source = "custom"
scan = { native = "manual" }
launch = { uri = "custom://run/{slug}" }
"#;
    let loaded = registry::load(&[("custom.toml", manifest)], Platform::Linux);
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    let custom = loaded.providers.into_iter().next().unwrap();
    let opener = FakeOpener::default();
    assert_eq!(
        open_action(
            &opener,
            &custom,
            &record("1", "One", "/g"),
            LaunchKind::Play
        ),
        Err(LaunchError::Incomplete)
    );
    assert!(opener.opened.lock().unwrap().is_empty());
}

#[test]
fn manual_launch_requires_approved_available_registration_and_never_installs() {
    use crate::drm_free::{Evidence, InstallationHost, LocalError, Registration};
    struct Host(Mutex<usize>);
    impl InstallationHost for Host {
        fn inspect(&self, _: &str) -> Result<Registration, LocalError> {
            unreachable!()
        }
        fn evidence(&self, _: &Registration) -> Result<Vec<Evidence>, LocalError> {
            unreachable!()
        }
        fn launch(&self, _: &Registration) -> Result<(), LocalError> {
            *self.0.lock().unwrap() += 1;
            Ok(())
        }
    }
    let providers = registry::load(
        &[(
            "manual.toml",
            include_str!("../../../../../providers/manual.toml"),
        )],
        Platform::Windows,
    );
    let manual = providers.get("manual").unwrap();
    let mut registration = Registration {
        id: "uuid".into(),
        title: "Game".into(),
        executable: "C:/game/game.exe".into(),
        file_identity: "file".into(),
        root: "C:/game".into(),
        working_directory: "C:/game".into(),
        arguments: vec!["&literal".into()],
        gameplay: vec![],
        launchers: vec![],
        revision: 1,
        removed: false,
        rediscovery_allowed: false,
        transferred_from: None,
        available: true,
    };
    let record = crate::drm_free::records(std::slice::from_ref(&registration)).remove(0);
    let host = Host(Mutex::new(0));
    assert_eq!(
        launch_manual(&host, manual, &record, &registration, LaunchKind::Play),
        Ok(())
    );
    assert_eq!(
        launch_manual(&host, manual, &record, &registration, LaunchKind::Install),
        Err(LaunchError::Unsupported)
    );
    assert_eq!(
        launch_manual(&host, &steam(), &record, &registration, LaunchKind::Play),
        Err(LaunchError::Unsupported)
    );
    registration.available = false;
    assert_eq!(
        launch_manual(&host, manual, &record, &registration, LaunchKind::Play),
        Err(LaunchError::Incomplete)
    );
    assert_eq!(*host.0.lock().unwrap(), 1);
    assert!(!manual.can_install(&record));
}

#[test]
fn a_port_launch_passes_its_provenance_environment_to_the_host() {
    use crate::drm_free::{
        Evidence, InstallationHost, LaunchEnvironment, LocalError, Registration,
    };
    struct Host(Mutex<Option<LaunchEnvironment>>);
    impl InstallationHost for Host {
        fn inspect(&self, _: &str) -> Result<Registration, LocalError> {
            unreachable!()
        }
        fn evidence(&self, _: &Registration) -> Result<Vec<Evidence>, LocalError> {
            unreachable!()
        }
        fn launch(&self, _: &Registration) -> Result<(), LocalError> {
            unreachable!("an environment-aware launch must not take the plain path")
        }
        fn launch_with_environment(
            &self,
            _: &Registration,
            environment: &LaunchEnvironment,
        ) -> Result<(), LocalError> {
            *self.0.lock().unwrap() = Some(environment.clone());
            Ok(())
        }
    }

    let providers = registry::load(
        &[(
            "manual.toml",
            include_str!("../../../../../providers/manual.toml"),
        )],
        Platform::Windows,
    );
    let manual = providers.get("manual").unwrap();
    let registration = Registration {
        id: "uuid".into(),
        title: "Game".into(),
        executable: "C:/game/game.exe".into(),
        file_identity: "file".into(),
        root: "C:/game".into(),
        working_directory: "C:/game".into(),
        arguments: vec!["--iso".into(), "D:/dump.iso".into()],
        gameplay: vec![],
        launchers: vec![],
        revision: 1,
        removed: false,
        rediscovery_allowed: false,
        transferred_from: None,
        available: true,
    };
    let record = crate::drm_free::records(std::slice::from_ref(&registration)).remove(0);
    let environment =
        LaunchEnvironment::from([("BLOODBORNE_PATH".to_string(), "D:/dump.iso".to_string())]);
    let host = Host(Mutex::new(None));

    assert_eq!(
        launch_manual_with_environment(
            &host,
            manual,
            &record,
            &registration,
            &environment,
            LaunchKind::Play
        ),
        Ok(())
    );
    assert_eq!(host.0.lock().unwrap().as_ref(), Some(&environment));
}
