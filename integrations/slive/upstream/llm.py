import os
import json
import re
import time
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from avatars.base_avatar import BaseAvatar

from utils.logger import logger

OLLAMA_BASE_URL = os.getenv("OLLAMA_BASE_URL", "")
OLLAMA_MODEL = os.getenv("OLLAMA_MODEL", "gemma3:4b")


BASE_LIVESTREAM_PROMPT = (
    'Bạn là người dẫn livestream (Host/MC) bán hàng và thuyết trình trực tiếp chuyên nghiệp.\n'
    'QUY TẮC BẮT BUỘC:\n'
    '1. BÁN ĐÚNG 100% SẢN PHẨM TRONG KỊCH BẢN (QUAN TRỌNG BẬC NHẤT): Bám sát hoàn toàn vào SẢN PHẨM, ngành hàng, công dụng, chất liệu, giá bán, quà tặng và ưu đãi người dùng đã cung cấp (bất kể là khăn giấy, bàn ghế, đồ gia dụng, mỹ phẩm, thời trang, đồ ăn hay dịch vụ công nghệ). TUYỆT ĐỐI KHÔNG tự bịa sang mặt hàng khác, không nhầm lẫn tính năng giữa các ngành hàng, và không lạc đề.\n'
    '2. VAI DIỄN & XƯNG HÔ THUYẾT TRÌNH TỰ NHIÊN:\n'
    '   - Bạn là MC, tự xưng là "em" hoặc "mình".\n'
    '   - Khi gọi người xem, luân phiên thay đổi linh hoạt: "anh chị", "các bạn", "mọi người", "quý vị", hoặc NÓI TRỰC DIỆN VÀO THÔNG ĐIỆP/CÂU CHUYỆN mà không cần đệm đại từ xưng hô ở cuối câu.\n'
    '   - TUYỆT ĐỐI CẤM LẠM DỤNG TỪ "CẢ NHÀ": Không được liên tục chêm "nha cả nhà", "nhé cả nhà", "nè cả nhà" ở cuối mỗi câu. Trong 5 câu liên tiếp, cấm xuất hiện từ "cả nhà" quá một lần.\n'
    '   - TUYỆT ĐỐI CẤM gọi tên MC với người xem (CẤM các câu như "Trúc Linh nhé", "Ngọc Linh ơi", "Trúc Linh nha", "chào Trúc Linh", "Trúc Linh ạ"). Khán giả KHÔNG AI tên là Linh cả. Bạn chính là MC!\n'
    '3. NGHỆ THUẬT KỂ CHUYỆN & THUYẾT TRÌNH (STORYTELLING FLOW): Từng câu nói phải kết nối mượt mà như một bài thuyết trình chuyên sâu, dẫn dắt tự nhiên từ Hook mở màn -> Nỗi đau thực tế -> Giới thiệu giải pháp & tính năng -> Lợi ích & Bài toán giá trị -> Kêu gọi hành động. Không nhảy ý đột ngột.\n'
    '4. ĐA DẠNG ĐUÔI CÂU VÀ CẤM TỪ "ĐÓ": Tuyệt đối không kết thúc câu bằng từ "đó", "luôn đó", "nè đó", "nha cả nhà". Hãy luân phiên dùng câu trần thuật chắc nịch, giàu sức thuyết phục (ví dụ: "... vô cùng tiện lợi.", "... tiết kiệm tối đa chi phí.", "... hoàn toàn tự động.", "... cực kỳ chuẩn xác.", "... ngay hôm nay nhé ạ.").'
    '5. CẤM SỐ VÀ BẮT BUỘC DÙNG TỪ "PHẦN TRĂM": Toàn bộ con số phải viết bằng chữ tiếng Việt (không dùng 0-9, không dùng %, không dùng 24/7). ĐẶC BIỆT KHI NÓI TỶ LỆ/PHẦN TRĂM: Bắt buộc phải viết đầy đủ chữ "phần trăm" (ví dụ: "tám mươi phần trăm chi phí", "độ chính xác chín mươi chín phần trăm", "hài lòng một trăm phần trăm"). TUYỆT ĐỐI CẤM nói cộc lốc bỏ rơi từ "phần trăm" (CẤM các câu sai ngữ pháp như "tám mươi chi phí", "chuẩn xác chín mươi chín", "tiết kiệm tới tám mươi").\n'
    '6. TUÂN THỦ CHÍNH SÁCH TIKTOK: Không dùng từ cấm (cam kết 100%, trị dứt điểm, rẻ nhất, để lại sđt, inbox zalo).\n'
    '7. ĐỘ DÀI CÂU: Mỗi câu dài vừa phải từ 12 đến 20 từ để máy đọc âm thanh có nhịp thở tự nhiên, không bị hụt hơi.'
)


class LivestreamFunnelManager:
    """
    Quản lý phễu kịch bản bán hàng kể chuyện (Storytelling Narrative Funnel)
    và xoay vòng 6 góc nhìn chuyển đổi TMĐT vạn năng (Universal E-Commerce Angles).
    Áp dụng hoàn hảo cho MỌI ngành hàng: gia dụng, tiêu dùng, thời trang, mỹ phẩm, ẩm thực, công nghệ...
    Mỗi chu trình (Cycle) là một bài thuyết trình hoàn chỉnh từ Hook -> Nỗi đau -> Tính năng -> Lợi ích -> CTA.
    Mỗi lần lặp lại, hệ thống tự động đổi sang một góc nhìn mới mẻ để giữ chân người xem.
    """
    PHASES = [
        {
            "id": "GREET_HOOK",
            "step": 1,
            "name": "Bước 1: Chào & thu hút",
            "instruction": "Chào người xem và gợi sự chú ý bằng 1 câu hỏi hoặc câu nói mở đầu đánh trúng tâm lý."
        },
        {
            "id": "PROBLEM",
            "step": 2,
            "name": "Bước 2: Nêu vấn đề",
            "instruction": "Khách hàng đang gặp vấn đề gì và vì sao vấn đề này rất đáng quan tâm."
        },
        {
            "id": "PRODUCT_INTRO",
            "step": 3,
            "name": "Bước 3: Giới thiệu sản phẩm",
            "instruction": "Sản phẩm là gì và giải quyết vấn đề gì cho khách hàng."
        },
        {
            "id": "HIGHLIGHTS",
            "step": 4,
            "name": "Bước 4: Điểm nổi bật",
            "instruction": "2 đến 3 ưu điểm chính và điểm khác biệt so với sản phẩm thông thường."
        },
        {
            "id": "DEMO_USAGE",
            "step": 5,
            "name": "Bước 5: Demo / cách sử dụng",
            "instruction": "Sử dụng như thế nào và kết quả mang lại thực tế."
        },
        {
            "id": "PRICE_OFFER",
            "step": 6,
            "name": "Bước 6: Giá & ưu đãi",
            "instruction": "Giá bán ưu đãi và khuyến mãi / quà tặng nếu có."
        },
        {
            "id": "CTA",
            "step": 7,
            "name": "Bước 7: Kêu gọi hành động",
            "instruction": "Comment để được tư vấn, comment từ khóa để đặt hàng hoặc bấm giỏ hàng."
        },
        {
            "id": "ENGAGEMENT",
            "step": 8,
            "name": "Bước 8: Kéo tương tác",
            "instruction": "Hỏi người xem có nhu cầu không, mời đặt câu hỏi và cảm ơn người vừa vào live."
        },
        {
            "id": "NEXT_CYCLE",
            "step": 9,
            "name": "Bước 9: Chuyển sang vòng tiếp theo",
            "instruction": "Nhắc lại ngắn sản phẩm và mở đầu lại bằng một cách diễn đạt khác, không đọc y nguyên kịch bản cũ."
        }
    ]

    ANGLES = [
        {
            "id": "DAILY_PAIN",
            "title": "Vòng 1: Nỗi Đau & Trải Nghiệm Khó Chịu Thường Nhật",
            "focus": "Tiếp cận từ những phiền toái, bực bội và tốn kém thực tế mà người dùng gặp phải hằng ngày khi chưa sở hữu sản phẩm này hoặc khi dùng loại kém chất lượng.",
            "hook_hint": "Mở đầu bằng câu hỏi trúng tim đen hoặc tình huống oái oăm thường gặp liên quan đến sản phẩm.",
            "pain_hint": "Kể chi tiết nỗi phiền toái, sự mệt mỏi, bất tiện hoặc tốn tiền vô ích khi dùng sản phẩm đời cũ hoặc hàng trôi nổi.",
            "solution_hint": "Giới thiệu sản phẩm trong kịch bản như giải pháp giải quyết dứt điểm nỗi đau đó bằng chất liệu, tính năng hoặc công nghệ vượt trội.",
            "value_hint": "Nhấn mạnh giá trị giải phóng sự bực bội, mang lại sự tiện nghi, thoải mái vượt bậc mỗi ngày.",
            "cta_hint": "Kêu gọi chốt đơn ngay để chấm dứt phiền toái."
        },
        {
            "id": "EXTREME_TEST",
            "title": "Vòng 2: Thử Nghiệm Khắc Nghiệt & Chất Lượng Thực Chiến",
            "focus": "Tiếp cận bằng mắt thấy tai nghe: chứng minh chất lượng sản phẩm qua bài kiểm tra độ bền, độ an toàn, độ hiệu quả tức thì hoặc thông số kỹ thuật ấn tượng.",
            "hook_hint": "Mở đầu bằng lời khẳng định ấn tượng về độ bền bỉ hoặc bài test chất lượng thực tế ngay trên phiên live.",
            "pain_hint": "Nêu nỗi sợ mua phải hàng giả, hàng nhái, đồ mỏng manh nhanh hỏng, quảng cáo một đằng giao một nẻo.",
            "solution_hint": "Mô tả chi tiết thử nghiệm thực tế (độ chịu lực, độ dai, khả năng chống nước, thông số kỹ thuật) chứng minh chất lượng đỉnh cao của sản phẩm.",
            "value_hint": "Khẳng định độ bền bỉ dài lâu, mua một lần dùng nhiều năm, chuẩn chính hãng an tâm tuyệt đối.",
            "cta_hint": "Kêu gọi đặt hàng ngay trên phiên live để kiểm chứng chất lượng và nhận ưu đãi."
        },
        {
            "id": "CONTRAST_COMPARE",
            "title": "Vòng 3: So Sánh Đối Đầu - Hàng Trôi Nổi vs Hàng Chuẩn Xịn",
            "focus": "Đặt lên bàn cân so sánh trực diện: hàng trôi nổi giá rẻ nhưng nhanh hỏng, nguy hại, tốn tiền gấp đôi vs sản phẩm chính hãng hôm nay dùng sướng, an toàn, tiết kiệm dài hạn.",
            "hook_hint": "Đặt ra câu hỏi hoặc phép so sánh giật mình giữa đồ rẻ tiền trôi nổi và hàng chuẩn chất lượng.",
            "pain_hint": "Chỉ rõ cái bẫy ham rẻ của người mua: nhanh hỏng, độc hại, dùng bực mình, cuối cùng tốn tiền nhiều hơn.",
            "solution_hint": "Phân tích sự khác biệt vượt trội của sản phẩm chính hãng: từng đường nét, chất liệu cao cấp, độ hoàn thiện tinh xảo.",
            "value_hint": "Khẳng định giá trị thực sự: đầu tư một lần xứng đáng từng đồng, tiết kiệm chi phí thay mới liên tục.",
            "cta_hint": "Kêu gọi chọn hàng chuẩn ngay hôm nay, đừng để mất tiền oan cho hàng trôi nổi."
        },
        {
            "id": "SMART_MATH_DEAL",
            "title": "Vòng 4: Bài Toán Kinh Tế & Deal Hời Độc Quyền",
            "focus": "Tiếp cận từ con số: chia nhỏ chi phí mỗi ngày (chỉ bằng một cốc trà đá) kết hợp combo giảm sâu, quà tặng kèm và chính sách freeship độc quyền trong phiên live.",
            "hook_hint": "Công bố mức giá gây sốc hoặc chương trình ưu đãi độc quyền chỉ có trong phiên live hôm nay.",
            "pain_hint": "Nêu sự tiếc nuối nếu bỏ lỡ cơ hội săn deal hôm nay vì hết live sẽ quay về giá gốc đắt đỏ.",
            "solution_hint": "Nhắc lại sản phẩm và quyền lợi khi mua combo: tặng quà xịn, miễn phí vận chuyển tận nhà.",
            "value_hint": "Làm phép tính kinh tế: chia nhỏ chi phí dùng cả tháng hoặc cả năm chỉ tốn vài nghìn đồng mỗi ngày, siêu tiết kiệm.",
            "cta_hint": "Hối thúc hành động khẩn cấp: số lượng deal có hạn, bấm giỏ hàng hoặc nhắn tin giữ voucher ngay."
        },
        {
            "id": "SOCIAL_PROOF",
            "title": "Vòng 5: Kể Chuyện Khách Hàng & Trải Nghiệm Thực Tế",
            "focus": "Tiếp cận từ câu chuyện người thật việc thật: khách hàng đã mua phản hồi ra sao, MC đã trải nghiệm cảm nhận thế nào, lý do sản phẩm được hàng nghìn người tin dùng.",
            "hook_hint": "Mở đầu bằng lời chia sẻ bất ngờ từ một khách hàng đã mua hoặc con số bán chạy ấn tượng.",
            "pain_hint": "Kể lại tâm lý nghi ngại ban đầu của khách (sợ không hợp, sợ không tốt như quảng cáo).",
            "solution_hint": "Kể khoảnh khắc khách nhận hàng và sử dụng: hoàn toàn vượt ngoài mong đợi, khen nức nở và giới thiệu người quen.",
            "value_hint": "Khẳng định uy tín và sự hài lòng một trăm phần trăm của cộng đồng người tiêu dùng thông thái.",
            "cta_hint": "Kêu gọi tham gia cùng hàng nghìn khách hàng thông thái: đặt đơn ngay hôm nay."
        },
        {
            "id": "RISK_FREE_GUARANTEE",
            "title": "Vòng 6: Phá Bỏ Hoài Nghi & Cam Kết Rủi Ro Bằng Không",
            "focus": "Giải tỏa triệt để mọi nỗi sợ cuối cùng của khách hàng: chính sách cho kiểm tra hàng trước khi thanh toán, bảo hành chính hãng, bao đổi trả nếu không ưng ý.",
            "hook_hint": "Mở đầu bằng câu hỏi giải tỏa nỗi sợ lớn nhất: 'Nếu nhận hàng không ưng thì sao?'",
            "pain_hint": "Đồng cảm với nỗi sợ bị lừa khi mua hàng qua mạng, sợ nhận hàng lỗi hỏng không ai giải quyết.",
            "solution_hint": "Khẳng định chính sách vàng: kiểm tra hàng tận tay, lỗi một đổi một, bảo hành rõ ràng từ nhà bán.",
            "value_hint": "Mang lại sự an tâm tuyệt đối: trải nghiệm mua sắm không chút rủi ro, quyền lợi khách hàng đặt lên hàng đầu.",
            "cta_hint": "Kêu gọi yên tâm đặt hàng ngay: chốt đơn ngay trên phiên live để được bảo hộ chính sách tốt nhất."
        }
    ]

    def __init__(self):
        self.phase_idx = 0
        self.angle_idx = 0
        self.cycle_count = 1

    def get_current_cycle(self) -> dict:
        return self.ANGLES[self.angle_idx % len(self.ANGLES)]

    def next_cycle(self) -> dict:
        self.angle_idx = (self.angle_idx + 1) % len(self.ANGLES)
        self.cycle_count += 1
        angle = self.ANGLES[self.angle_idx]
        logger.info(f"[FunnelManager] Chuyển sang chu trình phễu {self.cycle_count}: {angle['title']}")
        return angle

    def next_phase(self) -> tuple[dict, dict]:
        """Duy trì tương thích ngược nếu code cũ gọi next_phase."""
        phase = self.PHASES[self.phase_idx % len(self.PHASES)]
        angle = self.ANGLES[self.angle_idx % len(self.ANGLES)]
        self.phase_idx += 1
        if self.phase_idx >= len(self.PHASES):
            self.phase_idx = 0
            self.next_cycle()
        return phase, angle


def _build_system_prompt(extra_prompt: str = '', recent_history: list = None, cycle_info: dict = None, phase_info: dict = None, angle_info: dict = None) -> str:
    parts = [BASE_LIVESTREAM_PROMPT]

    angle = cycle_info or angle_info or LivestreamFunnelManager.ANGLES[0]

    # Phân tích phương thức CTA phù hợp dựa trên kịch bản người dùng
    cta_direction = "bấm vào giỏ hàng góc trái màn hình để săn deal và áp mã giảm giá"
    if extra_prompt:
        lower_p = extra_prompt.lower()
        if any(kw in lower_p for kw in ["nhắn tin", "inbox", "in bốc", "ib", "liên hệ", "tư vấn", "nhân vật số", "phần mềm", "giải pháp ai"]):
            cta_direction = "liên hệ in bốc cho em để nhận tư vấn chi tiết và xem thực tế"
        elif any(kw in lower_p for kw in ["giỏ hàng", "đặt hàng", "áp mã", "voucher"]):
            cta_direction = "bấm ngay vào giỏ hàng góc trái màn hình để đặt hàng và áp mã giảm giá"

    if extra_prompt and extra_prompt.strip():
        parts.append(
            f"\n[SẢN PHẨM & KỊCH BẢN GỐC TỪ NGƯỜI DÙNG]:\n{extra_prompt.strip()}\n\n"
            f"NHIỆM VỤ CỦA BẠN: Xây dựng bài THUYẾT TRÌNH BÁN HÀNG LIVE 2-3 PHÚT hoàn chỉnh theo đúng KHUNG 9 BƯỚC CHUẨN, "
            f"tập trung 100% vào SẢN PHẨM VÀ THÔNG TIN Ở TRÊN (tuyệt đối không nói sang sản phẩm khác), dưới góc nhìn tiếp cận sau:"
        )

    parts.append(
        f"\n[GÓC NHÌN TIẾP CẬN CỦA VÒNG NÀY / CURRENT PERSPECTIVE]:\n"
        f"🎯 {angle['title']}\n"
        f"💡 Định hướng câu chuyện: {angle['focus']}\n"
    )

    parts.append(
        f"[CẤU TRÚC BÀI THUYẾT TRÌNH LIVE 2-3 PHÚT (KHUNG 9 BƯỚC LIỀN MẠCH)]:\n"
        f"1. BƯỚC 1 (CHÀO & THU HÚT): Chào người xem, đặt 1 câu hỏi hoặc câu nói mở đầu gợi sự chú ý. ({angle.get('hook_hint', '')})\n"
        f"2. BƯỚC 2 (NÊU VẤN ĐỀ): Khách hàng đang gặp vấn đề gì? Vì sao vấn đề này rất đáng quan tâm? ({angle.get('pain_hint', '')})\n"
        f"3. BƯỚC 3 (GIỚI THIỆU SẢN PHẨM): Sản phẩm là gì? Giải quyết vấn đề gì? ({angle.get('solution_hint', '')})\n"
        f"4. BƯỚC 4 (ĐIỂM NỔI BẬT): Nêu 2-3 ưu điểm chính và điểm khác biệt so với sản phẩm thông thường.\n"
        f"5. BƯỚC 5 (DEMO / CÁCH SỬ DỤNG): Sử dụng như thế nào? Kết quả thực tế mang lại.\n"
        f"6. BƯỚC 6 (GIÁ & ƯU ĐÃI): Giá bán ưu đãi, khuyến mãi/quà tặng nếu có. ({angle.get('value_hint', '')})\n"
        f"7. BƯỚC 7 (KÊU GỌI HÀNH ĐỘNG): Comment để được tư vấn, comment từ khóa để đặt hàng hoặc {cta_direction}. ({angle.get('cta_hint', '')})\n"
        f"8. BƯỚC 8 (KÉO TƯƠNG TÁC): Hỏi người xem có nhu cầu không, mời đặt câu hỏi, cảm ơn người vừa vào LIVE.\n"
        f"9. BƯỚC 9 (CHUYỂN SANG VÒNG TIẾP THEO): Nhắc lại ngắn sản phẩm, mở đầu lại bằng một cách diễn đạt khác, xoay vòng không đọc y nguyên kịch bản cũ.\n"
        f"\n[QUY TẮC BẮT BUỘC VỀ NỘI DUNG & TỪ NGỮ]:\n"
        f"- BÁN ĐÚNG MẶT HÀNG: Chỉ nói về sản phẩm trong kịch bản. CẤM mang tính năng của sản phẩm khác gán vào.\n"
        f"- CẤM lạm dụng từ 'cả nhà'. Tuyệt đối không chêm 'nha cả nhà', 'nhé cả nhà' ở cuối mỗi câu. Hãy xưng hô phong phú ('anh chị', 'các bạn', 'mọi người', 'quý vị') hoặc nói thẳng vào thông điệp.\n"
        f"- CẤM dùng các từ đệm cụt như 'đó', 'luôn đó', 'nè đó'.\n"
        f"- Toàn bộ con số phải viết bằng chữ tiếng Việt (không dùng 0-9, không dùng %, không dùng 24/7). Khi nói tỷ lệ/phần trăm, BẮT BUỘC phải kèm chữ 'phần trăm'.\n"
        f"- Từng câu phải nối tiếp câu trước tự nhiên như một MC livestream thực thụ, mượt mà từ đầu đến cuối."
    )

    if recent_history:
        last_sentences = [s.strip() for s in recent_history if s.strip()][-3:]
        if last_sentences:
            history_str = "\n".join(f"- {s}" for s in last_sentences)
            parts.append(
                f"\n[CÁC CÂU VỪA NÓI TRƯỚC ĐÓ]:\n{history_str}\n"
                f"LƯU Ý: Không lặp lại nguyên văn các câu trên. Câu đầu tiên của lượt này phải chuyển tiếp mượt mà từ nội dung trước."
            )

    return "\n".join(parts)


def _is_valid_sentence(text: str) -> bool:
    if not text or not isinstance(text, str):
        return False
    cleaned = text.strip()
    if not cleaned or len(cleaned.strip('.!? ,;:-\n\r\t')) == 0:
        return False
    # Must contain at least one alphanumeric character
    if not re.search(r'[\w\u00C0-\u1EF9]', cleaned):
        return False
    return True


def _filter_valid_sentences(items):
    return [str(item).strip() for item in items if _is_valid_sentence(str(item))]


def _decode_llm_line(raw_line: str):
    """Accept plain-text, SSE and common JSON streaming response formats."""
    line = str(raw_line or "").strip()
    if not line:
        return []
    if line.startswith("data:"):
        line = line[5:].strip()
    if not line or line == "[DONE]":
        return []
    if not line.startswith(("{", "[")):
        return _filter_valid_sentences([line])
    try:
        payload = json.loads(line)
    except json.JSONDecodeError:
        return _filter_valid_sentences([line])
    if isinstance(payload, list):
        return _filter_valid_sentences(payload)
    if not isinstance(payload, dict):
        return _filter_valid_sentences([payload]) if str(payload).strip() else []
    if payload.get("error"):
        error = payload["error"]
        if isinstance(error, dict):
            error = error.get("message") or error.get("detail") or error
        raise RuntimeError(str(error))
    sentences = payload.get("sentences")
    if isinstance(sentences, list):
        return _filter_valid_sentences(sentences)
    for key in ("sentence", "text", "response", "content"):
        value = payload.get(key)
        if isinstance(value, str) and value.strip():
            return _filter_valid_sentences([value])
    choices = payload.get("choices")
    if isinstance(choices, list) and choices:
        choice = choices[0] if isinstance(choices[0], dict) else {}
        message = choice.get("message") if isinstance(choice.get("message"), dict) else {}
        delta = choice.get("delta") if isinstance(choice.get("delta"), dict) else {}
        value = message.get("content") or delta.get("content") or choice.get("text")
        if isinstance(value, str) and value.strip():
            return _filter_valid_sentences([value])
    return []


def _visible_llm_error(exc: Exception) -> str:
    response = getattr(exc, "response", None)
    status = getattr(response, "status_code", None)
    if status == 404:
        return "Sai địa chỉ LLM: máy chủ không có endpoint /generate."
    if status:
        return f"Máy chủ LLM trả về lỗi HTTP {status}."
    name = type(exc).__name__.lower()
    if "timeout" in name:
        return "Máy chủ LLM phản hồi quá chậm hoặc đã hết thời gian chờ."
    detail = re.sub(r"\s+", " ", str(exc)).strip()
    if len(detail) > 240:
        detail = detail[:237] + "..."
    return f"Không kết nối được LLM: {detail or type(exc).__name__}"


def _call_llm_api(payload, quit_event=None):
    """
    Calls the LLM API at http://localhost:8080/generate and yields sentences line by line.
    Falls back to urllib if requests is not available.
    """
    runtime_config = {}
    try:
        from runtime_config import load_config, refresh_remote_runtime_config, apply_runtime_environment
        runtime_config = load_config()
        refresh_remote_runtime_config(runtime_config, max_age_seconds=3)
        apply_runtime_environment(runtime_config)
    except Exception:
        pass
    base_url = runtime_config.get("runtime_llm_base_url") or os.getenv("LLM_API_BASE_URL") or os.getenv("OLLAMA_BASE_URL") or OLLAMA_BASE_URL
    if not base_url:
        raise RuntimeError("Chưa có link LLM trên trang Cấu hình hệ thống.")
    api_url = os.getenv("LLM_API_URL") or (
        base_url.rstrip("/") if base_url.rstrip("/").endswith("/generate")
        else base_url.rstrip("/") + "/generate"
    )
    
    # Try using requests first
    try:
        import requests
        with requests.post(api_url, json=payload, stream=True, timeout=30.0) as r:
            r.raise_for_status()
            for line in r.iter_lines():
                if quit_event and quit_event.is_set():
                    break
                if line:
                    decoded = line.decode("utf-8").strip()
                    for sentence in _decode_llm_line(decoded):
                        yield sentence
    except ImportError:
        # Fallback to urllib.request if requests is not installed
        import urllib.request
        import json
        req = urllib.request.Request(
            api_url,
            data=json.dumps(payload).encode('utf-8'),
            headers={'Content-Type': 'application/json'},
            method='POST'
        )
        try:
            with urllib.request.urlopen(req, timeout=30.0) as response:
                while True:
                    if quit_event and quit_event.is_set():
                        break
                    line = response.readline()
                    if not line:
                        break
                    decoded = line.decode("utf-8").strip()
                    for sentence in _decode_llm_line(decoded):
                        yield sentence
        except Exception as e:
            logger.error(f"Error calling LLM API via urllib: {e}")
            raise
    except Exception as e:
        logger.error(f"Error calling LLM API: {e}")
        raise

def refine_prompt_with_chatgpt(prompt: str) -> str:
    """
    Gửi prompt gốc của người dùng qua ChatGPT (OpenAI API) để tối ưu và làm cho prompt
    chỉnh chu, chi tiết và chuyên nghiệp hơn trước khi đưa vào LLM sinh kịch bản livestream.
    """
    if not prompt or not prompt.strip():
        return prompt

    runtime_config = {}
    try:
        from runtime_config import load_config, refresh_remote_runtime_config, apply_runtime_environment
        runtime_config = load_config()
        refresh_remote_runtime_config(runtime_config, max_age_seconds=3)
        apply_runtime_environment(runtime_config)
    except Exception:
        pass

    llm_api_key = (
        os.getenv('OPENAI_API_KEY') or 
        runtime_config.get("openai_api_key") or 
        os.getenv('LIVE_LLM_API_KEY') or 
        os.getenv('LLM_API_KEY') or 
        runtime_config.get("llm_api_key") or ""
    )
    llm_url = (
        runtime_config.get("openai_llm_base_url") or 
        os.getenv('OPENAI_LLM_API_BASE_URL') or 
        os.getenv('LLM_API_BASE_URL') or 
        'https://api.openai.com/v1/'
    )
    llm_model = (
        os.getenv('OPENAI_LLM_MODEL') or 
        runtime_config.get("openai_llm_model") or 
        os.getenv('LLM_MODEL') or 
        "gpt-4o-mini"
    )

    if llm_api_key.startswith(('AQ.', 'AIzaSy')):
        if not llm_url or "api.openai.com" in llm_url:
            llm_url = "https://generativelanguage.googleapis.com/v1beta/openai/"
        llm_model = llm_model or "gemini-1.5-flash"
    elif llm_api_key.startswith('sk-'):
        openai_url = runtime_config.get("openai_llm_base_url") or os.getenv('OPENAI_LLM_API_BASE_URL')
        llm_url = openai_url if (openai_url and 'trycloudflare.com' not in openai_url and 'localhost' not in openai_url) else "https://api.openai.com/v1/"
        llm_model = os.getenv('OPENAI_LLM_MODEL') or runtime_config.get("openai_llm_model") or "gpt-4o-mini"

    if not llm_api_key:
        logger.warning("Không tìm thấy API Key để tối ưu prompt qua ChatGPT, giữ nguyên prompt gốc.")
        return prompt

    try:
        from openai import OpenAI
        client = OpenAI(api_key=llm_api_key, base_url=llm_url, timeout=15.0)
        
        system_instruction = (
            "Bạn là chuyên gia biên kịch và đạo diễn livestream bán hàng thương mại điện tử hàng đầu. "
            "Nhiệm vụ của bạn: Nhận prompt hoặc chủ đề sản phẩm ngắn từ người dùng (bất kể là hàng tiêu dùng, gia dụng, thời trang, mỹ phẩm, ẩm thực hay dịch vụ công nghệ) và xây dựng lại thành một kịch bản livestream hoàn chỉnh, phong phú nhiều khía cạnh, TUYỆT ĐỐI KHÔNG LẶP LẠI NỘI DUNG và CHUYỂN TIẾP CỰC KỲ MƯỢT MÀ. "
            "QUY TẮC BẮT BUỘC VỀ SẢN PHẨM & THƯƠNG HIỆU: "
            "- Giữ nguyên 100% sản phẩm, ngành hàng, công dụng, chất liệu, tính năng, giá bán, ưu đãi và quà tặng trong prompt gốc. Tuyệt đối không tự ý đổi sang sản phẩm khác hoặc mang tính năng của sản phẩm khác gán vào. "
            "- Nếu người dùng có cung cấp tên của họ, tên MC/Host, tên Shop hoặc tên Thương hiệu (ví dụ: 'tên tôi là ...', 'em tên là ...', 'mình là [Tên]', 'Shop ...', 'MC ...'), bạn BẮT BUỘC phải giữ nguyên chính xác 100% tên đó trong toàn bộ kịch bản, và thiết lập kịch bản xưng hô đúng bằng tên đó (ví dụ: xưng 'em [Tên]', '[Tên]'). "
            "QUY TẮC BẮT BUỘC VỀ TỪ NGỮ VÀ PHONG CÁCH: "
            "- TUYỆT ĐỐI KHÔNG lạm dụng từ 'cả nhà' hay chêm 'nha cả nhà', 'nhé cả nhà' ở cuối mỗi câu. Hãy xưng hô phong phú ('anh chị', 'các bạn', 'mọi người', 'quý vị') hoặc nói trực diện theo lối thuyết trình, kể chuyện lôi cuốn. "
            "- Tuyệt đối không dùng các từ đệm cụt như 'đó', 'luôn đó', 'nè đó'. "
            "Kịch bản tối ưu BẮT BUỘC tuân thủ KHUNG KỊCH BẢN LIVE 2-3 PHÚT CHUẨN 9 BƯỚC: "
            "1. Chào & thu hút: Chào người xem thân thiện, gợi sự chú ý ngay bằng 1 câu hỏi hoặc câu nói mở đầu đánh trúng tâm lý. "
            "2. Nêu vấn đề: Khách hàng đang gặp vấn đề, khó khăn, phiền toái gì? Vì sao vấn đề này rất đáng quan tâm? "
            "3. Giới thiệu sản phẩm: Sản phẩm là gì? Giải quyết vấn đề đó một cách triệt để ra sao? "
            "4. Điểm nổi bật: 2-3 ưu điểm chính và điểm khác biệt vượt trội so với sản phẩm thông thường trên thị trường. "
            "5. Demo / cách sử dụng: Sử dụng như thế nào? Kết quả mang lại thực tế tuyệt vời ra sao? "
            "6. Giá & ưu đãi: Giá bán ưu đãi, khuyến mãi, combo hoặc quà tặng hấp dẫn trong phiên live. "
            "7. Kêu gọi hành động: Comment để được tư vấn, comment từ khóa đặt hàng hoặc bấm vào giỏ hàng góc trái màn hình. "
            "8. Kéo tương tác: Hỏi người xem có nhu cầu không, mời đặt câu hỏi, cảm ơn những người vừa vào xem live. "
            "9. Chuyển sang vòng tiếp theo: Nhắc lại ngắn gọn sản phẩm, mở đầu lại bằng một cách diễn đạt khác, xoay vòng không đọc y nguyên kịch bản cũ. "
            "Yêu cầu: Viết thành một bài kịch bản livestream hoàn chỉnh 2-3 phút, từ ngữ đời thường, gãy gọn, liền mạch từ đầu đến cuối, dùng cùng ngôn ngữ với prompt gốc (tiếng Việt). "
            "LƯU Ý: Chỉ xuất duy nhất nội dung kịch bản lời thoại MC đọc, không có lời chào hay câu dẫn phụ của AI."
        )

        completion = client.chat.completions.create(
            model=llm_model,
            messages=[
                {"role": "system", "content": system_instruction},
                {"role": "user", "content": f"Hãy tối ưu và mở rộng kịch bản livestream sau đây, giữ nguyên và nhấn mạnh tên/thương hiệu nếu có:\n{prompt.strip()}"}
            ],
            temperature=0.7,
            max_tokens=700
        )
        refined = (completion.choices[0].message.content or "").strip()
        if refined:
            logger.info(f"Đã tối ưu prompt qua ChatGPT thành công:\n[Gốc]: {prompt}\n[Tối ưu]: {refined}")
            return refined
    except Exception as e:
        logger.error(f"Lỗi khi gửi prompt qua ChatGPT để tối ưu: {e}")

    return prompt


def llm_response(message, avatar_session: 'BaseAvatar', datainfo: dict = {}):
    try:
        refined_prompt = refine_prompt_with_chatgpt(message)
        if refined_prompt and refined_prompt != message:
            avatar_session.custom_prompt = refined_prompt
            try:
                prompt_path = os.path.join(os.getcwd(), 'live_prompt.txt')
                with open(prompt_path, 'w', encoding='utf-8') as f:
                    f.write(refined_prompt)
            except Exception as e:
                logger.warning(f"Failed to write live_prompt.txt: {e}")
            avatar_session.push_chat_message('optimized_prompt', refined_prompt)
            message = refined_prompt

        start = time.perf_counter()
        
        funnel = LivestreamFunnelManager()
        angle = funnel.get_current_cycle()
        payload = {
            "prompt": message,
            "system_prompt": _build_system_prompt(message, cycle_info=angle),
            "max_iterations": 1,
            "max_sentences": 10,
            "history_size": 3,
            "session_id": getattr(avatar_session, "avatar_id", None)
        }
        
        first = True
        generated_sentences = []
        for sentence in _call_llm_api(payload):
            sentence = _sanitize_sentence(sentence)
            if not sentence:
                continue

            if _is_duplicate_or_too_similar(sentence, generated_sentences):
                logger.info(f"[Chống lặp đơn] Bỏ qua câu trùng: {sentence}")
                continue

            if first:
                end = time.perf_counter()
                logger.info(f"llm Time to first chunk (API): {end-start:.3f}s")
                first = False
            
            logger.info(f"Câu hoàn chỉnh (API): {sentence}")
            generated_sentences.append(sentence)
            avatar_session.push_chat_message('assistant', sentence)
            avatar_session.put_msg_txt(sentence, datainfo)
            
        end = time.perf_counter()
        logger.info(f"llm Time to last chunk (API): {end-start:.3f}s")
        
    except Exception as exc:
        logger.exception('llm exception calling API:')
        avatar_session.push_chat_message('error', _visible_llm_error(exc))
        return

def _flush_complete_sentences(buffer: str, text: str, avatar_session: 'BaseAvatar', datainfo: dict):
    sentence_end_chars = '.!?。！？'
    buffer += text
    start = 0
    emitted = []

    for i, ch in enumerate(buffer):
        if ch in sentence_end_chars:
            sentence = buffer[start:i+1].strip()
            if sentence:
                sentence = _sanitize_sentence(sentence)
                if sentence:
                    emitted.append(sentence)
            start = i+1

    if start > 0:
        buffer = buffer[start:]
    return buffer, emitted


def _sanitize_sentence(text: str) -> str:
    if not text:
        return ""
    try:
        from utils.vietnamese_ecom_normalizer import normalize_ecom_livestream_text
        text = normalize_ecom_livestream_text(text)
    except Exception as exc:
        logger.warning(f"Lỗi sanitize qua normalize_ecom_livestream_text: {exc}")
        text = re.sub(r"[^\w\s\.,!\?:;\'\"\(\)\-–]+", "", text, flags=re.UNICODE)
        text = re.sub(r"\s+", " ", text).strip()
        # Fallback dọn dẹp cơ bản
        text = re.sub(r'\s+(?:luôn\s+|nè\s+)?đó\s*([.!?]?)$', r'\1', text, flags=re.IGNORECASE).strip()
        if text and not text.endswith(('.', '!', '?')):
            text += '.'
    return text


def _is_duplicate_or_too_similar(sentence: str, history: list) -> bool:
    """Kiểm tra xem câu mới có bị trùng lặp hoặc quá giống với các câu đã nói trước đó không."""
    if not sentence or not history:
        return False
    norm_s = re.sub(r'[^\w\s]', '', sentence.lower()).strip()
    if not norm_s:
        return True
    words_s = set(norm_s.split())
    for prev in history[-15:]:
        norm_prev = re.sub(r'[^\w\s]', '', prev.lower()).strip()
        if norm_s == norm_prev:
            return True
        words_prev = set(norm_prev.split())
        if words_s and words_prev:
            intersection = len(words_s & words_prev)
            union = len(words_s | words_prev)
            # Nếu tỷ lệ từ ngữ trùng nhau > 70% thì coi là lặp nội dung
            if union > 0 and (intersection / union) > 0.70:
                return True
    return False


def llm_continuous(prompt: str, avatar_session: 'BaseAvatar', quit_event, datainfo=None):
    """
    Chế độ sinh liên tiếp: LLM sẽ sinh liên tục để duy trì hàng đợi câu nói của avatar.
    Hàng đợi mục tiêu là 10 câu, khi giảm xuống còn từ 7 câu trở xuống sẽ gọi request sinh thêm.
    Tự động điều phối theo Phễu 5 Bước và Xoay Vòng Góc Nhìn (LivestreamFunnelManager).
    """
    try:
        # Bước 1: Tối ưu prompt qua ChatGPT trước khi bắt đầu sinh kịch bản liên tiếp
        # Nếu datainfo yêu cầu skip_refine (ví dụ đã có sẵn kịch bản chi tiết) thì bỏ qua để tiết kiệm 2-3s API latency
        if not (datainfo and datainfo.get('skip_refine')):
            refined_prompt = refine_prompt_with_chatgpt(prompt)
            if refined_prompt and refined_prompt != prompt:
                avatar_session.custom_prompt = refined_prompt
                try:
                    prompt_path = os.path.join(os.getcwd(), 'live_prompt.txt')
                    with open(prompt_path, 'w', encoding='utf-8') as f:
                        f.write(refined_prompt)
                except Exception as e:
                    logger.warning(f"Failed to write live_prompt.txt: {e}")
                avatar_session.push_chat_message('optimized_prompt', refined_prompt)
                prompt = refined_prompt

        logger.info(f"Bắt đầu chế độ liên tiếp gọi API (hàng đợi 10/7) với prompt: {prompt}")

        funnel = LivestreamFunnelManager()
        base_datainfo = dict(datainfo or {})
        iteration = 0
        history = []
        history_size = 8
        max_queue_size = 10
        refill_threshold = 7
        first_call_done = False
        error_reported = False

        while not quit_event.is_set():
            # Lấy số lượng câu hiện tại trong hàng đợi TTS
            qsize = 0
            if hasattr(avatar_session, 'tts') and hasattr(avatar_session.tts, 'msgqueue'):
                qsize = avatar_session.tts.msgqueue.qsize()

            # Gọi request khi lần đầu tiên chạy hoặc khi hàng đợi giảm xuống còn từ 7 câu trở xuống
            if not first_call_done or qsize <= refill_threshold:
                iteration += 1
                
                # Mỗi lần gọi request đều sinh trọn vẹn 10 câu theo bài thuyết trình 5 bước phễu
                num_to_request = max_queue_size
                angle = funnel.get_current_cycle()

                logger.info(
                    f"Lần sinh thứ {iteration} (API) - Hàng đợi hiện tại: {qsize}/{max_queue_size}. "
                    f"[{angle['title']}]. Thuyết trình kể chuyện 10 câu phễu 5 bước."
                )

                payload = {
                    "prompt": prompt,
                    "system_prompt": _build_system_prompt(
                        prompt, 
                        history[-history_size:], 
                        cycle_info=angle
                    ),
                    "history": history[-history_size:],
                    "max_iterations": 1,
                    "max_sentences": num_to_request,
                    "history_size": history_size,
                    "session_id": getattr(avatar_session, "avatar_id", None)
                }

                try:
                    # Gọi API và đẩy các câu nhận được vào hàng đợi theo luồng stream
                    generated_count = 0
                    for sentence in _call_llm_api(payload, quit_event):
                        if quit_event.is_set():
                            break
                        
                        sentence = _sanitize_sentence(sentence)
                        if not sentence:
                            continue

                        # Kiểm tra chống lặp lại nội dung đã nói
                        if _is_duplicate_or_too_similar(sentence, history):
                            logger.info(f"[Chống lặp] Bỏ qua câu trùng hoặc quá giống lịch sử: {sentence}")
                            continue

                        logger.info(f"[Liên tiếp {iteration} - Vòng {funnel.cycle_count}] {sentence}")
                        sentence_datainfo = dict(base_datainfo)
                        active_tts = getattr(avatar_session, 'speech_settings', None)
                        if active_tts:
                            sentence_datainfo['tts'] = dict(active_tts)
                        avatar_session.put_msg_txt(sentence, sentence_datainfo)
                        history.append(sentence)
                        avatar_session.push_chat_message('assistant', sentence)
                        generated_count += 1
                        error_reported = False

                    # Sau khi hoàn tất 1 đợt sinh trọn vẹn theo góc nhìn hiện tại, chuyển sang góc nhìn tiếp theo
                    if generated_count > 0:
                        funnel.next_cycle()

                    # Đánh dấu đã hoàn thành cuộc gọi đầu tiên sau khi stream xong
                    if not first_call_done and not quit_event.is_set():
                        first_call_done = True
                    if generated_count == 0:
                        if not error_reported:
                            avatar_session.push_chat_message(
                                'error',
                                'LLM đã kết nối nhưng không trả về nội dung. Hãy kiểm tra endpoint /generate.'
                            )
                            error_reported = True
                        if quit_event.wait(timeout=3.0):
                            break

                except Exception as e:
                    logger.error(f"Lỗi gọi API sinh lần {iteration}: {str(e)}")
                    if not error_reported:
                        avatar_session.push_chat_message('error', _visible_llm_error(e))
                        error_reported = True
                    # Chờ 2 giây trước khi thử lại nếu gặp lỗi để tránh lặp vô hạn quá nhanh
                    if quit_event.wait(timeout=2.0):
                        break
            else:
                # Nếu hàng đợi vẫn còn đủ câu (> 7 câu), ngủ 0.5 giây rồi kiểm tra lại
                if quit_event.wait(timeout=0.5):
                    break

        logger.info("Chế độ liên tiếp đã kết thúc")

    except Exception as exc:
        logger.exception('llm_continuous exception:')
        avatar_session.push_chat_message('error', _visible_llm_error(exc))
        return
