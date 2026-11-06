//! String path helpers that work for both Windows- and Unix-style host paths on any OS.
//!
//! Scanners handle paths as text in the host's own style, so a fixture with `C:\Games` behaves
//! the same on a Linux CI machine as on Windows.

/// Whether `path` looks like a Windows path: a drive letter (`C:`) or a UNC prefix.
pub fn is_windows_style(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || path.starts_with("\\\\")
}

/// Normalizes separators and drops trailing ones (`C:/Steam/` becomes `C:\Steam`, `/a//b/` becomes
/// `/a/b`). A root (`/` or `C:\`) keeps its separator.
/// Windows verbatim drive and UNC paths compare like their ordinary forms.
pub fn normalize(path: &str) -> String {
    // canonicalize() can persist verbatim paths while Windows process queries
    // return ordinary paths. Normalize both sides without touching the disk.
    if let Some(rest) = path.strip_prefix(r"\\?\") {
        if let Some(unc) = rest.strip_prefix(r"UNC\") {
            return normalize(&format!(r"\\{unc}"));
        }
        let bytes = rest.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/')
        {
            return normalize(rest);
        }
    }
    if is_windows_style(path) {
        let unc = path.starts_with("\\\\") || path.starts_with("//");
        let body: Vec<&str> = path
            .split(['\\', '/'])
            .filter(|part| !part.is_empty())
            .collect();
        let mut out = if unc {
            format!("\\\\{}", body.join("\\"))
        } else {
            body.join("\\")
        };
        if !unc && body.len() == 1 && body[0].ends_with(':') {
            out.push('\\');
        }
        out
    } else {
        let absolute = path.starts_with('/');
        let body: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        match (absolute, body.is_empty()) {
            (true, true) => "/".to_owned(),
            (true, false) => format!("/{}", body.join("/")),
            (false, _) => body.join("/"),
        }
    }
}

/// Joins `child` onto `base` using `base`'s separator.
pub fn join(base: &str, child: &str) -> String {
    let separator = if is_windows_style(base) { '\\' } else { '/' };
    let base = normalize(base);
    let child = child.trim_matches(['\\', '/']);
    if child.is_empty() {
        return base;
    }
    if base.ends_with(separator) {
        normalize(&format!("{base}{child}"))
    } else {
        normalize(&format!("{base}{separator}{child}"))
    }
}

/// Whether `name` is a single path component: no separators, no `.`/`..`, no NUL.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && !name.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_both_styles() {
        let cases = [
            (
                "C:/Program Files (x86)/Steam",
                "C:\\Program Files (x86)\\Steam",
            ),
            ("D:\\Games\\", "D:\\Games"),
            ("C:\\", "C:\\"),
            ("C:", "C:\\"),
            ("\\\\nas\\games\\steam", "\\\\nas\\games\\steam"),
            ("/home/me//.steam/", "/home/me/.steam"),
            ("/", "/"),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize(input), expected, "{input}");
        }
    }

    #[test]
    fn joins_with_the_base_separator() {
        assert_eq!(join("C:/Steam", "steamapps"), "C:\\Steam\\steamapps");
        assert_eq!(join("D:\\", "steamapps"), "D:\\steamapps");
        assert_eq!(
            join("/home/me/.steam", "steamapps"),
            "/home/me/.steam/steamapps"
        );
        assert_eq!(join("/", "etc"), "/etc");
        assert_eq!(join("/a", ""), "/a");
    }

    #[test]
    fn plain_names_reject_traversal() {
        for good in ["Half-Life 2", "Portal", "a.b"] {
            assert!(is_plain_name(good), "{good}");
        }
        for bad in ["", ".", "..", "a/b", "a\\b", "C:", "../x", "a\0b"] {
            assert!(!is_plain_name(bad), "{bad:?}");
        }
    }
}

#[cfg(test)]
mod verbatim_tests {
    use super::normalize;

    #[test]
    fn verbatim_drive_and_unc_paths_match_ordinary_paths() {
        for (extended, ordinary) in [
            (r"\\?\F:\Games\Stellar Blade", r"F:\Games\Stellar Blade"),
            (r"\\?\C:\", r"C:\"),
            (r"\\?\UNC\server\games\", r"\\server\games"),
        ] {
            assert_eq!(normalize(extended), normalize(ordinary));
            assert_eq!(normalize(&normalize(extended)), normalize(ordinary));
        }
    }
}
