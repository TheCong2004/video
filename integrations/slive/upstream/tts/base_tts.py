from threading import Thread
import queue
from queue import Queue
from io import BytesIO
from enum import Enum

from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from avatars.base_avatar import BaseAvatar

from utils.logger import logger

class State(Enum):
    RUNNING = 0
    PAUSE = 1

class BaseTTS:
    def __init__(self, opt, parent: "BaseAvatar"):
        self.opt = opt
        self.parent = parent

        #self.fps = opt.fps # 20 ms per frame
        self.sample_rate = 16000
        self.chunk = self.sample_rate // (opt.fps*2) # 320 samples per chunk (20ms * 16000 / 1000)
        self.input_stream = BytesIO()

        self.msgqueue = Queue()
        # Keep TikTok replies separate from the livestream script so the
        # scheduler can alternate one script sentence and one reply without
        # interrupting the audio that is currently playing.
        self.priority_queue = Queue()
        self._last_queue_source = None
        self._speech_token_sequence = 0
        self.state = State.RUNNING

    def flush_talk(self):
        for target in (self.priority_queue, self.msgqueue):
            with target.mutex:
                target.queue.clear()
                target.unfinished_tasks = 0
                target.all_tasks_done.notify_all()
                target.not_full.notify_all()
        self._last_queue_source = None
        self.state = State.PAUSE

    def put_msg_txt(self, msg: str, datainfo: dict = {}, priority: bool = False):
        if len(msg) > 0:
            target = self.priority_queue if priority else self.msgqueue
            target.put((msg, datainfo))

    def render(self, quit_event):
        process_thread = Thread(target=self.process_tts, args=(quit_event,))
        process_thread.start()

    def _get_next_message(self):
        """Return the next item using fair script/TikTok round-robin."""
        if self._last_queue_source == 'script':
            candidates = (('tiktok', self.priority_queue), ('script', self.msgqueue))
        elif self._last_queue_source == 'tiktok':
            candidates = (('script', self.msgqueue), ('tiktok', self.priority_queue))
        else:
            # A live session normally starts with its script. If only a TikTok
            # reply exists it is still consumed immediately.
            candidates = (('script', self.msgqueue), ('tiktok', self.priority_queue))

        for source, target in candidates:
            try:
                message = target.get_nowait()
                self._last_queue_source = source
                return message
            except queue.Empty:
                continue
        raise queue.Empty
    
    def process_tts(self, quit_event):        
        while not quit_event.is_set():
            try:
                msg: tuple[str, dict] = self._get_next_message()
                self.state = State.RUNNING
            except queue.Empty:
                # Poll both queues. Blocking on only the script queue can leave
                # a newly arrived TikTok reply waiting indefinitely.
                quit_event.wait(0.05)
                continue
            text, datainfo = msg
            self._speech_token_sequence += 1
            speech_token = f'{id(self)}-{self._speech_token_sequence}'
            enriched_datainfo = dict(datainfo or {})
            enriched_datainfo['_speech_token'] = speech_token
            generation = (
                self.parent.speech_wait_generation()
                if hasattr(self.parent, 'speech_wait_generation') else 0
            )
            self.txt_to_audio((text, enriched_datainfo))
            if hasattr(self.parent, 'put_silence_duration') and self.state == State.RUNNING:
                # Chèn khoảng dừng thở tự nhiên giữa các câu (mặc định 0.3s)
                # giúp avatar ngậm miệng nghỉ tự nhiên, không bị phát dồn dập đè câu
                pause_sec = 0.3
                if hasattr(self.parent, 'speech_settings') and isinstance(self.parent.speech_settings, dict):
                    pause_sec = float(self.parent.speech_settings.get('pause_between_sentences', 0.3))
                elif isinstance(enriched_datainfo.get('tts'), dict):
                    pause_sec = float(enriched_datainfo['tts'].get('pause_between_sentences', 0.3))
                self.parent.put_silence_duration(pause_sec)
            if hasattr(self.parent, 'wait_for_speech_completion'):
                # Cho phép gối đầu (pre-buffering) thông minh:
                # Hệ thống sẽ mở khóa để sinh sẵn câu tiếp theo khi câu trước sắp phát xong,
                # giúp audio nối tiếp liền mạch mà không bị khoảng lặng chết giữa các câu.
                self.parent.wait_for_speech_completion(
                    speech_token, generation, quit_event
                )
        self.stop_tts()
        logger.info('ttsreal thread stop')
    
    def txt_to_audio(self, msg: tuple[str, dict]):
        pass

    def stop_tts(self):
        pass
