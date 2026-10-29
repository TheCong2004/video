import io
import json
import os
import sys
import shutil
import hashlib
import threading
import subprocess
import re
import unicodedata
import urllib.request
from pathlib import Path
import numpy as np
import soundfile as sf
import librosa
from fastapi import Body, FastAPI, HTTPException, UploadFile, File, Form, Response
from fastapi.responses import StreamingResponse
from vieneu import Vieneu

from runtime_config import load_config
from runtime_paths import tts_cache_dir

try:
    from utils.vietnamese_ecom_normalizer import normalize_ecom_livestream_text
except ImportError:
    from pathlib import Path
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
    from utils.vietnamese_ecom_normalizer import normalize_ecom_livestream_text

app = FastAPI(title="S-Live Voice Pro")
config = load_config()

# Danh sách giọng đọc mặc định và chi tiết phân loại
DEFAULT_VOICE = "Trúc Ly"

# Bộ metadata phong phú cho các giọng đọc v3-Turbo
VOICE_METADATA = {
    "Minh Đức": {
        "gender": "Nam",
        "region": "Bắc",
        "style": "tin_tuc",
        "description": "Nam · Miền Bắc · Chuẩn phát thanh viên, phong cách tin tức, thời sự, MC",
        "tags": ["nam", "bac", "tin_tuc", "chuyen_nghiep", "mc"]
    },
    "Phạm Tuyên": {
        "gender": "Nam",
        "region": "Bắc",
        "style": "tu_nhien",
        "description": "Nam · Miền Bắc · Tự nhiên, trầm ấm, truyền cảm hứng",
        "tags": ["nam", "bac", "tu_nhien", "tram_am"]
    },
    "Thái Sơn": {
        "gender": "Nam",
        "region": "Bắc",
        "style": "tu_nhien",
        "description": "Nam · Miền Bắc · Trầm ấm, phóng sự, thuyết minh",
        "tags": ["nam", "bac", "phong_su", "thuyet_minh"]
    },
    "Thanh Bình": {
        "gender": "Nam",
        "region": "Bắc",
        "style": "tu_nhien",
        "description": "Nam · Miền Bắc · Giọng trẻ, năng động, đời thường",
        "tags": ["nam", "bac", "tre_trung", "doi_thuong"]
    },
    "Xuân Vĩnh": {
        "gender": "Nam",
        "region": "Nam",
        "style": "doc_truyen",
        "description": "Nam · Miền Nam · Truyền cảm, ấm áp, đọc truyện, tâm sự",
        "tags": ["nam", "nam_bo", "doc_truyen", "tam_su", "mientay"]
    },
    "Mai Anh": {
        "gender": "Nữ",
        "region": "Bắc",
        "style": "tu_nhien",
        "description": "Nữ · Miền Bắc · Tươi sáng, duyên dáng, livestream, bán hàng, quảng cáo",
        "tags": ["nu", "bac", "livestream", "ban_hang", "tuoi_sang"]
    },
    "Trúc Ly": {
        "gender": "Nữ",
        "region": "Bắc",
        "style": "tu_nhien",
        "description": "Nữ · Miền Bắc · Ngọt ngào, tự nhiên, rõ ràng, dễ nghe",
        "tags": ["nu", "bac", "ngot_ngao", "pho_bien", "tu_nhien"]
    },
    "Ngọc Linh": {
        "gender": "Nữ",
        "region": "Bắc",
        "style": "tu_nhien",
        "description": "Nữ · Miền Bắc · Nhẹ nhàng, tâm sự, truyền cảm",
        "tags": ["nu", "bac", "nhe_nhang", "tam_su"]
    },
    "Đoan Trang": {
        "gender": "Nữ",
        "region": "Bắc",
        "style": "tin_tuc",
        "description": "Nữ · Miền Bắc · Đĩnh đạc, rành mạch, tin tức, học thuật",
        "tags": ["nu", "bac", "dinh_dac", "tin_tuc"]
    },
    "Thục Đoan": {
        "gender": "Nữ",
        "region": "Nam",
        "style": "doc_truyen",
        "description": "Nữ · Miền Nam · Dịu dàng, cuốn hút, giọng miền Nam chuẩn",
        "tags": ["nu", "nam_bo", "diu_dang", "mientay", "tu_nhien"]
    },
    "Minh Triết": {
        "gender": "Nam",
        "region": "Nam",
        "style": "tin_tuc",
        "description": "Nam · Miền Nam · Chững chạc, tin tức, MC thời sự",
        "tags": ["nam", "nam_bo", "tin_tuc", "chuyen_nghiep"]
    },
    "Thùy Dung": {
        "gender": "Nữ",
        "region": "Nam",
        "style": "tin_tuc",
        "description": "Nữ · Miền Nam · Năng động, sắc sảo, tin tức, livestream",
        "tags": ["nu", "nam_bo", "tin_tuc", "nang_dong"]
    },
    "Quang Sơn": {
        "gender": "Nam",
        "region": "Trung",
        "style": "tu_nhien",
        "description": "Nam · Miền Trung · Ấm áp, mộc mạc, chuẩn giọng miền Trung",
        "tags": ["nam", "mientrung", "tu_nhien", "tram_am"]
    },
    "Ngọc Trân": {
        "gender": "Nữ",
        "region": "Trung",
        "style": "tu_nhien",
        "description": "Nữ · Miền Trung · Duyên dáng, dịu dàng, chuẩn giọng miền Trung",
        "tags": ["nu", "mientrung", "tu_nhien", "diu_dang"]
    }
}

# Alias mặc định ánh xạ tiện lợi
BUILTIN_ALIASES = {
    "voice1": "Minh Đức",
    "voice2": "Mai Anh",
    "voice3": "Phạm Tuyên",
    "voice4": "Ngọc Linh",
    "voice5": "Xuân Vĩnh",
    "voice6": "Trúc Ly",
    "voice7": "Thái Sơn",
    "voice8": "Thanh Bình",
    "voice9": "Đoan Trang",
    "voice10": "Thục Đoan",
    "voice11": "Minh Triết",
    "voice12": "Thùy Dung",
    "voice13": "Quang Sơn",
    "voice14": "Ngọc Trân",
    # Ánh xạ theo tên viết liền hoặc không dấu
    "minh_duc": "Minh Đức",
    "minhduc": "Minh Đức",
    "pham_tuyen": "Phạm Tuyên",
    "phamtuyen": "Phạm Tuyên",
    "tuyen": "Phạm Tuyên",
    "thai_son": "Thái Sơn",
    "thaison": "Thái Sơn",
    "thanh_binh": "Thanh Bình",
    "thanhbinh": "Thanh Bình",
    "binh": "Thanh Bình",
    "xuan_vinh": "Xuân Vĩnh",
    "xuanvinh": "Xuân Vĩnh",
    "vinh": "Xuân Vĩnh",
    "mai_anh": "Mai Anh",
    "maianh": "Mai Anh",
    "truc_ly": "Trúc Ly",
    "trucly": "Trúc Ly",
    "ngoc_linh": "Ngọc Linh",
    "ngoclinh": "Ngọc Linh",
    "doan_trang": "Đoan Trang",
    "doantrang": "Đoan Trang",
    "thuc_doan": "Thục Đoan",
    "thucdoan": "Thục Đoan",
    "minh_triet": "Minh Triết",
    "minhtriet": "Minh Triết",
    "triet": "Minh Triết",
    "thuy_dung": "Thùy Dung",
    "thuydung": "Thùy Dung",
    "dung": "Thùy Dung",
    "quang_son": "Quang Sơn",
    "quangson": "Quang Sơn",
    "ngoc_tran": "Ngọc Trân",
    "ngoctran": "Ngọc Trân",
    # Ánh xạ theo nhu cầu
    "nam_bac": "Minh Đức",
    "nu_bac": "Trúc Ly",
    "nam_nam": "Xuân Vĩnh",
    "nu_nam": "Thục Đoan",
    "nam_trung": "Quang Sơn",
    "nu_trung": "Ngọc Trân",
    "livestream": "Mai Anh",
    "tin_tuc": "Minh Đức",
    "doc_truyen": "Xuân Vĩnh",
}

# Gộp cấu hình alias từ config người dùng
CONFIG_ALIASES = config.get("tts_voice_aliases") or {}
ALL_VOICE_ALIASES = {**BUILTIN_ALIASES, **CONFIG_ALIASES}

print("Loading VieNeu-TTS Engine (v3-Turbo)...", flush=True)
try:
    cache_hub = tts_cache_dir() / "hub"
    model_snapshots = cache_hub / "models--pnnbao-ump--VieNeu-TTS-v3-Turbo" / "snapshots"
    codec_snapshots = cache_hub / "models--OpenMOSS-Team--MOSS-Audio-Tokenizer-Nano-ONNX" / "snapshots"
    model_snapshot = next((p for p in model_snapshots.iterdir() if p.is_dir()), None)
    codec_snapshot = next((p for p in codec_snapshots.iterdir() if p.is_dir()), None)
    if not model_snapshot or not codec_snapshot:
        raise FileNotFoundError("Cache VieNeu-TTS/codec chưa đầy đủ.")
    
    def _ensure_speaker_encoder(target_dir: Path) -> bool:
        """Đảm bảo speaker_encoder.onnx có mặt trong model_snapshot để phục vụ tính năng clone giọng."""
        if target_dir is None:
            return False
        target_file = target_dir / "speaker_encoder.onnx"
        if target_file.is_file() and target_file.stat().st_size > 1000000:
            return True

        # 1. Quét các nguồn cache có sẵn trên máy
        base_app_dir = Path(__file__).resolve().parent.parent
        candidates = [
            Path("E:/huggingface/hub/models--pnnbao-ump--VieNeu-TTS-v3-Turbo"),
            Path.home() / ".cache" / "huggingface" / "hub" / "models--pnnbao-ump--VieNeu-TTS-v3-Turbo",
            base_app_dir / "tts_model_cache" / "hub" / "models--pnnbao-ump--VieNeu-TTS-v3-Turbo",
        ]
        for cdir in candidates:
            if cdir.is_dir():
                found = list(cdir.glob("**/speaker_encoder.onnx"))
                if found:
                    try:
                        shutil.copy2(found[0], target_file)
                        print(f"[TTS_SERVER] Auto-cached speaker_encoder.onnx from {found[0]}", flush=True)
                        return True
                    except Exception as copy_err:
                        print(f"WARN: Failed to copy speaker_encoder.onnx: {copy_err}", flush=True)

        # 2. Tự động tải từ Cloudflare R2 / Hugging Face về (khoảng 28MB)
        urls = [
            "https://pub-3ff033cbf4f2464d8c4d082f00e2180c.r2.dev/speaker_encoder.onnx",
            "https://huggingface.co/pnnbao-ump/VieNeu-TTS-v3-Turbo/resolve/main/speaker_encoder.onnx",
        ]
        temp_target = target_file.with_suffix(".tmp_download")
        for url in urls:
            try:
                print(f"[TTS_SERVER] Đang tự động tải speaker_encoder.onnx từ {url}...", flush=True)
                req = urllib.request.Request(
                    url,
                    headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) SliveVip/1.0"}
                )
                with urllib.request.urlopen(req, timeout=120) as resp:
                    with open(temp_target, "wb") as out_f:
                        shutil.copyfileobj(resp, out_f)
                if temp_target.is_file() and temp_target.stat().st_size > 1000000:
                    temp_target.replace(target_file)
                    print(f"[TTS_SERVER] Tải thành công speaker_encoder.onnx ({target_file.stat().st_size} bytes)", flush=True)
                    return True
            except Exception as dl_err:
                print(f"WARN: Không thể tải speaker_encoder.onnx từ {url}: {dl_err}", flush=True)
                if temp_target.is_file():
                    temp_target.unlink(missing_ok=True)

        return target_file.is_file() and target_file.stat().st_size > 1000000

    # Đảm bảo speaker_encoder.onnx có mặt trong model_snapshot
    _ensure_speaker_encoder(model_snapshot)

    import torch
    tts_device = "cuda" if torch.cuda.is_available() else "cpu"
    try:
        tts_model = Vieneu(
            backbone_repo=str(model_snapshot),
            backend="onnx",
            device=tts_device,
            precision="int8",
            onnx_dir=str(model_snapshot / "onnx_int8"),
            codec_dir=str(codec_snapshot),
        )
        print(f"VieNeu-TTS Engine v3-Turbo ready on {tts_device.upper()}!", flush=True)
    except Exception as cuda_exc:
        if tts_device == "cuda":
            print(f"VieNeu-TTS CUDA load fallback to CPU: {cuda_exc}", flush=True)
            tts_model = Vieneu(
                backbone_repo=str(model_snapshot),
                backend="onnx",
                device="cpu",
                precision="int8",
                onnx_dir=str(model_snapshot / "onnx_int8"),
                codec_dir=str(codec_snapshot),
            )
            print("VieNeu-TTS Engine v3-Turbo ready on CPU!", flush=True)
        else:
            raise cuda_exc
except Exception as exc:
    print(f"Error loading VieNeu-TTS model: {exc}", flush=True)
    tts_model = None

SAMPLE_RATE = getattr(tts_model, "sample_rate", None) or 48000

_local_appdata = Path(os.environ.get("LOCALAPPDATA") or (Path.home() / "AppData" / "Local"))
VOICE_REGISTRY_DIR = _local_appdata / "SLiveAI" / "voices"
VOICE_REGISTRY_DIR.mkdir(parents=True, exist_ok=True)
_voice_profiles = {}
_voice_profiles_lock = threading.RLock()


def _voice_id_from_name(name: str) -> str:
    value = unicodedata.normalize("NFKD", str(name or "")).encode("ascii", "ignore").decode("ascii")
    value = re.sub(r"[^a-zA-Z0-9]+", "-", value).strip("-").lower()
    return value or "voice"


def _load_voice_profiles() -> None:
    """Load persisted speaker embeddings from SLiveAI voice registry."""
    if tts_model is None:
        return
    try:
        profile_files = list(VOICE_REGISTRY_DIR.glob("*/profile.json"))
    except OSError as exc:
        print(f"WARN: Voice registry is not readable: {exc}", flush=True)
        profile_files = []
    for profile_json in profile_files:
        try:
            metadata = json.loads(profile_json.read_text(encoding="utf-8"))
            voice_id = str(metadata.get("voice_id") or profile_json.parent.name)
            name = str(metadata.get("name") or voice_id).strip()
            embedding_path = profile_json.parent / "embedding.npy"
            codes_path = profile_json.parent / "reference_codes.npy"
            if not embedding_path.is_file():
                continue
            embedding = np.load(embedding_path, allow_pickle=False).astype(np.float32)
            codes = np.load(codes_path, allow_pickle=False).astype(np.int64) if codes_path.is_file() else None
            tts_model._preset_voices[name] = {
                "description": metadata.get("description", "Giọng cá nhân"),
                "gender": metadata.get("gender", ""),
                "style": metadata.get("style", "tu_nhien"),
                "speaker_emb": embedding,
                "codes": codes,
            }
            metadata["voice_id"] = voice_id
            metadata["name"] = name
            _voice_profiles[voice_id] = metadata
            AVAILABLE_VOICES.add(name)
            print(f"[TTS_SERVER] Loaded cloned voice profile: '{name}' ({voice_id})", flush=True)
        except Exception as exc:
            print(f"WARN: Unable to load voice profile {profile_json}: {exc}", flush=True)


# Lấy danh sách giọng từ model nếu có
if tts_model is not None and hasattr(tts_model, "_preset_voices") and tts_model._preset_voices:
    AVAILABLE_VOICES = set(tts_model._preset_voices.keys())
else:
    AVAILABLE_VOICES = set(VOICE_METADATA.keys())

_load_voice_profiles()


def _ensure_wav_pcm16(audio_path: Path) -> sf.SoundFile:
    """Đảm bảo file âm thanh đầu vào luôn được chuyển đổi về chuẩn 16-bit PCM WAV (mono, 48kHz)."""
    # 1. Thử đọc nhanh nếu đã là WAV chuẩn PCM mono 48kHz
    try:
        info = sf.info(str(audio_path))
        if info.format == "WAV" and "PCM" in (info.subtype or "") and info.channels == 1 and info.samplerate == 48000:
            return info
    except Exception:
        pass

    # 2. Chuyển đổi bằng ffmpeg lên chuẩn 48kHz mono
    temp_wav = audio_path.with_suffix(".tmp_pcm16.wav")
    try:
        import imageio_ffmpeg
        ffmpeg_exe = imageio_ffmpeg.get_ffmpeg_exe()
        cmd = [
            ffmpeg_exe, "-y", "-i", str(audio_path),
            "-ac", "1", "-ar", "48000", "-c:a", "pcm_s16le",
            str(temp_wav)
        ]
        kwargs = {}
        if hasattr(subprocess, "CREATE_NO_WINDOW"):
            kwargs["creationflags"] = subprocess.CREATE_NO_WINDOW
        subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True, **kwargs)
        if temp_wav.exists() and temp_wav.stat().st_size > 44:
            shutil.move(str(temp_wav), str(audio_path))
            return sf.info(str(audio_path))
    except Exception as e:
        print(f"WARN: ffmpeg conversion failed: {e}", flush=True)
        if temp_wav.exists():
            temp_wav.unlink(missing_ok=True)

    # 3. Fallback dùng librosa nếu không có ffmpeg
    try:
        y, sr = librosa.load(str(audio_path), sr=48000, mono=True)
        if y is not None and len(y) > 0:
            sf.write(str(audio_path), y, 48000, subtype='PCM_16', format='WAV')
            return sf.info(str(audio_path))
    except Exception as e:
        print(f"WARN: librosa fallback failed: {e}", flush=True)

    return sf.info(str(audio_path))


def _smart_preprocess_voice_clip(audio_path: Path, max_duration: float = 7.8, target_sr: int = 48000) -> tuple:
    """
    Tiền xử lý âm thanh mẫu để đạt độ tương đồng (voice similarity) cao nhất:
    1. Cắt tỉa khoảng lặng thừa (VAD trimming) ở đầu và đuôi file.
    2. Quét và trích xuất phân đoạn liên tục tối ưu (lên tới 7.8s) nếu file dài hơn.
    3. Chuẩn hóa Peak âm lượng về -1.0 dBFS để tránh méo hoặc quá nhỏ.
    4. Đo độ ồn nền (noise floor) để quyết định tự động có nên chạy bộ lọc khử ồn (denoiser) hay không.
    """
    wav, sr = sf.read(str(audio_path), dtype='float32')
    if wav.ndim > 1:
        wav = wav.mean(axis=1)

    # Resample sang 48000 nếu chưa đúng
    if sr != target_sr:
        try:
            import soxr
            wav = soxr.resample(wav, sr, target_sr).astype(np.float32)
        except Exception:
            try:
                wav = librosa.resample(wav, orig_sr=sr, target_sr=target_sr).astype(np.float32)
            except Exception:
                pass
        sr = target_sr

    # 1. Cắt tỉa khoảng lặng đầu/đuôi bằng năng lượng khung (25ms window, 10ms hop)
    frame_len = int(sr * 0.025)
    hop_len = int(sr * 0.010)
    if len(wav) > frame_len + hop_len:
        frames = np.lib.stride_tricks.sliding_window_view(wav, frame_len)[::hop_len]
        energy = np.mean(frames ** 2, axis=1)
        peak_energy = max(float(np.max(energy)), 1e-12)
        energy_db = 10.0 * np.log10(np.maximum(energy, 1e-12) / peak_energy)

        # Ngưỡng phát âm: các khung năng lượng > -38 dB so với đỉnh
        active = np.where(energy_db > -38.0)[0]
        if len(active) > 0:
            start_sample = max(0, int(active[0] * hop_len - sr * 0.05))
            end_sample = min(len(wav), int((active[-1] * hop_len) + frame_len + sr * 0.05))
            wav = wav[start_sample:end_sample]

    # 2. Nếu sau khi tỉa, âm thanh vẫn dài hơn max_duration (7.8s)
    # VieNeu chỉ nhận tối đa 8.0s -> quét cửa sổ 7.8s có năng lượng phát âm cao nhất
    max_samples = int(max_duration * sr)
    if len(wav) > max_samples:
        step = int(sr * 0.25)
        best_start = 0
        best_score = -1e9
        for s in range(0, len(wav) - max_samples + 1, step):
            segment = wav[s:s + max_samples]
            score = float(np.sum(segment ** 2))
            if score > best_score:
                best_score = score
                best_start = s
        wav = wav[best_start:best_start + max_samples]

    # 3. Chuẩn hóa Peak âm lượng về -1.0 dBFS (~0.89)
    peak = float(np.max(np.abs(wav))) if len(wav) > 0 else 0.0
    if peak > 1e-5:
        target_peak = 0.89
        wav = wav * (target_peak / peak)

    # 4. Đo độ ồn nền (noise floor) ở cửa sổ tĩnh nhất 150ms
    noise_win = int(sr * 0.15)
    if len(wav) > noise_win:
        win_rms = [np.sqrt(np.mean(wav[i:i+noise_win]**2)) for i in range(0, len(wav) - noise_win, noise_win // 2)]
        min_rms = float(min(win_rms)) if win_rms else 0.0
    else:
        min_rms = 0.0
    noise_dbfs = 20.0 * np.log10(max(min_rms, 1e-9))

    recommend_denoise = bool(noise_dbfs > -32.0)

    # Ghi lại file đã chuẩn hóa tối ưu
    wav_int16 = (np.clip(wav, -1.0, 1.0) * 32767.0).astype(np.int16)
    sf.write(str(audio_path), wav_int16, sr, subtype='PCM_16', format='WAV')

    stats = {
        "duration_sec": round(len(wav) / sr, 2),
        "peak_dbfs": round(20.0 * np.log10(max(peak, 1e-9)), 1),
        "noise_floor_dbfs": round(noise_dbfs, 1),
        "recommend_denoise": recommend_denoise,
    }
    return recommend_denoise, stats


def normalize_vietnamese_text(text: str) -> str:
    """Chuẩn hoá câu chữ tiếng Việt toàn diện qua bộ chuẩn hóa TMĐT & Phòng thu VIP."""
    if not text:
        return ""
    try:
        return normalize_ecom_livestream_text(text)
    except Exception as exc:
        print(f"[TTS_SERVER] Lỗi chuẩn hóa ngữ âm: {exc}", flush=True)
        # Fallback cơ bản
        text = unicodedata.normalize("NFC", str(text))
        text = re.sub(r'[\r\n\t]+', ' ', text)
        text = re.sub(r'\s+', ' ', text).strip()
        return text


def resolve_voice(voice_input) -> str:
    """Ánh xạ tên giọng nhập vào thành giọng chuẩn hỗ trợ tốt nhất."""
    if not voice_input:
        return DEFAULT_VOICE
    
    raw = str(voice_input).strip()
    if raw in AVAILABLE_VOICES:
        return raw

    # Check preset_voices in tts_model directly
    if tts_model is not None and hasattr(tts_model, "_preset_voices") and tts_model._preset_voices:
        if raw in tts_model._preset_voices:
            return raw
        for vname in tts_model._preset_voices.keys():
            if vname.lower() == raw.lower():
                return vname
    
    # Tìm trong alias
    lower_raw = raw.lower().replace(" ", "_").replace("-", "_")
    if raw in ALL_VOICE_ALIASES:
        target = ALL_VOICE_ALIASES[raw]
        if target in AVAILABLE_VOICES:
            return target
            
    if lower_raw in ALL_VOICE_ALIASES:
        target = ALL_VOICE_ALIASES[lower_raw]
        if target in AVAILABLE_VOICES:
            return target
            
    # Thử tìm tương đối theo tên
    for vname in AVAILABLE_VOICES:
        if vname.lower() == raw.lower():
            return vname
            
    return DEFAULT_VOICE


def stretch_audio_wsola(audio: np.ndarray, sr: int, speed: float) -> np.ndarray:
    """Điều chỉnh tốc độ phát âm thanh bằng thuật toán WSOLA (FFmpeg atempo).
    Giữ nguyên cao độ (pitch) và chất âm tự nhiên của giọng nói, không bị méo tiếng/kim loại như STFT."""
    if abs(speed - 1.0) < 0.02 or audio.size == 0:
        return audio
    speed = max(0.5, min(2.0, float(speed)))
    try:
        in_buf = io.BytesIO()
        sf.write(in_buf, audio, sr, format="WAV", subtype="PCM_16")
        cmd = [
            "ffmpeg", "-y",
            "-f", "wav", "-i", "pipe:0",
            "-af", f"atempo={speed:.3f}",
            "-f", "wav", "pipe:1"
        ]
        kwargs = {}
        if hasattr(subprocess, "CREATE_NO_WINDOW"):
            kwargs["creationflags"] = subprocess.CREATE_NO_WINDOW
        proc = subprocess.run(
            cmd,
            input=in_buf.getvalue(),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
            **kwargs
        )
        out_audio, _ = sf.read(io.BytesIO(proc.stdout), dtype="float32")
        return out_audio
    except Exception as exc:
        # Fallback an toàn về librosa nếu FFmpeg gặp sự cố
        try:
            return librosa.effects.time_stretch(y=audio, rate=speed)
        except Exception:
            return audio


def post_process_audio(audio: np.ndarray, speed: float = 1.0) -> np.ndarray:
    """Xử lý hậu kỳ âm thanh: Cắt khoảng lặng tự nhiên, Điều chỉnh tốc độ WSOLA, Peak Normalization, Fade, Soft Limiter."""
    if not isinstance(audio, np.ndarray) or audio.size == 0:
        return np.zeros(0, dtype=np.float32)
        
    audio = audio.reshape(-1).astype(np.float32)

    # 1. Cắt khoảng lặng dư thừa ở đầu và cuối nhưng giữ lại đệm tự nhiên để không bị nuốt âm (top_db=38)
    try:
        trimmed_audio, index = librosa.effects.trim(audio, top_db=38)
        if len(trimmed_audio) > int(SAMPLE_RATE * 0.08):
            # Giữ lại một khoảng thở tự nhiên (20ms đầu, 40ms đuôi) để âm bật và âm kết không bị cụt
            lead_pad = int(SAMPLE_RATE * 0.02)
            tail_pad = int(SAMPLE_RATE * 0.04)
            start_idx = max(0, int(index[0]) - lead_pad)
            end_idx = min(len(audio), int(index[1]) + tail_pad)
            audio = audio[start_idx:end_idx]
    except Exception:
        pass
    
    # 2. Điều chỉnh tốc độ nói bằng WSOLA chuẩn phòng thu (chỉ can thiệp khi người dùng chỉnh khác 1.0x)
    speed = float(speed or 1.0)
    if abs(speed - 1.0) >= 0.02:
        audio = stretch_audio_wsola(audio, SAMPLE_RATE, speed)
            
    # 3. Chống pop/click ở đầu và cuối bằng short fade rất ngắn (2ms)
    fade_len = min(int(SAMPLE_RATE * 0.002), len(audio) // 4)
    if fade_len > 0:
        fade_in = np.linspace(0.0, 1.0, fade_len, dtype=np.float32)
        fade_out = np.linspace(1.0, 0.0, fade_len, dtype=np.float32)
        audio[:fade_len] *= fade_in
        audio[-fade_len:] *= fade_out
        
    # 4. Peak Normalization: chuẩn hoá âm lượng về mức -0.5 dB (~0.94) để giọng đọc rõ nét, không bị vỡ tiếng
    max_peak = np.max(np.abs(audio)) if audio.size else 0.0
    if max_peak > 0.001:
        target_peak = 0.94
        if max_peak > 1.0 or max_peak < 0.70:
            scale = target_peak / max_peak
            # Không tăng quá 3x nếu âm lượng vào quá nhỏ
            scale = min(3.0, scale)
            audio = audio * scale
            
    # Soft Limiter an toàn
    return np.clip(audio, -0.98, 0.98)


def synthesize(
    text: str,
    voice: str,
    speed: float = 1.0,
    style: str = "tu_nhien",
    temperature: float = 0.50,
    top_p: float = 0.95,
    top_k: int = 25,
    repetition_penalty: float = 1.25
) -> np.ndarray:
    """Tạo âm thanh giọng nói với các siêu tham số tối ưu độ trong trẻo và tự nhiên."""
    if tts_model is None:
        raise RuntimeError("Mô hình giọng nói AI chưa tải thành công.")
        
    cleaned_text = normalize_vietnamese_text(text)
    if not cleaned_text:
        return np.zeros(0, dtype=np.float32)
        
    selected_voice = resolve_voice(voice)
    voice_info = VOICE_METADATA.get(selected_voice, {})
    
    # Nếu người dùng không chỉ định style cụ thể, dùng style tối ưu của giọng
    if not style or style == "tu_nhien":
        style = voice_info.get("style", "tu_nhien")
        
    # Tinh chỉnh tham số inference để tốc độ sinh âm thanh nhanh nhất và giọng tự nhiên
    infer_kwargs = {
        "text": cleaned_text,
        "voice": selected_voice,
        "style": style,
        "use_ref_codes": True,      # Bật trích xuất mã tham chiếu âm học v3 để giọng chuẩn xác nhất
        "denoise": False,           # Tắt denoise để tăng tốc độ sinh audio lên gấp đôi, giảm độ trễ
        "temperature": max(0.2, min(0.9, float(temperature or 0.50))), # 0.50 giữ giọng ổn định, không bị méo/run
        "top_k": int(top_k or 25),
        "top_p": max(0.7, min(1.0, float(top_p or 0.95))),
        "repetition_penalty": float(repetition_penalty or 1.25),
        "max_chars": 220,           # Chunk kích thước chuẩn cho v3-Turbo
        "silence_p": 0.1,           # Khoảng dừng thở tự nhiên (100ms) giữa các câu/mệnh đề giúp nhịp điệu tự nhiên, không bị nói dồn dập
    }
    
    try:
        raw_audio = tts_model.infer(**infer_kwargs)
    except Exception as exc:
        # Fallback với tham số cơ bản nếu có lỗi
        raw_audio = tts_model.infer(text=cleaned_text, voice=selected_voice)
        
    effective_speed = float(speed if speed is not None else 1.0)
    return post_process_audio(raw_audio, speed=effective_speed)


@app.post("/v1/audio/speech")
def tts(payload: dict = Body(...)):
    if tts_model is None:
        raise HTTPException(status_code=503, detail="Mô hình giọng nói AI chưa tải thành công.")
        
    text = str(payload.get("input") or "").strip()
    if not text:
        raise HTTPException(status_code=400, detail="Nội dung đọc đang trống.")
        
    voice = payload.get("voice", DEFAULT_VOICE)
    speed = float(payload.get("speed", 1.0) or 1.0)
    style = payload.get("style", "tu_nhien")
    temperature = payload.get("temperature", 0.50)
    
    try:
        audio = synthesize(
            text=text,
            voice=voice,
            speed=speed,
            style=style,
            temperature=temperature
        )
    except Exception as exc:
        raise HTTPException(status_code=500, detail=str(exc)) from exc
        
    if audio.ndim > 1:
        audio = audio.mean(axis=-1)
        
    # Resample sang 16000 Hz chuẩn để gửi qua HTTP nhanh gấp 3 lần và triệt tiêu CPU resampy ở client
    target_sr = 16000
    if SAMPLE_RATE == 48000 and len(audio) > 0:
        try:
            from scipy.signal import resample_poly
            audio = resample_poly(audio, 1, 3).astype(np.float32)
        except Exception:
            try:
                audio = librosa.resample(audio, orig_sr=SAMPLE_RATE, target_sr=target_sr)
            except Exception:
                target_sr = SAMPLE_RATE
    else:
        target_sr = SAMPLE_RATE

    output = io.BytesIO()
    sf.write(output, audio, target_sr, format="WAV")
    output.seek(0)
    return StreamingResponse(output, media_type="audio/wav")


@app.get("/v1/voices")
def get_voices():
    """Trả về danh sách giọng đọc đầy đủ kèm phân loại vùng miền, giới tính và mô tả (hỗ trợ cả custom và presets)."""
    custom = []
    with _voice_profiles_lock:
        for voice_id, profile in _voice_profiles.items():
            custom.append({
                "id": voice_id,
                "voice_id": voice_id,
                "name": profile.get("name", voice_id),
                "description": profile.get("description", "Giọng cá nhân"),
                "gender": profile.get("gender", ""),
                "style": profile.get("style", "tu_nhien"),
                "revision": profile.get("revision", ""),
                "custom": True,
            })

    preset_names = sorted(set(VOICE_METADATA.keys()))
    preset_details = []
    voice_list = []

    for vname in preset_names:
        meta = VOICE_METADATA.get(vname, {
            "gender": "N/A",
            "region": "Toàn quốc",
            "style": "tu_nhien",
            "description": f"Giọng đọc AI {vname}",
            "tags": []
        })
        item = {
            "id": vname,
            "name": vname,
            "gender": meta.get("gender", ""),
            "region": meta.get("region", ""),
            "style": meta.get("style", "tu_nhien"),
            "description": meta.get("description", ""),
            "is_default": (vname == DEFAULT_VOICE),
            "custom": False,
        }
        preset_details.append(item)
        voice_list.append(item)

    for c in custom:
        voice_list.append({
            "id": c["name"],
            "name": c["name"],
            "gender": c.get("gender", ""),
            "region": "Cá nhân",
            "style": c.get("style", "tu_nhien"),
            "description": c.get("description", "Giọng cá nhân"),
            "is_default": False,
            "custom": True,
        })

    return {
        "success": True,
        "voices": sorted(AVAILABLE_VOICES),
        "presets": preset_details,
        "custom": sorted(custom, key=lambda item: item["name"].lower()),
        "default_voice": DEFAULT_VOICE,
        "voice_details": voice_list,
        "aliases": ALL_VOICE_ALIASES
    }


@app.post("/v1/voices/enroll")
async def enroll_voice(
    file: UploadFile = File(...),
    name: str = Form(...),
    description: str = Form("Giọng cá nhân"),
    gender: str = Form(""),
    style: str = Form("tu_nhien"),
    denoise: str = Form("auto"),
):
    if tts_model is None:
        raise HTTPException(status_code=503, detail="Mô hình giọng nói AI chưa được tải thành công.")
    name = unicodedata.normalize("NFC", str(name or "").strip())
    if not name or len(name) > 64:
        raise HTTPException(status_code=400, detail="Tên giọng phải có từ 1 đến 64 ký tự.")
    preset_names = set(getattr(tts_model, "_preset_voices", {}) or {})
    with _voice_profiles_lock:
        if name in preset_names or any(profile.get("name") == name for profile in _voice_profiles.values()):
            raise HTTPException(status_code=409, detail="Tên giọng đã tồn tại.")
    voice_id = _voice_id_from_name(name)
    suffix = 2
    while (VOICE_REGISTRY_DIR / voice_id).exists():
        voice_id = f"{_voice_id_from_name(name)}-{suffix}"
        suffix += 1
    profile_dir = VOICE_REGISTRY_DIR / voice_id
    try:
        profile_dir.mkdir(parents=True, exist_ok=False)
    except OSError as exc:
        raise HTTPException(status_code=500, detail=f"Không thể tạo thư mục profile giọng: {exc}")
    ref_path = profile_dir / "reference.wav"
    try:
        content = await file.read()
        if len(content) < 1024 or len(content) > 50 * 1024 * 1024:
            raise ValueError("File âm thanh không hợp lệ hoặc quá lớn.")
        ref_path.write_bytes(content)
        info = None
        try:
            info = _ensure_wav_pcm16(ref_path)
        except Exception as conv_err:
            raise ValueError(
                "Định dạng file âm thanh không được hỗ trợ hoặc bị lỗi. "
                "Hệ thống hỗ trợ file .WAV, .MP3, .M4A hoặc bạn có thể dùng chức năng 'Ghi âm trực tiếp' qua Micro."
            )
        if info.channels > 2 or info.duration < 1.0 or info.duration > 35.0:
            raise ValueError("Mẫu giọng phải là âm thanh mono/stereo dài từ 1 đến 35 giây.")
        if not hasattr(tts_model, "encode_reference"):
            raise RuntimeError("Engine hiện tại không hỗ trợ đăng ký giọng cá nhân.")

        # Tiền xử lý âm thanh thông minh: VAD trimming, tối ưu phân đoạn 7.8s và chuẩn hóa âm lượng
        rec_denoise, audio_stats = _smart_preprocess_voice_clip(ref_path)

        denoise_clean = str(denoise or "auto").strip().lower()
        if denoise_clean in ("true", "1", "yes"):
            should_denoise = True
        elif denoise_clean in ("false", "0", "no"):
            should_denoise = False
        else:
            should_denoise = rec_denoise

        print(f"[VoiceEnroll] Voice: '{name}', Denoise Mode: {denoise_clean} (Applied: {should_denoise}), Audio Stats: {audio_stats}", flush=True)

        if model_snapshot is not None:
            _ensure_speaker_encoder(model_snapshot)

        embedding, codes = tts_model.encode_reference(str(ref_path), denoise=should_denoise)
        np.save(profile_dir / "embedding.npy", np.asarray(embedding, dtype=np.float32))
        if codes is not None:
            np.save(profile_dir / "reference_codes.npy", np.asarray(codes, dtype=np.int64))
        revision = hashlib.sha256(content).hexdigest()[:16]
        metadata = {
            "voice_id": voice_id,
            "name": name,
            "description": str(description or "Giọng cá nhân")[:200],
            "gender": str(gender or "")[:32],
            "style": str(style or "tu_nhien")[:64],
            "revision": revision,
            "reference_sha256": hashlib.sha256(content).hexdigest(),
            "model_version": getattr(tts_model, "version", "v3-Turbo"),
            "denoised": should_denoise,
            "audio_stats": audio_stats,
        }
        (profile_dir / "profile.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")
        tts_model._preset_voices[name] = {
            "description": metadata["description"], "gender": metadata["gender"], "style": metadata["style"],
            "speaker_emb": np.asarray(embedding, dtype=np.float32),
            "codes": None if codes is None else np.asarray(codes, dtype=np.int64),
        }
        AVAILABLE_VOICES.add(name)
        with _voice_profiles_lock:
            _voice_profiles[voice_id] = metadata
        return {"success": True, "voice": metadata}
    except Exception as exc:
        shutil.rmtree(profile_dir, ignore_errors=True)
        raise HTTPException(status_code=400, detail=str(exc))


@app.delete("/v1/voices/{voice_id}")
def delete_voice(voice_id: str):
    with _voice_profiles_lock:
        profile = _voice_profiles.get(voice_id)
        if not profile:
            raise HTTPException(status_code=404, detail="Không tìm thấy giọng cá nhân.")
        _voice_profiles.pop(voice_id, None)
    name = profile.get("name")
    if name in AVAILABLE_VOICES:
        AVAILABLE_VOICES.remove(name)
    if tts_model is not None and hasattr(tts_model, "remove_voice"):
        tts_model.remove_voice(name, save=False)
    elif tts_model is not None and hasattr(tts_model, "_preset_voices") and name in tts_model._preset_voices:
        del tts_model._preset_voices[name]
    shutil.rmtree(VOICE_REGISTRY_DIR / voice_id, ignore_errors=True)
    return {"success": True, "voice_id": voice_id}


@app.get("/")
def health():
    return {
        "status": "ready" if tts_model is not None else "error",
        "engine": "S-Live Voice Pro",
        "sample_rate": SAMPLE_RATE,
        "default_voice": DEFAULT_VOICE,
        "total_voices": len(AVAILABLE_VOICES),
        "error": None if tts_model is not None else "Không tải được mô hình giọng nói AI."
    }
