//! Messages exchanged between launches of GameNode Desktop.
//!
//! Frame: a 4-byte big-endian length followed by that many bytes of JSON. The owner answers
//! with a single byte: [`ACK`] if the message was accepted, [`NACK`] otherwise. Everything is
//! bounded and strictly typed; the payload is data and is never passed to a shell.

use gamenode_core::callback::MAX_CALLBACK_LEN;
use serde::{Deserialize, Serialize};

/// Largest accepted frame body, comfortably above the largest valid callback.
pub const MAX_FRAME: usize = MAX_CALLBACK_LEN + 512;
pub const ACK: u8 = 1;
pub const NACK: u8 = 0;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Message {
    /// An ordinary launch: show the existing window.
    Activate,
    /// An `app.gamenode://` URL from the OS handler. It is validated again by the owner.
    AuthCallback { url: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("message too large")]
    TooLarge,
    #[error("message malformed")]
    Malformed,
}

pub fn encode(message: &Message) -> Vec<u8> {
    let body = serde_json::to_vec(message).expect("messages serialize");
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    frame
}

/// Validates a frame header and returns the body length.
pub fn body_len(header: [u8; 4]) -> Result<usize, ProtocolError> {
    let len = u32::from_be_bytes(header) as usize;
    match len {
        0 => Err(ProtocolError::Malformed),
        l if l > MAX_FRAME => Err(ProtocolError::TooLarge),
        l => Ok(l),
    }
}

pub fn decode(body: &[u8]) -> Result<Message, ProtocolError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| ProtocolError::Malformed)?;
    // serde cannot reject unknown fields on unit variants of internally tagged enums, so the
    // exact shape is checked here: `type` plus only the fields the variant defines.
    let field_count = value
        .as_object()
        .map(|o| o.len())
        .ok_or(ProtocolError::Malformed)?;
    let message: Message = serde_json::from_value(value).map_err(|_| ProtocolError::Malformed)?;
    let expected = match message {
        Message::Activate => 1,
        Message::AuthCallback { .. } => 2,
    };
    if field_count != expected {
        return Err(ProtocolError::Malformed);
    }
    if let Message::AuthCallback { url } = &message
        && url.len() > MAX_CALLBACK_LEN
    {
        return Err(ProtocolError::TooLarge);
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_both_messages() {
        for message in [
            Message::Activate,
            Message::AuthCallback {
                url: "app.gamenode://home?x=a b&c=%E2%9C%93\"; rm -rf /".into(),
            },
        ] {
            let frame = encode(&message);
            let len = body_len(frame[..4].try_into().unwrap()).unwrap();
            assert_eq!(len, frame.len() - 4);
            assert_eq!(decode(&frame[4..]).unwrap(), message);
        }
    }

    #[test]
    fn rejects_bad_lengths_and_bodies() {
        assert_eq!(body_len([0, 0, 0, 0]), Err(ProtocolError::Malformed));
        assert_eq!(
            body_len(u32::MAX.to_be_bytes()),
            Err(ProtocolError::TooLarge)
        );
        assert_eq!(
            body_len((MAX_FRAME as u32 + 1).to_be_bytes()),
            Err(ProtocolError::TooLarge)
        );
        for body in [
            &b"not json"[..],
            br#"{"type":"unknown"}"#,
            br#"{"type":"activate","extra":1}"#,
            br#"{"type":"auth-callback"}"#,
            br#"{"type":"auth-callback","url":5}"#,
            b"[]",
            &[0xff, 0xfe],
        ] {
            assert_eq!(decode(body), Err(ProtocolError::Malformed), "{body:?}");
        }
        let huge = format!(
            r#"{{"type":"auth-callback","url":"{}"}}"#,
            "a".repeat(MAX_CALLBACK_LEN + 1)
        );
        assert_eq!(decode(huge.as_bytes()), Err(ProtocolError::TooLarge));
    }
}
