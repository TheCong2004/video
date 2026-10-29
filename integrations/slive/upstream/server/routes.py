###############################################################################
#  Tuyến máy chủ — các API route có xử lý ngoại lệ thống nhất
###############################################################################

import json
import numpy as np
import asyncio
from threading import Thread, Event
from aiohttp import web
import subprocess
import sys
import os
import uuid
import time
import sys
import shlex
import re
import unicodedata

from utils.logger import logger
import aiohttp
import urllib.parse
from runtime_config import load_config
from runtime_paths import avatar_path, default_avatars_dir, user_avatars_dir, program_dir

# Import TikTok listener
try:
    from live import start_tiktok_listener
except Exception as e:
    logger.warning(f'Could not import TikTok listener: {e}')
    start_tiktok_listener = None

from playlist_manager import playlist_manager
try:
    from utils.crawler import extract_url, crawl_url, parse_product_info
except ImportError:
    extract_url, crawl_url, parse_product_info = None, None, None


# ─── Hàm tiện ích cho route ──────────────────────────────────────────────────────────

def json_ok(data=None, **extra):
    """Trả về phản hồi JSON thành công"""
    body = {"code": 0, "msg": "ok"}
    if data is not None:
        body["data"] = data
    if extra:
        body.update(extra)
    return web.Response(
        content_type="application/json",
        text=json.dumps(body),
    )


def json_error(msg: str, code: int = -1):
    """Trả về phản hồi JSON lỗi"""
    return web.Response(
        content_type="application/json",
        text=json.dumps({"code": code, "msg": str(msg)}),
    )


from server.session_manager import session_manager

def get_session(request, sessionid: str):
    """Lấy thể hiện session từ app"""
    avatar_session = session_manager.get_session(sessionid)
    if avatar_session is not None:
        return avatar_session
    if not sessionid:
        return session_manager.get_single_session()
    return session_manager.get_single_session()


def _stop_continuous_llm(avatar_session):
    quit_event = getattr(avatar_session, "llm_quit_event", None)
    if quit_event is not None:
        quit_event.set()
    avatar_session.llm_quit_event = None


def _normalise_speech_settings(settings):
    settings = settings if isinstance(settings, dict) else {}
    voice = str(settings.get('ref_file') or settings.get('voice') or 'Trúc Ly').strip() or 'Trúc Ly'
    try:
        speed = max(0.8, min(1.6, float(settings.get('speed', 1.0))))
    except (TypeError, ValueError):
        speed = 1.0
    try:
        pause_sec = max(0.1, min(2.0, float(settings.get('pause_between_sentences', 0.3))))
    except (TypeError, ValueError):
        pause_sec = 0.3
    return {'ref_file': voice, 'speed': speed, 'pause_between_sentences': pause_sec}


# ─── Hàm xử lý route ──────────────────────────────────────────────────────────

async def human(request):
    """Nhập văn bản (chế độ echo/chat), hỗ trợ tham số voice/emotion"""
    try:
        params: dict = await request.json()

        sessionid: str = params.get('sessionid', '')
        avatar_session = get_session(request, sessionid)
        if avatar_session is None:
            return json_error("session not found")

        if params.get('interrupt'):
            # Chỉ thực hiện ngắt lời và dừng luồng liên tục nếu đây KHÔNG phải là sự kiện TikTok
            if not params.get('is_tiktok_event'):
                avatar_session.flush_talk()
                _stop_continuous_llm(avatar_session)

        saved_settings = getattr(avatar_session, 'speech_settings', {})
        incoming_settings = params.get('tts') if isinstance(params.get('tts'), dict) else {}
        merged_settings = dict(saved_settings)
        merged_settings.update(incoming_settings)
        normalised_settings = _normalise_speech_settings(merged_settings)
        avatar_session.speech_settings = normalised_settings
        datainfo = {'tts': dict(normalised_settings), 'source': 'live'}
        if params.get('is_tiktok_event'):
            datainfo['source'] = 'tiktok'
        if params.get('skip_refine'):
            datainfo['skip_refine'] = True

        # ZERO-LATENCY INSTANT SPEECH BUFFER:
        # Phát tức thì câu mở đầu chuyển cảnh hoặc kịch bản có sẵn vào hàng đợi TTS (<0.3s)
        # Loại bỏ hoàn toàn khoảng lặng chết (dead-air) trong khi LLM chạy nền sinh tiếp các câu sau
        immediate_speech = str(params.get('immediate_speech') or '').strip()
        if immediate_speech and not params.get('is_tiktok_event'):
            avatar_session.push_chat_message('assistant', immediate_speech)
            avatar_session.put_msg_txt(immediate_speech, datainfo)

        if params['type'] == 'echo':
            is_tiktok_event = params.get('is_tiktok_event', False)
            if is_tiktok_event:
                # TikTok replies use a dedicated FIFO queue. BaseTTS alternates
                # this queue with the livestream script one sentence at a time.
                # The reply is already displayed by /event_message, so it must
                # not be inserted into the livestream topic feed.
                avatar_session.put_msg_txt(params['text'], datainfo, priority=True)
            else:
                # Yêu cầu bình thường từ Dashboard (nếu có interrupt thì đã dọn sạch hàng đợi ở trên)
                avatar_session.push_chat_message('user', params['text'])
                avatar_session.put_msg_txt(params['text'], datainfo)
        elif params['type'] == 'chat':
            avatar_session.push_chat_message('user', params['text'])
            # Save the prompt for TikTok live replies
            avatar_session.custom_prompt = params['text']
            try:
                prompt_path = os.path.join(os.getcwd(), 'live_prompt.txt')
                with open(prompt_path, 'w', encoding='utf-8') as f:
                    f.write(params['text'])
            except Exception as e:
                logger.warning(f"Failed to write live_prompt.txt: {e}")
            if params.get('continuous'):
                llm_continuous = request.app.get("llm_continuous")
                if llm_continuous:
                    _stop_continuous_llm(avatar_session)
                    quit_event = Event()
                    avatar_session.llm_quit_event = quit_event
                    Thread(
                        target=llm_continuous,
                        args=(params['text'], avatar_session, quit_event, datainfo),
                        daemon=True,
                    ).start()
            else:
                llm_response = request.app.get("llm_response")
                if llm_response:
                    asyncio.get_event_loop().run_in_executor(
                        None, llm_response, params['text'], avatar_session, datainfo
                    )

        return json_ok()
    except Exception as e:
        logger.exception('human route exception:')
        return json_error(str(e))


async def event_message(request):
    """Nhận và lưu luồng event TikTok để dashboard hiển thị"""
    try:
        params: dict = await request.json()

        sessionid: str = params.get('sessionid', '')
        avatar_session = get_session(request, sessionid)
        if avatar_session is None:
            return json_error("session not found")

        viewer = str(params.get('viewer', '')).strip()
        comment = str(params.get('comment', '')).strip()
        reply = str(params.get('reply', '')).strip()

        if comment:
            prefix = f'{viewer}: ' if viewer else ''
            avatar_session.push_event_message('user', f'{prefix}{comment}')
        if reply:
            avatar_session.push_event_message('assistant', reply)

        return json_ok()
    except Exception as e:
        logger.exception('event_message exception:')
        return json_error(str(e))


async def chat_messages(request):
    """Lấy các tin nhắn hội thoại mới để hiển thị trên web"""
    try:
        params = await request.json()
        sessionid = params.get('sessionid', '')
        avatar_session = get_session(request, sessionid)
        if avatar_session is None:
            return json_error("session not found")
        return json_ok(data=avatar_session.pop_chat_messages())
    except Exception as e:
        logger.exception('chat_messages exception:')
        return json_error(str(e))


async def event_messages(request):
    """Lấy các sự kiện TikTok mới để hiển thị trên dashboard"""
    try:
        params = await request.json()
        sessionid = params.get('sessionid', '')
        avatar_session = get_session(request, sessionid)
        if avatar_session is None:
            return json_error("session not found")
        return json_ok(data=avatar_session.pop_event_messages())
    except Exception as e:
        logger.exception('event_messages exception:')
        return json_error(str(e))


async def interrupt_talk(request):
    """Ngắt lời đang phát"""
    try:
        params = await request.json()
        sessionid = params.get('sessionid', '')
        avatar_session = get_session(request, sessionid)
        if avatar_session is None:
            return json_error("session not found")
        avatar_session.flush_talk()
        _stop_continuous_llm(avatar_session)
        return json_ok()
    except Exception as e:
        logger.exception('interrupt_talk exception:')
        return json_error(str(e))


async def humanaudio(request):
    """Tải lên tệp âm thanh"""
    try:
        form = await request.post()
        sessionid = str(form.get('sessionid', ''))
        fileobj = form["file"]
        filebytes = fileobj.file.read()

        datainfo = {}

        avatar_session = get_session(request, sessionid)
        if avatar_session is None:
            return json_error("session not found")
        avatar_session.put_audio_file(filebytes, datainfo)
        return json_ok()
    except Exception as e:
        logger.exception('humanaudio exception:')
        return json_error(str(e))


async def set_audiotype(request):
    """Đặt trạng thái tùy chỉnh (dàn dựng hành động)"""
    try:
        params = await request.json()
        sessionid = params.get('sessionid', '')
        avatar_session = get_session(request, sessionid)
        if avatar_session is None:
            return json_error("session not found")
        avatar_session.set_custom_state(params['audiotype'])
        return json_ok()
    except Exception as e:
        logger.exception('set_audiotype exception:')
        return json_error(str(e))


async def record(request):
    """Điều khiển ghi"""
    try:
        params = await request.json()
        sessionid = params.get('sessionid', '')
        avatar_session = get_session(request, sessionid)
        if avatar_session is None:
            return json_error("session not found")
        if params['type'] == 'start_record':
            avatar_session.start_recording()
        elif params['type'] == 'end_record':
            avatar_session.stop_recording()
        return json_ok()
    except Exception as e:
        logger.exception('record exception:')
        return json_error(str(e))


async def is_speaking(request):
    """Truy vấn xem có đang nói hay không"""
    params = await request.json()
    sessionid = params.get('sessionid', '')
    avatar_session = get_session(request, sessionid)
    if avatar_session is None:
        return json_error("session not found")
    return json_ok(data=avatar_session.is_speaking())


async def speech_status(request):
    """Trạng thái câu đang thực sự phát qua pipeline WebRTC."""
    try:
        params = await request.json()
        avatar_session = get_session(request, params.get('sessionid', ''))
        if avatar_session is None:
            return json_error('session not found')
        return json_ok(data=avatar_session.get_speech_status())
    except Exception as exc:
        logger.exception('speech_status exception:')
        return json_error(str(exc))


async def speech_settings(request):
    """Lưu cấu hình giọng/tốc độ cho cả nội dung LLM và phản hồi TikTok."""
    try:
        params = await request.json()
        avatar_session = get_session(request, params.get('sessionid', ''))
        if avatar_session is None:
            return json_error('session not found')
        settings = _normalise_speech_settings(params)
        avatar_session.speech_settings = settings
        tts_instance = getattr(avatar_session, 'tts', None)
        for tts_queue in (
            getattr(tts_instance, 'msgqueue', None),
            getattr(tts_instance, 'priority_queue', None),
        ):
            if tts_queue is None:
                continue
            with tts_queue.mutex:
                queued = []
                for text, datainfo in list(tts_queue.queue):
                    updated = dict(datainfo or {})
                    updated['tts'] = dict(settings)
                    queued.append((text, updated))
                tts_queue.queue.clear()
                tts_queue.queue.extend(queued)
        return json_ok(data=settings)
    except Exception as exc:
        logger.exception('speech_settings exception:')
        return json_error(str(exc))


# ─── Quản lý tiến trình TikTok Live Listener ──────────────────────────────
TIKTOK_PROCESS: subprocess.Popen = None
TIKTOK_ACTIVE_USER: str = ""
TIKTOK_LAST_ERROR: str = ""
TIKTOK_LAST_STATUS: str = ""


async def tiktok_report_status(request):
    """Endpoint nhận báo cáo trạng thái và lỗi từ live.py"""
    global TIKTOK_LAST_ERROR, TIKTOK_LAST_STATUS
    try:
        data = await request.json()
        status = data.get('status', '')
        error = data.get('error', '').strip()
        TIKTOK_LAST_STATUS = status
        if error:
            TIKTOK_LAST_ERROR = error
            logger.warning(f"[TikTok Status Reported] {status} - {error}")
    except Exception as e:
        logger.exception("tiktok_report_status error:")
    return json_ok()


def _extract_error_from_tiktok_log(max_lines: int = 150) -> str:
    """Đọc các dòng log cuối cùng của tiktok_listener.log để trích xuất lỗi nếu có."""
    log_path = os.path.join(os.getcwd(), 'tiktok_listener.log')
    if not os.path.isfile(log_path):
        return ""
    try:
        with open(log_path, 'r', encoding='utf-8', errors='ignore') as f:
            lines = f.readlines()[-max_lines:]
        content = "".join(lines)
        if "SIGN_NOT_200" in content or "Failed request to Sign API" in content or "status code 500" in content:
            return "Máy chủ chữ ký TikTok (Sign API) gặp lỗi HTTP 500. Vui lòng bấm KẾT NỐI lại để thử tạo phiên mới."
        for line in reversed(lines):
            stripped = line.strip()
            if not stripped:
                continue
            # Bỏ qua các cảnh báo dọn dẹp nội bộ của asyncio trên Windows
            if "_cancel_overlapped" in stripped or "Task was destroyed but it is pending" in stripped:
                continue
            if stripped.startswith("OSError: [WinError 6]") or "The handle is invalid" in stripped:
                continue
            if "SignAPIError" in stripped or "Error starting TikTok listener" in stripped:
                return "Máy chủ chữ ký TikTok (Sign API) gặp lỗi HTTP 500. Vui lòng bấm KẾT NỐI lại."
            if any(err in stripped for err in ["Error:", "Exception:", "Failed:"]):
                return stripped[:160]
        if "The handle is invalid" in content or "WinError 6" in content:
            return "Đường truyền kết nối TikTok Live bị đóng bất ngờ."
    except Exception:
        pass
    return ""


def parse_tiktok_username(value: str) -> str:
    """Parse and normalize TikTok username or live link into a clean unique username."""
    raw = unicodedata.normalize('NFC', value or '').strip()
    if not raw:
        return ''
    raw = raw.split('?', 1)[0].split('#', 1)[0].rstrip('/')
    raw = re.sub(r'/live/?$', '', raw, flags=re.IGNORECASE)
    match = re.search(r'@([A-Za-z0-9_.]+)', raw)
    if match:
        username = match.group(1)
    else:
        username = raw
    if '/' in username:
        username = username.rsplit('/', 1)[-1]
    username = username.lstrip('@').strip()
    if not re.fullmatch(r'[A-Za-z0-9_.]{2,24}', username):
        return ''
    return username


def friendly_tiktok_error(error_info, username=None):
    if isinstance(error_info, str):
        try:
            error_info = json.loads(error_info)
        except Exception:
            error_info = {'code': '', 'message': error_info}
    error_info = error_info or {}
    code = str(error_info.get('code') or '').strip()
    msg = str(error_info.get('message') or '').strip()
    user_str = f"@{username}" if username else "tài khoản"

    if 'USER_NOT_FOUND' in code or 'UserNotFoundError' in msg:
        return f"Không tìm thấy tài khoản TikTok {user_str}. Vui lòng kiểm tra lại chính xác username."
    if 'USER_OFFLINE' in code or 'UserOfflineError' in msg:
        return f"Tài khoản {user_str} hiện không phát trực tiếp (Live). Hãy chắc chắn bạn đã bắt đầu live trên TikTok."
    if 'SIGN_NOT_200' in code or 'Sign API' in msg or '500' in msg:
        return "Máy chủ chữ ký TikTok đang quá tải hoặc gặp lỗi. Vui lòng bấm KẾT NỐI lại sau vài giây."
    if 'NETWORK' in code or 'Timeout' in msg or 'Connection' in msg:
        return "Lỗi kết nối mạng đến TikTok. Vui lòng kiểm tra đường truyền Internet và thử lại."
    return msg or "Không thể kết nối đến TikTok Live. Vui lòng thử lại."


async def tiktok_disconnect(request):
    """Ngắt tiến trình TikTok Live listener và đồng bộ lên backend trung tâm"""
    global TIKTOK_PROCESS, TIKTOK_ACTIVE_USER, TIKTOK_LAST_ERROR, TIKTOK_LAST_STATUS
    try:
        if TIKTOK_PROCESS is not None:
            try:
                TIKTOK_PROCESS.terminate()
                try:
                    TIKTOK_PROCESS.wait(timeout=1.5)
                except Exception:
                    TIKTOK_PROCESS.kill()
            except Exception as e:
                logger.warning(f"Error terminating tiktok process: {e}")
            TIKTOK_PROCESS = None
        TIKTOK_ACTIVE_USER = ""
        TIKTOK_LAST_ERROR = ""
        TIKTOK_LAST_STATUS = "disconnected"

        # Đồng bộ trạng thái ngắt kênh lên backend trung tâm
        auth = request.app.get('central_auth')
        if auth and hasattr(auth, 'update_tiktok_channel'):
            try:
                asyncio.create_task(auth.update_tiktok_channel(channel="", action="disconnect"))
            except Exception as sync_err:
                logger.warning(f"Could not disconnect TikTok channel on central backend: {sync_err}")

        logger.info("TikTok listener disconnected successfully.")
        return json_ok(data={'status': 'disconnected'})
    except Exception as e:
        logger.exception("tiktok_disconnect error:")
        return json_error(str(e))


async def tiktok_status(request):
    """Kiểm tra trạng thái kết nối TikTok Live và phát hiện lỗi nếu tiến trình bị sập"""
    global TIKTOK_PROCESS, TIKTOK_ACTIVE_USER, TIKTOK_LAST_ERROR, TIKTOK_LAST_STATUS
    is_alive = False
    error = ""
    if TIKTOK_PROCESS is not None:
        returncode = TIKTOK_PROCESS.poll()
        if returncode is None:
            is_alive = True
        else:
            # Tiến trình đã thoát (bị lỗi hoặc crash)
            is_alive = False
            detected_error = TIKTOK_LAST_ERROR or _extract_error_from_tiktok_log()
            error = detected_error or f"Tiến trình TikTok Live đã dừng bất ngờ (mã thoát: {returncode})."
            TIKTOK_LAST_ERROR = error
            TIKTOK_PROCESS = None
            logger.warning(f"TikTok process exited (code {returncode}). Error: {error}")
    else:
        if TIKTOK_LAST_ERROR:
            error = TIKTOK_LAST_ERROR
            TIKTOK_LAST_ERROR = ""

    friendly_err = friendly_tiktok_error(error, username=TIKTOK_ACTIVE_USER) if error else ""

    return json_ok(data={
        'connected': is_alive,
        'username': TIKTOK_ACTIVE_USER,
        'error': friendly_err,
        'status': 'connected' if is_alive else (TIKTOK_LAST_STATUS or 'disconnected')
    })


async def tiktok_connect(request):
    """Kết nối đến TikTok Live với username được chỉ định và đồng bộ lên backend trung tâm"""
    global TIKTOK_PROCESS, TIKTOK_ACTIVE_USER, TIKTOK_LAST_ERROR, TIKTOK_LAST_STATUS
    try:
        params: dict = await request.json()
        
        raw_username = params.get('username', '').strip()
        sessionid = params.get('sessionid', 'current')
        
        if not raw_username:
            return json_error("Hãy nhập username hoặc link TikTok Live.")

        clean_username = parse_tiktok_username(raw_username)
        if not clean_username:
            return json_error("Username hoặc link TikTok không hợp lệ. Vui lòng kiểm tra lại.")

        tiktok_username = '@' + clean_username

        # Xóa lỗi cũ khi bắt đầu kết nối mới
        TIKTOK_LAST_ERROR = ""
        TIKTOK_LAST_STATUS = "connecting"

        # Dừng tiến trình cũ nếu đang chạy
        if TIKTOK_PROCESS is not None:
            try:
                TIKTOK_PROCESS.terminate()
                try:
                    TIKTOK_PROCESS.wait(timeout=1.5)
                except Exception:
                    TIKTOK_PROCESS.kill()
            except Exception:
                pass
            TIKTOK_PROCESS = None
            TIKTOK_ACTIVE_USER = ""

        # Khởi chạy tiến trình lắng nghe TikTok Live trong nền
        if getattr(sys, 'frozen', False):
            try:
                args = [sys.executable, '--run-tiktok', tiktok_username, sessionid]
                log_path = os.path.join(os.getcwd(), 'tiktok_listener.log')
                log_file = open(log_path, 'a', encoding='utf-8')
                proc = subprocess.Popen(args, cwd=os.getcwd(), stdout=log_file, stderr=log_file)
                TIKTOK_PROCESS = proc
                TIKTOK_ACTIVE_USER = clean_username
                logger.info(f'Spawned TikTok listener process (frozen) for {tiktok_username} with sessionid {sessionid}')
            except Exception as e:
                logger.exception('Failed to spawn TikTok listener (frozen):')
                return json_error(str(e))
        else:
            live_py = os.path.join(os.getcwd(), 'live.py')
            if os.path.exists(live_py):
                try:
                    args = [sys.executable, live_py, tiktok_username, sessionid]
                    log_path = os.path.join(os.getcwd(), 'tiktok_listener.log')
                    log_file = open(log_path, 'a', encoding='utf-8')
                    env = os.environ.copy()
                    env["TIKTOK_USER"] = tiktok_username
                    server_port = str(request.url.port or 8010)
                    env["LIVE_APP_URL"] = f"http://127.0.0.1:{server_port}"
                    env['PYTHONIOENCODING'] = 'utf-8'
                    env['PYTHONUTF8'] = '1'
                    env['PYTHONUNBUFFERED'] = '1'
                    proc = subprocess.Popen(args, cwd=os.getcwd(), env=env, stdout=log_file, stderr=log_file)
                    TIKTOK_PROCESS = proc
                    TIKTOK_ACTIVE_USER = clean_username
                    logger.info(f'Spawned live.py process for {tiktok_username} on port {server_port} with sessionid {sessionid}')
                except Exception as e:
                    logger.exception('Failed to spawn live.py process:')
                    return json_error(str(e))
            elif start_tiktok_listener is not None:
                thread = Thread(
                    target=start_tiktok_listener,
                    args=(tiktok_username, sessionid),
                    daemon=True,
                )
                thread.start()
                TIKTOK_ACTIVE_USER = clean_username
                logger.info(f'Started TikTok listener thread for {tiktok_username} with sessionid {sessionid}')
            else:
                return json_error("TikTok listener not available")

        # Đồng bộ kênh TikTok lên backend trung tâm & trang admin
        auth = request.app.get('central_auth')
        if auth and hasattr(auth, 'update_tiktok_channel'):
            try:
                asyncio.create_task(auth.update_tiktok_channel(channel=clean_username))
            except Exception as sync_err:
                logger.warning(f"Could not sync TikTok channel to central backend: {sync_err}")

        return json_ok(data={'status': 'connecting', 'username': clean_username, 'tiktokChannel': clean_username})
    except Exception as e:
        logger.exception('tiktok_connect exception:')
        return json_error(str(e))


async def avatars_list(request):
    names = set()
    for root in (default_avatars_dir(), user_avatars_dir(), program_dir() / "data" / "avatars"):
        if not root.is_dir():
            continue
        names.update(
            item.name for item in root.iterdir()
            if (item.is_dir() and not item.name.startswith('.')
                and (item / 'coords.pkl').is_file()
                and (item / 'full_imgs').is_dir()
                and (item / 'face_imgs').is_dir())
        )
    avatar_list = sorted(names)
    # Giữ đúng 1 mục Webcam trực tiếp ở CUỐI CÙNG danh sách mẫu live theo yêu cầu
    avatar_list.append("webcam")
    return json_ok(avatar_list)


async def cameras_list_route(request):
    """API trả về danh sách camera khả dụng trên thiết bị."""
    from utils.camera_detector import get_available_cameras
    force = request.query.get('refresh', '').lower() in ('1', 'true', 'yes')
    cameras = get_available_cameras(force_refresh=force)
    return json_ok(cameras)


async def camera_stream_route(request):
    """
    Stream MJPEG thời gian thực của Camera gốc (Raw Camera Input Monitor).
    Hình ảnh sạch 100%, không bị AI đè miệng hay can thiệp.
    """
    from avatars.webcam_stream import get_active_or_preview_webcam
    cam_index = request.query.get('cam_id', None)
    try:
        cam_index = int(cam_index) if cam_index is not None else None
    except (ValueError, TypeError):
        cam_index = None

    response = web.StreamResponse(
        status=200,
        reason='OK',
        headers={
            'Content-Type': 'multipart/x-mixed-replace; boundary=frame',
            'Cache-Control': 'no-cache, no-store, must-revalidate',
            'Pragma': 'no-cache',
            'Expires': '0',
            'Access-Control-Allow-Origin': '*',
        }
    )
    await response.prepare(request)

    webcam = get_active_or_preview_webcam(cam_index)
    try:
        while True:
            jpeg_bytes = webcam.get_jpeg_frame(quality=70)
            if jpeg_bytes:
                header = (
                    b'--frame\r\n'
                    b'Content-Type: image/jpeg\r\n'
                    b'Content-Length: ' + str(len(jpeg_bytes)).encode('ascii') + b'\r\n\r\n'
                )
                await response.write(header + jpeg_bytes + b'\r\n')
            await asyncio.sleep(0.033)  # ~30 FPS
    except (asyncio.CancelledError, ConnectionResetError, web.HTTPException):
        pass
    except Exception as e:
        logger.debug(f"camera_stream_route disconnected: {e}")
    finally:
        try:
            await response.write_eof()
        except Exception:
            pass
    return response


async def camera_snapshot_route(request):
    """Trả về 1 frame JPEG tĩnh của Camera gốc"""
    from avatars.webcam_stream import get_active_or_preview_webcam
    cam_index = request.query.get('cam_id', None)
    try:
        cam_index = int(cam_index) if cam_index is not None else None
    except (ValueError, TypeError):
        cam_index = None

    webcam = get_active_or_preview_webcam(cam_index)
    jpeg_bytes = webcam.get_jpeg_frame(quality=80)
    if jpeg_bytes:
        return web.Response(
            body=jpeg_bytes,
            content_type="image/jpeg",
            headers={
                'Cache-Control': 'no-cache, no-store, must-revalidate',
                'Access-Control-Allow-Origin': '*',
            }
        )
    return json_error("Khong the lay frame hinh tu camera")


async def camera_release_route(request):
    """Giải phóng preview camera khi chuyển sang avatar có sẵn"""
    from avatars.webcam_stream import release_preview_webcam
    release_preview_webcam()
    return json_ok()


async def avatar_preview(request):
    """Trả về ảnh thumbnail/snapshot frame đầu tiên của avatar"""
    avatar_id = request.match_info.get('avatar_id', '').strip()
    if not avatar_id or avatar_id.lower() in ('webcam', 'camera', 'livecam') or avatar_id.lower().startswith('webcam'):
        cam_title = "WEBCAM LIVE"
        m = re.search(r'webcam[_\-:]?(\d+)', avatar_id.lower())
        if m:
            cam_title = f"CAMERA {m.group(1)}"
        svg_content = (
            '<svg xmlns="http://www.w3.org/2000/svg" width="360" height="640" viewBox="0 0 360 640">'
            '<rect width="100%" height="100%" fill="#060c1c"/>'
            '<circle cx="180" cy="280" r="50" fill="none" stroke="#00f0ff" stroke-width="4"/>'
            '<circle cx="180" cy="280" r="20" fill="#00f0ff" opacity="0.8"/>'
            f'<text x="180" y="380" text-anchor="middle" fill="#00f0ff" font-family="sans-serif" font-size="20" font-weight="bold">{cam_title}</text>'
            '<text x="180" y="410" text-anchor="middle" fill="#64748b" font-family="sans-serif" font-size="13">Camera trực tiếp</text>'
            '</svg>'
        )
        return web.Response(text=svg_content, content_type="image/svg+xml")

    av_dir = avatar_path(avatar_id)
    first_frame = av_dir / 'full_imgs' / '00000000.png'
    if first_frame.is_file():
        return web.FileResponse(first_frame)
    
    full_imgs_dir = av_dir / 'full_imgs'
    if full_imgs_dir.is_dir():
        imgs = list(full_imgs_dir.glob('*.png')) + list(full_imgs_dir.glob('*.jpg'))
        if imgs:
            return web.FileResponse(sorted(imgs)[0])

    preview_file = av_dir / 'preview.jpg'
    if preview_file.is_file():
        return web.FileResponse(preview_file)

    video_file = av_dir / 'video.mp4'
    if not video_file.is_file():
        mp4s = list(av_dir.glob('*.mp4'))
        if mp4s:
            video_file = mp4s[0]
    if video_file.is_file():
        try:
            import cv2
            cap = cv2.VideoCapture(str(video_file))
            ret, frame = cap.read()
            cap.release()
            if ret and frame is not None:
                cv2.imwrite(str(preview_file), frame)
                return web.FileResponse(preview_file)
        except Exception as e:
            logger.warning(f"Failed to extract preview frame: {e}")

    svg_content = (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="360" height="640" viewBox="0 0 360 640">'
        f'<rect width="100%" height="100%" fill="#070e1f"/>'
        f'<text x="180" y="310" text-anchor="middle" fill="#00f0ff" font-family="sans-serif" font-size="24" font-weight="bold">{avatar_id}</text>'
        f'<text x="180" y="345" text-anchor="middle" fill="#64748b" font-family="sans-serif" font-size="14">Avatar Model</text>'
        f'</svg>'
    )
    return web.Response(text=svg_content, content_type="image/svg+xml")


async def avatar_video(request):
    """Trả về file video mp4 của avatar để phát trên tab xem video mẫu live"""
    avatar_id = request.match_info.get('avatar_id', '').strip()
    if not avatar_id or avatar_id.lower().startswith('webcam'):
        return web.Response(status=404, text="Chế độ camera trực tiếp.")

    av_dir = avatar_path(avatar_id)
    video_file = av_dir / 'video.mp4'
    if not video_file.is_file():
        mp4s = list(av_dir.glob('*.mp4'))
        if mp4s:
            video_file = mp4s[0]

    if video_file.is_file():
        return web.FileResponse(video_file, headers={
            'Accept-Ranges': 'bytes',
            'Content-Type': 'video/mp4',
            'Cache-Control': 'public, max-age=86400',
        })
    # Fallback trả về ảnh nếu mẫu live chưa có file mp4
    return await avatar_preview(request)


async def select_avatar(request):
    params = await request.json()
    avatar_id = str(params.get('avatar_id') or '').strip()
    if not avatar_id or os.path.basename(avatar_id) != avatar_id:
        return json_error('avatar_id không hợp lệ')

    # Không đổi avatar trực tiếp khi đang Live để tránh méo hình ảnh
    # Áp dụng cho phiên Live tiếp theo
    switched_now = False

    if avatar_id.lower() in ('webcam', 'camera', 'livecam') or avatar_id.lower().startswith('webcam'):
        return json_ok({'avatar_id': avatar_id, 'applyOnNextSession': not switched_now, 'switchedNow': switched_now})
    
    try:
        from avatars.webcam_stream import release_preview_webcam
        release_preview_webcam()
    except Exception:
        pass
    
    avatar_dir = avatar_path(avatar_id)
    if not ((avatar_dir / 'coords.pkl').is_file()
            and (avatar_dir / 'full_imgs').is_dir()
            and (avatar_dir / 'face_imgs').is_dir()):
        if not switched_now:
            return json_error('avatar không tồn tại')
            
    return json_ok({'avatar_id': avatar_id, 'applyOnNextSession': not switched_now, 'switchedNow': switched_now})


async def upload_avatar(request):
    reader = await request.multipart()
    avatar_id = ''
    written = 0
    async for part in reader:
        if part.name == 'avatar_id':
            avatar_id = (await part.text()).strip()
        elif part.name == 'file' and part.filename:
            if not avatar_id or os.path.basename(avatar_id) != avatar_id or not avatar_id.replace('-', '').replace('_', '').isalnum():
                return json_error('avatar_id phải được gửi trước file và phải hợp lệ')
            root = str(user_avatars_dir().resolve())
            target = str((user_avatars_dir() / avatar_id).resolve())
            relative = str(part.filename).replace('\\', '/').split('/', 1)[-1]
            parts = [item for item in relative.split('/') if item not in ('', '.', '..')]
            if not parts or os.path.commonpath([root, target]) != root:
                continue
            destination = os.path.abspath(os.path.join(target, *parts))
            if os.path.commonpath([target, destination]) != target:
                continue
            os.makedirs(os.path.dirname(destination), exist_ok=True)
            with open(destination, 'wb') as output:
                while True:
                    chunk = await part.read_chunk(size=1024 * 1024)
                    if not chunk:
                        break
                    output.write(chunk)
            written += 1
    return json_ok({'avatar_id': avatar_id, 'files': written})


# Simple in-memory task store for avatar tasks
AVATAR_TASKS = {}
AVATAR_TASKS_LOCK = asyncio.Lock()


async def api_avatar_task(request):
    """Nhận video nguồn và đưa tác vụ tiền xử lý Wav2Lip vào hàng đợi."""
    try:
        form = await request.post()
        model = str(form.get('model', 'wav2lip')).strip().lower()
        avatar_id = str(form.get('avatar_id', '')).strip()
        fileobj = form.get('video_file') or form.get('file')

        if not fileobj:
            return json_error('video_file is required')
        if not avatar_id:
            avatar_id = f"avatar_{int(time.time())}"
        if (os.path.basename(avatar_id) != avatar_id
                or not avatar_id.replace('-', '').replace('_', '').isalnum()):
            return json_error('Avatar ID chỉ được dùng chữ không dấu, số, dấu - hoặc _.')
        if model != 'wav2lip':
            return json_error('Bản desktop hiện hỗ trợ tạo avatar bằng Wav2Lip.')

        try:
            img_size = max(64, min(1024, int(form.get('img_size', 384))))
            face_det_batch_size = max(1, min(32, int(form.get('face_det_batch_size', 4))))
            pads = [int(value) for value in str(form.get('pads', '0 10 0 0')).split()]
            if len(pads) != 4 or any(value < 0 or value > 500 for value in pads):
                raise ValueError
        except (TypeError, ValueError):
            return json_error('Tham số img_size, face batch size hoặc pads không hợp lệ.')

        avatar_dir = str(avatar_path(avatar_id, for_write=True))
        if avatar_path(avatar_id).exists():
            return json_error('Avatar ID đã tồn tại. Hãy chọn một ID khác.')
        os.makedirs(avatar_dir)

        file_path = os.path.join(avatar_dir, 'video.mp4')
        with open(file_path, 'wb') as f:
            f.write(fileobj.file.read())
        if os.path.getsize(file_path) == 0:
            os.remove(file_path)
            os.rmdir(avatar_dir)
            return json_error('Video tải lên rỗng.')

        task_id = str(uuid.uuid4())
        task = {
            'task_id': task_id,
            'model_type': model,
            'avatar_id': avatar_id,
            'file_path': file_path,
            'status': 'pending',
            'progress': 0,
            'start_time': int(time.time()),
            'error_msg': '',
            'result': '',
            'img_size': img_size,
            'face_det_batch_size': face_det_batch_size,
            'pads': pads,
        }

        async with AVATAR_TASKS_LOCK:
            AVATAR_TASKS[task_id] = task

        logger.info(f"Avatar upload queued: {task_id} -> {file_path}")
        return json_ok(data={'task_id': task_id})
    except Exception as e:
        logger.exception('api_avatar_task exception:')
        return json_error(str(e))



async def api_avatar_tasks(request):
    try:
        async with AVATAR_TASKS_LOCK:
            tasks = list(AVATAR_TASKS.values())
        return json_ok(data={'tasks': tasks})
    except Exception as e:
        logger.exception('api_avatar_tasks exception:')
        return json_error(str(e))


async def api_avatar_task_delete(request):
    try:
        taskid = request.match_info.get('taskid')
        async with AVATAR_TASKS_LOCK:
            task = AVATAR_TASKS.get(taskid)
            if not task:
                return json_error('task not found')
            if task.get('status') != 'pending':
                return json_error('only pending tasks can be deleted')
            task['status'] = 'cancelled'
        return json_ok()
    except Exception as e:
        logger.exception('api_avatar_task_delete exception:')
        return json_error(str(e))


async def avatar_worker(app):
    """Background worker that picks pending avatar tasks and runs the corresponding genavatar scripts."""
    logger.info('Avatar worker started')
    try:
        while True:
            # find a pending task
            task = None
            async with AVATAR_TASKS_LOCK:
                for t in AVATAR_TASKS.values():
                    if t.get('status') == 'pending':
                        task = t
                        task_id = t['task_id']
                        t['status'] = 'running'
                        t['progress'] = 1
                        break

            if task is None:
                await asyncio.sleep(1.0)
                continue

            try:
                model = task.get('model_type', 'wav2lip')
                avatar_id = task.get('avatar_id') or 'liveai'
                video_path = task.get('file_path')

                # choose script and arguments based on model
                base = os.path.join(os.getcwd(), 'avatars')
                # Run as a module so package imports like `avatars.xxx` work
                if model.startswith('wav2lip'):
                    mod = 'avatars.wav2lip.genavatar'
                    generator_args = [
                        '--img_size', str(task.get('img_size', 384)),
                        '--face_det_batch_size', str(task.get('face_det_batch_size', 4)),
                        '--pads', *[str(value) for value in task.get('pads', [0, 10, 0, 0])],
                    ]
                    if getattr(sys, 'frozen', False):
                        cmd = [sys.executable, '--run-genavatar', video_path, avatar_id, *generator_args]
                    else:
                        cmd = [sys.executable, '-m', mod, '--video_path', video_path,
                               '--avatar_id', avatar_id, *generator_args]
                elif model.startswith('musetalk'):
                    mod = 'avatars.musetalk.genavatar'
                    cmd = [sys.executable, '-m', mod, '--file', video_path, '--avatar_id', avatar_id]
                elif model.startswith('ultralight'):
                    mod = 'avatars.ultralight.genavatar'
                    cmd = [sys.executable, '-m', mod, '--video_path', video_path, '--avatar_id', avatar_id]
                else:
                    # unknown model, mark failed
                    async with AVATAR_TASKS_LOCK:
                        AVATAR_TASKS[task_id]['status'] = 'error'
                        AVATAR_TASKS[task_id]['error_msg'] = f'unknown model: {model}'
                    continue

                logger.info(f'Running avatar task {task_id}: {shlex.join(cmd)}')

                # run subprocess and stream output
                proc = await asyncio.create_subprocess_exec(*cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.STDOUT, cwd=os.getcwd())
                last_update = 1
                # read output lines and update progress heuristically
                while True:
                    line = await proc.stdout.readline()
                    if not line:
                        break
                    decoded = line.decode(errors='ignore').strip()
                    logger.info(f'[avatar {task_id}] {decoded}')
                    progress_match = re.search(r'PROGRESS:\s*(\d+)', decoded, re.IGNORECASE)
                    if progress_match:
                        last_update = max(last_update, min(99, int(progress_match.group(1))))
                        async with AVATAR_TASKS_LOCK:
                            AVATAR_TASKS[task_id]['progress'] = last_update
                    stage_match = re.search(r'STAGE:\s*(.+)', decoded, re.IGNORECASE)
                    if stage_match:
                        stage_text = stage_match.group(1).strip()
                        async with AVATAR_TASKS_LOCK:
                            AVATAR_TASKS[task_id]['stage'] = stage_text

                returncode = await proc.wait()
                if returncode == 0:
                    result_dir = str(avatar_path(avatar_id, for_write=True))
                    full_dir = os.path.join(result_dir, 'full_imgs')
                    face_dir = os.path.join(result_dir, 'face_imgs')
                    ready = (os.path.isfile(os.path.join(result_dir, 'coords.pkl'))
                             and os.path.isdir(full_dir) and bool(os.listdir(full_dir))
                             and os.path.isdir(face_dir) and bool(os.listdir(face_dir)))
                    async with AVATAR_TASKS_LOCK:
                        if ready:
                            AVATAR_TASKS[task_id]['status'] = 'done'
                            AVATAR_TASKS[task_id]['progress'] = 100
                            AVATAR_TASKS[task_id]['stage'] = 'Hoàn tất'
                            AVATAR_TASKS[task_id]['result'] = result_dir
                        else:
                            AVATAR_TASKS[task_id]['status'] = 'error'
                            AVATAR_TASKS[task_id]['stage'] = 'Thất bại'
                            AVATAR_TASKS[task_id]['error_msg'] = 'Không tạo đủ dữ liệu avatar.'
                else:
                    async with AVATAR_TASKS_LOCK:
                        AVATAR_TASKS[task_id]['status'] = 'error'
                        AVATAR_TASKS[task_id]['stage'] = 'Thất bại'
                        AVATAR_TASKS[task_id]['error_msg'] = f'exit {returncode}'

            except Exception as e:
                logger.exception('avatar_worker processing exception:')
                async with AVATAR_TASKS_LOCK:
                    AVATAR_TASKS[task_id]['status'] = 'error'
                    AVATAR_TASKS[task_id]['stage'] = 'Thất bại'
                    AVATAR_TASKS[task_id]['error_msg'] = str(e)

    except asyncio.CancelledError:
        logger.info('Avatar worker cancelled')
        raise




async def get_session_prompt(request):
    """Lấy prompt hiện tại của avatar session"""
    try:
        params: dict = await request.json()
        sessionid = params.get('sessionid', '')
        avatar_session = get_session(request, sessionid)
        if avatar_session is None:
            return json_error("session not found")
        prompt = getattr(avatar_session, 'custom_prompt', '')
        return json_ok(data={'prompt': prompt})
    except Exception as e:
        return json_error(str(e))


# ==========================================
# PLAYLIST & TIKTOK AUTO-PIN ROTATION SYSTEM
# ==========================================

def _save_prompt_snapshot(prompt: str) -> None:
    """Atomically update the prompt used by the TikTok comment listener and avatar session."""
    if not prompt:
        return
    try:
        prompt_path = os.path.join(os.getcwd(), 'live_prompt.txt')
        temporary_file = f"{prompt_path}.{uuid.uuid4().hex}.tmp"
        with open(temporary_file, 'w', encoding='utf-8') as handle:
            handle.write(prompt)
        os.replace(temporary_file, prompt_path)
    except Exception as exc:
        try:
            if os.path.exists(temporary_file):
                os.remove(temporary_file)
        except Exception:
            pass
        logger.warning(f"WARN: Failed to save prompt snapshot: {exc}")

    try:
        for s in session_manager.sessions.values():
            if s is not None:
                s.custom_prompt = prompt
    except Exception as e:
        logger.debug(f"Could not update session custom_prompt: {e}")


def run_playlist_rotation_worker():
    """Background scheduler to rotate scripts and auto-pin products on TikTok Shop."""
    while True:
        try:
            state = playlist_manager.get_state()
            cfg = state.get("config", {})
            runtime = state.get("runtime", {})

            # Auto-repin check: Định kỳ 30s kiểm tra sản phẩm hiện tại có đang ghim trên TikTok không, nếu không thì tự ghim lại
            playlist_manager.check_and_repin_active_product()

            # Graceful rotation is handled by frontend when sentence finishes.
            # Backend acts as a safety fallback if overdue by > 20s.
            if cfg.get("auto_rotation") and runtime.get("rotation_running"):
                elapsed = int(time.time() - runtime.get("start_time", 0)) if runtime.get("start_time", 0) > 0 else 0
                active_prod = playlist_manager._get_product_by_id(runtime.get("active_product_id"))
                duration = int(cfg.get("default_duration") or (active_prod.get("duration") if active_prod else 60) or 60)
                if elapsed >= (duration + 20) and len(state.get("products", [])) > 0:
                    next_prod = playlist_manager.get_next_product()
                    if next_prod:
                        logger.info(f"[PLAYLIST SCHEDULER FALLBACK] Switching to next product #{next_prod['index']}: {next_prod['title']}")
                        playlist_manager.select_product(next_prod["id"], trigger_pin=True, reason="rotation")
                        script_text = next_prod.get("script", "")
                        if script_text:
                            _save_prompt_snapshot(f"Tên sản phẩm: {next_prod['title']}. Giá: {next_prod.get('price','')}. Kịch bản: {script_text}")
        except Exception as e:
            logger.warning(f"[PLAYLIST ROTATION ERROR] {e}")
        time.sleep(1)

Thread(target=run_playlist_rotation_worker, daemon=True).start()


async def api_playlist(request):
    """Get the full playlist state and runtime timer."""
    state = playlist_manager.get_state()
    return json_ok(data=state, success=True)


async def api_playlist_config(request):
    """Update playlist rotation settings."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    new_cfg = playlist_manager.update_config(payload)
    return json_ok(config=new_cfg, success=True)


async def api_playlist_save(request):
    """Save full playlist list."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    prods = payload.get('products', [])
    updated = playlist_manager.save_products(prods)
    if 'config' in payload:
        playlist_manager.update_config(payload['config'])
    return json_ok(products=updated, success=True)


async def api_playlist_add(request):
    """Add a product item to playlist."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    item = playlist_manager.add_product(payload)
    return json_ok(product=item, success=True)


async def api_playlist_delete(request):
    """Delete a product item from playlist."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    prod_id = payload.get('id')
    ok = playlist_manager.delete_product(prod_id)
    return json_ok(success=ok)


async def api_playlist_clear(request):
    """Clear all products from playlist."""
    playlist_manager.clear_all()
    return json_ok(message="Đã xóa toàn bộ giỏ hàng", success=True)



async def api_playlist_generate_script(request):
    """Extract product info from link (if provided) and generate selling script."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    title = str(payload.get('title') or '').strip()
    price = str(payload.get('price') or '').strip()
    link = str(payload.get('link') or '').strip()

    crawled_info = {}
    if link and extract_url and crawl_url and parse_product_info:
        try:
            found_url = extract_url(link)
            if found_url:
                html = crawl_url(found_url)
                if html:
                    crawled_info = parse_product_info(html, found_url)
                    if not title and crawled_info.get('title'):
                        title = crawled_info['title']
                    if not price and crawled_info.get('price'):
                        price = crawled_info['price']
        except Exception as e:
            logger.warning(f"[CRAWL SCRIPT ERROR] {e}")

    prod_title = title or "siêu phẩm trong giỏ hàng"
    prod_price = price or "siêu ưu đãi"
    prod_desc = str(crawled_info.get('description') or '').strip()

    # Tạo kịch bản LIVE 2-3 phút chuẩn khung 9 bước
    script = ""
    try:
        from llm import refine_prompt_with_chatgpt
        prompt_req = (
            f"Sản phẩm: {prod_title}\n"
            f"Giá bán ưu đãi: {prod_price}\n"
        )
        if prod_desc:
            prompt_req += f"Thông tin chi tiết: {prod_desc[:300]}\n"
        prompt_req += "Hãy viết kịch bản livestream bán hàng 2-3 phút tuân thủ nghiêm ngặt khung 9 bước."
        ai_script = refine_prompt_with_chatgpt(prompt_req)
        if ai_script and len(ai_script.strip()) > 100:
            script = ai_script.strip()
    except Exception as e:
        logger.warning(f"[SCRIPT GENERATE AI ERROR] {e}")

    if not script:
        # Kịch bản mẫu chuẩn 9 bước 2-3 phút
        script = (
            f"Chào mừng tất cả mọi người đang có mặt trong buổi livestream hôm nay ạ! "
            f"Ai trong số các anh chị ở đây từng gặp rắc rối và mất nhiều thời gian để tìm kiếm một sản phẩm thật sự ưng ý, vừa bền vừa tiện dụng chưa ạ? "
            f"Thực tế rất nhiều người thường mua phải hàng trôi nổi, dùng vài lần là hư hỏng hoặc không đúng như quảng cáo, vừa lãng phí tiền bạc lại vừa mang bực vào người. "
            f"Giải pháp hoàn hảo nhất dành cho anh chị ngay lúc này chính là {prod_title}! "
            f"Sản phẩm này sở hữu ưu điểm vượt trội với độ bền cực cao, tính ứng dụng thực tế tối ưu và tiện lợi hơn hẳn so với các dòng thông thường trên thị trường. "
            f"Anh chị chỉ cần sử dụng theo đúng hướng dẫn là sẽ thấy ngay hiệu quả tuyệt vời và cảm giác hài lòng tối đa. "
            f"Đặc biệt, giá gốc thị trường rất cao nhưng duy nhất trong phiên live này chỉ còn {prod_price}, kèm quà tặng và ưu đãi cực sốc. "
            f"Số lượng ưu đãi có hạn, anh chị hãy bấm ngay vào sản phẩm đang được ghim hoặc giỏ hàng góc trái màn hình, comment ngay để giữ deal giá tốt nhé! "
            f"Ai đang quan tâm hay cần em tư vấn kỹ hơn thì cứ để lại bình luận bên dưới, và em xin cảm ơn các anh chị vừa mới tham gia live. "
            f"Nhắc lại một lần nữa, đây là {prod_title} với giá cực kỳ ưu đãi {prod_price}, mọi người nhanh tay chốt đơn ngay nhé ạ!"
        )

    return json_ok(script=script, title=title, price=price, crawled=bool(crawled_info), success=True)


async def api_playlist_select(request):
    """Select active product and trigger TikTok pin."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    prod_id = payload.get('id')
    trigger_pin = payload.get('trigger_pin', True)
    prod = playlist_manager.select_product(prod_id, trigger_pin=trigger_pin, reason="manual")
    if prod:
        script_text = prod.get("script", "")
        if script_text:
            _save_prompt_snapshot(f"Tên sản phẩm: {prod['title']}. Giá: {prod.get('price','')}. Kịch bản: {script_text}")
        return json_ok(product=prod, success=True)
    return json_error("Product not found", code=404)


async def api_playlist_toggle_rotation(request):
    """Start or stop automated playlist rotation."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    enabled = bool(payload.get('enabled'))
    with playlist_manager.lock:
        playlist_manager.config['auto_rotation'] = enabled
        playlist_manager.runtime_state['rotation_running'] = enabled
        if enabled and not playlist_manager.runtime_state.get('active_product_id'):
            if playlist_manager.products:
                playlist_manager.select_product(playlist_manager.products[0]['id'], trigger_pin=True, reason="rotation")
        playlist_manager._save_data()
    return json_ok(rotation_running=enabled, success=True)


async def api_playlist_next(request):
    """Manually advance to next product in playlist."""
    next_prod = playlist_manager.get_next_product()
    if next_prod:
        prod = playlist_manager.select_product(next_prod["id"], trigger_pin=True, reason="rotation")
        script_text = prod.get("script", "")
        if script_text:
            _save_prompt_snapshot(f"Tên sản phẩm: {prod['title']}. Giá: {prod.get('price','')}. Kịch bản: {script_text}")
        return json_ok(product=prod, success=True)
    return json_error("No products in playlist", code=400)


async def api_playlist_check_comment(request):
    """
    Check if a viewer comment refers to a specific product in playlist.
    If matched and auto_pin_on_comment is enabled:
    - Selects the product in playlist
    - Generates PIN action for TikTok Shop
    - Updates prompt snapshot live_prompt.txt
    - Returns matched product details
    """
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}

    comment = str(payload.get('comment', '')).strip()
    viewer = str(payload.get('viewer', '')).strip()

    cfg = playlist_manager.config
    if not cfg.get("auto_pin_on_comment", True):
        return json_ok(matched=False, reason="feature_disabled")

    matched_prod = playlist_manager.match_product_from_comment(comment)
    if matched_prod:
        is_already_active = (playlist_manager.runtime_state.get("active_product_id") == matched_prod.get("id"))
        prod = playlist_manager.select_product(matched_prod["id"], trigger_pin=True, reason="comment")
        if prod:
            script_text = prod.get("script", "")
            if script_text:
                _save_prompt_snapshot(f"Tên sản phẩm: {prod['title']}. Giá: {prod.get('price','')}. Kịch bản: {script_text}")
        logger.info(f"🎯 [Smart Auto-Pin on Comment] Matched product #{matched_prod.get('index')}: '{matched_prod.get('title')}' for viewer '{viewer}' comment: '{comment}'")
        return json_ok(matched=True, already_active=is_already_active, product=prod or matched_prod)

    return json_ok(matched=False)


def _get_active_product_ext_info():
    state = playlist_manager.get_state()
    cfg = state.get("config", {})
    active_prod = playlist_manager.get_current_speaking_product()
    active_info = None
    if active_prod:
        active_info = {
            "id": active_prod["id"],
            "index": active_prod["index"],
            "title": active_prod["title"],
            "tiktok_product_id": active_prod.get("tiktok_product_id", ""),
            "auto_pin": active_prod.get("auto_pin", True)
        }
    return {
        "active_product": active_info,
        "auto_pin_tiktok": cfg.get("auto_pin_tiktok", True),
        "auto_repin_30s": cfg.get("auto_repin_30s", True),
        "recheck_pin_interval": cfg.get("recheck_pin_interval", 30)
    }


async def api_tiktok_pending_actions(request):
    """Polled by Chrome Extension to receive queued actions (PIN/UNPIN) and active product state."""
    actions = playlist_manager.pop_pending_actions()
    ext_info = _get_active_product_ext_info()
    return json_ok(actions=actions, **ext_info, success=True)


async def api_tiktok_report_status_ext(request):
    """Reported by Chrome Extension periodically to confirm connection."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    switched_prod = playlist_manager.record_extension_heartbeat(payload)
    if switched_prod:
        script_text = switched_prod.get("script", "") or switched_prod.get("title", "")
        _save_prompt_snapshot(f"Tên sản phẩm: {switched_prod['title']}. Giá: {switched_prod.get('price','')}. Kịch bản: {script_text}")
    ext_info = _get_active_product_ext_info()
    return json_ok(**ext_info, success=True)


async def api_tiktok_status_ext(request):
    """Get connection status of TikTok Extension."""
    state = playlist_manager.get_state()
    return json_ok(connected=state['runtime']['extension_connected'], runtime=state['runtime'], success=True)


async def api_tiktok_manual_pin(request):
    """Trigger manual pin action for testing or immediate override."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    idx = int(payload.get('index', 1))
    title = payload.get('title', '')
    prod_id = payload.get('product_id', '')
    target = playlist_manager.select_product(prod_id or idx, trigger_pin=True, reason="manual")
    if target:
        script_text = target.get("script", "") or target.get("title", "")
        _save_prompt_snapshot(f"Tên sản phẩm: {target['title']}. Giá: {target.get('price','')}. Kịch bản: {script_text}")
    else:
        playlist_manager.enqueue_pin_action('PIN', index=idx, product_id=prod_id, title=title)
    return json_ok(success=True, product=target)


async def api_tiktok_sync_cart(request):
    """Received scraped products list from Chrome Extension and save to playlist."""
    try:
        payload = await request.json() if request.can_read_body else {}
    except Exception:
        payload = {}
    scraped_prods = payload.get('products', [])
    allow_empty = bool(payload.get('allow_empty', False))
    synced = playlist_manager.sync_scraped_cart(scraped_prods, allow_empty=allow_empty)
    return json_ok(count=len(synced), products=synced, success=True)


async def api_tiktok_request_sync_cart(request):
    """UI requests Extension to scrape and sync current TikTok Live Cart."""
    playlist_manager.request_sync_cart()
    return json_ok(message='Đã gửi yêu cầu lấy giỏ hàng sang Extension.', success=True)


def get_extension_dir() -> str:
    """Locate the tiktok_pin_extension directory dynamically across dev and installed frozen environments."""
    try:
        from runtime_paths import tiktok_extension_dir
        p = tiktok_extension_dir()
        if p.exists():
            return str(p.resolve())
    except Exception:
        pass
    candidates = []
    if getattr(sys, 'frozen', False):
        exe_dir = os.path.dirname(sys.executable)
        candidates.append(os.path.join(exe_dir, "tiktok_pin_extension"))
        candidates.append(os.path.join(exe_dir, "_internal", "tiktok_pin_extension"))
    candidates.append(os.path.join(os.getcwd(), "tiktok_pin_extension"))
    candidates.append(os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "tiktok_pin_extension"))

    for p in candidates:
        if os.path.exists(p):
            return os.path.abspath(p)
    return os.path.abspath(candidates[0] if candidates else "tiktok_pin_extension")


async def api_tiktok_extension_info(request):
    """Returns the absolute path to tiktok_pin_extension on this machine."""
    ext_dir = get_extension_dir()
    return json_ok(path=ext_dir, exists=os.path.exists(ext_dir), success=True)


async def api_tiktok_open_extension_folder(request):
    """Opens the extension folder in File Explorer so the user can easily load it into Chrome."""
    ext_dir = get_extension_dir()
    if os.path.exists(ext_dir):
        try:
            if sys.platform == 'win32':
                os.startfile(ext_dir)
            elif sys.platform == 'darwin':
                subprocess.Popen(['open', ext_dir])
            else:
                subprocess.Popen(['xdg-open', ext_dir])
            return json_ok(path=ext_dir, success=True)
        except Exception as e:
            return json_error(str(e), code=500)
    return json_error(f'Thư mục không tồn tại: {ext_dir}', code=404)


# ─── TTS Voice Catalog & Clone Voice API ─────────────────────────────────────────────

def _get_tts_server_url() -> str:
    try:
        cfg = load_config()
        return str(cfg.get("tts_server") or "http://127.0.0.1:8006").rstrip("/")
    except Exception:
        return "http://127.0.0.1:8006"


async def api_voices(request):
    """Lấy danh sách giọng đọc mặc định và các giọng đã clone cá nhân từ TTS server."""
    tts_url = f"{_get_tts_server_url()}/v1/voices"
    try:
        async with aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=10)) as session:
            async with session.get(tts_url) as resp:
                if resp.status == 200:
                    data = await resp.json()
                    return web.json_response(data)
                text = await resp.text()
                return json_error(f"TTS service error: {text}", code=resp.status)
    except Exception as exc:
        logger.exception("api_voices exception:")
        return json_error(f"Không thể kết nối dịch vụ TTS ({tts_url}): {exc}", code=503)


async def api_voices_enroll(request):
    """Nhận file âm thanh và thông tin để nạp / clone giọng đọc AI cá nhân mới."""
    tts_url = f"{_get_tts_server_url()}/v1/voices/enroll"
    try:
        reader = await request.multipart()
        form_data = aiohttp.FormData()
        while True:
            part = await reader.next()
            if part is None:
                break
            if part.name == 'file':
                file_bytes = await part.read()
                form_data.add_field(
                    'file', file_bytes,
                    filename=part.filename or 'reference.wav',
                    content_type=part.headers.get(aiohttp.hdrs.CONTENT_TYPE, 'audio/wav')
                )
            else:
                val = await part.text()
                form_data.add_field(part.name, val)
        async with aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=180)) as session:
            async with session.post(tts_url, data=form_data) as resp:
                data = await resp.json()
                return web.json_response(data, status=resp.status)
    except Exception as exc:
        logger.exception("api_voices_enroll exception:")
        return json_error(f"Lỗi nạp giọng: {exc}", code=500)


async def api_voices_delete(request):
    """Xóa một profile giọng cá nhân đã clone."""
    voice_id = request.match_info.get("voice_id", "").strip()
    if not voice_id:
        return json_error("Thiếu voice_id", code=400)
    tts_url = f"{_get_tts_server_url()}/v1/voices/{urllib.parse.quote(voice_id, safe='')}"
    try:
        async with aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=30)) as session:
            async with session.delete(tts_url) as resp:
                data = await resp.json()
                return web.json_response(data, status=resp.status)
    except Exception as exc:
        logger.exception("api_voices_delete exception:")
        return json_error(f"Lỗi xóa giọng: {exc}", code=500)


async def api_voices_test(request):
    """Sinh thử audio phát âm thanh mẫu cho giọng đọc để người dùng nghe thử."""
    tts_url = f"{_get_tts_server_url()}/v1/audio/speech"
    try:
        params = await request.json()
        text = str(params.get("text") or params.get("input") or "").strip()
        voice = str(params.get("voice") or "Trúc Ly").strip()
        speed = float(params.get("speed", 1.0) or 1.0)
        if not text:
            return json_error("Nội dung đọc không được để trống", code=400)
        payload = {"input": text, "voice": voice, "speed": speed}
        async with aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=60)) as session:
            async with session.post(tts_url, json=payload) as resp:
                if resp.status == 200:
                    audio_bytes = await resp.read()
                    return web.Response(body=audio_bytes, content_type="audio/wav")
                text_err = await resp.text()
                return json_error(f"TTS synth error: {text_err}", code=resp.status)
    except Exception as exc:
        logger.exception("api_voices_test exception:")
        return json_error(f"Lỗi thử giọng: {exc}", code=500)


async def api_optimize_prompt(request):
    """Gọi LLM để tối ưu hóa chủ đề thô thành kịch bản bán hàng chi tiết chuẩn 9 bước."""
    try:
        data = await request.json()
        raw_prompt = str(data.get("prompt") or "").strip()
        if not raw_prompt:
            return json_error("Vui lòng nhập nội dung chủ đề cơ bản cần tối ưu.", code=400)
        from llm import refine_prompt_with_chatgpt
        optimized = refine_prompt_with_chatgpt(raw_prompt)
        return json_ok(data={"original": raw_prompt, "optimized": optimized})
    except Exception as e:
        logger.exception("api_optimize_prompt exception:")
        return json_error(str(e), code=500)


async def api_save_prompt(request):
    """Lưu kịch bản tối ưu vào hệ thống để chuẩn bị cho phiên live."""
    try:
        data = await request.json()
        prompt = str(data.get("prompt") or "").strip()
        if not prompt:
            return json_error("Nội dung kịch bản không được để trống.", code=400)
        prompt_path = os.path.join(os.getcwd(), 'live_prompt.txt')
        with open(prompt_path, 'w', encoding='utf-8') as f:
            f.write(prompt)
        sessionid = str(data.get('sessionid') or '').strip()
        if sessionid and sessionid != '0':
            avatar_session = get_session(request, sessionid)
            if avatar_session:
                avatar_session.custom_prompt = prompt
        return json_ok(data={"saved": True, "prompt": prompt})
    except Exception as e:
        logger.exception("api_save_prompt exception:")
        return json_error(str(e), code=500)


async def api_app_version(request):
    """Trả về thông tin phiên bản bản build hiện tại và kiểm tra cập nhật."""
    try:
        from update_manager import current_version, check_update
        v = current_version()
        update_info = {}
        try:
            update_info = check_update(timeout=5)
        except Exception as exc:
            logger.warning(f"check_update error: {exc}")
            update_info = {
                "available": False,
                "currentVersion": v,
                "version": v,
                "notes": ""
            }
        payload = {
            "version": v,
            "currentVersion": v,
            "available": update_info.get("available", False),
            "latestVersion": update_info.get("version", v),
            "url": update_info.get("url", ""),
            "sha256": update_info.get("sha256", ""),
            "notes": update_info.get("notes", ""),
            "mandatory": update_info.get("mandatory", False)
        }
        return json_ok(data=payload, **payload)
    except Exception as e:
        return json_error(str(e))


# ─── Đăng ký route ──────────────────────────────────────────────────────────────

def setup_routes(app):
    """Đăng ký toàn bộ route vào aiohttp app"""
    # Version & Update API
    app.router.add_get('/api/app/version', api_app_version)
    app.router.add_post('/api/app/version', api_app_version)

    # Voice Clone & Management API
    app.router.add_get('/api/voices', api_voices)
    app.router.add_post('/api/voices/enroll', api_voices_enroll)
    app.router.add_delete('/api/voices/{voice_id}', api_voices_delete)
    app.router.add_post('/api/voices/test', api_voices_test)
    app.router.add_post('/tts-stream', api_voices_test)

    # Prompt Optimization & Save API
    app.router.add_post('/api/prompt/optimize', api_optimize_prompt)
    app.router.add_post('/api/prompt/save', api_save_prompt)

    app.router.add_post("/get_prompt", get_session_prompt)
    app.router.add_post("/human", human)
    app.router.add_post("/humanaudio", humanaudio)
    app.router.add_post("/set_audiotype", set_audiotype)
    app.router.add_post("/record", record)
    app.router.add_post("/interrupt_talk", interrupt_talk)
    app.router.add_post("/is_speaking", is_speaking)
    app.router.add_post("/speech_status", speech_status)
    app.router.add_post("/speech_settings", speech_settings)
    app.router.add_post("/chat_messages", chat_messages)
    app.router.add_post("/event_message", event_message)
    app.router.add_post("/event_messages", event_messages)
    app.router.add_post("/tiktok_connect", tiktok_connect)
    app.router.add_post("/tiktok_disconnect", tiktok_disconnect)
    app.router.add_get("/tiktok_status", tiktok_status)
    app.router.add_post("/tiktok_report_status", tiktok_report_status)
    app.router.add_post("/avatars_list", avatars_list)
    app.router.add_get("/api/cameras", cameras_list_route)
    app.router.add_post("/api/cameras", cameras_list_route)
    app.router.add_get("/api/camera/stream", camera_stream_route)
    app.router.add_get("/api/camera/snapshot", camera_snapshot_route)
    app.router.add_post("/api/camera/release", camera_release_route)
    app.router.add_get("/avatar_preview/{avatar_id}", avatar_preview)
    app.router.add_get("/avatar_video/{avatar_id}", avatar_video)
    app.router.add_post("/select_avatar", select_avatar)
    app.router.add_post("/upload_avatar", upload_avatar)
    # Avatar task API
    app.router.add_post('/api/avatar/task', api_avatar_task)
    app.router.add_get('/api/avatar/tasks', api_avatar_tasks)
    app.router.add_delete('/api/avatar/task/{taskid}', api_avatar_task_delete)

    # Playlist & TikTok Shop Auto-Pin API
    app.router.add_get('/api/playlist', api_playlist)
    app.router.add_get('/api/playlist/state', api_playlist)
    app.router.add_post('/api/playlist/config', api_playlist_config)
    app.router.add_post('/api/playlist/save', api_playlist_save)
    app.router.add_post('/api/playlist/add', api_playlist_add)
    app.router.add_post('/api/playlist/delete', api_playlist_delete)
    app.router.add_post('/api/playlist/clear', api_playlist_clear)
    app.router.add_post('/api/playlist/generate_script', api_playlist_generate_script)
    app.router.add_post('/api/playlist/select', api_playlist_select)
    app.router.add_post('/api/playlist/toggle_rotation', api_playlist_toggle_rotation)
    app.router.add_post('/api/playlist/next', api_playlist_next)
    app.router.add_post('/api/playlist/check_comment', api_playlist_check_comment)
    app.router.add_get('/api/tiktok/pending_actions', api_tiktok_pending_actions)
    app.router.add_post('/api/tiktok/report_status', api_tiktok_report_status_ext)
    app.router.add_get('/api/tiktok/status', api_tiktok_status_ext)
    app.router.add_post('/api/tiktok/manual_pin', api_tiktok_manual_pin)
    app.router.add_post('/api/tiktok/sync_cart', api_tiktok_sync_cart)
    app.router.add_post('/api/tiktok/request_sync_cart', api_tiktok_request_sync_cart)
    app.router.add_get('/api/tiktok/extension_info', api_tiktok_extension_info)
    app.router.add_post('/api/tiktok/open_extension_folder', api_tiktok_open_extension_folder)
    
    # start avatar worker on app startup
    async def _start_avatar_worker(a):
        a['avatar_worker_task'] = asyncio.create_task(avatar_worker(a))

    async def _stop_avatar_worker(a):
        t = a.get('avatar_worker_task')
        if t:
            t.cancel()
            try:
                await t
            except asyncio.CancelledError:
                pass

    app.on_startup.append(_start_avatar_worker)
    app.on_cleanup.append(_stop_avatar_worker)
    app.router.add_static('/', path='web')
