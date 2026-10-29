"""Concat-owned CLI adapter for the bundled SLive 1.0.18 face preparation.

No old desktop app, HTTP server, auth, billing or ArtCraft imports are started.
Only operator-configured local resources are used; never download weights here.
"""
from __future__ import annotations

import argparse
import importlib.util
import importlib
import hashlib
import json
import math
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import types

UPSTREAM = Path(__file__).resolve().parents[1] / "upstream"
ROOT = Path(__file__).resolve().parents[3]


def detector_path() -> Path:
    return Path(os.environ.get("CONCAT_SLIVE_S3FD", str(ROOT / ".runtime/slive-models/s3fd.pth")))


def readiness() -> dict:
    missing = []
    for name in ("torch", "cv2", "numpy", "scipy", "tqdm"):
        try:
            if importlib.util.find_spec(name) is None:
                raise ImportError(name)
            importlib.import_module(name)
        except Exception:
            missing.append(name)
    for name in ("ffmpeg", "ffprobe"):
        if not shutil.which(os.environ.get(f"CONCAT_SLIVE_{name.upper()}", name)):
            missing.append(name)
    detector = detector_path()
    if not detector.is_absolute() or not detector.is_file():
        missing.append("s3fd_model")
    elif hashlib.sha256(detector.read_bytes()).hexdigest() != "619a31681264d3f7f7fc7a16a42cbbe8b23f31a256f75a366e5a1bcd59b33543":
        missing.append("s3fd_model_integrity")
    if not (UPSTREAM / "avatars/wav2lip/genavatar.py").is_file():
        missing.append("slive_source")
    return {"ready": not missing, "missing": missing, "engine": "slive-1.0.18",
            "capabilities": {"face_preparation": not missing, "source_motion": not missing,
                             "realtime_lipsync": False, "voice_clone": False}}


def validate_probe(probe: dict) -> dict:
    streams = [s for s in probe.get("streams", []) if s.get("codec_type") == "video"]
    if not streams:
        raise ValueError("VIDEO_STREAM_REQUIRED")
    stream = streams[0]
    duration = float(probe.get("format", {}).get("duration", stream.get("duration", 0)))
    width, height = int(stream.get("width", 0)), int(stream.get("height", 0))
    if not math.isfinite(duration) or not 1 <= duration <= 30:
        raise ValueError("VIDEO_DURATION_1_TO_30_SECONDS")
    if not 16 <= width <= 4096 or not 16 <= height <= 4096:
        raise ValueError("VIDEO_DIMENSIONS_INVALID")
    return {"duration_seconds": duration, "width": width, "height": height}


def prepare(source: Path, output: Path) -> dict:
    if not readiness()["ready"]:
        raise RuntimeError("AVATAR_RUNTIME_NOT_READY")
    if not source.is_file() or not 1 <= source.stat().st_size <= 25 * 1024 * 1024:
        raise ValueError("VIDEO_SIZE_INVALID")
    ffprobe = os.environ.get("CONCAT_SLIVE_FFPROBE", "ffprobe")
    result = subprocess.run([ffprobe, "-v", "error", "-show_format", "-show_streams", "-of", "json", str(source)],
                            capture_output=True, timeout=20, check=True)
    info = validate_probe(json.loads(result.stdout))
    output.mkdir(parents=True, exist_ok=False)
    normalized = output / "source.mp4"
    ffmpeg = os.environ.get("CONCAT_SLIVE_FFMPEG", "ffmpeg")
    subprocess.run([ffmpeg, "-nostdin", "-v", "error", "-threads", "2", "-i", str(source),
                    "-map", "0:v:0", "-an", "-sn", "-dn", "-vf",
                    "scale=384:684:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=25",
                    "-frames:v", "750", "-c:v", "libx264", "-threads", "2", "-pix_fmt", "yuv420p", str(normalized)],
                   capture_output=True, timeout=120, check=True)
    # Replace only the path module inside this one-use subprocess. Imported
    # upstream remains byte-identical; it cannot write into the old SLive app.
    paths = types.ModuleType("runtime_paths")
    paths.avatar_path = lambda avatar_id, for_write=False: output
    sys.modules["runtime_paths"] = paths
    sys.path.insert(0, str(UPSTREAM))
    os.environ["SLIVE_S3FD_PATH"] = str(detector_path().resolve())
    import torch.utils.model_zoo
    torch.set_num_threads(min(4, os.cpu_count() or 1))
    def no_download(*args, **kwargs):
        raise RuntimeError("LOCAL_MODEL_REQUIRED")
    torch.utils.model_zoo.load_url = no_download
    sys.argv = ["genavatar", "--avatar_id", "concat", "--video_path", str(normalized),
                "--img_size", "384", "--face_det_batch_size", "2"]
    runpy.run_module("avatars.wav2lip.genavatar", run_name="__main__")
    frames = sorted((output / "full_imgs").glob("*.png"))
    faces = sorted((output / "face_imgs").glob("*.png"))
    if not frames or len(frames) != len(faces) or not (output / "coords.pkl").is_file():
        raise RuntimeError("FACE_PREPARATION_INCOMPLETE")
    shutil.copyfile(frames[0], output / "preview.png")
    info.update({"frames": len(frames), "fps": 25, "engine": "slive-1.0.18"})
    (output / "result.json").write_text(json.dumps(info), encoding="utf-8")
    return info


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--source", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.check:
        print(json.dumps(readiness()))
        return 0
    if not args.source or not args.output or not args.source.is_absolute() or not args.output.is_absolute():
        parser.error("absolute source/output required")
    try:
        prepare(args.source, args.output)
        return 0
    except Exception:
        # Never print personal filenames, subprocess output or model internals.
        print("AVATAR_PREPARATION_FAILED", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
