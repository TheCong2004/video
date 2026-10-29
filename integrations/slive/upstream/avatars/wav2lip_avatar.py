###############################################################################
#  Copyright (C) 2024 LiveTalking@lipku https://github.com/lipku/LiveTalking
#  email: lipku@foxmail.com
# 
#  Licensed under the Apache License, Version 2.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at
#  
#       http://www.apache.org/licenses/LICENSE-2.0
# 
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
###############################################################################
#
#  Avatar Wav2Lip - duoc chuyen tu lipreal.py + lipasr.py
#

import math
import torch
if torch.cuda.is_available():
    torch.backends.cudnn.benchmark = True
import numpy as np

import os
import time
import cv2
import glob
import pickle
import copy
import re

import queue
from queue import Queue
from threading import Thread, Event
import torch.multiprocessing as mp

from avatars.audio_features.mel import MelASR
import asyncio
from av import AudioFrame, VideoFrame
from avatars.wav2lip.models import Wav2Lip, Wav2Lip384, Wav2Lip256
from avatars.base_avatar import BaseAvatar

from tqdm import tqdm
from utils.logger import logger
from utils.image import read_imgs, mirror_index
from utils.device import initialize_device
from registry import register
from runtime_paths import avatar_path as resolve_avatar_path

device = initialize_device()
logger.info('Using {} for inference.'.format(device))

def detect_wav2lip_variant(state_dict):
    keys = set(state_dict.keys())
    if "sam.sa.conv1.weight" in keys or any(key.startswith("audio_refine.") for key in keys):
        return "wav2lip384", 384
    if any(key.startswith("face_encoder_blocks.7.") for key in keys):
        return "wav2lip256", 256
    return "wav2lip96", 96

def subpixel_peak(res: np.ndarray, max_loc: tuple) -> tuple:
    """Nội suy đỉnh cực đại parabol cận pixel (sub-pixel peak interpolation)."""
    x, y = max_loc
    h, w = res.shape
    if 1 <= x < w - 1 and 1 <= y < h - 1:
        denom_x = 2.0 * res[y, x] - res[y, x - 1] - res[y, x + 1]
        dx = (res[y, x + 1] - res[y, x - 1]) / (2.0 * denom_x + 1e-6) if abs(denom_x) > 1e-6 else 0.0
        denom_y = 2.0 * res[y, x] - res[y - 1, x] - res[y + 1, x]
        dy = (res[y + 1, x] - res[y - 1, x]) / (2.0 * denom_y + 1e-6) if abs(denom_y) > 1e-6 else 0.0
        return float(x + np.clip(dx, -0.5, 0.5)), float(y + np.clip(dy, -0.5, 0.5))
    return float(x), float(y)

def _load(checkpoint_path):
    if device == 'cuda':
        checkpoint = torch.load(checkpoint_path)
    else:
        checkpoint = torch.load(checkpoint_path,
                                map_location=lambda storage, loc: storage)
    return checkpoint

def load_model(path):
    logger.info("Load checkpoint from: {}".format(path))
    checkpoint = _load(path)
    s = checkpoint.get("state_dict", checkpoint)
    new_s = {}
    for k, v in s.items():
        new_s[k.replace('module.', '')] = v

    variant, input_size = detect_wav2lip_variant(new_s)
    logger.info(f"Detected Wav2Lip variant: {variant} (input_size={input_size})")

    if variant == "wav2lip384":
        model = Wav2Lip384()
    elif variant == "wav2lip256":
        model = Wav2Lip256()
    else:
        model = Wav2Lip()

    model.load_state_dict(new_s)
    model.img_size = input_size
    model = model.to(device)
    return model.eval()

def load_avatar(avatar_id):
    if str(avatar_id).lower() in ("webcam", "camera", "livecam") or str(avatar_id).lower().startswith("webcam"):
        logger.info(f"Loaded live webcam avatar mode for '{avatar_id}'")
        placeholder = np.zeros((720, 1280, 3), dtype=np.uint8)
        return [placeholder], [None], [None]

    avatar_path = str(resolve_avatar_path(avatar_id))
    full_imgs_path = f"{avatar_path}/full_imgs"
    face_imgs_path = f"{avatar_path}/face_imgs"
    coords_path = f"{avatar_path}/coords.pkl"
    
    # 1. First, try to load preprocessed files from disk (instant startup)
    if os.path.exists(coords_path) and os.path.exists(full_imgs_path) and os.path.exists(face_imgs_path):
        logger.info(f"Loading preprocessed wav2lip data from disk for {avatar_id} (Instant startup)...")
        with open(coords_path, 'rb') as f:
            coord_list_cycle = pickle.load(f)
        input_img_list = glob.glob(os.path.join(full_imgs_path, '*.[jpJP][pnPN]*[gG]'))
        input_img_list = sorted(input_img_list, key=lambda x: int(os.path.splitext(os.path.basename(x))[0]))
        frame_list_cycle = read_imgs(input_img_list)
        input_face_list = glob.glob(os.path.join(face_imgs_path, '*.[jpJP][pnPN]*[gG]'))
        input_face_list = sorted(input_face_list, key=lambda x: int(os.path.splitext(os.path.basename(x))[0]))
        face_list_cycle = read_imgs(input_face_list)
        return frame_list_cycle, face_list_cycle, coord_list_cycle
        
    # 2. If preprocessed files are missing, check for a raw video file to process
    video_file = None
    if os.path.exists(avatar_path):
        mp4_files = glob.glob(os.path.join(avatar_path, "*.mp4"))
        if mp4_files:
            video_file = mp4_files[0]
            
    if not video_file:
        if os.path.isfile(avatar_id):
            video_file = avatar_id
        elif os.path.isfile(f"{avatar_id}.mp4"):
            video_file = f"{avatar_id}.mp4"
            
    if not video_file:
        root_mp4_files = glob.glob(f"./{avatar_id}*.mp4")
        if root_mp4_files:
            video_file = root_mp4_files[0]

    # If video file is found, process dynamically in the main thread and save to disk
    if video_file:
        logger.info(f"Preprocessed data not found. Video file found at {video_file}. Reading frames into memory instantly...")
        cap = cv2.VideoCapture(video_file)
        if not cap.isOpened():
            raise IOError(f"Could not open video file: {video_file}")
            
        frame_list_cycle = []
        while True:
            ret, frame = cap.read()
            if not ret:
                break
            cv2.putText(frame, "LiveTalking", (10, 20), cv2.FONT_HERSHEY_SIMPLEX, 0.3, (128, 128, 128), 1)
            frame_list_cycle.append(frame)
        cap.release()
        
        if not frame_list_cycle:
            raise ValueError(f"No frames extracted from video file: {video_file}")
            
        face_list_cycle = [None] * len(frame_list_cycle)
        coord_list_cycle = [None] * len(frame_list_cycle)
        return frame_list_cycle, face_list_cycle, coord_list_cycle
    else:
        raise FileNotFoundError(f"Could not load avatar '{avatar_id}'. No video file or preprocessed data found.")

@torch.no_grad()
def warm_up(batch_size,model,modelres):
    # Ham lam nong
    img_size = getattr(model, "img_size", 384)
    logger.info(f'warmup model with resolution {img_size}x{img_size}...')
    img_batch = torch.ones(batch_size, 6, img_size, img_size).to(device)
    mel_batch = torch.ones(batch_size, 1, 80, 16).to(device)
    model(mel_batch, img_batch)

@register("avatar", "wav2lip")
class LipReal(BaseAvatar):
    @torch.no_grad()
    def __init__(self, opt, model, avatar):
        super().__init__(opt)
        self.model = model
        self.target_size = getattr(self.model, "img_size", 384)
        self.frame_list_cycle, self.face_list_cycle, self.coord_list_cycle = avatar
        self.detector = None # Initialize lazily
        self.last_bbox = None
        self.webcam_frame_data = {}
        self.current_webcam_meta = None
        self.last_tracked_pos = None
        self.webcam_color_gain = None
        self.frame_counter = 0
        self._mask_cache = {}

        # Tối ưu hoá: Tiền chuẩn hoá kích thước toàn bộ face_list_cycle về target_size ngay khi khởi tạo
        # giúp triệt tiêu hoàn toàn chi phí cv2.resize trên từng frame trong vòng lặp suy luận thời gian thực
        if self.face_list_cycle:
            for idx in range(len(self.face_list_cycle)):
                f = self.face_list_cycle[idx]
                if f is not None and (f.shape[0] != self.target_size or f.shape[1] != self.target_size):
                    self.face_list_cycle[idx] = cv2.resize(f, (self.target_size, self.target_size), interpolation=cv2.INTER_CUBIC)

        avatar_id_str = str(getattr(opt, 'avatar_id', '')).lower()
        self.is_webcam = avatar_id_str in ("webcam", "camera", "livecam") or avatar_id_str.startswith("webcam")
        if self.is_webcam:
            from avatars.webcam_stream import WebcamStream, release_preview_webcam
            release_preview_webcam()
            cam_idx = getattr(opt, 'camera_index', None)
            m = re.search(r'webcam[_\-:]?(\d+)', avatar_id_str)
            if m:
                cam_idx = int(m.group(1))
            self.webcam = WebcamStream(camera_index=cam_idx)
            self.cached_webcam_data = {}
            target_desc = f"ID {cam_idx}" if cam_idx is not None else "Auto-detect"
            logger.info(f"LipReal khoi tao che do Live Webcam ({target_desc})")

        self.asr = MelASR(opt, self)
        self.asr.warm_up()

    def switch_avatar(self, avatar_id: str) -> bool:
        """Chuyển đổi avatar trực tiếp (hot-swap) trong thời gian thực khi đang Live."""
        try:
            logger.info(f"LipReal dang chuyen doi sang avatar: {avatar_id}")
            avatar_data = load_avatar(avatar_id)
            frame_list_cycle, face_list_cycle, coord_list_cycle = avatar_data
            if face_list_cycle:
                for idx in range(len(face_list_cycle)):
                    f = face_list_cycle[idx]
                    if f is not None and (f.shape[0] != self.target_size or f.shape[1] != self.target_size):
                        face_list_cycle[idx] = cv2.resize(f, (self.target_size, self.target_size), interpolation=cv2.INTER_CUBIC)

            avatar_id_str = str(avatar_id).lower()
            is_webcam = avatar_id_str in ("webcam", "camera", "livecam") or avatar_id_str.startswith("webcam")
            if is_webcam:
                from avatars.webcam_stream import WebcamStream
                cam_idx = getattr(self.opt, 'camera_index', None)
                m = re.search(r'webcam[_\-:]?(\d+)', avatar_id_str)
                if m:
                    cam_idx = int(m.group(1))

                need_new_stream = (
                    not getattr(self, 'is_webcam', False)
                    or not hasattr(self, 'webcam')
                    or self.webcam is None
                    or (cam_idx is not None and getattr(self.webcam, 'actual_camera_index', None) != cam_idx)
                )
                if need_new_stream:
                    if hasattr(self, 'webcam') and self.webcam:
                        try:
                            self.webcam.stop()
                        except Exception:
                            pass
                    self.webcam = WebcamStream(camera_index=cam_idx)
                    self.cached_webcam_data = {}
            elif not is_webcam and getattr(self, 'is_webcam', False):
                if hasattr(self, 'webcam') and self.webcam:
                    try:
                        self.webcam.stop()
                    except Exception:
                        pass
                self.webcam = None
                self.cached_webcam_data = {}

            self.is_webcam = is_webcam
            self.frame_list_cycle = frame_list_cycle
            self.face_list_cycle = face_list_cycle
            self.coord_list_cycle = coord_list_cycle
            self.last_bbox = None
            self.webcam_frame_data = {}
            self.current_webcam_meta = None
            self.last_tracked_pos = None
            self.webcam_color_gain = None
            self.frame_counter = 0
            self.opt.avatar_id = avatar_id
            logger.info(f"LipReal da chuyen thanh cong sang avatar: {avatar_id}")
            return True
        except Exception as e:
            logger.exception(f"Loi khi switch_avatar sang {avatar_id}: {e}")
            return False

    def on_webcam_silence_frame(self, frame_key: int):
        """Lưu trữ frame từ buffer webcam khi im lặng để giữ luồng video 3s trơn tru liên tục"""
        if self.is_webcam and hasattr(self, 'webcam') and self.webcam is not None:
            frame = self.webcam.get_buffered_frame()
            self.webcam_frame_data[frame_key] = (frame, None)

    def get_webcam_silent_frame(self, frame_key: int):
        """Lấy frame webcam đã đồng bộ thứ tự khi ở trạng thái im lặng"""
        meta = self.webcam_frame_data.pop(frame_key, None)
        if meta is not None and meta[0] is not None:
            return meta[0]
        if hasattr(self, 'webcam') and self.webcam is not None:
            return self.webcam.get_buffered_frame()
        return np.zeros((720, 1280, 3), dtype=np.uint8)

    def _create_face_blend_mask(self, height: int, width: int) -> np.ndarray:
        """
        Tạo mặt nạ hòa trộn biên mượt (Soft Feathered Mask) bảo toàn độ tự nhiên 100% cho khẩu hình:
        - Giữ nguyên 100% nửa trên khuôn mặt gốc (mắt, trán, sống mũi).
        - Vùng chuyển tiếp mượt mà ở khoảng 40% - 48% chiều cao mặt (vùng nhân trung trên môi).
        - Nửa dưới (toàn bộ môi trên, môi dưới, răng, vòm miệng, cằm nhép theo âm thanh)
          đạt trọng số 1.0 trọn vẹn để cử động khẩu hình rõ ràng, tự nhiên và sắc nét.
        - Các viền ngoài cùng (má, đáy cằm) được hòa tan nhẹ nhàng vào da thật.
        """
        y_indices = np.arange(height, dtype=np.float32)
        y_mask = np.zeros(height, dtype=np.float32)

        # Chuyển tiếp mượt từ sống mũi/nhân trung xuống môi trên
        y_top_start = int(0.40 * height)
        y_top_end = int(0.48 * height)
        y_bot_start = int(0.90 * height)
        y_bot_end = height

        if y_top_end > y_top_start:
            y_mask[y_top_start:y_top_end] = 0.5 - 0.5 * np.cos(
                np.pi * (y_indices[y_top_start:y_top_end] - y_top_start) / (y_top_end - y_top_start)
            )
        # Toàn bộ vùng miệng nhép (môi trên, môi dưới, răng, cằm) đạt trọng số 1.0 hoàn hảo
        y_mask[y_top_end:y_bot_start] = 1.0

        # Làm mờ dần ở phần tiếp giáp cổ dưới cằm
        if y_bot_end > y_bot_start:
            y_mask[y_bot_start:y_bot_end] = 0.5 + 0.5 * np.cos(
                np.pi * (y_indices[y_bot_start:y_bot_end] - y_bot_start) / (y_bot_end - y_bot_start)
            )

        # Trục X (2 bên má): Bảo toàn trọn vẹn bề ngang khóe miệng khi cười/mở miệng rộng
        x_indices = np.arange(width, dtype=np.float32)
        x_mask = np.ones(width, dtype=np.float32)
        x_margin = max(1, int(0.10 * width))

        x_mask[:x_margin] = 0.5 - 0.5 * np.cos(
            np.pi * x_indices[:x_margin] / x_margin
        )
        x_mask[-x_margin:] = 0.5 + 0.5 * np.cos(
            np.pi * (x_indices[-x_margin:] - (width - x_margin)) / x_margin
        )

        # Kết hợp 2D
        mask_2d = np.outer(y_mask, x_mask)

        # Gaussian blur nhẹ để làm mềm đường biên mà không làm mờ chi tiết môi
        ksize = int(max(width, height) * 0.05) // 2 * 2 + 1
        ksize = max(5, min(ksize, 21))
        mask_2d = cv2.GaussianBlur(mask_2d, (ksize, ksize), 0)

        return np.clip(mask_2d[:, :, np.newaxis], 0.0, 1.0)

    def _create_webcam_blend_mask(self, height: int, width: int) -> np.ndarray:
        """
        Mặt nạ hữu cơ siêu elip (Super-ellipse / Squircle) chuyên biệt cho Live Webcam:
        - Bo tròn tự nhiên 100% theo giải phẫu vòm miệng và cằm, triệt tiêu hoàn toàn mọi góc cạnh viền hộp.
        - Giữ nguyên 100% da camera thật ở hai bên má (x < 0.18w hoặc x > 0.82w), sống mũi (y < 0.52h), quai hàm và cổ (y > 0.93h).
        - Vùng khẩu hình (môi trên, môi dưới, răng, vòm miệng, khóe miệng rộng) đạt trọng số 1.0 trọn vẹn.
        - Vùng chuyển tiếp cosine siêu êm kết hợp Gaussian blur mềm (~7% kích thước mặt).
        """
        y_idx, x_idx = np.ogrid[:height, :width]
        cy = 0.72 * height
        cx = 0.50 * width
        ry = 0.20 * height
        rx = 0.32 * width

        # Khoảng cách siêu elip bậc 3.5 bo tròn tự nhiên theo vòm miệng và cằm
        dist = (((np.abs(x_idx - cx)) / rx) ** 3.5 + ((np.abs(y_idx - cy)) / ry) ** 3.5) ** (1.0 / 3.5)
        mask = np.zeros((height, width), dtype=np.float32)
        inner = dist <= 0.65
        blend = (dist > 0.65) & (dist < 1.0)
        mask[inner] = 1.0
        mask[blend] = 0.5 + 0.5 * np.cos(np.pi * (dist[blend] - 0.65) / 0.35)

        ksize = int(max(width, height) * 0.07) // 2 * 2 + 1
        mask = cv2.GaussianBlur(mask, (ksize, ksize), 0)
        return np.clip(mask[:, :, np.newaxis], 0.0, 1.0)

    def get_blend_mask(self, h: int, w: int, is_webcam: bool = False) -> np.ndarray:
        key = (h, w, is_webcam)
        if key not in self._mask_cache:
            if len(self._mask_cache) > 60:
                self._mask_cache.clear()
            if is_webcam:
                self._mask_cache[key] = self._create_webcam_blend_mask(h, w)
            else:
                self._mask_cache[key] = self._create_face_blend_mask(h, w)
        return self._mask_cache[key]

    def inference_batch(self, index, audiofeat_batch):
        length = len(self.face_list_cycle)
        img_batch = []
        target_size = self.target_size

        if self.is_webcam:
            num_frames = len(audiofeat_batch)
            frames = self.webcam.get_buffered_frames(num_frames)
            h_orig, w_orig = frames[-1].shape[:2]

            # Nhận diện khuôn mặt chuẩn genavatar.py
            if self.detector is None:
                logger.info("Khoi tao bo nhan dien khuon mat cho Live Webcam...")
                from avatars.wav2lip import face_detection
                self.detector = face_detection.FaceAlignment(face_detection.LandmarksType._2D,
                                                            flip_input=False, device=str(device))
            
            detect_frame = frames[-1]
            detect_h = min(360, h_orig)
            scale = detect_h / float(h_orig)
            detect_w = int(w_orig * scale)
            small_frame = cv2.resize(detect_frame, (detect_w, detect_h), interpolation=cv2.INTER_LINEAR)

            rects = self.detector.get_detections_for_batch(np.array([small_frame]))
            rect = rects[0]
            if rect is not None:
                rx1, ry1, rx2, ry2 = [int(v / scale) for v in rect]
                face_h = max(1, ry2 - ry1)
                pady2 = max(10, int(0.04 * face_h))
                target_y1 = max(0, ry1)
                target_y2 = min(h_orig, ry2 + pady2)
                target_x1 = max(0, rx1)
                target_x2 = min(w_orig, rx2)
                new_bbox = np.array([target_y1, target_y2, target_x1, target_x2], dtype=np.float32)

                if self.last_bbox is None:
                    self.last_bbox = new_bbox
                else:
                    diff = np.max(np.abs(new_bbox - self.last_bbox))
                    if diff < 1.5:
                        pass
                    elif diff > 15.0:
                        self.last_bbox = 0.85 * new_bbox + 0.15 * self.last_bbox
                    else:
                        self.last_bbox = 0.50 * new_bbox + 0.50 * self.last_bbox
            elif self.last_bbox is None:
                self.last_bbox = np.array([0, h_orig, 0, w_orig], dtype=np.float32)
            
            y1, y2, x1, x2 = map(int, self.last_bbox)
            y1 = max(0, min(y1, h_orig - 1))
            y2 = max(y1 + 1, min(y2, h_orig))
            x1 = max(0, min(x1, w_orig - 1))
            x2 = max(x1 + 1, min(x2, w_orig))

            # Giới hạn bộ đệm chống phình bộ nhớ
            if len(self.webcam_frame_data) > 100:
                oldest_keys = sorted(self.webcam_frame_data.keys())[:-50]
                for ok in oldest_keys:
                    self.webcam_frame_data.pop(ok, None)

            # Cắt mặt 1:1 cho từng frame trong batch và lưu ánh xạ frame_key
            for bi, f in enumerate(frames):
                face = f[y1:y2, x1:x2]
                if face.shape[0] != target_size or face.shape[1] != target_size:
                    face = cv2.resize(face, (target_size, target_size), interpolation=cv2.INTER_CUBIC)
                img_batch.append(face)
                self.webcam_frame_data[index + bi] = (f, (y1, y2, x1, x2))

            self.frame_counter += num_frames

        else:
            for i in range(self.batch_size):
                seq_id = index + i
                idx = mirror_index(length, seq_id)

                # --- KEYFRAME FACE TRACKING ---
                if self.coord_list_cycle[idx] is not None:
                    # Load preprocessed crop directly
                    face = self.face_list_cycle[idx]
                else:
                    # Dynamically crop using cached bbox
                    frame = self.frame_list_cycle[idx]
                    h_orig, w_orig = frame.shape[:2]
                    if self.last_bbox is None or self.frame_counter % 6 == 0:
                        if self.detector is None:
                            logger.info("Initializing wav2lip face detector lazily...")
                            from avatars.wav2lip import face_detection
                            self.detector = face_detection.FaceAlignment(face_detection.LandmarksType._2D,
                                                                    flip_input=False, device=str(device))
                        
                        detect_h = 360
                        scale = detect_h / float(h_orig)
                        detect_w = int(w_orig * scale)
                        small_frame = cv2.resize(frame, (detect_w, detect_h), interpolation=cv2.INTER_LINEAR)
                        
                        rects = self.detector.get_detections_for_batch(np.array([small_frame]))
                        rect = rects[0]
                        
                        if rect is not None:
                            rx1, ry1, rx2, ry2 = [int(v / scale) for v in rect]
                            cx = (rx1 + rx2) / 2.0
                            cy = (ry1 + ry2) / 2.0 + 0.04 * (ry2 - ry1)
                            side = max(rx2 - rx1, ry2 - ry1) * 1.08
                            
                            y1 = max(0, int(cy - side / 2.0))
                            y2 = min(h_orig, int(cy + side / 2.0))
                            x1 = max(0, int(cx - side / 2.0))
                            x2 = min(w_orig, int(cx + side / 2.0))
                            new_bbox = np.array([y1, y2, x1, x2], dtype=np.float32)
                            
                            if self.last_bbox is None:
                                self.last_bbox = new_bbox
                            else:
                                self.last_bbox = 0.3 * new_bbox + 0.7 * self.last_bbox
                        elif self.last_bbox is None:
                            self.last_bbox = np.array([0, h_orig, 0, w_orig], dtype=np.float32)
                    
                    y1, y2, x1, x2 = map(int, self.last_bbox)
                    y1 = max(0, min(y1, h_orig - 1))
                    y2 = max(y1 + 1, min(y2, h_orig))
                    x1 = max(0, min(x1, w_orig - 1))
                    x2 = max(x1 + 1, min(x2, w_orig))
                    face = frame[y1:y2, x1:x2]
                    
                if face.shape[0] != target_size or face.shape[1] != target_size:
                    face = cv2.resize(face, (target_size, target_size), interpolation=cv2.INTER_CUBIC)
                img_batch.append(face)
                self.frame_counter += 1
            
        img_batch = np.asarray(img_batch)
        audiofeat_batch = np.asarray(audiofeat_batch)

        img_masked = img_batch.copy()
        img_masked[:, target_size // 2:] = 0

        img_concat = np.concatenate((img_masked, img_batch), axis=3)
        img_transposed = np.ascontiguousarray(np.transpose(img_concat, (0, 3, 1, 2)))
        img_tensor = torch.from_numpy(img_transposed).float().to(device, non_blocking=True) / 255.0

        audiofeat_reshaped = np.reshape(audiofeat_batch, [len(audiofeat_batch), audiofeat_batch.shape[1], audiofeat_batch.shape[2], 1])
        audio_transposed = np.ascontiguousarray(np.transpose(audiofeat_reshaped, (0, 3, 1, 2)))
        audio_tensor = torch.from_numpy(audio_transposed).float().to(device, non_blocking=True)

        with torch.inference_mode():
            if str(device).startswith('cuda'):
                with torch.amp.autocast(device_type='cuda', dtype=torch.float16):
                    pred = self.model(audio_tensor, img_tensor)
            else:
                pred = self.model(audio_tensor, img_tensor)
        pred = pred.float().cpu().numpy().transpose(0, 2, 3, 1) * 255.
        return pred

    def paste_back_frame(self, pred_frame, frame_key: int):
        if self.is_webcam:
            meta = self.webcam_frame_data.pop(frame_key, None)
            if meta is not None and meta[0] is not None:
                combine_frame, bbox = meta
                combine_frame = combine_frame.copy()
            else:
                combine_frame = self.webcam.get_buffered_frame().copy()
                bbox = None

            if bbox is not None:
                y1, y2, x1, x2 = bbox
            elif self.last_bbox is not None:
                y1, y2, x1, x2 = map(int, self.last_bbox)
            else:
                h, w = combine_frame.shape[:2]
                y1, y2, x1, x2 = 0, h, 0, w

            h_orig, w_orig = combine_frame.shape[:2]
            y1 = max(0, min(y1, h_orig - 1))
            y2 = max(y1 + 1, min(y2, h_orig))
            x1 = max(0, min(x1, w_orig - 1))
            x2 = max(x1 + 1, min(x2, w_orig))

            h_crop = max(1, y2 - y1)
            w_crop = max(1, x2 - x1)
            res_frame = cv2.resize(pred_frame.astype(np.uint8), (w_crop, h_crop), interpolation=cv2.INTER_CUBIC)

            # Tăng cường độ sắc nét cho răng và khóe môi
            blur = cv2.GaussianBlur(res_frame, (0, 0), sigmaX=1.5)
            res_frame = cv2.addWeighted(res_frame, 1.25, blur, -0.25, 0)

            orig_crop = combine_frame[y1:y2, x1:x2]
            if orig_crop.shape[:2] == (h_crop, w_crop):
                # Tái sử dụng 100% mặt nạ và thuật toán cân bằng màu da của Avatar có sẵn
                mask = self.get_blend_mask(h_crop, w_crop, is_webcam=False)
                orig_upper = orig_crop[:int(0.38 * h_crop), :]
                res_upper = res_frame[:int(0.38 * h_crop), :]
                if orig_upper.size > 0 and res_upper.size > 0:
                    mean_orig = np.mean(orig_upper, axis=(0, 1), keepdims=True)
                    mean_res = np.mean(res_upper, axis=(0, 1), keepdims=True)
                    gain = np.clip(mean_orig / (mean_res + 1e-5), 0.88, 1.12)
                    res_frame = np.clip(res_frame.astype(np.float32) * gain, 0, 255).astype(np.uint8)

                blended = orig_crop.astype(np.float32) * (1.0 - mask) + res_frame.astype(np.float32) * mask
                combine_frame[y1:y2, x1:x2] = np.clip(blended, 0, 255).astype(np.uint8)
            else:
                combine_frame[y1:y2, x1:x2] = res_frame

            return combine_frame

        else:
            idx = frame_key
            combine_frame = self.frame_list_cycle[idx].copy()
            if self.coord_list_cycle[idx] is not None:
                y1, y2, x1, x2 = self.coord_list_cycle[idx]
            elif self.last_bbox is not None:
                y1, y2, x1, x2 = map(int, self.last_bbox)
            else:
                h, w = combine_frame.shape[:2]
                y1, y2, x1, x2 = 0, h, 0, w

            h_orig, w_orig = combine_frame.shape[:2]
            y1 = max(0, min(y1, h_orig - 1))
            y2 = max(y1 + 1, min(y2, h_orig))
            x1 = max(0, min(x1, w_orig - 1))
            x2 = max(x1 + 1, min(x2, w_orig))

            h_crop = max(1, y2 - y1)
            w_crop = max(1, x2 - x1)
            res_frame = cv2.resize(pred_frame.astype(np.uint8), (w_crop, h_crop), interpolation=cv2.INTER_CUBIC)
            
            orig_crop = combine_frame[y1:y2, x1:x2]
            if orig_crop.shape[:2] == (h_crop, w_crop):
                mask = self.get_blend_mask(h_crop, w_crop, is_webcam=False)
                orig_upper = orig_crop[:int(0.38 * h_crop), :]
                res_upper = res_frame[:int(0.38 * h_crop), :]
                if orig_upper.size > 0 and res_upper.size > 0:
                    mean_orig = np.mean(orig_upper, axis=(0, 1), keepdims=True)
                    mean_res = np.mean(res_upper, axis=(0, 1), keepdims=True)
                    gain = np.clip(mean_orig / (mean_res + 1e-5), 0.88, 1.12)
                    res_frame = np.clip(res_frame.astype(np.float32) * gain, 0, 255).astype(np.uint8)

                blended = orig_crop.astype(np.float32) * (1.0 - mask) + res_frame.astype(np.float32) * mask
                combine_frame[y1:y2, x1:x2] = np.clip(blended, 0, 255).astype(np.uint8)
            else:
                combine_frame[y1:y2, x1:x2] = res_frame

            return combine_frame

