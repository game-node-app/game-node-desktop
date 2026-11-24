use super::*;

struct Fake {
    documents: bool,
}

impl Folders for Fake {
    fn home(&self) -> Option<PathBuf> {
        Some("/home/ana".into())
    }
    fn documents(&self) -> Option<PathBuf> {
        self.documents.then(|| "/home/ana/Documents".into())
    }
    fn data(&self) -> Option<PathBuf> {
        Some("/home/ana/.local/share".into())
    }
    fn local_data(&self) -> Option<PathBuf> {
        Some("/home/ana/AppData/Local".into())
    }
    fn config(&self) -> Option<PathBuf> {
        Some("/home/ana/.config".into())
    }
}

fn expand(location: &str, os: Os) -> Option<PathBuf> {
    parse(location)
        .unwrap()
        .expand(os, &Fake { documents: true })
}

#[test]
fn placeholders_expand_to_the_systems_folders_below_plain_names() {
    assert_eq!(
        expand("{home}/Games/Saves", Os::Linux),
        Some("/home/ana/Games/Saves".into())
    );
    assert_eq!(
        expand("{documents}", Os::Linux),
        Some("/home/ana/Documents".into())
    );
    assert_eq!(
        expand("{xdg_data}/portal", Os::Linux),
        Some("/home/ana/.local/share/portal".into())
    );
    assert_eq!(
        expand("{xdg_config}/portal/", Os::Linux),
        Some("/home/ana/.config/portal".into())
    );
    assert_eq!(
        expand("{appdata}/Portal", Os::Windows),
        Some("/home/ana/.local/share/Portal".into())
    );
    assert_eq!(
        expand("{localappdata}/Portal", Os::Windows),
        Some("/home/ana/AppData/Local/Portal".into())
    );
    // Both separators separate names, and repeated ones and `.` collapse.
    assert_eq!(
        expand("{home}\\My Games\\.\\\\Portal", Os::Linux),
        Some("/home/ana/My Games/Portal".into())
    );
}

#[test]
fn what_cannot_apply_on_this_system_expands_to_nothing_not_to_an_error() {
    // The placeholders of the other system.
    assert_eq!(expand("{appdata}/X", Os::Linux), None);
    assert_eq!(expand("{localappdata}/X", Os::Linux), None);
    assert_eq!(expand("{xdg_data}/X", Os::Windows), None);
    assert_eq!(expand("{xdg_config}/X", Os::Windows), None);
    // A system folder that does not exist.
    assert_eq!(
        parse("{documents}/X")
            .unwrap()
            .expand(Os::Linux, &Fake { documents: false }),
        None
    );
    // Absolute paths of the other system.
    assert_eq!(
        expand("C:\\Program Files (x86)\\Steam\\steamapps", Os::Linux),
        None
    );
    assert_eq!(expand("\\\\server\\share\\saves", Os::Linux), None);
    assert_eq!(expand("/opt/games", Os::Windows), None);
}

#[test]
fn absolute_paths_of_this_system_are_kept_as_written() {
    assert_eq!(
        expand("/opt/games/portal", Os::Linux),
        Some("/opt/games/portal".into())
    );
    assert_eq!(expand("/", Os::Linux), Some("/".into()));
    // Windows paths are only meaningful on Windows; the drive letter is normalised.
    #[cfg(windows)]
    {
        assert_eq!(
            expand("c:/Program Files (x86)/Steam", Os::Windows),
            Some("C:\\Program Files (x86)\\Steam".into())
        );
        assert_eq!(
            expand("\\\\nas\\games\\saves", Os::Windows),
            Some("\\\\nas\\games\\saves".into())
        );
    }
}

#[test]
fn a_location_that_is_not_well_formed_is_an_error_the_plugin_is_told_about() {
    for (text, error) in [
        ("", LocationError::Empty),
        ("{nope}/x", LocationError::UnknownPlaceholder),
        ("{Home}/x", LocationError::UnknownPlaceholder),
        ("{home", LocationError::Malformed),
        ("{home}x", LocationError::Malformed),
        ("{home}/../etc", LocationError::Malformed),
        ("{home}/a/{documents}", LocationError::Malformed),
        ("/opt/../etc", LocationError::Malformed),
        ("/opt/{home}", LocationError::Malformed),
        ("C:\\games\\..\\x", LocationError::Malformed),
        ("saves/slot1", LocationError::Relative),
        ("./saves", LocationError::Relative),
        ("~/saves", LocationError::Relative),
        ("C:saves", LocationError::Relative),
        ("\\saves", LocationError::Relative),
        ("{home}/a\u{7}b", LocationError::Control),
        ("{home}/a\nb", LocationError::Control),
    ] {
        assert_eq!(parse(text), Err(error), "{text:?}");
    }
    let long = format!("/{}", "a".repeat(MAX_LOCATION_BYTES));
    assert_eq!(parse(&long), Err(LocationError::TooLong));
    // Exactly at the limit is fine.
    assert!(parse(&format!("/{}", "a".repeat(MAX_LOCATION_BYTES - 1))).is_ok());
}

#[test]
fn the_system_folders_are_the_real_ones() {
    assert_eq!(SystemFolders.home(), dirs::home_dir());
}
