// SPDX-License-Identifier: AGPL-3.0-or-later
//! Export prepared clips from an authored project folder; this does not clone a person's identity.
use concat_studio::{
    live_adapters::{MotionClip, SceneManifest},
    live_scene::AvatarManifest,
};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 5 {
        return Err("Usage: kedi_scene_manifest ASSET_ROOT CONSENT_REF RIGHTS_EXPIRES_AT --commercial-approved".into());
    }
    if args[4] != "--commercial-approved" || args[2].trim().is_empty() {
        return Err(
            "Explicit commercial rights acknowledgement and consent reference required".into(),
        );
    }
    let root = Path::new(&args[1]).canonicalize()?;
    let mut clips = Vec::new();
    let ffprobe = std::env::var_os("KEDI_FFPROBE")
        .ok_or("Set KEDI_FFPROBE to the approved local ffprobe executable")?;
    for (file, gesture) in [
        ("idle.mp4", "idle"),
        ("smile.mp4", "smile"),
        ("present_product.mp4", "present_product"),
        ("hand_detail.mp4", "hand_detail"),
        ("attentive.mp4", "attentive"),
    ] {
        let path = root.join(file);
        if !path.is_file() {
            continue;
        }
        let mut input = std::fs::File::open(&path)?;
        if input.metadata()?.len() > 25 * 1024 * 1024 {
            return Err("Prepared clips must be at most 25 MB".into());
        }
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let output = std::process::Command::new(&ffprobe)
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,width,height",
                "-show_entries",
                "format=duration",
                "-of",
                "json",
            ])
            .arg(&path)
            .output()?;
        if !output.status.success() || output.stdout.len() > 65536 {
            return Err("ffprobe rejected prepared clip".into());
        }
        let info: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let duration = info["format"]["duration"]
            .as_str()
            .and_then(|d| d.parse::<f64>().ok())
            .filter(|d| d.is_finite() && (1.0..=10.05).contains(d))
            .ok_or("Prepared clip duration must be 1–10 seconds")?;
        if info["streams"][0]["width"].as_u64().unwrap_or(0) == 0
            || info["streams"][0]["height"].as_u64().unwrap_or(0) == 0
        {
            return Err("Prepared clip must contain video".into());
        }
        clips.push(MotionClip {
            clip_id: gesture.into(),
            path: file.into(),
            sha256: format!("{:x}", hasher.finalize()),
            duration_ms: (duration * 1000.0).round() as u32,
            gesture: gesture.into(),
            emotion: "neutral".into(),
            loopable: gesture == "idle",
        });
    }
    if !clips.iter().any(|c| c.gesture == "idle") {
        return Err("ASSET_ROOT must contain idle.mp4".into());
    }
    let manifest = AvatarManifest {
        schema_version: 1,
        avatar_id: "prepared-host".into(),
        consent_ref: args[2].clone(),
        commercial_allowed: true,
        rights_expires_at: args[3].clone(),
        motions: SceneManifest {
            schema_version: 1,
            manifest_id: "prepared-motions".into(),
            revision: 1,
            clips,
        },
    };
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}
