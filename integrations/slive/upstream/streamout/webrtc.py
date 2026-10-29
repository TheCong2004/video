###############################################################################
#  Output — đầu ra WebRTC
###############################################################################

from streamout.base_output import BaseOutput
from registry import register
from utils.logger import logger
from typing import TYPE_CHECKING, Optional

if TYPE_CHECKING:
    from avatars.base_avatar import BaseAvatar


@register("streamout", "webrtc")
class WebRTCOutput(BaseOutput):
    """Chế độ đầu ra WebRTC — đẩy âm thanh và video qua aiortc"""

    def __init__(self, opt=None, parent: Optional['BaseAvatar'] = None, **kwargs):
        super().__init__(opt, parent)
        self._player = None

    def start(self) -> None:
        """Đầu ra WebRTC được rtc_manager quản lý, không cần khởi động thêm ở đây"""
        pass

    def push_video_frame(self, frame) -> None:
        if self._player:
            self._player.push_video(frame)

    def push_audio_frame(self, frame, eventpoint=None) -> None:
        if self._player:
            self._player.push_audio(frame, eventpoint)



    def get_buffer_size(self) -> int:
        if self._player and hasattr(self._player, 'get_buffer_size'):
            return self._player.get_buffer_size()
        return 0

    def stop(self) -> None:
        pass
