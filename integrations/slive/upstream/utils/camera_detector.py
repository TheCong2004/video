import os
import subprocess
import re
import cv2
import time
from utils.logger import logger

_cached_cameras = None
_last_scan_time = 0
CACHE_TTL = 20.0  # cache 20 seconds

VIRTUAL_KEYWORDS = [
    'obs', 'vmix', 'ndi', 'virtual', 'bytecast', 'screen', 
    'splitcam', 'ivcam', 'droidcam', 'manycam', 'snap camera'
]

HARDWARE_KEYWORDS = [
    'webcam', 'logi', 'c270', 'c920', 'c922', 'brio', 'integrated', 
    'built-in', 'usb', 'camera', 'facetime', 'truevision', 'chicony', 
    'realtek', 'sunplus', 'hd pro', 'front', 'rear', 'lenovo', 'dell'
]


def score_camera(name: str) -> int:
    """Chấm điểm ưu tiên camera: Camera vật lý/thực tế ưu tiên cao nhất, camera ảo ưu tiên thấp."""
    n_lower = name.lower()
    score = 0
    if any(k in n_lower for k in HARDWARE_KEYWORDS):
        score += 10
    if any(k in n_lower for k in VIRTUAL_KEYWORDS):
        score -= 20
    return score


def get_available_cameras(force_refresh: bool = False):
    """
    Quét và trả về danh sách các camera đang có trên máy tính,
    tự động sắp xếp ưu tiên camera vật lý thực tế lên đầu.
    """
    global _cached_cameras, _last_scan_time
    now = time.time()
    if not force_refresh and _cached_cameras is not None and (now - _last_scan_time < CACHE_TTL):
        return list(_cached_cameras)

    dshow_names = []
    # 1. Thử lấy tên thiết bị từ DirectShow qua ffmpeg (cực nhanh trên Windows ~0.2s)
    try:
        cmd = ['ffmpeg', '-list_devices', 'true', '-f', 'dshow', '-i', 'dummy']
        creation_flags = 0x08000000 if hasattr(subprocess, 'CREATE_NO_WINDOW') or os.name == 'nt' else 0
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=2.5, creationflags=creation_flags)
        for line in p.stderr.splitlines():
            if '(video)' in line:
                m = re.search(r'"([^"]+)"\s+\(video\)', line)
                if m:
                    dshow_names.append(m.group(1))
    except Exception:
        pass

    cameras = []
    if dshow_names:
        for idx, name in enumerate(dshow_names):
            cameras.append({
                "id": "webcam",
                "index": idx,
                "name": name,
                "score": score_camera(name),
                "label": f"Webcam {idx}: {name}"
            })
        # Sắp xếp ưu tiên: Camera vật lý lên đầu
        cameras.sort(key=lambda x: x["score"], reverse=True)
    else:
        # Fallback dò bằng OpenCV nếu không có ffmpeg
        consecutive_fails = 0
        for idx in range(6):
            try:
                cap = cv2.VideoCapture(idx, cv2.CAP_DSHOW)
                if not cap.isOpened():
                    cap = cv2.VideoCapture(idx)
                if cap.isOpened():
                    cap.release()
                    consecutive_fails = 0
                    cameras.append({
                        "id": "webcam",
                        "index": idx,
                        "name": f"Camera {idx}",
                        "score": 0,
                        "label": f"Webcam {idx}"
                    })
                else:
                    consecutive_fails += 1
                    if consecutive_fails >= 2 and idx >= 2:
                        break
            except Exception:
                consecutive_fails += 1

    if not cameras:
        cameras.append({
            "id": "webcam",
            "index": 0,
            "name": "Camera mặc định",
            "score": 0,
            "label": "Webcam 0 (Mặc định)"
        })

    _cached_cameras = cameras
    _last_scan_time = now
    logger.info(f"Da nhan dien {len(cameras)} camera tren he thong (Camera uu tien cao nhat: {cameras[0]['name']}).")
    return list(cameras)


def get_candidate_camera_indices(preferred_idx=None):
    """
    Trả về danh sách index camera theo thứ tự ưu tiên tối ưu nhất:
    1. preferred_idx (nếu người dùng chỉ định rõ trong cấu hình)
    2. Camera vật lý thực tế được phát hiện (ví dụ: Logi, Integrated Camera)
    3. Các camera còn lại
    4. Index cơ bản 0, 1, 2, 3
    """
    candidates = []
    if preferred_idx is not None:
        try:
            candidates.append(int(preferred_idx))
        except (ValueError, TypeError):
            pass

    try:
        cams = get_available_cameras()
        for c in cams:
            if c["index"] not in candidates:
                candidates.append(c["index"])
    except Exception:
        pass

    for idx in (0, 1, 2, 3):
        if idx not in candidates:
            candidates.append(idx)

    return candidates
