//! Operating-system randomness.

/// A 256-bit random value as 64 lowercase hex characters, for single-use correlation values.
pub fn nonce() -> String {
    let mut bytes = [0u8; 32];
    // Failing to read OS randomness leaves nothing safe to fall back to.
    getrandom::fill(&mut bytes).expect("operating system randomness is unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// UUID v4 session id from OS randomness, independent of provider and account identifiers.
pub fn session_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("operating system randomness is unavailable");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn nonces_are_unique_url_safe_and_the_expected_length() {
        let (a, b) = (super::nonce(), super::nonce());
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }
}
