###############################################################################
#  Trình quản lý phiên toàn cục (Session Manager)
###############################################################################

import asyncio
import uuid
from typing import Dict, Optional
from utils.logger import logger
from avatars.base_avatar import BaseAvatar

def _rand_session_id() -> str:
    """Tạo session ID dạng UUID"""
    return str(uuid.uuid4())

class SessionManager:
    """
    Trình quản lý phiên digital human toàn cục.
    
    Quản lý thống nhất vòng đời avatar_sessions và vẫn giữ dịch vụ hoạt động khi tách khỏi WebRTC.
    """
    _instance = None
    
    def __new__(cls, *args, **kwargs):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance

    def __init__(self):
        if not hasattr(self, "initialized"):
            self.sessions: Dict[str, BaseAvatar] = {}
            self.build_session_fn = None
            self.initialized = True

    def init_builder(self, build_session_fn):
        """Cấu hình hàm factory để tạo avatar_session"""
        self.build_session_fn = build_session_fn
        
    def get_latest_session(self) -> Optional[BaseAvatar]:
        """Lấy session đang hoạt động mới nhất"""
        for session in reversed(list(self.sessions.values())):
            if session is not None:
                return session
        return None

    def get_session(self, sessionid: str) -> Optional[BaseAvatar]:
        """Lấy session còn sống hoặc fallback về session mới nhất nếu hợp lệ"""
        if sessionid and sessionid not in ('current', '0', 'None', ''):
            s = self.sessions.get(sessionid)
            if s is not None:
                return s
        return self.get_latest_session()

    def get_single_session(self) -> Optional[BaseAvatar]:
        """Trả về session đang hoạt động mới nhất"""
        return self.get_latest_session()

    def has_session(self, sessionid: str) -> bool:
        """Kiểm tra session có tồn tại hay không"""
        if sessionid and sessionid not in ('current', '0', 'None', ''):
            return sessionid in self.sessions and self.sessions[sessionid] is not None
        return self.get_latest_session() is not None
        
    async def create_session(self, params: dict, sessionid: str = None) -> str:
        """
        Tạo một session mới trong môi trường bất đồng bộ.
        Nếu sessionid là None thì tự động sinh.
        """
        if self.build_session_fn is None:
            raise Exception("Trình tạo SessionManager chưa được khởi tạo")
            
        if sessionid is None:
            sessionid = _rand_session_id()
            
        logger.info('Creating sessionid=%s, current session num=%d', sessionid, len(self.sessions))
        # Dọn bớt các session cũ không còn sử dụng để tránh rò rỉ bộ nhớ
        for k in list(self.sessions.keys()):
            if self.sessions[k] is None and k != sessionid:
                self.sessions.pop(k, None)

        # Tạo chỗ giữ trước để tránh trùng lặp
        self.sessions[sessionid] = None

        # Xây dựng session trong thread pool (tải mô hình rất tốn thời gian)
        avatar_session = await asyncio.get_event_loop().run_in_executor(
            None, self.build_session_fn, sessionid, params
        )
        self.sessions[sessionid] = avatar_session
        return sessionid
        
    def add_session(self, sessionid: str, avatar_session: BaseAvatar):
        """Thêm đồng bộ session tĩnh hoặc do bên ngoài quản lý (dùng cho điểm vào không phải server)"""
        self.sessions[sessionid] = avatar_session
        
    def remove_session(self, sessionid: str):
        """Hủy tài nguyên session"""
        if sessionid in self.sessions:
            logger.info(f"Removing session {sessionid}")
            self.sessions.pop(sessionid, None)

# Xuất singleton
session_manager = SessionManager()
