//! Target executable resolution and anti-cheat folder inspection.

use gamenode_core::tools::{TargetCandidate, TargetInspection, ToolError, evaluate_anticheat};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Strips Windows verbatim prefix (`\\?\` or `\\?\UNC\`) from a canonical path.
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{unc}"))
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

/// Checks if `path` resides within `base` directory, handling case-insensitive Windows comparisons.
pub fn path_starts_with(path: &Path, base: &Path) -> bool {
    #[cfg(windows)]
    {
        let p = path.to_string_lossy().to_lowercase();
        let b = base.to_string_lossy().to_lowercase();
        let b_trimmed = b.trim_end_matches(['/', '\\']);
        if p == b_trimmed {
            return true;
        }
        if p.starts_with(b_trimmed) {
            let remainder = &p[b_trimmed.len()..];
            return remainder.starts_with('/') || remainder.starts_with('\\');
        }
        false
    }
    #[cfg(not(windows))]
    {
        path.starts_with(base)
    }
}

/// Computes the relative path from `base` to `path` with forward slashes.
pub fn relative_path_from(path: &Path, base: &Path) -> Option<String> {
    if !path_starts_with(path, base) {
        return None;
    }
    if let Ok(rel) = path.strip_prefix(base) {
        let s = rel.to_string_lossy().replace('\\', "/");
        let trimmed = s.trim_start_matches('/').to_string();
        return Some(trimmed);
    }
    #[cfg(windows)]
    {
        let p_str = path.to_string_lossy();
        let b_str = base.to_string_lossy();
        let b_len = b_str.trim_end_matches(['/', '\\']).len();
        if let Some(rel_str) = p_str.get(b_len..) {
            let trimmed = rel_str.trim_start_matches(['/', '\\']).replace('\\', "/");
            return Some(trimmed);
        }
    }
    None
}

/// Identifies maintenance or redistributable binaries that should not be candidate targets.
pub fn is_maintenance_binary(file_name: &str) -> bool {
    if gamenode_core::drm_free::identity::maintenance(file_name) {
        return true;
    }
    let lower = file_name.to_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    lower.starts_with("vc_redist")
        || lower.starts_with("vcredist")
        || lower.contains("redist")
        || lower.starts_with("crashhandler")
        || lower.starts_with("unitycrashhandler")
}

/// Checks whether an executable path is a primary shipping or gameplay binary.
pub fn is_shipping_binary(rel_path: &str) -> bool {
    let lower = rel_path.to_lowercase();
    let filename = lower.rsplit('/').next().unwrap_or(&lower);
    filename.ends_with("-shipping.exe")
        || filename.ends_with("_shipping.exe")
        || filename.ends_with("-win64-shipping.exe")
        || filename.ends_with("-win32-shipping.exe")
        || filename.contains("shipping")
        || (lower.contains("binaries/win64/") && filename.ends_with(".exe"))
        || (lower.contains("binaries/win32/") && filename.ends_with(".exe"))
}

/// Assigns a human-friendly label for candidate executables.
pub fn candidate_label(filename: &str, is_shipping: bool) -> String {
    if is_shipping {
        "Shipping Executable".to_string()
    } else if gamenode_core::drm_free::identity::launcher(filename) {
        "Launcher".to_string()
    } else {
        filename.to_string()
    }
}

/// Inspects the PE header of an executable file directly in pure cross-platform Rust.
///
/// Returns `Some(0x8664)` for 64-bit AMD64/x86_64, `Some(0x14c)` for 32-bit x86, or `None`
/// if the file is invalid, a DLL, or unsupported architecture.
pub fn inspect_pe_architecture(path: &Path) -> Option<u16> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos).ok()?;
    if &dos[0..2] != b"MZ" {
        return None;
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().ok()?);
    if offset > 16 * 1024 * 1024 {
        return None;
    }
    file.seek(SeekFrom::Start(u64::from(offset))).ok()?;
    let mut signature = [0u8; 4];
    file.read_exact(&mut signature).ok()?;
    if &signature != b"PE\0\0" {
        return None;
    }
    let mut coff = [0u8; 20];
    file.read_exact(&mut coff).ok()?;
    let machine = u16::from_le_bytes(coff[0..2].try_into().ok()?);
    let sections = u16::from_le_bytes(coff[2..4].try_into().ok()?);
    let optional_size = u16::from_le_bytes(coff[16..18].try_into().ok()?);
    let flags = u16::from_le_bytes(coff[18..20].try_into().ok()?);

    if ![0x14c, 0x8664].contains(&machine)
        || sections == 0
        || sections > 96
        || (flags & 2 == 0) // Must be executable image
        || (flags & 0x2000 != 0) // Must NOT be a DLL
        || !(96..=4096).contains(&optional_size)
    {
        return None;
    }

    let mut optional = vec![0u8; usize::from(optional_size)];
    file.read_exact(&mut optional).ok()?;
    let magic = u16::from_le_bytes(optional[0..2].try_into().ok()?);
    if ![0x10b, 0x20b].contains(&magic) {
        return None;
    }
    if optional.len() < 70 {
        return None;
    }
    let subsystem = u16::from_le_bytes(optional[68..70].try_into().ok()?);
    if ![2, 3].contains(&subsystem) {
        return None;
    }

    let min_len = u64::from(offset) + 24 + u64::from(optional_size) + u64::from(sections) * 40;
    let file_len = file.metadata().ok()?.len();
    if file_len < min_len {
        return None;
    }

    Some(machine)
}

/// Constructs a valid minimal 64-bit (`0x8664`, `0x20b`) or 32-bit (`0x14c`, `0x10b`)
/// Windows PE executable file header in memory for tests.
pub fn make_minimal_pe_bytes(is_64bit: bool) -> Vec<u8> {
    let mut data = vec![0u8; 512];
    // DOS header
    data[0..2].copy_from_slice(b"MZ");
    let pe_offset = 64u32;
    data[60..64].copy_from_slice(&pe_offset.to_le_bytes());

    // PE signature
    let pe_idx = pe_offset as usize;
    data[pe_idx..pe_idx + 4].copy_from_slice(b"PE\0\0");

    // COFF header (20 bytes at pe_idx + 4)
    let coff_idx = pe_idx + 4;
    let machine: u16 = if is_64bit { 0x8664 } else { 0x14c };
    data[coff_idx..coff_idx + 2].copy_from_slice(&machine.to_le_bytes());
    let sections: u16 = 1;
    data[coff_idx + 2..coff_idx + 4].copy_from_slice(&sections.to_le_bytes());
    let optional_size: u16 = 240;
    data[coff_idx + 16..coff_idx + 18].copy_from_slice(&optional_size.to_le_bytes());
    let flags: u16 = 2; // IMAGE_FILE_EXECUTABLE_IMAGE
    data[coff_idx + 18..coff_idx + 20].copy_from_slice(&flags.to_le_bytes());

    // Optional header (starts at pe_idx + 24)
    let opt_idx = coff_idx + 20;
    let magic: u16 = if is_64bit { 0x20b } else { 0x10b };
    data[opt_idx..opt_idx + 2].copy_from_slice(&magic.to_le_bytes());
    let subsystem: u16 = 2; // IMAGE_SUBSYSTEM_WINDOWS_GUI
    data[opt_idx + 68..opt_idx + 70].copy_from_slice(&subsystem.to_le_bytes());

    data
}

/// Resolves a path inside `install_dir`, rejects any `..` traversal or symlink/junction
/// whose canonical target escapes the canonical `install_dir`, and returns
/// `(canonical_full_path, normalized_relative_path_with_forward_slashes)`.
pub fn validate_target_within_install_dir(
    install_dir: &Path,
    relative_or_abs_path: &str,
) -> Result<(PathBuf, String), ToolError> {
    let canonical_install =
        strip_verbatim_prefix(&install_dir.canonicalize().map_err(|_| {
            ToolError::InstallDirNotFound(install_dir.to_string_lossy().to_string())
        })?);
    if !canonical_install.is_dir() {
        return Err(ToolError::InstallDirNotFound(
            install_dir.to_string_lossy().to_string(),
        ));
    }

    let trimmed = relative_or_abs_path.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err(ToolError::TargetExeNotFound(
            relative_or_abs_path.to_string(),
        ));
    }

    let target_path = Path::new(trimmed);

    // Lexical escape check: e.g. ../../outside or outside absolute path
    let mut depth: isize = 0;
    let mut lexically_escapes = false;
    for component in target_path.components() {
        match component {
            std::path::Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    lexically_escapes = true;
                    break;
                }
            }
            std::path::Component::Normal(_) => {
                depth += 1;
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_)
                if !path_starts_with(target_path, &canonical_install) =>
            {
                lexically_escapes = true;
                break;
            }
            _ => {}
        }
    }

    if lexically_escapes {
        return Err(ToolError::PathEscapesInstallDir(
            relative_or_abs_path.to_string(),
        ));
    }

    let candidate = if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        canonical_install.join(target_path)
    };

    // If candidate is a symlink, check whether its link target escapes
    if candidate.is_symlink()
        && let Ok(link_target) = std::fs::read_link(&candidate)
    {
        let resolved = if link_target.is_absolute() {
            link_target
        } else {
            candidate
                .parent()
                .unwrap_or(&canonical_install)
                .join(link_target)
        };
        if let Ok(canon_link) = resolved.canonicalize() {
            let canon_link = strip_verbatim_prefix(&canon_link);
            if !path_starts_with(&canon_link, &canonical_install) {
                return Err(ToolError::PathEscapesInstallDir(
                    relative_or_abs_path.to_string(),
                ));
            }
        } else {
            let mut d: isize = 0;
            let mut esc = false;
            for c in resolved.components() {
                match c {
                    std::path::Component::ParentDir => {
                        d -= 1;
                        if d < 0 {
                            esc = true;
                            break;
                        }
                    }
                    std::path::Component::Normal(_) => d += 1,
                    std::path::Component::RootDir | std::path::Component::Prefix(_)
                        if !path_starts_with(&resolved, &canonical_install) =>
                    {
                        esc = true;
                        break;
                    }
                    _ => {}
                }
            }
            if esc {
                return Err(ToolError::PathEscapesInstallDir(
                    relative_or_abs_path.to_string(),
                ));
            }
        }
    }

    let canonical_target = match candidate.canonicalize() {
        Ok(canon) => strip_verbatim_prefix(&canon),
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                return Err(ToolError::TargetExeNotFound(
                    relative_or_abs_path.to_string(),
                ));
            } else {
                return Err(ToolError::Io(format!(
                    "Failed to resolve target executable {relative_or_abs_path}: {err}"
                )));
            }
        }
    };

    if !path_starts_with(&canonical_target, &canonical_install) {
        return Err(ToolError::PathEscapesInstallDir(
            relative_or_abs_path.to_string(),
        ));
    }

    if canonical_target.is_dir() {
        return Err(ToolError::TargetExeNotFound(
            relative_or_abs_path.to_string(),
        ));
    }

    let rel_str = relative_path_from(&canonical_target, &canonical_install)
        .ok_or_else(|| ToolError::PathEscapesInstallDir(relative_or_abs_path.to_string()))?;

    Ok((canonical_target, rel_str))
}

fn score_candidate(
    rel_path: &str,
    filename: &str,
    machine: u16,
    has_shipping: bool,
    manifest_hints: &[String],
) -> (u8, u8, u8, usize, String) {
    let lower_rel = rel_path.to_lowercase();

    let mut manifest_rank = 2u8;
    for hint in manifest_hints {
        let hint = hint.replace('\\', "/");
        let hint = hint
            .strip_prefix("<base>/")
            .unwrap_or(&hint)
            .trim_start_matches('/');
        if hint.is_empty() || hint.contains(['*', '?', '<', '>']) {
            continue;
        }
        if hint.eq_ignore_ascii_case(rel_path) {
            manifest_rank = 0;
            break;
        }
        if !hint.contains('/') && hint.eq_ignore_ascii_case(filename) {
            manifest_rank = manifest_rank.min(1);
        }
    }

    let is_shipping = is_shipping_binary(rel_path);
    let is_raw_launcher = gamenode_core::drm_free::identity::launcher(filename)
        || gamenode_core::drm_free::identity::launcher(rel_path);
    let is_demoted_root = has_shipping && !rel_path.contains('/') && !is_shipping;

    let role_rank = if is_shipping {
        0u8
    } else if is_raw_launcher || is_demoted_root {
        2u8
    } else {
        1u8
    };

    let arch_rank = if machine == 0x8664 {
        0u8
    } else if machine == 0x14c {
        2u8
    } else {
        1u8
    };

    let depth = rel_path.bytes().filter(|&b| b == b'/').count();

    (manifest_rank, role_rank, arch_rank, depth, lower_rel)
}

/// Universal target executable resolution and anti-cheat folder scanning.
pub fn inspect_game_target(
    entry_key: &str,
    install_dir: &str,
    title: &str,
    manual_exe: Option<&str>,
) -> Result<TargetInspection, ToolError> {
    let install_path = Path::new(install_dir);
    let canonical_install = strip_verbatim_prefix(
        &install_path
            .canonicalize()
            .map_err(|_| ToolError::InstallDirNotFound(install_dir.to_string()))?,
    );
    if !canonical_install.is_dir() {
        return Err(ToolError::InstallDirNotFound(install_dir.to_string()));
    }

    // Validate manual_exe if provided
    let validated_manual = if let Some(man_exe) = manual_exe {
        let (canon_exe, rel_exe) = validate_target_within_install_dir(&canonical_install, man_exe)?;
        let machine = inspect_pe_architecture(&canon_exe).ok_or_else(|| {
            ToolError::TargetExeNotFound(format!(
                "Manual target is not a valid PE executable: {man_exe}"
            ))
        })?;
        Some((canon_exe, rel_exe, machine))
    } else {
        None
    };

    // Walk install_dir up to depth 6 (bounded at 10,000 entries)
    let mut queue = vec![(canonical_install.clone(), 0usize)];
    let mut visited_dirs = BTreeSet::new();
    visited_dirs.insert(canonical_install.clone());
    let mut entry_count = 0usize;
    let mut all_relative_paths: Vec<String> = Vec::new();
    let mut candidate_exes: Vec<(String, String, u16)> = Vec::new();

    while let Some((dir, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            entry_count += 1;
            if entry_count > 10_000 {
                break;
            }
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_symlink() {
                let Ok(canonical_target) = path.canonicalize() else {
                    continue;
                };
                let canonical_target = strip_verbatim_prefix(&canonical_target);
                if !path_starts_with(&canonical_target, &canonical_install) {
                    continue;
                }
                // Symlink executables/directories are skipped during discovery per requirements
                continue;
            }

            let Some(rel_path) = relative_path_from(&path, &canonical_install) else {
                continue;
            };

            all_relative_paths.push(rel_path.clone());

            if file_type.is_dir() {
                if depth < 6 {
                    let Ok(canonical_sub) = path.canonicalize() else {
                        continue;
                    };
                    let canonical_sub = strip_verbatim_prefix(&canonical_sub);
                    if path_starts_with(&canonical_sub, &canonical_install)
                        && visited_dirs.insert(canonical_sub.clone())
                    {
                        queue.push((canonical_sub, depth + 1));
                    }
                }
            } else if file_type.is_file() {
                let is_exe = path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
                if is_exe {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    if is_maintenance_binary(&filename) {
                        continue;
                    }
                    if let Some(machine) = inspect_pe_architecture(&path) {
                        candidate_exes.push((rel_path, filename, machine));
                    }
                }
            }
        }
        if entry_count > 10_000 {
            break;
        }
    }

    // Anti-cheat evaluation
    let anticheat = evaluate_anticheat(
        entry_key,
        title,
        all_relative_paths.iter().map(|s| s.as_str()),
    );

    // If validated_manual is present and was not discovered, append it
    if let Some((_, ref man_rel, man_machine)) = validated_manual
        && !candidate_exes.iter().any(|(rel, _, _)| rel == man_rel)
    {
        let filename = Path::new(man_rel)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| man_rel.clone());
        candidate_exes.push((man_rel.clone(), filename, man_machine));
    }

    let manifest_hints: Vec<String> = if let Some(data) = crate::paths::DataPaths::resolve() {
        if let Ok(cache) = crate::manifest_cache::ManifestCache::bundled(data.data_dir()) {
            if let Ok(manifest) = cache.load() {
                match manifest.index.exact(title).as_slice() {
                    [game] => game.launch_hints.iter().cloned().collect(),
                    _ => Vec::new(),
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let has_shipping = candidate_exes
        .iter()
        .any(|(rel, _, _)| is_shipping_binary(rel));

    // Sort candidates: manual_exe first (if specified), then score
    candidate_exes.sort_by(|(rel_a, file_a, mach_a), (rel_b, file_b, mach_b)| {
        if let Some((_, ref man_rel, _)) = validated_manual {
            let a_is_manual = rel_a == man_rel;
            let b_is_manual = rel_b == man_rel;
            if a_is_manual != b_is_manual {
                return b_is_manual.cmp(&a_is_manual);
            }
        }

        let score_a = score_candidate(rel_a, file_a, *mach_a, has_shipping, &manifest_hints);
        let score_b = score_candidate(rel_b, file_b, *mach_b, has_shipping, &manifest_hints);
        score_a.cmp(&score_b)
    });

    let preferred_rel = candidate_exes.first().map(|(rel, _, _)| rel.clone());

    let candidates: Vec<TargetCandidate> = candidate_exes
        .into_iter()
        .map(|(rel, file, machine)| {
            let is_pref = preferred_rel.as_ref() == Some(&rel);
            let is_shipping = is_shipping_binary(&rel);
            TargetCandidate {
                label: candidate_label(&file, is_shipping),
                relative_exe: rel,
                architecture: if machine == 0x8664 {
                    "x64".to_string()
                } else {
                    "x86".to_string()
                },
                preferred: is_pref,
            }
        })
        .collect();

    Ok(TargetInspection {
        entry_key: entry_key.to_string(),
        install_dir: canonical_install.to_string_lossy().to_string(),
        candidates,
        selected_exe: preferred_rel,
        anticheat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamenode_core::tools::AntiCheatSystem;

    #[test]
    fn test_pe_minimal_bytes_and_inspect_architecture() {
        let dir = tempfile::tempdir().unwrap();

        let exe64_path = dir.path().join("test64.exe");
        std::fs::write(&exe64_path, make_minimal_pe_bytes(true)).unwrap();
        assert_eq!(inspect_pe_architecture(&exe64_path), Some(0x8664));

        let exe32_path = dir.path().join("test32.exe");
        std::fs::write(&exe32_path, make_minimal_pe_bytes(false)).unwrap();
        assert_eq!(inspect_pe_architecture(&exe32_path), Some(0x14c));

        // Invalid: text file
        let invalid_path = dir.path().join("invalid.exe");
        std::fs::write(&invalid_path, b"not a PE binary").unwrap();
        assert_eq!(inspect_pe_architecture(&invalid_path), None);

        // DLL: set DLL flag 0x2000
        let mut dll_bytes = make_minimal_pe_bytes(true);
        dll_bytes[87] |= 0x20; // flags high byte 0x20 => 0x2000
        let dll_path = dir.path().join("test.dll");
        std::fs::write(&dll_path, dll_bytes).unwrap();
        assert_eq!(inspect_pe_architecture(&dll_path), None);
    }

    #[test]
    fn test_unreal_engine_layout_selects_shipping_binary() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Root launcher/bootstrap
        let root_exe = root.join("Game.exe");
        std::fs::write(&root_exe, make_minimal_pe_bytes(true)).unwrap();

        // Nested shipping executable
        let shipping_dir = root.join("Game/Binaries/Win64");
        std::fs::create_dir_all(&shipping_dir).unwrap();
        let shipping_exe = shipping_dir.join("Game-Win64-Shipping.exe");
        std::fs::write(&shipping_exe, make_minimal_pe_bytes(true)).unwrap();

        let inspection =
            inspect_game_target("steam:12345", root.to_str().unwrap(), "Unreal Game", None)
                .expect("inspection succeeds");

        assert_eq!(
            inspection.selected_exe,
            Some("Game/Binaries/Win64/Game-Win64-Shipping.exe".to_string())
        );
        assert_eq!(inspection.candidates.len(), 2);
        assert_eq!(
            inspection.candidates[0].relative_exe,
            "Game/Binaries/Win64/Game-Win64-Shipping.exe"
        );
        assert!(inspection.candidates[0].preferred);
        assert_eq!(inspection.candidates[0].label, "Shipping Executable");
        assert_eq!(inspection.candidates[0].architecture, "x64");

        assert_eq!(inspection.candidates[1].relative_exe, "Game.exe");
        assert!(!inspection.candidates[1].preferred);
    }

    #[test]
    fn test_manual_game_registration_with_manual_exe_override() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let root_exe = root.join("Game.exe");
        std::fs::write(&root_exe, make_minimal_pe_bytes(true)).unwrap();

        let shipping_dir = root.join("Game/Binaries/Win64");
        std::fs::create_dir_all(&shipping_dir).unwrap();
        let shipping_exe = shipping_dir.join("Game-Win64-Shipping.exe");
        std::fs::write(&shipping_exe, make_minimal_pe_bytes(true)).unwrap();

        // User overrides target to root Game.exe
        let inspection = inspect_game_target(
            "manual:custom-game",
            root.to_str().unwrap(),
            "Custom Game",
            Some("Game.exe"),
        )
        .expect("manual inspection succeeds");

        assert_eq!(inspection.selected_exe, Some("Game.exe".to_string()));
        assert_eq!(inspection.candidates.len(), 2);
        assert_eq!(inspection.candidates[0].relative_exe, "Game.exe");
        assert!(inspection.candidates[0].preferred);

        assert_eq!(
            inspection.candidates[1].relative_exe,
            "Game/Binaries/Win64/Game-Win64-Shipping.exe"
        );
        assert!(!inspection.candidates[1].preferred);
    }

    #[test]
    fn test_skipping_maintenance_and_redistributable_executables() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let game_exe = root.join("Game.exe");
        std::fs::write(&game_exe, make_minimal_pe_bytes(true)).unwrap();

        let unity_crash = root.join("UnityCrashHandler64.exe");
        std::fs::write(&unity_crash, make_minimal_pe_bytes(true)).unwrap();

        let vc_redist = root.join("vc_redist.x64.exe");
        std::fs::write(&vc_redist, make_minimal_pe_bytes(true)).unwrap();

        let inspection = inspect_game_target(
            "steam:99999",
            root.to_str().unwrap(),
            "Game with redist",
            None,
        )
        .expect("inspection succeeds");

        assert_eq!(inspection.candidates.len(), 1);
        assert_eq!(inspection.candidates[0].relative_exe, "Game.exe");
    }

    #[test]
    fn test_anti_cheat_detection_and_blocklist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let game_exe = root.join("Game.exe");
        std::fs::write(&game_exe, make_minimal_pe_bytes(true)).unwrap();

        // EasyAntiCheat marker
        let eac_dir = root.join("EasyAntiCheat");
        std::fs::create_dir_all(&eac_dir).unwrap();
        std::fs::write(
            eac_dir.join("EasyAntiCheat_EOS.exe"),
            make_minimal_pe_bytes(true),
        )
        .unwrap();

        // BattlEye marker
        let be_dir = root.join("BattlEye");
        std::fs::create_dir_all(&be_dir).unwrap();
        std::fs::write(
            be_dir.join("BEService_x64.exe"),
            make_minimal_pe_bytes(true),
        )
        .unwrap();

        // Test with blocklisted steam entry (Apex Legends: steam:1172470)
        let inspection = inspect_game_target(
            "steam:1172470",
            root.to_str().unwrap(),
            "Apex Legends",
            None,
        )
        .expect("inspection succeeds");

        assert!(inspection.anticheat.has_warning());
        assert!(
            inspection
                .anticheat
                .detected
                .contains(&AntiCheatSystem::EasyAntiCheat)
        );
        assert!(
            inspection
                .anticheat
                .detected
                .contains(&AntiCheatSystem::BattlEye)
        );
        assert!(
            inspection
                .anticheat
                .detected
                .contains(&AntiCheatSystem::Blocklisted)
        );
    }

    #[test]
    fn test_rejecting_symlinks_and_traversal_escaping_install_dir() {
        let temp_parent = tempfile::tempdir().unwrap();
        let install_dir = temp_parent.path().join("game");
        let outside_dir = temp_parent.path().join("outside");
        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::create_dir_all(&outside_dir).unwrap();

        let secret_exe = outside_dir.join("secret.exe");
        std::fs::write(&secret_exe, make_minimal_pe_bytes(true)).unwrap();

        // 1. Direct parent directory traversal
        let err =
            validate_target_within_install_dir(&install_dir, "../outside/secret.exe").unwrap_err();
        assert!(matches!(err, ToolError::PathEscapesInstallDir(_)));

        // 2. Symlink escaping install_dir (on Unix)
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let symlink_path = install_dir.join("escaped_symlink.exe");
            symlink(&secret_exe, &symlink_path).unwrap();

            let err = validate_target_within_install_dir(&install_dir, "escaped_symlink.exe")
                .unwrap_err();
            assert!(matches!(err, ToolError::PathEscapesInstallDir(_)));

            // Manual exe targeting escaping symlink fails
            let err = inspect_game_target(
                "manual:1",
                install_dir.to_str().unwrap(),
                "Title",
                Some("escaped_symlink.exe"),
            )
            .unwrap_err();
            assert!(matches!(err, ToolError::PathEscapesInstallDir(_)));

            // Escaped symlink is skipped during discovery
            let inspection =
                inspect_game_target("manual:1", install_dir.to_str().unwrap(), "Title", None)
                    .unwrap();
            assert!(inspection.candidates.is_empty());
        }

        // 3. Nonexistent file returns TargetExeNotFound
        let err = validate_target_within_install_dir(&install_dir, "missing.exe").unwrap_err();
        assert!(matches!(err, ToolError::TargetExeNotFound(_)));

        // 4. Nonexistent install_dir returns InstallDirNotFound
        let missing_dir = temp_parent.path().join("missing_dir");
        let err = validate_target_within_install_dir(&missing_dir, "game.exe").unwrap_err();
        assert!(matches!(err, ToolError::InstallDirNotFound(_)));

        let err = inspect_game_target("steam:1", missing_dir.to_str().unwrap(), "Title", None)
            .unwrap_err();
        assert!(matches!(err, ToolError::InstallDirNotFound(_)));

        // 5. Absolute path inside install_dir succeeds, outside escapes
        let valid_exe = install_dir.join("valid.exe");
        std::fs::write(&valid_exe, make_minimal_pe_bytes(true)).unwrap();
        let (canon, rel) =
            validate_target_within_install_dir(&install_dir, valid_exe.to_str().unwrap())
                .expect("absolute path inside install_dir succeeds");
        assert_eq!(canon, valid_exe.canonicalize().unwrap());
        assert_eq!(rel, "valid.exe");

        let err = validate_target_within_install_dir(&install_dir, secret_exe.to_str().unwrap())
            .unwrap_err();
        assert!(matches!(err, ToolError::PathEscapesInstallDir(_)));
    }
}
