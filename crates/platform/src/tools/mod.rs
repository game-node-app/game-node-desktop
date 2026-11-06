//! Game tools platform adapters: release catalog & archive caching, target executable
//! inspection & anti-cheat folder scanning, and transactional file deployment with
//! byte-identical rollback.

pub mod catalog;
pub mod inspect;
pub mod installer;

pub use installer::PlatformToolInstaller;
