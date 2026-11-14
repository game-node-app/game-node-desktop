//! Opaque, tamper-proof tokens the host hands to plugins in place of its own identifiers.
//!
//! A token seals a small payload with a key that never leaves the process, so a plugin cannot
//! read what it holds, change it or build one. Each token is bound to a purpose and to a
//! context string (a plugin session), and only opens with the same ones: a token issued to
//! one session is meaningless to another, and a cursor is not a handle. Tokens are random per
//! issue, so two for the same payload differ.

use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

/// Issues and opens sealed tokens with one random key.
pub struct Sealer {
    key: LessSafeKey,
    random: SystemRandom,
}

impl Default for Sealer {
    fn default() -> Self {
        Self::new()
    }
}

impl Sealer {
    /// A sealer with a fresh random key. Tokens do not survive the process.
    pub fn new() -> Self {
        let random = SystemRandom::new();
        let mut key = [0u8; 32];
        random
            .fill(&mut key)
            .expect("the system random source is available");
        let key = LessSafeKey::new(
            UnboundKey::new(&CHACHA20_POLY1305, &key).expect("a 32-byte ChaCha20-Poly1305 key"),
        );
        Self { key, random }
    }

    fn aad(purpose: u8, binding: &str) -> Vec<u8> {
        let mut aad = Vec::with_capacity(binding.len() + 1);
        aad.push(purpose);
        aad.extend_from_slice(binding.as_bytes());
        aad
    }

    /// Seals `payload` for `purpose` and `binding`, as lowercase hex text.
    pub fn seal(&self, purpose: u8, binding: &str, payload: &[u8]) -> String {
        let mut nonce = [0u8; NONCE_LEN];
        self.random
            .fill(&mut nonce)
            .expect("the system random source is available");
        let mut buffer = payload.to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(Self::aad(purpose, binding)),
                &mut buffer,
            )
            .expect("sealing in memory cannot fail");
        let mut token = Vec::with_capacity(NONCE_LEN + buffer.len());
        token.extend_from_slice(&nonce);
        token.extend_from_slice(&buffer);
        hex(&token)
    }

    /// The payload of `token` if it was sealed by this sealer for the same purpose and binding.
    pub fn open(&self, purpose: u8, binding: &str, token: &str) -> Option<Vec<u8>> {
        let bytes = unhex(token)?;
        if bytes.len() < NONCE_LEN + CHACHA20_POLY1305.tag_len() {
            return None;
        }
        let (nonce, sealed) = bytes.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
        let mut buffer = sealed.to_vec();
        let opened = self
            .key
            .open_in_place(nonce, Aad::from(Self::aad(purpose, binding)), &mut buffer)
            .ok()?;
        Some(opened.to_vec())
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    text
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    bytes
        .chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_opens_only_for_its_purpose_and_binding() {
        let sealer = Sealer::new();
        let token = sealer.seal(1, "session-a", b"payload");
        assert_eq!(
            sealer.open(1, "session-a", &token).as_deref(),
            Some(&b"payload"[..])
        );
        assert_eq!(sealer.open(2, "session-a", &token), None, "another purpose");
        assert_eq!(sealer.open(1, "session-b", &token), None, "another session");
        assert_eq!(
            Sealer::new().open(1, "session-a", &token),
            None,
            "another key"
        );
    }

    #[test]
    fn tokens_are_random_and_do_not_show_their_payload() {
        let sealer = Sealer::new();
        let (a, b) = (
            sealer.seal(1, "s", b"secret/path"),
            sealer.seal(1, "s", b"secret/path"),
        );
        assert_ne!(a, b);
        assert!(
            !a.contains("736563726574"),
            "the payload is not in the token"
        );
        assert!(a.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn damaged_or_forged_tokens_open_nothing() {
        let sealer = Sealer::new();
        let token = sealer.seal(1, "s", b"payload");
        for bad in [
            "".to_owned(),
            "zz".to_owned(),
            "abc".to_owned(),
            token[..token.len() - 2].to_owned(),
            format!("{token}00"),
            token.to_uppercase(),
        ] {
            assert_eq!(sealer.open(1, "s", &bad), None, "{bad}");
        }
        // Flipping any single hex digit breaks authentication.
        for index in (0..token.len()).step_by(7) {
            let mut bytes = token.clone().into_bytes();
            bytes[index] = if bytes[index] == b'0' { b'1' } else { b'0' };
            assert_eq!(
                sealer.open(1, "s", &String::from_utf8(bytes).unwrap()),
                None
            );
        }
    }
}
