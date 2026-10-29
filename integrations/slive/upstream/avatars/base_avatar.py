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
#  Lop co so Avatar - hop nhat tu basereal.py, tich hop vao Async Pipeline
#

import math
from numpy.typing import NDArray
import torch
import numpy as np
import subprocess
import os
import time
import cv2
import glob
import resampy
import queue
from threading import Lock, Condition
from queue import Queue
from threading import Thread, Event
from io import BytesIO
import soundfile as sf
import asyncio
from enum import Enum
import json
import importlib
import registry

import torch.multiprocessing as mp
from dataclasses import dataclass, field

from av import AudioFrame, VideoFrame
from fractions import Fraction

from utils.logger import logger
from utils.image import read_imgs,mirror_index

# class State(Enum):
#     INIT=0
#     WAIT=1
#     QUESTION=2
#     ANSWER=3

@dataclass
class AudioFrameData:
    data: NDArray[np.float32]
    type: int = 0  # Gia tri mac dinh
    userdata: dict = field(default_factory=dict)

class BaseAvatar:
    def __init__(self, opt):
        self.opt = opt
        self.sample_rate = 16000
        self.chunk = self.sample_rate // (opt.fps*2) # 320 samples per chunk (20ms)
        self.sessionid = self.opt.sessionid

        self.speaking = False
        self.recording = False
        self._record_video_pipe = None
        self._record_audio_pipe = None
        self.width = self.height = 0
        self.chat_messages = []
        self.chat_messages_lock = Lock()
        self.event_messages = []
        self.event_messages_lock = Lock()
        self.speech_status_lock = Lock()
        self.speech_status_condition = Condition(self.speech_status_lock)
        self._active_speech_token = ''
        self._completed_speech_token = ''
        self._speech_wait_generation = 0
        self.speech_status = {
            'status': 'idle', 'text': '', 'duration': 0.0,
            'startedAt': 0.0, 'endedAt': 0.0,
            'sequence': 0, 'completedSequence': 0, 'completedText': '',
            'source': '', 'completedSource': ''
        }
        self.speech_settings = {
            'ref_file': str(getattr(opt, 'REF_FILE', '') or 'Trúc Ly'),
            'speed': 1.0,
            'pause_between_sentences': 0.3,
        }

        self.custom_audiotype = 0 # 0: normal, 1: sinlence, >1: custom audio
        self.custom_img_cycle = {}
        self.custom_audio_cycle = {}
        self.custom_audio_index = {}
        self.custom_index = {}
        # self.custom_opt = {}
        self.__loadcustom()

        self.batch_size = opt.batch_size
        self.res_frame_queue = Queue(self.batch_size*2)
        self.render_event = Event()

        _tts_modules = {
            'edgetts': 'tts.edge',
            'gpt-sovits': 'tts.sovits',
            'xtts': 'tts.xtts',
            'cosyvoice': 'tts.cosyvoice',
            'fishtts': 'tts.fish',
            'f5api': 'tts.f5api',
            'tencent': 'tts.tencent',
            'doubao': 'tts.doubao',
            'indextts2': 'tts.indextts2',
            'azuretts': 'tts.azure',
            'qwentts': 'tts.qwentts'
        }

        if opt.tts in _tts_modules:
            importlib.import_module(_tts_modules[opt.tts])
            self.tts = registry.create("tts", opt.tts, opt=opt, parent=self)
        else:
            logger.error(f"TTS module {opt.tts} not found.")

        _output_modules = {
            'webrtc': 'streamout.webrtc',
            'rtcpush': 'streamout.webrtc',
            'rtmp': 'streamout.rtmp',
            'virtualcam': 'streamout.virtualcam'
        }

        # Khoi tao module Output
        if opt.transport in _output_modules:
            try:
                importlib.import_module(_output_modules[opt.transport])
                self.output = registry.create("streamout", opt.transport, opt=opt, parent=self)
            except ModuleNotFoundError:
                logger.error(f"Output transport module {_output_modules[opt.transport]} not found.")
        else:
            logger.error(f"Output transport {opt.transport} not found in map.")

    # Neu he thong khong dung pipeline, hoac de tuong thich nguoc voi ttsreal.py cu
    def put_msg_txt(self, msg, datainfo:dict={}, priority: bool = False):
        if hasattr(self, 'tts'):
            self.tts.put_msg_txt(msg, datainfo, priority=priority)

    def push_chat_message(self, role: str, text: str):
        text = str(text).strip()
        if not text:
            return
        with self.chat_messages_lock:
            self.chat_messages.append({
                'role': role,
                'text': text,
                'timestamp': time.time(),
            })

    def pop_chat_messages(self):
        with self.chat_messages_lock:
            messages = self.chat_messages[:]
            self.chat_messages.clear()
        return messages

    def push_event_message(self, role: str, text: str):
        text = str(text).strip()
        if not text:
            return
        with self.event_messages_lock:
            self.event_messages.append({
                'role': role,
                'text': text,
                'timestamp': time.time(),
            })

    def pop_event_messages(self):
        with self.event_messages_lock:
            messages = self.event_messages[:]
            self.event_messages.clear()
        return messages
    
    def put_audio_frame(self, audio_chunk:NDArray[np.float32], datainfo:dict={}, type: int = 0): # 16khz 20ms pcm
        if hasattr(self, 'asr'):
            self.asr.put_audio_frame(audio_chunk, datainfo, type=type)

    def put_silence_duration(self, duration_seconds: float = 0.3):
        """Chèn khoảng đệm im lặng (silence) giữa các câu để avatar nghỉ thở tự nhiên."""
        if duration_seconds <= 0 or not hasattr(self, 'asr'):
            return
        num_chunks = max(1, int(round(duration_seconds * 1000 / 20)))  # 20ms mỗi chunk
        silent_chunk = np.zeros(self.chunk, dtype=np.float32)
        for _ in range(num_chunks):
            self.put_audio_frame(silent_chunk, datainfo={}, type=1)

    def put_audio_file(self, filebyte, datainfo:dict={}): 
        input_stream = BytesIO(filebyte)
        stream = self.__create_bytes_stream(input_stream)
        streamlen = stream.shape[0]
        idx = 0
        first = True
        while streamlen >= self.chunk:
            eventpoint = {}
            if first:
                eventpoint = {'status': 'start'}
                first = False
            if streamlen - self.chunk < self.chunk:
                eventpoint = {'status': 'end'}
            eventpoint.update(**datainfo) 
            self.put_audio_frame(stream[idx:idx+self.chunk], eventpoint)
            streamlen -= self.chunk
            idx += self.chunk

    def put_audio_filepath(self, filepath, datainfo:dict={}): 
        stream = self.__create_bytes_stream(filepath)
        streamlen = stream.shape[0]
        idx = 0
        first = True
        while streamlen >= self.chunk:
            eventpoint = {}
            if first:
                eventpoint = {'status': 'start'}
                first = False
            if streamlen - self.chunk < self.chunk:
                eventpoint = {'status': 'end'}
            eventpoint.update(**datainfo) 
            self.put_audio_frame(stream[idx:idx+self.chunk], eventpoint)
            streamlen -= self.chunk
            idx += self.chunk
    
    def __create_bytes_stream(self, byte_stream):
        stream, sample_rate = sf.read(byte_stream) # [T*sample_rate,] float64
        logger.info(f'[INFO]put audio stream {sample_rate}: {stream.shape}')
        stream = stream.astype(np.float32)

        if stream.ndim > 1:
            logger.info(f'[WARN] audio has {stream.shape[1]} channels, only use the first.')
            stream = stream[:, 0]
    
        if sample_rate != self.sample_rate and stream.shape[0] > 0:
            logger.info(f'[WARN] audio sample rate is {sample_rate}, resampling into {self.sample_rate}.')
            stream = resampy.resample(x=stream, sr_orig=sample_rate, sr_new=self.sample_rate)

        return stream

    def flush_talk(self):
        if hasattr(self, 'tts') and hasattr(self.tts, 'flush_talk'):
            self.tts.flush_talk()
        if hasattr(self, 'asr') and hasattr(self.asr, 'flush_talk'):
            self.asr.flush_talk()
        self.custom_audiotype = 0
        with self.speech_status_condition:
            self._speech_wait_generation += 1
            self.speech_status.update({'status': 'idle', 'text': '', 'duration': 0.0,
                                       'endedAt': time.time()})
            self.speech_status_condition.notify_all()

    # def flush(self):
    #     self.flush_talk()

    def is_speaking(self) -> bool:
        return self.speaking

    def update_speech_status(self, eventpoint: dict):
        if not eventpoint or eventpoint.get('status') not in ('start', 'end'):
            return
        with self.speech_status_condition:
            speech_token = str(eventpoint.get('_speech_token') or '')
            if eventpoint['status'] == 'start':
                self._active_speech_token = speech_token
                self.speech_status['sequence'] += 1
                self.speech_status.update({
                    'status': 'speaking',
                    'text': str(eventpoint.get('text') or ''),
                    'duration': max(0.0, float(eventpoint.get('duration') or 0.0)),
                    'source': str(eventpoint.get('source') or 'live'),
                    'startedAt': time.time(),
                    'endedAt': 0.0,
                })
            else:
                self._completed_speech_token = speech_token
                self.speech_status.update({
                    'status': 'idle',
                    'completedSequence': self.speech_status['sequence'],
                    'completedText': str(eventpoint.get('text') or self.speech_status.get('text') or ''),
                    'completedSource': str(eventpoint.get('source') or self.speech_status.get('source') or 'live'),
                    'endedAt': time.time(),
                })
            self.speech_status_condition.notify_all()

    def speech_wait_generation(self):
        with self.speech_status_lock:
            return self._speech_wait_generation

    def wait_for_speech_completion(self, speech_token, generation, quit_event, lead_time_chunks=100):
        """
        Cho phép gối đầu (pre-buffering) liên tục giữa các câu:
        Mở khóa khi hàng đợi âm thanh ASR còn dưới lead_time_chunks (~2 giây), giúp câu tiếp theo
        được tổng hợp sẵn trong lúc câu trước đang phát sắp xong.
        Tránh trường hợp nạp dồn dập quá nhiều câu cùng lúc làm nuốt khoảng dừng hoặc cướp lời.
        """
        completion_deadline = time.time() + 300.0
        with self.speech_status_condition:
            while not quit_event.is_set():
                if generation != self._speech_wait_generation:
                    return False
                if self._completed_speech_token == speech_token:
                    return True

                # Kiểm tra số lượng chunk audio còn lại trong hàng đợi ASR
                asr_qsize = self.asr.queue.qsize() if hasattr(self, 'asr') and hasattr(self.asr, 'queue') else 0

                # Nếu buffer còn dưới ~2.0 giây (100 chunks * 20ms) -> Cho phép TTS sinh tiếp câu mới
                if asr_qsize <= lead_time_chunks:
                    return True

                if time.time() >= completion_deadline:
                    return False
                self.speech_status_condition.wait(timeout=0.05)
        return False

    def get_speech_status(self):
        with self.speech_status_lock:
            return dict(self.speech_status)
    
    def __loadcustom(self):
        if not hasattr(self.opt, 'customopt') or not self.opt.customopt:
            return
        for item in self.opt.customopt:
            logger.info(item)
            input_img_list = glob.glob(os.path.join(item['imgpath'], '*.[jpJP][pnPN]*[gG]'))
            input_img_list = sorted(input_img_list, key=lambda x: int(os.path.splitext(os.path.basename(x))[0]))
            self.custom_img_cycle[item['audiotype']] = read_imgs(input_img_list)
            if item.get('audiopath'):
                self.custom_audio_cycle[item['audiotype']], sample_rate = sf.read(item['audiopath'], dtype='float32')
                self.custom_audio_index[item['audiotype']] = 0
            self.custom_index[item['audiotype']] = 0
            # self.custom_opt[item['audiotype']] = item

    def init_customindex(self):
        self.custom_audiotype = 0
        for key in self.custom_audio_index:
            self.custom_audio_index[key] = 0
        for key in self.custom_index:
            self.custom_index[key] = 0

    def notify(self, eventpoint:dict):
        if eventpoint and eventpoint.get('status'):
            logger.info("notify:%s", eventpoint)

    def start_recording(self):
        if self.recording:
            return
        command = ['ffmpeg',
                    '-y', '-an',
                    '-f', 'rawvideo',
                    '-vcodec','rawvideo',
                    '-pix_fmt', 'bgr24',
                    '-s', "{}x{}".format(self.width, self.height),
                    '-r', str(25),
                    '-i', '-',
                    '-pix_fmt', 'yuv420p', 
                    '-vcodec', "h264",
                    f'temp{self.opt.sessionid}.mp4']
        self._record_video_pipe = subprocess.Popen(command, shell=False, stdin=subprocess.PIPE)

        acommand = ['ffmpeg',
                    '-y', '-vn',
                    '-f', 's16le',
                    '-ac', '1',
                    '-ar', '16000',
                    '-i', '-',
                    '-acodec', 'aac',
                    f'temp{self.opt.sessionid}.aac']
        self._record_audio_pipe = subprocess.Popen(acommand, shell=False, stdin=subprocess.PIPE)

        self.recording = True
    
    def record_video_data(self, image):
        if self.width == 0:
            self.height, self.width, _ = image.shape
        if self.recording:
            self._record_video_pipe.stdin.write(image.tostring())

    def record_audio_data(self, frame):
        if self.recording:
            self._record_audio_pipe.stdin.write(frame.tostring())
		
    def stop_recording(self):
        if not self.recording:
            return
        self.recording = False 
        self._record_video_pipe.stdin.close()
        self._record_video_pipe.wait()
        self._record_audio_pipe.stdin.close()
        self._record_audio_pipe.wait()
        cmd_combine_audio = f"ffmpeg -y -i temp{self.opt.sessionid}.aac -i temp{self.opt.sessionid}.mp4 -c:v copy -c:a copy data/record.mp4"
        os.system(cmd_combine_audio)

    # def mirror_index(self, size, index):
    #     turn = index // size
    #     res = index % size
    #     if turn % 2 == 0:
    #         return res
    #     else:
    #         return size - res - 1 
    
    def get_custom_audio_stream(self, audiotype):
        idx = self.custom_audio_index[audiotype]
        stream = self.custom_audio_cycle[audiotype][idx:idx+self.chunk]
        self.custom_audio_index[audiotype] += self.chunk
        if self.custom_audio_index[audiotype] >= self.custom_audio_cycle[audiotype].shape[0]:
            self.custom_audiotype = 1
        return stream
    
    def set_custom_state(self, audiotype, reinit=True):
        print('set_custom_state:', audiotype)
        if self.custom_audio_index.get(audiotype) is None:
            return
        self.custom_audiotype = audiotype
        if reinit:
            self.custom_audio_index[audiotype] = 0
            self.custom_index[audiotype] = 0

    # ========================== Ket xuat chinh va cau noi Pipeline ==========================
    def get_avatar_length(self):
        if hasattr(self, 'frame_list_cycle'):
            return len(self.frame_list_cycle)
        return 1
        
    def inference(self, quit_event):
        length = self.get_avatar_length()
        index = 0
        count = 0
        counttime = 0
        last_speaking = False

        # syncnet_T = 12  # Buoc thoi gian
        # weight_dtype = torch.float16  # Kieu du lieu
        # infernum = 0
        logger.info('start inference')
        while not quit_event.is_set():
            starttime = time.perf_counter()
            audiofeat_batch = []
            try:
                audiofeat_batch = self.asr.feat_queue.get(block=True, timeout=1)
            except queue.Empty:
                continue
                
            is_all_silence = True
            audio_frames: list[AudioFrameData] = []
            for _ in range(self.batch_size * 2):
                audioframe:AudioFrameData = self.asr.output_queue.get()
                if audioframe.type == 0:
                    is_all_silence = False               
                audio_frames.append(audioframe)

            # Kiem tra thay doi trang thai
            current_speaking = not is_all_silence

            if is_all_silence: #Toan du lieu la lang, chi can lay fullimg, khong can suy luan
                for i in range(self.batch_size):
                    frame_key = index if getattr(self, 'is_webcam', False) else mirror_index(length, index)
                    if getattr(self, 'is_webcam', False) and hasattr(self, 'on_webcam_silence_frame'):
                        self.on_webcam_silence_frame(frame_key)
                    self.res_frame_queue.put((None, audio_frames[i*2:i*2+2], frame_key))
                    index = index + 1
            else:
                if current_speaking and not last_speaking and self.custom_index.get(1) is not None: #Chuyen tu lang sang noi, va co video tinh tuy chinh
                    index = 0
                t = time.perf_counter()

                pred = self.inference_batch(index, audiofeat_batch)

                counttime += (time.perf_counter() - t)
                count += self.batch_size
                if count >= 100:
                    logger.info(f"------actual avg infer fps:{count/counttime:.4f}")
                    count = 0
                    counttime = 0
                for i, res_frame in enumerate(pred):
                    frame_key = (index + i) if getattr(self, 'is_webcam', False) else mirror_index(length, index + i)
                    self.res_frame_queue.put((res_frame, audio_frames[i*2:i*2+2], frame_key))
                index = index + self.batch_size
                    
            if current_speaking != last_speaking:
                logger.info(f"inference chuyen trang thai: {'noi' if last_speaking else 'lang'} → {'noi' if current_speaking else 'lang'}")
                last_speaking = current_speaking         
        logger.info('baseavatar inference thread stop')

    def process_frames(self,quit_event):
        enable_transition = not getattr(self, 'is_webcam', False)  # Bat chuyen canh cho avatar co san, tat cho live webcam de tranh bong ma (ghosting) khi dau cu dong
        
        _last_speaking = False
        _transition_start = time.time()
        _transition_duration = 0.08  # Thời gian chuyển cảnh ngắn (~0.08s ~ 2 frames)
        _last_silent_frame = None  # Cache frame lặng
        _last_speaking_frame = None  # Cache frame nói

        self.output.start()
        
        while not quit_event.is_set():
            try:
                audio_frames: list[AudioFrameData]
                res_frame,audio_frames,idx = self.res_frame_queue.get(block=True, timeout=1)
            except queue.Empty:
                continue
            
            # Kiem tra thay doi trang thai
            current_speaking = not (audio_frames[0].type!=0 and audio_frames[1].type!=0)
            if current_speaking != _last_speaking:
                logger.info(f"Trang thai: {'Noi' if _last_speaking else 'Im lang'} → {'Noi' if current_speaking else 'Im lang'}")
                _transition_start = time.time()
            _last_speaking = current_speaking

            if audio_frames[0].type!=0 and audio_frames[1].type!=0: #Toan du lieu la lang, chi can lay fullimg
                self.speaking = False
                audiotype = audio_frames[0].type
                if getattr(self, 'is_webcam', False) and hasattr(self, 'get_webcam_silent_frame'):
                    target_frame = self.get_webcam_silent_frame(idx)
                elif getattr(self, 'is_webcam', False) and hasattr(self, 'webcam'):
                    target_frame = self.webcam.get_frame()
                elif self.custom_index.get(audiotype) is not None: #Co video tuy chinh
                    mirindex = mirror_index(len(self.custom_img_cycle[audiotype]),self.custom_index[audiotype])
                    target_frame = self.custom_img_cycle[audiotype][mirindex]
                    self.custom_index[audiotype] += 1
                else:
                    target_frame = self.frame_list_cycle[idx]
                
                if enable_transition:
                    # Chuyen canh noi→lang
                    if time.time() - _transition_start < _transition_duration and _last_speaking_frame is not None and _last_speaking_frame.shape == target_frame.shape:
                        alpha = min(1.0, (time.time() - _transition_start) / _transition_duration)
                        combine_frame = cv2.addWeighted(_last_speaking_frame, 1-alpha, target_frame, alpha, 0)
                    else:
                        combine_frame = target_frame
                    # Cache frame lang
                    _last_silent_frame = combine_frame.copy()
                else:
                    combine_frame = target_frame
            else:
                self.speaking = True
                try:
                    current_frame = self.paste_back_frame(res_frame,idx)
                except Exception as e:
                    logger.warning(f"paste_back_frame error: {e}")
                    continue
                if enable_transition:
                    # Chuyen canh lang→noi
                    if time.time() - _transition_start < _transition_duration and _last_silent_frame is not None and _last_silent_frame.shape == current_frame.shape:
                        alpha = min(1.0, (time.time() - _transition_start) / _transition_duration)
                        combine_frame = cv2.addWeighted(_last_silent_frame, 1-alpha, current_frame, alpha, 0)
                    else:
                        combine_frame = current_frame
                    # Cache frame noi
                    _last_speaking_frame = combine_frame.copy()
                else:
                    combine_frame = current_frame

            cv2.putText(combine_frame, "LiveTalking", (10, 20), cv2.FONT_HERSHEY_SIMPLEX, 0.3, (128,128,128), 1)
            
            # Sử dụng interface xuất video
            self.output.push_video_frame(combine_frame)
            self.record_video_data(combine_frame)

            for audio_frame in audio_frames:
                #frame,type,eventpoint = audio_frame
                frame = (audio_frame.data * 32767).astype(np.int16)
                self.update_speech_status(audio_frame.userdata)

                # Sử dụng interface xuất âm thanh
                self.output.push_audio_frame(frame, audio_frame.userdata)
                self.record_audio_data(frame)
                
            # if self.opt.transport == 'virtualcam' and hasattr(self.output, '_cam') and self.output._cam:
            #     self.output._cam.sleep_until_next_frame()

        if getattr(self, 'webcam', None) is not None:
            try:
                self.webcam.stop()
            except Exception:
                pass
        self.output.stop()
        logger.info('baseavatar process_frames thread stop') 

    def render(self,quit_event):
        self.quit_event = quit_event
        
        self.init_customindex()
        self.tts.render(quit_event)

        infer_quit_event = mp.Event()
        infer_thread = Thread(target=self.inference, args=(infer_quit_event,))
        infer_thread.start()
        
        process_quit_event = Event()
        process_thread = Thread(target=self.process_frames, args=(process_quit_event,))
        process_thread.start()

        count=0
        totaltime=0
        _starttime=time.perf_counter()
        _totalframe=0
        while not quit_event.is_set(): 
            t = time.perf_counter()
            self.asr.run_step()

            buffer_size = self.output.get_buffer_size() if hasattr(self.output, 'get_buffer_size') else 0
            if buffer_size >= 5:
                logger.debug('sleep qsize=%d', buffer_size)
                time.sleep(0.01)
        logger.info('baseavatar render thread stop')

        infer_quit_event.set()
        infer_thread.join()

        process_quit_event.set()
        process_thread.join()

