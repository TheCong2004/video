import os
import time
import numpy as np
import soundfile as sf
import resampy

from utils.logger import logger
from .base_tts import BaseTTS, State
from registry import register

class IndexTTS2(BaseTTS):
    def __init__(self, opt, parent):
        super().__init__(opt, parent)
        # Tham so cau hinh IndexTTS2
        self.server_url = opt.TTS_SERVER  # Dia chi may chu Gradio, vi du "http://127.0.0.1:7860/"
        self.ref_audio_path = opt.REF_FILE  # Duong dan file am thanh tham chieu
        self.max_tokens = getattr(opt, 'MAX_TOKENS', 120)  # So token toi da
        
        # Khoi tao client Gradio
        try:
            from gradio_client import Client, handle_file
            self.client = Client(self.server_url)
            self.handle_file = handle_file
            logger.info(f"IndexTTS2 Gradio client khoi tao thanh cong: {self.server_url}")
        except ImportError:
            logger.error("IndexTTS2 can cai gradio_client: pip install gradio_client")
            raise
        except Exception as e:
            logger.error(f"IndexTTS2 khoi tao Gradio client that bai: {e}")
            raise
        
    def txt_to_audio(self, msg):
        text, textevent = msg
        try:
            # Truoc tien tach van ban
            segments = self.split_text(text)
            if not segments:
                logger.error("IndexTTS2 tach van ban that bai")
                return
            
            logger.info(f"IndexTTS2 tach van ban thanh {len(segments)} doan")
            
            # Lap de sinh am thanh cho tung doan
            for i, segment_text in enumerate(segments):
                if self.state != State.RUNNING:
                    break
                    
                logger.info(f"IndexTTS2 dang sinh doan am thanh thu {i+1}/{len(segments)}...")
                audio_file = self.indextts2_generate(segment_text)
                
                if audio_file:
                    # Tao thong tin su kien cho tung doan
                    segment_msg = (segment_text, textevent)
                    self.file_to_stream(audio_file, segment_msg, is_first=(i==0), is_last=(i==len(segments)-1))
                else:
                    logger.error(f"IndexTTS2 sinh doan am thanh thu {i+1} that bai")
                    
        except Exception as e:
            logger.exception(f"IndexTTS2 txt_to_audio loi: {e}")

    def split_text(self, text):
        """Su dung API IndexTTS2 de tach van ban"""
        try:
            logger.info(f"IndexTTS2 bat dau tach van ban, do dai: {len(text)}")
            
            # Goi API tach van ban
            result = self.client.predict(
                text=text,
                max_text_tokens_per_segment=self.max_tokens,
                api_name="/on_input_text_change"
            )
            
            # Phan tich ket qua tach
            if 'value' in result and 'data' in result['value']:
                data = result['value']['data']
                logger.info(f"IndexTTS2 da tach tong cong {len(data)} doan")
                
                segments = []
                for i, item in enumerate(data):
                    so_thu_tu = item[0] + 1
                    noi_dung_doan = item[1]
                    so_token = item[2]
                    logger.info(f"Doan {so_thu_tu}: {len(noi_dung_doan)} ky tu, {so_token} tokens")
                    segments.append(noi_dung_doan)
                
                return segments
            else:
                logger.error(f"IndexTTS2 dinh dang ket qua tach van ban bat thuong: {result}")
                return [text]  # Neu tach that bai, tra ve nguyen van ban
                
        except Exception as e:
            logger.exception(f"IndexTTS2 tách văn bản thất bại: {e}")
            return [text]  # Nếu tách thất bại, trả về nguyên văn bản

    def indextts2_generate(self, text):
        """Gọi API Gradio của IndexTTS2 để tạo giọng nói"""
        start = time.perf_counter()
        
        try:
            # Gọi API gen_single
            result = self.client.predict(
                emo_control_method="Same as the voice reference",
                prompt=self.handle_file(self.ref_audio_path),
                text=text,
                emo_ref_path=self.handle_file(self.ref_audio_path),
                emo_weight=0.8,
                vec1=0.5,
                vec2=0,
                vec3=0,
                vec4=0,
                vec5=0,
                vec6=0,
                vec7=0,
                vec8=0,
                emo_text="",
                emo_random=False,
                max_text_tokens_per_segment=self.max_tokens,
                param_16=True,
                param_17=0.8,
                param_18=30,
                param_19=0.8,
                param_20=0,
                param_21=3,
                param_22=10,
                param_23=1500,
                api_name="/gen_single"
            )
            
            end = time.perf_counter()
            logger.info(f"IndexTTS2 tạo phân đoạn hoàn tất, thời gian: {end-start:.2f}s")
            
            # Trả về đường dẫn tệp âm thanh đã tạo
            if 'value' in result:
                audio_file = result['value']
                return audio_file
            else:
                logger.error(f"IndexTTS2 định dạng kết quả bất thường: {result}")
                return None
                
        except Exception as e:
            logger.exception(f"IndexTTS2 gọi API thất bại: {e}")
            return None

    def file_to_stream(self, audio_file, msg, is_first=False, is_last=False):
        """Chuyển tệp âm thanh thành luồng âm thanh"""
        text, textevent = msg
        
        try:
            # Đọc tệp âm thanh
            stream, sample_rate = sf.read(audio_file)
            logger.info(f'IndexTTS2 tệp âm thanh {sample_rate}Hz: {stream.shape}')
            
            # Chuyển sang float32
            stream = stream.astype(np.float32)
            
            # Nếu là đa kênh, chỉ lấy kênh đầu tiên
            if stream.ndim > 1:
                logger.info(f'IndexTTS2 âm thanh có {stream.shape[1]} kênh, chỉ dùng kênh đầu tiên')
                stream = stream[:, 0]
            
            # Nội suy lại về tần số lấy mẫu mục tiêu
            if sample_rate != self.sample_rate and stream.shape[0] > 0:
                logger.info(f'IndexTTS2 nội suy lại: {sample_rate}Hz -> {self.sample_rate}Hz')
                stream = resampy.resample(x=stream, sr_orig=sample_rate, sr_new=self.sample_rate)
            
            # Chia khối và gửi luồng âm thanh
            streamlen = stream.shape[0]
            idx = 0
            first_chunk = True
            
            while streamlen >= self.chunk and self.state == State.RUNNING:
                eventpoint = None
                
                # Chỉ gửi sự kiện start ở chunk đầu tiên của phân đoạn đầu tiên
                if is_first and first_chunk:
                    eventpoint = {'status': 'start', 'text': text, 'msgevent': textevent}
                    first_chunk = False
                
                self.parent.put_audio_frame(stream[idx:idx + self.chunk], eventpoint)
                idx += self.chunk
                streamlen -= self.chunk
            
            # Chỉ gửi sự kiện end ở phân đoạn cuối cùng
            if is_last:
                eventpoint = {'status': 'end', 'text': text, 'msgevent': textevent}
                self.parent.put_audio_frame(np.zeros(self.chunk, np.float32), eventpoint)
            
            # Dọn dẹp tệp tạm
            try:
                if os.path.exists(audio_file):
                    os.remove(audio_file)
                    logger.info(f"IndexTTS2 已删除临时文件: {audio_file}")
            except Exception as e:
                logger.warning(f"IndexTTS2 删除临时文件失败: {e}")
                
        except Exception as e:
            logger.exception(f"IndexTTS2 音频流处理失败: {e}")
