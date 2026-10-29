import os
import numpy as np
import azure.cognitiveservices.speech as speechsdk

from utils.logger import logger
from .base_tts import BaseTTS, State
from registry import register

@register("tts", "azuretts")
class AzureTTS(BaseTTS):
    CHUNK_SIZE = 640  # 16kHz, 20ms, 16-bit Mono PCM size
    def __init__(self, opt, parent):
        super().__init__(opt,parent)
        self.audio_buffer = b''
        voicename = self.opt.REF_FILE   # Vi du "zh-CN-XiaoxiaoMultilingualNeural"
        speech_key = os.getenv("AZURE_SPEECH_KEY")
        tts_region = os.getenv("AZURE_TTS_REGION")
        speech_endpoint = f"wss://{tts_region}.tts.speech.microsoft.com/cognitiveservices/websocket/v2"
        self.speech_config = speechsdk.SpeechConfig(subscription=speech_key,endpoint=speech_endpoint)
        self.speech_config.speech_synthesis_voice_name = voicename
        self.speech_config.set_speech_synthesis_output_format(speechsdk.SpeechSynthesisOutputFormat.Raw16Khz16BitMonoPcm)
        
        # Lay ket qua duoi dang stream trong bo nho
        self.speech_synthesizer = speechsdk.SpeechSynthesizer(speech_config=self.speech_config, audio_config=None)
        self.speech_synthesizer.synthesizing.connect(self._on_synthesizing)
        
    def txt_to_audio(self,msg:tuple[str, dict]):
        msg_text, textevent = msg
        ref_file = textevent.get('tts', {}).get('ref_file',self.opt.REF_FILE)
        self.speech_config.speech_synthesis_voice_name = ref_file
        # Tao synthesizer moi theo cau hinh nguoi noi moi co the cham, nhung de giu tuong thich code ta khong dieu chinh lon o day
        # self.speech_synthesizer = speechsdk.SpeechSynthesizer(speech_config=self.speech_config, audio_config=None)
        # self.speech_synthesizer.synthesizing.connect(self._on_synthesizing)

        result=self.speech_synthesizer.speak_text(msg_text)

        # Chi so do tre
        fb_latency = int(result.properties.get_property(
            speechsdk.PropertyId.SpeechServiceResponse_SynthesisFirstByteLatencyMs
        ))
        fin_latency = int(result.properties.get_property(
            speechsdk.PropertyId.SpeechServiceResponse_SynthesisFinishLatencyMs
        ))
        logger.info(f"Lien quan den sinh am thanh Azure: do tre byte dau tien: {fb_latency} ms, do tre hoan tat: {fin_latency} ms, result_id: {result.result_id}")

    # === Callback ===
    def _on_synthesizing(self, evt: speechsdk.SpeechSynthesisEventArgs):
        if evt.result.reason == speechsdk.ResultReason.SynthesizingAudioCompleted:
            logger.info("SynthesizingAudioCompleted")
        elif evt.result.reason == speechsdk.ResultReason.Canceled:
            cancellation_details = evt.result.cancellation_details
            logger.info(f"Speech synthesis canceled: {cancellation_details.reason}")
            if cancellation_details.reason == speechsdk.CancellationReason.Error:
                if cancellation_details.error_details:
                    logger.info(f"Error details: {cancellation_details.error_details}")        
        if self.state != State.RUNNING:
            self.audio_buffer = b''
            return

        # evt.result.audio_data la mot doan PCM thuan moi den
        self.audio_buffer += evt.result.audio_data
        while len(self.audio_buffer) >= self.CHUNK_SIZE:
            chunk = self.audio_buffer[:self.CHUNK_SIZE]
            self.audio_buffer = self.audio_buffer[self.CHUNK_SIZE:]

            frame = (np.frombuffer(chunk, dtype=np.int16)
                       .astype(np.float32) / 32767.0)
            self.parent.put_audio_frame(frame)
