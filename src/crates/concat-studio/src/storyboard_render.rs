// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic local 2D/layered assembly of real shot assets using FFmpeg.
use crate::storyboard::{Shot, Storyboard};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Frozen assembly dimensions. Rendering never modifies the source assets.
#[derive(Clone, Copy)]
#[allow(missing_docs)]
pub struct RenderSize {
    pub width: u32,
    pub height: u32,
}
/// Render a whole board with optional per-shot progress and cooperative stop.
pub fn render_board(
    ffmpeg: &Path,
    font: &Path,
    board: &Storyboard,
    root: &Path,
    size: RenderSize,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<PathBuf, String> {
    board.validate()?;
    if board.shots.is_empty() {
        return Err("Chưa có phân cảnh".into());
    }
    if !matches!(
        (size.width, size.height),
        (720, 1280) | (1280, 720) | (720, 720) | (1080, 1920) | (1920, 1080) | (1080, 1080)
    ) {
        return Err("Kích thước xuất chưa được hỗ trợ".into());
    }
    fs::create_dir(root).map_err(|_| "Không tạo được thư mục render mới")?;
    let mut list = String::new();
    for (i, shot) in board.shots.iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            return Err("Đã dừng dựng phim".into());
        }
        progress(i, board.shots.len());
        let name = format!("shot-{i:04}.mp4");
        render_shot(ffmpeg, font, shot, root, &name, size, cancel)
            .map_err(|error| format!("Cảnh {} ({}): {error}", i + 1, shot.title))?;
        list.push_str(&format!("file '{name}'\n"));
    }
    fs::write(root.join("assembly.txt"), list).map_err(|_| "Không lưu được danh sách cảnh")?;
    fs::write(root.join("captions.srt"), board.srt()?).map_err(|_| "Không lưu được phụ đề")?;
    fs::write(
        root.join("storyboard.json"),
        serde_json::to_vec_pretty(board).map_err(|_| "Phân cảnh không hợp lệ")?,
    )
    .map_err(|_| "Không lưu được phân cảnh")?;
    let args = [
        "-hide_banner",
        "-nostdin",
        "-n",
        "-f",
        "concat",
        "-safe",
        "1",
        "-i",
        "assembly.txt",
        "-c",
        "copy",
        "-movflags",
        "+faststart",
        "film.mp4",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    execute(ffmpeg, root, args, cancel)?;
    progress(board.shots.len(), board.shots.len());
    // Parts are strictly generated files in this newly reserved directory.
    for i in 0..board.shots.len() {
        let _ = fs::remove_file(root.join(format!("shot-{i:04}.mp4")));
    }
    Ok(root.join("film.mp4"))
}
fn still(path: &Path) -> bool {
    path.extension().and_then(|v| v.to_str()).is_some_and(|v| {
        ["png", "jpg", "jpeg", "webp", "bmp"]
            .iter()
            .any(|s| v.eq_ignore_ascii_case(s))
    })
}
fn input(args: &mut Vec<OsString>, path: &Path) {
    if still(path) {
        args.extend(["-loop".into(), "1".into()]);
    } else {
        args.extend(["-stream_loop".into(), "-1".into()]);
    }
    args.extend(["-i".into(), path.as_os_str().to_owned()]);
}
fn render_shot(
    ffmpeg: &Path,
    font: &Path,
    shot: &Shot,
    root: &Path,
    name: &str,
    size: RenderSize,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let source = shot
        .generated
        .as_ref()
        .or(shot.background.as_ref())
        .ok_or("Cảnh chưa có ảnh hoặc video")?;
    for path in [
        Some(source),
        shot.character.as_ref(),
        shot.voice.as_ref(),
        shot.music.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if !path.is_file() {
            return Err("Tư liệu không còn tồn tại; chọn lại file".into());
        }
    }
    if shot.layers.iter().any(|l| !l.path.is_file()) {
        return Err("Một lớp hình không còn tồn tại".into());
    }
    if !shot.narration.trim().is_empty() && shot.voice.is_none() {
        return Err(
            "Cảnh có lời dẫn nhưng chưa có giọng đọc. Tạo giọng hoặc tải âm thanh trước khi dựng"
                .into(),
        );
    }
    let mut args = vec!["-hide_banner".into(), "-nostdin".into(), "-n".into()];
    input(&mut args, source);
    let (w, h) = (size.width, size.height);
    let mut next = 1;
    let foreground = shot.character.as_ref().map(|path| {
        let i = next;
        next += 1;
        input(&mut args, path);
        i
    });
    let layers: Vec<_> = shot
        .layers
        .iter()
        .map(|layer| {
            let i = next;
            next += 1;
            input(&mut args, &layer.path);
            (i, layer)
        })
        .collect();
    let voice = shot.voice.as_ref().map(|path| {
        let i = next;
        next += 1;
        args.extend(["-i".into(), path.as_os_str().to_owned()]);
        i
    });
    let music = shot.music.as_ref().map(|path| {
        let i = next;
        args.extend([
            "-stream_loop".into(),
            "-1".into(),
            "-i".into(),
            path.as_os_str().to_owned(),
        ]);
        i
    });
    let frames = (shot.duration * 24.0).ceil() as u64;
    let motion = &shot.direction.motion;
    let base = match shot.direction.camera.as_str() {
        "Trung cảnh" => 1.15,
        "Cận mặt" => 1.35,
        _ => 1.0,
    };
    let zoom = if motion == "Đẩy vào" {
        format!("if(eq(on,0),{base},min(zoom+0.0005,{}))", base + 0.15)
    } else if motion == "Lùi ra" {
        format!("if(eq(on,0),{},max({base},zoom-0.0005))", base + 0.15)
    } else {
        base.to_string()
    };
    let x = if motion == "Máy lia ngang" {
        format!("(iw-iw/zoom)*on/{frames}")
    } else {
        "iw/2-iw/zoom/2".into()
    };
    let zoom = if motion == "Máy lia ngang" {
        (base + 0.12).to_string()
    } else {
        zoom
    };
    let mut graph = format!(
        "[0:v]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},setsar=1,zoompan=z='{zoom}':x='{x}':y='ih/2-ih/zoom/2':d=1:s={w}x{h}:fps=24,trim=duration={},setpts=PTS-STARTPTS[base];",
        shot.duration
    );
    let mut label = "base".to_string();
    if let Some(i) = foreground {
        let fw = (w as f64 * 0.7) as u32 / 2 * 2;
        graph.push_str(&format!("[{i}:v]scale={fw}:{h}:force_original_aspect_ratio=decrease,setsar=1,format=rgba[person];[base][person]overlay=(W-w)/2:H-h:shortest=1[layered];"));
        label = "layered".into();
    }
    for (i, layer) in layers {
        let lw = ((w as f64 * layer.width / 100.0) as u32 / 2 * 2).max(2);
        graph.push_str(&format!("[{i}:v]scale={lw}:-2,format=rgba,colorchannelmixer=aa={}[layer{i}];[{label}][layer{i}]overlay=x=W*{}/100:y=H*{}/100:shortest=1[composite{i}];", layer.opacity,layer.x,layer.y));
        label = format!("composite{i}");
    }
    let caption = if shot.caption.trim().is_empty() {
        shot.narration.trim()
    } else {
        shot.caption.trim()
    };
    if !caption.is_empty() {
        fs::copy(font, root.join("font.ttf")).map_err(|_| "Không tìm thấy phông chữ phụ đề")?;
        let text_name = format!("{name}.txt");
        let mut lines = String::new();
        let mut width = 0;
        for word in caption.split_whitespace() {
            if width + word.chars().count() > 42 {
                lines.push('\n');
                width = 0;
            } else if width > 0 {
                lines.push(' ');
                width += 1;
            }
            lines.push_str(word);
            width += word.chars().count();
        }
        fs::write(root.join(&text_name), lines).map_err(|_| "Không lưu được chữ phụ đề")?;
        graph.push_str(&format!("[{label}]drawtext=fontfile=font.ttf:textfile={text_name}:expansion=none:fontsize={}:fontcolor=white:borderw=2:bordercolor=black:x=(w-tw)/2:y=h-th-{}[text];", w / 28, h / 18));
        label = "text".into();
    }
    graph.push_str(&format!("[{label}]format=yuv420p[v];"));
    match (voice, music) {
        (Some(v), Some(m)) => graph.push_str(&format!("[{v}:a]aresample=48000,apad[voice];[{m}:a]aresample=48000,volume=0.18[music];[voice][music]amix=inputs=2:duration=first:normalize=0[a]")),
        (Some(v), None) => graph.push_str(&format!("[{v}:a]aresample=48000,apad[a]")),
        (None, Some(m)) => graph.push_str(&format!("[{m}:a]aresample=48000,volume=0.18,apad[a]")),
        (None, None) => graph.push_str("anullsrc=r=48000:cl=stereo[a]"),
    }
    args.extend([
        "-filter_complex".into(),
        graph.into(),
        "-map".into(),
        "[v]".into(),
        "-map".into(),
        "[a]".into(),
        "-t".into(),
        shot.duration.to_string().into(),
        "-r".into(),
        "24".into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "20".into(),
        "-threads".into(),
        "2".into(),
        "-filter_complex_threads".into(),
        "1".into(),
        "-c:a".into(),
        "aac".into(),
        "-ar".into(),
        "48000".into(),
        "-ac".into(),
        "2".into(),
        name.into(),
    ]);
    execute(ffmpeg, root, args, cancel)
}
fn execute(
    program: &Path,
    root: &Path,
    args: Vec<OsString>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let log =
        fs::File::create(root.join("render.log")).map_err(|_| "Không mở được nhật ký render")?;
    let mut command = Command::new(program);
    command
        .current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Không khởi động được FFmpeg; kiểm tra bộ cài")?;
    #[cfg(windows)]
    let _owned_job = match crate::local_ai::owned_job(&child) {
        Ok(job) => job,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Không quản lý được tiến trình render".into());
        }
    };
    loop {
        if cancel.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Đã dừng render".into());
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                return Err(format!(
                    "Dựng cảnh thất bại. Nhật ký: {}",
                    root.join("render.log").display()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Không đọc được tiến trình render".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_layered_film_has_expected_streams_duration_and_subtitles() {
        let Some(program) = std::env::var_os("CONCAT_TEST_FFMPEG").map(PathBuf::from) else {
            return;
        };
        let root = std::env::temp_dir().join(format!(
            "concat-storyboard-test-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let cancel = AtomicBool::new(false);
        let make = |name: &str, color: &str| {
            let args = [
                "-hide_banner",
                "-nostdin",
                "-n",
                "-f",
                "lavfi",
                "-i",
                color,
                "-frames:v",
                "1",
                "-threads",
                "1",
                name,
            ]
            .iter()
            .map(OsString::from)
            .collect();
            execute(&program, &root, args, &cancel).unwrap();
        };
        make("background.png", "color=c=navy:s=1280x720");
        make("person.png", "color=c=orange:s=160x240");
        let font = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../concat-text/fonts/HankenGrotesk-Regular.ttf");
        let mut a = Shot::new("", 1.0);
        a.background = Some(root.join("background.png"));
        a.character = Some(root.join("person.png"));
        a.layers.push(crate::storyboard::VisualLayer {
            path: root.join("person.png"),
            x: 10.0,
            y: 15.0,
            width: 20.0,
            opacity: 0.5,
        });
        a.caption = "Xin chào · cảnh một".into();
        a.direction.motion = "Đẩy vào".into();
        let mut b = a.clone();
        b.id = Shot::new("", 1.0).id;
        b.duration = 1.5;
        b.caption = "Cảnh hai · lời kết".into();
        b.direction.motion = "Máy lia ngang".into();
        let board = Storyboard {
            title: "Layer test".into(),
            shots: vec![a, b],
            ..Storyboard::default()
        };
        let render_root = root.join("render");
        let output = render_board(
            &program,
            &font,
            &board,
            &render_root,
            RenderSize {
                width: 1280,
                height: 720,
            },
            &cancel,
            |_, _| {},
        )
        .unwrap();
        let probe = program.with_file_name("ffprobe.exe");
        let result = Command::new(probe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration:stream=codec_type,width,height",
                "-of",
                "json",
            ])
            .arg(&output)
            .output()
            .unwrap();
        assert!(result.status.success());
        let info: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let duration: f64 = info["format"]["duration"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((duration - 2.5).abs() < 0.15, "duration={duration}");
        assert!(
            info["streams"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["codec_type"] == "video" && s["width"] == 1280 && s["height"] == 720)
        );
        assert!(
            info["streams"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["codec_type"] == "audio")
        );
        assert!(
            fs::read_to_string(render_root.join("captions.srt"))
                .unwrap()
                .contains("00:00:01,000 --> 00:00:02,500")
        );
        assert!(
            !render_root.join("shot-0000.mp4").exists(),
            "temporary encoded parts must be removed after assembly"
        );
        // This exact root was freshly created by this test and owns all descendants.
        fs::remove_dir_all(root).unwrap();
    }
}
