"""Resident MuseTalk 1.5 wrapper using official utilities, not a fake idle loop.

GPU validation is required before deployment. Only the face patch is generated;
the body/background stay from the source. No face enhancer is silently enabled.
"""
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
from types import SimpleNamespace
from uuid import uuid4


def command(argv, **kwargs):
    return subprocess.run(argv, check=True, stdin=subprocess.DEVNULL,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60, **kwargs)


def probe(path):
    raw = command(["ffprobe", "-v", "error", "-protocol_whitelist", "file,pipe",
                   "-format_whitelist", "mov,wav,mp3,ogg,flac,aac", "-show_streams", "-show_format", "-of", "json", str(path)])
    data = json.loads(raw.stdout)
    duration = float(data["format"]["duration"])
    if not math.isfinite(duration) or duration <= 0:
        raise ValueError("INVALID_DURATION")
    return data, duration


class MuseTalkEngine:
    def __init__(self, root):
        self.root = Path(root).resolve(strict=True)
        for required in ["models/musetalkV15/unet.pth", "models/musetalkV15/musetalk.json",
                         "models/whisper/pytorch_model.bin", "models/sd-vae/diffusion_pytorch_model.bin",
                         "models/dwpose/dw-ll_ucoco_384.pth", "models/face-parse-bisent/79999_iter.pth"]:
            if not (self.root / required).is_file():
                raise RuntimeError("MODEL_FILES_MISSING")
        os.chdir(self.root)
        sys.path.insert(0, str(self.root))
        import torch
        from scripts import realtime_inference as model
        if not torch.cuda.is_available():
            raise RuntimeError("CUDA_REQUIRED")
        self.torch, self.model = torch, model
        model.args = SimpleNamespace(version="v15", extra_margin=10, parsing_mode="jaw",
                                     audio_padding_length_left=2, audio_padding_length_right=2)
        model.device = torch.device("cuda:0")
        model.vae, model.unet, model.pe = model.load_all_model(
            unet_model_path="models/musetalkV15/unet.pth", vae_type="sd-vae",
            unet_config="models/musetalkV15/musetalk.json", device=model.device)
        model.timesteps = torch.tensor([0], device=model.device)
        model.pe = model.pe.half().to(model.device)
        model.vae.vae = model.vae.vae.half().to(model.device).eval()
        model.unet.model = model.unet.model.half().to(model.device).eval()
        model.weight_dtype = model.unet.model.dtype
        model.audio_processor = model.AudioProcessor(feature_extractor_path="models/whisper")
        model.whisper = model.WhisperModel.from_pretrained("models/whisper", local_files_only=True)
        model.whisper = model.whisper.to(device=model.device, dtype=model.weight_dtype).eval()
        model.whisper.requires_grad_(False)
        model.fp = model.FaceParsing(left_cheek_width=90, right_cheek_width=90)
        self.avatar = None
        self.source_hash = None

    def prepare(self, source, work):
        identity = hashlib.sha256(source.read_bytes()).hexdigest()
        if self.avatar is not None and self.source_hash == identity:
            return self.avatar
        data, duration = probe(source)
        video = next((v for v in data["streams"] if v["codec_type"] == "video"), None)
        if video is None or duration > 10.1 or not 0 < video["width"] * video["height"] <= 3840 * 2160:
            raise ValueError("SOURCE_LIMIT")
        normalized = work / "normalized.mp4"
        scale = ("scale='if(gte(iw,ih),min(1280,iw),min(720,iw))':"
                 "'if(gte(iw,ih),min(720,ih),min(1280,ih))':"
                 "force_original_aspect_ratio=decrease:force_divisible_by=2,fps=25")
        command(["ffmpeg", "-nostdin", "-v", "error", "-protocol_whitelist", "file,pipe", "-format_whitelist", "mov", "-i", str(source), "-map", "0:v:0", "-an",
                 "-vf", scale, "-c:v", "libx264", "-crf", "16", "-preset", "fast", str(normalized)])
        if self.avatar is not None:
            # Exactly the cache created by this worker, never a user media path.
            shutil.rmtree(self.root / self.avatar.avatar_path)
            self.avatar = None
            self.source_hash = None
        identity_id = "concat-" + uuid4().hex
        owned = self.root / "results/v15/avatars" / identity_id
        try:
            avatar = self.model.Avatar(identity_id, str(normalized), 0, 8, True)
            if not avatar.coord_list_cycle or len(avatar.coord_list_cycle) != len(avatar.input_latent_list_cycle):
                raise ValueError("FACE_MISSING_IN_SOURCE")
            self.avatar, self.source_hash = avatar, identity
            return avatar
        except BaseException:
            if owned.is_dir():
                shutil.rmtree(owned)
            raise

    def render(self, source, speech, work):
        started = time.monotonic()
        _, audio_seconds = probe(speech)
        if audio_seconds > 15.05:
            raise ValueError("AUDIO_LIMIT")
        audio = work / "reply.wav"
        command(["ffmpeg", "-nostdin", "-v", "error", "-protocol_whitelist", "file,pipe", "-format_whitelist", "mov,wav,mp3,ogg,flac,aac", "-i", str(speech), "-map", "0:a:0",
                 "-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", str(audio)])
        m, torch = self.model, self.torch
        with torch.inference_mode():
            avatar = self.prepare(source, work)
            features, length = m.audio_processor.get_audio_feature(str(audio), weight_dtype=m.weight_dtype)
            chunks = m.audio_processor.get_whisper_chunk(features, m.device, m.weight_dtype, m.whisper, length,
                                                        fps=25, audio_padding_length_left=2, audio_padding_length_right=2)
            if not 1 <= len(chunks) <= 380:
                raise ValueError("AUDIO_LIMIT")
            height, width = avatar.frame_list_cycle[0].shape[:2]
            output = work / "answer.mp4"
            encoder = subprocess.Popen(["ffmpeg", "-nostdin", "-v", "error", "-f", "rawvideo",
                "-pix_fmt", "bgr24", "-s", f"{width}x{height}", "-r", "25", "-i", "pipe:0",
                "-i", str(audio), "-map", "0:v:0", "-map", "1:a:0", "-c:v", "libx264", "-crf", "17",
                "-preset", "fast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "160k",
                "-shortest", "-movflags", "+faststart", str(output)],
                stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            frame_index = 0
            try:
                for whisper_batch, latent_batch in m.datagen(chunks, avatar.input_latent_list_cycle, avatar.batch_size):
                    audio_features = m.pe(whisper_batch.to(m.device))
                    latent_batch = latent_batch.to(device=m.device, dtype=m.unet.model.dtype)
                    prediction = m.unet.model(latent_batch, m.timesteps, encoder_hidden_states=audio_features).sample
                    pictures = m.vae.decode_latents(prediction.to(dtype=m.vae.vae.dtype))
                    for picture in pictures:
                        index = frame_index % len(avatar.frame_list_cycle)
                        frame = copy.deepcopy(avatar.frame_list_cycle[index])
                        bbox = avatar.coord_list_cycle[index]
                        x1, y1, x2, y2 = bbox
                        patch = m.cv2.resize(picture.astype(m.np.uint8), (x2 - x1, y2 - y1))
                        frame = m.get_image_blending(frame, patch, bbox, avatar.mask_list_cycle[index],
                                                    avatar.mask_coords_list_cycle[index])
                        encoder.stdin.write(frame.tobytes())
                        frame_index += 1
                encoder.stdin.close()
                if encoder.wait(timeout=60) != 0:
                    raise RuntimeError("ENCODE_FAILED")
            finally:
                if encoder.poll() is None:
                    encoder.kill()
                    encoder.wait(timeout=10)
        result_data, result_seconds = probe(output)
        if abs(result_seconds - audio_seconds) > 0.15 or not any(s["codec_type"] == "audio" for s in result_data["streams"]):
            raise RuntimeError("AV_DURATION_MISMATCH")
        if output.stat().st_size > 60 * 1024 * 1024:
            raise RuntimeError("RESULT_LIMIT")
        elapsed = time.monotonic() - started
        print(f"LIPSYNC_RENDER frames={frame_index} seconds={elapsed:.3f} audio_seconds={audio_seconds:.3f}", flush=True)
        return output.read_bytes(), {"render_seconds": elapsed, "audio_seconds": audio_seconds}
