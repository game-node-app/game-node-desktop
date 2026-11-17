//! Operating-system identification and privacy-safe diagnostics text.

use std::path::PathBuf;

/// A human-readable OS name and version, read once by the caller. On Windows this is the
/// `CurrentVersion` registry key; on Linux the `/etc/os-release` pretty name. Never includes the
/// user's name or a path.
pub fn os_version() -> String {
    #[cfg(windows)]
    {
        let key = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        let product = crate::host::registry::read_string(key, "ProductName");
        let edition = crate::host::registry::read_string(key, "DisplayVersion");
        let build = crate::host::registry::read_string(key, "CurrentBuildNumber");
        let mut name = product.unwrap_or_else(|| "Windows".to_owned());
        if let Some(edition) = edition.filter(|edition| !edition.trim().is_empty()) {
            name.push(' ');
            name.push_str(edition.trim());
        }
        if let Some(build) = build.filter(|build| !build.trim().is_empty()) {
            name.push_str(&format!(" (build {})", build.trim()));
        }
        return name;
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = std::fs::read_to_string("/etc/os-release") {
            let pretty = text.lines().find_map(|line| {
                let value = line.strip_prefix("PRETTY_NAME=")?;
                Some(unquote(value.trim()))
            });
            if let Some(pretty) = pretty.filter(|name| !name.is_empty()) {
                return pretty;
            }
        }
        if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            let release = release.trim();
            if !release.is_empty() {
                return format!("Linux {release}");
            }
        }
        "Linux".to_owned()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        std::env::consts::OS.to_owned()
    }
}

/// The build target architecture, as reported by the Rust compiler.
pub fn arch() -> &'static str {
    std::env::consts::ARCH
}

/// The current user's profile directory, when the OS reports one.
pub fn user_home() -> Option<String> {
    dirs::home_dir()
        .map(PathBuf::into_os_string)
        .and_then(|home| home.into_string().ok())
}

/// Replaces every occurrence of the user's profile path with `%USERPROFILE%` on Windows or `~`
/// elsewhere. Both slash spellings are covered so a scan result such as `C:\Users\me\Games` or
/// `/home/me/Games` never reaches the clipboard as-is. Windows paths are matched without regard
/// to case or slash direction; other platforms keep case-sensitive path semantics.
pub fn redact_profile_paths(text: &str, home: Option<&str>) -> String {
    let Some(home) = home.map(str::trim).filter(|home| !home.is_empty()) else {
        return text.to_owned();
    };
    redact_profile_paths_for(text, home, cfg!(windows))
}

/// Platform-independent implementation so the Windows rules can be exercised from a Linux test.
fn redact_profile_paths_for(text: &str, home: &str, windows: bool) -> String {
    if windows {
        redact_windows_profile_paths(text, home)
    } else {
        let placeholder = "~";
        let mut variants = vec![home.to_owned()];
        let forward = home.replace('\\', "/");
        if forward != home {
            variants.push(forward);
        }
        let backward = home.replace('/', "\\");
        if backward != home {
            variants.push(backward);
        }
        variants.into_iter().fold(text.to_owned(), |text, home| {
            text.replace(&home, placeholder)
        })
    }
}

/// Windows path matching: case-insensitive (the filesystem is) and separator-insensitive (mixed
/// `\` and `/` appear in scan results). Lowercasing is done per char with a folded-to-original
/// boundary map so expansions such as `İ` never slice a UTF-8 sequence.
fn redact_windows_profile_paths(text: &str, home: &str) -> String {
    let trimmed_home = home.trim_end_matches(['\\', '/']);
    let home = if trimmed_home.is_empty() {
        home
    } else {
        trimmed_home
    };
    let mut needle = String::new();
    for ch in home.chars() {
        if ch == '\\' || ch == '/' {
            needle.push('/');
        } else {
            needle.extend(ch.to_lowercase());
        }
    }
    if needle.is_empty() {
        return text.to_owned();
    }

    let mut folded = String::new();
    // (folded byte offset, original byte offset) at every folded char boundary, plus the end.
    let mut boundaries: Vec<(usize, usize)> = Vec::new();
    for (original, ch) in text.char_indices() {
        boundaries.push((folded.len(), original));
        if ch == '\\' || ch == '/' {
            folded.push('/');
        } else {
            folded.extend(ch.to_lowercase());
        }
    }
    boundaries.push((folded.len(), text.len()));

    let mut matches = Vec::new();
    let mut index = 0;
    while index + 1 < boundaries.len() {
        let (folded_start, original_start) = boundaries[index];
        if folded[folded_start..].starts_with(&needle)
            && let Ok(offset) = boundaries[index..]
                .binary_search_by_key(&(folded_start + needle.len()), |(folded, _)| *folded)
        {
            let matched = index + offset;
            let (_, original_end) = boundaries[matched];
            // A longer path component is a different directory (`C:\Users\Cassi2`); punctuation
            // such as a closing quote or sentence period ends the profile path. A dot is treated
            // as a boundary so a sentence-ending path is still redacted.
            let followed_by_path_char = text[original_end..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-');
            if !followed_by_path_char {
                matches.push((original_start, original_end));
                index = matched;
                continue;
            }
        }
        index += 1;
    }

    let mut result = text.to_owned();
    for (start, end) in matches.into_iter().rev() {
        result.replace_range(start..end, "%USERPROFILE%");
    }
    result
}

/// Removes optional quoting from an `os-release` value.
#[cfg(target_os = "linux")]
fn unquote(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_paths_are_replaced_in_both_slash_spellings() {
        let text = "Steam at /home/me/Games/Steam and C:\\Users\\me\\Games";
        let redacted = redact_profile_paths(text, Some("/home/me"));
        assert!(!redacted.contains("/home/me"));
        assert!(redacted.contains("~/Games/Steam"));

        let windows = redact_profile_paths("C:\\Users\\me\\Games", Some("C:\\Users\\me"));
        let placeholder = if cfg!(windows) { "%USERPROFILE%" } else { "~" };
        assert!(!windows.contains("Users\\me"));
        assert!(windows.contains(&format!("{placeholder}\\Games")));

        let mixed = redact_profile_paths("C:/Users/me/Games", Some("C:\\Users\\me"));
        assert!(!mixed.contains("Users/me"));
    }

    #[test]
    fn a_missing_home_leaves_text_unchanged() {
        assert_eq!(redact_profile_paths("/tmp/game", None), "/tmp/game");
        assert_eq!(redact_profile_paths("/tmp/game", Some("")), "/tmp/game");
    }

    #[test]
    fn windows_profile_paths_ignore_case_and_separator_direction() {
        let home = r"C:\Users\Cassi";
        let text =
            "Steam at c:\\users\\cassi\\Games and mixed C:/USERS/CASSI\\Steam and C:\\Users\\Cassi";
        let redacted = redact_profile_paths_for(text, home, true);
        assert!(
            !redacted.to_lowercase().contains("cassi"),
            "the profile spelling must be gone: {redacted}"
        );
        assert_eq!(redacted.matches("%USERPROFILE%").count(), 3);
        assert!(redacted.contains("%USERPROFILE%\\Games"));
        assert!(redacted.contains("%USERPROFILE%\\Steam"));

        let sentence =
            redact_profile_paths_for("Installed in c:\\users\\cassi\\Games.", home, true);
        assert!(!sentence.to_lowercase().contains("cassi"));
    }

    #[test]
    fn windows_redaction_preserves_unrelated_paths_and_unicode() {
        let home = r"C:\Users\Cassi";
        let unrelated = redact_profile_paths_for(
            "C:\\Users\\Cassian\\Games and D:\\Users\\Cassi\\Games",
            home,
            true,
        );
        assert!(unrelated.contains("C:\\Users\\Cassian\\Games"));
        assert!(unrelated.contains("D:\\Users\\Cassi\\Games"));

        let unicode_home = r"C:\Users\José";
        let text = "C:\\USERS\\JOSÉ\\Jogos and C:\\Users\\José\\Música 🎮";
        let redacted = redact_profile_paths_for(text, unicode_home, true);
        assert!(!redacted.to_lowercase().contains("josé"));
        assert_eq!(redacted.matches("%USERPROFILE%").count(), 2);
        assert!(redacted.contains("🎮"));

        // `İ` lowercases to two code points; the folded boundary map must still match safely.
        let dotted =
            redact_profile_paths_for("C:\\Users\\İlker\\Games and 🎮", "C:\\Users\\İlker", true);
        assert!(dotted.contains("%USERPROFILE%\\Games"));
        assert!(!dotted.contains("İlker"));
        assert!(dotted.contains("🎮"));
    }

    #[test]
    fn linux_redaction_keeps_case_sensitive_paths() {
        let redacted =
            redact_profile_paths_for("/Home/Me/Games and /home/me/Games", "/home/me", false);
        assert!(redacted.contains("/Home/Me/Games"));
        assert!(redacted.contains("~/Games"));
    }

    #[test]
    fn diagnostics_never_keep_the_profile_spelling() {
        let text = concat!(
            "GameNode Desktop 0.1.0\n",
            "Build 2026-11-16\n",
            "Linux 6.8 x86_64\n",
            "Locale: pt-BR\n",
            "Detected stores:\n",
            "- Steam: 18 installed at c:\\users\\cassi\\Steam\n"
        );
        let redacted = redact_profile_paths_for(text, r"C:\Users\Cassi", true);
        let lowered = redacted.to_lowercase();
        assert!(!lowered.contains("cassi"));
        assert!(!lowered.contains("users\\cassi"));
        assert!(!lowered.contains("users/cassi"));
        assert!(redacted.contains("Locale: pt-BR"));
        assert!(redacted.contains("%USERPROFILE%\\Steam"));
    }

    #[test]
    fn the_os_name_is_available_and_has_no_paths() {
        let name = os_version();
        assert!(!name.trim().is_empty());
        assert!(!name.contains('/'));
        assert!(!name.contains('\\'));
        assert!(!arch().is_empty());
    }
}
