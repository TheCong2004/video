// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bounded, cached thumbnail preparation off the UI thread. No original is modified.
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{Duration, Instant},
};

static IO: Mutex<()> = Mutex::new(());
static TEMP_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn temporary(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}-{}.tmp.png",
        std::process::id(),
        TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}
#[derive(Clone, Default)]
pub(crate) struct Prepared {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub duration: String,
}
impl Prepared {
    pub(crate) fn image(&self) -> slint::Image {
        if self.width == 0 || self.height == 0 {
            return Default::default();
        }
        slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
            &self.pixels,
            self.width,
            self.height,
        ))
    }
}
pub(crate) fn duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "Video".into();
    }
    let n = seconds.ceil().min(86400.0 * 7.0) as u64;
    if n >= 3600 {
        format!("{}:{:02}:{:02}", n / 3600, n / 60 % 60, n % 60)
    } else {
        format!("{}:{:02}", n / 60, n % 60)
    }
}
fn decode(bytes: &[u8], duration: String) -> Option<Prepared> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?.thumbnail(320, 180).to_rgba8();
    Some(Prepared {
        width: img.width(),
        height: img.height(),
        pixels: img.into_raw(),
        duration,
    })
}
fn cached(path: &Path, duration: String) -> Option<Prepared> {
    if path.metadata().ok()?.len() > 4 * 1024 * 1024 {
        return None;
    }
    decode(&std::fs::read(path).ok()?, duration)
}
fn cache_path(root: &Path, identity: &str) -> PathBuf {
    root.join(format!("{:x}.png", Sha256::digest(identity.as_bytes())))
}
fn save(path: &Path, preview: &Prepared) {
    let temporary = temporary(path);
    if let Some(img) =
        image::RgbaImage::from_raw(preview.width, preview.height, preview.pixels.clone())
    {
        if img.save(&temporary).is_ok() {
            let _ = std::fs::rename(&temporary, path);
        }
        // Exact freshly-created cache temporary only, never an original video.
        let _ = std::fs::remove_file(&temporary);
    }
}
// TikTok cover CDNs also return AVIF. Decode unsupported image formats with the
// bundled FFmpeg, not an unbounded codec allocation on the UI thread.
fn extract_frame(source: &Path, output: &Path, seconds: f64) -> Option<()> {
    let mut cmd = concat_studio::lux::make_command(&crate::studio_scenes::ffmpeg().ok()?);
    cmd.args([
        "-hide_banner",
        "-v",
        "error",
        "-nostdin",
        "-n",
        "-max_alloc",
        "67108864",
        "-threads",
        "1",
        "-ss",
    ])
    .arg(format!("{seconds:.3}"))
    .arg("-i")
    .arg(source)
    .args([
        "-frames:v",
        "1",
        "-vf",
        "scale=320:180:force_original_aspect_ratio=decrease",
        "-filter_threads",
        "1",
        "-threads",
        "1",
    ])
    .arg(output)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then_some(()),
            Ok(None) if started.elapsed() < Duration::from_secs(8) => {}
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}
fn decode_cover(bytes: &[u8], label: String, path: &Path) -> Option<Prepared> {
    if let Some(preview) = decode(bytes, label.clone()) {
        return Some(preview);
    }
    // Only AVIF takes this fallback. Other unknown data never reaches FFmpeg.
    if bytes.len() < 16 || &bytes[4..8] != b"ftyp" || !matches!(&bytes[8..12], b"avif" | b"avis") {
        return None;
    }
    // AVIF Image Spatial Extents: reject missing/oversized dimensions before
    // invoking a codec. All matching extents must fit the image decode budget.
    let mut extents = 0;
    for i in 4..=bytes.len().saturating_sub(16) {
        if &bytes[i..i + 4] == b"ispe" {
            let width = u32::from_be_bytes(bytes[i + 8..i + 12].try_into().ok()?);
            let height = u32::from_be_bytes(bytes[i + 12..i + 16].try_into().ok()?);
            if width == 0
                || height == 0
                || width > 8192
                || height > 8192
                || u64::from(width) * u64::from(height) > 16 * 1024 * 1024
            {
                return None;
            }
            extents += 1;
        }
    }
    if extents == 0 {
        return None;
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    let input = temporary(path).with_extension("avif");
    let output = temporary(path);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&input)
            .ok()?;
        file.write_all(bytes).ok()?;
        drop(file);
        extract_frame(&input, &output, 0.0)?;
        cached(&output, label)
    })();
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
    result
}
pub(crate) fn cover_smoke(source: &Path, root: &Path) {
    let bytes = std::fs::read(source).expect("explicit cover fixture");
    assert!(bytes.len() <= 4 * 1024 * 1024);
    let preview = decode_cover(&bytes, "Video".into(), &root.join("cover-smoke.png"))
        .expect("actual CDN cover must decode");
    assert!(
        preview.width > 0 && preview.width <= 320 && preview.height > 0 && preview.height <= 180
    );
    save(&root.join("cover-smoke.png"), &preview);
    assert!(!preview.pixels.is_empty());
}
pub(crate) fn remote(cover: &str, video: &str, seconds: u32, root: &Path) -> Prepared {
    let label = duration(seconds as f64);
    let _guard = IO.lock().unwrap_or_else(|p| p.into_inner());
    let path = cache_path(root, video);
    if let Some(preview) = cached(&path, label.clone()) {
        return preview;
    }
    let result = concat_studio::lux::fetch_thumbnail(cover)
        .ok()
        .and_then(|bytes| decode_cover(&bytes, label.clone(), &path));
    if let Some(preview) = result {
        let _ = std::fs::create_dir_all(root);
        save(&path, &preview);
        preview
    } else {
        Prepared {
            duration: label,
            ..Default::default()
        }
    }
}
pub(crate) fn local(source: &Path, root: &Path) -> Prepared {
    let _guard = IO.lock().unwrap_or_else(|p| p.into_inner());
    let info = concat_media::probe::probe(source).ok();
    let seconds = info
        .as_ref()
        .and_then(|i| i.duration)
        .map(|d| d.as_f64())
        .unwrap_or(0.0);
    let label = duration(seconds);
    let fallback = || Prepared {
        duration: label.clone(),
        ..Default::default()
    };
    if info.as_ref().is_none_or(|i| i.video.is_none()) {
        return fallback();
    }
    let Ok(meta) = source.metadata() else {
        return fallback();
    };
    let identity = format!(
        "{}:{}:{:?}",
        source.display(),
        meta.len(),
        meta.modified().ok()
    );
    let path = cache_path(root, &identity);
    if let Some(preview) = cached(&path, label.clone()) {
        return preview;
    }
    if std::fs::create_dir_all(root).is_err() {
        return fallback();
    }
    let temporary = temporary(&path);
    let result = (|| {
        extract_frame(source, &temporary, (seconds / 3.0).clamp(0.0, 1.0))?;
        let preview = cached(&temporary, label.clone())?;
        let _ = std::fs::rename(&temporary, &path);
        Some(preview)
    })();
    let _ = std::fs::remove_file(&temporary);
    result.unwrap_or_else(fallback)
}
