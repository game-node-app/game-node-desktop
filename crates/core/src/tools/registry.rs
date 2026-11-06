//! Declarative game tools registry, channels, install strategies, and descriptors.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Strongly-typed identifier for a game tool (e.g. `"optiscaler"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ToolId(pub String);

impl ToolId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for ToolId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for ToolId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for ToolId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ToolId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ToolId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Release channel for a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ToolChannel {
    Stable,
    Nightly,
}

impl ToolChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Nightly => "nightly",
        }
    }
}

impl fmt::Display for ToolChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stable => write!(f, "Stable"),
            Self::Nightly => write!(f, "Nightly"),
        }
    }
}

impl FromStr for ToolChannel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "stable" => Ok(Self::Stable),
            "nightly" => Ok(Self::Nightly),
            other => Err(format!("Unknown tool channel: {other}")),
        }
    }
}

/// Format of the tool release archive asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ArchiveFormat {
    SevenZip,
    Zip,
}

impl ArchiveFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SevenZip => ".7z",
            Self::Zip => ".zip",
        }
    }
}

impl fmt::Display for ArchiveFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Ordered proxy DLL names supported by OptiScaler and standard DLL injectors.
///
/// Order matches the OptiScaler wiki recommendation hierarchy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProxyDllName {
    #[default]
    Dxgi,
    Winmm,
    Version,
    D3d12,
    Dbghelp,
    Wininet,
    Winhttp,
}

impl ProxyDllName {
    /// All 7 proxy DLL names in wiki priority order.
    pub const ALL: &'static [ProxyDllName] = &[
        ProxyDllName::Dxgi,
        ProxyDllName::Winmm,
        ProxyDllName::Version,
        ProxyDllName::D3d12,
        ProxyDllName::Dbghelp,
        ProxyDllName::Wininet,
        ProxyDllName::Winhttp,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dxgi => "dxgi.dll",
            Self::Winmm => "winmm.dll",
            Self::Version => "version.dll",
            Self::D3d12 => "d3d12.dll",
            Self::Dbghelp => "dbghelp.dll",
            Self::Wininet => "wininet.dll",
            Self::Winhttp => "winhttp.dll",
        }
    }

    pub fn parse_flexible(s: &str) -> Option<Self> {
        let clean = s.trim().to_lowercase();
        let stem = clean.strip_suffix(".dll").unwrap_or(&clean);
        match stem {
            "dxgi" => Some(Self::Dxgi),
            "winmm" => Some(Self::Winmm),
            "version" => Some(Self::Version),
            "d3d12" => Some(Self::D3d12),
            "dbghelp" => Some(Self::Dbghelp),
            "wininet" => Some(Self::Wininet),
            "winhttp" => Some(Self::Winhttp),
            _ => None,
        }
    }
}

impl fmt::Display for ProxyDllName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ProxyDllName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_flexible(s).ok_or_else(|| format!("Unknown proxy DLL: {s}"))
    }
}

/// A declarative mutation applied to an INI config file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IniPatch {
    pub config_file: String,
    pub section: String,
    pub key: String,
    pub value: String,
}

/// Target resolution mode for sibling library swappers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwapperMode {
    /// Replaces matching target DLLs anywhere in the game's native installation directory.
    NativeInstallDir,
    /// Prefers replacing/providing matching companion DLLs in an installed host tool's target directory,
    /// falling back to matching native files in the game installation directory.
    CompanionOrNative { host_tool_ids: Vec<ToolId> },
}

/// Deployment mode for addons/plugins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddonMode {
    /// Deploys as a plugin to an installed host tool (such as OptiScaler).
    HostPlugin {
        host_tool_id: ToolId,
        plugin_dir: String,
        extension: String,
        ini_patches: Vec<IniPatch>,
    },
    /// Supports falling back to standalone proxy injection when no host tool is present.
    StandaloneFallback {
        injector: Box<InstallStrategy>,
    },
}

/// Installation strategy governing archive planning, companion files, and rollback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallStrategy {
    /// Injects a primary DLL via Windows DLL proxy hijacking next to the game executable.
    ProxyDllInjector {
        main_dll_candidates: Vec<String>,
        supported_proxies: Vec<ProxyDllName>,
        default_proxy: ProxyDllName,
        preserved_configs: Vec<String>,
        ignored_extensions: Vec<String>,
        companion_dirs: Vec<String>,
    },
    /// Replaces or provides specific target SDK DLLs (e.g. DLSS, FSR 4 -> FSR 4 RDNA 2 Lite).
    SiblingLibrarySwapper {
        mode: SwapperMode,
        target_dll_names: Vec<String>,
    },
    /// Addon or plugin overlay attached to a host injector or running standalone.
    AddonOverlay {
        mode: AddonMode,
    },
}

/// Specification of a release channel for a tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolChannelSpec {
    pub channel: ToolChannel,
    pub github_repo: String,
    pub archive_format: ArchiveFormat,
}

/// Declarative descriptor representing a supported tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub id: ToolId,
    pub name: String,
    pub summary: String,
    pub license: String,
    pub source_url: String,
    pub channels: Vec<ToolChannelSpec>,
    pub strategy: InstallStrategy,
    pub requires_anticheat_check: bool,
}

impl ToolDescriptor {
    pub fn channel_spec(&self, channel: ToolChannel) -> Option<&ToolChannelSpec> {
        self.channels.iter().find(|c| c.channel == channel)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_str().trim().is_empty() {
            return Err("Tool id cannot be empty".to_string());
        }
        if self.name.trim().is_empty() {
            return Err("Tool name cannot be empty".to_string());
        }
        if self.license.trim().is_empty() {
            return Err("Tool license cannot be empty".to_string());
        }
        if self.channels.is_empty() {
            return Err("Tool must declare at least one channel".to_string());
        }
        match &self.strategy {
            InstallStrategy::ProxyDllInjector {
                main_dll_candidates,
                supported_proxies,
                default_proxy,
                ..
            } => {
                if main_dll_candidates.is_empty() {
                    return Err("ProxyDllInjector requires main_dll_candidates".to_string());
                }
                if supported_proxies.is_empty() {
                    return Err("ProxyDllInjector requires supported_proxies".to_string());
                }
                if !supported_proxies.contains(default_proxy) {
                    return Err("supported_proxies must contain default_proxy".to_string());
                }
            }
            InstallStrategy::SiblingLibrarySwapper {
                target_dll_names, ..
            } => {
                if target_dll_names.is_empty() {
                    return Err("SiblingLibrarySwapper requires target_dll_names".to_string());
                }
            }
            InstallStrategy::AddonOverlay { mode } => match mode {
                AddonMode::HostPlugin {
                    plugin_dir,
                    extension,
                    ..
                } => {
                    if plugin_dir.trim().is_empty() || extension.trim().is_empty() {
                        return Err("AddonMode::HostPlugin requires plugin_dir and extension".to_string());
                    }
                }
                AddonMode::StandaloneFallback { injector } => {
                    if let InstallStrategy::AddonOverlay { .. } = **injector {
                        return Err("AddonOverlay cannot nest inside itself".to_string());
                    }
                }
            },
        }
        Ok(())
    }
}

/// Built-in OptiScaler tool descriptor.
pub fn optiscaler_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::new("optiscaler"),
        name: "OptiScaler".to_string(),
        summary: "Universal upscaler and frame generation drop-in replacement with DLSS, FSR, and XeSS backends.".to_string(),
        license: "GPL-3.0".to_string(),
        source_url: "https://github.com/optiscaler/OptiScaler".to_string(),
        channels: vec![
            ToolChannelSpec {
                channel: ToolChannel::Stable,
                github_repo: "optiscaler/OptiScaler".to_string(),
                archive_format: ArchiveFormat::SevenZip,
            },
            ToolChannelSpec {
                channel: ToolChannel::Nightly,
                github_repo: "optiscaler/OptiScaler-nightly".to_string(),
                archive_format: ArchiveFormat::SevenZip,
            },
        ],
        strategy: InstallStrategy::ProxyDllInjector {
            main_dll_candidates: vec!["OptiScaler.dll".to_string(), "nvngx.dll".to_string()],
            supported_proxies: ProxyDllName::ALL.to_vec(),
            default_proxy: ProxyDllName::Dxgi,
            preserved_configs: vec!["OptiScaler.ini".to_string(), "fakenvapi.ini".to_string()],
            ignored_extensions: vec![
                ".bat".to_string(),
                ".cmd".to_string(),
                ".sh".to_string(),
                ".ps1".to_string(),
            ],
            companion_dirs: vec!["D3D12_OptiScaler".to_string(), "streamline".to_string()],
        },
        requires_anticheat_check: true,
    }
}

/// Helper descriptor for testing FSR 4 -> FSR 4 RDNA 2 Lite swapper.
pub fn fsr4_rdna2_lite_test_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::new("fsr4-rdna2-lite"),
        name: "FSR 4 RDNA 2 Lite".to_string(),
        summary: "Lightweight FSR 4 replacement library optimized for RDNA 2/3 GPUs.".to_string(),
        license: "MIT".to_string(),
        source_url: "https://github.com/example/fsr4-rdna2-lite".to_string(),
        channels: vec![ToolChannelSpec {
            channel: ToolChannel::Stable,
            github_repo: "example/fsr4-rdna2-lite".to_string(),
            archive_format: ArchiveFormat::Zip,
        }],
        strategy: InstallStrategy::SiblingLibrarySwapper {
            mode: SwapperMode::CompanionOrNative {
                host_tool_ids: vec![ToolId::new("optiscaler")],
            },
            target_dll_names: vec![
                "amd_fidelityfx_dx12.dll".to_string(),
                "amd_fidelityfx_upscaler_dx12.dll".to_string(),
            ],
        },
        requires_anticheat_check: true,
    }
}

/// Helper descriptor for testing DLSS swapper.
pub fn dlss_swapper_test_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::new("dlss-swapper"),
        name: "DLSS Swapper".to_string(),
        summary: "Swap native DLSS/FrameGen DLLs in game folder.".to_string(),
        license: "GPL-3.0".to_string(),
        source_url: "https://github.com/example/dlss-swapper".to_string(),
        channels: vec![ToolChannelSpec {
            channel: ToolChannel::Stable,
            github_repo: "example/dlss-swapper".to_string(),
            archive_format: ArchiveFormat::Zip,
        }],
        strategy: InstallStrategy::SiblingLibrarySwapper {
            mode: SwapperMode::NativeInstallDir,
            target_dll_names: vec![
                "nvngx_dlss.dll".to_string(),
                "nvngx_dlssg.dll".to_string(),
                "nvngx_dlssd.dll".to_string(),
            ],
        },
        requires_anticheat_check: true,
    }
}

/// Helper descriptor for testing MFG Unlocker as an OptiScaler ASI plugin.
pub fn mfg_unlocker_test_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::new("mfg-unlocker"),
        name: "MFG Unlocker".to_string(),
        summary: "Multi-Frame Generation unlocker plugin for OptiScaler.".to_string(),
        license: "MIT".to_string(),
        source_url: "https://github.com/example/mfg-unlocker".to_string(),
        channels: vec![ToolChannelSpec {
            channel: ToolChannel::Stable,
            github_repo: "example/mfg-unlocker".to_string(),
            archive_format: ArchiveFormat::Zip,
        }],
        strategy: InstallStrategy::AddonOverlay {
            mode: AddonMode::HostPlugin {
                host_tool_id: ToolId::new("optiscaler"),
                plugin_dir: "plugins".to_string(),
                extension: "asi".to_string(),
                ini_patches: vec![IniPatch {
                    config_file: "OptiScaler.ini".to_string(),
                    section: "Plugins".to_string(),
                    key: "LoadAsiPlugins".to_string(),
                    value: "true".to_string(),
                }],
            },
        },
        requires_anticheat_check: true,
    }
}

/// Returns the built-in tool descriptors registry.
pub fn builtin_tools() -> Vec<ToolDescriptor> {
    vec![optiscaler_descriptor()]
}

/// Finds a built-in tool descriptor by ID.
pub fn find_tool(id: &str) -> Option<ToolDescriptor> {
    builtin_tools().into_iter().find(|t| t.id.as_str().eq_ignore_ascii_case(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optiscaler_descriptor_validates() {
        let opti = optiscaler_descriptor();
        assert!(opti.validate().is_ok());
        assert_eq!(opti.id.as_str(), "optiscaler");
        assert_eq!(opti.channels.len(), 2);
        assert_eq!(opti.channel_spec(ToolChannel::Stable).unwrap().archive_format, ArchiveFormat::SevenZip);
        assert_eq!(opti.channel_spec(ToolChannel::Nightly).unwrap().archive_format, ArchiveFormat::SevenZip);
        assert!(opti.requires_anticheat_check);
    }

    #[test]
    fn test_test_descriptors_validate() {
        assert!(fsr4_rdna2_lite_test_descriptor().validate().is_ok());
        assert!(dlss_swapper_test_descriptor().validate().is_ok());
        assert!(mfg_unlocker_test_descriptor().validate().is_ok());
    }

    #[test]
    fn test_proxy_dll_name_parsing_and_order() {
        assert_eq!(ProxyDllName::ALL.len(), 7);
        assert_eq!(ProxyDllName::ALL[0], ProxyDllName::Dxgi);
        assert_eq!(ProxyDllName::ALL[1], ProxyDllName::Winmm);
        assert_eq!(ProxyDllName::ALL[2], ProxyDllName::Version);
        assert_eq!(ProxyDllName::ALL[3], ProxyDllName::D3d12);
        assert_eq!(ProxyDllName::ALL[4], ProxyDllName::Dbghelp);
        assert_eq!(ProxyDllName::ALL[5], ProxyDllName::Wininet);
        assert_eq!(ProxyDllName::ALL[6], ProxyDllName::Winhttp);

        assert_eq!(ProxyDllName::parse_flexible("dxgi.dll"), Some(ProxyDllName::Dxgi));
        assert_eq!(ProxyDllName::parse_flexible("DXGI"), Some(ProxyDllName::Dxgi));
        assert_eq!(ProxyDllName::parse_flexible("winmm.dll"), Some(ProxyDllName::Winmm));
        assert_eq!(ProxyDllName::parse_flexible("unknown.dll"), None);
        assert_eq!(ProxyDllName::default(), ProxyDllName::Dxgi);
    }

    #[test]
    fn test_find_tool() {
        assert!(find_tool("optiscaler").is_some());
        assert!(find_tool("OptiScaler").is_some());
        assert!(find_tool("nonexistent").is_none());
    }
}
