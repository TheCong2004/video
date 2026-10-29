###############################################################################
#  Lớp cơ sở cho chế độ đầu ra — giao diện xuất video/âm thanh
###############################################################################

from abc import ABC, abstractmethod
from typing import TYPE_CHECKING, Optional
from numpy.typing import NDArray
import numpy as np

if TYPE_CHECKING:
    from avatars.base_avatar import BaseAvatar


class BaseOutput(ABC):
    """
    Lớp cơ sở trừu tượng cho chế độ truyền đầu ra.
    
    Phần triển khai cần:
    1. start(): khởi động đầu ra
    2. push_video_frame(): đẩy khung hình video
    3. push_audio_frame(): đẩy khung hình âm thanh
    4. stop(): đóng đầu ra
    """

    def __init__(self, opt=None, parent: Optional['BaseAvatar'] = None, **kwargs):
        self.opt = opt
        self.parent = parent

    @abstractmethod
    def start(self) -> None:
        """Khởi động kênh đầu ra"""
        ...

    @abstractmethod
    def push_video_frame(self, frame) -> None:
        """Đẩy khung hình video"""
        ...

    @abstractmethod
    def push_audio_frame(self, frame:NDArray[np.int16], eventpoint=None) -> None:
        """Đẩy khung hình âm thanh"""
        ...


    def get_buffer_size(self) -> int:
        """Lấy số khung bị dồn trong hàng đợi gửi bên dưới để giảm tốc và giới hạn lưu lượng"""
        return 0

    @abstractmethod
    def stop(self) -> None:
        """Đóng kênh đầu ra"""
        ...
