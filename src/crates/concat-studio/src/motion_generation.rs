// SPDX-License-Identifier: AGPL-3.0-or-later
//! Offline gesture generation. Explicit upload, no retry or automatic model download.
use crate::lipsync::{LipClient, LipError, read};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::time::Duration;

/// A separate offline generator; never runs on the LIVE reply critical path.
#[derive(Clone)]
pub struct MotionClient(LipClient);
impl MotionClient {
    /// Same private endpoint/token restrictions as the lip-sync client.
    pub fn new(url: &str, token: &str) -> Result<Self, LipError> {
        Ok(Self(LipClient::with_read_timeout(
            url,
            token,
            Duration::from_secs(1800),
        )?))
    }
    /// Check installed runtime/GPU, without uploading or starting generation.
    pub fn readiness(&self) -> Result<(), LipError> {
        let response = self
            .0
            .request("GET", "/v1/motion/health")
            .timeout(Duration::from_secs(8))
            .call()
            .map_err(|_| LipError::Unavailable)?;
        let value: serde_json::Value =
            serde_json::from_slice(&read(response, 8192)?).map_err(|_| LipError::Unavailable)?;
        if value["version"] != 1
            || value["ready"] != true
            || value["model"] != "echomimic-v3-flash"
            || value["mode"] != "offline-gesture"
            || value["busy"] == true
        {
            return Err(LipError::Unavailable);
        }
        Ok(())
    }
    /// Generate one silent motion source, bounded to the LIVE source duration.
    pub fn render(
        &self,
        image: &[u8],
        audio: &[u8],
        audio_type: &str,
        role: usize,
        seconds: u32,
        consent: bool,
    ) -> Result<Vec<u8>, LipError> {
        if !consent
            || role > 2
            || !(2..=10).contains(&seconds)
            || image.len() > 4 * 1024 * 1024
            || !image.starts_with(b"\x89PNG\r\n\x1a\n")
            || audio.is_empty()
            || audio.len() > 5 * 1024 * 1024
            || !matches!(
                audio_type,
                "audio/wav" | "audio/mpeg" | "audio/mp3" | "audio/flac" | "audio/mp4" | "audio/aac"
            )
        {
            return Err(LipError::Media);
        }
        let body = serde_json::json!({"image": STANDARD.encode(image), "audio": STANDARD.encode(audio),
            "audio_type": audio_type, "role": role, "seconds": seconds, "consent": true});
        let response = self
            .0
            .request("POST", "/v1/motion/render")
            .timeout(Duration::from_secs(1800))
            .set("Content-Type", "application/json")
            .send_bytes(body.to_string().as_bytes())
            .map_err(|_| LipError::Unavailable)?;
        if response.header("Content-Type") != Some("video/mp4") {
            return Err(LipError::Media);
        }
        let bytes = read(response, 25 * 1024 * 1024)?;
        if bytes.get(4..8) != Some(b"ftyp") {
            return Err(LipError::Media);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_duration_consent_and_endpoint_before_upload() {
        assert!(MotionClient::new("http://public.invalid", &"x".repeat(48)).is_err());
        let client = MotionClient::new("http://127.0.0.1:1", &"x".repeat(48)).unwrap();
        for seconds in [0, 1, 11, u32::MAX] {
            assert_eq!(
                client.render(b"\x89PNG\r\n\x1a\n", b"a", "audio/wav", 0, seconds, true),
                Err(LipError::Media)
            );
        }
        assert_eq!(
            client.render(b"\x89PNG\r\n\x1a\n", b"a", "audio/wav", 0, 5, false),
            Err(LipError::Media)
        );
    }
    #[test]
    fn authenticated_transport_preserves_requested_seconds_and_role() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = MotionClient::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            &"x".repeat(48),
        )
        .unwrap();
        let worker = std::thread::spawn(move || {
            for index in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    headers.push(byte[0]);
                    assert!(headers.len() < 8192);
                }
                let headers = String::from_utf8(headers).unwrap();
                assert!(headers.contains(&format!("Bearer {}", "x".repeat(48))));
                let body = if index < 2 {
                    assert!(headers.starts_with("GET /v1/motion/health "));
                    serde_json::to_vec(&serde_json::json!({"version": 1, "ready": true,
                        "model": "echomimic-v3-flash", "mode": "offline-gesture", "busy": index == 0})).unwrap()
                } else {
                    assert!(headers.starts_with("POST /v1/motion/render "));
                    let size = headers
                        .lines()
                        .find_map(|l| {
                            l.split_once(':')
                                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    let mut data = vec![0; size];
                    stream.read_exact(&mut data).unwrap();
                    let value: serde_json::Value = serde_json::from_slice(&data).unwrap();
                    assert_eq!(value["seconds"], 7);
                    assert_eq!(value["role"], 2);
                    assert_eq!(value["consent"], true);
                    b"\0\0\0\x18ftypisomfixture".to_vec()
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", if index < 2 { "application/json" } else { "video/mp4" }, body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        assert_eq!(client.readiness(), Err(LipError::Unavailable));
        client.readiness().unwrap();
        assert_eq!(
            &client
                .render(b"\x89PNG\r\n\x1a\n", b"audio", "audio/wav", 2, 7, true)
                .unwrap()[4..8],
            b"ftyp"
        );
        worker.join().unwrap();
    }
}
