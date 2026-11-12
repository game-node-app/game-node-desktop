//! Pure evaluation of a signed package target against the detected execution
//! environment. Author-tested environments are informational and never affect
//! this verdict.

use gamenode_platform::plugin_environment::{
    EmulationPolicy, EmulationStatus, LibcIdentity, OperatingSystemKind, PluginEnvironment,
};

use super::package::{LibcFamily, Target, WindowsProduct};
use super::package::{Manifest, ManifestError};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatibilityState {
    Compatible,
    Incompatible,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatibilityReason {
    OperatingSystemMismatch,
    ArchitectureMismatch,
    AbiMismatch,
    UnknownOperatingSystem,
    UnknownArchitecture,
    UnknownAbi,
    EmulationRejected,
    EmulationUnknown,
    WindowsProductMismatch,
    WindowsVersionTooOld,
    WindowsVersionTooNew,
    WindowsBuildTooOld,
    WindowsBuildTooNew,
    WindowsProductUnknown,
    WindowsVersionUnknown,
    WindowsBuildUnknown,
    LinuxLibcMismatch,
    LinuxLibcVersionTooOld,
    LinuxLibcUnknown,
    LinuxDistributionMismatch,
    LinuxDistributionVersionMismatch,
    LinuxDistributionUnknown,
    NoAdditionalRequirements,
    RequirementsMalformed,
    RequirementsSatisfied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatibilityVerdict {
    pub state: CompatibilityState,
    /// Stable machine-readable reasons; strings suitable for logs/UI belong in
    /// a presentation layer.
    pub reasons: Vec<CompatibilityReason>,
    /// Facts the detector could not establish, preserved for diagnostics.
    pub unknown_facts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetCompatibilityAssessment {
    /// Retains all signed target declarations, including informational `tested_on` entries.
    pub target: Target,
    pub verdict: CompatibilityVerdict,
}

#[derive(Debug, thiserror::Error)]
pub enum PackageCompatibilityError {
    #[error("invalid package manifest: {0}")]
    InvalidManifest(ManifestError),
    #[error("executable resolves outside the package directory")]
    ExecutableOutsidePackage,
    #[error("executable is not a declared manifest target")]
    ExecutableNotDeclared,
    #[error("execution environment detection failed: {0}")]
    EnvironmentDetection(String),
    #[error("compatibility worker failed")]
    WorkerFailed,
}

/// Validate a package manifest against the target executable and a detected
/// effective environment. This is reusable by import/update/activation flows;
/// pre-launch also calls it with refreshed environment facts.
pub fn assess_package(
    package_root: &Path,
    executable: &Path,
    manifest_bytes: &[u8],
    environment: &PluginEnvironment,
    emulation_policy: EmulationPolicy,
) -> Result<CompatibilityVerdict, PackageCompatibilityError> {
    let manifest =
        Manifest::parse_json(manifest_bytes).map_err(PackageCompatibilityError::InvalidManifest)?;
    let root = std::fs::canonicalize(package_root)
        .map_err(|_| PackageCompatibilityError::ExecutableOutsidePackage)?;
    let executable = std::fs::canonicalize(executable)
        .map_err(|_| PackageCompatibilityError::ExecutableOutsidePackage)?;
    let relative = executable
        .strip_prefix(&root)
        .map_err(|_| PackageCompatibilityError::ExecutableOutsidePackage)?
        .to_string_lossy()
        .replace('\\', "/");
    let target = manifest
        .manifest()
        .targets
        .iter()
        .find(|target| target.entrypoint == relative)
        .ok_or(PackageCompatibilityError::ExecutableNotDeclared)?;
    Ok(evaluate_target(target, environment, emulation_policy))
}

/// Evaluate every manifest target for import or update selection. Callers can
/// display the retained `tested_on` declarations separately from the verdict;
/// tested coverage never changes eligibility.
pub fn assess_manifest_targets(
    manifest_bytes: &[u8],
    environment: &PluginEnvironment,
    emulation_policy: EmulationPolicy,
) -> Result<Vec<TargetCompatibilityAssessment>, PackageCompatibilityError> {
    let manifest =
        Manifest::parse_json(manifest_bytes).map_err(PackageCompatibilityError::InvalidManifest)?;
    Ok(manifest
        .manifest()
        .targets
        .iter()
        .cloned()
        .map(|target| TargetCompatibilityAssessment {
            verdict: evaluate_target(&target, environment, emulation_policy),
            target,
        })
        .collect())
}

/// Detect the current execution environment and assess every declared target.
/// Both the OS probes and manifest evaluation run off the UI/executor thread.
pub async fn assess_manifest_for_current_environment(
    manifest_bytes: Vec<u8>,
    emulation_policy: EmulationPolicy,
) -> Result<(PluginEnvironment, Vec<TargetCompatibilityAssessment>), PackageCompatibilityError> {
    let environment = gamenode_platform::plugin_environment::detect()
        .await
        .map_err(PackageCompatibilityError::EnvironmentDetection)?;
    let evaluation_environment = environment.clone();
    let assessments = tokio::task::spawn_blocking(move || {
        assess_manifest_targets(&manifest_bytes, &evaluation_environment, emulation_policy)
    })
    .await
    .map_err(|_| PackageCompatibilityError::WorkerFailed)??;
    Ok((environment, assessments))
}

impl CompatibilityVerdict {
    fn new(
        state: CompatibilityState,
        reasons: Vec<CompatibilityReason>,
        environment: &PluginEnvironment,
    ) -> Self {
        Self {
            state,
            reasons,
            unknown_facts: environment.unknown_facts.clone(),
        }
    }
}

/// Evaluate one already-validated target. `tested_on` is deliberately ignored:
/// it records author testing and is not a runtime requirement.
pub fn evaluate_target(
    target: &Target,
    environment: &PluginEnvironment,
    emulation_policy: EmulationPolicy,
) -> CompatibilityVerdict {
    let mut reasons = Vec::new();
    let mut indeterminate = false;

    let detected_os = detect_os(environment);
    match detected_os {
        Some(os) if os == target.os => {}
        Some(_) => reasons.push(CompatibilityReason::OperatingSystemMismatch),
        None => {
            reasons.push(CompatibilityReason::UnknownOperatingSystem);
            indeterminate = true;
        }
    }

    match environment.process_architecture.as_deref() {
        Some(arch) if normalize_arch(arch) == normalize_arch(&target.arch) => {}
        Some(_) => reasons.push(CompatibilityReason::ArchitectureMismatch),
        None => {
            reasons.push(CompatibilityReason::UnknownArchitecture);
            indeterminate = true;
        }
    }

    match environment.process_abi.as_deref() {
        Some(abi) if abi == target.abi => {}
        Some(_) => reasons.push(CompatibilityReason::AbiMismatch),
        None => {
            reasons.push(CompatibilityReason::UnknownAbi);
            indeterminate = true;
        }
    }

    if !emulation_policy.accepts(&environment.emulation) {
        match &environment.emulation {
            EmulationStatus::Unknown => {
                reasons.push(CompatibilityReason::EmulationUnknown);
                indeterminate = true;
            }
            EmulationStatus::Emulated => reasons.push(CompatibilityReason::EmulationRejected),
            EmulationStatus::Native => unreachable!("native is accepted by every policy"),
        }
    }

    if !reasons.is_empty() && !indeterminate {
        return CompatibilityVerdict::new(CompatibilityState::Incompatible, reasons, environment);
    }

    if let Some(requirements) = &target.requires {
        match target.os.as_str() {
            "windows" => {
                let Some(req) = requirements.windows.as_ref() else {
                    reasons.push(CompatibilityReason::RequirementsMalformed);
                    return CompatibilityVerdict::new(
                        CompatibilityState::Indeterminate,
                        reasons,
                        environment,
                    );
                };
                indeterminate |= evaluate_windows(req, environment, &mut reasons);
            }
            "linux" => {
                let Some(req) = requirements.linux.as_ref() else {
                    reasons.push(CompatibilityReason::RequirementsMalformed);
                    return CompatibilityVerdict::new(
                        CompatibilityState::Indeterminate,
                        reasons,
                        environment,
                    );
                };
                indeterminate |= evaluate_linux(req, environment, &mut reasons);
            }
            _ => {
                reasons.push(CompatibilityReason::RequirementsMalformed);
                return CompatibilityVerdict::new(
                    CompatibilityState::Indeterminate,
                    reasons,
                    environment,
                );
            }
        }
    } else {
        reasons.push(CompatibilityReason::NoAdditionalRequirements);
    }

    if reasons.iter().any(is_definite_mismatch) {
        CompatibilityVerdict::new(CompatibilityState::Incompatible, reasons, environment)
    } else if indeterminate {
        CompatibilityVerdict::new(CompatibilityState::Indeterminate, reasons, environment)
    } else {
        if !reasons.contains(&CompatibilityReason::RequirementsSatisfied) {
            reasons.push(CompatibilityReason::RequirementsSatisfied);
        }
        CompatibilityVerdict::new(CompatibilityState::Compatible, reasons, environment)
    }
}

fn detect_os(environment: &PluginEnvironment) -> Option<&'static str> {
    if environment
        .userspace
        .as_ref()
        .is_some_and(|userspace| userspace.family.eq_ignore_ascii_case("windows"))
    {
        Some("windows")
    } else if environment
        .host
        .as_ref()
        .is_some_and(|host| host.family.eq_ignore_ascii_case("linux"))
        || environment.libc.is_some()
    {
        Some("linux")
    } else {
        None
    }
}

fn evaluate_windows(
    requirements: &super::package::WindowsRequirements,
    environment: &PluginEnvironment,
    reasons: &mut Vec<CompatibilityReason>,
) -> bool {
    let Some(userspace) = environment.userspace.as_ref() else {
        reasons.push(CompatibilityReason::WindowsProductUnknown);
        reasons.push(CompatibilityReason::WindowsVersionUnknown);
        reasons.push(CompatibilityReason::WindowsBuildUnknown);
        return true;
    };
    let mut unknown = false;

    if let Some(required) = requirements.product {
        let actual = userspace.kind;
        let matches = matches!(
            (required, actual),
            (WindowsProduct::Client, Some(OperatingSystemKind::Client))
                | (WindowsProduct::Server, Some(OperatingSystemKind::Server))
        );
        if matches {
            // matched
        } else if actual.is_some_and(|kind| kind != OperatingSystemKind::Unknown) {
            reasons.push(CompatibilityReason::WindowsProductMismatch);
        } else {
            reasons.push(CompatibilityReason::WindowsProductUnknown);
            unknown = true;
        }
    }

    if requirements.min_version.is_some() || requirements.max_version.is_some() {
        let Some(actual) = userspace.version_id.as_deref() else {
            reasons.push(CompatibilityReason::WindowsVersionUnknown);
            unknown = true;
            return unknown;
        };
        for (bound, is_minimum) in [
            (requirements.min_version.as_deref(), true),
            (requirements.max_version.as_deref(), false),
        ] {
            let Some(bound) = bound else { continue };
            match compare_numeric_versions(actual, bound) {
                Some(ordering) if is_minimum && ordering.is_lt() => {
                    reasons.push(CompatibilityReason::WindowsVersionTooOld)
                }
                Some(ordering) if !is_minimum && ordering.is_gt() => {
                    reasons.push(CompatibilityReason::WindowsVersionTooNew)
                }
                Some(_) => {}
                None => {
                    reasons.push(CompatibilityReason::WindowsVersionUnknown);
                    unknown = true;
                }
            }
        }
    }

    if requirements.min_build.is_some() || requirements.max_build.is_some() {
        let Some(actual) = userspace.build else {
            reasons.push(CompatibilityReason::WindowsBuildUnknown);
            return true;
        };
        if requirements.min_build.is_some_and(|min| actual < min) {
            reasons.push(CompatibilityReason::WindowsBuildTooOld);
        }
        if requirements.max_build.is_some_and(|max| actual > max) {
            reasons.push(CompatibilityReason::WindowsBuildTooNew);
        }
    }
    unknown
}

fn evaluate_linux(
    requirements: &super::package::LinuxRequirements,
    environment: &PluginEnvironment,
    reasons: &mut Vec<CompatibilityReason>,
) -> bool {
    let mut unknown = false;
    if let Some(required) = &requirements.libc {
        let actual = environment.libc.as_ref();
        let (actual_family, actual_version) = match actual {
            Some(LibcIdentity::Gnu(version)) => (Some(LibcFamily::Glibc), version.as_deref()),
            Some(LibcIdentity::Musl(version)) => (Some(LibcFamily::Musl), version.as_deref()),
            Some(LibcIdentity::Unknown) | None => (None, None),
        };
        match actual_family {
            Some(family) if family != required.family => {
                reasons.push(CompatibilityReason::LinuxLibcMismatch)
            }
            Some(_) => match actual_version {
                Some(version) => match compare_numeric_versions(version, &required.min_version) {
                    Some(ordering) if ordering.is_lt() => {
                        reasons.push(CompatibilityReason::LinuxLibcVersionTooOld)
                    }
                    Some(_) => {}
                    None => {
                        reasons.push(CompatibilityReason::LinuxLibcUnknown);
                        unknown = true;
                    }
                },
                None => {
                    reasons.push(CompatibilityReason::LinuxLibcUnknown);
                    unknown = true;
                }
            },
            None => {
                reasons.push(CompatibilityReason::LinuxLibcUnknown);
                unknown = true;
            }
        }
    }

    if let Some(alternatives) = &requirements.distributions {
        let Some(userspace) = environment.userspace.as_ref() else {
            reasons.push(CompatibilityReason::LinuxDistributionUnknown);
            return true;
        };
        let (Some(id), Some(version)) = (userspace.id.as_deref(), userspace.version_id.as_deref())
        else {
            reasons.push(CompatibilityReason::LinuxDistributionUnknown);
            return true;
        };
        if alternatives
            .iter()
            .any(|entry| entry.id == id && entry.versions.iter().any(|v| v == version))
        {
            // At least one author-declared OS/version alternative matches.
        } else if alternatives.iter().any(|entry| entry.id == id) {
            reasons.push(CompatibilityReason::LinuxDistributionVersionMismatch);
        } else {
            reasons.push(CompatibilityReason::LinuxDistributionMismatch);
        }
    }
    unknown
}

fn is_definite_mismatch(reason: &CompatibilityReason) -> bool {
    matches!(
        reason,
        CompatibilityReason::OperatingSystemMismatch
            | CompatibilityReason::ArchitectureMismatch
            | CompatibilityReason::AbiMismatch
            | CompatibilityReason::EmulationRejected
            | CompatibilityReason::WindowsProductMismatch
            | CompatibilityReason::WindowsVersionTooOld
            | CompatibilityReason::WindowsVersionTooNew
            | CompatibilityReason::WindowsBuildTooOld
            | CompatibilityReason::WindowsBuildTooNew
            | CompatibilityReason::LinuxLibcMismatch
            | CompatibilityReason::LinuxLibcVersionTooOld
            | CompatibilityReason::LinuxDistributionMismatch
            | CompatibilityReason::LinuxDistributionVersionMismatch
    )
}

fn normalize_arch(architecture: &str) -> String {
    match architecture.to_ascii_lowercase().as_str() {
        "amd64" | "x86-64" => "x86_64".into(),
        "arm64" => "aarch64".into(),
        "i386" | "i486" | "i586" | "i686" => "x86".into(),
        other => other.into(),
    }
}

fn compare_numeric_versions(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let parse = |version: &str| {
        if version.is_empty()
            || !version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return None;
        }
        version
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()
    };
    let mut left = parse(left)?;
    let mut right = parse(right)?;
    let len = left.len().max(right.len());
    left.resize(len, 0);
    right.resize(len, 0);
    Some(left.cmp(&right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::package::{
        DistributionRequirement, LibcRequirements, LinuxRequirements, Target, TargetRequirements,
    };
    use gamenode_platform::plugin_environment::{HostIdentity, LibcIdentity, UserspaceIdentity};
    use std::collections::BTreeMap;

    fn target(os: &str, arch: &str, abi: &str, requires: Option<TargetRequirements>) -> Target {
        Target {
            os: os.into(),
            arch: arch.into(),
            abi: abi.into(),
            entrypoint: "bin/plugin".into(),
            requires,
            tested_on: vec![],
        }
    }

    fn linux_environment() -> PluginEnvironment {
        PluginEnvironment {
            userspace: Some(UserspaceIdentity {
                family: "debian".into(),
                id: Some("debian".into()),
                version_id: Some("13".into()),
                name: Some("Debian GNU/Linux".into()),
                product: None,
                major: Some(13),
                minor: None,
                build: None,
                kind: None,
                extra: BTreeMap::new(),
            }),
            host: Some(HostIdentity {
                family: "Linux".into(),
                version: Some("6.12".into()),
                architecture: Some("x86_64".into()),
            }),
            process_architecture: Some("x86_64".into()),
            process_abi: Some("gnu".into()),
            libc: Some(LibcIdentity::Gnu(Some("2.36".into()))),
            emulation: EmulationStatus::Native,
            unknown_facts: vec![],
        }
    }

    fn windows_environment() -> PluginEnvironment {
        PluginEnvironment {
            userspace: Some(UserspaceIdentity {
                family: "windows".into(),
                id: None,
                version_id: Some("10.0.22631".into()),
                name: Some("Windows 11".into()),
                product: Some("Windows product SKU".into()),
                major: Some(10),
                minor: Some(0),
                build: Some(22631),
                kind: Some(OperatingSystemKind::Client),
                extra: BTreeMap::new(),
            }),
            host: Some(HostIdentity {
                family: "windows_nt_kernel".into(),
                version: Some("10.0".into()),
                architecture: Some("x86_64".into()),
            }),
            process_architecture: Some("x86_64".into()),
            process_abi: Some("msvc".into()),
            libc: None,
            emulation: EmulationStatus::Native,
            unknown_facts: vec![],
        }
    }

    #[test]
    fn linux_requirements_and_alternative_distributions_are_evaluated() {
        let requirements = TargetRequirements {
            windows: None,
            linux: Some(LinuxRequirements {
                libc: Some(LibcRequirements {
                    family: LibcFamily::Glibc,
                    min_version: "2.34".into(),
                }),
                distributions: Some(vec![DistributionRequirement {
                    id: "debian".into(),
                    versions: vec!["13".into(), "13.1".into()],
                }]),
            }),
        };
        let verdict = evaluate_target(
            &target("linux", "amd64", "gnu", Some(requirements)),
            &linux_environment(),
            EmulationPolicy::RequireNative,
        );
        assert_eq!(verdict.state, CompatibilityState::Compatible);
    }

    #[test]
    fn incompatible_and_indeterminate_are_distinct() {
        let mismatch = evaluate_target(
            &target("linux", "x86_64", "gnu", None),
            &linux_environment(),
            EmulationPolicy::RequireNative,
        );
        assert_eq!(mismatch.state, CompatibilityState::Compatible);
        let incompatible = evaluate_target(
            &target("linux", "aarch64", "gnu", None),
            &linux_environment(),
            EmulationPolicy::RequireNative,
        );
        assert_eq!(incompatible.state, CompatibilityState::Incompatible);
        let unknown = PluginEnvironment {
            unknown_facts: vec!["architecture".into()],
            ..Default::default()
        };
        let indeterminate = evaluate_target(
            &target("linux", "x86_64", "gnu", None),
            &unknown,
            EmulationPolicy::AllowUnknown,
        );
        assert_eq!(indeterminate.state, CompatibilityState::Indeterminate);
        assert_eq!(indeterminate.unknown_facts, vec!["architecture".to_owned()]);
    }

    #[test]
    fn tested_coverage_is_not_a_runtime_requirement_and_emulation_is_explicit() {
        let mut target = target("linux", "x86_64", "gnu", None);
        target
            .tested_on
            .push(super::super::package::TestedEnvironment {
                product: None,
                release: None,
                distribution: Some("fedora".into()),
                version: Some("40".into()),
            });
        let mut environment = linux_environment();
        environment.emulation = EmulationStatus::Emulated;
        assert_eq!(
            evaluate_target(&target, &environment, EmulationPolicy::AllowEmulated).state,
            CompatibilityState::Compatible
        );
        assert_eq!(
            evaluate_target(&target, &environment, EmulationPolicy::RequireNative).state,
            CompatibilityState::Incompatible
        );
    }

    #[test]
    fn package_assessment_uses_only_the_signed_declared_entrypoint() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("bin")).unwrap();
        let executable = directory.path().join("bin/plugin");
        std::fs::write(&executable, b"fixture").unwrap();
        let manifest = br#"{
            "schema":1,
            "id":"org.example.reader",
            "kind":"provider",
            "provider_id":"example-reader",
            "name":"Reader",
            "version":"1.0.0",
            "api":{"min":"1.4.0","max":"1.4.0"},
            "targets":[{"os":"linux","arch":"x86_64","abi":"gnu","entrypoint":"bin/plugin","tested_on":[{"distribution":"fedora","version":"40"}]}],
            "publisher":{"key_id":"fixture","public_key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},
            "license":{"expression":"MIT","files":["licenses/LICENSE"]}
        }"#;

        let compatible = assess_package(
            directory.path(),
            &executable,
            manifest,
            &linux_environment(),
            EmulationPolicy::RequireNative,
        )
        .unwrap();
        assert_eq!(compatible.state, CompatibilityState::Compatible);
        let assessments = assess_manifest_targets(
            manifest,
            &linux_environment(),
            EmulationPolicy::RequireNative,
        )
        .unwrap();
        assert_eq!(assessments.len(), 1);
        assert_eq!(assessments[0].verdict.state, CompatibilityState::Compatible);
        assert_eq!(
            assessments[0].target.tested_on[0].distribution.as_deref(),
            Some("fedora")
        );

        let outside = tempfile::tempdir().unwrap();
        let outside_executable = outside.path().join("plugin");
        std::fs::write(&outside_executable, b"fixture").unwrap();
        assert!(matches!(
            assess_package(
                directory.path(),
                &outside_executable,
                manifest,
                &linux_environment(),
                EmulationPolicy::RequireNative,
            ),
            Err(PackageCompatibilityError::ExecutableOutsidePackage)
        ));
    }

    #[test]
    fn windows_product_version_and_build_bounds_are_checked() {
        let requirements = TargetRequirements {
            windows: Some(super::super::package::WindowsRequirements {
                product: Some(WindowsProduct::Client),
                min_version: Some("10.0".into()),
                max_version: Some("10.0.30000".into()),
                min_build: Some(22000),
                max_build: Some(30000),
            }),
            linux: None,
        };
        let target = target("windows", "x86_64", "msvc", Some(requirements));
        assert_eq!(
            evaluate_target(
                &target,
                &windows_environment(),
                EmulationPolicy::RequireNative,
            )
            .state,
            CompatibilityState::Compatible
        );

        let mut old_build = windows_environment();
        let userspace = old_build.userspace.as_mut().unwrap();
        userspace.version_id = Some("10.0.19045".into());
        userspace.build = Some(19045);
        assert_eq!(
            evaluate_target(&target, &old_build, EmulationPolicy::RequireNative).state,
            CompatibilityState::Incompatible
        );

        let mut unknown_build = windows_environment();
        unknown_build.userspace.as_mut().unwrap().build = None;
        assert_eq!(
            evaluate_target(&target, &unknown_build, EmulationPolicy::RequireNative).state,
            CompatibilityState::Indeterminate
        );
    }
}
