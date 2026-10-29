// SPDX-License-Identifier: AGPL-3.0-or-later
//! Owned adapter for Factory editing; other storyboard consumers stay unchanged.
use std::{fs, path::{Path, PathBuf}, process::Stdio, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};

/// Frozen UI snapshot, saved with every render for review and reproduction.
#[derive(serde::Serialize)]
#[allow(missing_docs)]
pub struct EditRequest {
    pub paths: Vec<PathBuf>,
    pub seconds: f64,
    pub size: (u32, u32),
    pub script: String,
    pub audio_mode: i32,
    pub voice_index: usize,
    pub audio_path: String,
}

/// Locate the existing speech environment, with an explicit deployment override.
pub fn python(repo: &Path) -> PathBuf {
    let repo=repo.canonicalize().unwrap_or_else(|_|repo.to_owned());
    std::env::var_os("KEDI_TTS_PYTHON").map(PathBuf::from)
        .or_else(|| repo.parent().map(|p| p.join("artcraft/engines/opentalking/.venv/Scripts/python.exe")).filter(|p| p.is_file()))
        .unwrap_or_else(|| "python".into())
}

/// Render in a fresh owned folder; cancellation terminates its process tree.
pub fn render(python: &Path, ffmpeg: &Path, font: &Path, request: &EditRequest,
              root: &Path, cancel: &AtomicBool, mut progress: impl FnMut(String)) -> Result<PathBuf, String> {
    render_target(python,ffmpeg,font,request,root,None,cancel,&mut progress)
}

/// Control workflows may require a duration; never fill a long request with silent padding.
pub fn render_target(python: &Path, ffmpeg: &Path, font: &Path, request: &EditRequest,
              root: &Path, target_duration:Option<f64>, cancel: &AtomicBool, mut progress: impl FnMut(String)) -> Result<PathBuf, String> {
    if target_duration.is_some_and(|n|!n.is_finite() || !(1.0..=600.0).contains(&n)) {
        return Err("Thời lượng yêu cầu phải trong 1–600 giây.".into());
    }
    if request.paths.is_empty() || request.paths.len()>100 || !request.seconds.is_finite()
        || !(0.5..=600.0).contains(&request.seconds) || request.script.len()>12000
        || !(0..=2).contains(&request.audio_mode) || request.voice_index>1 {
        return Err("Kiểm tra tư liệu, kịch bản, giọng và thời lượng trước khi dựng.".into());
    }
    if request.audio_mode==0 && request.script.trim().is_empty() {
        return Err("Nhập kịch bản lời đọc trước khi dựng.".into());
    }
    fs::create_dir(root).map_err(|e| e.to_string())?;
    let mut config=serde_json::to_value(request).map_err(|e|e.to_string())?;
    config["ffmpeg"]=serde_json::json!(ffmpeg);
    config["font"]=serde_json::json!(font);
    if let Some(n)=target_duration {config["target_duration"]=serde_json::json!(n);}
    fs::write(root.join("request.json"), serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?).map_err(|e|e.to_string())?;
    fs::write(root.join("factory-edit.py"), include_str!("factory_edit.py")).map_err(|e|e.to_string())?;
    let log=fs::File::create(root.join("edit-progress.log")).map_err(|e|e.to_string())?;
    let errors=fs::File::create(root.join("edit-error.log")).map_err(|e|e.to_string())?;
    let mut command=crate::lux::make_command(python);
    command.current_dir(root).args(["-u", "factory-edit.py", "request.json"])
        .env("PYTHONUTF8", "1").env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null()).stdout(log).stderr(errors);
    let mut child=command.spawn().map_err(|_| "Không tìm thấy Python cho giọng đọc; cấu hình KEDI_TTS_PYTHON tới môi trường có edge_tts.")?;
    #[cfg(windows)]
    let _job=match crate::local_ai::owned_job(&child) {
        Ok(job)=>job,
        Err(_)=>{let _=child.kill();let _=child.wait();return Err("Không quản lý được tiến trình dựng.".into());}
    };
    let started=Instant::now();
    let mut last=String::new();
    loop {
        if cancel.load(Ordering::Acquire) || started.elapsed()>Duration::from_secs(1800) {
            let _=child.kill();let _=child.wait();
            return Err("Đã dừng dựng video.".into());
        }
        let current=fs::read_to_string(root.join("edit-progress.log")).unwrap_or_default();
        let line=current.lines().last().unwrap_or_default();
        if line!=last {last=line.into();progress(last.clone());}
        match child.try_wait() {
            Ok(Some(status)) if status.success()=>return Ok(root.join("film.mp4")),
            Ok(Some(_))=>return Err(fs::read_to_string(root.join("edit-error.log")).unwrap_or_else(|_|"Dựng video thất bại.".into())),
            Ok(None)=>std::thread::sleep(Duration::from_millis(200)),
            Err(_)=>{let _=child.kill();let _=child.wait();return Err("Không đọc được tiến trình dựng.".into());}
        }
    }
}
