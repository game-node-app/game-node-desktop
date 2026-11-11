//! Read-only detection of the environment in which plugin processes would execute.
//!
//! This module never starts or inspects a candidate plugin. Detection reads the current
//! process environment and OS metadata only. Call [`detect`] from async code; all OS calls
//! and filesystem reads run on Tokio's blocking pool.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginEnvironment {
    /// Effective userspace identity (for example the container's os-release on Linux).
    pub userspace: Option<UserspaceIdentity>,
    /// Kernel or host facts visible from this process; these can differ from userspace.
    pub host: Option<HostIdentity>,
    pub process_architecture: Option<String>,
    pub process_abi: Option<String>,
    pub libc: Option<LibcIdentity>,
    pub emulation: EmulationStatus,
    /// Facts unavailable to this detector remain explicit instead of receiving guesses.
    pub unknown_facts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserspaceIdentity {
    pub family: String,
    pub id: Option<String>,
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub product: Option<String>,
    pub major: Option<u32>,
    pub minor: Option<u32>,
    pub build: Option<u32>,
    pub kind: Option<OperatingSystemKind>,
    /// Unrecognized os-release keys are retained for future evaluators.
    pub extra: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostIdentity {
    pub family: String,
    pub version: Option<String>,
    pub architecture: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatingSystemKind {
    Client,
    Server,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "family", content = "version", rename_all = "snake_case")]
pub enum LibcIdentity {
    Gnu(Option<String>),
    Musl(Option<String>),
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmulationStatus {
    Native,
    Emulated,
    #[default]
    Unknown,
}

/// Caller-selected behavior for environments whose execution mode is emulated or unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmulationPolicy {
    RequireNative,
    AllowEmulated,
    AllowUnknown,
}

impl EmulationPolicy {
    pub fn accepts(self, status: &EmulationStatus) -> bool {
        match (self, status) {
            (_, EmulationStatus::Native) => true,
            (Self::AllowEmulated, EmulationStatus::Emulated) => true,
            (Self::AllowUnknown, EmulationStatus::Unknown) => true,
            _ => false,
        }
    }
}

/// Detects the current effective environment without blocking the async executor.
pub async fn detect() -> Result<PluginEnvironment, String> {
    tokio::task::spawn_blocking(detect_blocking)
        .await
        .map_err(|error| format!("plugin environment worker failed: {error}"))
        .and_then(|result| result)
}

fn detect_blocking() -> Result<PluginEnvironment, String> {
    #[cfg(target_os = "linux")]
    {
        detect_linux()
    }
    #[cfg(windows)]
    {
        detect_windows()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Ok(PluginEnvironment {
            unknown_facts: vec!["unsupported_host_platform".into()],
            ..Default::default()
        })
    }
}

#[cfg(target_os = "linux")]
fn detect_linux() -> Result<PluginEnvironment, String> {
    let mut unknown_facts = Vec::new();
    let os_release = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"));
    let userspace = match os_release {
        Ok(text) => {
            let identity = parse_os_release(&text);
            if identity.id.is_none() {
                unknown_facts.push("linux_distribution_id".into());
            }
            if identity.version_id.is_none() {
                unknown_facts.push("linux_distribution_version".into());
            }
            Some(identity)
        }
        Err(_) => {
            unknown_facts.push("linux_os_release".into());
            None
        }
    };
    let uts = rustix::system::uname();
    let kernel = uts.sysname().to_string_lossy().into_owned();
    let kernel_version = uts.release().to_string_lossy().into_owned();
    let kernel_arch = normalize_arch(&uts.machine().to_string_lossy());
    let process_arch = normalize_arch(std::env::consts::ARCH);
    let emulation = match (&process_arch, &kernel_arch) {
        (Some(process), Some(host)) if process == host => EmulationStatus::Native,
        (Some(_), Some(_)) => EmulationStatus::Emulated,
        _ => EmulationStatus::Unknown,
    };
    let (libc, abi) = detect_libc();
    if libc.is_none() {
        unknown_facts.push("libc_identity".into());
    } else if matches!(
        libc.as_ref(),
        Some(LibcIdentity::Gnu(None) | LibcIdentity::Musl(None))
    ) {
        unknown_facts.push("libc_version".into());
    }
    if abi.is_none() {
        unknown_facts.push("process_abi".into());
    }
    Ok(PluginEnvironment {
        userspace,
        host: Some(HostIdentity {
            family: kernel,
            version: Some(kernel_version),
            architecture: kernel_arch.clone(),
        }),
        process_architecture: process_arch,
        process_abi: abi,
        libc,
        emulation,
        unknown_facts,
    })
}

#[cfg(target_os = "linux")]
fn parse_os_release(input: &str) -> UserspaceIdentity {
    let mut fields = BTreeMap::new();
    for line in input.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            continue;
        }
        let value = value.trim();
        let value = if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        fields.insert(
            key.to_owned(),
            value.replace("\\\"", "\"").replace("\\\\", "\\"),
        );
    }
    let id = fields.remove("ID");
    let version_id = fields.remove("VERSION_ID");
    let name = fields.remove("NAME");
    let version = version_id.as_deref().unwrap_or_default();
    let mut parts = version.split(['.', '-']);
    let major = parts.next().and_then(|part| part.parse().ok());
    let minor = parts.next().and_then(|part| part.parse().ok());
    // ID_LIKE is retained as diagnostic ancestry only; it does not imply compatibility.
    let family = id.clone().unwrap_or_else(|| "unknown".into());
    UserspaceIdentity {
        family,
        id,
        version_id,
        name,
        product: None,
        major,
        minor,
        build: None,
        kind: None,
        extra: fields,
    }
}

#[cfg(target_os = "linux")]
fn detect_libc() -> (Option<LibcIdentity>, Option<String>) {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else {
        return (None, None);
    };
    let path = maps
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .find(|path| path.contains("libc.so") || path.contains("ld-musl"));
    let Some(path) = path else {
        return (None, None);
    };
    let name = path.rsplit('/').next().unwrap_or(path);
    let Ok(bytes) = std::fs::read(path) else {
        return (
            Some(if name.contains("musl") {
                LibcIdentity::Musl(None)
            } else {
                LibcIdentity::Unknown
            }),
            None,
        );
    };
    let version = find_version(
        &bytes,
        if name.contains("musl") {
            b"Version "
        } else {
            b"release version "
        },
    );
    let identity = if name.contains("musl") {
        LibcIdentity::Musl(version.clone())
    } else if name.contains("libc.so") {
        LibcIdentity::Gnu(version.clone())
    } else {
        LibcIdentity::Unknown
    };
    let abi = match &identity {
        LibcIdentity::Gnu(_) => Some("gnu".into()),
        LibcIdentity::Musl(_) => Some("musl".into()),
        LibcIdentity::Unknown => None,
    };
    (Some(identity), abi)
}

#[cfg(any(target_os = "linux", test))]
fn find_version(bytes: &[u8], marker: &[u8]) -> Option<String> {
    let start = bytes
        .windows(marker.len())
        .position(|window| window == marker)?
        + marker.len();
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    while end + 1 < bytes.len() && bytes[end] == b'.' && bytes[end + 1].is_ascii_digit() {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    let digits = bytes[start..end].to_vec();
    (!digits.is_empty()).then(|| String::from_utf8_lossy(&digits).into_owned())
}

#[cfg(any(target_os = "linux", test))]
fn normalize_arch(arch: &str) -> Option<String> {
    let lowercase = arch.to_ascii_lowercase();
    let normalized = match lowercase.as_str() {
        "x86_64" | "amd64" => "x86_64",
        "i386" | "i486" | "i586" | "i686" | "x86" => "x86",
        "aarch64" | "arm64" => "aarch64",
        "armv7l" | "armv7" => "armv7",
        "riscv64" => "riscv64",
        "s390x" => "s390x",
        other if !other.is_empty() => other,
        _ => return None,
    };
    Some(normalized.into())
}

#[cfg(windows)]
fn detect_windows() -> Result<PluginEnvironment, String> {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOEXW;
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OSVERSIONINFOEXW) -> i32;
    }
    let mut version = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
        ..Default::default()
    };
    // SAFETY: the struct is correctly sized and writable for the duration of the OS call.
    if unsafe { RtlGetVersion(&mut version) } < 0 {
        return Err("Windows version query failed".into());
    }
    let arch = windows_process_architecture();
    let process_architecture = arch.as_ref().map(|(process, _)| process.clone());
    let host_architecture = arch.as_ref().map(|(_, host)| host.clone());
    let emulation = match (&process_architecture, &host_architecture) {
        (Some(process), Some(host)) if process == host => EmulationStatus::Native,
        (Some(_), Some(_)) => EmulationStatus::Emulated,
        _ => EmulationStatus::Unknown,
    };
    let mut unknown_facts = Vec::new();
    if process_architecture.is_none() {
        unknown_facts.push("windows_process_architecture".into());
    }
    if host_architecture.is_none() {
        unknown_facts.push("windows_native_architecture".into());
    }
    if version.wProductType == 0 {
        unknown_facts.push("windows_product_kind".into());
    }
    let mut product_sku = 0u32;
    // SAFETY: scalar version inputs and writable output match the Win32 contract.
    let product = if unsafe {
        windows_sys::Win32::System::SystemInformation::GetProductInfo(
            version.dwMajorVersion,
            version.dwMinorVersion,
            version.wServicePackMajor as u32,
            version.wServicePackMinor as u32,
            &mut product_sku,
        )
    } != 0
    {
        Some(format!("Windows product SKU 0x{product_sku:04x}"))
    } else {
        unknown_facts.push("windows_product_sku".into());
        None
    };
    let userspace = windows_userspace(
        version.dwMajorVersion,
        version.dwMinorVersion,
        version.dwBuildNumber,
        version.wProductType,
        product,
    );
    let abi = if cfg!(target_env = "msvc") {
        Some("msvc".into())
    } else if cfg!(target_env = "gnu") {
        Some("gnu".into())
    } else {
        unknown_facts.push("windows_abi".into());
        None
    };
    Ok(PluginEnvironment {
        userspace: Some(userspace),
        host: Some(HostIdentity {
            family: "windows_nt_kernel".into(),
            version: Some(format!(
                "{}.{}",
                version.dwMajorVersion, version.dwMinorVersion
            )),
            architecture: host_architecture,
        }),
        process_architecture,
        process_abi: abi,
        libc: None,
        emulation,
        unknown_facts,
    })
}

#[cfg(any(windows, test))]
fn windows_userspace(
    major: u32,
    minor: u32,
    build: u32,
    product_type: u8,
    product: Option<String>,
) -> UserspaceIdentity {
    let kind = match product_type {
        1 => OperatingSystemKind::Client,
        2 | 3 => OperatingSystemKind::Server,
        _ => OperatingSystemKind::Unknown,
    };
    UserspaceIdentity {
        family: "windows".into(),
        id: None,
        version_id: Some(format!("{major}.{minor}.{build}")),
        name: Some(
            if kind == OperatingSystemKind::Server {
                "Windows Server"
            } else {
                "Windows"
            }
            .into(),
        ),
        product,
        major: Some(major),
        minor: Some(minor),
        build: Some(build),
        kind: Some(kind),
        extra: BTreeMap::new(),
    }
}

#[cfg(windows)]
fn windows_process_architecture() -> Option<(String, String)> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
    let mut process_machine = 0u16;
    let mut native_machine = 0u16;
    // SAFETY: valid pseudo-handle and writable outputs supplied to the OS API.
    let ok = unsafe {
        IsWow64Process2(
            GetCurrentProcess(),
            &mut process_machine,
            &mut native_machine,
        )
    };
    if ok == 0 {
        return None;
    }
    let native = machine_name(native_machine)?;
    let process = if process_machine == 0 {
        native.clone()
    } else {
        machine_name(process_machine)?
    };
    Some((process, native))
}

#[cfg(windows)]
fn machine_name(machine: u16) -> Option<String> {
    match machine {
        0x014c => Some("x86".into()),
        0x8664 => Some("x86_64".into()),
        0xaa64 => Some("aarch64".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_release_identity_and_preserves_unknown_fields() {
        let identity = parse_os_release(
            "ID=debian\nNAME=\"Debian GNU/Linux\"\nVERSION_ID=\"12.4\"\nBUILD_ID=custom\n",
        );
        assert_eq!(identity.id.as_deref(), Some("debian"));
        assert_eq!(identity.name.as_deref(), Some("Debian GNU/Linux"));
        assert_eq!(identity.major, Some(12));
        assert_eq!(identity.minor, Some(4));
        assert_eq!(
            identity.extra.get("BUILD_ID").map(String::as_str),
            Some("custom")
        );
    }

    #[test]
    fn missing_os_release_values_remain_unknown() {
        let identity = parse_os_release("PRETTY_NAME=\"Future Linux\"\n");
        assert_eq!(identity.family, "unknown");
        assert_eq!(identity.id, None);
        assert_eq!(identity.version_id, None);
        assert_eq!(
            identity.extra.get("PRETTY_NAME").map(String::as_str),
            Some("Future Linux")
        );
    }

    #[test]
    fn emulation_must_follow_the_explicit_callers_policy() {
        assert!(!EmulationPolicy::RequireNative.accepts(&EmulationStatus::Emulated));
        assert!(EmulationPolicy::AllowEmulated.accepts(&EmulationStatus::Emulated));
        assert!(!EmulationPolicy::RequireNative.accepts(&EmulationStatus::Unknown));
        assert!(EmulationPolicy::AllowUnknown.accepts(&EmulationStatus::Unknown));
    }

    #[test]
    fn normalizes_architecture_aliases_without_guessing_empty_values() {
        assert_eq!(normalize_arch("AMD64").as_deref(), Some("x86_64"));
        assert_eq!(normalize_arch("aarch64").as_deref(), Some("aarch64"));
        assert_eq!(normalize_arch("").as_deref(), None);
    }

    #[test]
    fn windows_client_and_server_fixture_preserves_build_and_product() {
        let client = windows_userspace(10, 0, 22631, 1, Some("Windows product SKU 0x0030".into()));
        assert_eq!(client.kind, Some(OperatingSystemKind::Client));
        assert_eq!(client.build, Some(22631));
        assert_eq!(
            client.product.as_deref(),
            Some("Windows product SKU 0x0030")
        );
        let server = windows_userspace(10, 0, 20348, 3, None);
        assert_eq!(server.kind, Some(OperatingSystemKind::Server));
        assert_eq!(server.name.as_deref(), Some("Windows Server"));
        assert_eq!(server.build, Some(20348));
    }

    #[test]
    fn recognizes_libc_versions_from_mapped_binary_metadata() {
        assert_eq!(
            find_version(b"...release version 2.37 GNU...", b"release version ").as_deref(),
            Some("2.37")
        );
        assert_eq!(
            find_version(b"...Version 1.2.4...", b"Version ").as_deref(),
            Some("1.2.4")
        );
        assert_eq!(
            find_version(b"no version metadata", b"release version "),
            None
        );
    }
}
