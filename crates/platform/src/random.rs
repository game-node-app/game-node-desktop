//! Operating-system randomness.

/// A 256-bit random value as 64 lowercase hex characters, for single-use correlation values.
pub fn nonce() -> String {
    let mut bytes = [0u8; 32];
    // Failing to read OS randomness leaves nothing safe to fall back to.
    getrandom::fill(&mut bytes).expect("operating system randomness is unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
