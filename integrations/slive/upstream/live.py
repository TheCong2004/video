from TikTokLive import TikTokLiveClient
from TikTokLive.events import ConnectEvent, CommentEvent, GiftEvent, LikeEvent, FollowEvent
try:
    from TikTokLive.client.errors import UserNotFoundError, UserOfflineError
except Exception:
    UserNotFoundError = type('UserNotFoundError', (Exception,), {})
    UserOfflineError = type('UserOfflineError', (Exception,), {})
import os
import sys
import json
import re
import random
import time
import unicodedata
import urllib.request
import urllib.error
from typing import Optional, Dict, Any, List
from openai import OpenAI
from utils.logger import logger
from llm import BASE_LIVESTREAM_PROMPT

# Force sys.stdout and sys.stderr to UTF-8 with character replacement on Windows
if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
if hasattr(sys.stderr, 'reconfigure'):
    sys.stderr.reconfigure(encoding='utf-8', errors='replace')

# Configuration
LIVE_APP_URL = os.getenv('LIVE_APP_URL', 'http://127.0.0.1:8010')

OLLAMA_MODEL = 'qwen2.5:3b'

def normalize_tiktok_unique_id(value: str) -> str:
    raw = unicodedata.normalize('NFC', value or '').strip()
    if not raw:
        return '@s_live_ai'
    raw = raw.split('?', 1)[0].split('#', 1)[0].rstrip('/')
    raw = re.sub(r'/live/?$', '', raw, flags=re.IGNORECASE)
    match = re.search(r'@([A-Za-z0-9_.]+)', raw)
    if match:
        username = match.group(1)
    else:
        username = raw.rsplit('/', 1)[-1].lstrip('@').strip()
    if not re.fullmatch(r'[A-Za-z0-9_.]{2,24}', username):
        username = 's_live_ai'
    return f'@{username}'


def _clean_username(username: str) -> str:
    return normalize_tiktok_unique_id(username)


def emit_live_error(code, message, detail=None):
    payload = {
        'code': code,
        'message': message,
        'detail': str(detail or message),
        'username': unique_id.lstrip('@') if 'unique_id' in globals() else '',
    }
    print('LIVE:ERROR:' + json.dumps(payload, ensure_ascii=False), flush=True)


# Create the client
unique_id = normalize_tiktok_unique_id(os.getenv('TIKTOK_USER') or (sys.argv[1] if len(sys.argv) > 1 else '@s_live_ai'))
print(f'LIVE:USERNAME:{unique_id}', flush=True)
client: TikTokLiveClient = TikTokLiveClient(unique_id=unique_id)
DEFAULT_API_KEY = (
    os.getenv('LIVE_LLM_API_KEY') or 
    os.getenv('GEMINI_API_KEY') or 
    os.getenv('OPENAI_API_KEY') or ""
)

# Global sessionid, connection state, and cooldowns
current_sessionid: str = 'current'
connection_time = time.time()
thanked_likers = set()
like_cooldowns = {}


def _fetch_session_prompt(sessionid: str) -> str:
    """Fetch the active custom prompt from the server dynamically, or fallback to live_prompt.txt."""
    try:
        url = LIVE_APP_URL.rstrip('/') + '/get_prompt'
        payload = json.dumps({'sessionid': sessionid}).encode('utf-8')
        req = urllib.request.Request(
            url,
            data=payload,
            headers={'Content-Type': 'application/json'},
            method='POST'
        )
        with urllib.request.urlopen(req, timeout=2) as resp:
            data = json.loads(resp.read().decode('utf-8'))
            if data.get('code') == 0:
                prompt = data.get('data', {}).get('prompt', '')
                if prompt:
                    return prompt
    except Exception as e:
        logger.warning(f"Could not fetch session prompt from server: {e}")

    try:
        prompt_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'live_prompt.txt')
        if os.path.exists(prompt_path):
            with open(prompt_path, 'r', encoding='utf-8') as f:
                prompt = f.read().strip()
                if prompt:
                    return prompt
    except Exception as e:
        logger.warning(f"Could not read live_prompt.txt fallback: {e}")
    return ""


def _check_playlist_comment(comment_text: str, user_name: str) -> Optional[dict]:
    """Check with main app if comment asks about any product in playlist and trigger auto-pin."""
    try:
        url = LIVE_APP_URL.rstrip('/') + '/api/playlist/check_comment'
        payload = json.dumps({'comment': comment_text, 'viewer': user_name}).encode('utf-8')
        req = urllib.request.Request(
            url,
            data=payload,
            headers={'Content-Type': 'application/json'},
            method='POST'
        )
        with urllib.request.urlopen(req, timeout=2) as resp:
            data = json.loads(resp.read().decode('utf-8'))
            if data.get('code') == 0 and data.get('matched'):
                return data.get('product')
    except Exception as e:
        logger.debug(f"Playlist comment check error: {e}")
    return None


def _sanitize_tts_text(text: str) -> str:
    """Remove emojis, symbols, variation selectors, and collapse whitespace so TTS stays clean and stable."""
    text = text.replace('\n', ' ').replace('\r', ' ')
    text = re.sub(r'[\U00010000-\U0010FFFF\u2600-\u27BF\u2300-\u23FF\u2B50-\u2B55\uFE00-\uFE0F\u200D\u200B]', '', text)
    text = re.sub(r'\s+', ' ', text).strip()
    return text


def _first_sentence(text: str) -> str:
    """Keep only the first sentence to force concise livestream replies."""
    text = _sanitize_tts_text(text)
    parts = re.split(r'(?<=[.!?])\s+', text)
    first = parts[0].strip() if parts else text
    return first.rstrip('.!?') + '.' if first else ''


def _clean_comment_reply(text: str, user_name: str) -> str:
    """Remove apology/self-intro phrases and repeated greetings from model output."""
    text = _sanitize_tts_text(text)
    text = text.replace('```', ' ')
    
    # Strip unwanted prefix role labels like "Bạn:", "Shop!", "AI -", "Host:" first
    while True:
        new_text = re.sub(r'^(xin\s+)?(bạn|shop|ai|trợ lý|tro ly|mc|host)\b\s*[:!?-]?\s*', '', text, flags=re.IGNORECASE).strip()
        if new_text == text:
            break
        text = new_text

    name_parts = [part for part in re.split(r'\s+|[-–—]+', user_name) if part]
    name_parts = name_parts[:3]
    if name_parts:
        for size in range(len(name_parts), 0, -1):
            fragment = ' '.join(name_parts[:size])
            text = re.sub(r'^' + re.escape(fragment) + r'\b[,.:!?-]?\s*', '', text, flags=re.IGNORECASE).strip()
    if name_parts:
        for part in name_parts:
            text = re.sub(r'\b' + re.escape(part) + r'\b[,.:!?-]?\s*', '', text, flags=re.IGNORECASE).strip()
    text = re.sub(r'\b(xin lỗi|xin loi|tôi xin lỗi|toi xin loi|rất xin lỗi|rat xin loi)\b[^,.:!?]*[,.:!?]?\s*', '', text, flags=re.IGNORECASE).strip()
    text = re.sub(r'^(xin\s+)?(chào|hello|hi|chao)\s+' + re.escape(user_name) + r'\s*[,.:!?]?\s*', '', text, flags=re.IGNORECASE).strip()
    text = re.sub(r'^(xin\s+)?(chào|hello|hi|chao)\b(\s+(bạn|quý khách|khách|moi nguoi|mọi người|anh|chị|em|ban|quy khach|khach|cả nhà|ca nha))?\b\s*[,.:!?-]?\s*', '', text, flags=re.IGNORECASE).strip()
    text = re.sub(r'\b(tôi là|toi la|mình là|minh la|của tôi|cua toi|của mình|cua minh)\b[^,.:!?]*[,.:!?]?\s*', '', text, flags=re.IGNORECASE).strip()

    # Again strip role labels or greetings if left after removing apologies
    while True:
        new_text = re.sub(r'^(xin\s+)?(bạn|shop|ai|trợ lý|tro ly|mc|host)\b\s*[:!?-]?\s*', '', text, flags=re.IGNORECASE).strip()
        if new_text == text:
            break
        text = new_text

    text = re.sub(r'`+', ' ', text)
    text = re.sub(r'\s+', ' ', text).strip()
    return text


def _clean_viewer_name_for_speech(raw_name: str) -> str:
    """Chuẩn hóa tên người xem để TTS phát âm tự nhiên, loại bỏ emoji và ID máy tính."""
    if not raw_name:
        return "bạn"
    name = _sanitize_tts_text(str(raw_name))
    name = re.sub(r'[@_.\-#]', ' ', name).strip()
    name = re.sub(r'\s+', ' ', name).strip()

    # Nếu là chuỗi số hoặc id dạng user123456 / khach999 / rỗng
    if not name or re.match(r'^(?:user|khach|tiktok|acc|id)?\d+$', name, flags=re.IGNORECASE) or len(name) < 2:
        return "bạn"

    parts = name.split()
    # Nếu tên quá dài (trên 3 từ), lấy 2 từ cuối để xưng hô thân mật (ví dụ "Nguyễn Thùy Linh" -> "Thùy Linh")
    if len(parts) > 3:
        return " ".join(parts[-2:])
    return name


def get_active_pinned_product() -> dict:
    """Retrieve currently active or pinned product from playlist state."""
    try:
        playlist_file = os.path.join(
            os.environ.get('LOCALAPPDATA') or os.path.expanduser('~'),
            'SLiveAI',
            'playlist_data.json'
        )
        if os.path.exists(playlist_file):
            with open(playlist_file, 'r', encoding='utf-8') as f:
                data = json.load(f)
                products = data.get('products', [])
                runtime = data.get('config', {})
                active_id = runtime.get('active_product_id')
                if active_id:
                    for p in products:
                        if p.get('id') == active_id:
                            return p
                if products:
                    return products[0]
    except Exception as e:
        logger.debug(f"Could not get pinned product: {e}")
    return {}


FAST_RULE_PATTERNS = [
    (r'\b(chào|hi|hello|chào shop|chuc shop|đắt hàng)\b', [
        'Chào bạn {user_name}, rất vui được gặp bạn trong livestream hôm nay.',
        'Cảm ơn bạn {user_name} đã ghé xem livestream, chúc bạn săn được deal hời nha.',
        'Chào {user_name}, cảm ơn bạn đã tương tác cùng shop nhé.'
    ]),
    (r'\b(ship|phí ship|phi ship|giao hàng|giao hang|khi nào giao|mấy ngày tới|may ngay toi)\b', [
        'Shop giao hàng toàn quốc siêu nhanh chỉ 1 đến 3 ngày là nhận được nha bạn {user_name}.',
        'Đơn hàng sẽ được đóng gói giao nhanh tận nơi cho bạn {user_name} nha.',
        'Giao nhanh 1 đến 3 ngày toàn quốc bạn {user_name} yên tâm chốt đơn nha.'
    ]),
    (r'\b(còn hàng|het hang|hết hàng|còn không|còn ko|con khong|con ko|mua duoc ko)\b', [
        'Sản phẩm vẫn còn sẵn hàng trong kho nha bạn {user_name}, chốt đơn ngay nhé.',
        'Hàng sẵn trong kho nha bạn {user_name}, bạn bấm giỏ hàng lên đơn ngay nhé.'
    ])
]


def find_matching_prompt_guideline(custom_prompt: str, comment_text: str) -> Optional[str]:
    """Scan custom prompt to find any specific answer or guideline matching the user's question."""
    if not custom_prompt or not comment_text:
        return None
    
    norm_prompt = custom_prompt.strip()
    norm_comment = comment_text.lower().strip()
    
    # 1. Search in [QUY TẮC TRẢ LỜI BÌNH LUẬN] section or whole prompt
    rules_content = norm_prompt
    for marker in ["QUY TẮC TRẢ LỜI BÌNH LUẬN", "COMMENT REPLY RULES", "QUY TẮC TRẢ LỜI"]:
        if marker.lower() in norm_prompt.lower():
            idx = norm_prompt.lower().find(marker.lower())
            rules_content = norm_prompt[idx + len(marker):]
            break
            
    # 2. Check for explicit pattern: "Khi/Nếu hỏi X trả lời Y" or "Hỏi X: Y"
    lines_and_sentences = re.split(r'[\r\n.]+', rules_content)
    for seg in lines_and_sentences:
        seg_clean = seg.strip()
        if not seg_clean or len(seg_clean) < 3:
            continue
        m = re.search(r'(?:hỏi|hỏi về|khi hỏi|nếu hỏi)\s+["\']?([^:"\',.\n?]+?)["\']?\s*(?:thì)?\s*(?:hãy)?\s*(?:trả lời|nói)\s*(?:đúng|là)?\s*[:\-–—]?\s*["\']?([^"\']+)["\']?', seg_clean, flags=re.IGNORECASE)
        if m:
            q_kw = m.group(1).lower().strip()
            ans = m.group(2).strip()
            if any(w in norm_comment for w in q_kw.split() if len(w) >= 2):
                return f"Yêu cầu từ kịch bản: Khi hỏi '{q_kw}', hãy trả lời: '{ans}'"

        topic_keywords = {
            'giá': ['giá', 'nhiêu', 'bao tiền', 'bn', 'bao nhieu', 'nhiu'],
            'ship': ['ship', 'giao hàng', 'vận chuyển', 'mấy ngày', 'phi ship', 'phí ship'],
            'cách dùng': ['cách dùng', 'dùng sao', 'hướng dẫn', 'pha sao', 'uống sao', 'sử dụng', 'cách pha', 'dùng thế nào'],
            'nguồn gốc': ['nguồn gốc', 'xuất xứ', 'ở đâu', 'chính hãng', 'sản xuất', 'nhập khẩu'],
            'chất liệu': ['chất liệu', 'làm bằng', 'thành phần', 'nguyên liệu', 'vải'],
            'màu sắc': ['màu', 'size', 'kích thước', 'mẫu'],
            'bảo hành': ['bảo hành', 'đổi trả', 'hỏng', 'lỗi'],
            'quà tặng': ['quà', 'tặng', 'khuyến mãi', 'voucher', 'deal', 'ưu đãi']
        }
        for topic, kws in topic_keywords.items():
            if any(kw in norm_comment for kw in kws) and any(kw in seg_clean.lower() for kw in kws):
                return f"Nội dung quy tắc trong kịch bản liên quan đến câu hỏi này: '{seg_clean}'"
                
    return None


def fast_rule_reply(user_name: str, event_text: str) -> Optional[str]:
    normalized = unicodedata.normalize('NFC', str(event_text or '')).casefold()
    user = _clean_viewer_name_for_speech(user_name)

    # 1. Nhận diện câu hỏi GIÁ / BAO NHIÊU TIỀN -> Điền ngay giá sản phẩm đang ghim (< 0.05s)
    price_pattern = r'\b(giá|gia|bao nhiêu|bao nhieu|nhiêu|nhiu|bn|giá sao|gia sao|giá thế nào|bao tiền|bao tien)\b'
    if re.search(price_pattern, normalized, flags=re.IGNORECASE | re.UNICODE):
        prod = get_active_pinned_product()
        raw_title = prod.get('title', 'sản phẩm')
        cleaned_title = re.sub(r'\[[^\]]*\]|\([^\)]*\)|[#*_\-]', ' ', raw_title).strip()
        short_title = ' '.join(cleaned_title.split()[:6]) or 'sản phẩm'
        price = prod.get('price') or 'siêu ưu đãi'
        idx = prod.get('index', 1)
        templates = [
            f"Dạ bạn {user} ơi, {short_title} đang ghim số #{idx} giá ưu đãi chỉ {price} thôi nha, bạn bấm góc trái chốt liền nhé!",
            f"Dạ {short_title} đang ghim trên màn hình giá hôm nay chỉ {price} nha bạn {user}, số lượng có hạn bạn bấm giỏ hàng đặt ngay nhé!",
            f"Giá của {short_title} hôm nay cực kỳ hời chỉ {price} thôi nha bạn {user} ơi, chốt đơn ngay kẻo hết deal nhé!"
        ]
        return random.choice(templates)

    # 2. Nhận diện các mẫu câu chào hỏi, ship, còn hàng
    for pattern, templates in FAST_RULE_PATTERNS:
        if re.search(pattern, normalized, flags=re.IGNORECASE | re.UNICODE):
            return random.choice(templates).format(user_name=user)
    return None



def _enforce_comment_response(user_name: str, llm_response: str, comment_text: str = '') -> str:
    """Keep replies concise, direct, natural, and ready for TTS without directing to inbox."""
    clean_name = _clean_viewer_name_for_speech(user_name)
    cleaned = _clean_comment_reply(llm_response, clean_name)

    # Lọc bỏ các câu/mệnh đề hướng người dùng vào inbox nếu model vô tình sinh ra
    cleaned = re.sub(
        r'(?:^|[.!?])\s*[^.!?]*(?:inbox|nhắn tin riêng|in bốc|\bib\b|nhắn shop|nhắn riêng|nhắn cho em)[^.!?]*[.,!?]?',
        '.',
        cleaned,
        flags=re.IGNORECASE
    )
    cleaned = re.sub(r'\.+', '.', cleaned).strip(' .')

    fallback_templates = [
        f"Dạ {clean_name} ơi, mình bấm ngay vào giỏ hàng góc trái màn hình để xem ưu đãi độc quyền hôm nay nha.",
        f"Dạ em cảm ơn {clean_name} đã quan tâm nha, mình xem chi tiết và săn deal giá hời ngay ở giỏ hàng góc trái nhé.",
        f"{clean_name} ơi, sản phẩm này đang có trợ giá cực tốt, mình bấm vào giỏ hàng góc trái để chốt đơn liền nha.",
        f"Dạ {clean_name} ơi, em nghe nè! Mình vào ngay giỏ hàng góc trái để nhận mã giảm giá tốt nhất phiên live nha.",
        f"Em chào {clean_name} nha! Mọi thông tin ưu đãi và voucher em đã ghim sẵn trong giỏ hàng góc trái rồi ạ."
    ]

    if not cleaned or len(cleaned.split()) < 3:
        return random.choice(fallback_templates)

    # Lấy 1-2 câu đầu tiên (tổng độ dài khoảng dưới 32 từ) để phát âm TTS tự nhiên
    sentences = [s.strip() for s in re.split(r'(?<=[.!?])\s+', cleaned) if s.strip()]
    selected = []
    word_count = 0
    for s in sentences:
        words = s.split()
        if word_count + len(words) > 32 and selected:
            break
        selected.append(s)
        word_count += len(words)
        if len(selected) >= 2:
            break

    final_text = ' '.join(selected).strip() if selected else cleaned
    final_text = _sanitize_tts_text(final_text)
    final_text = re.sub(r'^(bạn|shop|ai|mc|host)\s*[:!?-]\s*', '', final_text, flags=re.IGNORECASE).strip()

    # Kiểm tra xem câu đã có xưng hô tự nhiên hoặc nhắc tên khách hay chưa
    has_name_ref = False
    if clean_name and clean_name.lower() != "bạn":
        if clean_name.lower() in final_text.lower():
            has_name_ref = True
    else:
        if re.search(r'\b(bạn|mình|khách|cả nhà|anh|chị)\b', final_text, flags=re.IGNORECASE):
            has_name_ref = True

    # Nếu model chưa nhắc tên hoặc câu chưa có xưng hô tự nhiên, bổ sung mở đầu duyên dáng
    if not has_name_ref:
        if clean_name and clean_name.lower() != "bạn":
            openers = [
                f"Dạ {clean_name} ơi, ",
                f"{clean_name} ơi em nghe nè, ",
                f"Dạ em chào {clean_name} nha, ",
                f"{clean_name} ơi em giải đáp cho mình ngay đây ạ, ",
                f"Dạ cảm ơn {clean_name} đã quan tâm nha, ",
                f"{clean_name} ơi, "
            ]
        else:
            openers = [
                "Dạ mình ơi, ",
                "Dạ em chào mình nha, ",
                "Dạ em nghe nè mình ơi, ",
                "Dạ cảm ơn mình đã quan tâm nha, ",
                "Mình ơi em giải đáp cho mình ngay đây ạ, "
            ]
        selected_opener = random.choice(openers)
        if final_text:
            first_char = final_text[0].lower()
            rest = final_text[1:]
            final_text = f"{selected_opener}{first_char}{rest}"
        else:
            final_text = random.choice(fallback_templates)

    # Đảm bảo kết thúc bằng dấu chấm
    if not final_text.endswith(('.', '!', '?')):
        final_text += '.'

    return final_text


def send_to_tts(text: str, sessionid: str = 'current', event: bool = False, priority: int | bool = None, interrupt: bool = False) -> bool:
    """Send text to LiveTalking app via /human endpoint for TTS."""
    try:
        url = LIVE_APP_URL.rstrip('/') + '/human'
        payload_data = {
            'type': 'echo',
            'text': text,
            'sessionid': sessionid,
            'interrupt': interrupt,
            'is_tiktok_event': event
        }
        if event:
            payload_data['event'] = False
        if priority is not None:
            payload_data['priority'] = priority
        payload = json.dumps(payload_data).encode('utf-8')
        req = urllib.request.Request(
            url,
            data=payload,
            headers={'Content-Type': 'application/json'},
            method='POST'
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode('utf-8'))
            success = data.get('code') == 0
            if not success:
                logger.error(f'Live TTS endpoint returned error: {data}')
            return success
    except urllib.error.URLError as e:
        logger.error(f'Could not reach LiveTalking app at {LIVE_APP_URL}: {e}')
    except Exception as e:
        logger.exception(f'Error sending text to TTS: {e}')
    return False


def send_tiktok_event(viewer: str, comment: str, reply: str, sessionid: str = 'current', event_type: str = 'comment') -> bool:
    """Send TikTok comment/reply pairs to the dashboard event stream."""
    try:
        url = LIVE_APP_URL.rstrip('/') + '/event_message'
        payload_data = {
            'sessionid': sessionid,
            'viewer': viewer,
            'comment': comment,
            'reply': reply,
            'event_type': event_type,
        }
        payload = json.dumps(payload_data).encode('utf-8')
        req = urllib.request.Request(
            url,
            data=payload,
            headers={'Content-Type': 'application/json'},
            method='POST',
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode('utf-8'))
            success = data.get('code') == 0
            if not success:
                logger.error(f'Live event endpoint returned error: {data}')
            return success
    except urllib.error.URLError as e:
        logger.error(f'Could not reach LiveTalking app at {LIVE_APP_URL}: {e}')
    except Exception as e:
        logger.exception(f'Error sending TikTok event: {e}')
    return False


def generate_live_response(user_name: str, event_text: str, event_type: str = 'comment', matched_product: Optional[dict] = None) -> str:
    runtime_config = {}
    try:
        from runtime_config import load_config, refresh_remote_runtime_config, apply_runtime_environment
        runtime_config = load_config()
        refresh_remote_runtime_config(runtime_config, max_age_seconds=3)
        apply_runtime_environment(runtime_config)
    except Exception:
        pass
    current_llm_url = runtime_config.get("openai_llm_base_url") or os.getenv('OPENAI_LLM_API_BASE_URL') or os.getenv('LLM_API_BASE_URL') or 'https://api.openai.com/v1/'
    llm_api_key = (
        os.getenv('OPENAI_API_KEY') or 
        runtime_config.get("openai_api_key") or 
        os.getenv('LIVE_LLM_API_KEY') or 
        os.getenv('LLM_API_KEY') or 
        runtime_config.get("llm_api_key") or 
        DEFAULT_API_KEY
    )
    llm_model = os.getenv('OPENAI_LLM_MODEL') or runtime_config.get("openai_llm_model") or os.getenv('LLM_MODEL') or "gpt-4o-mini"
    llm_url = current_llm_url

    if not llm_url and not llm_api_key.startswith(('sk-', 'AQ.', 'AIzaSy')):
        raise RuntimeError('Chưa có link OpenAI API trên trang Cấu hình hệ thống.')

    custom_prompt = _fetch_session_prompt(current_sessionid)
    clean_name = _clean_viewer_name_for_speech(user_name)

    matched_rule = None
    if not matched_product:
        # Kiểm tra xem kịch bản có câu trả lời hoặc quy tắc cụ thể cho câu hỏi này không
        matched_rule = find_matching_prompt_guideline(custom_prompt, event_text)

        # Phản xạ nhanh fast_rule_reply nếu không có quy tắc cụ thể trong kịch bản
        if not matched_rule:
            fast_reply = fast_rule_reply(user_name, event_text)
            if fast_reply:
                logger.info(f"[FAST RULE HIT] Immediate reply generated for '{event_text[:20]}...': {fast_reply}")
                return fast_reply
        else:
            logger.info(f"[CUSTOM PROMPT RULE DETECTED] Bắt được quy tắc trong kịch bản cho '{event_text[:25]}': {matched_rule[:60]}... Chuyển sang LLM trả lời chuẩn xác!")

    if matched_product:
        raw_title = matched_product.get('title', '')
        # Loại bỏ STT và ký tự thừa trong tên sản phẩm trước khi đọc
        clean_title = re.sub(r'\[[^\]]*\]', ' ', raw_title)
        clean_title = re.sub(r'\([^\)]*\)', ' ', clean_title)
        clean_title = re.sub(r'【[^】]*】', ' ', clean_title)
        clean_title = re.sub(r'^(?:sản\s*phẩm|sp|mẫu|deal)?\s*#?\s*\d+[\s:.\-]+', '', clean_title, flags=re.IGNORECASE)
        clean_title = re.sub(r'[#*_\-]', ' ', clean_title)
        clean_title = re.sub(r'\s+', ' ', clean_title).strip()
        words = clean_title.split()
        prod_title = " ".join(words[:8]) if len(words) > 8 else (clean_title or "sản phẩm")

        prod_price = matched_product.get('price', '')
        prod_script = matched_product.get('script', '')

        system_prompt = (
            "Bạn là Host/MC livestream bán hàng duyên dáng, cực kỳ hoạt ngôn và chốt đơn siêu đỉnh trên TikTok Live.\n"
            f"TÌNH HUỐNG: Khách hàng '{clean_name}' vừa hỏi về sản phẩm: {prod_title}.\n"
            f"Hệ thống đã TỰ ĐỘNG GHIM sản phẩm {prod_title} lên màn hình cho người xem.\n\n"
            f"THÔNG TIN SẢN PHẨM:\n"
            f"- Tên sản phẩm: {prod_title}\n"
            f"- Mức giá: {prod_price or 'Ưu đãi cực tốt trong giỏ hàng'}\n"
            f"- Điểm nổi bật & Kịch bản: {prod_script or prod_title}\n\n"
            "QUY TẮC BẮT BUỘC KHI TRẢ LỜI:\n"
            f"1. ĐI THẲNG VÀO NỘI DUNG & XÁC NHẬN ĐÃ GHIM: Bắt đầu tự nhiên, thân mật (ví dụ: 'Dạ {clean_name} ơi, em vừa ghim ngay {prod_title} lên màn hình cho mình rồi nè').\n"
            "2. TUYỆT ĐỐI KHÔNG ĐỌC SỐ THỨ TỰ (STT): Nghiêm cấm đọc 'số 1', 'số 2', 'sản phẩm số...', '#1', '#2'... CHỈ gọi duy nhất TÊN SẢN PHẨM.\n"
            "3. TUYỆT ĐỐI KHÔNG CHÀO KIỂU BẮT ĐẦU PHIÊN LIVE: Cấm các câu chào rập khuôn như 'Chào mừng bạn đến với phiên live', 'Xin chào cả nhà'. Buổi live đang diễn ra liên tục, phải tiếp nối mạch nói sôi nổi.\n"
            f"4. TƯ VẤN NHANH VỀ SẢN PHẨM: Nêu ngay 1 điểm vượt trội nhất của sản phẩm (công dụng, chất liệu, ưu điểm) hoặc mức giá ưu đãi {prod_price or 'rất hời'}.\n"
            "5. KÊU GỌI CHỐT ĐƠN DUYÊN DÁNG: Nhắc khách bấm ngay vào sản phẩm đang ghim trên màn hình hoặc giỏ hàng góc trái để chốt đơn ngay kẻo hết quà.\n"
            "6. ĐỘ DÀI: Ngắn gọn từ 1 đến 2 câu (khoảng 20-30 từ) để đọc trôi chảy, nhịp nhàng, kết thúc bằng dấu chấm, không dùng icon emoji hay markdown."
        )
        user_message = f"Khách hàng '{clean_name}' vừa hỏi: \"{event_text}\". Hãy xác nhận đã ghim {prod_title} và tư vấn chốt đơn tự nhiên. Nhớ: TUYỆT ĐỐI KHÔNG đọc số thứ tự (STT), chỉ gọi tên sản phẩm."
    elif custom_prompt:
        system_prompt = (
            "Bạn là Host/MC livestream bán hàng chuyên nghiệp, thông minh, duyên dáng và bán hàng xuất sắc trên TikTok Live.\n"
            "NHIỆM VỤ: Trả lời bình luận của khán giả một cách tự nhiên, nhanh nhạy, đi thẳng vào trọng tâm câu hỏi và giải đáp thắc mắc ngay trên phiên live.\n\n"
            "THÔNG TIN KỊCH BẢN & SẢN PHẨM:\n"
            f"{custom_prompt}\n\n"
            "QUY TẮC TRẢ LỜI BÌNH LUẬN:\n"
            "1. TRẢ LỜI TRỰC TIẾP & ĐÚNG CÂU HỎI (QUAN TRỌNG BẬC NHẤT):\n"
            "   - Trả lời thẳng vào thắc mắc của khách (công dụng, thành phần, cách dùng, giá bán, thời gian giao hàng, quà tặng, ưu đãi...).\n"
            "   - TUYỆT ĐỐI KHÔNG trả lời rập khuôn 'bạn inbox/nhắn tin riêng cho shop' hay hướng người dùng vào inbox. Hãy giải đáp cặn kẽ, rõ ràng ngay trên phiên live để khách an tâm mua hàng.\n"
            "   - Nếu khách hỏi Giá bán: Trả lời trực tiếp mức giá có trong kịch bản. Nếu kịch bản chưa có giá cụ thể, hãy hướng dẫn khách bấm vào giỏ hàng góc trái màn hình để xem mức giá và voucher ưu đãi tốt nhất trong phiên live hôm nay.\n"
            "   - Nếu khách hỏi Giao hàng / Ship: Báo shop giao hàng toàn quốc nhanh chóng chỉ từ 1 đến 3 ngày, được kiểm tra hàng trước khi thanh toán.\n"
            "   - Nếu khách hỏi công dụng, cách dùng hoặc đối tượng sử dụng: Dựa vào thông tin kịch bản để giải đáp nhiệt tình, thông minh và tích cực.\n"
            "2. XƯNG HÔ & THÁI ĐỘ THÂN THIỆN:\n"
            "   - Xưng hô 'em' hoặc 'shop', chào và gọi tên khách thân mật (ví dụ: 'Dạ [Tên] ơi', '[Tên] ơi em nghe nè', 'Dạ em chào [Tên] nha').\n"
            "   - Giọng điệu lịch sự, dễ mến, sử dụng các từ đệm tự nhiên như: 'dạ', 'ạ', 'nha', 'nhé', 'nè'.\n"
            "3. ĐỘ DÀI & ĐỊNH DẠNG:\n"
            "   - Ngắn gọn từ 1 đến 2 câu (khoảng 18-28 từ) để máy đọc TTS chuẩn xác, nhịp nhàng, không làm ngắt quãng livestream quá lâu.\n"
            "   - Kết thúc bằng dấu chấm, không dùng icon emoji, không dùng dấu cảm thán (!), không dùng ký hiệu markdown hay gạch đầu dòng.\n"
            "4. BÌNH LUẬN SPAM / PHÁ HOẠI:\n"
            "   - CHỈ XUẤT DUY NHẤT từ khóa '[IGNORE]' khi bình luận là ký tự spam vô nghĩa (như '.....', '111') hoặc lời lẽ xúc phạm, thô tục."
        )
        user_message = f"Khách hàng '{clean_name}' vừa bình luận: \"{event_text}\". Hãy chào đón và trả lời trực tiếp câu hỏi của khách dựa trên kịch bản. Nếu là spam vô nghĩa hoặc từ ngữ thô tục, chỉ xuất duy nhất [IGNORE]."
        if matched_rule:
            user_message += (
                f"\n\n[CHỈ DẪN TỪ KỊCH BẢN SHOP]: {matched_rule}.\n"
                f"=> YÊU CẦU BẮT BUỘC: Hãy trả lời khách hàng {clean_name} ĐÚNG CHÍNH XÁC theo nội dung và yêu cầu chỉ dẫn trên!"
            )
    else:
        system_prompt = (
            f"{BASE_LIVESTREAM_PROMPT}\n"
            "Nhiệm vụ: Trả lời bình luận của người xem TikTok Live một cách thân thiện, duyên dáng, đi thẳng vào trọng tâm câu hỏi.\n"
            "Quy tắc bắt buộc:\n"
            "1. TRẢ LỜI TRỰC TIẾP câu hỏi của khách (công dụng, cách dùng, giao hàng, ưu đãi). TUYỆT ĐỐI KHÔNG bảo khách inbox hay nhắn tin riêng.\n"
            "2. Nếu hỏi giá hoặc cách mua, hướng dẫn khách bấm vào giỏ hàng góc trái màn hình để săn deal.\n"
            "3. Chào hỏi người xem theo tên thân mật, xưng 'em' hoặc 'shop', ngắn gọn 1-2 câu (dưới 28 từ).\n"
            "4. Không xin lỗi, không icon emoji, không dấu cảm thán, không ký tự đặc biệt."
        )
        user_message = f"Khách hàng '{clean_name}' vừa bình luận: \"{event_text}\""

    extra_params = {
        'max_tokens': 100,
        'temperature': 0.7,
        'presence_penalty': 0.5,
        'frequency_penalty': 0.5,
    }
    if llm_api_key.startswith(('AQ.', 'AIzaSy')):
        if not llm_url or "api.openai.com" in llm_url:
            llm_url = "https://generativelanguage.googleapis.com/v1beta/openai/"
        llm_model = llm_model or "gemini-1.5-flash"
    elif llm_api_key.startswith('sk-'):
        openai_url = runtime_config.get("openai_llm_base_url") or os.getenv('OPENAI_LLM_API_BASE_URL')
        llm_url = openai_url if (openai_url and 'trycloudflare.com' not in openai_url and 'localhost' not in openai_url) else "https://api.openai.com/v1/"
        llm_model = os.getenv('OPENAI_LLM_MODEL') or runtime_config.get("openai_llm_model") or "gpt-4o-mini"
    else:
        extra_params['extra_body'] = {
            'keep_alive': '10m',
            'options': {
                'num_predict': 100,
            },
        }

    try:
        client_llm = OpenAI(api_key=llm_api_key, base_url=llm_url)
        completion = client_llm.chat.completions.create(
            model=llm_model,
            messages=[
                {'role': 'system', 'content': system_prompt},
                {'role': 'user', 'content': user_message},
            ],
            **extra_params
        )
        response = completion.choices[0].message.content.strip()
        if not response:
            raise ValueError('Empty response from LLM')

        if custom_prompt and '[IGNORE]' in response:
            return ""

        response = _enforce_comment_response(user_name, response, event_text)
        return response
    except Exception as e:
        logger.exception(f'LLM generation failed: {e}')
        fast_fallback = fast_rule_reply(user_name, event_text)
        if fast_fallback:
            return fast_fallback
        fallbacks = [
            f"Dạ {clean_name} ơi, mình bấm vào giỏ hàng góc trái màn hình để xem chi tiết và săn deal hời nha.",
            f"Dạ em cảm ơn {clean_name} đã quan tâm nha, mọi ưu đãi em đã ghim sẵn trong giỏ hàng góc trái rồi ạ.",
            f"{clean_name} ơi em nghe nè, mình vào ngay giỏ hàng góc trái để nhận mã giảm giá phiên live nha.",
            f"Em chào {clean_name} nha! Sản phẩm đang có giá cực tốt, mình bấm giỏ hàng góc trái chốt đơn liền nhé."
        ]
        return random.choice(fallbacks)


async def on_connect(event: ConnectEvent):
    global connection_time
    connection_time = time.time()
    logger.info(f'Connected to @{event.unique_id} (Room ID: {client.room_id})')
    try:
        print(f'LIVE:CONNECTED:{event.unique_id}')
    except Exception:
        pass


async def on_comment(event: CommentEvent) -> None:
    global connection_time, current_sessionid
    if connection_time is None or (time.time() - connection_time < 0.5):
        return
    user = getattr(event, 'user', None)
    if not user:
        return
    user_name = getattr(user, 'nickname', '') or getattr(user, 'unique_id', '') or 'bạn'
    comment_text = getattr(event, 'comment', '') or ''
    if not comment_text:
        return

    logger.info(f'💬 Comment: {user_name} -> {comment_text}')

    # Check if comment refers to a specific product in playlist to auto-pin & switch script
    matched_prod = _check_playlist_comment(comment_text, user_name)
    if matched_prod:
        logger.info(f"🎯 [Auto-Pin on Comment] Switched & Pinned product #{matched_prod.get('index')}: {matched_prod.get('title')}")

    ai_response = generate_live_response(user_name, comment_text, event_type='comment', matched_product=matched_prod)
    if not ai_response:
        logger.info('[INFO] Comment ignored (unrelated to prompt)')
        return
    logger.info(f'AI Response: {ai_response}')

    reply_display = ai_response
    if matched_prod:
        reply_display = f"🎯 [ĐÃ GHIM SP #{matched_prod.get('index', 1)}] {ai_response}"

    send_tiktok_event(user_name, comment_text, reply_display, sessionid=current_sessionid, event_type='comment')

    if send_to_tts(ai_response, sessionid=current_sessionid, event=True, priority=0, interrupt=True):
        logger.info('✓ Sent to TTS (Interrupted)')
    else:
        logger.error('✗ Failed to send to TTS')


async def on_gift(event: GiftEvent) -> None:
    global connection_time, current_sessionid
    if connection_time is None or (time.time() - connection_time < 0.5):
        return
    user = getattr(event, 'user', None)
    if not user:
        return
    user_name = getattr(user, 'nickname', '') or getattr(user, 'unique_id', '') or 'bạn'
    gift = getattr(event, 'gift', None)
    gift_name = getattr(gift, 'name', 'quà') if gift else 'quà'
    repeat_count = getattr(event, 'repeat_count', 1) or 1
    gift_text = f'{gift_name} x{repeat_count}'
    logger.info(f'🎁 Gift: {user_name} sent {gift_text}')

    gift_templates = [
        f'Cảm ơn {user_name} đã tặng {gift_name} nhé.',
        f'Ui, cảm ơn {user_name} đã gửi tặng shop {gift_name} nha. Yêu quá!',
        f'Cảm ơn {user_name} rất nhiều vì đã tặng {gift_name} dễ thương này nha.',
        f'Cảm ơn món quà {gift_name} từ bạn {user_name} nhé, biết ơn bạn nhiều.',
        f'Tuyệt vời quá, cảm ơn {user_name} đã gửi tặng shop món quà {gift_name} nhé.'
    ]
    ai_response = random.choice(gift_templates)
    logger.info(f'Gift Response: {ai_response}')

    send_tiktok_event(user_name, gift_text, ai_response, sessionid=current_sessionid, event_type='gift')

    if send_to_tts(ai_response, sessionid=current_sessionid, event=True, priority=0, interrupt=True):
        logger.info('✓ Sent to TTS (Interrupted)')
    else:
        logger.error('✗ Failed to send to TTS')


async def on_like(event: LikeEvent) -> None:
    global connection_time, current_sessionid, like_cooldowns
    if connection_time is None or (time.time() - connection_time < 0.5):
        return
    user = getattr(event, 'user', None)
    if not user:
        return
    user_name = getattr(user, 'nickname', '') or getattr(user, 'unique_id', '') or 'bạn'
    
    # 60s cooldown per user so AI thanks likes periodically without spamming
    now = time.time()
    last_thanked = like_cooldowns.get(user_name, 0)
    if now - last_thanked < 60.0:
        return
    like_cooldowns[user_name] = now

    like_count = getattr(event, 'count', 1) or getattr(event, 'total_likes', 1) or 1
    like_text = f'thả {like_count} tim'
    logger.info(f'👍 Like: {user_name} liked x{like_count}')

    like_templates = [
        f'Cảm ơn {user_name} đã thả tim ủng hộ shop nha.',
        f'Yêu quá, cảm ơn {user_name} đã thả tim dễ thương nhé.',
        f'Cảm ơn bạn {user_name} đã nhiệt tình thả tim cho live nhé.',
        f'Thật tuyệt vời, cảm ơn {user_name} đã thả tim cho shop.'
    ]
    ai_response = random.choice(like_templates)
    logger.info(f'Like Response: {ai_response}')

    send_tiktok_event(user_name, like_text, ai_response, sessionid=current_sessionid, event_type='like')

    if send_to_tts(ai_response, sessionid=current_sessionid, event=True, priority=0, interrupt=True):
        logger.info('[OK] Like TTS sent')
    else:
        logger.error('[ERR] Like TTS failed')


async def on_follow(event: FollowEvent) -> None:
    global connection_time, current_sessionid
    if connection_time is None or (time.time() - connection_time < 0.5):
        return
    user = getattr(event, 'user', None)
    if not user:
        return
    user_name = getattr(user, 'nickname', '') or getattr(user, 'unique_id', '') or 'bạn'
    follow_text = 'đã theo dõi kênh'
    logger.info(f'➕ Follow: {user_name} followed')

    follow_templates = [
        f'Chào mừng {user_name} đã theo dõi kênh. Cảm ơn bạn rất nhiều nhé.',
        f'Cảm ơn {user_name} đã bấm theo dõi kênh của shop nha. Chào mừng bạn!',
        f'Chào mừng {user_name} gia nhập kênh, cảm ơn bạn đã nhấn theo dõi nhé.',
        f'Cảm ơn bạn {user_name} rất nhiều vì đã nhấn theo dõi kênh ủng hộ shop.',
        f'Cực kỳ biết ơn {user_name} đã theo dõi kênh của chúng tôi hôm nay.'
    ]
    ai_response = random.choice(follow_templates)
    logger.info(f'Follow Response: {ai_response}')

    send_tiktok_event(user_name, follow_text, ai_response, sessionid=current_sessionid, event_type='follow')

    if send_to_tts(ai_response, sessionid=current_sessionid, event=True, priority=0, interrupt=True):
        logger.info('[OK] Follow TTS sent')
    else:
        logger.error('[ERR] Follow TTS failed')


def _register_listeners():
    """Register all live event listeners on the active client instance."""
    client.add_listener(ConnectEvent, on_connect)
    client.add_listener(CommentEvent, on_comment)
    client.add_listener(GiftEvent, on_gift)
    client.add_listener(LikeEvent, on_like)
    client.add_listener(FollowEvent, on_follow)


def report_tiktok_status(status: str, error: str = '', sessionid: str = 'current'):
    """Báo cáo trạng thái và lỗi của TikTok listener về web server chính"""
    try:
        url = LIVE_APP_URL.rstrip('/') + '/tiktok_report_status'
        payload = json.dumps({'sessionid': sessionid, 'status': status, 'error': error}).encode('utf-8')
        req = urllib.request.Request(
            url,
            data=payload,
            headers={'Content-Type': 'application/json'},
            method='POST'
        )
        with urllib.request.urlopen(req, timeout=3):
            pass
    except Exception:
        pass


def start_tiktok_listener(username: str, sessionid: str = 'current'):
    """Khởi động TikTok listener với cơ chế tự động thử lại khi gặp lỗi Sign API hoặc mạng"""
    global current_sessionid, client, thanked_likers, like_cooldowns, unique_id
    cleaned_user = normalize_tiktok_unique_id(username)
    unique_id = cleaned_user
    print(f'LIVE:USERNAME:{cleaned_user}', flush=True)
    current_sessionid = sessionid
    thanked_likers.clear()
    like_cooldowns.clear()

    max_retries = 3
    for attempt in range(1, max_retries + 1):
        try:
            logger.info(f'Starting TikTok listener for {cleaned_user} with sessionid {sessionid} (Lần {attempt}/{max_retries})')
            client = TikTokLiveClient(unique_id=cleaned_user)
            _register_listeners()
            report_tiktok_status('connected', error='', sessionid=sessionid)
            client.run()
            logger.info(f'TikTok listener kết thúc bình thường cho {cleaned_user}')
            report_tiktok_status('ended', error='Phiên TikTok Live đã kết thúc.', sessionid=sessionid)
            break
        except UserNotFoundError as exc:
            emit_live_error('USER_NOT_FOUND', f'Không tìm thấy tài khoản TikTok {cleaned_user}.', exc)
            report_tiktok_status('error', error=f'Không tìm thấy tài khoản TikTok {cleaned_user}.', sessionid=sessionid)
            raise
        except UserOfflineError as exc:
            emit_live_error('USER_OFFLINE', f'Tài khoản {cleaned_user} hiện không phát LIVE.', exc)
            report_tiktok_status('error', error=f'Tài khoản {cleaned_user} hiện không phát LIVE.', sessionid=sessionid)
            raise
        except Exception as e:
            err_str = str(e)
            name = e.__class__.__name__
            if 'NotFound' in name:
                emit_live_error('USER_NOT_FOUND', f'Không tìm thấy tài khoản TikTok {cleaned_user}.', e)
                report_tiktok_status('error', error=f'Không tìm thấy tài khoản TikTok {cleaned_user}.', sessionid=sessionid)
                raise
            elif 'Offline' in name:
                emit_live_error('USER_OFFLINE', f'Tài khoản {cleaned_user} hiện không phát LIVE.', e)
                report_tiktok_status('error', error=f'Tài khoản {cleaned_user} hiện không phát LIVE.', sessionid=sessionid)
                raise

            is_sign_error = (
                'SIGN_NOT_200' in err_str or
                'Sign API' in err_str or
                '500' in err_str or
                'SignAPIError' in name
            )
            logger.exception(f'Lỗi TikTok listener (Lần {attempt}/{max_retries}): {e}')

            if is_sign_error:
                error_msg = 'Máy chủ chữ ký TikTok (Sign API) gặp lỗi HTTP 500. Vui lòng bấm KẾT NỐI lại để thử tạo phiên mới.'
            else:
                error_msg = f'Lỗi kết nối TikTok Live: {err_str[:120]}'

            if attempt < max_retries:
                report_tiktok_status('retrying', error=f'{error_msg} Đang thử lại ({attempt}/{max_retries})...', sessionid=sessionid)
                time.sleep(2.5)
            else:
                emit_live_error('UNKNOWN', error_msg, e)
                report_tiktok_status('error', error=error_msg, sessionid=sessionid)
                raise


if __name__ == '__main__':
    print('🚀 Starting TikTok Live listener...')
    print(f'   Sending TTS to: {LIVE_APP_URL}')
    print(f'   LLM Model: {OLLAMA_MODEL}')
    
    username = None
    sessionid = 'current'
    if len(sys.argv) > 1:
        username = sys.argv[1]
    if len(sys.argv) > 2:
        sessionid = sys.argv[2]

    if not username:
        username = os.getenv('TIKTOK_USER')

    if not username:
        username = '@s_live_ai'

    start_tiktok_listener(username, sessionid)
