import sys
import os
import argparse
import subprocess
import tempfile
import cv2
import torch
import numpy as np
from pathlib import Path

engine_root = Path("d:/capcutpolot/artcraft/engines/opentalking")
sys.path.insert(0, str(engine_root))
os.environ["OPENTALKING_MODELS_DIR"] = str(engine_root / "models")
os.environ["OPENTALKING_WAV2LIP_DEVICE"] = "cpu"
os.environ["OPENTALKING_WAV2LIP_CHECKPOINT"] = str(engine_root / "models" / "wav2lip" / "wav2lip.pth")

from opentalking.models.wav2lip.loader import load_wav2lip_torch
from opentalking.models.wav2lip.feature_extractor import pcm_to_wav2lip_mel, MEL_STEP_SIZE
from opentalking.models.wav2lip.postprocess import (
    MouthGeometry,
    BlendConfig,
    blend_mouth_patch,
    match_patch_color,
)

def detect_face_and_mouth(frame):
    """
    Detect face bounding box and mouth geometry matching standard Wav2Lip coordinates:
    - 1:1 square crop from eyebrow level down to chin.
    - Midpoint at philtrum so bottom 50% covers mouth, lips, teeth, and jaw.
    - Exact mouth landmarks for smooth anatomical blending.
    """
    h, w = frame.shape[:2]
    try:
        import mediapipe as mp
        mp_face_mesh = mp.solutions.face_mesh
        with mp_face_mesh.FaceMesh(static_image_mode=True, max_num_faces=1, refine_landmarks=True) as face_mesh:
            res = face_mesh.process(cv2.cvtColor(frame, cv2.COLOR_BGR2RGB))
            if res.multi_face_landmarks:
                lm = res.multi_face_landmarks[0]
                top_lip = lm.landmark[0]
                bot_lip = lm.landmark[17]
                left_corner = lm.landmark[61]
                right_corner = lm.landmark[291]
                
                mouth_cx = int((left_corner.x + right_corner.x) / 2.0 * w)
                mouth_cy = int((top_lip.y + bot_lip.y) / 2.0 * h)
                mouth_w = int(abs(right_corner.x - left_corner.x) * w)
                mouth_h = int(abs(bot_lip.y - top_lip.y) * h)
                
                chin = lm.landmark[152]
                eyebrows_y = (lm.landmark[66].y + lm.landmark[296].y) / 2.0
                nose_tip = lm.landmark[1]
                face_cx = int(nose_tip.x * w)
                
                # Height from eyebrows to chin
                face_span_h = int((chin.y - eyebrows_y) * h)
                
                # Standard Wav2Lip 1:1 crop
                crop_size = int(face_span_h * 1.15)
                # Ensure even size
                crop_size = crop_size + (crop_size % 2)
                
                y1 = max(0, int(eyebrows_y * h - face_span_h * 0.12))
                y2 = min(h, y1 + crop_size)
                if y2 == h:
                    y1 = max(0, h - crop_size)
                    
                actual_h = y2 - y1
                x1 = max(0, face_cx - actual_h // 2)
                x2 = min(w, x1 + actual_h)
                if x2 == w:
                    x1 = max(0, w - actual_h)
                actual_w = x2 - x1
                
                # Ensure square
                sq_size = min(actual_h, actual_w)
                y2 = y1 + sq_size
                x2 = x1 + sq_size
                
                rel_mx = mouth_cx - x1
                rel_my = mouth_cy - y1
                
                geom = MouthGeometry.ellipse(
                    center=(rel_mx, rel_my),
                    rx=max(10, int(mouth_w * 0.52)),
                    ry=max(8, int(max(mouth_h * 0.70, mouth_w * 0.28)))
                )
                
                return (y1, y2, x1, x2), geom
    except Exception as e:
        print(f"MediaPipe detection warning: {e}", file=sys.stderr)

    # OpenCV Haar Cascade fallback
    cascade_path = cv2.data.haarcascades + "haarcascade_frontalface_default.xml"
    face_cascade = cv2.CascadeClassifier(cascade_path)
    gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
    faces = face_cascade.detectMultiScale(gray, 1.1, 4)
    if len(faces) > 0:
        x, y, fw, fh = faces[0]
        # Pad slightly towards chin
        sq = max(fw, fh)
        y1 = max(0, y)
        y2 = min(h, y1 + sq)
        x1 = max(0, x)
        x2 = min(w, x1 + sq)
    else:
        y1, y2, x1, x2 = int(h * 0.18), int(h * 0.50), int(w * 0.32), int(w * 0.68)
        
    crop_h = y2 - y1
    crop_w = x2 - x1
    geom = MouthGeometry.ellipse(
        center=(crop_w // 2, int(crop_h * 0.72)),
        rx=int(crop_w * 0.25),
        ry=int(crop_h * 0.12)
    )
    return (y1, y2, x1, x2), geom

def sharpen_face_patch(patch_bgr: np.ndarray) -> np.ndarray:
    """
    Two-stage high-frequency unsharp mask sharpening:
    Stage 1: Fine edge contrast (sharp teeth and lip lines).
    Stage 2: Mid-frequency contour enhancement (prevents softness on upscaling).
    """
    # Fine unsharp mask
    blur_fine = cv2.GaussianBlur(patch_bgr, (0, 0), sigmaX=1.0)
    sharp1 = cv2.addWeighted(patch_bgr, 1.45, blur_fine, -0.45, 0)
    
    # Mid-frequency boost
    blur_mid = cv2.GaussianBlur(sharp1, (0, 0), sigmaX=2.2)
    sharp2 = cv2.addWeighted(sharp1, 1.20, blur_mid, -0.20, 0)
    
    return np.clip(sharp2, 0, 255).astype(np.uint8)

def render_lipsync(avatar_path: Path, audio_path: Path, output_mp4_path: Path):
    if not avatar_path.exists():
        raise FileNotFoundError(f"Avatar not found: {avatar_path}")
    if not audio_path.exists():
        raise FileNotFoundError(f"Audio file not found: {audio_path}")

    script_dir = Path(__file__).resolve().parent
    ffmpeg_candidates = [
        Path(r"d:\capcutpolot\ConcatStudio-Portable\ffmpeg.exe"),
        script_dir / "ffmpeg.exe",
        script_dir.parent / "ffmpeg.exe",
    ]
    ffmpeg_bin = "ffmpeg"
    for cand in ffmpeg_candidates:
        if cand.exists():
            ffmpeg_bin = str(cand)
            break

    # Convert audio to PCM 16kHz mono 16-bit
    pcm_path = Path(tempfile.gettempdir()) / f"temp_16k_{os.getpid()}.pcm"
    try:
        subprocess.run([
            ffmpeg_bin, "-y", "-i", str(audio_path),
            "-ar", "16000", "-ac", "1", "-f", "s16le", str(pcm_path)
        ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        with open(pcm_path, "rb") as f:
            pcm_bytes = f.read()
    finally:
        if pcm_path.exists():
            try:
                os.remove(pcm_path)
            except OSError:
                pass

    if len(pcm_bytes) < 3200:
        raise ValueError("Audio PCM is too short or empty")

    pcm = np.frombuffer(pcm_bytes, dtype=np.int16)
    mel = pcm_to_wav2lip_mel(pcm, 16000)

    fps = 25
    mel_idx_multiplier = 80.0 / fps
    mel_chunks = []
    frame_idx = 0
    while True:
        start_idx = int(frame_idx * mel_idx_multiplier)
        if start_idx + MEL_STEP_SIZE > mel.shape[1]:
            break
        mel_chunks.append(mel[:, start_idx : start_idx + MEL_STEP_SIZE])
        frame_idx += 1

    num_frames = len(mel_chunks)
    if num_frames == 0:
        raise ValueError("No audio frames to render")

    # Load Wav2Lip model
    bundle = load_wav2lip_torch(engine_root / "models" / "wav2lip" / "wav2lip.pth", "cpu")
    model = bundle["model"]
    input_size = int(bundle["input_size"])

    # Locate matching source video if available for this specific avatar
    source_video = None
    if avatar_path.suffix.lower() == ".mp4" and avatar_path.exists():
        # Check if avatar_path has multiple frames or is just 1 frame looped
        source_video = avatar_path
    else:
        candidate_same_stem = avatar_path.with_suffix(".mp4")
        if candidate_same_stem.exists():
            source_video = candidate_same_stem

    if source_video is None:
        # Search assets for matching avatar
        assets_dir = Path(r"d:\capcutpolot\concat\src\crates\concat\assets\avatars")
        avatar_stem = avatar_path.stem.lower()
        if "mia" in avatar_stem or "wanx" in avatar_stem:
            demo_video = assets_dir / "demo-video.mp4"
            if demo_video.exists():
                source_video = demo_video
        elif "alex" in avatar_stem and (assets_dir / "mc-alex.mp4").exists():
            source_video = assets_dir / "mc-alex.mp4"
        elif "linh" in avatar_stem and (assets_dir / "mc-linh.mp4").exists():
            source_video = assets_dir / "mc-linh.mp4"
        elif ("newscaster" in avatar_stem or "david" in avatar_stem) and (assets_dir / "newscaster.mp4").exists():
            source_video = assets_dir / "newscaster.mp4"

    video_frames = []
    if source_video is not None:
        cap = cv2.VideoCapture(str(source_video))
        while True:
            ret, frame = cap.read()
            if not ret:
                break
            video_frames.append(frame)
        cap.release()

    if len(video_frames) > 0:
        video_frames = [video_frames[i % len(video_frames)] for i in range(num_frames)]
    else:
        img = cv2.imread(str(avatar_path))
        if img is None:
            raise FileNotFoundError(f"Could not read image: {avatar_path}")
        video_frames = [img.copy() for _ in range(num_frames)]

    ref_frame = video_frames[0]
    (y1, y2, x1, x2), geom = detect_face_and_mouth(ref_frame)
    crop_h, crop_w = y2 - y1, x2 - x1

    # Extract face crops and resize to Wav2Lip input size (96x96)
    crops = [f[y1:y2, x1:x2] for f in video_frames]
    face_96 = [cv2.resize(c, (input_size, input_size), interpolation=cv2.INTER_AREA) for c in crops]

    face_crops_np = np.stack(face_96, axis=0)
    mel_batch_np = np.stack(mel_chunks, axis=0)

    face_tensor = torch.from_numpy(np.ascontiguousarray(np.transpose(face_crops_np, (0, 3, 1, 2)))).float().div_(255.0)
    masked_tensor = face_tensor.clone()
    masked_tensor[:, :, input_size // 2 :, :] = 0.0

    img_tensor = torch.cat([masked_tensor, face_tensor], dim=1)
    mel_tensor = torch.from_numpy(np.ascontiguousarray(mel_batch_np)).unsqueeze(1).float()

    with torch.no_grad():
        preds = model(mel_tensor, img_tensor)
        preds_np = preds.detach().cpu().numpy().transpose(0, 2, 3, 1) * 255.0

    # Configure seamless natural blending
    blend_cfg = BlendConfig(
        mouth_dilation=1.08,
        feather=0.75,
        lower_lip_expand=0.35,
        lower_lip_dynamic_expand=0.20,
        color_match_strength=0.92,
        enable_jaw_motion_blend=True,
        jaw_blend_alpha=0.22,
    )

    output_frames = []
    for i in range(num_frames):
        orig_frame = video_frames[i].copy()
        orig_crop = orig_frame[y1:y2, x1:x2]
        
        # High quality Lanczos-4 upscaling to original crop resolution
        pred_raw = np.clip(preds_np[i], 0.0, 255.0).astype(np.uint8)
        pred_upscaled = cv2.resize(pred_raw, (crop_w, crop_h), interpolation=cv2.INTER_LANCZOS4)
        
        # Apply high-frequency edge and detail sharpening (crisp lips and teeth)
        pred_sharpened = sharpen_face_patch(pred_upscaled)
        
        # Blend mouth patch seamlessly with exact anatomical mask & color match
        blended_crop = blend_mouth_patch(
            pred=pred_sharpened,
            original=orig_crop,
            geometry=geom,
            config=blend_cfg,
        )
        
        orig_frame[y1:y2, x1:x2] = blended_crop
        output_frames.append(orig_frame)

    # Encode with FFmpeg (CRF 18 for high visual fidelity and sharpness)
    proc = subprocess.Popen([
        ffmpeg_bin, "-y",
        "-f", "image2pipe",
        "-vcodec", "mjpeg",
        "-r", str(fps),
        "-i", "pipe:0",
        "-i", str(audio_path),
        "-c:v", "libx264",
        "-preset", "faster",
        "-crf", "18",
        "-pix_fmt", "yuv420p",
        "-c:a", "aac",
        "-b:a", "192k",
        "-shortest",
        str(output_mp4_path)
    ], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    for f in output_frames:
        ret, jpeg = cv2.imencode(".jpg", f, [cv2.IMWRITE_JPEG_QUALITY, 98])
        proc.stdin.write(jpeg.tobytes())
    proc.stdin.close()
    proc.wait()

    if proc.returncode != 0 or not output_mp4_path.exists():
        raise RuntimeError(f"FFmpeg encoding failed with return code {proc.returncode}")

def main():
    parser = argparse.ArgumentParser(description="Render animated lipsync avatar video")
    parser.add_argument("--avatar", required=True, help="Path to avatar image or video")
    parser.add_argument("--audio", required=True, help="Path to speech audio file (.mp3/.wav)")
    parser.add_argument("--output", required=True, help="Path to destination .mp4 file")
    args = parser.parse_args()

    render_lipsync(Path(args.avatar), Path(args.audio), Path(args.output))

if __name__ == "__main__":
    main()
