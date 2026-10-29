import cv2
import threading
import time
import numpy as np
from utils.logger import logger
from utils.camera_detector import get_candidate_camera_indices


class WebcamStream:
    """
    Quản lý luồng Camera thời gian thực trên background thread.
    Cơ chế non-blocking: Luôn lưu khung hình mới nhất vào bộ đệm để tránh giật lag.
    Tự động nhận diện và fallback sang camera khả dụng nếu camera chỉ định không tồn tại.
    """
    def __init__(self, camera_index=None, width=1280, height=720, fps=25):
        try:
            self.camera_index = int(camera_index) if camera_index is not None else None
        except (ValueError, TypeError):
            self.camera_index = None

        self.actual_camera_index = self.camera_index
        self.width = width
        self.height = height
        self.target_fps = fps
        self.cap = None
        self.running = False
        self.thread = None
        self.lock = threading.Lock()
        self.latest_frame = None
        self.last_read_time = 0
        import os
        from collections import deque
        env_delay_sec = os.environ.get("LIVEAI_BUFFER_DELAY_SEC")
        env_delay_frames = os.environ.get("LIVEAI_BUFFER_DELAY_FRAMES")
        if env_delay_frames is not None:
            self.buffer_delay_frames = max(10, int(env_delay_frames))
        elif env_delay_sec is not None:
            self.buffer_delay_frames = max(10, int(float(env_delay_sec) * self.target_fps))
        else:
            self.buffer_delay_frames = 45  # ~1.8s buffer trễ để tổng độ trễ toàn hệ thống đạt đúng mức ~5s
        self.buffer_queue = deque()
        self._cached_jpeg = None
        self._cached_jpeg_time = 0
        self._fallback_frame = None
        self.start()

    def _create_fallback_frame(self, message=None):
        frame = np.zeros((self.height, self.width, 3), dtype=np.uint8)
        cam_desc = f"ID {self.actual_camera_index}" if self.actual_camera_index is not None else "Auto"
        text = message or f"Live Camera ({cam_desc}) - Dang ket noi camera..."
        cv2.putText(frame, text, (40, self.height // 2),
                    cv2.FONT_HERSHEY_SIMPLEX, 0.75, (0, 255, 255), 2)
        return frame

    def start(self):
        if self.running:
            return
        self._fallback_frame = self._create_fallback_frame()
        self.running = True
        self.thread = threading.Thread(target=self._capture_loop, daemon=True, name="WebcamCaptureThread")
        self.thread.start()

    def _try_open_device(self, idx: int):
        for backend in (cv2.CAP_DSHOW, cv2.CAP_ANY):
            try:
                cap = cv2.VideoCapture(idx, backend)
                if cap.isOpened():
                    try:
                        cap.set(cv2.CAP_PROP_FOURCC, cv2.VideoWriter_fourcc(*'MJPG'))
                        cap.set(cv2.CAP_PROP_FRAME_WIDTH, self.width)
                        cap.set(cv2.CAP_PROP_FRAME_HEIGHT, self.height)
                        cap.set(cv2.CAP_PROP_FPS, self.target_fps)
                        cap.set(cv2.CAP_PROP_BUFFERSIZE, 1)
                    except Exception:
                        pass

                    ret, test_frame = cap.read()
                    if ret and test_frame is not None:
                        w = test_frame.shape[1]
                        h = test_frame.shape[0]
                        brightness = float(np.mean(test_frame))
                        if self.camera_index is not None and self.camera_index != idx:
                            logger.info(f"Camera ID {self.camera_index} khong the mo tren may. Tu dong chuyen sang Camera ID {idx} (backend={backend}, res={w}x{h}, do sang={brightness:.1f})")
                        else:
                            logger.info(f"Camera ID {idx} da mo thanh cong (backend={backend}, res={w}x{h}, do sang={brightness:.1f})")
                        self.actual_camera_index = idx
                        with self.lock:
                            self.latest_frame = test_frame
                            self.last_read_time = time.time()
                        return cap
                    else:
                        cap.release()
            except Exception as e:
                logger.debug(f"Loi thu mo Camera ID {idx} backend {backend}: {e}")
        return None

    def _open_camera(self):
        # 1. Thử ngay camera được yêu cầu nếu có
        if self.camera_index is not None:
            cap = self._try_open_device(self.camera_index)
            if cap is not None:
                return cap

        # 2. Nếu không có hoặc mở thất bại, tự động tìm và fallback sang các camera khả dụng
        candidates = get_candidate_camera_indices(self.camera_index)
        for idx in candidates:
            if idx == self.camera_index:
                continue  # Đã thử ở trên
            cap = self._try_open_device(idx)
            if cap is not None:
                return cap

        logger.warning("Khong tim thay hoac khong the mo bat ky Camera nao tren may. Vui long cam Webcam.")
        self._fallback_frame = self._create_fallback_frame("Khong tim thay Webcam. Vui long kiem tra ket noi Camera.")
        return None

    def _capture_loop(self):
        target_info = f"ID {self.camera_index}" if self.camera_index is not None else "Auto-detect"
        logger.info(f"Bat dau khoi dong Camera ({target_info})...")
        self.cap = self._open_camera()

        while self.running:
            if self.cap is not None and self.cap.isOpened():
                ret, frame = self.cap.read()
                if ret and frame is not None:
                    with self.lock:
                        self.latest_frame = frame
                        self.last_read_time = time.time()
                        self.buffer_queue.append(frame.copy())
                        # Giữ hàng đợi đúng chuẩn theo độ trễ mục tiêu (tránh tích lũy trễ khi webcam chạy 30fps)
                        max_queue_len = self.buffer_delay_frames + 3
                        while len(self.buffer_queue) > max_queue_len:
                            self.buffer_queue.popleft()
                else:
                    time.sleep(0.01)
            else:
                time.sleep(2.0)
                if self.running and (self.cap is None or not self.cap.isOpened()):
                    self.cap = self._open_camera()

    def get_frame(self):
        with self.lock:
            if self.latest_frame is not None:
                return self.latest_frame.copy()
        return self._fallback_frame.copy()

    def get_buffered_frame(self):
        """Lấy 1 frame từ hàng đợi trễ (FIFO) để đảm bảo tổng độ trễ toàn hệ thống đạt ~5s chuẩn xác"""
        with self.lock:
            if not self.buffer_queue:
                if self.latest_frame is not None:
                    return self.latest_frame.copy()
                return self._fallback_frame.copy()
            if len(self.buffer_queue) <= self.buffer_delay_frames:
                return self.buffer_queue[0].copy()
            return self.buffer_queue.popleft()

    def get_buffered_frames(self, count: int) -> list:
        """Lấy danh sách N frames liên tiếp từ hàng đợi trễ cho batch suy luận"""
        frames = []
        with self.lock:
            for _ in range(count):
                if not self.buffer_queue:
                    fallback = self.latest_frame if self.latest_frame is not None else self._fallback_frame
                    frames.append(fallback.copy())
                elif len(self.buffer_queue) <= self.buffer_delay_frames:
                    frames.append(self.buffer_queue[0].copy())
                else:
                    frames.append(self.buffer_queue.popleft())
        return frames

    def set_buffer_delay_seconds(self, seconds: float):
        """Thay đổi độ trễ webcam linh hoạt theo giây (ví dụ: 1.5s, 2.0s)"""
        with self.lock:
            self.buffer_delay_frames = max(10, int(seconds * self.target_fps))
            logger.info(f"Cap nhat do tre webcam: {seconds:.1f}s ({self.buffer_delay_frames} frames)")

    def get_jpeg_frame(self, quality: int = 75) -> bytes:
        with self.lock:
            if self.latest_frame is not None:
                if self._cached_jpeg is not None and self._cached_jpeg_time == self.last_read_time:
                    return self._cached_jpeg
                frame_to_encode = self.latest_frame.copy()
            else:
                frame_to_encode = self._fallback_frame.copy()

        h, w = frame_to_encode.shape[:2]
        if w > 960:
            scale = 960.0 / w
            frame_to_encode = cv2.resize(frame_to_encode, (960, int(h * scale)), interpolation=cv2.INTER_LINEAR)

        ret, buffer = cv2.imencode('.jpg', frame_to_encode, [cv2.IMWRITE_JPEG_QUALITY, quality])
        if ret:
            jpeg_bytes = buffer.tobytes()
            with self.lock:
                self._cached_jpeg = jpeg_bytes
                self._cached_jpeg_time = self.last_read_time
            return jpeg_bytes
        return b''

    def stop(self):
        self.running = False
        if self.thread and self.thread.is_alive():
            self.thread.join(timeout=1.0)
        if self.cap is not None:
            try:
                self.cap.release()
            except Exception:
                pass
            self.cap = None
        with self.lock:
            self.latest_frame = None
            self.buffer_queue.clear()
            self._cached_jpeg = None
        logger.info("Da dung luong Camera.")


# Quản lý preview camera khi chưa bắt đầu phiên Live
_preview_webcam = None
_preview_lock = threading.Lock()


def get_or_create_preview_webcam(camera_index=None) -> WebcamStream:
    global _preview_webcam
    with _preview_lock:
        if _preview_webcam is None or not _preview_webcam.running:
            _preview_webcam = WebcamStream(camera_index=camera_index)
        elif camera_index is not None and _preview_webcam.actual_camera_index != camera_index:
            try:
                _preview_webcam.stop()
            except Exception:
                pass
            _preview_webcam = WebcamStream(camera_index=camera_index)
        return _preview_webcam


def release_preview_webcam():
    global _preview_webcam
    with _preview_lock:
        if _preview_webcam is not None:
            try:
                _preview_webcam.stop()
            except Exception:
                pass
            _preview_webcam = None


def get_active_or_preview_webcam(camera_index=None) -> WebcamStream:
    """Lấy webcam từ session đang chạy nếu có, hoặc tạo preview webcam"""
    try:
        from server.session_manager import session_manager
        session = session_manager.get_latest_session()
        if session is not None and getattr(session, 'is_webcam', False) and getattr(session, 'webcam', None) is not None:
            return session.webcam
    except Exception:
        pass
    return get_or_create_preview_webcam(camera_index)

