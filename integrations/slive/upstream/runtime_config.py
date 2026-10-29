import agency_runtime
import json
import os
import re
import sys
import time
import urllib.request
from pathlib import Path
from runtime_paths import (
    apply_path_environment,
    config_path,
    migrate_legacy_layout,
    program_dir,
    tts_cache_dir,
)

CENTRAL_API_URL = agency_runtime.api_origin()
_last_remote_refresh = 0.0
_last_remote_llm = {}

DEFAULT_CONFIG = {
    "host": "127.0.0.1",
    "port": 8010,
    "model": "wav2lip",
    "avatar_id": "mau10",
    "transport": "webrtc",
    "llm_provider": "openai",
    "llm_model": "gpt-4o-mini",
    "llm_api_key": "",
    "tts": "f5api",
    "tts_host": "127.0.0.1",
    "tts_port": 8006,
    "tts_server": "http://127.0.0.1:8006",
    "tts_voice": "voice6",
    "tts_startup_timeout_seconds": 180,
    "tts_voice_aliases": {
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
        "voice14": "Ngọc Trân"
    },
    "llm": {
        "model": "gpt-4o-mini",
        "api_key": ""
    }
}


def application_dir() -> Path:
    return program_dir()


def get_app_version(app_dir=None) -> str:
    """Đọc version duy nhất từ version.json (Single Source of Truth)."""
    if agency_runtime.manifest():
        return agency_runtime.installed_version()
    search_dirs = [
        Path(app_dir) if app_dir else None,
        program_dir(),
        Path(__file__).resolve().parent,
        Path.cwd(),
    ]
    for d in search_dirs:
        if not d:
            continue
        v_file = d / "version.json"
        if v_file.is_file():
            try:
                data = json.loads(v_file.read_text(encoding="utf-8-sig"))
                v = data.get("version")
                if v:
                    return str(v).strip()
            except Exception:
                pass
    return "1.0.0"


def sync_version_files(base_dir=None):
    """Tự động đồng bộ version.json sang web/version.json và installer/version.iss."""
    try:
        root = Path(base_dir) if base_dir else Path(__file__).resolve().parent
        version_file = root / "version.json"
        if not version_file.is_file():
            return
        version_data = json.loads(version_file.read_text(encoding="utf-8-sig"))
        v = version_data.get("version", "1.0.0")

        # Đồng bộ web/version.json
        web_v = root / "web" / "version.json"
        web_v.parent.mkdir(parents=True, exist_ok=True)
        web_v.write_text(json.dumps({"version": v}, indent=2), encoding="utf-8")

        # Đồng bộ installer/version.iss cho Inno Setup
        iss_v = root / "installer" / "version.iss"
        iss_v.parent.mkdir(parents=True, exist_ok=True)
        iss_v.write_text(f'#define MyAppVersion "{v}"\n', encoding="utf-8")
    except Exception:
        pass


def load_config(app_dir=None):
    base = Path(app_dir) if app_dir else application_dir()
    config = json.loads(json.dumps(DEFAULT_CONFIG, ensure_ascii=False))
    migrate_legacy_layout(str(config.get("avatar_id") or "mau10"))
    path = config_path() if getattr(sys, "frozen", False) else base / "config.json"
    if getattr(sys, "frozen", False) and not path.is_file():
        default_path = base / "config.default.json"
        if not default_path.is_file():
            default_path = base / "config.json"
        if default_path.is_file():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(default_path.read_text(encoding="utf-8-sig"), encoding="utf-8")
    if path.is_file():
        with path.open("r", encoding="utf-8-sig") as handle:
            incoming = json.load(handle)
        if not isinstance(incoming, dict):
            raise ValueError("config.json phải chứa một JSON object.")
        migrated = False
        for obsolete_key in ("central_api_url", "llm_api_base_url", "ollama_base_url"):
            if obsolete_key in incoming:
                incoming.pop(obsolete_key, None)
                migrated = True
        incoming_llm = incoming.get("llm")
        if isinstance(incoming_llm, dict) and "base_url" in incoming_llm:
            incoming_llm.pop("base_url", None)
            migrated = True
        if migrated:
            path.write_text(json.dumps(incoming, ensure_ascii=False, indent=2), encoding="utf-8")
        llm = (config.get("llm") or {}).copy()
        llm.update(incoming.get("llm") or {})
        config.update(incoming)
        config["llm"] = llm

    # Development-only overrides. PyInstaller builds never read this file and
    # the file is intentionally excluded from Git and all build specifications.
    local_path = base / "config.local.json"
    if not getattr(sys, "frozen", False) and local_path.is_file():
        with local_path.open("r", encoding="utf-8-sig") as handle:
            local_config = json.load(handle)
        if not isinstance(local_config, dict):
            raise ValueError("config.local.json phải chứa một JSON object.")
        local_backend = str(local_config.get("central_api_url") or "").strip().rstrip("/")
        local_llm = str(local_config.get("llm_api_base_url") or "").strip().rstrip("/")
        if local_backend:
            config["runtime_central_api_url"] = local_backend
        if local_llm:
            config["runtime_llm_base_url"] = local_llm
        local_key = str(local_config.get("openai_api_key") or local_config.get("llm_api_key") or "").strip()
        if local_key:
            config["openai_api_key"] = local_key
            config["llm_api_key"] = local_key
        # Local-only service ports let developers run Live Thuong and Live VIP
        # side by side. These overrides are ignored by frozen customer builds.
        for key in ("host", "port", "tts_host", "tts_port", "tts_server"):
            if key in local_config:
                config[key] = local_config[key]
        config["local_development"] = True

    env_key = str(os.environ.get("OPENAI_API_KEY") or os.environ.get("LIVE_LLM_API_KEY") or os.environ.get("LLM_API_KEY") or "").strip()
    if env_key:
        if not config.get("openai_api_key"):
            config["openai_api_key"] = env_key
        if not config.get("llm_api_key"):
            config["llm_api_key"] = env_key

    config["port"] = int(config["port"])
    config["tts_port"] = int(config["tts_port"])
    config["tts_startup_timeout_seconds"] = int(config["tts_startup_timeout_seconds"])
    config["tts_server"] = str(config.get("tts_server") or
                               f"http://{config['tts_host']}:{config['tts_port']}").rstrip("/")
    if agency_runtime.manifest():
        config["runtime_central_api_url"] = agency_runtime.api_origin()
        config["port"] = agency_runtime.local_port(8010)
        config["tts_port"] = agency_runtime.local_port(8006)
        config["tts_server"] = f"http://127.0.0.1:{config['tts_port']}"
    return config


def _runtime_cache_path(config=None) -> Path:
    local = application_dir()
    if local.name.lower() == "app":
        local = local.parent
    filename = "runtime-config.dev.json" if (config or {}).get("local_development") else "runtime-config.json"
    path = local / "data" / filename
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def refresh_remote_runtime_config(config, timeout=8, max_age_seconds=60):
    """Merge public, non-secret runtime settings and keep an offline cache."""
    global _last_remote_refresh, _last_remote_llm
    if config.get("local_llm_override"):
        return config
    if _last_remote_llm and time.monotonic() - _last_remote_refresh < max_age_seconds:
        config.update(_last_remote_llm)
        return config
    cache_path = _runtime_cache_path(config)
    central_url = config.get("runtime_central_api_url") or CENTRAL_API_URL
    payload = None
    try:
        request = urllib.request.Request(
            f"{central_url}/api/desktop/runtime-config",
            headers={"Accept": "application/json", "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"},
        )
        import ssl
        ssl_ctx = ssl.create_default_context()
        with urllib.request.urlopen(request, timeout=timeout, context=ssl_ctx) as response:
            payload = json.loads(response.read().decode("utf-8"))
        if not payload.get("success"):
            raise ValueError("runtime-config response is not successful")
        temporary = cache_path.with_suffix(".tmp")
        temporary.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
        temporary.replace(cache_path)
    except Exception:
        try:
            payload = json.loads(cache_path.read_text(encoding="utf-8-sig"))
        except Exception:
            payload = None

    remote = (payload or {}).get("config") or {}
    llm = remote.get("llm") or {}
    if llm.get("baseUrl") or llm.get("apiKey") or llm.get("model"):
        remote_llm_key = str(llm.get("apiKey") or "").strip()
        _last_remote_llm = {
            "runtime_llm_base_url": str(llm.get("baseUrl") or "").strip().rstrip("/"),
            "llm_provider": str(llm.get("provider") or "proxy").strip().lower(),
            "llm_model": str(llm.get("model") or config.get("llm_model") or "gpt-4o-mini").strip(),
        }
        if remote_llm_key:
            _last_remote_llm["llm_api_key"] = remote_llm_key
        config.update(_last_remote_llm)

    openai_llm = remote.get("openaiLlm") or {}
    if openai_llm.get("baseUrl") or openai_llm.get("apiKey") or openai_llm.get("model"):
        remote_openai_key = str(openai_llm.get("apiKey") or "").strip()
        update_dict = {
            "openai_llm_base_url": str(openai_llm.get("baseUrl") or "https://api.openai.com/v1/").strip(),
            "openai_llm_provider": str(openai_llm.get("provider") or "openai").strip().lower(),
            "openai_llm_model": str(openai_llm.get("model") or "gpt-4o-mini").strip(),
        }
        if remote_openai_key:
            update_dict["openai_api_key"] = remote_openai_key
        config.update(update_dict)

    _last_remote_refresh = time.monotonic()
    return config


def apply_runtime_environment(config, app_dir=None):
    base = Path(app_dir) if app_dir else application_dir()
    apply_path_environment()
    llm = config.get("llm") or {}
    base_url = config.get("runtime_llm_base_url") or os.environ.get("LLM_API_BASE_URL") or ""
    model = config.get("llm_model") or llm.get("model") or "gpt-4o-mini"
    api_key = config.get("llm_api_key") or llm.get("api_key") or ""
    provider = config.get("llm_provider") or llm.get("provider") or DEFAULT_CONFIG["llm_provider"]
    if base_url:
        os.environ["LLM_API_BASE_URL"] = str(base_url).rstrip("/")
        os.environ["OLLAMA_BASE_URL"] = str(base_url).rstrip("/")
    os.environ["LLM_PROVIDER"] = str(provider).lower()
    os.environ["OLLAMA_MODEL"] = str(model)
    os.environ["LLM_MODEL"] = str(model)
    os.environ["LLM_API_KEY"] = str(api_key or "")
    os.environ["OLLAMA_API_KEY"] = str(api_key or "ollama")

    openai_base_url = config.get("openai_llm_base_url") or os.environ.get("OPENAI_LLM_API_BASE_URL") or "https://api.openai.com/v1/"
    openai_model = config.get("openai_llm_model") or os.environ.get("OPENAI_LLM_MODEL") or "gpt-4o-mini"
    openai_api_key = config.get("openai_api_key") or os.environ.get("OPENAI_API_KEY") or ""
    openai_provider = config.get("openai_llm_provider") or os.environ.get("OPENAI_LLM_PROVIDER") or "openai"

    if openai_base_url:
        os.environ["OPENAI_LLM_API_BASE_URL"] = str(openai_base_url).rstrip("/") + "/"
    os.environ["OPENAI_LLM_MODEL"] = str(openai_model)
    if openai_api_key:
        os.environ["OPENAI_API_KEY"] = str(openai_api_key)
    os.environ["OPENAI_LLM_PROVIDER"] = str(openai_provider).lower()
    os.environ["HF_HOME"] = str(tts_cache_dir())
    os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
    model_snapshots = (tts_cache_dir() / "hub" /
                       "models--pnnbao-ump--VieNeu-TTS-v3-Turbo" / "snapshots")
    if model_snapshots.is_dir() and any(model_snapshots.iterdir()):
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["SLIVE_CONFIG_PATH"] = str(config_path())
    # Keep Numba enabled in the frozen build. Disabling JIT breaks some of
    # librosa's guvectorize functions at runtime. Give it a normal writable
    # cache outside PyInstaller's read-only bundle instead.
    if getattr(sys, "frozen", False):
        numba_cache = (base.parent if base.name.lower() == "app" else base) / "data" / "numba-cache"
        numba_cache.mkdir(parents=True, exist_ok=True)
        os.environ["NUMBA_CACHE_DIR"] = str(numba_cache)
        os.environ.pop("NUMBA_DISABLE_JIT", None)
