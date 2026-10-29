// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit, bounded GPU utterance transport. No retries or redirect credentials.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use std::{io::Read, path::Path, time::Duration};

/// Safe categories, never raw server bodies/credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LipError {
    /// Invalid endpoint/credential.
    Configuration,
    /// Invalid or oversized source/audio/result.
    Media,
    /// GPU busy, missing or network unavailable; never auto-retry the request.
    Unavailable,
}

/// Reported by a loaded model, not merely a running HTTP process.
#[derive(Deserialize)]
pub struct Readiness {
    /// Wire revision.
    pub version: u32,
    /// Model and GPU are loaded.
    pub ready: bool,
    /// Exact supported model.
    pub model: String,
    /// Explicit buffered operation, not streaming.
    pub mode: String,
}

/// One configured operator-controlled server. Not Debug: contains a credential.
#[derive(Clone)]
pub struct LipClient {
    origin: String,
    token: String,
    agent: ureq::Agent,
}
impl LipClient {
    /// HTTPS required except numeric loopback for an SSH tunnel/local tests.
    pub fn new(origin: &str, token: &str) -> Result<Self, LipError> {
        Self::with_read_timeout(origin, token, Duration::from_secs(180))
    }
    pub(crate) fn with_read_timeout(
        origin: &str,
        token: &str,
        read_timeout: Duration,
    ) -> Result<Self, LipError> {
        Self::configured(origin, token, read_timeout, false)
    }
    // VieNeu upstream has no token authentication. Permit an empty credential
    // only on numeric loopback (local service/SSH tunnel), never on public TLS.
    pub(crate) fn for_voice(origin: &str, token: &str) -> Result<Self, LipError> {
        Self::configured(origin, token, Duration::from_secs(180), true)
    }
    fn configured(
        origin: &str,
        token: &str,
        read_timeout: Duration,
        local_voice: bool,
    ) -> Result<Self, LipError> {
        let url = url::Url::parse(origin.trim()).map_err(|_| LipError::Configuration)?;
        let local = match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if url.host().is_none()
            || !(url.scheme() == "https" || url.scheme() == "http" && local)
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port() == Some(0)
            || (!(local_voice && local && token.is_empty())
                && (!(32..=256).contains(&token.len())
                    || !token.bytes().all(|b| b.is_ascii_alphanumeric())))
        {
            return Err(LipError::Configuration);
        }
        Ok(Self {
            origin: url.as_str().trim_end_matches('/').into(),
            token: token.into(),
            agent: ureq::AgentBuilder::new()
                .redirects(0)
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(read_timeout)
                .timeout_write(Duration::from_secs(45))
                .build(),
        })
    }
    pub(crate) fn request(&self, method: &str, route: &str) -> ureq::Request {
        let request = self
            .agent
            .request(method, &format!("{}{route}", self.origin));
        if self.token.is_empty() {
            request
        } else {
            request.set("Authorization", &format!("Bearer {}", self.token))
        }
    }
    /// Does not upload media or start a GPU job.
    pub fn readiness(&self) -> Result<(), LipError> {
        let response = self
            .request("GET", "/v1/lipsync/health")
            .timeout(Duration::from_secs(8))
            .call()
            .map_err(|_| LipError::Unavailable)?;
        let data = read(response, 8192)?;
        let value: Readiness = serde_json::from_slice(&data).map_err(|_| LipError::Unavailable)?;
        if value.version != 1
            || !value.ready
            || value.model != "musetalk-1.5"
            || value.mode != "buffered-utterance"
        {
            return Err(LipError::Unavailable);
        }
        Ok(())
    }
    /// Upload only after the user explicitly enables GPU replies for this session.
    pub fn render(
        &self,
        source: &Path,
        audio: &[u8],
        audio_type: &str,
    ) -> Result<Vec<u8>, LipError> {
        if audio.is_empty()
            || audio.len() > 5 * 1024 * 1024
            || !matches!(
                audio_type,
                "audio/wav"
                    | "audio/x-wav"
                    | "audio/mpeg"
                    | "audio/mp3"
                    | "audio/ogg"
                    | "audio/flac"
                    | "audio/aac"
                    | "audio/mp4"
            )
        {
            return Err(LipError::Media);
        }
        let file = std::fs::File::open(source).map_err(|_| LipError::Media)?;
        let metadata = file.metadata().map_err(|_| LipError::Media)?;
        if !metadata.is_file() || metadata.len() > 25 * 1024 * 1024 {
            return Err(LipError::Media);
        }
        let mut video = Vec::new();
        file.take(25 * 1024 * 1024 + 1)
            .read_to_end(&mut video)
            .map_err(|_| LipError::Media)?;
        if video.len() > 25 * 1024 * 1024 || video.get(4..8) != Some(b"ftyp") {
            return Err(LipError::Media);
        }
        let body = serde_json::json!({"video": STANDARD.encode(video), "audio": STANDARD.encode(audio), "audio_type": audio_type});
        let response = self
            .request("POST", "/v1/lipsync/render")
            .set("Content-Type", "application/json")
            .send_bytes(body.to_string().as_bytes())
            .map_err(|_| LipError::Unavailable)?;
        if response.header("Content-Type") != Some("video/mp4") {
            return Err(LipError::Media);
        }
        let bytes = read(response, 60 * 1024 * 1024)?;
        if bytes.get(4..8) != Some(b"ftyp") {
            return Err(LipError::Media);
        }
        Ok(bytes)
    }
}
pub(crate) fn read(response: ureq::Response, maximum: u64) -> Result<Vec<u8>, LipError> {
    if response
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .is_some_and(|n| n > maximum)
    {
        return Err(LipError::Media);
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| LipError::Unavailable)?;
    if bytes.len() as u64 > maximum {
        return Err(LipError::Media);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener};
    fn fixture(
        response: String,
        inspect: impl FnOnce(&str) + Send + 'static,
    ) -> (LipClient, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = LipClient::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            &"x".repeat(48),
        )
        .unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut raw = Vec::new();
            loop {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                raw.push(byte[0]);
                if raw.ends_with(b"\r\n\r\n") {
                    break;
                }
                assert!(raw.len() < 16384);
            }
            let headers = String::from_utf8(raw).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            inspect(&headers);
            stream.write_all(response.as_bytes()).unwrap();
        });
        (client, worker)
    }
    #[test]
    fn requires_model_readiness_not_just_http_success() {
        let body =
            r#"{"version":1,"ready":false,"model":"musetalk-1.5","mode":"buffered-utterance"}"#;
        let (client, worker) = fixture(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
            |headers| {
                assert!(headers.starts_with("GET /v1/lipsync/health "));
                assert!(headers.contains(&format!("Bearer {}", "x".repeat(48))));
            },
        );
        assert_eq!(client.readiness(), Err(LipError::Unavailable));
        worker.join().unwrap();
    }
    #[test]
    fn does_not_follow_redirect_and_rejects_wrong_media_body() {
        let (client, worker) = fixture("HTTP/1.1 302 Found\r\nLocation: https://other.invalid\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(), |_| {});
        assert!(client.readiness().is_err());
        worker.join().unwrap();
        let path =
            std::env::temp_dir().join(format!("concat-lip-test-{}.mp4", rand::random::<u64>()));
        std::fs::write(&path, b"\0\0\0\x18ftypisomfixture").unwrap();
        let (client, worker) = fixture("HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: 4\r\nConnection: close\r\n\r\nhtml".into(), |headers| {
            assert!(headers.starts_with("POST /v1/lipsync/render "));
        });
        assert_eq!(
            client.render(&path, b"audio", "audio/wav"),
            Err(LipError::Media)
        );
        worker.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejects_cleartext_remote_redirect_targets_and_invalid_secrets() {
        for origin in [
            "http://gpu.example",
            "https://u:p@gpu.example",
            "https://gpu.example/path",
            "https://gpu.example?token=x",
            "https://gpu.example/#x",
        ] {
            assert!(matches!(
                LipClient::new(origin, &"x".repeat(48)),
                Err(LipError::Configuration)
            ));
        }
        assert!(LipClient::new("http://127.0.0.1:8766", &"x".repeat(48)).is_ok());
        assert!(LipClient::new("https://gpu.example", "bad\r\ntoken").is_err());
    }
    #[test]
    fn rejects_media_before_contacting_server() {
        let client = LipClient::new("http://127.0.0.1:1", &"x".repeat(48)).unwrap();
        assert!(matches!(
            client.render(Path::new("not-a-file"), b"audio", "audio/wav"),
            Err(LipError::Media)
        ));
        assert!(matches!(
            client.render(Path::new("not-a-file"), b"audio", "text/plain"),
            Err(LipError::Media)
        ));
    }
}
