// SPDX-License-Identifier: AGPL-3.0-or-later
//! Validated JSONL boundary for the optional TikTok LIVE connector runtime.
//!
//! This protocol layer has no process, account, audio, or publication side
//! effects. A supervisor and explicit UI confirmation are required to use it.

use serde_json::{Map, Value};

/// Protocol revision implemented by the bundled ArtCraft-compatible connector.
pub const PROTOCOL_VERSION: u64 = 1;
/// Maximum accepted JSONL frame size, matching the source runtime contract.
pub const MAX_FRAME_BYTES: usize = 32 * 1024;

/// A command that can be written to the connector's stdin JSONL stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveCommand {
    /// Connect to a public TikTok username after explicit user action.
    Start {
        /// Correlates this request with the connector response.
        request_id: String,
        /// Public TikTok username, with an optional leading `@`.
        username: String,
    },
    /// Inspect a username/room without starting a LIVE session.
    Inspect {
        /// Correlates this request with the connector response.
        request_id: String,
        /// Public TikTok username, with an optional leading `@`.
        username: String,
    },
    /// Stop the active TikTok session, retaining the connector process.
    Stop {
        /// Correlates this request with the connector response.
        request_id: String,
    },
    /// Send a text message through the active session.
    SendMessage {
        /// Correlates this request with the connector response.
        request_id: String,
        /// Text to send, limited to 500 characters.
        content: String,
    },
    /// Shut down the connector process during application exit.
    Shutdown {
        /// Correlates this request with the connector response.
        request_id: String,
    },
}

impl LiveCommand {
    /// Validate and serialize one command as a single JSONL frame (without LF).
    pub fn encode(&self) -> Result<Vec<u8>, LiveProtocolError> {
        let value = match self {
            Self::Start {
                request_id,
                username,
            } => {
                validate_request_id(request_id)?;
                let username = validate_username(username)?;
                serde_json::json!({ "protocolVersion": PROTOCOL_VERSION, "type": "start", "requestId": request_id, "username": username })
            }
            Self::Inspect {
                request_id,
                username,
            } => {
                validate_request_id(request_id)?;
                let username = validate_username(username)?;
                serde_json::json!({ "protocolVersion": PROTOCOL_VERSION, "type": "inspect", "requestId": request_id, "username": username })
            }
            Self::Stop { request_id } | Self::Shutdown { request_id } => {
                validate_request_id(request_id)?;
                let command_type = if matches!(self, Self::Stop { .. }) {
                    "stop"
                } else {
                    "shutdown"
                };
                serde_json::json!({ "protocolVersion": PROTOCOL_VERSION, "type": command_type, "requestId": request_id })
            }
            Self::SendMessage {
                request_id,
                content,
            } => {
                validate_request_id(request_id)?;
                let content = content.trim();
                if content.is_empty() || content.chars().count() > 500 {
                    return Err(LiveProtocolError::InvalidMessage);
                }
                serde_json::json!({ "protocolVersion": PROTOCOL_VERSION, "type": "sendMessage", "requestId": request_id, "content": content })
            }
        };
        let bytes = serde_json::to_vec(&value).map_err(|_| LiveProtocolError::InvalidJson)?;
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(LiveProtocolError::FrameTooLarge);
        }
        Ok(bytes)
    }
}

/// A parsed, bounded frame from the connector's stdout JSONL stream.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveFrame {
    /// Protocol event discriminator, preserved after validation.
    pub kind: LiveFrameKind,
    /// Validated JSON object; consumers still decide whether to display or act.
    pub value: Value,
}

/// Supported connector frame kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveFrameKind {
    /// Connector completed startup and announced its identity.
    Ready,
    /// Connector session state changed.
    Status,
    /// A TikTok viewer event arrived.
    Event,
    /// Room metadata changed.
    Room,
    /// Connector metrics snapshot.
    Metrics,
    /// An inspection or command completed.
    Result,
    /// Connector accepted an asynchronous command.
    Accepted,
    /// Connector rejected a command.
    CommandError,
}

/// Sanitized protocol failures; raw upstream payloads are never included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveProtocolError {
    /// Invalid username syntax.
    InvalidUsername,
    /// Invalid or oversized request identifier.
    InvalidRequestId,
    /// Empty or oversized reply message.
    InvalidMessage,
    /// Input frame exceeds the protocol's size limit.
    FrameTooLarge,
    /// Frame is not valid JSON.
    InvalidJson,
    /// Frame is not an object or omits a required value.
    InvalidShape,
    /// Connector and native client use different protocol versions.
    UnsupportedVersion,
    /// Frame type is unsupported or its fields violate the contract.
    UnsupportedFrame,
}

/// Parse and validate one JSONL frame, rejecting unbounded or nested event data.
pub fn parse_frame(bytes: &[u8]) -> Result<LiveFrame, LiveProtocolError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(LiveProtocolError::FrameTooLarge);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| LiveProtocolError::InvalidJson)?;
    let object = value.as_object().ok_or(LiveProtocolError::InvalidShape)?;
    if object.get("protocolVersion").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err(LiveProtocolError::UnsupportedVersion);
    }
    let kind = match object.get("type").and_then(Value::as_str) {
        Some("runtime.ready")
            if string(object, "runtime", 64)? == "artcraft-tiktok-live"
                && nonempty_string(object, "runtimeVersion", 64)
                && nonempty_string(object, "nodeVersion", 64) =>
        {
            LiveFrameKind::Ready
        }
        Some("runtime.status")
            if matches!(
                string(object, "status", 32)?,
                "connecting" | "connected" | "degraded" | "disconnected" | "failed" | "stopped"
            ) =>
        {
            LiveFrameKind::Status
        }
        Some("live.event") => {
            if !nonempty_string(object, "eventId", 256)
                || !nonempty_string(object, "sessionId", 128)
                || string(object, "source", 16)? != "tiktok"
                || !safe_identifier(string(object, "eventKind", 64)?)
                || !nullable_string(object, "roomId", 128)
                || !nullable_string(object, "sourceTimestamp", 32)
                || !nonempty_string(object, "receivedAt", 64)
                || !nullable_string(object, "dedupeKey", 256)
            {
                return Err(LiveProtocolError::UnsupportedFrame);
            }
            let actor = object
                .get("actor")
                .and_then(Value::as_object)
                .ok_or(LiveProtocolError::UnsupportedFrame)?;
            if !["userId", "userName", "displayId"]
                .iter()
                .all(|key| nullable_string(actor, key, 128))
            {
                return Err(LiveProtocolError::UnsupportedFrame);
            }
            let payload = object
                .get("payload")
                .and_then(Value::as_object)
                .ok_or(LiveProtocolError::UnsupportedFrame)?;
            if payload.len() > 64 || !payload.values().all(is_scalar) {
                return Err(LiveProtocolError::UnsupportedFrame);
            }
            LiveFrameKind::Event
        }
        Some("runtime.room") => {
            if !object.get("isLive").is_some_and(Value::is_boolean)
                || !nullable_string(object, "roomId", 128)
                || !nullable_string(object, "roomInfo", 4096)
                || !nullable_string(object, "availableGifts", 4096)
            {
                return Err(LiveProtocolError::UnsupportedFrame);
            }
            LiveFrameKind::Room
        }
        Some("runtime.metrics")
            if object.get("active").is_some_and(Value::is_boolean)
                && ["droppedEvents", "duplicateEvents"]
                    .iter()
                    .all(|key| object.get(*key).and_then(Value::as_u64).is_some()) =>
        {
            LiveFrameKind::Metrics
        }
        Some("command.result")
            if nonempty_string(object, "requestId", 128)
                && nonempty_string(object, "operation", 32) =>
        {
            LiveFrameKind::Result
        }
        Some("command.accepted") if nonempty_string(object, "requestId", 128) => {
            LiveFrameKind::Accepted
        }
        Some("command.error") if nonempty_string(object, "code", 80) => LiveFrameKind::CommandError,
        _ => return Err(LiveProtocolError::UnsupportedFrame),
    };
    Ok(LiveFrame { kind, value })
}

fn validate_username(input: &str) -> Result<String, LiveProtocolError> {
    let value = input.strip_prefix('@').unwrap_or(input);
    if value.is_empty()
        || value.len() > 24
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_'))
    {
        return Err(LiveProtocolError::InvalidUsername);
    }
    Ok(value.to_owned())
}

fn validate_request_id(input: &str) -> Result<(), LiveProtocolError> {
    if input.is_empty() || input.len() > 128 || input.chars().any(char::is_control) {
        Err(LiveProtocolError::InvalidRequestId)
    } else {
        Ok(())
    }
}

fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<&'a str, LiveProtocolError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= max)
        .ok_or(LiveProtocolError::UnsupportedFrame)
}

fn nonempty_string(object: &Map<String, Value>, key: &str, max: usize) -> bool {
    object
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|value| {
            !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
        })
}

fn nullable_string(object: &Map<String, Value>, key: &str, max: usize) -> bool {
    matches!(object.get(key), Some(Value::Null))
        || object
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| {
                !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
            })
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_and_send_commands_validate_and_serialize_expected_frames() {
        let start = LiveCommand::Start {
            request_id: "r-1".into(),
            username: "@creator.name".into(),
        }
        .encode()
        .expect("valid start");
        assert_eq!(
            serde_json::from_slice::<Value>(&start).expect("json"),
            serde_json::json!({"protocolVersion": 1, "type": "start", "requestId": "r-1", "username": "creator.name"})
        );
        let message = LiveCommand::SendMessage {
            request_id: "r-2".into(),
            content: "  Xin chào  ".into(),
        }
        .encode()
        .expect("valid message");
        assert_eq!(
            serde_json::from_slice::<Value>(&message).expect("json")["content"],
            "Xin chào"
        );
        assert_eq!(
            LiveCommand::Start {
                request_id: "x".into(),
                username: "creator/evil".into()
            }
            .encode(),
            Err(LiveProtocolError::InvalidUsername)
        );
        assert_eq!(
            LiveCommand::SendMessage {
                request_id: "x".into(),
                content: " ".into()
            }
            .encode(),
            Err(LiveProtocolError::InvalidMessage)
        );
    }

    #[test]
    fn validates_and_classifies_live_event_without_accepting_nested_payloads() {
        let frame = br#"{"protocolVersion":1,"type":"live.event","eventId":"chat-1","sessionId":"session-1","eventKind":"chat","source":"tiktok","roomId":"42","sourceTimestamp":"1","receivedAt":"now","actor":{"userId":"u1","userName":"creator","displayId":null},"dedupeKey":"chat-1","payload":{"content":"hello"}}"#;
        assert_eq!(
            parse_frame(frame).expect("valid event").kind,
            LiveFrameKind::Event
        );
        let invalid = br#"{"protocolVersion":1,"type":"live.event","eventId":"chat-1","sessionId":"session-1","eventKind":"chat","source":"tiktok","roomId":null,"sourceTimestamp":null,"receivedAt":"now","actor":{"userId":null,"userName":null,"displayId":null},"dedupeKey":null,"payload":{"nested":{"secret":true}}}"#;
        assert_eq!(
            parse_frame(invalid),
            Err(LiveProtocolError::UnsupportedFrame)
        );
    }

    #[test]
    fn rejects_version_mismatch_oversize_and_non_object_frames() {
        assert_eq!(
            parse_frame(br#"{"protocolVersion":2,"type":"runtime.status","status":"connected"}"#),
            Err(LiveProtocolError::UnsupportedVersion)
        );
        assert_eq!(
            parse_frame(&vec![b' '; MAX_FRAME_BYTES + 1]),
            Err(LiveProtocolError::FrameTooLarge)
        );
        assert_eq!(parse_frame(b"[]"), Err(LiveProtocolError::InvalidShape));
    }
}
