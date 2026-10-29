import re
import json
import logging
import ipaddress
import socket
from urllib.parse import urlparse
import requests
from bs4 import BeautifulSoup

logger = logging.getLogger(__name__)

HEADERS = {
    'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
    'Accept': 'text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8',
    'Accept-Language': 'vi,en-US;q=0.9,en;q=0.8',
    'Cache-Control': 'no-cache',
    'Pragma': 'no-cache',
}
MAX_RESPONSE_BYTES = 1_500_000


def validate_public_url(url: str) -> str:
    """Allow only public HTTP(S) URLs and reject SSRF targets."""
    parsed = urlparse(str(url or '').strip())
    if parsed.scheme not in {'http', 'https'} or not parsed.hostname:
        raise ValueError('Chỉ hỗ trợ URL http/https hợp lệ.')
    if parsed.username or parsed.password:
        raise ValueError('URL có thông tin đăng nhập không được hỗ trợ.')

    host = parsed.hostname.rstrip('.').lower()
    try:
        addresses = {item[4][0] for item in socket.getaddrinfo(host, parsed.port, type=socket.SOCK_STREAM)}
    except socket.gaierror as exc:
        raise ValueError(f'Không phân giải được tên miền: {host}') from exc
    for address in addresses:
        ip = ipaddress.ip_address(address)
        if not ip.is_global:
            raise ValueError(f'URL trỏ tới địa chỉ mạng nội bộ hoặc không công khai: {host}')
    return parsed.geturl()

def extract_url(text: str) -> str:
    """Trích xuất URL đầu tiên (http hoặc https) trong đoạn text."""
    if not text:
        return None
    match = re.search(r'https?://[^\s\u3000-\u303F\uFF00-\uFFEF]+', text)
    return match.group(0) if match else None

def clean_description(text: str) -> str:
    """Làm sạch phần mô tả, loại bỏ các ký tự trắng thừa và giới hạn độ dài."""
    if not text:
        return ""
    text = re.sub(r'\s+', ' ', text).strip()
    if len(text) > 800:
        text = text[:800] + "..."
    return text

def parse_product_info(html_content: str, url: str) -> dict:
    """Phân tích nội dung HTML để trích xuất thông tin sản phẩm."""
    info = {
        'title': '',
        'description': '',
        'price': '',
        'image_url': '',
        'site_name': ''
    }
    
    if not html_content:
        return info

    try:
        soup = BeautifulSoup(html_content, 'html.parser')
    except Exception as e:
        logger.error("Lỗi khi parse HTML bằng BeautifulSoup: %s", e)
        return info

    # 1. Thử parse JSON-LD (Schema.org Product)
    for script in soup.find_all('script', type='application/ld+json'):
        try:
            data = json.loads(script.string or '')
            
            def find_product(obj):
                if isinstance(obj, list):
                    for item in obj:
                        res = find_product(item)
                        if res:
                            return res
                elif isinstance(obj, dict):
                    if obj.get('@type') == 'Product' or obj.get('type') == 'Product':
                        return obj
                    for key, val in obj.items():
                        if isinstance(val, (dict, list)):
                            res = find_product(val)
                            if res:
                                return res
                return None

            product_data = find_product(data)
            if product_data:
                if 'name' in product_data:
                    info['title'] = product_data['name']
                if 'description' in product_data:
                    info['description'] = product_data['description']
                if 'offers' in product_data:
                    offers = product_data['offers']
                    if isinstance(offers, list) and offers:
                        offers = offers[0]
                    if isinstance(offers, dict):
                        price = offers.get('price')
                        currency = offers.get('priceCurrency') or 'VND'
                        if price:
                            info['price'] = f"{price} {currency}"
                if 'image' in product_data:
                    img = product_data['image']
                    if isinstance(img, list) and img:
                        img = img[0]
                    if isinstance(img, str):
                        info['image_url'] = img
                    elif isinstance(img, dict) and 'url' in img:
                        info['image_url'] = img['url']
                break
        except Exception:
            continue

    # 2. Thử Open Graph và các thẻ Meta chuẩn (dành cho SEO của các web)
    def get_meta_content(selector):
        tag = soup.select_one(selector)
        return tag.get('content', '').strip() if tag else ''

    if not info['title']:
        info['title'] = (
            get_meta_content('meta[property="og:title"]') or 
            get_meta_content('meta[name="twitter:title"]') or 
            (soup.title.string.strip() if soup.title else '')
        )
        
    if not info['description']:
        info['description'] = (
            get_meta_content('meta[property="og:description"]') or 
            get_meta_content('meta[name="twitter:description"]') or 
            get_meta_content('meta[name="description"]')
        )

    if not info['price']:
        price_amount = (
            get_meta_content('meta[property="og:price:amount"]') or 
            get_meta_content('meta[property="product:price:amount"]')
        )
        price_currency = (
            get_meta_content('meta[property="og:price:currency"]') or 
            get_meta_content('meta[property="product:price:currency"]') or
            'VND'
        )
        if price_amount:
            info['price'] = f"{price_amount} {price_currency}"

    if not info['image_url']:
        info['image_url'] = (
            get_meta_content('meta[property="og:image"]') or 
            get_meta_content('meta[name="twitter:image"]')
        )

    info['site_name'] = get_meta_content('meta[property="og:site_name"]')
    if not info['site_name']:
        parsed_url = urlparse(url)
        info['site_name'] = parsed_url.netloc

    # Làm sạch các trường thông tin
    info['title'] = clean_description(info['title'])
    info['description'] = clean_description(info['description'])
    info['price'] = clean_description(info['price'])
    
    return info

from urllib.parse import urlparse, parse_qs

def safe_print(msg: str):
    """In ra màn hình console một cách an toàn, tránh lỗi UnicodeEncodeError trên Windows console."""
    try:
        print(msg)
    except UnicodeEncodeError:
        try:
            print(msg.encode('ascii', errors='replace').decode('ascii'))
        except Exception:
            pass

def extract_info_from_url_query(url: str) -> dict:
    """Trích xuất thông tin sản phẩm trực tiếp từ các tham số trên URL nếu có (ví dụ: og_info trên TikTok Shop)."""
    info = {}
    try:
        parsed_url = urlparse(url)
        query_params = parse_qs(parsed_url.query)
        
        # Check og_info (TikTok Shop copy link thường có tham số này chứa title và image)
        if 'og_info' in query_params:
            try:
                og_info_data = json.loads(query_params['og_info'][0])
                if isinstance(og_info_data, dict):
                    if 'title' in og_info_data and og_info_data['title']:
                        info['title'] = og_info_data['title']
                    if 'image' in og_info_data and og_info_data['image']:
                        info['image_url'] = og_info_data['image']
            except Exception as e:
                logger.error("Lỗi parse og_info từ URL: %s", e)
                
        # Check các tham số title, name phổ biến khác nếu có
        if not info.get('title'):
            for key in ['title', 'name', 'product_name', 'p_name']:
                if key in query_params and query_params[key][0]:
                    info['title'] = query_params[key][0]
                    break
    except Exception as e:
        logger.error("Lỗi khi trích xuất thông tin từ URL query: %s", e)
    return info

def fetch_product_info(url: str) -> dict:
    """Tải URL và trích xuất dữ liệu chi tiết của sản phẩm."""
    logger.info("Đang lấy thông tin sản phẩm từ link: %s", url)
    safe_print(f"[PRODUCT CRAWLER] Đang kết nối tải trang: {url}")
    
    # 1. Trích xuất thông tin từ URL query trước (ví dụ TikTok Shop og_info)
    url_query_info = extract_info_from_url_query(url)
    
    response = None
    try:
        url = validate_public_url(url)
        response = requests.get(url, headers=HEADERS, timeout=(4, 8), stream=True, allow_redirects=True)
        response.raise_for_status()
        
        # Lấy URL cuối cùng sau khi đã chuyển hướng (redirect)
        final_url = response.url
        if final_url != url:
            final_url = validate_public_url(final_url)
            safe_print(f"[PRODUCT CRAWLER] Đã chuyển hướng từ {url} đến {final_url}")
            # Trích xuất thêm thông tin từ URL cuối cùng
            redirect_query_info = extract_info_from_url_query(final_url)
            for key, val in redirect_query_info.items():
                if val:
                    url_query_info[key] = val

        content_type = (response.headers.get('Content-Type') or '').lower()
        if content_type and 'text/html' not in content_type and 'application/xhtml+xml' not in content_type:
            raise ValueError(f'Bỏ qua nội dung không phải HTML: {content_type}')
        declared_length = response.headers.get('Content-Length')
        if declared_length and int(declared_length) > MAX_RESPONSE_BYTES:
            raise ValueError('Trang sản phẩm vượt quá giới hạn kích thước cho phép.')
        body = response.raw.read(MAX_RESPONSE_BYTES + 1)
        if len(body) > MAX_RESPONSE_BYTES:
            raise ValueError('Trang sản phẩm vượt quá giới hạn kích thước cho phép.')
        encoding = response.encoding or 'utf-8'
        info = parse_product_info(body.decode(encoding, errors='replace'), final_url)
        
        # Kiểm tra xem tiêu đề lấy được từ HTML có phải là trang chặn bảo mật hoặc generic
        title_lower = info.get('title', '').lower()
        is_blocked = any(kw in title_lower for kw in ['security check', 'captcha', 'robot check', 'security verification', 'access denied', 'forbidden'])
        if title_lower == 'tiktok shop' or not title_lower:
            is_blocked = True

        if is_blocked:
            safe_print(f"[PRODUCT CRAWLER] Phát hiện trang chặn bảo mật hoặc không có tiêu đề cụ thể ({info.get('title')}). Chuyển sang sử dụng thông tin từ URL query.")
            info['title'] = ''
            info['description'] = ''
        
        # Gộp thông tin từ URL query nếu trong HTML không có
        for key in ['title', 'description', 'price', 'image_url', 'site_name']:
            if not info.get(key) and url_query_info.get(key):
                info[key] = url_query_info[key]
                
        safe_print(f"[PRODUCT CRAWLER] Lấy thông tin thành công: Tên={info.get('title')}, Giá={info.get('price')}")
        return info
    except Exception as e:
        logger.error("Lỗi khi tải thông tin sản phẩm từ %s: %s", url, e)
        safe_print(f"[PRODUCT CRAWLER] WARN: Lỗi tải trang: {e}")
        
        # Thử lấy từ url_query_info của URL chuyển hướng (nếu requests ném ngoại lệ nhưng response đã được tạo)
        if response is not None:
            final_url = response.url
            if final_url != url:
                safe_print(f"[PRODUCT CRAWLER] Hỗ trợ lấy URL chuyển hướng khi lỗi: {final_url}")
                redirect_query_info = extract_info_from_url_query(final_url)
                for key, val in redirect_query_info.items():
                    if val:
                        url_query_info[key] = val

        # Nếu tải trang lỗi (ví dụ bị chặn bởi Cloudflare/TikTok), nhưng chúng ta đã lấy được title từ URL query,
        # thì sử dụng luôn thông tin đó thay vì dùng fallback mặc định!
        if url_query_info.get('title'):
            parsed_url = urlparse(url)
            fallback_info = {
                'title': url_query_info['title'],
                'description': f"Sản phẩm được chia sẻ qua liên kết: {url}",
                'price': '',
                'image_url': url_query_info.get('image_url', ''),
                'site_name': parsed_url.netloc
            }
            safe_print(f"[PRODUCT CRAWLER] Sử dụng thông tin trích xuất từ URL query thành công.")
            return fallback_info
            
        parsed_url = urlparse(url)
        return {
            'title': f"Sản phẩm từ trang {parsed_url.netloc}",
            'description': f"Chi tiết sản phẩm tại liên kết: {url}",
            'price': '',
            'image_url': '',
            'site_name': parsed_url.netloc
        }

def process_prompt_with_links(prompt: str) -> str:
    """Kiểm tra xem prompt có chứa link hay không. Nếu có, cào thông tin và thay thế vào prompt."""
    url = extract_url(prompt)
    if not url:
        return prompt

    info = fetch_product_info(url)
    
    # Tạo chuỗi thông tin chi tiết sản phẩm
    details = []
    if info.get('title'):
        details.append(f"Tên sản phẩm: {info['title']}")
    if info.get('price'):
        details.append(f"Giá: {info['price']}")
    if info.get('description'):
        details.append(f"Mô tả sản phẩm: {info['description']}")
    if info.get('site_name'):
        details.append(f"Nguồn/Website: {info['site_name']}")
    
    product_details_str = "\n".join(details)
    
    # Thay thế link gốc bằng chuỗi thông tin chi tiết để ChatGPT có thể tối ưu
    replaced_prompt = prompt.replace(url, f"\n[THÔNG TIN SẢN PHẨM TỪ LINK TRUY CẬP TRỰC TIẾP]:\n{product_details_str}\n")
    return replaced_prompt
