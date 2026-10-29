// SPDX-License-Identifier: AGPL-3.0-or-later
//! Real file/decoder/audio/caption-frame checks. No synthetic success receipts.
use super::{Asset, QualityReport};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub fn hash(path: &Path) -> Result<String, String> {
    let info = std::fs::symlink_metadata(path).map_err(|_| "Không tìm thấy file media")?;
    if !info.is_file()
        || info.file_type().is_symlink()
        || info.len() == 0
        || info.len() > 2 * 1024 * 1024 * 1024
    {
        return Err("File media không hợp lệ hoặc quá lớn".into());
    }
    let mut file = std::fs::File::open(path).map_err(|_| "Không mở được media")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|_| "Không đọc được media")?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires explicitly configured FFmpeg; renders actual local test media, no external model or publishing"]
    fn real_media_decodes_has_audible_audio_and_measured_in_frame_captions() {
        let ffmpeg = std::env::var_os("CONCAT_TEST_FFMPEG")
            .map(std::path::PathBuf::from)
            .expect("Set CONCAT_TEST_FFMPEG");
        let root =
            std::env::temp_dir().join(format!("campaign-real-qc-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("captions.srt"),
            "1\n00:00:00,000 --> 00:00:02,000\nActual quality check\n",
        )
        .unwrap();
        std::fs::write(
            root.join("font.ttf"),
            include_bytes!("../../../concat-text/fonts/HankenGrotesk-Regular.ttf"),
        )
        .unwrap();
        let output = root.join("film.mp4");
        let cancel = AtomicBool::new(false);
        run(Command::new(&ffmpeg).current_dir(&root).args(["-hide_banner","-nostdin","-y","-f","lavfi","-i","testsrc2=size=720x1280:rate=24","-f","lavfi","-i","sine=frequency=660:sample_rate=48000","-vf","subtitles=captions.srt:fontsdir=.:force_style='FontName=Hanken Grotesk,FontSize=20,Outline=2,Shadow=0,MarginV=32,Alignment=2'","-t","2","-c:v","libx264","-preset","ultrafast","-threads","2","-crf","30","-c:a","aac","-movflags","+faststart"]).arg(&output),&cancel,Duration::from_secs(60)).unwrap();
        let asset = Asset {
            path: output.clone(),
            sha256: hash(&output).unwrap(),
            rights: "local generated test source".into(),
            provider: "ffmpeg-test".into(),
        };
        let result = verify(&asset, &ffmpeg, 2.0, &cancel).unwrap();
        assert!(result.decoded && result.audio && result.audible && result.subtitles_in_frame);
        assert!(verify(&asset, &ffmpeg, 10.0, &cancel).is_err());
        std::fs::write(&output, b"tampered").unwrap();
        assert!(verify(&asset, &ffmpeg, 2.0, &cancel).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
pub fn run(
    command: &mut Command,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<(String, String), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Không mở được engine đã cấu hình")?;
    #[cfg(windows)]
    let _owner =
        crate::local_ai::owned_job(&child).map_err(|_| "Không quản lý được tiến trình engine")?;
    fn drain(mut reader: impl Read) -> String {
        let mut all = Vec::new();
        let mut buffer = [0; 8192];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 {
                break;
            }
            if all.len() + n <= 2 * 1024 * 1024 {
                all.extend_from_slice(&buffer[..n]);
            }
        }
        String::from_utf8_lossy(&all).into_owned()
    }
    let out = child.stdout.take().ok_or("Engine stdout unavailable")?;
    let err = child.stderr.take().ok_or("Engine stderr unavailable")?;
    let out = std::thread::spawn(move || drain(out));
    let err = std::thread::spawn(move || drain(err));
    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Acquire) || started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            return Err("Engine đã dừng hoặc hết thời gian dự trù".into());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "Không đọc được trạng thái engine")?
        {
            break status;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stdout = out.join().map_err(|_| "Engine output unavailable")?;
    let stderr = err.join().map_err(|_| "Engine output unavailable")?;
    if !status.success() {
        return Err(format!(
            "Engine kiểm tra không thành công: {}",
            stderr
                .chars()
                .rev()
                .take(1500)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        ));
    }
    Ok((stdout, stderr))
}
pub fn verify(
    asset: &Asset,
    ffmpeg: &Path,
    target: f64,
    cancel: &AtomicBool,
) -> Result<QualityReport, String> {
    let sha = hash(&asset.path)?;
    if sha != asset.sha256 {
        return Err("File đã thay đổi sau render; cần kiểm tra lại".into());
    }
    let probe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    let (output, _) = run(
        Command::new(probe)
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(&asset.path),
        cancel,
        Duration::from_secs(30),
    )?;
    let info: serde_json::Value =
        serde_json::from_str(&output).map_err(|_| "ffprobe không trả metadata hợp lệ")?;
    let streams = info["streams"].as_array().ok_or("Không có luồng media")?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or("Không có video")?;
    let audio = streams.iter().any(|s| s["codec_type"] == "audio");
    let width = video["width"].as_u64().ok_or("Không có chiều rộng")? as u32;
    let height = video["height"].as_u64().ok_or("Không có chiều cao")? as u32;
    let seconds = info["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or("Thời lượng không hợp lệ")?;
    if !seconds.is_finite()
        || (seconds - target).abs() > 0.75
        || !audio
        || (width, height) != (720, 1280)
        || video["codec_name"] != "h264"
    {
        return Err("Video chưa đúng thời lượng, H.264 720×1280 hoặc thiếu âm thanh".into());
    }
    run(
        Command::new(ffmpeg)
            .args(["-v", "error", "-nostdin", "-xerror", "-i"])
            .arg(&asset.path)
            .args(["-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-"]),
        cancel,
        Duration::from_secs((target * 3.0 + 30.0) as u64),
    )?;
    let (_, volume) = run(
        Command::new(ffmpeg)
            .args(["-hide_banner", "-nostdin", "-i"])
            .arg(&asset.path)
            .args(["-vn", "-af", "volumedetect", "-f", "null", "-"]),
        cancel,
        Duration::from_secs((target * 2.0 + 30.0) as u64),
    )?;
    let mean = volume.lines().find_map(|line| {
        line.split_once("mean_volume:")
            .and_then(|(_, tail)| tail.split_whitespace().next()?.parse::<f64>().ok())
    });
    if mean.is_none_or(|n| !n.is_finite() || n < -50.0) {
        return Err("Âm thanh câm hoặc quá nhỏ; chưa cho phép đăng".into());
    }
    let root = asset.path.parent().ok_or("Không có thư mục render")?;
    let captions = std::fs::read_to_string(root.join("captions.srt"))
        .map_err(|_| "Thiếu phụ đề để kiểm tra khung hình")?;
    fn timestamp(value: &str) -> Option<f64> {
        let n: Vec<f64> = value
            .trim()
            .split([':', ','])
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        if n.len() != 4 {
            return None;
        }
        Some(n[0] * 3600.0 + n[1] * 60.0 + n[2] + n[3] / 1000.0)
    }
    let mut samples = vec![];
    for line in captions
        .lines()
        .filter(|line| line.contains(" --> "))
        .take(501)
    {
        let (a, b) = line.split_once(" --> ").ok_or("Subtitle time invalid")?;
        let a = timestamp(a).ok_or("Subtitle start invalid")?;
        let b = timestamp(b).ok_or("Subtitle end invalid")?;
        if a < 0.0 || b <= a || b > seconds + 0.1 {
            return Err("Thời gian phụ đề nằm ngoài video".into());
        }
        samples.push(((a + b) / 2.0 * 24.0).round() as u64);
    }
    if samples.is_empty() || samples.len() > 500 {
        return Err("Phụ đề trống hoặc vượt giới hạn kiểm tra".into());
    }
    let select = samples
        .iter()
        .map(|n| format!("eq(n\\,{n})"))
        .collect::<Vec<_>>()
        .join("+");
    let filter=format!("select='{select}',subtitles=captions.srt:fontsdir=.:force_style='FontName=Hanken Grotesk,FontSize=20,Outline=2,Shadow=0,MarginV=32,Alignment=2',bbox=min_val=16");
    let (_, bounds) = run(
        Command::new(ffmpeg).current_dir(root).args([
            "-hide_banner",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=black:s={width}x{height}:r=24:d={seconds}"),
            "-vf",
            &filter,
            "-fps_mode",
            "vfr",
            "-an",
            "-f",
            "null",
            "-",
        ]),
        cancel,
        Duration::from_secs((target * 2.0 + 30.0) as u64),
    )?;
    let mut checked = 0;
    for line in bounds
        .lines()
        .filter(|line| line.contains("Parsed_bbox") && line.contains("x1:"))
    {
        let number = |field: &str| {
            line.split_once(field)
                .and_then(|(_, s)| s.split_whitespace().next()?.parse::<u32>().ok())
        };
        if let (Some(x1), Some(x2), Some(y1), Some(y2)) =
            (number("x1:"), number("x2:"), number("y1:"), number("y2:"))
        {
            checked += 1;
            if x1 < 8 || y1 < 8 || x2 >= width - 8 || y2 >= height - 8 {
                return Err("Phụ đề chạm hoặc bị cắt ở mép khung hình".into());
            }
        }
    }
    if checked == 0 {
        return Err("Chưa đo được vùng phụ đề; không tự báo đạt".into());
    }
    Ok(QualityReport {
        sha256: sha,
        seconds,
        width,
        height,
        audio,
        audible: true,
        decoded: true,
        subtitles_in_frame: true,
        checked_at: super::store::now(),
    })
}
