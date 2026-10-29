# -*- coding: utf-8 -*-
"""
Module Chuẩn Hóa Văn Bản Tiếng Việt Toàn Diện Cho Livestream AI (SLive AI VIP)
Kế thừa và mở rộng từ live_noface với các tính năng:
1. Chuẩn hóa số, số thập phân, phần trăm, giá tiền (149k, 59 cành, 1xx, 2k5, 1.500.000đ).
2. Chuẩn hóa đơn vị đo lường, kích cỡ (500ml, 1.5L, 2kg, size S/M/L/XL).
3. Thuật ngữ livestream & TMĐT (freeship, voucher, combo, deal).
4. TikTok Compliance Guardrail: Tự động loại bỏ từ cấm tránh bị khóa phòng / bóp tương tác.
5. Từ điển phiên âm thuần Việt đặc thù VIP Studio: AI -> ây ai, OBS -> o bét ét, TikTok -> tóp tóp, VIP -> víp, Lip-sync -> khớp khẩu hình...
6. MC Self-Addressing Sanitizer: Triệt tiêu các lỗi đảo lộn ngôi xưng hô (như 'Trúc Linh nhé', 'Ngọc Linh ơi').
7. Tối ưu hóa dấu câu (!) và dấu phẩy ngắt nhịp thở cho VieNeu.
8. Chunker chia đoạn câu 12-20 từ.
"""

import re
import sys
import unicodedata
from typing import List

# Đảm bảo in tiếng Việt trên console Windows không bị lỗi encoding
if hasattr(sys.stdout, 'reconfigure'):
    sys.stdout.reconfigure(encoding='utf-8', errors='replace')
if hasattr(sys.stderr, 'reconfigure'):
    sys.stderr.reconfigure(encoding='utf-8', errors='replace')

# 1. BẢNG TỪ SỐ CƠ BẢN TIẾNG VIỆT
DIGIT_WORDS = ["không", "một", "hai", "ba", "bốn", "năm", "sáu", "bảy", "tám", "chín"]

def _read_2_digits(num_str: str) -> str:
    n = int(num_str)
    chuc = n // 10
    don_vi = n % 10
    res = []
    if chuc == 1:
        res.append("mười")
    else:
        res.append(DIGIT_WORDS[chuc] + " mươi")
    
    if don_vi == 1:
        res.append("mốt" if chuc > 1 else "một")
    elif don_vi == 4:
        res.append("tư" if chuc > 1 else "bốn")
    elif don_vi == 5:
        res.append("lăm")
    elif don_vi > 0:
        res.append(DIGIT_WORDS[don_vi])
    return " ".join(res)

def _read_3_digits(num_str: str) -> str:
    n = int(num_str)
    tram = n // 100
    chuc = (n % 100) // 10
    don_vi = n % 10
    
    res = [DIGIT_WORDS[tram], "trăm"]
    if chuc == 0 and don_vi == 0:
        return " ".join(res)
    if chuc == 0 and don_vi > 0:
        res.append("lẻ")
        res.append(DIGIT_WORDS[don_vi])
        return " ".join(res)
        
    res.append(_read_2_digits(f"{chuc}{don_vi}"))
    return " ".join(res)

def number_to_vietnamese(n: int) -> str:
    """Đọc số nguyên dương thành chữ tiếng Việt chính xác."""
    if n == 0:
        return "không"
    if n < 0:
        return "âm " + number_to_vietnamese(abs(n))
    if n < 10:
        return DIGIT_WORDS[n]
    if n < 100:
        return _read_2_digits(f"{n:02d}")
    if n < 1000:
        return _read_3_digits(f"{n:03d}")
        
    units = ["", "nghìn", "triệu", "tỷ"]
    groups = []
    temp = n
    while temp > 0:
        groups.append(temp % 1000)
        temp //= 1000
        
    parts = []
    for i in range(len(groups) - 1, -1, -1):
        g = groups[i]
        if g > 0:
            if i == len(groups) - 1 and g < 100:
                if g < 10:
                    text = DIGIT_WORDS[g]
                else:
                    text = _read_2_digits(f"{g:02d}")
            else:
                text = _read_3_digits(f"{g:03d}")
            unit = units[i % 4]
            parts.append(text + (" " + unit if unit else ""))
            
    return " ".join(parts).strip()


# 2. TỪ ĐIỂN PHIÊN ÂM THUẦN VIỆT CHO STUDIO VIP & CÔNG NGHỆ LIVESTREAM
TECH_PHONETIC_REPLACEMENTS = [
    # Thuật ngữ AI & Công nghệ Studio
    (r'\b(?:A\.I|AI)\b', 'ây ai'),
    (r'\b(?:OBS\s*Studio|OBS)\b', 'o bét ét'),
    (r'\b(?:TikTok|Tiktok|tik tok|Tik Tok|TopTop|tóp tóp)\b', 'tóp tóp'),
    (r'\b(?:Virtual\s*Camera|Virtual\s*cam)\b', 'máy quay ảo', re.IGNORECASE),
    (r'\b(?:Lip-sync|Lipsync|Lip\s*sync)\b', 'khớp khẩu hình', re.IGNORECASE),
    (r'\b(?:Avatar|Avatars)\b', 'nhân vật số', re.IGNORECASE),
    (r'\b(?:Studio)\b', 'sờ-tu-đi-ô', re.IGNORECASE),
    (r'\b(?:VIP|V\.I\.P)\b', 'víp'),
    (r'\b(?:MC|mc)\b', 'em xi'),
    (r'\b(?:Demo)\b', 'xem mẫu', re.IGNORECASE),
    (r'\b(?:Livestream|Live\s*stream)\b', 'phiên live', re.IGNORECASE),
    (r'\b(?:Video|Clip)\b', 'vi-đê-ô', re.IGNORECASE),
    (r'\b(?:Webcam)\b', 'máy quay máy tính', re.IGNORECASE),
    (r'\b(?:4K|4k)\b', 'bốn ca'),
    (r'\b(?:Full\s*HD|FHD)\b', 'phun hát đê', re.IGNORECASE),
    (r'\b(?:HD)\b', 'hát đê'),
    (r'\b(?:Setup|Set-up)\b', 'cài đặt', re.IGNORECASE),
    (r'\b(?:Update)\b', 'cập nhật', re.IGNORECASE),
    (r'\b(?:Pro)\b', 'chuyên nghiệp', re.IGNORECASE),
    (r'\b(?:Auto)\b', 'tự động', re.IGNORECASE),
    (r'\b(?:Online)\b', 'trực tuyến', re.IGNORECASE),
]

# 3. BỘ LỌC CHỐNG BÓP TƯƠNG TÁC / TUÂN THỦ CHÍNH SÁCH TIKTOK (TIKTOK COMPLIANCE)
TIKTOK_COMPLIANCE_RULES = [
    (r'\b(?:cam kết|chắc chắn)\s*100(?:\s*%|\s*phần\s*trăm)?', 'đảm bảo tiêu chuẩn chất lượng', re.IGNORECASE),
    (r'\b(?:trị dứt điểm|trị tận gốc|khỏi hẳn\s*100%)\b', 'hỗ trợ cải thiện rõ rệt', re.IGNORECASE),
    (r'\b(?:rẻ nhất thị trường|rẻ nhất việt nam|rẻ nhất tiktok|rẻ vô địch)\b', 'mức giá cực kỳ ưu đãi', re.IGNORECASE),
    (r'\b(?:chuyển khoản ngoài|chuyển khoản riêng|ck ngoài)\b', 'đặt đơn trực tiếp trong live', re.IGNORECASE),
    (r'\b(?:inbox zalo|nhắn zalo|qua zalo)\b', 'nhắn tin trực tiếp cho shop', re.IGNORECASE),
    (r'\b(?:để lại sđt|cho xin sđt|cho xin số điện thoại|để lại số điện thoại)\b', 'bấm vào giỏ hàng để đặt đơn', re.IGNORECASE),
]

# 4. BỘ LỌC TRIỆT TIÊU LỖI ĐẢO LỘN NGÔI XƯNG HÔ (MC SELF-ADDRESSING SANITIZER)
# Xử lý triệt để hiện tượng LLM nói "Trúc Linh nhé", "Ngọc Linh ơi", "chào Trúc Linh"
def strip_mc_self_addressing(text: str) -> str:
    if not text:
        return ""
    
    # 1. Câu xưng hô giữa câu trước: "cảm ơn Trúc Linh nha", "chào Trúc Linh nhé"
    text = re.sub(
        r'\b(?:cảm ơn|cám ơn)\s+(?:trúc\s*linh|ngọc\s*linh|linh)\s*(?:nhé|nha|ạ|nè|nhá)?\b',
        'cảm ơn mọi người nhé ',
        text,
        flags=re.IGNORECASE
    )
    text = re.sub(
        r'\b(?:hẹn gặp|chào)\s+(?:trúc\s*linh|ngọc\s*linh|linh)\s*(?:nhé|nha|ạ|nè|nhá)?\b',
        'chào mọi người nhé ',
        text,
        flags=re.IGNORECASE
    )

    # 2. Các đầu câu gọi tên MC: "Trúc Linh ơi", "Ngọc Linh ơi", "Chào Trúc Linh"
    mc_head_pattern = re.compile(
        r'^(?:dạ\s+)?(?:chào\s+)?(?:trúc\s*linh|ngọc\s*linh|linh)\s*(?:ơi|nè|ạ)[,!\s]*',
        flags=re.IGNORECASE
    )
    text = mc_head_pattern.sub('Dạ em xin chào mọi người, ', text)

    # 3. Các đuôi câu tự gọi tên MC còn lại: "Trúc Linh nhé", "Ngọc Linh nha", "Trúc Linh ạ", "Ngọc Linh nè"
    mc_tail_pattern = re.compile(
        r'([,\s]|^)(?:dạ\s+)?(?:trúc\s*linh|ngọc\s*linh|linh)\s*(?:nhé|nha|ạ|nè|nhá|đó)\b[.!?,]*',
        flags=re.IGNORECASE
    )
    text = mc_tail_pattern.sub(r'\1nhé ạ!', text)
    
    # 4. Dọn dẹp câu nếu bắt đầu bằng "nhé cả nhà!" hoặc "nhé ạ!" cộc lốc
    text = re.sub(r'^\s*(?:nhé\s+cả\s+nhà|nhé\s+ạ)[!,\s]*', '', text, flags=re.IGNORECASE)
    
    # 5. Bỏ các từ "đó." hoặc "luôn đó." thừa thãi ở cuối câu thành đuôi câu gãy gọn
    text = re.sub(r'(\s+)(?:luôn\s+)?đó(?:\s*[.!?]+|\s*$)', r'\1nhé ạ.', text, flags=re.IGNORECASE)
    
    return text.strip()


def deduplicate_overused_particles(text: str) -> str:
    """
    Loại bỏ triệt để việc lặp từ 'cả nhà', 'nha cả nhà', 'nhé cả nhà', 'nè cả nhà'.
    Chuyển đổi thành câu trần thuật gãy gọn hoặc đại từ tự nhiên, phong phú (anh chị, mọi người).
    """
    if not text:
        return ""

    # Bỏ 'nha cả nhà', 'nhé cả nhà', 'nè cả nhà' ở cuối câu hoặc trước dấu câu
    text = re.sub(r'[, ]+\b(?:nha|nhé|nè)\s+cả\s+nhà\b(?:\s*([.!?]))?', r'\1', text, flags=re.IGNORECASE)
    
    # Bỏ 'nha cả nhà', 'nhé cả nhà' đơn lẻ còn sót lại
    text = re.sub(r'\b(?:nha|nhé|nè)\s+cả\s+nhà\b', 'nhé ạ', text, flags=re.IGNORECASE)
    
    # Bỏ 'cả nhà' đứng sau thán từ
    text = re.sub(r'\b(?:dạ\s+)?cả\s+nhà\s+mình\s+ơi[, ]*', 'Dạ mọi người ơi, ', text, flags=re.IGNORECASE)

    # Nếu câu có nhiều hơn 1 lần từ 'cả nhà', thay từ thứ hai trở đi bằng 'mọi người' hoặc 'anh chị'
    parts = re.split(r'(\bcả\s+nhà\b)', text, flags=re.IGNORECASE)
    if len(parts) > 3:
        new_parts = []
        ca_nha_count = 0
        for p in parts:
            if re.match(r'^\bcả\s+nhà\b$', p, flags=re.IGNORECASE):
                ca_nha_count += 1
                if ca_nha_count > 1:
                    new_parts.append("mọi người" if ca_nha_count % 2 == 0 else "anh chị")
                else:
                    new_parts.append(p)
            else:
                new_parts.append(p)
        text = "".join(new_parts)

    return text.strip()


def restore_missing_percentage_words(text: str) -> str:
    """
    Tự động phục hồi chữ 'phần trăm' khi LLM nói thiếu hoặc nuốt mất từ phần trăm,
    đặc biệt là các lỗi hay gặp: 'tám mươi chi phí', 'độ chính xác đến chín mươi chín',
    'tiết kiệm tới tám mươi đấy ạ', 'tối ưu chi phí lên đến tám mươi so với'.
    """
    if not text:
        return ""

    num_pattern = r'(?:mười|hai mươi|ba mươi|bốn mươi|năm mươi|sáu mươi|bảy mươi|tám mươi|chín mươi(?:\s+chín|\s+tám)?|một trăm)(?:\s+phẩy\s+\w+)?'

    # 1. Các trường hợp đứng trước 'chi phí', 'ngân sách' (vd: tám mươi chi phí -> tám mươi phần trăm chi phí)
    text = re.sub(
        rf'\b({num_pattern})\s+(chi\s+phí|ngân\s+sách)\b',
        r'\1 phần trăm \2',
        text,
        flags=re.IGNORECASE
    )

    # 2. Các trường hợp độ chính xác, khớp khẩu hình (vd: chính xác đến chín mươi chín -> chín mươi chín phần trăm)
    accuracy_pattern = (
        r'(\b(?:chuẩn\s*xác|chính\s*xác|độ\s*chính\s*xác|khớp\s*chuẩn\s*xác|tỷ\s*lệ\s*khớp|tỉ\s*lệ\s*khớp|khẩu\s*hình\s*khớp|khẩu\s*hình|nhận\s*diện)'
        r'(?:\s+(?:lên\s*tới|lên\s*đến|tới|đến|đạt|khoảng))?\s+)'
        rf'({num_pattern})(?!\s*phần\s*trăm)'
    )
    text = re.sub(accuracy_pattern, r'\1\2 phần trăm', text, flags=re.IGNORECASE)

    # 3. Các trường hợp giống ... đến chín mươi chín (vd: giống người thật đến chín mươi chín đấy)
    resemble_pattern = (
        r'(\bgiống\s+[^,.!?]{1,30}?\s+đến\s+)'
        rf'({num_pattern})(?!\s*phần\s*trăm)'
    )
    text = re.sub(resemble_pattern, r'\1\2 phần trăm', text, flags=re.IGNORECASE)

    # 4. Các trường hợp tiết kiệm, giảm, tối ưu, bằng ... tám mươi (so với / chi phí / đấy / nhé / cuối câu)
    save_pattern = (
        r'(\b(?:tiết\s*kiệm|giảm(?:\s*thiểu)?(?:\s*chi\s*phí)?|tối\s*ưu(?:\s*hóa)?(?:\s*chi\s*phí)?|chỉ\s*bằng|bằng)'
        r'(?:\s+(?:lên\s*tới|lên\s*đến|tới|đến|khoảng|khoảng\s*chừng|đạt))?\s+)'
        rf'({num_pattern})(?!\s*phần\s*trăm)'
        r'(?!\s*(?:nghìn|triệu|tỷ|đồng|cành|cá|củ|gói|hộp|chai|sản\s*phẩm|lần))'
        r'(?=\s*(?:so\s*với|chi\s*phí|đấy|nha|nhé|ạ|[.!,?]|$))'
    )
    text = re.sub(save_pattern, r'\1\2 phần trăm', text, flags=re.IGNORECASE)

    # 5. Hài lòng một trăm -> hài lòng một trăm phần trăm
    text = re.sub(
        r'(\b(?:hài\s*lòng|tin\s*tưởng|yên\s*tâm)\s+(?:đến\s+)?)(một\s*trăm)(?!\s*phần\s*trăm)',
        r'\1\2 phần trăm',
        text,
        flags=re.IGNORECASE
    )

    return text


# 5. QUY TẮC CHUẨN HÓA ĐƠN VỊ & KÍCH CỠ
UNIT_REPLACEMENTS = [
    (r'\b(\d+)\s*(?:ml|ML)\b', r'\1 mi-li-lít'),
    (r'\b(\d+)[.,](\d+)\s*(?:l|L|lít|lit)\b', r'\1 phẩy \2 lít'),
    (r'\b(\d+)\s*(?:l|L|lít|lit)\b', r'\1 lít'),
    (r'\b(\d+)[.,](\d+)\s*(?:kg|Kg|KG|kí|ki)\b', r'\1 phẩy \2 ki-lô-gam'),
    (r'\b(\d+)\s*(?:kg|Kg|KG|kí|ki)\b', r'\1 ki-lô-gam'),
    (r'\b(\d+)\s*(?:g|gr|gram|gam)\b', r'\1 gam'),
    (r'\b(\d+)\s*(?:cm|CM)\b', r'\1 xăng-ti-mét'),
    (r'\b(\d+)\s*(?:mm|MM)\b', r'\1 mi-li-mét'),
    (r'\b(\d+)[.,](\d+)\s*%', r'\1 phẩy \2 phần trăm'),
    (r'\b(\d+)\s*%', r'\1 phần trăm'),
    (r'%', ' phần trăm'),
    (r'\bsuốt\s*24[/.-]?7\b', 'suốt ngày đêm'),
    (r'\bsuốt\s*247\b', 'suốt ngày đêm'),
    (r'\b24/7\b', 'suốt ngày đêm'),
    (r'\b247\b', 'suốt ngày đêm'),
    (r'\b(\d+)\s*(?:sao|star)\b', r'\1 sao'),
    (r'\b(\d+)\s*(?:s|giây|sec)\b', r'\1 giây'),
    (r'\b(\d+)\s*(?:p|phút|min)\b', r'\1 phút'),
]

SIZE_REPLACEMENTS = [
    (r'\b(?:size|Size|SIZE)\s*XXL\b', 'size hai ích lờ'),
    (r'\b(?:size|Size|SIZE)\s*2XL\b', 'size hai ích lờ'),
    (r'\b(?:size|Size|SIZE)\s*3XL\b', 'size ba ích lờ'),
    (r'\b(?:size|Size|SIZE)\s*XL\b', 'size ích lờ'),
    (r'\b(?:size|Size|SIZE)\s*L\b', 'size lờ'),
    (r'\b(?:size|Size|SIZE)\s*M\b', 'size em'),
    (r'\b(?:size|Size|SIZE)\s*S\b', 'size ét'),
    (r'\b(?:size|Size|SIZE)\s*XS\b', 'size ích ét'),
]

ECOM_TERMS_REPLACEMENTS = [
    (r'\b(?:freeship|free ship|free-ship)\b', 'miễn phí giao hàng', re.IGNORECASE),
    (r'\b(?:voucher|vouchers)\b', 'mã giảm giá', re.IGNORECASE),
    (r'\b(?:flash\s*sale|flashsale)\b', 'ưu đãi chớp nhoáng', re.IGNORECASE),
    (r'\b(?:inbox|ib)\b', 'nhắn tin cho em', re.IGNORECASE),
    (r'\b(?:feedback|feedbacks)\b', 'đánh giá của khách', re.IGNORECASE),
    (r'\bcombo\s*(\d+)\b', r'com-bô \1', re.IGNORECASE),
    (r'\bset\s*(\d+)\b', r'sét \1', re.IGNORECASE),
    (r'\bdeal\b', 'ưu đãi', re.IGNORECASE),
    (r'\bhot\s*deal\b', 'ưu đãi cực sốc', re.IGNORECASE),
    (r'\badd to cart\b', 'thêm vào giỏ hàng', re.IGNORECASE),
]


def normalize_price_expressions(text: str) -> str:
    """Chuẩn hóa mọi biểu thức giá tiền và số trong livestream."""
    
    # 1. Dạng xk, xK (ví dụ: 149k, 59K, 29k)
    def _sub_k(match):
        val = int(match.group(1))
        return f"{number_to_vietnamese(val)} nghìn đồng"
        
    text = re.sub(r'\b(\d{1,4})\s*[kK]\b', _sub_k, text)
    
    # 2. Dạng tiếng lóng: cành, cá, củ (ví dụ: 149 cành, 59 cá, 2 củ)
    def _sub_slang(match):
        val = int(match.group(1))
        unit = match.group(2).lower()
        read_text = number_to_vietnamese(val)
        if unit in ('cành', 'cá'):
            return f"{read_text} cành"
        elif unit == 'củ':
            return f"{read_text} triệu"
        return match.group(0)

    text = re.sub(r'\b(\d{1,4})\s*(cành|cá|củ)\b', _sub_slang, text, flags=re.IGNORECASE)

    # 3. Dạng số lẻ: 1k2, 2k5
    def _sub_k_fraction(match):
        n1 = int(match.group(1))
        n2 = int(match.group(2))
        return f"{number_to_vietnamese(n1)} nghìn {number_to_vietnamese(n2)} trăm"
        
    text = re.sub(r'\b(\d+)[kK](\d)\b', _sub_k_fraction, text)

    # 4. Dạng x.xxx đ, x.xxx.xxx đ, x.xxxđ
    def _sub_vnd(match):
        raw = match.group(1).replace('.', '').replace(',', '')
        try:
            val = int(raw)
            return f"{number_to_vietnamese(val)} đồng"
        except ValueError:
            return match.group(0)
            
    text = re.sub(r'\b(\d{1,3}(?:[.,]\d{3})+)\s*(?:đ|₫|vnđ|vnd|đồng)?\b', _sub_vnd, text, flags=re.IGNORECASE)
    
    # 5. Dạng số viết liền hàng chục nghìn (59000 -> năm mươi chín nghìn)
    def _sub_pure_num_thousands(match):
        num_str = match.group(1)
        if len(num_str) in (5, 6) and num_str.endswith("000"):
            val = int(num_str)
            return f"{number_to_vietnamese(val)} đồng"
        return match.group(0)
        
    text = re.sub(r'\b(\d{5,6})\b', _sub_pure_num_thousands, text)

    # 6. Dạng ước lượng: 1xx, 2xx
    text = re.sub(r'\b1xx\b', 'hơn một trăm nghìn', text, flags=re.IGNORECASE)
    text = re.sub(r'\b2xx\b', 'hơn hai trăm nghìn', text, flags=re.IGNORECASE)
    text = re.sub(r'\b3xx\b', 'hơn ba trăm nghìn', text, flags=re.IGNORECASE)
    text = re.sub(r'\b4xx\b', 'hơn bốn trăm nghìn', text, flags=re.IGNORECASE)
    text = re.sub(r'\b5xx\b', 'hơn năm trăm nghìn', text, flags=re.IGNORECASE)

    return text


def convert_all_remaining_digits(text: str) -> str:
    """Chuyển đổi triệt để bất kỳ con số nào còn sót lại sang chữ tiếng Việt."""
    # Số thập phân (ví dụ: 98.9 hoặc 99,5)
    def _sub_decimal(match):
        n1 = int(match.group(1))
        n2 = int(match.group(2))
        return f"{number_to_vietnamese(n1)} phẩy {number_to_vietnamese(n2)}"
    text = re.sub(r'\b(\d+)[.,](\d+)\b', _sub_decimal, text)

    # Số nguyên đơn lẻ
    def _sub_int(match):
        val = int(match.group(0))
        return number_to_vietnamese(val)
    text = re.sub(r'\b\d+\b', _sub_int, text)
    return text


def optimize_live_punctuation(text: str) -> str:
    """Tối ưu hóa dấu câu (!) và dấu phẩy ngắt hơi tự nhiên cho VieNeu."""
    cleaned = unicodedata.normalize('NFC', text.strip())
    
    # Chuẩn hóa khoảng trắng quanh dấu phẩy và chấm
    cleaned = re.sub(r'\s*([,!?…])\s*', r'\1 ', cleaned)
    cleaned = re.sub(r'\s+', ' ', cleaned).strip()
    
    # Chèn dấu phẩy nhẹ trước các thán từ cuối câu nếu chưa có
    particles = ['nha mọi người', 'nhé mọi người', 'luôn á', 'luôn nha', 'nhé ạ']
    for p in particles:
        pattern = re.compile(rf'([^,!?…])\s+({re.escape(p)})\b', re.IGNORECASE)
        cleaned = pattern.sub(r'\1, \2', cleaned)

    cleaned = re.sub(r'\bnhé,\s*ạ\b', 'nhé ạ', cleaned, flags=re.IGNORECASE)
    cleaned = re.sub(r'\bnha,\s*ạ\b', 'nha ạ', cleaned, flags=re.IGNORECASE)
        
    # Nhận diện câu kêu gọi hành động / hào hứng để gắn dấu cảm thán !
    excited_patterns = [
        r'\b(?:chốt đơn ngay|bấm vào giỏ hàng|nhanh tay|kẻo hết|quá hời|siêu hời|cực sốc|duy nhất hôm nay|ngay lúc này|nhắn tin ngay)\b',
        r'\b(?:nha cả nhà|nha mọi người|nhé cả nhà|nhé mọi người|ạ|luôn á mọi người)\s*$'
    ]
    
    sentences = re.split(r'([.!?…]+)', cleaned)
    new_sentences = []
    
    for i in range(0, len(sentences) - 1, 2):
        s_text = sentences[i].strip()
        punct = sentences[i+1].strip()
        if not s_text:
            continue
            
        is_excited = any(re.search(pat, s_text, flags=re.IGNORECASE) for pat in excited_patterns)
        if is_excited and punct in ('.', ''):
            punct = '!'
            
        new_sentences.append(f"{s_text}{punct}")
        
    if len(sentences) % 2 == 1 and sentences[-1].strip():
        last = sentences[-1].strip()
        is_excited = any(re.search(pat, last, flags=re.IGNORECASE) for pat in excited_patterns)
        new_sentences.append(f"{last}{'!' if is_excited else '.'}")
        
    result = " ".join(new_sentences).strip()
    return result if result else cleaned


def split_tts_chunks(text: str, min_words: int = 10, max_words: int = 22) -> List[str]:
    """
    Chia văn bản thành các câu thoại có độ dài 10-22 từ (nhịp thở lý tưởng cho TTS).
    Gộp các mẩu câu quá ngắn (<6 từ) để tránh mô hình âm học méo tiếng.
    """
    if not text:
        return []
    
    text = re.sub(r"\s+", " ", text.strip())
    if not text:
        return []
        
    sentences = re.findall(r"[^.!?…]+[.!?…]*|[^.!?…]+$", text)
    chunks: List[str] = []
    current_chunk = []
    current_words = 0
    
    for raw_sentence in sentences:
        sentence = raw_sentence.strip()
        if not sentence:
            continue
        
        words = sentence.split()
        word_count = len(words)
        if word_count == 0:
            continue
            
        if current_chunk and (current_words + word_count > max_words) and (current_words >= min_words):
            chunks.append(" ".join(current_chunk))
            current_chunk = words
            current_words = word_count
        else:
            current_chunk.extend(words)
            current_words += word_count
            
    if current_chunk:
        chunks.append(" ".join(current_chunk))
        
    # Gộp chunk cuối nếu quá ngắn
    if len(chunks) > 1:
        last_chunk_words = len(chunks[-1].split())
        if last_chunk_words < 6:
            last = chunks.pop()
            chunks[-1] = chunks[-1] + " " + last
            
    return chunks


def normalize_ecom_livestream_text(text: str, apply_compliance: bool = True) -> str:
    """
    Hàm tổng thể chuẩn hóa văn bản trước khi đưa vào TTS VieNeu:
    1. Xử lý lỗi đảo lộn ngôi xưng hô MC (strip_mc_self_addressing)
    2. Lọc tuân thủ chính sách TikTok (TikTok compliance guardrail)
    3. Phiên âm thuật ngữ công nghệ studio (AI, OBS, TikTok, VIP, Virtual Cam...)
    4. Chuẩn hóa giá tiền, số, tỷ lệ %
    5. Chuẩn hóa đơn vị đo lường, kích cỡ
    6. Thuật ngữ TMĐT
    7. Chuyển đổi mọi số còn lại sang chữ
    8. Tối ưu hóa dấu than và dấu phẩy ngắt nhịp
    """
    if not text or not str(text).strip():
        return ""
        
    text = unicodedata.normalize('NFC', str(text).strip())
    
    # 1. Triệt tiêu gọi tên MC nhầm vai
    text = strip_mc_self_addressing(text)
    
    # 2. Tuân thủ TikTok
    if apply_compliance:
        for rule in TIKTOK_COMPLIANCE_RULES:
            pat = rule[0]
            rep = rule[1]
            flags = rule[2] if len(rule) > 2 else 0
            text = re.sub(pat, rep, text, flags=flags)
            
    # 3. Phiên âm thuật ngữ công nghệ
    for rule in TECH_PHONETIC_REPLACEMENTS:
        pat = rule[0]
        rep = rule[1]
        flags = rule[2] if len(rule) > 2 else 0
        text = re.sub(pat, rep, text, flags=flags)
        
    # 4. Chuẩn hóa giá tiền
    text = normalize_price_expressions(text)
    
    # 5. Chuẩn hóa đơn vị đo lường
    for pattern, replacement in UNIT_REPLACEMENTS:
        text = re.sub(pattern, replacement, text)
        
    # 6. Chuẩn hóa kích cỡ
    for pattern, replacement in SIZE_REPLACEMENTS:
        text = re.sub(pattern, replacement, text, flags=re.IGNORECASE)
        
    # 7. Thuật ngữ TMĐT
    for rule in ECOM_TERMS_REPLACEMENTS:
        pat = rule[0]
        rep = rule[1]
        flags = rule[2] if len(rule) > 2 else 0
        text = re.sub(pat, rep, text, flags=flags)
        
    # 8. Chuyển đổi toàn bộ số còn lại sang chữ
    text = convert_all_remaining_digits(text)
    
    # 8b. Tự động bù chữ 'phần trăm' nếu LLM sinh thiếu hoặc nuốt mất
    text = restore_missing_percentage_words(text)
    
    # 9. Tối ưu dấu câu và ngữ điệu
    text = optimize_live_punctuation(text)
    
    # 10. Triệt tiêu lặp từ 'cả nhà' / 'nha cả nhà'
    text = deduplicate_overused_particles(text)
    
    # Làm sạch khoảng trắng
    text = re.sub(r'\s+', ' ', text).strip()
    return text


if __name__ == '__main__':
    # Test cases
    samples = [
        "Trúc Linh nhé, cảm ơn Trúc Linh nha!",
        "Trúc Linh ơi cùng trải nghiệm giải pháp AI và OBS Studio này nè.",
        "Khẩu hình miệng khớp 99%, nhân bản giọng nói chỉ trong 1 phút 30s.",
        "Hệ thống vận hành 24/7, cam kết 100% tiết kiệm 80% chi phí.",
        "Rẻ nhất thị trường, hãy để lại sđt hoặc inbox zalo để nhận demo nhé.",
        "Trang bị camera ảo phát trực tiếp chuẩn Full HD và 4K sắc nét đó.",
        "Chi phí sử dụng nhân vật số này chỉ bằng tám mươi chi phí thuê người mẫu.",
        "Với độ chính xác đến chín mươi chín, nhân vật số ây ai sẽ có biểu cảm sống động.",
        "Tiết kiệm tới tám mươi đấy ạ, giảm chi phí tới tám mươi so với thuê người thật.",
    ]
    print("=== KIỂM THỬ CHUẨN HÓA VĂN BẢN LIVESTREAM ===")
    for s in samples:
        print("GỐC :", s)
        print("CHUẨN:", normalize_ecom_livestream_text(s))
        print("-" * 50)
