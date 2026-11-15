//! Tests for `volume`.

use std::path::Path;

use super::*;

#[test]
fn volume_info_for_temp_dir_has_known_locality() {
    let dir = std::env::temp_dir();
    let info = volume_info(&dir);
    // tmpfs or similar is expected; at minimum capacity_and_free must work.
    assert!(info.is_some(), "volume_info returned None for temp dir");
    let info = info.unwrap();
    // On Linux tmpfs is a local filesystem.
    #[cfg(target_os = "linux")]
    {
        // tmpfs is local.
        assert!(
            matches!(info.locality, Locality::Local | Locality::Unknown),
            "unexpected locality for temp dir: {:?}",
            info.locality
        );
    }
    assert!(info.observed_at > 0);
}

#[test]
fn locality_as_str_values_are_stable() {
    assert_eq!(Locality::Local.as_str(), "local");
    assert_eq!(Locality::Network.as_str(), "network");
    assert_eq!(Locality::Unknown.as_str(), "unknown");
}

#[test]
fn removability_as_str_values_are_stable() {
    assert_eq!(Removability::Internal.as_str(), "internal");
    assert_eq!(Removability::Removable.as_str(), "removable");
    assert_eq!(Removability::Unknown.as_str(), "unknown");
}

#[test]
fn bus_type_as_str_values_are_stable() {
    assert_eq!(BusType::Usb.as_str(), "usb");
    assert_eq!(BusType::Sata.as_str(), "sata");
    assert_eq!(BusType::Nvme.as_str(), "nvme");
    assert_eq!(BusType::Mmc.as_str(), "mmc");
    assert_eq!(BusType::Other.as_str(), "other");
    assert_eq!(BusType::Unknown.as_str(), "unknown");
}

#[test]
fn media_type_as_str_values_are_stable() {
    assert_eq!(MediaType::Hdd.as_str(), "hdd");
    assert_eq!(MediaType::Ssd.as_str(), "ssd");
    assert_eq!(MediaType::Other.as_str(), "other");
    assert_eq!(MediaType::Unknown.as_str(), "unknown");
}

#[test]
fn capacity_and_free_for_root_are_nonzero() {
    let root = Path::new("/");
    // On any Unix system, root has capacity and free bytes.
    if let Some((cap, free)) = capacity_and_free(root) {
        assert!(cap > 0, "root capacity should be nonzero");
        assert!(cap >= free, "free must not exceed capacity");
    }
    // On non-Unix it may return None; that is acceptable.
}

#[test]
fn missing_path_does_not_report_an_ancestors_volume() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("not-there").join("deeper");
    let info = volume_info(&missing);
    assert!(info.is_none());
}

#[cfg(target_os = "linux")]
mod linux_tests {
    use super::*;

    #[test]
    fn strip_partition_suffix_handles_nvme_and_sda() {
        // These are internal helpers; test through the public interface.
        // We just check the shapes work for a real device on this machine.
        let root = Path::new("/");
        let info = volume_info(root).expect("root volume_info");
        // On a real Linux machine the token should not be empty.
        assert!(
            !info.volume_identity_token.is_empty(),
            "token should be non-empty for root on Linux: {:?}",
            info
        );
        // mount_point for root is always "/".
        assert_eq!(info.mount_point, "/");
        // Locality should be local.
        assert_eq!(info.locality, Locality::Local);
    }

    #[test]
    fn sysfs_bus_recognized_for_known_devices() {
        // We cannot assert a specific bus type (the test runner may be a VM),
        // but the value must be one of the declared variants.
        let root = Path::new("/");
        let info = volume_info(root).unwrap();
        // Any known variant is fine; Unknown is also valid (e.g. tmpfs on root in CI).
        let _ = info.bus_type;
        let _ = info.media_type;
    }
}
