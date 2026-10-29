import os
import base64
import time
import threading
import numpy as np
import resampy

from utils.logger import logger
from .base_tts import BaseTTS, State
from registry import register

try:
    import dashscope
    from dashscope.audio.qwen_tts_realtime import (
        QwenTtsRealtime,
        QwenTtsRealtimeCallback,
        AudioFormat,
    )
except ImportError:
    logger.error("QwenTTS can phai cai dashscope SDK: pip install dashscope>=1.25.11")
    raise


SRC_SR = 24000   # Qwen TTS chi ho tro dau ra 24kHz
DST_SR = 16000   # Tan so lay mau chuan cua du an


@register("tts", "qwentts")
class QwenTTS(BaseTTS):
    """
    Tong hop giong noi thoi gian thuc Alibaba Cloud Tongyi Qwen (Qwen TTS Realtime)
    Dua tren DashScope Python SDK (dashscope >= 1.25.11)
    Su dung che do commit: chi tao mot ket noi WebSocket, moi lan tong hop se kich hoat qua append_text + commit.

    Can cai dat bien moi truong DASHSCOPE_API_KEY.
    Cach dung:
        python app.py --tts qwentts --REF_FILE Cherry
    Trong do REF_FILE duoc dung lam ten giong noi (voice), nhu Cherry / Ethan va cac giong noi he thong khac.
    """

    def __init__(self, opt, parent):
        super().__init__(opt, parent)

        # Ten giong noi, tai su dung tham so REF_FILE
        self.voice = opt.REF_FILE if opt.REF_FILE else 'Cherry'
        # Ten model
        self.model = getattr(opt, 'qwen_tts_model', 'qwen3-tts-flash-realtime')
        # URL WebSocket
        self.ws_url = getattr(opt, 'qwen_tts_url',
                              'wss://dashscope.aliyuncs.com/api-ws/v1/realtime')

        # Cai dat DashScope API Key
        api_key = getattr(opt, 'dashscope_api_key', None) or os.environ.get('DASHSCOPE_API_KEY')
        if api_key:
            dashscope.api_key = api_key
        else:
            logger.warning("QwenTTS: DASHSCOPE_API_KEY chua duoc cai dat, hay dat bien moi truong hoac truyen qua tham so")

        # ---------- Trang thai noi bo ----------
        self._remainder = np.array([], dtype=np.float32)  # Mau 16kHz con thieu mot chunk sau lan resample truoc
        self._response_event = threading.Event()
        self._first_chunk = True          # Goi am dau tien cua cau dang tong hop
        self._current_text = ''
        self._current_textevent = {}

        # ---------- Lop callback ----------
        tts_ref = self

        class _Callback(QwenTtsRealtimeCallback):
            def on_open(self) -> None:
                logger.info("QwenTTS WebSocket da duoc ket noi")

            def on_close(self, close_status_code, close_msg) -> None:
                logger.info(f"QwenTTS WebSocket da dong: code={close_status_code}, msg={close_msg}")
                tts_ref._response_event.set()

            def on_event(self, response: dict) -> None:
                try:
                    event_type = response.get('type', '')

                    if event_type == 'session.created':
                        logger.info(f"QwenTTS session: {response.get('session', {}).get('id', '')}")

                    elif event_type == 'response.audio.delta':
                        audio_b64 = response.get('delta', '')
                        if audio_b64:
                            pcm_data = base64.b64decode(audio_b64)
                            tts_ref._on_audio_data(pcm_data)

                    elif event_type == 'response.done':
                        logger.info("QwenTTS response da hoan tat")
                        tts_ref._flush_remainder()
                        tts_ref._response_event.set()

                    elif event_type == 'error':
                        logger.error(f"QwenTTS loi: {response}")
                        tts_ref._response_event.set()

                except Exception as e:
                    logger.exception(f"QwenTTS callback gap ngoai le: {e}")

        # ---------- Tao ket noi duy nhat ----------
        self._callback = _Callback()
        self._tts_client = QwenTtsRealtime(
            model=self.model,
            callback=self._callback,
            url=self.ws_url,
        )
        self._tts_client.connect()
        self._tts_client.update_session(
            voice=self.voice,
            response_format=AudioFormat.PCM_24000HZ_MONO_16BIT,  # Qwen TTS chi ho tro dau ra 24kHz
            sample_rate=16000,
            mode='commit',
        )
        logger.info(f"QwenTTS khoi tao xong: model={self.model}, voice={self.voice}")

    # ========================== Phuong thuc co ban ==========================

    def txt_to_audio(self, msg: tuple[str, dict]):
        text, textevent = msg
        t_start = time.perf_counter()

        ref_file = textevent.get('tts', {}).get('ref_file',self.opt.REF_FILE)

        # Dat lai trang thai
        self._remainder = np.array([], dtype=np.float32)
        self._first_chunk = True
        self._current_text = text
        self._current_textevent = textevent
        self._response_event.clear()

        try:
            #logger.info(f"QwenTTS gui van ban: {text[:80]}...")
            if ref_file != self.voice:
                logger.info(f'ref_file:{ref_file},self.voice:{self.voice}')
                self.voice=ref_file
                self._tts_client.close()
                self._tts_client.connect()
                self._tts_client.update_session(
                    voice=self.voice,
                    response_format=AudioFormat.PCM_24000HZ_MONO_16BIT,  # Qwen TTS chi ho tro dau ra 24kHz
                    sample_rate=16000,
                    mode='commit',
                )
            self._tts_client.append_text(text)
            self._tts_client.commit()

            # Cho response.done (am thanh duoc xu ly stream trong callback)
            self._response_event.wait(timeout=60)

            t_end = time.perf_counter()
            logger.info(f"QwenTTS tong hop hoan tat, thoi gian: {t_end - t_start:.2f}s")

        except Exception as e:
            logger.exception(f"QwenTTS txt_to_audio gap ngoai le: {e}")

    # ========================== Xu ly am thanh stream (goi trong callback) ==========================

    def _on_audio_data(self, pcm_data: bytes):
        """Nhan am thanh PCM 24kHz 16bit mono, resample sang 16kHz va day theo chunk"""
        if self.state != State.RUNNING:
            self._remainder = np.array([], dtype=np.float32)
            return

        # Toan bo PCM 24kHz -> float32 -> resample sang 16kHz mot lan
        samples_16k = np.frombuffer(pcm_data, dtype=np.int16).astype(np.float32) / 32768.0
        #samples_16k = resampy.resample(x=samples_24k, sr_orig=SRC_SR, sr_new=DST_SR)

        # Noi phan con lai cua lan truoc
        if self._remainder.shape[0] > 0:
            samples_16k = np.concatenate([self._remainder, samples_16k])

        # Chia theo self.chunk (320 mau = 20ms @16kHz) de day
        idx = 0
        total = samples_16k.shape[0]
        while total - idx >= self.chunk and self.state == State.RUNNING:
            frame = samples_16k[idx:idx + self.chunk]
            eventpoint = {}
            if self._first_chunk:
                eventpoint = {'status': 'start', 'text': self._current_text}
                self._first_chunk = False
            eventpoint.update(**self._current_textevent)
            self.parent.put_audio_frame(frame, eventpoint)
            idx += self.chunk

        # Phan khong du mot chunk de lai cho lan sau
        self._remainder = samples_16k[idx:] if idx < total else np.array([], dtype=np.float32)

    def _flush_remainder(self):
        """Tong hop xong, day phan mau con lai va gui su kien end"""
        if self.state != State.RUNNING:
            self._remainder = np.array([], dtype=np.float32)
            return

        # Day cac chunk con lai hoan chinh
        if self._remainder.shape[0] >= self.chunk:
            idx = 0
            total = self._remainder.shape[0]
            while total - idx >= self.chunk and self.state == State.RUNNING:
                frame = self._remainder[idx:idx + self.chunk]
                eventpoint = {}
                if self._first_chunk:
                    eventpoint = {'status': 'start', 'text': self._current_text}
                    self._first_chunk = False
                eventpoint.update(**self._current_textevent)
                self.parent.put_audio_frame(frame, eventpoint)
                idx += self.chunk

        self._remainder = np.array([], dtype=np.float32)

        # Gui su kien end
        eventpoint = {'status': 'end', 'text': self._current_text}
        eventpoint.update(**self._current_textevent)
        self.parent.put_audio_frame(np.zeros(self.chunk, np.float32), eventpoint)

    def stop_tts(self):
        self._tts_client.close()
        logger.info("QwenTTS da dong")