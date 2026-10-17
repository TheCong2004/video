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

def detect_face_and_mouth(frame):
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
                forehead = lm.landmark[10]
                left_cheek = lm.landmark[234]
                right_cheek = lm.landmark[454]
                
                fx1 = int(left_cheek.x * w)
                fx2 = int(right_cheek.x * w)
                fy1 = int(forehead.y * h)
                fy2 = int(chin.y * h)
                
                fw = fx2 - fx1
                fh = fy2 - fy1
                
                cx = (fx1 + fx2) // 2
                cy = (fy1 + fy2) // 2
                size = int(max(fw, fh) * 1.15)
                
                crop_x1 = max(0, cx - size // 2)
                crop_x2 = min(w, cx + size // 2)
                crop_y1 = max(0, cy - size // 2)
                crop_y2 = min(h, cy + size // 2)
                
                crop_w = crop_x2 - crop_x1
                crop_h = crop_y2 - crop_y1
                
                rel_mx = mouth_cx - crop_x1
                rel_my = mouth_cy - crop_y1
                
                mask = np.zeros((crop_h, crop_w), dtype=np.float32)
                rx = int(mouth_w * 0.62)
                ry = int(max(mouth_h * 1.05, mouth_w * 0.28))
                cv2.ellipse(mask, (rel_mx, rel_my), (rx, ry), 0, 0, 360, 1.0, -1)
                mask = cv2.GaussianBlur(mask, (9, 9), 2.0)
                mask = np.expand_dims(mask, axis=2)
                
                return (crop_y1, crop_y2, crop_x1, crop_x2), mask
    except Exception as e:
        print(f"MediaPipe detection warning: {e}", file=sys.stderr)

    # OpenCV Haar Cascade fallback
    cascade_path = cv2.data.haarcascades + "haarcascade_frontalface_default.xml"
    face_cascade = cv2.CascadeClassifier(cascade_path)
    gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
    faces = face_cascade.detectMultiScale(gray, 1.1, 4)
    if len(faces) > 0:
        x, y, fw, fh = faces[0]
        crop_y1 = max(0, y)
        crop_y2 = min(h, y + fh)
        crop_x1 = max(0, x)
        crop_x2 = min(w, x + fw)
    else:
        crop_y1, crop_y2, crop_x1, crop_x2 = int(h * 0.2), int(h * 0.55), int(w * 0.3), int(w * 0.7)
        
    crop_h = crop_y2 - crop_y1
    crop_w = crop_x2 - crop_x1
    mask = np.zeros((crop_h, crop_w), dtype=np.float32)
    cv2.ellipse(mask, (crop_w // 2, int(crop_h * 0.73)), (int(crop_w * 0.25), int(crop_h * 0.12)), 0, 0, 360, 1.0, -1)
    mask = cv2.GaussianBlur(mask, (11, 11), 2.5)
    mask = np.expand_dims(mask, axis=2)
    return (crop_y1, crop_y2, crop_x1, crop_x2), mask

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
        source_video = avatar_path
    else:
        # Check if there is a matching mp4 with the same stem
        candidate_same_stem = avatar_path.with_suffix(".mp4")
        if candidate_same_stem.exists():
            source_video = candidate_same_stem
        else:
            # Check avatar assets folder for matching name
            assets_dir = Path(r"d:\capcutpolot\concat\src\crates\concat\assets\avatars")
            avatar_stem = avatar_path.stem.replace("concat-avatar-", "")
            for part in avatar_stem.split("-"):
                potential_name = f"mc-{part}.mp4"
                if (assets_dir / potential_name).exists():
                    source_video = assets_dir / potential_name
                    break
            if source_video is None:
                for candidate_file in assets_dir.glob("*.mp4"):
                    if candidate_file.stem in avatar_stem:
                        source_video = candidate_file
                        break

    if source_video is not None:
        cap = cv2.VideoCapture(str(source_video))
        raw_frames = []
        while True:
            ret, frame = cap.read()
            if not ret:
                break
            raw_frames.append(frame)
        cap.release()
        if len(raw_frames) > 0:
            video_frames = [raw_frames[i % len(raw_frames)] for i in range(num_frames)]
        else:
            source_video = None

    if source_video is None:
        img = cv2.imread(str(avatar_path))
        if img is None:
            raise FileNotFoundError(f"Could not read image: {avatar_path}")
        video_frames = [img.copy() for _ in range(num_frames)]

    ref_frame = video_frames[0]
    (y1, y2, x1, x2), mask = detect_face_and_mouth(ref_frame)
    crop_h, crop_w = y2 - y1, x2 - x1

    crops = [f[y1:y2, x1:x2] for f in video_frames]
    face_96 = [cv2.resize(c, (input_size, input_size)) for c in crops]

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

    output_frames = []
    for i in range(num_frames):
        orig_frame = video_frames[i].copy()
        orig_crop = orig_frame[y1:y2, x1:x2].astype(np.float32)
        pred_crop = cv2.resize(preds_np[i], (crop_w, crop_h), interpolation=cv2.INTER_CUBIC).astype(np.float32)
        blended_crop = pred_crop * mask + orig_crop * (1.0 - mask)
        orig_frame[y1:y2, x1:x2] = np.clip(blended_crop, 0, 255).astype(np.uint8)
        output_frames.append(orig_frame)

    proc = subprocess.Popen([
        ffmpeg_bin, "-y",
        "-f", "image2pipe",
        "-vcodec", "mjpeg",
        "-r", str(fps),
        "-i", "pipe:0",
        "-i", str(audio_path),
        "-c:v", "libx264",
        "-preset", "veryfast",
        "-pix_fmt", "yuv420p",
        "-c:a", "aac",
        "-shortest",
        str(output_mp4_path)
    ], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    for f in output_frames:
        ret, jpeg = cv2.imencode(".jpg", f, [cv2.IMWRITE_JPEG_QUALITY, 95])
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
