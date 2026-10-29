import os
import time
import json
import uuid
import threading
import re
import random
from typing import List, Dict, Any, Optional

PLAYLIST_FILE = os.path.join(
    os.environ.get('LOCALAPPDATA') or os.path.expanduser('~'),
    'SLiveAI',
    'playlist_data.json'
)

DEFAULT_PRODUCTS: List[Dict[str, Any]] = []

def safe_print(*args, **kwargs):
    try:
        print(*args, **kwargs)
    except Exception:
        try:
            safe_args = [str(a).encode('ascii', errors='backslashreplace').decode('ascii') for a in args]
            print(*safe_args, **kwargs)
        except Exception:
            pass


class PlaylistManager:
    def __init__(self, storage_file: str = PLAYLIST_FILE):
        self.storage_file = storage_file
        self.lock = threading.RLock()
        
        self.products: List[Dict[str, Any]] = []
        self.config: Dict[str, Any] = {
            "auto_rotation": False,
            "default_duration": 180,
            "loop_mode": "sequential",  # sequential, single
            "auto_pin_tiktok": True,
            "auto_pin_on_comment": True,
            "auto_repin_30s": True,
            "recheck_pin_interval": 30,
            "transition_phrase": True,
            "instant_transition": True
        }
        
        self.runtime_state: Dict[str, Any] = {
            "active_product_id": None,
            "active_product_index": 1,
            "active_product_title": "",
            "rotation_running": False,
            "start_time": 0,
            "current_timer": 0,
            "remaining_seconds": 0,
            "last_switch_at": 0,
            "last_pin_check_at": 0,
            "last_pinned_at": 0,
            "current_pinned_index": None,
            "current_pinned_title": None,
            "extension_connected": False,
            "extension_last_seen": 0,
            "extension_version": "1.0.3",
            "recent_rotation_indices": [],
            "recent_direct_pin_indices": []
        }
        
        self.pending_actions: List[Dict[str, Any]] = []
        self._load_data()
        
    def _ensure_dir(self):
        dir_path = os.path.dirname(self.storage_file)
        if dir_path:
            os.makedirs(dir_path, exist_ok=True)
            
    def _load_data(self):
        try:
            self._ensure_dir()
            if os.path.exists(self.storage_file):
                with open(self.storage_file, 'r', encoding='utf-8') as f:
                    saved = json.load(f)
                    raw_prods = saved.get('products', DEFAULT_PRODUCTS)
                    self.products = [
                        p for p in raw_prods
                        if not str(p.get('id', '')).startswith('prod_demo_')
                        and not str(p.get('title', '')).startswith('Khăn giấy đa năng')
                        and not str(p.get('title', '')).startswith('Nước lau sàn')
                        and not str(p.get('title', '')).startswith('Áo thun nam')
                    ]
                    self.config.update(saved.get('config', {}))
            else:
                self.products = list(DEFAULT_PRODUCTS)
                self._save_data()

            if not self.products:
                self.runtime_state["active_product_id"] = None
                self.runtime_state["active_product_index"] = None
                self.runtime_state["active_product_title"] = ""
                self.runtime_state["rotation_running"] = False
                self.runtime_state["remaining_seconds"] = 0
                self.runtime_state["start_time"] = 0
            elif self.config.get("auto_rotation", False):
                self.runtime_state["rotation_running"] = True
                self.runtime_state["start_time"] = time.time()
                self.runtime_state["last_switch_at"] = time.time()
                if not self.runtime_state.get("active_product_id"):
                    first_prod = self.products[0]
                    self.runtime_state["active_product_id"] = first_prod.get("id")
                    self.runtime_state["active_product_index"] = first_prod.get("index", 1)
                    self.runtime_state["active_product_title"] = first_prod.get("title", "")
        except Exception as e:
            print(f"[PLAYLIST] Error loading playlist: {e}")
            self.products = list(DEFAULT_PRODUCTS)

    def _save_data(self):
        try:
            self._ensure_dir()
            payload = {
                "products": self.products,
                "config": self.config,
                "updated_at": int(time.time())
            }
            temp_file = f"{self.storage_file}.{uuid.uuid4().hex}.tmp"
            with open(temp_file, 'w', encoding='utf-8') as f:
                json.dump(payload, f, ensure_ascii=False, indent=2)
            os.replace(temp_file, self.storage_file)
        except Exception as e:
            print(f"[PLAYLIST] Error saving playlist: {e}")

    def get_state(self) -> Dict[str, Any]:
        with self.lock:
            # Check extension heartbeat timeout (10 seconds)
            is_ext_alive = (time.time() - self.runtime_state["extension_last_seen"]) < 10
            self.runtime_state["extension_connected"] = is_ext_alive
            
            if not self.products:
                self.runtime_state["active_product_id"] = None
                self.runtime_state["active_product_index"] = None
                self.runtime_state["active_product_title"] = ""
                self.runtime_state["remaining_seconds"] = 0
                self.runtime_state["start_time"] = 0
                self.runtime_state["rotation_running"] = False
            elif self.config.get("auto_rotation", False):
                self.runtime_state["rotation_running"] = True
                if self.runtime_state["start_time"] <= 0:
                    self.runtime_state["start_time"] = time.time()
                    self.runtime_state["last_switch_at"] = time.time()
                    if not self.runtime_state.get("active_product_id") and self.products:
                        self.runtime_state["active_product_id"] = self.products[0]["id"]
                        self.runtime_state["active_product_index"] = self.products[0]["index"]
                        self.runtime_state["active_product_title"] = self.products[0]["title"]
            
            is_running = self.runtime_state["rotation_running"] or self.config.get("auto_rotation", False)
            if is_running and self.runtime_state["start_time"] > 0 and self.products:
                elapsed = int(time.time() - self.runtime_state["start_time"])
                active_prod = self._get_product_by_id(self.runtime_state["active_product_id"])
                duration = int(self.config.get("default_duration") or (active_prod.get("duration") if active_prod else 60) or 60)
                rem = max(0, duration - elapsed)
                self.runtime_state["remaining_seconds"] = rem
                self.runtime_state["current_timer"] = elapsed
            else:
                self.runtime_state["remaining_seconds"] = 0

            return {
                "products": self.products,
                "config": self.config,
                "runtime": dict(self.runtime_state),
                "server_time": int(time.time())
            }

    def _get_product_by_id(self, prod_id: Optional[str]) -> Optional[Dict[str, Any]]:
        if not prod_id:
            return None
        for p in self.products:
            if p.get("id") == prod_id:
                return p
        return None

    def _get_product_by_index(self, index: int) -> Optional[Dict[str, Any]]:
        for p in self.products:
            if p.get("index") == index:
                return p
        return None

    def _find_product_by_title(self, title: str) -> Optional[Dict[str, Any]]:
        if not title:
            return None
        t_clean = title.strip().lower()
        for p in self.products:
            p_title = (p.get("title") or "").strip().lower()
            if p_title and (p_title == t_clean or p_title in t_clean or t_clean in p_title):
                return p
        t_short = t_clean[:20]
        if len(t_short) >= 5:
            for p in self.products:
                p_title = (p.get("title") or "").strip().lower()
                if p_title and (t_short in p_title or p_title[:20] in t_clean):
                    return p
        return None

    def update_config(self, new_config: Dict[str, Any]) -> Dict[str, Any]:
        with self.lock:
            self.config.update(new_config)
            if "default_duration" in new_config:
                new_dur = int(new_config["default_duration"])
                self.config["default_duration"] = new_dur
                for p in self.products:
                    p["duration"] = new_dur
            self._save_data()
            return dict(self.config)

    def save_products(self, products_list: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
        with self.lock:
            cleaned = []
            for idx, p in enumerate(products_list):
                item = {
                    "id": p.get("id") or f"prod_{uuid.uuid4().hex[:8]}",
                    "index": int(p.get("index") or (idx + 1)),
                    "title": str(p.get("title") or f"Sản phẩm #{idx+1}").strip(),
                    "price": str(p.get("price") or "").strip(),
                    "link": str(p.get("link") or "").strip(),
                    "tiktok_product_id": str(p.get("tiktok_product_id") or "").strip(),
                    "script": str(p.get("script") or "").strip(),
                    "duration": int(p.get("duration") or self.config.get("default_duration", 180)),
                    "auto_pin": bool(p.get("auto_pin", True)),
                    "enabled": bool(p.get("enabled", True))
                }
                cleaned.append(item)
            self.products = cleaned
            self._save_data()
            return self.products

    def add_product(self, product_data: Dict[str, Any]) -> Dict[str, Any]:
        with self.lock:
            new_index = len(self.products) + 1
            item = {
                "id": product_data.get("id") or f"prod_{uuid.uuid4().hex[:8]}",
                "index": int(product_data.get("index") or new_index),
                "title": str(product_data.get("title") or f"Sản phẩm #{new_index}").strip(),
                "price": str(product_data.get("price") or "").strip(),
                "link": str(product_data.get("link") or "").strip(),
                "tiktok_product_id": str(product_data.get("tiktok_product_id") or "").strip(),
                "script": str(product_data.get("script") or "").strip(),
                "duration": int(product_data.get("duration") or self.config.get("default_duration", 180)),
                "auto_pin": bool(product_data.get("auto_pin", True)),
                "enabled": bool(product_data.get("enabled", True))
            }
            self.products.append(item)
            self._save_data()
            return item

    def delete_product(self, prod_id: str) -> bool:
        with self.lock:
            orig_len = len(self.products)
            self.products = [p for p in self.products if p.get("id") != prod_id]
            for i, p in enumerate(self.products):
                p["index"] = i + 1
            if len(self.products) < orig_len:
                if not self.products:
                    self.runtime_state["active_product_id"] = None
                    self.runtime_state["active_product_index"] = None
                    self.runtime_state["active_product_title"] = ""
                    self.runtime_state["remaining_seconds"] = 0
                    self.runtime_state["start_time"] = 0
                    self.runtime_state["rotation_running"] = False
                elif self.runtime_state.get("active_product_id") == prod_id:
                    self.runtime_state["active_product_id"] = self.products[0]["id"]
                    self.runtime_state["active_product_index"] = self.products[0]["index"]
                    self.runtime_state["active_product_title"] = self.products[0]["title"]
                self._save_data()
                return True
            return False

    def clear_all(self) -> List[Dict[str, Any]]:
        with self.lock:
            self.products = []
            self.runtime_state["active_product_id"] = None
            self.runtime_state["active_product_index"] = None
            self.runtime_state["active_product_title"] = ""
            self.runtime_state["remaining_seconds"] = 0
            self.runtime_state["start_time"] = 0
            self.runtime_state["rotation_running"] = False
            self.pending_actions.clear()
            self._save_data()
            return []


    @staticmethod
    def clean_product_title_for_speech(title: str) -> str:
        """Làm sạch tiêu đề sản phẩm để MC ảo đọc tự nhiên, gãy gọn như idol live thật."""
        if not title:
            return "sản phẩm"
        cleaned = re.sub(r'\[[^\]]*\]', ' ', title)
        cleaned = re.sub(r'\([^\)]*\)', ' ', cleaned)
        cleaned = re.sub(r'【[^】]*】', ' ', cleaned)
        cleaned = re.sub(r'\{[^\}]*\}', ' ', cleaned)
        cleaned = re.sub(r'^#?\s*(?:sản\s*phẩm|sp|mẫu|deal|mã)?\s*#?\s*\d+[\s:.\-]+', '', cleaned, flags=re.IGNORECASE)
        cleaned = re.sub(r'\b(?:chính\s*hãng|cao\s*cấp|hot\s*deal|sale\s*sốc|freeship(?:\s*toàn\s*quốc)?|xả\s*kho|siêu\s*rẻ|giảm\s*giá|chất\s*lượng)\b', ' ', cleaned, flags=re.IGNORECASE)
        cleaned = re.sub(r'[#*_\-~|]', ' ', cleaned)
        cleaned = re.sub(r'\s+', ' ', cleaned).strip()
        words = [w for w in cleaned.split() if w][:6]
        return " ".join(words) if words else "món này"

    @staticmethod
    def clean_script_greetings(text: str) -> str:
        """Làm sạch các câu chào đầu rập khuôn và STT khi chuyển sản phẩm giữa phiên live."""
        if not text:
            return ""
        cleaned = text.strip()
        patterns = [
            r'^(?:dạ\s+)?(?:em\s+)?(?:xin\s+)?chào\s+(?:cả\s+nhà|mọi\s+người|quý\s+vị|các\s+bác|anh\s+chị|mình|tất\s+cả\s+mọi\s+người)(?:\s+nhé|\s+nha|\s+ạ)?(?:[,.!?\-–—\s]+)',
            r'^(?:chào\s+mừng\s+(?:cả\s+nhà|mọi\s+người|các\s+bạn|quý\s+vị)\s+(?:đã\s+)?(?:đến\s+với|ghé\s+thăm)\s+(?:phiên|buổi)\s+live(?:stream)?(?:\s+hôm\s+nay)?)(?:[,.!?\-–—\s]+)',
            r'^(?:hello|hi)\s+(?:cả\s+nhà|mọi\s+người|các\s+bạn)(?:[,.!?\-–—\s]+)',
            r'^(?:sản\s*phẩm\s+số\s+#?\d+|deal\s+số\s+#?\d+|siêu\s+phẩm\s+số\s+#?\d+|mẫu\s+số\s+#?\d+)\s*(?:là|:|\-)?\s*'
        ]
        for p in patterns:
            cleaned = re.sub(p, '', cleaned, flags=re.IGNORECASE).strip()
        return cleaned

    def _get_smart_non_repeating_index(self, array_length: int, history_key: str, history_limit: int = 6) -> int:
        """Chọn ngẫu nhiên vị trí nhưng loại trừ các vị trí vừa dùng gần đây để tránh lặp mẫu câu."""
        if array_length <= 1:
            return 0
        effective_limit = min(history_limit, max(1, array_length // 2))
        recent_list = self.runtime_state.setdefault(history_key, [])
        recent_set = set(recent_list[-effective_limit:])
        candidates = [i for i in range(array_length) if i not in recent_set]
        if not candidates:
            candidates = list(range(array_length))
        chosen = random.choice(candidates)
        recent_list.append(chosen)
        if len(recent_list) > 20:
            del recent_list[:-20]
        return chosen

    def build_transition_sentence(self, prev_prod: Optional[Dict[str, Any]], next_prod: Optional[Dict[str, Any]], context: str = 'auto') -> str:
        """Tạo câu nối tự nhiên, đời thường, cuốn hút khi chuyển hoặc ghim sản phẩm, tuyệt đối không rập khuôn."""
        if not next_prod:
            return ""
        if not self.config.get("transition_phrase", True):
            return ""

        next_title = self.clean_product_title_for_speech(next_prod.get("title", ""))

        # Ngữ cảnh 1: Chuyển vòng xoay giữa 2 sản phẩm (Rotation / Switch)
        if prev_prod and prev_prod.get("title") and prev_prod.get("id") != next_prod.get("id") and context != "pin":
            prev_title = self.clean_product_title_for_speech(prev_prod.get("title", ""))
            rotation_templates = [
                # Giao lưu & Chiều lòng khách
                f"Nãy giờ nhiều bác nhắn tin hỏi em món này quá trời, nên thôi em xin phép cho lên sóng luôn nha, chính là em {next_title} đây cả nhà ơi!",
                f"Chiều lòng các bác luôn nè, món trước ai chốt xong rồi thì giữ đơn, giờ em chuyển qua ngay em {next_title} theo yêu cầu của mọi người đây ạ!",
                f"Em thấy trong phần bình luận nhiều chị dặn phải lên món này sớm, dạ vâng em nghe lời cả nhà, lên liền em {next_title} cho nóng nhé!",
                f"Dạ ai nãy giờ đang đợi {next_title} thì nhìn lên cùng em nha, vừa xong món trước là em ưu tiên lên liền cho mọi người đây ạ!",
                f"Nhiều bác khen món trước nhưng vẫn ngóng em này đúng không ạ? Dạ em lên ngay {next_title} cho cả nhà mình đây rồi nhé!",

                # Trải nghiệm thực tế & Khen chất lượng
                f"Món vừa rồi đã mê thì qua tới em {next_title} này chỉ có mê hơn thôi cả nhà ơi, hàng về tay bao xịn xò chuẩn chỉnh từng chi tiết luôn!",
                f"Ai vừa chốt đơn trước xong thì hoan hỉ nha, còn bây giờ cùng em nghía qua {next_title}, em này em tự tin chấm điểm mười trên mười luôn á!",
                f"Tiếp theo là một em chân ái của nhà em luôn nè cả nhà, chính là {next_title}, dùng một lần là bảo đảm không muốn đổi sang loại khác đâu ạ!",
                f"Món trước xịn một thì em {next_title} này xịn mười nha các bác, riêng em này nhận hàng về cứ mở ra kiểm tra thoải mái cho em nhé!",
                f"Bác nào kỹ tính thích đồ bền đẹp thì nhìn ngay qua em {next_title} này nha, chất lượng vượt xa ngoài mong đợi của mọi người luôn đấy ạ!",

                # Deal bất ngờ & Khan hiếm
                f"Món trước em xin phép tạm dừng ở đây nha, giờ em lên nhanh {next_title}, mã này số lượng trong kho còn ít nên ai ưng là bấm tay chốt sớm nè!",
                f"Trời ơi qua tới em {next_title} này thì em xin được deal hời chấn động luôn cả nhà ơi, đúng phiên live hôm nay mới có giá này thôi á!",
                f"Các bác tranh thủ món trước nhé, còn bây giờ nhìn ngay vào {next_title} giúp em, em này được trợ giá sâu bất ngờ luôn cả nhà ơi!",
                f"Dạ {next_title} phiên này em để giá tri ân siêu mềm luôn ạ, số lượng mở bán có hạn nên mọi người tranh thủ chớp cơ hội liền nha!",
                f"Suất ưu đãi cho món trước gần hết rồi, giờ em mở tiếp deal khủng cho em {next_title}, ai nhanh tay thì còn chậm tay là tiếc hùi hụi nha!",

                # Chuyển cảnh mềm mại & Nhịp nhàng
                f"Rồi cả nhà ơi, tạm khép lại món cũ nha, giờ cùng em lướt sang siêu phẩm tiếp theo chính là em {next_title} xuất sắc không kém đây ạ!",
                f"Tiếp tục phiên live sôi động hôm nay nhé cả nhà, cùng em khám phá liền em {next_title} cực kỳ đáng đồng tiền bát gạo này nha!",
                f"Dạ em qua món mới liền cho nóng đây cả nhà mình ơi, nghía sang em {next_title} này bảo đảm cả nhà sẽ thích mê ngay từ cái nhìn đầu tiên luôn!",
                f"Cả nhà săn món trước xong chưa ạ, cùng em đến với tâm điểm tiếp theo của hôm nay nha, chính là em {next_title} siêu tiện lợi này đây ạ!",
                f"Chuyển món luôn cho nóng cả nhà ơi, em {next_title} này phiên live nào lên sóng cũng hết hàng sớm nhất luôn á!"
            ]
            chosen_idx = self._get_smart_non_repeating_index(len(rotation_templates), "recent_rotation_indices", history_limit=6)
            return rotation_templates[chosen_idx]

        # Ngữ cảnh 2: Ghim sản phẩm trực tiếp (Direct Pin / Single Product Focus)
        direct_pin_templates = [
            # Giao lưu & Chiều lòng khách
            f"Ui nãy giờ có mấy bác hỏi em này quá trời luôn, đây nha cả nhà ơi, em lên liền em {next_title} theo yêu cầu của mọi người nè!",
            f"Chiều lòng cả nhà mình luôn nha, em đưa ngay em {next_title} lên live đây, ai ưng mắt thì vào xem cận cảnh liền nhé!",
            f"Dạ em thấy mọi người quan tâm em này nhiều ghê, thôi thì lên sóng luôn cho nóng, chính là em {next_title} đây cả nhà ơi!",
            f"Các bác ơi, món mà ai cũng mong chờ hôm nay đã lên rồi nè, chính là em {next_title} cực kỳ hot đây ạ!",
            f"Em chiều ý các chị đẹp luôn nha, lên ngay {next_title} cho cả nhà cùng ngắm, ưng là chốt liền tay chứ đừng chần chừ nha!",

            # Trải nghiệm thực tế & Khen chất
            f"Riêng với em {next_title} này thì em bao chất lượng cho cả nhà luôn nha, nhận hàng về tay mở ra kiểm tra ưng ý mới thanh toán ạ!",
            f"Riêng em {next_title} này chính tay em đã trải nghiệm rồi cả nhà ơi, dùng mê dã man nên hôm nay tự tin khoe với tất cả mọi người luôn!",
            f"Bác nào kỹ tính thích hàng chuẩn xịn thì vào xem ngay em {next_title} này nhé, xuất sắc từ đường kim mũi chỉ đến chất liệu luôn ạ!",
            f"Với em {next_title} này thì em nói thật, không chốt đợt này là tiếc hùi hụi luôn cả nhà ơi, chất lượng vượt xa tầm giá luôn á!",
            f"Ai đang tìm một món vừa bền vừa đẹp thì nghía ngay qua {next_title} nha, khách nhà em mua xong ai cũng quay lại khen nức nở!",

            # Deal hời & Giới hạn
            f"Trời ơi tin được không cả nhà ơi, hôm nay em xin được đúng một ít mã trợ giá sâu cho {next_title} thôi, chớp cơ hội liền tay nha!",
            f"Deal này siêu hời luôn á mọi người ơi, bình thường không bao giờ có giá này đâu, ai đang canh {next_title} thì tranh thủ liền nha!",
            f"Suất ưu đãi cho em {next_title} mở ra có hạn thôi cả nhà ơi, ai nhanh tay thì kịp, chậm một nhịp là hết sạch mã đấy ạ!",
            f"Dạ hôm nay em mở quà bất ngờ cho phiên live nè, em {next_title} này giảm giá chấn động luôn, mọi người lướt qua xem liền nha!",
            f"Ai ưng {next_title} thì nhanh tay thêm vào giỏ liền kẻo hết nhé cả nhà, số lượng ưu đãi hôm nay em còn rất ít thôi ạ!",

            # Dẫn dắt lôi cuốn & Tự nhiên
            f"Cả nhà nhìn lên live cùng em nha, đây chính là tâm điểm hôm nay của nhà em nè, em {next_title} siêu hot đã có mặt rồi đây!",
            f"Lên sóng liền một siêu phẩm chân ái cho cả nhà đây ạ, chính là {next_title}, món này bảo đảm làm xiêu lòng tất cả các bác hôm nay luôn!",
            f"Em lên liền món siêu tiện lợi cho cả nhà mình đây, nhìn qua em {next_title} này là thấy mê liền rồi đúng không cả nhà!",
            f"Bác nào đang xem live thì dừng lại vài giây ngắm em {next_title} này với em nha, xuất sắc không có chỗ nào để chê luôn á!",
            f"Mở màn cho deal đỉnh tiếp theo nha cả nhà ơi, em {next_title} này xứng đáng là món không thể thiếu trong giỏ hàng hôm nay của mọi người luôn!"
        ]
        chosen_idx = self._get_smart_non_repeating_index(len(direct_pin_templates), "recent_direct_pin_indices", history_limit=6)
        return direct_pin_templates[chosen_idx]


    def select_product(self, prod_id_or_index: Any, trigger_pin: bool = True, reason: str = "manual") -> Optional[Dict[str, Any]]:
        with self.lock:
            target = None
            if isinstance(prod_id_or_index, int) or (isinstance(prod_id_or_index, str) and prod_id_or_index.isdigit()):
                target = self._get_product_by_index(int(prod_id_or_index))
            else:
                target = self._get_product_by_id(str(prod_id_or_index))
                
            if not target and self.products:
                target = self.products[0]
                
            if target:
                self.runtime_state["active_product_id"] = target["id"]
                self.runtime_state["active_product_index"] = target["index"]
                self.runtime_state["active_product_title"] = target["title"]
                self.runtime_state["last_switch_reason"] = reason
                self.runtime_state["start_time"] = time.time()
                self.runtime_state["last_switch_at"] = time.time()
                self.runtime_state["play_counter"] = self.runtime_state.get("play_counter", 0) + 1
                target["last_played_order"] = self.runtime_state["play_counter"]
                target["last_played_at"] = time.time()

                # Cập nhật live_prompt.txt ngay khi chọn sản phẩm
                script_text = target.get("script", "") or target.get("title", "")
                if script_text:
                    try:
                        prompt_path = os.path.join(os.getcwd(), 'live_prompt.txt')
                        with open(prompt_path, 'w', encoding='utf-8') as f:
                            f.write(f"Tên sản phẩm: {target['title']}. Giá: {target.get('price','')}. Kịch bản: {script_text}")
                    except Exception as e:
                        safe_print(f"Failed to write live_prompt.txt: {e}")
                
                if trigger_pin and (target.get("auto_pin") and self.config.get("auto_pin_tiktok", True)):
                    self.enqueue_pin_action(
                        action="PIN",
                        index=target["index"],
                        product_id=target.get("tiktok_product_id"),
                        title=target["title"]
                    )
            return target

    def get_next_product(self) -> Optional[Dict[str, Any]]:
        with self.lock:
            enabled_prods = [p for p in self.products if p.get("enabled", True)]
            if not enabled_prods:
                return None
            if len(enabled_prods) == 1:
                return enabled_prods[0]
                
            curr_id = self.runtime_state.get("active_product_id")
            # Loại trừ sản phẩm đang đọc hiện tại để không lặp lại ngay lập tức
            candidates = [p for p in enabled_prods if p["id"] != curr_id]
            if not candidates:
                candidates = enabled_prods

            # Fair Round-Robin: Chọn sản phẩm chưa được đọc lâu nhất (last_played_order nhỏ nhất)
            # Khắc phục triệt để vấn đề bị lặp qua lại giữa 2 sản phẩm đầu khi TikTok liên tục đẩy sản phẩm vừa ghim lên #1
            candidates.sort(key=lambda p: (p.get("last_played_order", 0), p.get("last_played_at", 0), p.get("index", 0)))
            return candidates[0]

    def match_product_from_comment(self, comment_text: str) -> Optional[Dict[str, Any]]:
        """
        Detect if a viewer's comment refers to a specific product in the playlist.
        Returns the matched product dictionary, or None.
        """
        if not comment_text or not self.products:
            return None

        text = comment_text.strip().lower()
        active_products = [p for p in self.products if p.get("enabled", True)]
        if not active_products:
            active_products = self.products

        # 1. Match by index/number
        # 1A: Direct hashtag like #1, #2, #03
        m_hash = re.search(r'#\s*(\d{1,2})\b', text)
        if m_hash:
            idx = int(m_hash.group(1))
            for p in active_products:
                if p.get("index") == idx:
                    return p

        # 1B: Keywords (sp, san pham, mẫu, mau, số, cái, món, ghim, xem, link) + number
        pattern = r'(?:\b(?:sp|s[aả]n\s*ph[aẩ]m|m[aẫ]u(?:\s*s[oố])?|s[oố]|c[aá]i|m[oó]n|ghim(?:\s*s[oố])?|xem(?:\s*s[oố])?|link)\s*#?\s*(\d{1,2})\b)'
        m_kw = re.search(pattern, text)
        if m_kw:
            idx = int(m_kw.group(1))
            for p in active_products:
                if p.get("index") == idx:
                    return p

        # 1C: Compact format like "sp1", "sp02"
        m_compact = re.search(r'\bsp(\d{1,2})\b', text)
        if m_compact:
            idx = int(m_compact.group(1))
            for p in active_products:
                if p.get("index") == idx:
                    return p

        # 2. Match by product title keywords
        clean_comment = re.sub(r'[^\w\s]', ' ', text)
        comment_words = set(clean_comment.split())

        best_prod = None
        best_score = 0

        # Common stop words in Vietnamese e-commerce to ignore
        stop_words = {
            'cao', 'cấp', 'chính', 'hãng', 'hot', 'deal', 'giá', 'rẻ', 'combo', 
            'loại', 'mới', 'nhất', 'siêu', 'đẹp', 'tốt', 'hàng', 'ngày', 'được', 
            'cho', 'với', 'trong', 'các', 'của', 'và', 'này', 'nước', 'sấy', 'thơm',
            'ngon', 'rất', 'quá', 'nhé', 'nha', 'shop', 'ơi', 'em', 'anh', 'chị'
        }

        for p in active_products:
            title = (p.get("title") or "").lower()
            clean_title = re.sub(r'[^\w\s]', ' ', title)
            title_words = [w for w in clean_title.split() if len(w) >= 2]
            
            meaningful_words = [w for w in title_words if w not in stop_words]
            if not meaningful_words:
                meaningful_words = title_words

            # Check 2-word phrase matches (e.g. "chanh dây", "kim quất", "áo thun", "khăn giấy")
            matched_phrases = 0
            for i in range(len(meaningful_words) - 1):
                phrase = f"{meaningful_words[i]} {meaningful_words[i+1]}"
                if phrase in clean_comment:
                    matched_phrases += 1

            # Count individual word matches
            matched_words = sum(1 for w in meaningful_words if w in comment_words)

            score = (matched_phrases * 3) + matched_words

            # Need at least one 2-word phrase match OR at least 2 distinct words
            if (matched_phrases >= 1 or matched_words >= 2) and score > best_score:
                best_score = score
                best_prod = p

        if best_prod:
            return best_prod

        return None

    def enqueue_pin_action(self, action: str, index: int, product_id: Optional[str] = "", title: str = ""):
        payload = {
            "action_id": uuid.uuid4().hex,
            "action": action,
            "index": index,
            "product_id": product_id or "",
            "title": title or "",
            "created_at": time.time()
        }
        if len(self.pending_actions) >= 5:
            self.pending_actions.pop(0)
        self.pending_actions.append(payload)
        safe_print(f"[PLAYLIST TIKTOK PIN] Enqueued action: {action} on product #{index} ({title[:30]})")

    def pop_pending_actions(self) -> List[Dict[str, Any]]:
        with self.lock:
            actions = list(self.pending_actions)
            self.pending_actions.clear()
            return actions

    def get_current_speaking_product(self) -> Optional[Dict[str, Any]]:
        """
        Xác định chính xác sản phẩm đang nói tương ứng với kịch bản hiện tại.
        Đảm bảo ghim đúng sản phẩm với kịch bản:
        1. Phân tích live_prompt.txt (nếu có nhãn 'Tên sản phẩm: {title}' hoặc khớp nội dung kịch bản)
        2. active_product_id trong runtime_state
        3. Sản phẩm đầu tiên trong giỏ hàng
        """
        with self.lock:
            if not self.products:
                return None

            # 1. So khớp từ live_prompt.txt để bắt đúng kịch bản đang phát trên avatar
            try:
                prompt_path = os.path.join(os.getcwd(), 'live_prompt.txt')
                if os.path.exists(prompt_path):
                    with open(prompt_path, 'r', encoding='utf-8') as f:
                        prompt_content = f.read().strip()
                    
                    if prompt_content:
                        # Tìm cấu trúc "Tên sản phẩm: ..."
                        m_title = re.search(r'Tên sản phẩm:\s*([^.\n]+)', prompt_content, re.IGNORECASE)
                        if m_title:
                            extracted_title = m_title.group(1).strip()
                            matched = self._find_product_by_title(extracted_title)
                            if matched:
                                if self.runtime_state.get("active_product_id") != matched["id"]:
                                    self.runtime_state["active_product_id"] = matched["id"]
                                    self.runtime_state["active_product_index"] = matched["index"]
                                    self.runtime_state["active_product_title"] = matched["title"]
                                return matched

                        # So khớp nội dung kịch bản script
                        for p in self.products:
                            p_script = (p.get("script") or "").strip()
                            if p_script and len(p_script) >= 15 and (p_script[:40] in prompt_content or prompt_content[:40] in p_script):
                                if self.runtime_state.get("active_product_id") != p["id"]:
                                    self.runtime_state["active_product_id"] = p["id"]
                                    self.runtime_state["active_product_index"] = p["index"]
                                    self.runtime_state["active_product_title"] = p["title"]
                                return p

                        # So khớp tiêu đề sản phẩm trong prompt
                        for p in self.products:
                            p_title = (p.get("title") or "").strip()
                            if p_title and len(p_title) >= 5 and p_title.lower() in prompt_content.lower():
                                if self.runtime_state.get("active_product_id") != p["id"]:
                                    self.runtime_state["active_product_id"] = p["id"]
                                    self.runtime_state["active_product_index"] = p["index"]
                                    self.runtime_state["active_product_title"] = p["title"]
                                return p
            except Exception as e:
                safe_print(f"[PLAYLIST] Lỗi đọc prompt matching: {e}")

            # 2. Sử dụng active_product_id từ runtime_state
            curr_id = self.runtime_state.get("active_product_id")
            if curr_id:
                prod = self._get_product_by_id(curr_id)
                if prod:
                    return prod

            # 3. Mặc định sản phẩm đầu tiên
            first_prod = self.products[0]
            self.runtime_state["active_product_id"] = first_prod["id"]
            self.runtime_state["active_product_index"] = first_prod["index"]
            self.runtime_state["active_product_title"] = first_prod["title"]
            return first_prod

    def record_extension_heartbeat(self, status_data: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        with self.lock:
            self.runtime_state["extension_connected"] = True
            self.runtime_state["extension_last_seen"] = time.time()
            new_idx = status_data.get("current_pinned_index")
            new_title = status_data.get("current_pinned_title")

            if "current_pinned_index" in status_data:
                self.runtime_state["current_pinned_index"] = new_idx
            if "current_pinned_title" in status_data:
                self.runtime_state["current_pinned_title"] = new_title

            # Chỉ tự động đổi kịch bản theo sản phẩm ghim trên TikTok nếu tính năng auto-pin/auto-repin/rotation đang TẮT
            # và người dùng tự tay bấm ghim trên TikTok, hoặc khi chưa có sản phẩm nào được chọn.
            # TUYỆT ĐỐI KHÔNG để ghim cũ trên TikTok đè ngược lại kịch bản đang nói!
            switched_prod = None
            if new_idx is not None or new_title:
                matched = None
                if new_idx is not None:
                    matched = self._get_product_by_index(int(new_idx))
                if not matched and new_title:
                    matched = self._find_product_by_title(new_title)

                curr_active_id = self.runtime_state.get("active_product_id")
                auto_pin_enabled = self.config.get("auto_pin_tiktok", True)
                repin_enabled = self.config.get("auto_repin_30s", True)
                rotation_running = self.runtime_state.get("rotation_running", False)

                if matched and matched.get("id") != curr_active_id:
                    if not curr_active_id or (not auto_pin_enabled and not repin_enabled and not rotation_running):
                        safe_print(f"[PLAYLIST TIKTOK PIN SYNC] Phát hiện người dùng ghim thủ công SP #{matched.get('index')}: '{matched.get('title')[:30]}' trên TikTok. Đồng bộ kịch bản!")
                        switched_prod = self.select_product(matched["id"], trigger_pin=False, reason="tiktok_pin")

            return switched_prod

    def check_and_repin_active_product(self) -> Optional[Dict[str, Any]]:
        """
        Định kỳ kiểm tra xem sản phẩm hiện tại (active product theo kịch bản) có đang được ghim trên TikTok không.
        - Nếu chưa ghim: enqueue action PIN để ghim sản phẩm đang nói.
        - Nếu đã ghim rồi: enqueue action REPIN để bỏ ghim và ghim lại sản phẩm đang nói sau mỗi 30s.
        Phải đảm bảo ghim đúng sản phẩm với kịch bản.
        """
        with self.lock:
            # 1. Kiểm tra cấu hình có cho phép auto-pin và auto-repin không
            if not self.config.get("auto_pin_tiktok", True):
                return None
            if not self.config.get("auto_repin_30s", True):
                return None

            interval = max(10, int(self.config.get("recheck_pin_interval", 30)))
            now = time.time()
            last_check = self.runtime_state.get("last_pin_check_at", 0)

            if now - last_check < interval:
                return None

            self.runtime_state["last_pin_check_at"] = now

            # 2. Lấy sản phẩm hiện tại ĐANG NÓI VỚI KỊCH BẢN
            active_prod = self.get_current_speaking_product()
            if not active_prod:
                return None

            if not active_prod.get("auto_pin", True):
                return None

            # 3. Kiểm tra sản phẩm đang ghim trên TikTok
            current_pinned_title = (self.runtime_state.get("current_pinned_title") or "").strip().lower()
            current_pinned_index = self.runtime_state.get("current_pinned_index")
            active_title = active_prod.get("title", "").strip().lower()
            clean_active = self.clean_product_title_for_speech(active_title).lower()

            is_currently_pinned = False
            if current_pinned_index is not None and int(current_pinned_index) == int(active_prod.get("index", -1)):
                is_currently_pinned = True
            elif current_pinned_title:
                if active_title in current_pinned_title or current_pinned_title in active_title:
                    is_currently_pinned = True
                elif clean_active and clean_active in current_pinned_title:
                    is_currently_pinned = True

            # 4. Nếu chưa ghim -> Ghim sản phẩm đang nói
            if not is_currently_pinned:
                safe_print(f"[PLAYLIST AUTO-PIN {interval}s] SP #{active_prod['index']}: '{active_prod['title'][:25]}' chưa ghim trên TikTok -> Tự động ghim!")
                self.enqueue_pin_action(
                    action="PIN",
                    index=active_prod["index"],
                    product_id=active_prod.get("tiktok_product_id", ""),
                    title=active_prod["title"]
                )
                self.runtime_state["last_pinned_at"] = now
                return active_prod
            else:
                # 5. Nếu đã ghim rồi -> Bỏ ghim và ghim lại sản phẩm đang nói sau mỗi 30s
                safe_print(f"[PLAYLIST AUTO-REPIN {interval}s] SP #{active_prod['index']}: '{active_prod['title'][:25]}' ĐÃ GHIM -> Tự động BỎ GHIM & GHIM LẠI để kích hoạt pop-up trên TikTok!")
                self.enqueue_pin_action(
                    action="REPIN",
                    index=active_prod["index"],
                    product_id=active_prod.get("tiktok_product_id", ""),
                    title=active_prod["title"]
                )
                self.runtime_state["last_pinned_at"] = now
                return active_prod

    def sync_scraped_cart(self, scraped_products: List[Dict[str, Any]], allow_empty: bool = False) -> List[Dict[str, Any]]:
        """Sync products scraped directly from TikTok Shop Live Dashboard into playlist."""
        with self.lock:
            existing_by_prod_id = {p.get("tiktok_product_id"): p for p in self.products if p.get("tiktok_product_id")}
            existing_by_title = {p.get("title", "").strip().lower(): p for p in self.products if p.get("title")}

            blacklisted_keywords = [
                "product list in this live", "orders over",
                "give free prizes", "increase engagement", "lucky draw",
                "tặng quà cho người xem", "tạo voucher", "vòng quay may mắn",
                "free prize", "giveaway", "call-to-action", "next live time"
            ]

            valid_scraped = []
            for item in (scraped_products or []):
                t = str(item.get("title") or "").strip()
                if not t or len(t) < 3:
                    continue
                lower_t = t.lower()
                if any(kw in lower_t for kw in blacklisted_keywords):
                    safe_print(f"[PLAYLIST MANAGER] Bỏ qua card phi-sản-phẩm/khuyến mãi/tips: {t}")
                    continue

                # Lọc bỏ thẻ voucher độc lập (không đọc, không ghim theo yêu cầu người dùng)
                is_pure_voucher = any(kw in lower_t for kw in ["voucher", "phiếu giảm giá", "mã giảm giá", "coupon"]) and \
                                  not any(kw in lower_t for kw in ["áo", "quần", "váy", "đầm", "giày", "dép", "túi", "kem", "son", "sữa", "nước", "hộp", "bộ", "set", "combo", "gói", "chai", "bình", "máy", "tai nghe", "dây", "khăn", "trà", "bánh", "kẹo", "thịt", "cá", "hạt", "dầu", "men", "viên", "bút", "sách", "đèn", "kính", "đồng hồ", "nón", "mũ"]) and \
                                  not item.get("tiktok_product_id")
                if is_pure_voucher and ("giảm" in lower_t or "đơn từ" in lower_t or "tối thiểu" in lower_t or len(t) < 35):
                    safe_print(f"[PLAYLIST MANAGER] Bỏ qua thẻ voucher độc lập (không ghim, không đọc): {t}")
                    continue

                valid_scraped.append(item)

            if not valid_scraped:
                if self.products and not allow_empty:
                    safe_print(f"[PLAYLIST MANAGER] Quét được 0 SP nhưng đang có {len(self.products)} SP trong playlist. Giữ nguyên danh sách để tránh mất dữ liệu.")
                    return self.products
                self.products = []
                self.runtime_state["active_product_id"] = None
                self.runtime_state["active_product_index"] = None
                self.runtime_state["active_product_title"] = None
                self.runtime_state["last_sync_time"] = time.time()
                self.runtime_state["last_synced_count"] = 0
                self._save_data()
                safe_print("[PLAYLIST MANAGER] Giỏ hàng TikTok trống -> Đã làm sạch danh sách sản phẩm trên app.")
                return []

            merged = []
            for i, item in enumerate(valid_scraped):
                idx = i + 1
                title = str(item.get("title") or f"Sản phẩm #{idx}").strip()
                price = str(item.get("price") or "").strip()
                link = str(item.get("link") or "").strip()
                prod_id = str(item.get("tiktok_product_id") or "").strip()

                existing = None
                if prod_id and prod_id in existing_by_prod_id:
                    existing = existing_by_prod_id[prod_id]
                elif title.lower() in existing_by_title:
                    existing = existing_by_title[title.lower()]

                if existing and existing.get("script"):
                    # Bảo lưu 100% kịch bản người dùng đã tùy chỉnh và cài đặt của sản phẩm cũ
                    script = existing["script"]
                    duration = existing.get("duration", self.config.get("default_duration", 60))
                    auto_pin = existing.get("auto_pin", True)
                    enabled = existing.get("enabled", True)
                    item_id = existing.get("id") or f"prod_{uuid.uuid4().hex[:8]}"
                    last_played_at = existing.get("last_played_at", 0)
                    last_played_order = existing.get("last_played_order", 0)
                else:
                    script = (
                        f"Deal cực hot đang được ghim ngay trên màn hình lúc này là {title}, "
                        f"giá hôm nay ưu đãi chỉ còn {price or 'siêu hời'}. Số lượng deal trên phiên live có hạn thôi cả nhà ơi, "
                        f"mọi người bấm ngay vào sản phẩm đang ghim hoặc giỏ hàng góc trái màn hình để chốt đơn ngay nhé!"
                    )
                    duration = int(item.get("duration") or (existing.get("duration") if existing else self.config.get("default_duration", 60)))
                    auto_pin = bool(item.get("auto_pin", True))
                    enabled = bool(item.get("enabled", True))
                    item_id = (existing.get("id") if existing else None) or f"prod_{uuid.uuid4().hex[:8]}"
                    last_played_at = existing.get("last_played_at", 0) if existing else 0
                    last_played_order = existing.get("last_played_order", 0) if existing else 0

                merged.append({
                    "id": item_id,
                    "index": idx,
                    "title": title,
                    "price": price,
                    "link": link,
                    "tiktok_product_id": prod_id,
                    "script": script,
                    "duration": duration,
                    "auto_pin": auto_pin,
                    "enabled": enabled,
                    "is_pinned": bool(item.get("is_pinned", False)),
                    "last_played_at": last_played_at,
                    "last_played_order": last_played_order
                })

            self.runtime_state["last_sync_time"] = time.time()
            self.runtime_state["last_synced_count"] = len(merged)
            self.products = merged

            # Ghi nhận sản phẩm đang ghim từ TikTok
            pinned_item = next((p for p in merged if p.get("is_pinned")), None)
            if pinned_item:
                self.runtime_state["current_pinned_index"] = pinned_item["index"]
                self.runtime_state["current_pinned_title"] = pinned_item["title"]

            # Cập nhật số thứ tự index và tiêu đề của active product theo vị trí mới
            active_id = self.runtime_state.get("active_product_id")
            active_match = next((p for p in merged if p["id"] == active_id), None) if active_id else None

            if active_match:
                self.runtime_state["active_product_index"] = active_match["index"]
                self.runtime_state["active_product_title"] = active_match["title"]
            elif pinned_item:
                self.runtime_state["active_product_id"] = pinned_item["id"]
                self.runtime_state["active_product_index"] = pinned_item["index"]
                self.runtime_state["active_product_title"] = pinned_item["title"]
            elif merged:
                self.runtime_state["active_product_id"] = merged[0]["id"]
                self.runtime_state["active_product_index"] = merged[0]["index"]
                self.runtime_state["active_product_title"] = merged[0]["title"]

            self._save_data()
            safe_print(f"[PLAYLIST MANAGER] Successfully synced {len(merged)} products from TikTok Shop Live Cart.")
            return self.products

    def request_sync_cart(self):
        """Enqueue action telling Extension to scrape TikTok Live Cart."""
        payload = {
            "action_id": uuid.uuid4().hex,
            "action": "SYNC_CART",
            "created_at": time.time()
        }
        self.pending_actions.append(payload)
        safe_print("[PLAYLIST TIKTOK SYNC] Enqueued SYNC_CART request to Chrome Extension.")

playlist_manager = PlaylistManager()
