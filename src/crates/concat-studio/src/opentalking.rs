// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native OpenTalking controls over ArtCraft's local compatibility API.
//! The renderer/model runtime remains an explicitly started local service.
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    time::Duration,
};

const PREFIX: &str = "/api/v1/artcraft/opentalking";
const MAX_BODY: usize = 1024 * 1024;
const MAX_TTS_BYTES: usize = 8 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 1_500_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// A safe, actionable failure while communicating with the local renderer.
pub enum OpenTalkingError {
    /// The configured ArtCraft origin is not numeric loopback HTTP.
    Configuration,
    /// The ArtCraft compatibility API cannot be reached.
    Unavailable,
    /// The requested OpenTalking engine or an upstream capability is not ready.
    NotReady,
    /// The user supplied invalid input or an invalid job identifier.
    Input,
    /// The local service returned an unexpected or oversized response.
    InvalidResponse,
    /// A mutation may have reached the server, but its result was not confirmed.
    UnknownOutcome,
    /// The local backend answered, but its speech provider failed the request.
    SpeechUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A locally configured OpenTalking character.
pub struct Avatar {
    /// Stable identifier used by the local renderer.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// The character's configured generation driver.
    pub model_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A voice reported as configured by the local speech runtime.
pub struct Voice {
    /// Provider-qualified identifier expected by the API.
    pub id: String,
    /// Human-readable display label.
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A background scene reported by the local runtime.
pub struct Scene {
    /// Stable scene identifier.
    pub id: String,
    /// Human-readable scene name.
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A video driver the runtime reports as connected and supported.
pub struct VideoModel {
    /// The runtime's stable driver identifier.
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A video-generation job returned by the OpenTalking service.
pub struct VideoJob {
    /// Stable job identifier.
    pub id: String,
    /// User-supplied or service-generated title.
    pub title: String,
    /// Runtime status, such as queued, processing, completed or failed.
    pub status: String,
    /// Character used by the job.
    pub avatar_id: String,
    /// Driver used by the job.
    pub driver: String,
    /// Service-provided creation timestamp.
    pub created_at: String,
    /// Generated duration formatted for display.
    pub duration: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// A short synthesized speech response from the configured OpenTalking voice.
pub struct TtsAudio {
    /// The provider's audio media type, such as `audio/mpeg`.
    pub media_type: String,
    /// Encoded audio bytes, capped to 8 MiB.
    pub bytes: Vec<u8>,
}

/// Bounded HTTP client for ArtCraft's local OpenTalking compatibility API.
pub struct OpenTalkingClient {
    origin: String,
    agent: ureq::Agent,
}

impl OpenTalkingClient {
    /// Creates a client restricted to an explicit numeric loopback origin.
    pub fn new(origin: &str) -> Result<Self, OpenTalkingError> {
        let parsed = url::Url::parse(origin.trim()).map_err(|_| OpenTalkingError::Configuration)?;
        let loopback = parsed
            .host_str()
            .and_then(|host| host.parse::<IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
        if parsed.scheme() != "http"
            || !loopback
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
        {
            return Err(OpenTalkingError::Configuration);
        }
        let origin = parsed.as_str().trim_end_matches('/').to_owned();
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(2))
            .timeout_read(Duration::from_secs(8))
            .timeout_write(Duration::from_secs(8))
            .redirects(0)
            .build();
        Ok(Self { origin, agent })
    }

    /// Lists configured local characters.
    pub fn avatars(&self) -> Result<Vec<Avatar>, OpenTalkingError> {
        let rows = self.list("/avatars")?;
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            let id = text(&row, "id", 160)?;
            let name = text(&row, "name", 160)?;
            let model_type = row
                .get("model_type")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .chars()
                .take(80)
                .collect();
            result.push(Avatar {
                id,
                name,
                model_type,
            });
        }
        Ok(result)
    }

    /// Fetch the selected character's preview from the local asset endpoint.
    pub fn avatar_preview(&self, id: &str) -> Result<Vec<u8>, OpenTalkingError> {
        if id.is_empty() || id.len() > 160 || id.chars().any(char::is_control) {
            return Err(OpenTalkingError::Input);
        }
        let mut url = url::Url::parse(&format!("{}{PREFIX}/avatars/", self.origin))
            .map_err(|_| OpenTalkingError::Configuration)?;
        url.path_segments_mut()
            .map_err(|_| OpenTalkingError::Configuration)?
            .pop_if_empty()
            .push(id)
            .push("preview");
        let response = self
            .agent
            .get(url.as_str())
            .call()
            .map_err(|_| OpenTalkingError::Unavailable)?;
        if !response
            .header("Content-Type")
            .is_some_and(|value| value.starts_with("image/"))
        {
            return Err(OpenTalkingError::InvalidResponse);
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| OpenTalkingError::InvalidResponse)?;
        if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
            return Err(OpenTalkingError::InvalidResponse);
        }
        Ok(bytes)
    }

    /// Lists voices the runtime currently reports as configured.
    pub fn voices(&self) -> Result<Vec<Voice>, OpenTalkingError> {
        let rows = self.list("/voices")?;
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(Voice {
                id: text(&row, "id", 240)?,
                label: text(&row, "display_label", 160)?,
            });
        }
        Ok(result)
    }

    /// Synthesizes one short reply using a provider-qualified configured voice.
    ///
    /// The provider and voice are split from the stable `provider:voice_id`
    /// identifier returned by [`Self::voices`]. The response is capped before
    /// it is handed to an audio decoder or playback queue.
    pub fn synthesize_reply(
        &self,
        text: &str,
        provider_voice_id: &str,
    ) -> Result<TtsAudio, OpenTalkingError> {
        let text = text.trim();
        let Some((provider, voice_id)) = provider_voice_id.split_once(':') else {
            return Err(OpenTalkingError::Input);
        };
        if text.is_empty()
            || text.chars().count() > 300
            || provider.is_empty()
            || provider.len() > 48
            || !provider
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || voice_id.is_empty()
            || voice_id.len() > 240
            || voice_id.chars().any(char::is_control)
        {
            return Err(OpenTalkingError::Input);
        }

        let body = serde_json::json!({
            "text": text,
            "voice_id": voice_id,
            "provider": provider,
        });
        let response = match self
            .agent
            .post(&format!("{}{PREFIX}/tts/preview", self.origin))
            .timeout(Duration::from_secs(50))
            .set("Content-Type", "application/json")
            .set("Accept", "audio/*")
            .send_string(&body.to_string())
        {
            Ok(response) => response,
            Err(ureq::Error::Status(_, _)) => return Err(OpenTalkingError::SpeechUnavailable),
            Err(_) => return Err(OpenTalkingError::Unavailable),
        };
        let media_type = response
            .header("Content-Type")
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .filter(|value| value.starts_with("audio/"))
            .filter(|value| value.len() <= 80)
            .ok_or(OpenTalkingError::InvalidResponse)?
            .to_owned();
        if response
            .header("Content-Length")
            .and_then(|length| length.parse::<usize>().ok())
            .is_some_and(|length| length == 0 || length > MAX_TTS_BYTES)
        {
            return Err(OpenTalkingError::InvalidResponse);
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take((MAX_TTS_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| OpenTalkingError::InvalidResponse)?;
        if bytes.is_empty() || bytes.len() > MAX_TTS_BYTES {
            return Err(OpenTalkingError::InvalidResponse);
        }
        Ok(TtsAudio { media_type, bytes })
    }

    /// Lists backgrounds available from the runtime.
    pub fn scenes(&self) -> Result<Vec<Scene>, OpenTalkingError> {
        let rows = self.list("/scene-assets")?;
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(Scene {
                id: text(&row, "id", 100)?,
                name: text(&row, "name", 160)?,
            });
        }
        Ok(result)
    }

    /// Returns only model IDs reported connected by the local runtime.
    pub fn models(&self) -> Result<Vec<VideoModel>, OpenTalkingError> {
        self.models_with_status().map(|(_, models)| models)
    }

    /// Read video drivers and engine readiness from the same backend snapshot.
    pub fn models_with_status(&self) -> Result<(String, Vec<VideoModel>), OpenTalkingError> {
        let root = self.get_raw_json("/api/services")?;
        let rows = root
            .get("services")
            .and_then(Value::as_array)
            .filter(|rows| rows.len() <= 200)
            .ok_or(OpenTalkingError::InvalidResponse)?;
        let service = rows
            .iter()
            .find(|item| item.get("id").and_then(Value::as_str) == Some("opentalking"))
            .ok_or(OpenTalkingError::NotReady)?;
        if matches!(
            service.get("status").and_then(Value::as_str),
            Some("offline" | "error")
        ) {
            return Err(OpenTalkingError::NotReady);
        }
        let models = service
            .get("health")
            .and_then(|health| health.get("video_models"))
            .and_then(Value::as_array);
        let result: Vec<_> = models
            .into_iter()
            .flat_map(|m| m.iter())
            .filter_map(Value::as_str)
            .filter(|id| supported_model(id))
            .map(|id| VideoModel { id: id.to_owned() })
            .collect();
        let status = match service.get("status").and_then(Value::as_str) {
            Some("ready") => "READY",
            Some("degraded") => "ONLINE",
            _ => "STANDBY",
        };
        Ok((status.into(), result))
    }

    /// Lists video jobs, bounded to the newest 100 returned by the service.
    pub fn jobs(&self) -> Result<Vec<VideoJob>, OpenTalkingError> {
        self.list("/video-creation/jobs")?
            .into_iter()
            .take(100)
            .map(|row| parse_job(&row))
            .collect()
    }

    /// Submits one video job without automatically retrying an ambiguous POST.
    pub fn create_job(
        &self,
        avatar: &str,
        voice: &str,
        driver: &str,
        title: &str,
        script: &str,
        scene: Option<&str>,
    ) -> Result<VideoJob, OpenTalkingError> {
        if avatar.is_empty()
            || avatar.len() > 160
            || voice.is_empty()
            || voice.len() > 240
            || driver.len() > 80
            || !supported_model(driver)
            || title.trim().is_empty()
            || title.len() > 160
            || script.len() > 10000
            || scene.is_some_and(|id| id.len() > 100 || !id.starts_with("bg-"))
        {
            return Err(OpenTalkingError::Input);
        }
        let effective_script = if script.trim().is_empty() {
            "Xin chào quý khán giả, chúc mọi người một ngày tràn đầy năng lượng và niềm vui."
        } else {
            script.trim()
        };
        let body = serde_json::json!({
            "avatar_id": avatar,
            "voice_id": voice,
            "driver": driver,
            "title": title.trim(),
            "script_text": effective_script,
            "background_id": scene,
        });
        let value = self.post_json("/video-creation/jobs", &body)?;
        parse_job(&value).map_err(|_| OpenTalkingError::UnknownOutcome)
    }

    /// Stops a queued or processing job and validates its returned receipt.
    pub fn stop_job(&self, id: &str) -> Result<(), OpenTalkingError> {
        if !valid_job_id(id) {
            return Err(OpenTalkingError::Input);
        }
        let value = self.post_json(&format!("/video-creation/jobs/{id}/stop"), &Value::Null)?;
        let receipt = parse_job(&value).map_err(|_| OpenTalkingError::UnknownOutcome)?;
        if receipt.id != id {
            return Err(OpenTalkingError::UnknownOutcome);
        }
        Ok(())
    }

    /// Permanently deletes one job and validates the deletion receipt.
    pub fn delete_job(&self, id: &str) -> Result<(), OpenTalkingError> {
        if !valid_job_id(id) {
            return Err(OpenTalkingError::Input);
        }
        let route = format!("{PREFIX}/video-creation/jobs/{id}");
        match self.agent.delete(&format!("{}{route}", self.origin)).call() {
            Ok(response) if (200..300).contains(&response.status()) => {
                let receipt = parse_response(response)?;
                if receipt.get("id").and_then(Value::as_str) == Some(id)
                    && receipt.get("deleted").and_then(Value::as_bool) == Some(true)
                {
                    Ok(())
                } else {
                    Err(OpenTalkingError::UnknownOutcome)
                }
            }
            Ok(_) => Err(OpenTalkingError::Unavailable),
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                Err(OpenTalkingError::Input)
            }
            Err(_) => Err(OpenTalkingError::UnknownOutcome),
        }
    }

    /// Downloads a completed export into an app-owned folder without buffering
    /// the video in memory. A stable job filename makes repeat imports reuse it.
    pub fn download_video(&self, id: &str, directory: &Path) -> Result<PathBuf, OpenTalkingError> {
        if !valid_job_id(id) {
            return Err(OpenTalkingError::Input);
        }
        fs::create_dir_all(directory).map_err(|_| OpenTalkingError::Unavailable)?;
        let output = directory.join(format!("opentalking-{id}.mp4"));
        if output.is_file() {
            return Ok(output);
        }
        let partial = directory.join(format!("opentalking-{id}.mp4.part"));
        let response = match self
            .agent
            .get(&format!(
                "{}{PREFIX}/video-creation/jobs/{id}/video",
                self.origin
            ))
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                return Err(OpenTalkingError::NotReady);
            }
            Err(_) => return Err(OpenTalkingError::Unavailable),
        };
        let content_type = response.header("Content-Type").unwrap_or("");
        if !(content_type.starts_with("video/") || content_type == "application/octet-stream") {
            return Err(OpenTalkingError::InvalidResponse);
        }
        if response
            .header("Content-Length")
            .and_then(|length| length.parse::<u64>().ok())
            .is_some_and(|length| length == 0 || length > MAX_VIDEO_BYTES)
        {
            return Err(OpenTalkingError::InvalidResponse);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .map_err(|_| OpenTalkingError::Unavailable)?;
        let mut limited = response.into_reader().take(MAX_VIDEO_BYTES + 1);
        let copied = match std::io::copy(&mut limited, &mut file) {
            Ok(copied) => copied,
            Err(_) => {
                drop(file);
                let _ = fs::remove_file(&partial);
                return Err(OpenTalkingError::Unavailable);
            }
        };
        if copied == 0 || copied > MAX_VIDEO_BYTES {
            drop(file);
            let _ = fs::remove_file(&partial);
            return Err(OpenTalkingError::InvalidResponse);
        }
        if file.flush().is_err() {
            drop(file);
            let _ = fs::remove_file(&partial);
            return Err(OpenTalkingError::Unavailable);
        }
        drop(file);
        if fs::rename(&partial, &output).is_err() {
            let _ = fs::remove_file(&partial);
            return Err(OpenTalkingError::Unavailable);
        }
        Ok(output)
    }

    fn list(&self, suffix: &str) -> Result<Vec<Value>, OpenTalkingError> {
        self.get_json(&format!("{PREFIX}{suffix}"))?
            .as_array()
            .filter(|rows| rows.len() <= 500)
            .cloned()
            .ok_or(OpenTalkingError::InvalidResponse)
    }

    fn get_json(&self, route: &str) -> Result<Value, OpenTalkingError> {
        let response = match self.agent.get(&format!("{}{route}", self.origin)).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                return Err(OpenTalkingError::NotReady);
            }
            Err(_) => return Err(OpenTalkingError::Unavailable),
        };
        parse_response(response)
    }

    fn get_raw_json(&self, route: &str) -> Result<Value, OpenTalkingError> {
        let response = match self.agent.get(&format!("{}{route}", self.origin)).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                return Err(OpenTalkingError::NotReady);
            }
            Err(_) => return Err(OpenTalkingError::Unavailable),
        };
        read_json(response)
    }

    fn post_json(&self, route: &str, body: &Value) -> Result<Value, OpenTalkingError> {
        let result = self
            .agent
            .post(&format!("{}{PREFIX}{route}", self.origin))
            .timeout(Duration::from_secs(35))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string());
        match result {
            Ok(response) => parse_response(response),
            Err(ureq::Error::Status(code, _)) if (400..500).contains(&code) => {
                Err(OpenTalkingError::Input)
            }
            Err(_) => Err(OpenTalkingError::UnknownOutcome),
        }
    }
}

fn parse_response(response: ureq::Response) -> Result<Value, OpenTalkingError> {
    let root = read_json(response)?;
    if root.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(OpenTalkingError::InvalidResponse);
    }
    root.get("data")
        .cloned()
        .ok_or(OpenTalkingError::InvalidResponse)
}

fn read_json(response: ureq::Response) -> Result<Value, OpenTalkingError> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((MAX_BODY + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| OpenTalkingError::InvalidResponse)?;
    if bytes.len() > MAX_BODY {
        return Err(OpenTalkingError::InvalidResponse);
    }
    serde_json::from_slice(&bytes).map_err(|_| OpenTalkingError::InvalidResponse)
}

fn parse_job(value: &Value) -> Result<VideoJob, OpenTalkingError> {
    let id = text(value, "id", 64)?;
    if !valid_job_id(&id) {
        return Err(OpenTalkingError::InvalidResponse);
    }
    Ok(VideoJob {
        id,
        title: optional_text(value, "title", 160),
        status: optional_text(value, "status", 48),
        avatar_id: optional_text(value, "avatar_id", 160),
        driver: optional_text(value, "driver", 80),
        created_at: optional_text(value, "created_at", 80),
        duration: value
            .get("duration_seconds")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| format!("{n:.1}s"))
            .unwrap_or_default(),
    })
}

fn text(value: &Value, field: &str, max: usize) -> Result<String, OpenTalkingError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= max)
        .map(str::to_owned)
        .ok_or(OpenTalkingError::InvalidResponse)
}

fn optional_text(value: &Value, field: &str, max: usize) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(max)
        .collect()
}

fn supported_model(model: &str) -> bool {
    matches!(
        model,
        "flashtalk"
            | "flashhead"
            | "fasterliveportrait"
            | "musetalk"
            | "quicktalk"
            | "wav2lip"
            | "sadtalker"
    )
}

fn valid_job_id(id: &str) -> bool {
    id.strip_prefix("job_")
        .is_some_and(|hex| hex.len() == 16 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::{OpenTalkingClient, OpenTalkingError};
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    #[test]
    fn missing_runtime_models_are_not_reported_as_available() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0u8; 2048];
            let _ = socket.read(&mut bytes);
            let body = r#"{"services":[{"id":"opentalking","status":"standby","health":{"video_models":[]}}]}"#;
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        });
        let (status, models) = OpenTalkingClient::new(&origin)
            .unwrap()
            .models_with_status()
            .unwrap();
        assert_eq!(status, "STANDBY");
        assert!(models.is_empty());
        server.join().unwrap();
    }

    #[test]
    fn list_endpoints_unwrap_the_backend_envelope_exactly_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (route, data) in [
                (
                    "avatars",
                    serde_json::json!([{"id":"anchor","name":"Anchor","model_type":"flashhead"}]),
                ),
                (
                    "voices",
                    serde_json::json!([{"id":"edge:vi-VN-HoaiMyNeural","display_label":"HoaiMy"}]),
                ),
                (
                    "scene-assets",
                    serde_json::json!([{"id":"bg-default","name":"Studio"}]),
                ),
                ("video-creation/jobs", serde_json::json!([])),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 1024];
                    let count = socket.read(&mut bytes).unwrap();
                    request.extend_from_slice(&bytes[..count]);
                    if count == 0 || request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        break;
                    }
                }
                assert!(
                    String::from_utf8_lossy(&request)
                        .starts_with(&format!("GET /api/v1/artcraft/opentalking/{route} "))
                );
                let body = serde_json::json!({"ok":true,"data":data}).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let client = OpenTalkingClient::new(&origin).unwrap();
        assert_eq!(client.avatars().unwrap()[0].id, "anchor");
        assert_eq!(client.voices().unwrap()[0].id, "edge:vi-VN-HoaiMyNeural");
        assert_eq!(client.scenes().unwrap()[0].id, "bg-default");
        assert!(client.jobs().unwrap().is_empty());
        server.join().unwrap();
    }

    #[test]
    #[ignore = "requires explicitly configured running local OpenTalking backend"]
    fn running_backend_catalogue_smoke() {
        let origin =
            std::env::var("CONCAT_OPEN_TALKING_SMOKE_ORIGIN").expect("explicit smoke origin");
        let client = OpenTalkingClient::new(&origin).unwrap();
        let avatars = client.avatars().expect("configured avatars");
        let voices = client.voices().expect("configured voices");
        let scenes = client.scenes().expect("scene catalogue");
        let (status, models) = client.models_with_status().expect("connected video models");
        assert!(matches!(status.as_str(), "READY" | "PARTIALLY READY"));
        let jobs = client.jobs().expect("video job history");
        assert!(!avatars.is_empty());
        assert!(!voices.is_empty());
        assert!(!models.is_empty());
        assert!(
            !client
                .avatar_preview(&avatars[0].id)
                .expect("selected avatar preview")
                .is_empty()
        );
        println!(
            "OpenTalking native adapter: {} avatars, {} voices, {} backgrounds, {} drivers, {} jobs",
            avatars.len(),
            voices.len(),
            scenes.len(),
            models.len(),
            jobs.len()
        );
    }

    #[test]
    fn create_job_sends_native_form_and_accepts_backend_receipt() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut bytes = [0; 1024];
                let count = socket.read(&mut bytes).unwrap();
                request.extend_from_slice(&bytes[..count]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                if count == 0 {
                    break;
                }
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("POST /api/v1/artcraft/opentalking/video-creation/jobs "));
            let body: serde_json::Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["avatar_id"], "anchor");
            assert_eq!(body["voice_id"], "edge:vi-VN-HoaiMyNeural");
            assert_eq!(body["driver"], "sadtalker");
            assert_eq!(body["script_text"], "Xin chào");
            let body = serde_json::json!({"ok":true,"data":{"id":"job_0123456789abcdef","status":"queued","title":"Native video","avatar_id":"anchor","driver":"sadtalker"}}).to_string();
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let client = OpenTalkingClient::new(&origin).unwrap();
        let job = client
            .create_job(
                "anchor",
                "edge:vi-VN-HoaiMyNeural",
                "sadtalker",
                "Native video",
                "Xin chào",
                None,
            )
            .unwrap();
        assert_eq!(job.id, "job_0123456789abcdef");
        assert_eq!(job.status, "queued");
        server.join().unwrap();
    }

    #[test]
    fn tts_rejects_missing_voice_provider_and_oversize_reply_before_network() {
        let client = OpenTalkingClient::new("http://127.0.0.1:1").expect("loopback client");
        assert_eq!(
            client.synthesize_reply("hello", "unqualified-voice"),
            Err(OpenTalkingError::Input)
        );
        assert_eq!(
            client.synthesize_reply("hello", "edge:"),
            Err(OpenTalkingError::Input)
        );
        assert_eq!(
            client.synthesize_reply(&"x".repeat(301), "edge:voice"),
            Err(OpenTalkingError::Input)
        );
    }
}
