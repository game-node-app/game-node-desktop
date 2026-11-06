//! Surgical INI file reading, patching, and reversion.
//!
//! Preserves all existing comments (`;`, `#`), blank lines, line endings (LF vs CRLF),
//! indentation, and unrelated sections/keys.

#![allow(clippy::collapsible_if)]

/// Reads a key's value from the specified INI section (case-insensitive for section and key).
pub fn read_ini_key(content: &str, section: &str, key: &str) -> Option<String> {
    let target_section = section.trim().to_lowercase();
    let target_key = key.trim().to_lowercase();
    let mut current_section: Option<String> = None;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let sec_name = trimmed[1..trimmed.len() - 1].trim().to_lowercase();
            current_section = Some(sec_name);
            continue;
        }

        if let Some(sec) = &current_section {
            if sec == &target_section {
                if let Some((k, v)) = trimmed.split_once('=') {
                    if k.trim().to_lowercase() == target_key {
                        let clean_val = strip_inline_comment(v.trim());
                        return Some(clean_val.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Applies a key-value patch to a section in the INI content.
///
/// If the section exists and contains the key, updates the line and returns `(new_content, Some(old_val))`.
/// If the section exists but does not contain the key, inserts it at the end of that section and returns `(new_content, None)`.
/// If the section does not exist, appends the section and key to the document and returns `(new_content, None)`.
pub fn apply_ini_patch(
    content: &str,
    section: &str,
    key: &str,
    value: &str,
) -> (String, Option<String>) {
    let newline = detect_newline(content);
    let target_section = section.trim().to_lowercase();
    let target_key = key.trim().to_lowercase();

    let (mut lines, had_trailing_newline) = split_lines_clean(content);
    let mut current_section: Option<String> = None;
    let mut target_sec_start_idx: Option<usize> = None;
    let mut target_sec_end_idx: Option<usize> = None;
    let mut found_key_idx: Option<(usize, String)> = None;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let sec_name = trimmed[1..trimmed.len() - 1].trim().to_lowercase();
            if current_section.as_deref() == Some(&target_section) {
                target_sec_end_idx = Some(idx);
            }
            if sec_name == target_section {
                target_sec_start_idx = Some(idx);
            }
            current_section = Some(sec_name);
            continue;
        }

        if current_section.as_deref() == Some(&target_section) {
            if let Some((k, v)) = trimmed.split_once('=') {
                if k.trim().to_lowercase() == target_key {
                    let old_val = strip_inline_comment(v.trim()).to_string();
                    found_key_idx = Some((idx, old_val));
                }
            }
        }
    }

    if current_section.as_deref() == Some(&target_section) && target_sec_end_idx.is_none() {
        target_sec_end_idx = Some(lines.len());
    }

    if let Some((idx, old_val)) = found_key_idx {
        let leading_spaces = leading_whitespace(&lines[idx]);
        lines[idx] = format!("{leading_spaces}{key} = {value}");
        (
            join_lines(&lines, newline, had_trailing_newline || true),
            Some(old_val),
        )
    } else if let Some(sec_start) = target_sec_start_idx {
        let insert_idx = target_sec_end_idx.unwrap_or(lines.len());
        // If there are trailing blank lines before the next section, insert before them
        let mut actual_insert = insert_idx;
        while actual_insert > sec_start + 1 && lines[actual_insert - 1].trim().is_empty() {
            actual_insert -= 1;
        }
        lines.insert(actual_insert, format!("{key} = {value}"));
        (
            join_lines(&lines, newline, had_trailing_newline || true),
            None,
        )
    } else {
        // Section not found: append new section and key
        if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        lines.push(format!("{key} = {value}"));
        (join_lines(&lines, newline, true), None)
    }
}

/// Reverts a previously applied INI patch.
///
/// If `prior_value` is `Some(old)`, restores `{key} = {old}` in place.
/// If `prior_value` is `None`, removes the key line entirely.
pub fn revert_ini_patch(
    content: &str,
    section: &str,
    key: &str,
    prior_value: Option<&str>,
) -> String {
    let newline = detect_newline(content);
    let target_section = section.trim().to_lowercase();
    let target_key = key.trim().to_lowercase();

    let (mut lines, had_trailing_newline) = split_lines_clean(content);
    let mut current_section: Option<String> = None;
    let mut found_key_idx: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let sec_name = trimmed[1..trimmed.len() - 1].trim().to_lowercase();
            current_section = Some(sec_name);
            continue;
        }

        if current_section.as_deref() == Some(&target_section) {
            if let Some((k, _)) = trimmed.split_once('=') {
                if k.trim().to_lowercase() == target_key {
                    found_key_idx = Some(idx);
                    break;
                }
            }
        }
    }

    if let Some(idx) = found_key_idx {
        match prior_value {
            Some(old) => {
                let leading_spaces = leading_whitespace(&lines[idx]);
                lines[idx] = format!("{leading_spaces}{key} = {old}");
            }
            None => {
                lines.remove(idx);
            }
        }
    }

    join_lines(&lines, newline, had_trailing_newline)
}

fn strip_inline_comment(val: &str) -> &str {
    if let Some(pos) = val.find(" ;").or_else(|| val.find("\t;")) {
        val[..pos].trim()
    } else if let Some(pos) = val.find(" #").or_else(|| val.find("\t#")) {
        val[..pos].trim()
    } else {
        val.trim()
    }
}

fn detect_newline(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn split_lines_clean(content: &str) -> (Vec<String>, bool) {
    if content.is_empty() {
        return (Vec::new(), false);
    }
    let had_trailing_newline = content.ends_with('\n');
    let mut trimmed = content;
    if let Some(s) = trimmed.strip_suffix('\n') {
        trimmed = s;
    }
    if let Some(s) = trimmed.strip_suffix('\r') {
        trimmed = s;
    }
    let lines = trimmed
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect();
    (lines, had_trailing_newline)
}

fn join_lines(lines: &[String], newline: &str, trailing_newline: bool) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut joined = lines.join(newline);
    if trailing_newline {
        joined.push_str(newline);
    }
    joined
}

fn leading_whitespace(s: &str) -> &str {
    let len = s.chars().take_while(|c| c.is_whitespace()).count();
    &s[..len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_ini_key() {
        let content = r#"; Configuration
[General]
LogLevel = info

[Plugins]
; Enable ASI plugins
LoadAsiPlugins = auto ; default setting
Path = .\plugins
"#;

        assert_eq!(
            read_ini_key(content, "General", "LogLevel").as_deref(),
            Some("info")
        );
        assert_eq!(
            read_ini_key(content, "plugins", "loadasiplugins").as_deref(),
            Some("auto")
        );
        assert_eq!(
            read_ini_key(content, "Plugins", "Path").as_deref(),
            Some(r".\plugins")
        );
        assert_eq!(read_ini_key(content, "Plugins", "Missing"), None);
        assert_eq!(read_ini_key(content, "NonExistent", "Key"), None);
    }

    #[test]
    fn test_apply_and_revert_existing_key() {
        let original = "[Plugins]\nLoadAsiPlugins = auto\n";
        let (patched, prior) = apply_ini_patch(original, "Plugins", "LoadAsiPlugins", "true");
        assert_eq!(prior.as_deref(), Some("auto"));
        assert_eq!(patched, "[Plugins]\nLoadAsiPlugins = true\n");

        // Revert back
        let reverted = revert_ini_patch(&patched, "Plugins", "LoadAsiPlugins", prior.as_deref());
        assert_eq!(reverted, original);
    }

    #[test]
    fn test_apply_and_revert_new_key_in_existing_section() {
        let original = "[General]\nDebug = false\n\n[Plugins]\nPath = .\\plugins\n";
        let (patched, prior) = apply_ini_patch(original, "Plugins", "LoadAsiPlugins", "true");
        assert_eq!(prior, None);
        assert!(patched.contains("LoadAsiPlugins = true"));

        let reverted = revert_ini_patch(&patched, "Plugins", "LoadAsiPlugins", None);
        assert_eq!(reverted, original);
    }

    #[test]
    fn test_apply_and_revert_new_section() {
        let original = "[General]\nDebug = false\n";
        let (patched, prior) = apply_ini_patch(original, "Plugins", "LoadAsiPlugins", "true");
        assert_eq!(prior, None);
        assert!(patched.contains("[Plugins]"));
        assert!(patched.contains("LoadAsiPlugins = true"));

        let reverted = revert_ini_patch(&patched, "Plugins", "LoadAsiPlugins", None);
        assert_eq!(read_ini_key(&reverted, "Plugins", "LoadAsiPlugins"), None);
    }

    #[test]
    fn test_preserves_crlf() {
        let original = "[Plugins]\r\nLoadAsiPlugins = false\r\n";
        let (patched, prior) = apply_ini_patch(original, "Plugins", "LoadAsiPlugins", "true");
        assert_eq!(prior.as_deref(), Some("false"));
        assert_eq!(patched, "[Plugins]\r\nLoadAsiPlugins = true\r\n");
    }
}
