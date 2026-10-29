// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded native adapter for the EXISTING SLive VieNeu voice API:
//! upstream/tts_server/app.py + tts/f5api.py. No replacement synthesis engine.
use crate::{
    lipsync::{LipClient, LipError, read},
    opentalking::TtsAudio,
};
use serde_json::{Value, json};

/// RAM-only credential and validated operator-controlled endpoint.
#[derive(Clone)]
pub struct VoiceClient {
    transport: LipClient,
}
fn name_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().count() <= 64 && !s.chars().any(char::is_control)
}
impl VoiceClient {
    /// HTTPS requires a token; numeric loopback may use the unauthenticated
    /// upstream through a local service or SSH tunnel. No fake token required.
    pub fn new(origin: &str, token: &str) -> Result<Self, LipError> {
        Ok(Self {
            transport: LipClient::for_voice(origin, token)?,
        })
    }
    /// A ready empty service is valid: the first sample must be enrollable.
    pub fn connect(origin: &str, token: &str) -> Result<(Self, Vec<String>), LipError> {
        let client = Self::new(origin, token)?;
        client.readiness()?;
        let names = client.voices()?;
        Ok((client, names))
    }
    /// Auto-name unnamed samples; never silently overwrite an existing profile.
    pub fn enrollment_name(&self, requested: &str) -> Result<String, LipError> {
        let requested = requested.trim();
        if !requested.is_empty() && !name_ok(requested) {
            return Err(LipError::Media);
        }
        let names = self.voices()?;
        if !requested.is_empty() {
            return if names.iter().any(|name| name == requested) {
                Err(LipError::Media)
            } else {
                Ok(requested.into())
            };
        }
        for number in 1..=257 {
            let name = if number == 1 {
                "Giọng của tôi".into()
            } else {
                format!("Giọng của tôi {number}")
            };
            if !names.contains(&name) {
                return Ok(name);
            }
        }
        Err(LipError::Unavailable)
    }
    /// Verify engine readiness, not just a successful HTTP response.
    pub fn readiness(&self) -> Result<(), LipError> {
        let data = read(
            self.transport
                .request("GET", "/")
                .timeout(std::time::Duration::from_secs(8))
                .call()
                .map_err(|_| LipError::Unavailable)?,
            8192,
        )?;
        let value: Value = serde_json::from_slice(&data).map_err(|_| LipError::Unavailable)?;
        if value["status"] != "ready" || value["engine"] != "S-Live Voice Pro" {
            return Err(LipError::Unavailable);
        }
        Ok(())
    }
    /// Read exact server-side names, including enrolled sample voices. No fallback aliases.
    pub fn voices(&self) -> Result<Vec<String>, LipError> {
        let data = read(
            self.transport
                .request("GET", "/v1/voices")
                .timeout(std::time::Duration::from_secs(8))
                .call()
                .map_err(|_| LipError::Unavailable)?,
            256 * 1024,
        )?;
        let value: Value = serde_json::from_slice(&data).map_err(|_| LipError::Unavailable)?;
        let names = value["voices"]
            .as_array()
            .filter(|v| v.len() <= 256)
            .ok_or(LipError::Unavailable)?;
        let mut out = Vec::new();
        for name in names {
            let name = name
                .as_str()
                .filter(|s| name_ok(s))
                .ok_or(LipError::Unavailable)?;
            if !out.iter().any(|s| s == name) {
                out.push(name.to_string());
            }
        }
        Ok(out)
    }
    /// Explicit sample-upload action authorizes this registration; never called
    /// on page entry or avatar selection.
    pub fn enroll(&self, name: &str, wav: &[u8], consent: bool) -> Result<(), LipError> {
        if !consent || !name_ok(name) || !(3.0..=8.1).contains(&wav_seconds(wav)?) {
            return Err(LipError::Media);
        }
        let boundary = format!("concatVoice{:016x}", rand::random::<u64>());
        let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\n{name}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"reference.wav\"\r\nContent-Type: audio/wav\r\n\r\n").into_bytes();
        body.extend_from_slice(wav);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let response = self
            .transport
            .request("POST", "/v1/voices/enroll")
            .set(
                "Content-Type",
                &format!("multipart/form-data; boundary={boundary}"),
            )
            .send_bytes(&body)
            .map_err(|_| LipError::Unavailable)?;
        let data = read(response, 16384)?;
        let value: Value = serde_json::from_slice(&data).map_err(|_| LipError::Unavailable)?;
        if value["success"] != true || value["voice"]["name"] != name {
            return Err(LipError::Unavailable);
        }
        Ok(())
    }
    /// Reuse SLive's speech payload. A missing/failed chosen voice is an error, not Edge fallback.
    pub fn synthesize(&self, text: &str, voice: &str) -> Result<TtsAudio, LipError> {
        if text.trim().is_empty() || text.chars().count() > 240 || !name_ok(voice) {
            return Err(LipError::Media);
        }
        if !self.voices()?.iter().any(|name| name == voice) {
            return Err(LipError::Unavailable);
        }
        let body = json!({"input":text,"voice":voice,"speed":1.0}).to_string();
        let response = self
            .transport
            .request("POST", "/v1/audio/speech")
            .set("Content-Type", "application/json")
            .send_bytes(body.as_bytes())
            .map_err(|_| LipError::Unavailable)?;
        if !response
            .header("Content-Type")
            .is_some_and(|s| s.split(';').next() == Some("audio/wav"))
        {
            return Err(LipError::Media);
        }
        let bytes = read(response, 5 * 1024 * 1024)?;
        if !(0.05..=15.0).contains(&wav_seconds(&bytes)?) {
            return Err(LipError::Media);
        }
        Ok(TtsAudio {
            bytes,
            media_type: "audio/wav".into(),
        })
    }
}

/// Pack extracted native mono PCM16, without ffmpeg subprocesses or model execution.
pub fn reference_wav(samples: &[i16]) -> Result<Vec<u8>, LipError> {
    if !(48000..=128000).contains(&samples.len()) {
        return Err(LipError::Media);
    }
    let len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(len as usize + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(len + 36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&32000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(out)
}
fn wav_seconds(bytes: &[u8]) -> Result<f64, LipError> {
    if bytes.len() < 44
        || bytes.len() > 5 * 1024 * 1024
        || &bytes[..4] != b"RIFF"
        || &bytes[8..12] != b"WAVE"
    {
        return Err(LipError::Media);
    }
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    if u32_at(4) != bytes.len() - 8 {
        return Err(LipError::Media);
    }
    let mut offset = 12;
    let mut format = None;
    let mut data = None;
    while offset + 8 <= bytes.len() {
        let length = u32_at(offset + 4);
        let start = offset + 8;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or(LipError::Media)?;
        if &bytes[offset..offset + 4] == b"fmt " {
            if length < 16 || format.is_some() {
                return Err(LipError::Media);
            }
            let channels =
                u16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap()) as usize;
            let rate = u32_at(start + 4);
            if bytes[start..start + 2] != [1, 0]
                || ![1, 2].contains(&channels)
                || !(8000..=48000).contains(&rate)
                || bytes[start + 14..start + 16] != [16, 0]
                || u32_at(start + 8) != rate * channels * 2
                || u16::from_le_bytes(bytes[start + 12..start + 14].try_into().unwrap()) as usize
                    != channels * 2
            {
                return Err(LipError::Media);
            }
            format = Some((rate, channels));
        } else if &bytes[offset..offset + 4] == b"data" {
            if data.is_some() || length == 0 {
                return Err(LipError::Media);
            }
            data = Some(length);
        }
        offset = end + (length & 1);
    }
    let (rate, channels) = format.ok_or(LipError::Media)?;
    let data = data.ok_or(LipError::Media)?;
    if data % (channels * 2) != 0 {
        return Err(LipError::Media);
    }
    Ok(data as f64 / (rate * channels * 2) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    // Real HTTP contract fixture; no model, provider or external network.
    fn service(
        replies: Vec<(&'static str, Vec<u8>)>,
    ) -> (VoiceClient, std::thread::JoinHandle<Vec<(String, Vec<u8>)>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = VoiceClient::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            &"x".repeat(48),
        )
        .unwrap();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (kind, body) in replies {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut header = Vec::new();
                loop {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    header.push(byte[0]);
                    if header.ends_with(b"\r\n\r\n") {
                        break;
                    }
                    assert!(header.len() < 16384);
                }
                let header = String::from_utf8(header).unwrap();
                assert!(header.contains(&format!("Bearer {}", "x".repeat(48))));
                let length = header
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                            .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                assert!(length < 300000);
                let mut request_body = vec![0; length];
                socket.read_exact(&mut request_body).unwrap();
                requests.push((header, request_body));
                write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
                socket.write_all(&body).unwrap();
            }
            requests
        });
        (client, worker)
    }

    #[test]
    fn empty_loopback_service_can_accept_its_first_sample_without_a_dummy_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            for body in [
                r#"{"status":"ready","engine":"S-Live Voice Pro"}"#,
                r#"{"voices":[]}"#,
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    socket.read_exact(&mut byte).unwrap();
                    header.push(byte[0]);
                    assert!(header.len() < 16384);
                }
                assert!(
                    !String::from_utf8_lossy(&header)
                        .to_lowercase()
                        .contains("authorization:")
                );
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let (_, names) = VoiceClient::connect(&origin, "").unwrap();
        assert!(names.is_empty());
        worker.join().unwrap();
        assert!(VoiceClient::new("https://example.com", "").is_err());
        assert!(VoiceClient::new("http://192.168.1.2:8767", "").is_err());
        assert!(LipClient::new("http://127.0.0.1:8766", "").is_err());
    }

    #[test]
    fn default_sample_name_avoids_existing_names_but_explicit_duplicates_fail() {
        let (client, worker) = service(vec![(
            "application/json",
            r#"{"voices":["Giọng của tôi","Giọng của tôi 2"]}"#.as_bytes().to_vec(),
        )]);
        assert_eq!(client.enrollment_name("").unwrap(), "Giọng của tôi 3");
        worker.join().unwrap();
        let (client, worker) = service(vec![(
            "application/json",
            br#"{"voices":["Mine"]}"#.to_vec(),
        )]);
        assert!(client.enrollment_name("Mine").is_err());
        worker.join().unwrap();
    }

    #[test]
    fn sample_registration_then_speech_uses_exact_new_voice() {
        let wav = reference_wav(&vec![123; 64000]).unwrap();
        let (client, worker) = service(vec![
            (
                "application/json",
                br#"{"status":"ready","engine":"S-Live Voice Pro"}"#.to_vec(),
            ),
            ("application/json", br#"{"voices":[]}"#.to_vec()),
            (
                "application/json",
                br#"{"success":true,"voice":{"name":"Mine"}}"#.to_vec(),
            ),
            ("application/json", br#"{"voices":["Mine"]}"#.to_vec()),
            ("audio/wav", wav.clone()),
        ]);
        client.readiness().unwrap();
        assert_eq!(client.enrollment_name("Mine").unwrap(), "Mine");
        client.enroll("Mine", &wav, true).unwrap();
        assert_eq!(client.synthesize("Xin chào", "Mine").unwrap().bytes, wav);
        let requests = worker.join().unwrap();
        assert!(requests[2].0.starts_with("POST /v1/voices/enroll "));
        assert!(requests[4].0.starts_with("POST /v1/audio/speech "));
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[4].1).unwrap()["voice"],
            "Mine"
        );
    }

    #[test]
    fn existing_slive_health_voice_and_speech_contract() {
        let wav = reference_wav(&vec![123; 48000]).unwrap();
        let (client, worker) = service(vec![
            (
                "application/json",
                br#"{"status":"ready","engine":"S-Live Voice Pro"}"#.to_vec(),
            ),
            (
                "application/json",
                "{\"voices\":[\"Giọng mẫu\"]}".as_bytes().to_vec(),
            ),
            ("audio/wav", wav.clone()),
        ]);
        client.readiness().unwrap();
        assert_eq!(
            client.synthesize("Xin chào", "Giọng mẫu").unwrap().bytes,
            wav
        );
        let requests = worker.join().unwrap();
        assert!(requests[0].0.starts_with("GET / "));
        assert!(requests[1].0.starts_with("GET /v1/voices "));
        assert!(requests[2].0.starts_with("POST /v1/audio/speech "));
        let body: Value = serde_json::from_slice(&requests[2].1).unwrap();
        assert_eq!(
            body,
            json!({"input":"Xin chào","voice":"Giọng mẫu","speed":1.0})
        );
    }

    #[test]
    fn enroll_uses_multipart_and_missing_voice_never_synthesizes() {
        let (client, worker) = service(vec![(
            "application/json",
            br#"{"success":true,"voice":{"name":"Mine"}}"#.to_vec(),
        )]);
        client
            .enroll("Mine", &reference_wav(&vec![0; 48000]).unwrap(), true)
            .unwrap();
        let requests = worker.join().unwrap();
        assert!(requests[0].0.starts_with("POST /v1/voices/enroll "));
        assert!(
            requests[0]
                .0
                .contains("multipart/form-data; boundary=concatVoice")
        );
        assert!(String::from_utf8_lossy(&requests[0].1).contains("name=\"name\"\r\n\r\nMine"));
        let (client, worker) = service(vec![(
            "application/json",
            br#"{"voices":["Other"]}"#.to_vec(),
        )]);
        assert!(client.synthesize("Hi", "Missing").is_err());
        assert_eq!(worker.join().unwrap().len(), 1);
    }
    #[test]
    fn reference_is_bounded_pcm_and_requires_consent_before_any_network() {
        let wav = reference_wav(&vec![123; 64000]).unwrap();
        assert_eq!(wav_seconds(&wav).unwrap(), 4.0);
        assert!(reference_wav(&[0; 4]).is_err());
        assert!(reference_wav(&vec![0; 128001]).is_err());
        let c = VoiceClient::new("http://127.0.0.1:1", &"x".repeat(48)).unwrap();
        assert_eq!(c.enroll("Giọng của tôi", &wav, false), Err(LipError::Media));
        assert_eq!(c.enroll("bad\r\nname", &wav, true), Err(LipError::Media));
    }
    #[test]
    fn corrupt_or_forged_wav_is_not_speech() {
        assert!(wav_seconds(b"html").is_err());
        let mut wav = reference_wav(&vec![0; 48000]).unwrap();
        wav[24..28].copy_from_slice(&0u32.to_le_bytes());
        assert!(wav_seconds(&wav).is_err());
    }

    #[test]
    fn unknown_engine_or_error_health_cannot_enable_sample_voice() {
        for body in [
            br#"{"status":"ready","engine":"Other"}"#.as_slice(),
            br#"{"status":"error","engine":"S-Live Voice Pro"}"#.as_slice(),
        ] {
            let (client, worker) = service(vec![("application/json", body.to_vec())]);
            assert!(client.readiness().is_err());
            worker.join().unwrap();
        }
    }
}
