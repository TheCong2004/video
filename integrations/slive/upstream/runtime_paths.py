"""Canonical paths for program code, AI resources and mutable user data."""
from __future__ import annotations

import os
import shutil
import sys
from pathlib import Path


def program_dir() -> Path:
    if getattr(sys, "frozen", False):
        return Path(sys.executable).resolve().parent
    return Path(__file__).resolve().parent


def user_root() -> Path:
    p = program_dir()
    if p.name.lower() == "app":
        return p.parent
    return p


def installed_program_dir() -> Path:
    return user_root()


def resources_dir() -> Path:
    return user_root() / "resources"


def data_dir() -> Path:
    return user_root() / "data"


def logs_dir() -> Path:
    return user_root() / "logs"


def updates_dir() -> Path:
    return user_root() / "updates"


def updater_dir() -> Path:
    return user_root() / "updater"


def config_path() -> Path:
    return data_dir() / "config.json" if getattr(sys, "frozen", False) else program_dir() / "config.json"


def tiktok_extension_dir() -> Path:
    candidates = [
        program_dir() / "tiktok_pin_extension",
        program_dir() / "_internal" / "tiktok_pin_extension",
        Path.cwd() / "tiktok_pin_extension",
        Path(__file__).resolve().parent / "tiktok_pin_extension",
    ]
    for c in candidates:
        if c.is_dir():
            return c
    return program_dir() / "tiktok_pin_extension"


def user_avatars_dir() -> Path:
    return data_dir() / "avatars"


def default_avatars_dir() -> Path:
    candidate = resources_dir() / "default-avatars"
    if candidate.exists():
        return candidate
    dev_candidate = program_dir() / "data" / "avatars"
    if dev_candidate.exists():
        return dev_candidate
    return candidate


def avatar_path(avatar_id: str, for_write: bool = False) -> Path:
    user_path = user_avatars_dir() / str(avatar_id)
    default_path = default_avatars_dir() / str(avatar_id)
    dev_avatar_path = program_dir() / "avatars" / str(avatar_id)
    app_avatar_path = program_dir() / "data" / "avatars" / str(avatar_id)
    if for_write:
        return user_path
    if user_path.exists():
        return user_path
    if default_path.exists():
        return default_path
    if app_avatar_path.exists():
        return app_avatar_path
    if dev_avatar_path.exists():
        return dev_avatar_path
    return user_path


def model_path() -> Path:
    for name in ("wav2lip384.pth", "wav2lip.pth", "wav2lip256.pth"):
        for base in (resources_dir() / "models", program_dir() / "models"):
            candidate = base / name
            if candidate.is_file():
                return candidate
    return resources_dir() / "models" / "wav2lip384.pth"


def tts_cache_dir() -> Path:
    for base in (resources_dir() / "tts_model_cache", program_dir() / "tts_model_cache"):
        if (base / "hub").is_dir():
            return base
    if (resources_dir() / "tts_model_cache").is_dir():
        return resources_dir() / "tts_model_cache"
    return program_dir() / "tts_model_cache"


def face_detector_path() -> Path:
    candidates = [
        program_dir() / "avatars" / "wav2lip" / "face_detection" / "detection" / "sfd" / "s3fd.pth",
        resources_dir() / "face-detection" / "s3fd.pth",
        program_dir() / "face-detection" / "s3fd.pth",
        Path.home() / ".cache" / "torch" / "hub" / "checkpoints" / "s3fd-619a316812.pth",
    ]
    for c in candidates:
        if c.is_file():
            return c
    return resources_dir() / "face-detection" / "s3fd.pth"


def required_resource_path(install_root: Path, relative: str) -> Path:
    relative_path = Path(relative)
    if not getattr(sys, "frozen", False):
        if relative_path.parts[:1] == ("default-avatars",):
            dev_avatar = program_dir() / "data" / "avatars" / Path(*relative_path.parts[1:])
            if dev_avatar.exists():
                return dev_avatar
        if relative_path.as_posix() == "face-detection/s3fd.pth":
            dev_face = face_detector_path()
            if dev_face.exists():
                return dev_face
        dev_local = program_dir() / relative_path
        if dev_local.exists():
            return dev_local
    return Path(install_root) / relative_path


def installed_resources_path() -> Path:
    if not getattr(sys, "frozen", False):
        return user_root() / "resources-dev" / "installed-resources.json"
    return resources_dir() / "installed-resources.json"


def ensure_runtime_dirs() -> None:
    for path in (
        resources_dir(), data_dir(), user_avatars_dir(), logs_dir(),
        updates_dir(), updater_dir(),
    ):
        path.mkdir(parents=True, exist_ok=True)


def _move_if_missing(source: Path, destination: Path) -> None:
    if destination.exists() or not source.exists():
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    try:
        shutil.move(str(source), str(destination))
    except Exception:
        if source.is_dir():
            shutil.copytree(source, destination, dirs_exist_ok=True)
        else:
            shutil.copy2(source, destination)


def migrate_legacy_layout(default_avatar_id: str = "mau10") -> None:
    """Adopt resources installed by the legacy 4.6 GB all-in-one package."""
    if not getattr(sys, "frozen", False):
        return
    ensure_runtime_dirs()
    legacy = user_root() / "app"
    if not legacy.exists() or legacy.resolve() == program_dir().resolve():
        return
    _move_if_missing(legacy / "models" / "wav2lip.pth", model_path())
    _move_if_missing(legacy / "tts_model_cache", tts_cache_dir())
    _move_if_missing(
        legacy / "_internal" / "avatars" / "wav2lip" / "face_detection"
        / "detection" / "sfd" / "s3fd.pth",
        face_detector_path(),
    )
    legacy_avatars = legacy / "data" / "avatars"
    if legacy_avatars.is_dir():
        for item in legacy_avatars.iterdir():
            target_root = default_avatars_dir() if item.name == default_avatar_id else user_avatars_dir()
            _move_if_missing(item, target_root / item.name)
    _move_if_missing(legacy / "config.json", config_path())
    _move_if_missing(user_root() / "auth.json", data_dir() / "auth.json")


def apply_path_environment() -> None:
    ensure_runtime_dirs()
    os.environ["SLIVE_PROGRAM_DIR"] = str(program_dir())
    os.environ["SLIVE_RESOURCE_DIR"] = str(resources_dir())
    os.environ["SLIVE_DATA_DIR"] = str(data_dir())
    os.environ["SLIVE_AVATAR_DIR"] = str(user_avatars_dir())
    os.environ["SLIVE_DEFAULT_AVATAR_DIR"] = str(default_avatars_dir())
    os.environ["SLIVE_MODEL_PATH"] = str(model_path())
    os.environ["SLIVE_TTS_CACHE_DIR"] = str(tts_cache_dir())
    os.environ["SLIVE_S3FD_PATH"] = str(face_detector_path())
    os.environ["SLIVE_TIKTOK_EXTENSION_DIR"] = str(tiktok_extension_dir())

