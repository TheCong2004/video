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
#  UltraLight 数字人 — 迁移自 lightreal.py
#  使用 HubertASR 音频特征提取（与 wav2lipls 共享）
#

import math
import torch
import numpy as np

import os
import time
import cv2
import glob
import pickle
import copy

import queue
from queue import Queue
from threading import Thread, Event
import torch.multiprocessing as mp

from avatars.audio_features.hubert import HubertASR
import asyncio
from av import AudioFrame, VideoFrame
from avatars.base_avatar import BaseAvatar

from tqdm import tqdm

import torch.nn as nn
from torch import optim
from transformers import Wav2Vec2Processor, HubertModel
from torch.utils.data import DataLoader
from avatars.ultralight.unet import Model
from avatars.ultralight.audio2feature import Audio2Feature
from utils.logger import logger
from utils.image import read_imgs, mirror_index
from utils.device import initialize_device
from registry import register

device = initialize_device()
logger.info('Using {} for inference.'.format(device))

def load_model(opt):
    audio_processor = Audio2Feature()
    model = None
    return audio_processor,model

def load_avatar(avatar_id):
    avatar_path = f"./data/avatars/{avatar_id}"
    full_imgs_path = f"{avatar_path}/full_imgs" 
    face_imgs_path = f"{avatar_path}/face_imgs" 
    coords_path = f"{avatar_path}/coords.pkl" 
    
    # Locate model checkpoint
    checkpoint_path = f"{avatar_path}/ultralight.pth"
    if not os.path.exists(checkpoint_path):
        checkpoint_path = "./models/ultralight.pth"
    if not os.path.exists(checkpoint_path):
        raise FileNotFoundError(f"Could not find model weights 'ultralight.pth' under {avatar_path} or ./models/")
        
    model = Model(6, 'hubert').to(device)
    model.load_state_dict(torch.load(checkpoint_path))
    
    # 1. First, try to load preprocessed files from disk (instant startup)
    if os.path.exists(coords_path) and os.path.exists(full_imgs_path) and os.path.exists(face_imgs_path):
        logger.info(f"Loading preprocessed avatar data from disk for {avatar_id} (Instant startup)...")
        with open(coords_path, 'rb') as f:
            coord_list_cycle = pickle.load(f)
        input_img_list = glob.glob(os.path.join(full_imgs_path, '*.[jpJP][pnPN]*[gG]'))
        input_img_list = sorted(input_img_list, key=lambda x: int(os.path.splitext(os.path.basename(x))[0]))
        frame_list_cycle = read_imgs(input_img_list)
        input_face_list = glob.glob(os.path.join(face_imgs_path, '*.[jpJP][pnPN]*[gG]'))
        input_face_list = sorted(input_face_list, key=lambda x: int(os.path.splitext(os.path.basename(x))[0]))
        face_list_cycle = read_imgs(input_face_list)
        return model.eval(), frame_list_cycle, face_list_cycle, coord_list_cycle
        
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

    # If video file is found, process dynamically on the main thread and save to disk
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
            # Add watermark text
            cv2.putText(frame, "LiveTalking", (10, 20), cv2.FONT_HERSHEY_SIMPLEX, 0.3, (128, 128, 128), 1)
            frame_list_cycle.append(frame)
        cap.release()
        
        if not frame_list_cycle:
            raise ValueError(f"No frames extracted from video file: {video_file}")
            
        face_list_cycle = [None] * len(frame_list_cycle)
        coord_list_cycle = [None] * len(frame_list_cycle)
        return model.eval(), frame_list_cycle, face_list_cycle, coord_list_cycle
    else:
        raise FileNotFoundError(f"Could not load avatar '{avatar_id}'. No video file or preprocessed data found.")


@torch.no_grad()
def warm_up(batch_size,avatar,modelres):
    logger.info('warmup model...')
    model,_,_,_ = avatar
    img_batch = torch.ones(batch_size, 6, modelres, modelres).to(device)
    mel_batch = torch.ones(batch_size, 16, 32, 32).to(device)
    model(img_batch, mel_batch)

@register("avatar", "ultralight")
class LightReal(BaseAvatar):
    @torch.no_grad()
    def __init__(self, opt, model, avatar):
        super().__init__(opt)
        audio_processor,_ = model
        self.model,self.frame_list_cycle,self.face_list_cycle,self.coord_list_cycle = avatar
        self.landmark = None # Initialize lazily
        self.last_bbox = None
        self.frame_counter = 0
        self.asr = HubertASR(opt,self,audio_processor,audio_feat_length=[4,4])
        self.asr.warm_up()

    def inference_batch(self, index, audiofeat_batch):
        length = len(self.face_list_cycle)
        img_batch = []
        target_size = 168

        for i in range(self.batch_size):
            idx = mirror_index(length, index + i)
            
            # --- KEYFRAME FACE TRACKING ---
            if self.coord_list_cycle[idx] is not None:
                # Load preprocessed crop directly
                crop_img = self.face_list_cycle[idx]
            else:
                # Dynamically crop using cached bbox
                # Update last_bbox every 2 frames using EMA to eliminate coordinate noise/jitter
                if self.last_bbox is None or self.frame_counter % 2 == 0:
                    if self.landmark is None:
                        logger.info("Initializing ultralight face/landmark detector lazily...")
                        from avatars.ultralight.face_detect_utils.get_landmark import Landmark
                        self.landmark = Landmark()
                        
                    img = self.frame_list_cycle[idx]
                    try:
                        lms, x1, y1 = self.landmark.detect_frame(img)
                        xmin = lms[1][0] + x1
                        ymin = lms[52][1] + y1
                        xmax = lms[31][0] + x1
                        width = xmax - xmin
                        new_bbox = np.array([xmin, ymin, xmin + width, ymin + width], dtype=np.float32)
                        
                        if self.last_bbox is None:
                            self.last_bbox = new_bbox
                        else:
                            # EMA smoothing (alpha = 0.2) to prevent coordinate jitter
                            self.last_bbox = 0.2 * new_bbox + 0.8 * self.last_bbox
                    except Exception as e:
                        logger.warning(f"Face detection failed during keyframe update: {e}")
                        if self.last_bbox is None:
                            h, w = img.shape[:2]
                            self.last_bbox = np.array([0, 0, w, h], dtype=np.float32)
                
                # Crop using the smoothed bounding box coordinates
                x1, y1, x2, y2 = map(int, self.last_bbox)
                img = self.frame_list_cycle[idx]
                crop_img = img[y1:y2, x1:x2]
                if crop_img.size == 0:
                    crop_img = cv2.resize(img, (target_size, target_size), cv2.INTER_AREA)
                else:
                    crop_img = cv2.resize(crop_img, (target_size, target_size), cv2.INTER_AREA)
                    
            img_real_ex = crop_img[4:164, 4:164].copy()
            img_real_ex_ori = img_real_ex.copy()
            img_masked = cv2.rectangle(img_real_ex_ori,(5,5,150,145),(0,0,0),-1)

            img_masked = img_masked.transpose(2,0,1).astype(np.float32)
            img_real_ex = img_real_ex.transpose(2,0,1).astype(np.float32)

            img_real_ex_T = torch.from_numpy(img_real_ex / 255.0)
            img_masked_T = torch.from_numpy(img_masked / 255.0)
            img_concat_T = torch.cat([img_real_ex_T, img_masked_T], axis=0)[None]
            img_batch.append(img_concat_T)
            self.frame_counter += 1

        reshaped_audiofeat_batch = [arr.reshape(16, 32, 32) for arr in audiofeat_batch]
        audiofeat_batch = torch.stack([torch.from_numpy(arr) for arr in reshaped_audiofeat_batch])
        img_batch = torch.stack(img_batch).squeeze(1)

        with torch.no_grad():
            pred = self.model(img_batch.cuda(), audiofeat_batch.cuda())
        pred = pred.cpu().numpy().transpose(0, 2, 3, 1) * 255.
        return pred
    
    def paste_back_frame(self,pred_frame,idx:int):
        combine_frame = copy.deepcopy(self.frame_list_cycle[idx])
        if self.coord_list_cycle[idx] is not None:
            x1, y1, x2, y2 = self.coord_list_cycle[idx]
            crop_img = self.face_list_cycle[idx]
        else:
            x1, y1, x2, y2 = map(int, self.last_bbox)
            crop_img = combine_frame[y1:y2, x1:x2]
            if crop_img.size == 0:
                crop_img = cv2.resize(combine_frame, (168, 168))
            else:
                crop_img = cv2.resize(crop_img, (168, 168))

        crop_img_ori = crop_img.copy()
        crop_img_ori[4:164, 4:164] = pred_frame.astype(np.uint8)
        crop_img_ori = cv2.resize(crop_img_ori, (x2-x1,y2-y1))
        combine_frame[y1:y2, x1:x2] = crop_img_ori
        return combine_frame
