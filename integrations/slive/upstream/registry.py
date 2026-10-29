###############################################################################
#  Bảng đăng ký plugin — đăng ký bằng decorator, tạo thể hiện theo tên
###############################################################################

from typing import Dict, Type, Any
from utils.logger import logger

_REGISTRY: Dict[str, Dict[str, Type]] = {
    "stt": {},
    "llm": {},
    "tts": {},
    "avatar": {},
    "output": {},
}


def register(category: str, name: str):
    """
    Decorator: đăng ký lớp plugin vào bảng đăng ký toàn cục.

    Cách dùng::

        @register("tts", "edgetts")
        class EdgeTTS(BaseTTS): ...
    """
    def decorator(cls):
        if category not in _REGISTRY:
            _REGISTRY[category] = {}
        _REGISTRY[category][name] = cls
        logger.info(f"Registered {category}/{name}: {cls.__name__}")
        return cls
    return decorator


def create(category: str, name: str, **kwargs) -> Any:
    """
    Tạo thể hiện plugin theo tên.

    Usage::

        tts = registry.create("tts", "edgetts", opt=opt)
    """
    if category not in _REGISTRY or name not in _REGISTRY[category]:
        available = list(_REGISTRY.get(category, {}).keys())
        raise ValueError(
            f"Plugin '{name}' not found in category '{category}'. "
            f"Available: {available}"
        )
    cls = _REGISTRY[category][name]
    return cls(**kwargs)


def list_plugins(category: str = None) -> Dict[str, list]:
    """Liệt kê các plugin đã đăng ký"""
    if category:
        return {category: list(_REGISTRY.get(category, {}).keys())}
    return {cat: list(plugins.keys()) for cat, plugins in _REGISTRY.items()}
