// SPDX-License-Identifier: AGPL-3.0-or-later
//! Promax Concat avatar API. No ArtCraft routes or engine paths.

use serde::Deserialize;
use serde_json::Value;
use std::{fs, io::Read, path::Path, time::Duration};

const ROUTE: &str = "/api/v1/concat/avatars";
const MAX_VIDEO: u64 = 25 * 1024 * 1024;

/// Safe error categories; never display provider bodies or bearer credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarError {
    /// Unsafe origin or malformed configuration.
    Configuration,
    /// Backend requires a valid operator-configured access credential.
    Authentication,
    /// Invalid name, media type/size, or rejected request.
    Input,
    /// No usable Promax connection.
    Unavailable,
    /// Engine missing, busy, or media not ready.
    NotReady,
    /// Unexpected or oversized backend data.
    InvalidResponse,
    /// Mutation may have arrived; reconcile the list instead of retrying it.
    UnknownOutcome,
}

/// Server-reported preparation features, not synthesized body animation.
#[derive(Clone, Debug, Deserialize)]
#[allow(missing_docs)]
pub struct Features {
    pub source_motion: bool,
    pub face_prepared: bool,
    pub realtime_lipsync: bool,
    pub voice_clone: bool,
}

/// Derived from Promax's `docs/concat-avatar.openapi.json` AvatarJob schema.
#[derive(Clone, Debug, Deserialize)]
#[allow(missing_docs)]
pub struct AvatarJob {
    pub id: String,
    pub name: String,
    pub status: String,
    pub created_at: String,
    pub error_code: Option<String>,
    pub frames: u32,
    pub duration_seconds: f64,
    pub source_video_url: Option<String>,
    pub preview_url: Option<String>,
    pub capabilities: Features,
}

/// Explicit readiness receipt; a running HTTP server alone is not sufficient.
#[derive(Clone, Debug, Deserialize)]
#[allow(missing_docs)]
pub struct Readiness {
    pub ready: bool,
    pub missing: Vec<String>,
}

/// Bounded Promax-only transport. HTTP is allowed exclusively on numeric loopback.
pub struct AvatarClient {
    origin: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl AvatarClient {
    /// Bind credentials to this configured origin; never follow redirects.
    pub fn new(origin: &str, token: Option<String>) -> Result<Self, AvatarError> {
        let parsed = url::Url::parse(origin.trim()).map_err(|_| AvatarError::Configuration)?;
        let local = match parsed.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if parsed.host().is_none()
            || !(parsed.scheme() == "https" || (parsed.scheme() == "http" && local))
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port() == Some(0)
        {
            return Err(AvatarError::Configuration);
        }
        let token = token.filter(|s| !s.is_empty());
        if token.as_ref().is_some_and(|s| {
            s.len() > 8192
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(&b))
        }) {
            return Err(AvatarError::Configuration);
        }
        if !local && token.is_none() {
            return Err(AvatarError::Authentication);
        }
        Ok(Self {
            origin: parsed.as_str().trim_end_matches('/').into(),
            token,
            agent: ureq::AgentBuilder::new()
                .redirects(0)
                .timeout_connect(Duration::from_secs(2))
                .timeout_read(Duration::from_secs(20))
                .timeout_write(Duration::from_secs(20))
                .build(),
        })
    }

    fn request(&self, method: &str, suffix: &str) -> ureq::Request {
        let request = self
            .agent
            .request(method, &format!("{}{ROUTE}{suffix}", self.origin));
        if let Some(token) = &self.token {
            request.set("Authorization", &format!("Bearer {token}"))
        } else {
            request
        }
    }

    /// Read engine capability without starting a scan.
    pub fn readiness(&self) -> Result<Readiness, AvatarError> {
        let response = self
            .request("GET", "/capabilities")
            .call()
            .map_err(|e| map_error(e, false))?;
        let value: Readiness =
            serde_json::from_value(json(response)?).map_err(|_| AvatarError::InvalidResponse)?;
        if value.missing.len() > 12
            || value
                .missing
                .iter()
                .any(|s| s.len() > 80 || s.chars().any(char::is_control))
            || (value.ready && !value.missing.is_empty())
        {
            return Err(AvatarError::InvalidResponse);
        }
        Ok(value)
    }

    /// List only jobs the configured principal can access.
    pub fn list(&self) -> Result<Vec<AvatarJob>, AvatarError> {
        let data = json(
            self.request("GET", "")
                .call()
                .map_err(|e| map_error(e, false))?,
        )?;
        let rows = data
            .as_array()
            .filter(|r| r.len() <= 100)
            .ok_or(AvatarError::InvalidResponse)?;
        rows.iter().cloned().map(parse_job).collect()
    }

    /// Send one explicitly selected video. There is no automatic POST retry.
    pub fn import_video(&self, name: &str, path: &Path) -> Result<AvatarJob, AvatarError> {
        let name = name.trim();
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.is_empty()
            || name.chars().count() > 90
            || name.chars().any(char::is_control)
            || !matches!(extension.as_str(), "mp4" | "webm")
        {
            return Err(AvatarError::Input);
        }
        let file = fs::File::open(path).map_err(|_| AvatarError::Input)?;
        let metadata = file.metadata().map_err(|_| AvatarError::Input)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_VIDEO {
            return Err(AvatarError::Input);
        }
        let mut bytes = Vec::new();
        file.take(MAX_VIDEO + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| AvatarError::Input)?;
        let valid = if extension == "mp4" {
            bytes.get(4..8) == Some(b"ftyp")
        } else {
            bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        };
        if !valid || bytes.len() as u64 > MAX_VIDEO {
            return Err(AvatarError::Input);
        }
        let response = self
            .request("POST", "/import-video")
            .query("name", name)
            .timeout(Duration::from_secs(45))
            .set(
                "Content-Type",
                if extension == "mp4" {
                    "video/mp4"
                } else {
                    "video/webm"
                },
            )
            .send_bytes(&bytes)
            .map_err(|e| map_error(e, true))?;
        if response.status() != 202 {
            return Err(AvatarError::UnknownOutcome);
        }
        parse_job(json(response)?)
    }

    /// Archive exactly one local library entry; never automatically retry a mutation.
    pub fn delete(&self, id: &str) -> Result<(), AvatarError> {
        if !valid_id(id) {
            return Err(AvatarError::Input);
        }
        let response = self
            .request("DELETE", &format!("/{id}"))
            .call()
            .map_err(|error| map_error(error, true))?;
        let receipt = json(response)?;
        if receipt["id"].as_str() != Some(id)
            || receipt["deleted"] != true
            || receipt["archived"] != true
        {
            return Err(AvatarError::UnknownOutcome);
        }
        Ok(())
    }

    /// Retrieve protected source/preview from the same origin, never a response URL.
    pub fn media(&self, id: &str, preview: bool) -> Result<Vec<u8>, AvatarError> {
        if !valid_id(id) {
            return Err(AvatarError::Input);
        }
        let response = self
            .request(
                "GET",
                &format!("/{id}/{}", if preview { "preview" } else { "source-video" }),
            )
            .timeout(Duration::from_secs(35))
            .call()
            .map_err(|e| map_error(e, false))?;
        let expected = if preview { "image/png" } else { "video/mp4" };
        if response
            .header("Content-Type")
            .and_then(|v| v.split(';').next())
            != Some(expected)
        {
            return Err(AvatarError::InvalidResponse);
        }
        let max = if preview { 8 * 1024 * 1024 } else { MAX_VIDEO };
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| AvatarError::InvalidResponse)?;
        let signature = if preview {
            bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        } else {
            bytes.get(4..8) == Some(b"ftyp")
        };
        if !signature || bytes.len() as u64 > max {
            return Err(AvatarError::InvalidResponse);
        }
        Ok(bytes)
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

fn parse_job(value: Value) -> Result<AvatarJob, AvatarError> {
    let job: AvatarJob = serde_json::from_value(value).map_err(|_| AvatarError::InvalidResponse)?;
    if !valid_id(&job.id)
        || job.name.is_empty()
        || job.name.chars().count() > 90
        || job.name.chars().any(char::is_control)
        || !job.duration_seconds.is_finite()
        || !(0.0..=30.0).contains(&job.duration_seconds)
        || job.frames > 750
        || !matches!(job.status.as_str(), "processing" | "ready" | "failed")
    {
        return Err(AvatarError::InvalidResponse);
    }
    if job.status == "ready" {
        if job.frames < 2
            || job.duration_seconds < 1.0
            || !job.capabilities.source_motion
            || !job.capabilities.face_prepared
            || job.source_video_url.as_deref()
                != Some(format!("{ROUTE}/{}/source-video", job.id).as_str())
            || job.preview_url.as_deref() != Some(format!("{ROUTE}/{}/preview", job.id).as_str())
        {
            return Err(AvatarError::InvalidResponse);
        }
    } else if job.source_video_url.is_some()
        || job.preview_url.is_some()
        || job.capabilities.source_motion
        || job.capabilities.face_prepared
    {
        return Err(AvatarError::InvalidResponse);
    }
    Ok(job)
}

fn json(response: ureq::Response) -> Result<Value, AvatarError> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AvatarError::InvalidResponse)?;
    if bytes.len() > 1024 * 1024 {
        return Err(AvatarError::InvalidResponse);
    }
    let mut value: Value =
        serde_json::from_slice(&bytes).map_err(|_| AvatarError::InvalidResponse)?;
    if value["ok"] != true {
        return Err(AvatarError::InvalidResponse);
    }
    value
        .get_mut("data")
        .map(Value::take)
        .ok_or(AvatarError::InvalidResponse)
}

fn map_error(error: ureq::Error, mutation: bool) -> AvatarError {
    match error {
        ureq::Error::Status(401 | 403, _) => AvatarError::Authentication,
        ureq::Error::Status(400 | 413 | 422, _) => AvatarError::Input,
        ureq::Error::Status(404 | 429 | 503, _) => AvatarError::NotReady,
        _ if mutation => AvatarError::UnknownOutcome,
        _ => AvatarError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener};

    fn receive(socket: &mut std::net::TcpStream) -> Vec<u8> {
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut bytes = [0; 2048];
            let count = socket.read(&mut bytes).unwrap();
            request.extend_from_slice(&bytes[..count]);
            if let Some(end) = request.windows(4).position(|b| b == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]);
                let length = head
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|s| s.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    return request;
                }
            }
            assert_ne!(count, 0, "truncated request");
        }
    }

    #[test]
    fn sends_raw_upload_and_reads_protected_media_using_only_promax_contract() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let bytes = b"\0\0\0\x14ftypisom";
        let path = std::env::temp_dir().join(format!(
            "concat-promax-avatar-{:032x}.mp4",
            rand::random::<u128>()
        ));
        fs::write(&path, bytes).unwrap();
        let server = std::thread::spawn(move || {
            let id = "00000000-0000-4000-8000-000000000001";
            let (mut socket, _) = listener.accept().unwrap();
            let request = receive(&mut socket);
            let end = request.windows(4).position(|b| b == b"\r\n\r\n").unwrap();
            let head = String::from_utf8_lossy(&request[..end]);
            assert!(head.starts_with("POST /api/v1/concat/avatars/import-video?name="));
            assert!(head.contains("video/mp4"));
            assert!(!head.contains("multipart"));
            assert!(head.contains("Bearer fixture-token"));
            assert_eq!(&request[end + 4..], bytes);
            let body = serde_json::json!({"ok":true,"data":{"id":id,"name":"MC","status":"processing","created_at":"2026-10-19T00:00:00.000Z","error_code":null,"frames":0,"duration_seconds":0,"source_video_url":null,"preview_url":null,"capabilities":{"source_motion":false,"face_prepared":false,"realtime_lipsync":false,"voice_clone":false}}}).to_string();
            write!(socket, "HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            drop(socket);
            let (mut socket, _) = listener.accept().unwrap();
            let request = receive(&mut socket);
            assert!(
                String::from_utf8_lossy(&request)
                    .starts_with(&format!("GET /api/v1/concat/avatars/{id}/source-video "))
            );
            assert!(String::from_utf8_lossy(&request).contains("Bearer fixture-token"));
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).unwrap();
            socket.write_all(bytes).unwrap();
        });
        let client = AvatarClient::new(&origin, Some("fixture-token".into())).unwrap();
        let receipt = client.import_video("MC", &path);
        fs::remove_file(path).unwrap();
        let receipt = receipt.unwrap();
        assert_eq!(receipt.status, "processing");
        assert_eq!(client.media(&receipt.id, false).unwrap(), bytes);
        server.join().unwrap();
    }

    #[test]
    fn refuses_redirects_without_replaying_upload_or_forwarding_credentials() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            receive(&mut socket);
            write!(socket, "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let client = AvatarClient::new(&origin, Some("fixture-token".into())).unwrap();
        assert!(client.list().is_err());
        server.join().unwrap();
    }

    #[test]
    fn rejects_unsafe_origins_and_unbound_remote_credentials() {
        for origin in [
            "http://example.com",
            "http://127.0.0.1:43100/path",
            "https://user:pass@example.com",
            "http://127.0.0.1:43100?x=y",
        ] {
            assert!(AvatarClient::new(origin, None).is_err());
        }
        assert!(AvatarClient::new("http://127.0.0.1:43100", None).is_ok());
        assert!(AvatarClient::new("https://example.com", None).is_err());
    }

    #[test]
    fn ready_job_requires_its_own_media_and_real_preparation_receipt() {
        let id = "00000000-0000-4000-8000-000000000001";
        let mut value = serde_json::json!({"id":id,"name":"My MC","status":"ready","created_at":"2026-10-19T00:00:00.000Z","error_code":null,
            "frames":50,"duration_seconds":2,"source_video_url":format!("/api/v1/concat/avatars/{id}/source-video"),"preview_url":format!("/api/v1/concat/avatars/{id}/preview"),
            "capabilities":{"source_motion":true,"face_prepared":true,"realtime_lipsync":false,"voice_clone":false}});
        assert!(parse_job(value.clone()).is_ok());
        value["source_video_url"] = serde_json::json!("https://example.com/other.mp4");
        assert!(parse_job(value.clone()).is_err());
        value["source_video_url"] = serde_json::Value::Null;
        assert!(parse_job(value).is_err());
    }

    #[test]
    fn deletes_only_validated_id_and_requires_matching_archive_receipt() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let id = "00000000-0000-4000-8000-000000000001";
        let server = std::thread::spawn(move || {
            for receipt in [
                serde_json::json!({"id": id, "deleted": true, "archived": true}),
                serde_json::json!({"id": "another-job", "deleted": true, "archived": true}),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                let request = receive(&mut socket);
                let request = String::from_utf8_lossy(&request);
                assert!(request.starts_with(&format!("DELETE {ROUTE}/{id} HTTP/1.1")));
                assert!(request.contains("Bearer fixture-token"));
                let body = serde_json::json!({"ok": true, "data": receipt}).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let client = AvatarClient::new(&origin, Some("fixture-token".into())).unwrap();
        assert_eq!(client.delete("../original.mp4"), Err(AvatarError::Input));
        assert_eq!(client.delete(id), Ok(()));
        assert_eq!(client.delete(id), Err(AvatarError::UnknownOutcome));
        server.join().unwrap();
    }

    #[test]
    fn upload_validates_user_selected_media_before_network() {
        let client = AvatarClient::new("http://127.0.0.1:1", None).unwrap();
        assert!(matches!(
            client.import_video("", std::path::Path::new("a.mp4")),
            Err(AvatarError::Input)
        ));
        assert!(matches!(
            client.import_video("MC", std::path::Path::new("a.exe")),
            Err(AvatarError::Input)
        ));
        assert!(matches!(
            client.media("../secret", false),
            Err(AvatarError::Input)
        ));
    }
}
