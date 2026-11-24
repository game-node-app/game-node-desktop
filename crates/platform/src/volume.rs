//! Read-only volume observations for approved plugin resources (13.5).
//! Blocking: callers must use a worker. Linux reads mountinfo, sysfs and statvfs;
//! Windows reads volume GUIDs, drive type and capacity through Win32 metadata APIs.
//! Unknown dimensions remain unknown. Tokens hash filesystem UUIDs/volume GUIDs;
//! Linux falls back conservatively to a boot-scoped mount instance when no UUID is
//! visible. That fallback requires review after remount/reboot instead of trusting
//! a reused device number. Tokens expose no serial numbers.

use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Locality: whether the volume is local or on the network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locality {
    Local,
    Network,
    Unknown,
}

impl Locality {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Network => "network",
            Self::Unknown => "unknown",
        }
    }
}

/// Removability: whether the volume is permanently attached or removable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removability {
    Internal,
    External,
    Removable,
    Unknown,
}

impl Removability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::External => "external",
            Self::Removable => "removable",
            Self::Unknown => "unknown",
        }
    }
}

/// Bus type: the interface the volume is connected through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusType {
    Usb,
    Sata,
    Nvme,
    Mmc,
    Other,
    Unknown,
}

impl BusType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Usb => "usb",
            Self::Sata => "sata",
            Self::Nvme => "nvme",
            Self::Mmc => "mmc",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }
}

/// Media type: HDD (rotating disk), SSD, or unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaType {
    Hdd,
    Ssd,
    Other,
    Unknown,
}

impl MediaType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hdd => "hdd",
            Self::Ssd => "ssd",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }
}

/// Metadata about the volume that holds a path.
#[derive(Clone, Debug)]
pub struct VolumeInfo {
    /// An opaque token that identifies the volume (filesystem UUID or device number). Two paths
    /// on the same volume have the same token. A different disk at the same mount point has a
    /// different token.
    pub volume_identity_token: String,
    /// The mount point that this volume is at (best effort; empty if unknown).
    pub mount_point: String,
    pub locality: Locality,
    pub removability: Removability,
    pub bus_type: BusType,
    pub media_type: MediaType,
    /// Total capacity of the volume in bytes; `0` if unknown.
    pub capacity_bytes: u64,
    /// Free bytes available to the current user; `0` if unknown.
    pub free_bytes: u64,
    /// Unix seconds when the observation was made.
    pub observed_at: i64,
}

impl VolumeInfo {
    /// Returns an all-unknown `VolumeInfo` for `path`, filling in capacity/free from statvfs.
    #[allow(dead_code)]
    fn unknown(capacity_bytes: u64, free_bytes: u64) -> Self {
        Self {
            volume_identity_token: String::new(),
            mount_point: String::new(),
            locality: Locality::Unknown,
            removability: Removability::Unknown,
            bus_type: BusType::Unknown,
            media_type: MediaType::Unknown,
            capacity_bytes,
            free_bytes,
            observed_at: unix_now(),
        }
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Reads volume metadata for the volume that holds `path`. Blocking; call from a worker.
///
/// Returns `None` when capacity cannot be queried, including a missing path.
pub fn volume_info(path: &Path) -> Option<VolumeInfo> {
    // Resolve capacity/free first; they tell us the volume is reachable.
    let (capacity_bytes, free_bytes) = capacity_and_free(path)?;
    Some(volume_info_with_capacity(path, capacity_bytes, free_bytes))
}

/// Reads volume metadata when the caller already holds the capacity/free values.
pub fn volume_info_with_capacity(path: &Path, capacity_bytes: u64, free_bytes: u64) -> VolumeInfo {
    #[cfg(target_os = "linux")]
    {
        linux::volume_info_linux(path, capacity_bytes, free_bytes)
    }
    #[cfg(windows)]
    {
        windows_info(path, capacity_bytes, free_bytes)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = path;
        VolumeInfo::unknown(capacity_bytes, free_bytes)
    }
}

/// Returns `(capacity_bytes, free_bytes)` for the volume that holds `path`.
pub fn capacity_and_free(path: &Path) -> Option<(u64, u64)> {
    capacity_and_free_at(path)
}

#[cfg(unix)]
fn capacity_and_free_at(path: &Path) -> Option<(u64, u64)> {
    let stats = rustix::fs::statvfs(path).ok()?;
    let block = stats.f_frsize;
    let total = stats.f_blocks.saturating_mul(block);
    let free = stats.f_bavail.saturating_mul(block);
    Some((total, free))
}

#[cfg(windows)]
fn capacity_and_free_at(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut free, mut total) = (0, 0);
    // SAFETY: terminated path and writable counters live for this synchronous call.
    (unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) }
        != 0)
        .then_some((total, free))
}

#[cfg(not(any(unix, windows)))]
fn capacity_and_free_at(_path: &Path) -> Option<(u64, u64)> {
    None
}

#[cfg(windows)]
fn windows_info(path: &Path, capacity: u64, free: u64) -> VolumeInfo {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    };
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut mount = vec![0u16; 32768];
    let mut name = vec![0u16; 1024];
    let mut info = VolumeInfo::unknown(capacity, free);
    // SAFETY: buffers are writable, sized as passed, and input is terminated.
    if unsafe { GetVolumePathNameW(path.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0 {
        return info;
    }
    let len = mount.iter().position(|c| *c == 0).unwrap_or(0);
    info.mount_point = String::from_utf16_lossy(&mount[..len]);
    // SAFETY: mount is terminated and name has the advertised capacity.
    if unsafe {
        GetVolumeNameForVolumeMountPointW(mount.as_ptr(), name.as_mut_ptr(), name.len() as u32)
    } != 0
    {
        let len = name.iter().position(|c| *c == 0).unwrap_or(0);
        let bytes: Vec<_> = name[..len].iter().flat_map(|c| c.to_le_bytes()).collect();
        info.volume_identity_token = format!("volume:{:x}", Sha256::digest(bytes));
    }
    // SAFETY: mount is a terminated string.
    match unsafe { GetDriveTypeW(mount.as_ptr()) } {
        4 => info.locality = Locality::Network,
        2 => {
            info.locality = Locality::Local;
            info.removability = Removability::Removable;
        }
        3 | 5 | 6 => info.locality = Locality::Local,
        _ => {}
    }
    info
}

// ── Linux implementation ──────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    use super::*;

    /// Filesystem types that are network-backed.
    const NETWORK_FS: &[&str] = &[
        "nfs",
        "nfs4",
        "nfs3",
        "cifs",
        "smb3",
        "smbfs",
        "9p",
        "afs",
        "glusterfs",
        "ceph",
        "lustre",
        "beegfs",
        "gpfs",
        "ncpfs",
        "sshfs",
        "ftpfs",
        "davfs",
        "davfs2",
    ];

    pub(super) fn volume_info_linux(
        path: &Path,
        capacity_bytes: u64,
        free_bytes: u64,
    ) -> VolumeInfo {
        // Parse /proc/self/mountinfo for the best mount point and device name.
        let mounts = read_mountinfo().unwrap_or_default();
        let (mount_point, fs_type, dev_name, dev_major_minor) =
            best_mount_for(path, &mounts).unwrap_or_default();

        let locality = locality_from_fs(&fs_type);
        let (removability, bus_type, media_type) = if locality == Locality::Network {
            // Network volumes: physical medium is unknown.
            (Removability::Unknown, BusType::Unknown, MediaType::Unknown)
        } else {
            block_dimensions(&dev_name, &dev_major_minor)
        };

        let volume_identity_token = volume_token(&dev_name, &dev_major_minor, &mount_point);

        VolumeInfo {
            volume_identity_token,
            mount_point,
            locality,
            removability,
            bus_type,
            media_type,
            capacity_bytes,
            free_bytes,
            observed_at: unix_now(),
        }
    }

    /// One line of `/proc/self/mountinfo`.
    struct MountEntry {
        /// Major:minor of the device (e.g. `8:1`).
        dev_major_minor: String,
        /// The root of the mount within the filesystem.
        _fs_root: String,
        /// Where it is mounted in this process's view.
        mount_point: PathBuf,
        /// The filesystem type (e.g. `ext4`, `nfs`).
        fs_type: String,
        /// The source device or host (e.g. `/dev/sda1`, `server:/share`).
        source: String,
    }

    fn read_mountinfo() -> Option<Vec<MountEntry>> {
        let text = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
        parse_mountinfo(&text)
    }

    fn parse_mountinfo(text: &str) -> Option<Vec<MountEntry>> {
        let mut entries = Vec::new();
        for line in text.lines() {
            // Format: id parent_id major:minor fs_root mount_point opt... - fs_type source opt
            let mut parts = line.split_whitespace();
            let _id = parts.next()?;
            let _parent = parts.next()?;
            let dev_major_minor = parts.next()?.to_owned();
            let fs_root = parts.next()?.to_owned();
            let mount_point_str = parts.next()?;
            // Skip optional fields until the separator `-`
            let rest: Vec<&str> = parts.collect();
            let sep = rest.iter().position(|s| *s == "-")?;
            let after: &[&str] = &rest[sep + 1..];
            let fs_type = after.first()?.to_string();
            let source = after.get(1).unwrap_or(&"").to_string();
            entries.push(MountEntry {
                dev_major_minor,
                _fs_root: fs_root,
                mount_point: PathBuf::from(unescape_mount(mount_point_str)),
                fs_type,
                source,
            });
        }
        Some(entries)
    }

    /// Finds the most specific (longest) mount that is a prefix of `path`.
    fn best_mount_for(
        path: &Path,
        mounts: &[MountEntry],
    ) -> Option<(String, String, String, String)> {
        let canonical = std::fs::canonicalize(path)
            .ok()
            .unwrap_or_else(|| path.to_path_buf());
        let mut best: Option<&MountEntry> = None;
        let mut best_len = 0usize;
        for entry in mounts {
            if canonical.starts_with(&entry.mount_point) {
                let len = entry.mount_point.as_os_str().len();
                if len > best_len {
                    best_len = len;
                    best = Some(entry);
                }
            }
        }
        let entry = best?;
        let mount_point = entry.mount_point.to_str().unwrap_or("").to_owned();
        Some((
            mount_point,
            entry.fs_type.clone(),
            entry.source.clone(),
            entry.dev_major_minor.clone(),
        ))
    }

    fn locality_from_fs(fs_type: &str) -> Locality {
        // Fuse filesystems used by GVFS (gvfsd-fuse) may be local or remote; we cannot know.
        if NETWORK_FS.contains(&fs_type) || fs_type == "fuse.sshfs" {
            Locality::Network
        } else if fs_type.is_empty() || fs_type.starts_with("fuse") {
            Locality::Unknown
        } else {
            Locality::Local
        }
    }

    /// Derives the block device name from the source or device path, then reads
    /// /sys/block/<dev>/{queue/rotational,removable} and the symlink to find the bus.
    fn block_dimensions(source: &str, dev_major_minor: &str) -> (Removability, BusType, MediaType) {
        let dev_name = block_dev_name(source, dev_major_minor);
        let Some(dev_name) = dev_name else {
            return (Removability::Unknown, BusType::Unknown, MediaType::Unknown);
        };
        let rotational = read_sysfs_u8(&format!("/sys/block/{dev_name}/queue/rotational"));
        let removable = read_sysfs_u8(&format!("/sys/block/{dev_name}/removable"));
        let bus = bus_from_syslink(&dev_name);

        let media_type = match rotational {
            Some(1) => MediaType::Hdd,
            Some(0)
                if matches!(
                    bus,
                    BusType::Usb | BusType::Sata | BusType::Nvme | BusType::Mmc
                ) =>
            {
                MediaType::Ssd
            }
            _ => MediaType::Unknown,
        };
        let removability = match removable {
            Some(1) => Removability::Removable,
            Some(0) if bus == BusType::Usb => Removability::External,
            // Non-removable does not establish an internal connection.
            Some(0) => Removability::Unknown,
            _ => Removability::Unknown,
        };
        (removability, bus, media_type)
    }

    /// Returns the base block device name (e.g. `sda` from `/dev/sda1`, `nvme0n1`).
    fn block_dev_name(source: &str, dev_major_minor: &str) -> Option<String> {
        // If source is a /dev/... path, strip to base block device name by looking for
        // the device in /sys/block/. We try both the full name and the name without digits.
        if let Some(name) = source.strip_prefix("/dev/") {
            // Strip trailing partition suffix: sda1 → sda, nvme0n1p2 → nvme0n1.
            let base = strip_partition_suffix(name);
            if std::path::Path::new(&format!("/sys/block/{base}")).exists() {
                return Some(base.to_owned());
            }
        }
        // Fall back: look in /sys/block for a device whose `dev` file matches major:minor
        // (expensive but only reached for unusual setups like device-mapper).
        if !dev_major_minor.is_empty()
            && let Ok(entries) = std::fs::read_dir("/sys/block")
        {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let dev_path = format!("/sys/block/{name}/dev");
                if let Ok(content) = std::fs::read_to_string(&dev_path)
                    && content.trim() == dev_major_minor
                {
                    return Some(name);
                }
            }
        }
        None
    }

    fn strip_partition_suffix(name: &str) -> &str {
        // nvme0n1p2 → nvme0n1 (strip 'p' + digits at end when there's also 'n')
        if let Some(pos) = name.rfind('p')
            && pos > 0
            && name[pos + 1..].chars().all(char::is_numeric)
            && (name[..pos].starts_with("nvme") || name[..pos].starts_with("mmcblk"))
        {
            return &name[..pos];
        }
        // sda1 → sda (strip trailing digits)
        if name.starts_with("mmcblk") || name.starts_with("nvme") {
            return name;
        }
        let stripped = name.trim_end_matches(char::is_numeric);
        if !stripped.is_empty() && stripped.len() < name.len() {
            stripped
        } else {
            name
        }
    }

    fn read_sysfs_u8(path: &str) -> Option<u8> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
    }

    /// Reads the sysfs symlink for the block device and infers the bus type.
    fn bus_from_syslink(dev_name: &str) -> BusType {
        let link = match std::fs::read_link(format!("/sys/block/{dev_name}")) {
            Ok(l) => l.to_string_lossy().into_owned(),
            Err(_) => return BusType::Unknown,
        };
        // The symlink target contains bus identifiers in its path, e.g.:
        //   ../devices/pci0000:00/.../usb1/...  → USB
        //   ../devices/pci0000:00/.../ata1/...  → SATA/ATA
        //   ../devices/pci0000:00/.../nvme/...  → NVMe
        //   ../devices/pci0000:00/.../mmc/...   → MMC/SD
        let components: Vec<_> = link.split('/').collect();
        if components.iter().any(|c| c.starts_with("usb")) {
            return BusType::Usb;
        }
        if components.iter().any(|c| c.starts_with("nvme")) {
            return BusType::Nvme;
        }
        if components.iter().any(|c| c.starts_with("ata")) {
            return BusType::Sata;
        }
        if components.iter().any(|c| c.starts_with("mmc")) {
            return BusType::Mmc;
        }
        // A virtio or other PCI device that did not match anything specific.
        if link.contains("pci") || link.contains("virtio") {
            return BusType::Other;
        }
        BusType::Unknown
    }

    // A mount instance is deliberately conservative: after a reboot or remount, scope
    // approval requires review. A device number or mount path alone is never identity.
    fn volume_token(source: &str, device: &str, mount: &str) -> String {
        if let Ok(source) = std::fs::canonicalize(source)
            && let Ok(entries) = std::fs::read_dir("/dev/disk/by-uuid")
        {
            for entry in entries.flatten() {
                if std::fs::canonicalize(entry.path()).ok().as_ref() == Some(&source) {
                    return format!(
                        "fs:{:x}",
                        Sha256::digest(entry.file_name().as_encoded_bytes())
                    );
                }
            }
        }
        let Ok(boot) = std::fs::read_to_string("/proc/sys/kernel/random/boot_id") else {
            return String::new();
        };
        let Ok(text) = std::fs::read_to_string("/proc/self/mountinfo") else {
            return String::new();
        };
        for line in text.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() > 5 && fields[2] == device && unescape_mount(fields[4]) == mount {
                let input = format!("{}:{}:{}:{}", boot.trim(), fields[0], device, fields[3]);
                return format!("mount-instance:{:x}", Sha256::digest(input.as_bytes()));
            }
        }
        String::new()
    }

    fn unescape_mount(value: &str) -> String {
        value
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\")
    }
    #[cfg(test)]
    mod fixtures {
        use super::*;
        #[test]
        fn mount_paths_decode_escapes_and_match_components() {
            let entries = parse_mountinfo("1 0 8:1 / / rw - ext4 /dev/sda1 rw\n2 1 8:2 / /games\\040disk rw - ext4 /dev/sdb1 rw").unwrap();
            assert_eq!(
                best_mount_for(Path::new("/games disk/save"), &entries)
                    .unwrap()
                    .0,
                "/games disk"
            );
            assert_eq!(
                best_mount_for(Path::new("/games disks/save"), &entries)
                    .unwrap()
                    .0,
                "/"
            );
        }
        #[test]
        fn network_unknown_and_partition_shapes_remain_distinct() {
            assert_eq!(locality_from_fs("fuse.sshfs"), Locality::Network);
            assert_eq!(locality_from_fs("fuse.custom"), Locality::Unknown);
            assert_eq!(locality_from_fs(""), Locality::Unknown);
            for (input, expected) in [
                ("sda1", "sda"),
                ("nvme0n1", "nvme0n1"),
                ("nvme0n1p2", "nvme0n1"),
                ("mmcblk0p1", "mmcblk0"),
            ] {
                assert_eq!(strip_partition_suffix(input), expected);
            }
        }
    }
}

#[cfg(test)]
mod tests;
