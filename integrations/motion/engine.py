"""Offline EchoMimicV3-Flash adapter; uses an operator-installed upstream checkout.

No pip/git/download at runtime. One CLI job at a time, before LIVE starts.
"""
from pathlib import Path
import json
import math
import os
import subprocess
import sys

PROMPTS = (
    "A single presenter speaks naturally, small relaxed explanatory hand gestures, steady camera, same identity and clothes.",
    "A single friendly presenter greets the audience with a small gentle wave, steady camera, same identity and clothes.",
    "A single presenter explains a product with gentle open-palm gestures, empty hands, no invented objects, steady camera, same identity and clothes.",
)


def frame_budget(seconds):
    if type(seconds) is not int or not 2 <= seconds <= 10:
        raise ValueError("DURATION")
    # Wan temporal compression requires 4n+1 frames. Render enough, then trim
    # to the exact requested duration, never silently shorten to 81 frames.
    return math.ceil((seconds * 25 - 1) / 4) * 4 + 1


class EchoEngine:
    def __init__(self, root, models):
        self.root, self.models = Path(root).resolve(), Path(models).resolve()
        for path in (self.root / "infer_flash.py", self.root / "config/config.yaml",
                     self.models / "Wan2.1-Fun-V1.1-1.3B-InP",
                     self.models / "chinese-wav2vec2-base",
                     self.models / "transformer/diffusion_pytorch_model.safetensors"):
            if not path.exists():
                raise RuntimeError("MODEL_NOT_INSTALLED")
        import torch
        if not torch.cuda.is_available():
            raise RuntimeError("CUDA_UNAVAILABLE")
        # Upstream imports must resolve too; --help exits before loading weights.
        self.run([sys.executable, "infer_flash.py", "--help"], cwd=self.root, timeout=60)

    def run(self, args, cwd=None, timeout=90):
        env = dict(os.environ, HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1")
        env.pop("CONCAT_LIPSYNC_TOKEN", None)
        return subprocess.run(args, cwd=cwd, env=env, check=True, timeout=timeout,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                              stdin=subprocess.DEVNULL)

    def render(self, image, audio, role, seconds, work):
        from PIL import Image
        with Image.open(image) as photo:
            if photo.width > 4096 or photo.height > 4096 or photo.width < 128 or photo.height < 128:
                raise ValueError("IMAGE_LIMIT")
            photo.convert("RGB").save(work / "reference.png")
        frames = frame_budget(seconds)
        speech = work / "drive.wav"
        demuxer = {".wav": "wav", ".mp3": "mp3", ".flac": "flac", ".m4a": "mov", ".aac": "aac", ".ogg": "ogg"}.get(audio.suffix)
        if demuxer is None:
            raise ValueError("AUDIO_TYPE")
        # Do not auto-detect uploaded playlists or follow their local/remote refs.
        self.run(["ffmpeg", "-nostdin", "-v", "error", "-protocol_whitelist", "file,pipe", "-f", demuxer, "-i", str(audio), "-vn",
                  "-af", "apad", "-t", str(frames / 25 + 0.08), "-ar", "16000", "-ac", "1", str(speech)])
        out = work / "generated"
        self.run([sys.executable, "infer_flash.py", "--image_path", str(work / "reference.png"),
                  "--audio_path", str(speech), "--prompt", PROMPTS[role],
                  "--config_path", "config/config.yaml", "--model_name", str(self.models / "Wan2.1-Fun-V1.1-1.3B-InP"),
                  "--transformer_path", str(self.models / "transformer/diffusion_pytorch_model.safetensors"),
                  "--wav2vec_model_dir", str(self.models / "chinese-wav2vec2-base"),
                  "--save_path", str(out), "--video_length", str(frames), "--fps", "25",
                  "--num_inference_steps", "8", "--sample_size", "512", "512"], cwd=self.root, timeout=1500)
        target = work / "motion.mp4"
        # Always silent; LIVE will use a new TTS track + MuseTalk, never source sound.
        self.run(["ffmpeg", "-nostdin", "-v", "error", "-protocol_whitelist", "file,pipe", "-i", str(out / "reference_output.mp4"),
                  "-an", "-t", str(seconds), "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p",
                  "-movflags", "+faststart", str(target)])
        info = subprocess.run(["ffprobe", "-v", "error", "-protocol_whitelist", "file,pipe", "-show_format", "-show_streams", "-of", "json", str(target)],
                              capture_output=True, check=True, timeout=30)
        probe = json.loads(info.stdout)
        if abs(float(probe["format"]["duration"]) - seconds) > 0.08 or any(s["codec_type"] == "audio" for s in probe["streams"]):
            raise RuntimeError("DURATION_OR_AUDIO_INVALID")
        if target.stat().st_size > 25 * 1024 * 1024:
            raise RuntimeError("RESULT_LIMIT")
        return target.read_bytes()
