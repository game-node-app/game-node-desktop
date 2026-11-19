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
    let userspace = environment.userspace.as_ref();
    let mut unknown = false;

    if let Some(required) = requirements.product {
        let actual = userspace.and_then(|identity| identity.kind);
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

    let actual_version = userspace.and_then(|identity| identity.version_id.as_deref());
    for (bound, is_minimum) in [
        (requirements.min_version.as_deref(), true),
        (requirements.max_version.as_deref(), false),
    ] {
        let Some(bound) = bound else { continue };
        match actual_version.and_then(|actual| compare_numeric_versions(actual, bound)) {
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
                break;
            }
        }
    }

    if requirements.min_build.is_some() || requirements.max_build.is_some() {
        let Some(actual) = userspace.and_then(|identity| identity.build) else {
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
        let userspace = environment.userspace.as_ref();
        let Some(id) = userspace.and_then(|identity| identity.id.as_deref()) else {
            reasons.push(CompatibilityReason::LinuxDistributionUnknown);
            return true;
        };
        let Some(alternative) = alternatives.iter().find(|entry| entry.id == id) else {
            reasons.push(CompatibilityReason::LinuxDistributionMismatch);
            return unknown;
        };
        let Some(version) = userspace.and_then(|identity| identity.version_id.as_deref()) else {
            reasons.push(CompatibilityReason::LinuxDistributionUnknown);
            return true;
        };
        if !alternative
            .versions
            .iter()
            .any(|allowed| allowed == version)
        {
            reasons.push(CompatibilityReason::LinuxDistributionVersionMismatch);
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
    use gamenode_platform::plugin_environment::{HostIdentity, LibcIdentity, UserspaceIdentity};
    use std::collections::BTreeMap;

    fn linux_target(arch: &str) -> Target {
        Target {
            os: "linux".into(),
            arch: arch.into(),
            abi: "gnu".into(),
            entrypoint: "bin/plugin".into(),
            requires: None,
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

    const DOCUMENTED_MANIFEST: &[u8] =
        include_bytes!("../../../../docs/fixtures/plugin-target-compatibility.json");

    fn assert_documented_verdict(
        entrypoint: &str,
        environment: &PluginEnvironment,
        state: CompatibilityState,
        reasons: &[CompatibilityReason],
    ) {
        let assessments = assess_manifest_targets(
            DOCUMENTED_MANIFEST,
            environment,
            EmulationPolicy::RequireNative,
        )
        .unwrap();
        let assessment = assessments
            .iter()
            .find(|assessment| assessment.target.entrypoint == entrypoint)
            .unwrap();
        assert_eq!(
            assessment.verdict,
            CompatibilityVerdict {
                state,
                reasons: reasons.to_vec(),
                unknown_facts: environment.unknown_facts.clone(),
            },
            "{entrypoint}: {environment:?}",
        );
    }

    #[test]
    fn documented_glibc_target_requires_libc_and_an_exact_distribution_alternative() {
        use CompatibilityReason::*;
        use CompatibilityState::*;

        let mut environment = linux_environment();
        environment.process_architecture = Some("amd64".into());
        environment
            .userspace
            .as_mut()
            .unwrap()
            .extra
            .insert("ID_LIKE".into(), "debian".into());
        for (id, version, libc, state, reason) in [
            (
                "debian",
                Some("13"),
                "2.36",
                Compatible,
                RequirementsSatisfied,
            ),
            (
                "debian",
                Some("13.1"),
                "2.36",
                Compatible,
                RequirementsSatisfied,
            ),
            (
                "fedora",
                Some("43"),
                "2.40",
                Compatible,
                RequirementsSatisfied,
            ),
            (
                "debian",
                Some("13.0"),
                "2.36",
                Incompatible,
                LinuxDistributionVersionMismatch,
            ),
            (
                "debian",
                Some("12"),
                "2.36",
                Incompatible,
                LinuxDistributionVersionMismatch,
            ),
            (
                "ubuntu",
                Some("24.04"),
                "2.36",
                Incompatible,
                LinuxDistributionMismatch,
            ),
            (
                "ubuntu",
                None,
                "2.36",
                Incompatible,
                LinuxDistributionMismatch,
            ),
            (
                "debian",
                None,
                "2.36",
                Indeterminate,
                LinuxDistributionUnknown,
            ),
            (
                "debian",
                Some("13"),
                "2.35",
                Incompatible,
                LinuxLibcVersionTooOld,
            ),
        ] {
            let userspace = environment.userspace.as_mut().unwrap();
            userspace.id = Some(id.into());
            userspace.version_id = version.map(str::to_owned);
            environment.libc = Some(LibcIdentity::Gnu(Some(libc.into())));
            assert_documented_verdict("bin/linux-gnu-x64/plugin", &environment, state, &[reason]);
        }
    }

    #[test]
    fn documented_interpreted_target_rechecks_native_dependency_without_an_allowlist() {
        use CompatibilityReason::*;
        use CompatibilityState::*;

        let mut environment = linux_environment();
        environment.userspace = None;
        environment.process_abi = Some("musl".into());
        environment.unknown_facts =
            vec!["distribution.id".into(), "distribution.version_id".into()];
        let entrypoint = "bin/linux-musl-x64/python-plugin";
        for (libc, state, reason) in [
            (Some("1.2.4"), Compatible, RequirementsSatisfied),
            (Some("1.2.10"), Compatible, RequirementsSatisfied),
            (Some("1.2.3"), Incompatible, LinuxLibcVersionTooOld),
            (None, Indeterminate, LinuxLibcUnknown),
        ] {
            environment.libc = Some(LibcIdentity::Musl(libc.map(str::to_owned)));
            assert_documented_verdict(entrypoint, &environment, state, &[reason]);
        }

        // Hold the target ABI fact constant to isolate the libc-family comparison.
        environment.libc = Some(LibcIdentity::Gnu(Some("2.36".into())));
        assert_documented_verdict(entrypoint, &environment, Incompatible, &[LinuxLibcMismatch]);
        assert_documented_verdict(
            entrypoint,
            &linux_environment(),
            Incompatible,
            &[AbiMismatch],
        );
    }

    #[test]
    fn incompatible_and_indeterminate_are_distinct() {
        let mismatch = evaluate_target(
            &linux_target("x86_64"),
            &linux_environment(),
            EmulationPolicy::RequireNative,
        );
        assert_eq!(mismatch.state, CompatibilityState::Compatible);
        let incompatible = evaluate_target(
            &linux_target("aarch64"),
            &linux_environment(),
            EmulationPolicy::RequireNative,
        );
        assert_eq!(incompatible.state, CompatibilityState::Incompatible);
        let unknown = PluginEnvironment {
            unknown_facts: vec!["architecture".into()],
            ..Default::default()
        };
        let indeterminate = evaluate_target(
            &linux_target("x86_64"),
            &unknown,
            EmulationPolicy::AllowUnknown,
        );
        assert_eq!(indeterminate.state, CompatibilityState::Indeterminate);
        assert_eq!(indeterminate.unknown_facts, vec!["architecture".to_owned()]);
    }

    #[test]
    fn tested_coverage_is_not_a_runtime_requirement_and_emulation_is_explicit() {
        let mut target = linux_target("x86_64");
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
            "api":{"min": "1.6.0", "max": "1.6.0"},
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
    fn documented_windows_target_checks_product_version_and_inclusive_build_bounds() {
        use CompatibilityReason::*;
        use CompatibilityState::*;
        use OperatingSystemKind::{Client, Server, Unknown};

        let mut environment = windows_environment();
        let entrypoint = "bin/windows-x64/plugin.exe";
        for (product, version, build, state, reason) in [
            (
                Client,
                Some("10.0.22000"),
                Some(22000),
                Compatible,
                RequirementsSatisfied,
            ),
            (
                Client,
                Some("10.0.26100"),
                Some(26100),
                Compatible,
                RequirementsSatisfied,
            ),
            (
                Server,
                Some("10.0.22631"),
                Some(22631),
                Incompatible,
                WindowsProductMismatch,
            ),
            (
                Client,
                Some("6.3"),
                Some(22000),
                Incompatible,
                WindowsVersionTooOld,
            ),
            (
                Client,
                Some("10.0.19045"),
                Some(19045),
                Incompatible,
                WindowsBuildTooOld,
            ),
            (
                Client,
                Some("10.0.26101"),
                Some(26101),
                Incompatible,
                WindowsBuildTooNew,
            ),
            (
                Client,
                Some("10.0"),
                None,
                Indeterminate,
                WindowsBuildUnknown,
            ),
            (
                Unknown,
                Some("10.0"),
                Some(22631),
                Indeterminate,
                WindowsProductUnknown,
            ),
            (
                Client,
                None,
                Some(22631),
                Indeterminate,
                WindowsVersionUnknown,
            ),
        ] {
            let userspace = environment.userspace.as_mut().unwrap();
            userspace.kind = Some(product);
            userspace.version_id = version.map(str::to_owned);
            userspace.build = build;
            assert_documented_verdict(entrypoint, &environment, state, &[reason]);
        }

        let userspace = environment.userspace.as_mut().unwrap();
        userspace.build = Some(19045);
        assert_documented_verdict(
            entrypoint,
            &environment,
            Incompatible,
            &[WindowsVersionUnknown, WindowsBuildTooOld],
        );
    }

    #[test]
    fn manifest_assessment_rejects_unknown_selectors_and_ambiguous_targets() {
        let manifest: serde_json::Value = serde_json::from_slice(DOCUMENTED_MANIFEST).unwrap();
        let mut ambiguous = manifest.clone();
        let mut server = ambiguous["targets"][0].clone();
        server["entrypoint"] = "bin/windows-x64/server.exe".into();
        server["requires"]["windows"]["product"] = "server".into();
        ambiguous["targets"].as_array_mut().unwrap().push(server);

        let mut malformed = manifest;
        malformed["targets"][0]["requires"]["windows"]["release"] = "23H2".into();

        for (manifest, expected) in [
            (ambiguous, "ambiguous target selection"),
            (malformed, "unknown field `release`"),
        ] {
            let error = assess_manifest_targets(
                &serde_json::to_vec(&manifest).unwrap(),
                &windows_environment(),
                EmulationPolicy::RequireNative,
            )
            .unwrap_err();
            let PackageCompatibilityError::InvalidManifest(error) = error else {
                panic!("expected manifest rejection, got {error:?}");
            };
            assert!(error.to_string().contains(expected), "{error}");
        }
    }
}
