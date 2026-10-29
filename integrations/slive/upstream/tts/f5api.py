import time
import math
import numpy as np
import resampy
import soundfile as sf
import httpx
from io import BytesIO

from utils.logger import logger
from .base_tts import BaseTTS, State
from registry import register


@register("tts", "f5api")
class F5TTS(BaseTTS):
    def __init__(self, opt, parent):
        super().__init__(opt, parent)
        self.tts_server = opt.TTS_SERVER
        self.client = httpx.Client(timeout=120.0)
        self.max_retries = 3

    def txt_to_audio(self, msg: tuple[str, dict]):
        text, textevent = msg

        self.input_stream.seek(0)
        self.input_stream.truncate()

        if not text or not text.strip():
            logger.warning('F5TTS: Received empty text')
            return

        text = self._sanitize_text(text)
        if not text:
            logger.warning('F5TTS: Text becomes empty after sanitization')
            return

        voice = textevent.get('tts', {}).get('ref_file', self.opt.REF_FILE)
        t = time.time()

        try:
            self._generate_audio(text, voice, textevent)
        except TimeoutError:
            logger.error(f'F5TTS: Overall timeout for text: {text[:50]}...')
            self.input_stream.seek(0)
            self.input_stream.truncate()
            return
        except Exception as e:
            logger.error(f'F5TTS generation failed: {type(e).__name__}: {str(e)}')
            self.input_stream.seek(0)
            self.input_stream.truncate()
            return

        logger.info(f'-------f5 tts time:{time.time()-t:.4f}s')
        if self.input_stream.getbuffer().nbytes <= 0:
            logger.error(f'F5TTS: No audio data received for text: {text[:50]}...')
            return

        self.input_stream.seek(0)
        stream = self.__create_bytes_stream(self.input_stream)
        sample_count = stream.shape[0]
        duration = sample_count / float(self.sample_rate) if sample_count > 0 else 0.0
        total_chunks = int(math.ceil(sample_count / self.chunk)) if sample_count else 0
        for chunk_index in range(total_chunks):
            if self.state != State.RUNNING:
                break
            start = chunk_index * self.chunk
            audio_chunk = stream[start:start + self.chunk]
            if audio_chunk.shape[0] < self.chunk:
                audio_chunk = np.pad(audio_chunk, (0, self.chunk - audio_chunk.shape[0]))
            eventpoint = {}
            if chunk_index == 0:
                eventpoint = {'status': 'start', 'text': text, 'duration': duration}
            elif chunk_index == total_chunks - 1:
                eventpoint = {'status': 'end', 'text': text, 'duration': duration}
            eventpoint.update(**textevent)
            self.parent.put_audio_frame(audio_chunk, eventpoint)

        # Preserve both start/end notifications even for an extremely short
        # utterance that fits into a single 20 ms frame.
        if total_chunks == 1 and self.state == State.RUNNING:
            end_event = {'status': 'end', 'text': text, 'duration': duration}
            end_event.update(**textevent)
            self.parent.put_audio_frame(np.zeros(self.chunk, dtype=np.float32), end_event)

        self.input_stream.seek(0)
        self.input_stream.truncate()

    def _generate_audio(self, text: str, voice: str, textevent: dict):
        url = f"{self.tts_server.rstrip('/')}/v1/audio/speech"
        payload = {
            "input": text,
            "voice": voice,
            "speed": max(0.8, min(1.6, float(textevent.get('tts', {}).get('speed', 1.0)))),
        }

        last_error = None
        for attempt in range(self.max_retries):
            try:
                logger.debug(f'F5TTS: POST {url} voice={voice} text_len={len(text)} attempt={attempt+1}/{self.max_retries}')

                with self.client.stream("POST", url, json=payload) as resp:
                    if resp.status_code == 200:
                        bytes_received = 0
                        for chunk in resp.iter_bytes(chunk_size=8192):
                            if chunk and self.state == State.RUNNING:
                                self.input_stream.write(chunk)
                                bytes_received += len(chunk)

                        audio_size = self.input_stream.getbuffer().nbytes
                        if audio_size > 0:
                            logger.info(f'F5TTS success: voice={voice} audio_size={audio_size} bytes_received={bytes_received} attempt={attempt+1}')
                            return

                        logger.warning(f'F5TTS: No audio data in response (attempt {attempt+1}), retrying...')
                        if attempt < self.max_retries - 1:
                            self.input_stream.seek(0)
                            self.input_stream.truncate()
                    else:
                        logger.error(f'F5TTS stream failed: status={resp.status_code} attempt={attempt+1}')
                        if attempt < self.max_retries - 1:
                            self.input_stream.seek(0)
                            self.input_stream.truncate()

            except (httpx.WriteTimeout, httpx.ReadTimeout, TimeoutError) as e:
                last_error = e
                logger.warning(f'F5TTS: {type(e).__name__} on attempt {attempt+1}/{self.max_retries}')
                if attempt < self.max_retries - 1:
                    wait_time = 2 ** attempt
                    logger.info(f'F5TTS: Retry after {wait_time}s...')
                    time.sleep(wait_time)
                    self.input_stream.seek(0)
                    self.input_stream.truncate()

            except Exception as e:
                last_error = e
                logger.error(f'F5TTS API call failed on attempt {attempt+1}/{self.max_retries}: {type(e).__name__}: {str(e)[:100]}')
                if attempt < self.max_retries - 1:
                    wait_time = 2 ** attempt
                    logger.info(f'F5TTS: Retry after {wait_time}s...')
                    time.sleep(wait_time)
                    self.input_stream.seek(0)
                    self.input_stream.truncate()

        if last_error:
            logger.error(f'F5TTS: All {self.max_retries} attempts failed. Last error: {type(last_error).__name__}')
            raise last_error
        logger.error(f'F5TTS: All {self.max_retries} attempts failed with no audio data')

    def __create_bytes_stream(self, byte_stream):
        try:
            stream, sample_rate = sf.read(byte_stream)
            logger.info(f'[INFO]tts audio stream {sample_rate}: {stream.shape}')
            stream = stream.astype(np.float32)

            if stream.ndim > 1:
                logger.info(f'[WARN] audio has {stream.shape[1]} channels, only use the first.')
                stream = stream[:, 0]

            if sample_rate != self.sample_rate and stream.shape[0] > 0:
                logger.info(f'[WARN] audio sample rate is {sample_rate}, resampling into {self.sample_rate}.')
                stream = resampy.resample(x=stream, sr_orig=sample_rate, sr_new=self.sample_rate)

            return stream
        except Exception as e:
            logger.error(f'Failed to read audio stream: {e}')
            return np.array([], dtype=np.float32)

    def _sanitize_text(self, text: str) -> str:
        try:
            from utils.vietnamese_ecom_normalizer import normalize_ecom_livestream_text
            return normalize_ecom_livestream_text(text)
        except Exception as exc:
            logger.warning(f"Lỗi sanitize text qua normalizer: {exc}")
            import re
            import unicodedata

            text = unicodedata.normalize('NFC', text)
            text = text.replace('\r\n', ' ').replace('\n', ' ')
            text = re.sub(r'\s+', ' ', text).strip()
            text = re.sub(r'([.?!]){2,}', r'\1', text)
            text = re.sub(r'([,;:]){2,}', r'\1', text)
            text = text.strip(' \t\n\r')
            return text

    def stop_tts(self):
        try:
            self.client.close()
        except Exception as e:
            logger.error(f'Error closing F5TTS client: {e}')
