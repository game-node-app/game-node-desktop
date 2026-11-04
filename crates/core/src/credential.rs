//! Session credential and storage-method types.

use std::fmt;

/// The signed Better Auth session value, used as a bearer token.
///
/// It is deliberately opaque: `Debug` and `Display` never reveal it, so logs and errors cannot
/// leak it by accident. Call [`SessionCredential::expose`] only to send or store it.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionCredential(String);

impl SessionCredential {
    /// Wraps a credential value. Returns `None` for an empty value.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty()).then_some(Self(value))
    }

    /// The raw value. Never log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SessionCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionCredential(<redacted>)")
    }
}

/// Where the session credential is kept. Settings shows this; it never carries the credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageMethod {
    /// Windows Credential Manager.
    WindowsCredentialManager,
    /// A Secret Service implementation (GNOME Keyring, KWallet, ...) on Linux.
    SecretService,
    /// Linux only, when no Secret Service exists: a file readable only by the current user.
    PrivateFile,
}

impl StorageMethod {
    /// Whether this is the user-only file fallback that Settings must disclose.
    pub fn is_fallback_file(self) -> bool {
        matches!(self, Self::PrivateFile)
    }
}
