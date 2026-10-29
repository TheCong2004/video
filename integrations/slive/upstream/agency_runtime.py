"""Tenant identity shared by launcher, auth, billing and updater. No bank secrets."""
import hashlib
import json
import os
import re
import ssl
import sys
import time
import urllib.request
from pathlib import Path
from urllib.parse import urljoin, urlsplit

SERVICE_TYPE = "live_face"
_cache = None
_site = None
_site_at = 0
_manifest_path = None

def root_dir():
    root = Path(sys.executable).resolve().parent if getattr(sys, "frozen", False) else Path(__file__).resolve().parent
    return root.parent if root.name.lower() == "app" else root

def manifest():
    global _cache, _manifest_path
    if _cache is not None:
        return _cache
    path = root_dir() / "agency.json"
    if not getattr(sys, "frozen", False):
        agency_arg = os.environ.get("SLIVE_AGENCY", "").strip()
        for idx, arg in enumerate(sys.argv):
            if arg == "--agency" and idx + 1 < len(sys.argv):
                agency_arg = sys.argv[idx + 1].strip()
                break
            elif arg.startswith("--agency="):
                agency_arg = arg.split("=", 1)[1].strip()
                break
        if agency_arg:
            if not re.fullmatch(r"[A-Za-z0-9_-]+", agency_arg):
                raise RuntimeError("Tên đại lý không hợp lệ.")
            found = False
            for cand in [
                root_dir() / "agencies" / f"{agency_arg}.json",
                root_dir() / "agencies" / agency_arg / "config.json",
                root_dir() / f"agency_{agency_arg}.json"
            ]:
                if cand.is_file():
                    path = cand
                    found = True
                    break
            if not found:
                raise RuntimeError("Không tìm thấy cấu hình ứng dụng: " + agency_arg)
        elif os.environ.get("SLIVE_AGENCY_MANIFEST"):
            path = Path(os.environ["SLIVE_AGENCY_MANIFEST"])
    if not path.is_file():
        if (root_dir() / "agency.required").exists():
            raise RuntimeError("Thiếu cấu hình ứng dụng. Vui lòng cài đặt lại ứng dụng.")
        _cache = {}
        return _cache
    data = json.loads(path.read_text(encoding="utf-8-sig"))
    data.setdefault("schemaVersion", 1)
    data.setdefault("serviceType", SERVICE_TYPE)
    data.setdefault("channel", "stable")
    if not data.get("agencyId"):
        slug_key = str(data.get("slug") or "slive").lower()
        if slug_key in ("slive", "platform"):
            data["agencyId"] = "000000000000000000000001"
        else:
            raise RuntimeError("Cấu hình ứng dụng không hợp lệ. Vui lòng cài đặt lại ứng dụng.")
    origin = urlsplit(str(data.get("agencyOrigin", "")))
    if (data.get("schemaVersion") != 1 or not re.fullmatch(r"[a-f0-9]{24}", str(data.get("agencyId", "")))
        or data.get("serviceType") != SERVICE_TYPE or origin.scheme != "https" or not origin.hostname
        or origin.username or origin.password or origin.query or origin.fragment or origin.path not in ("", "/")):
        raise RuntimeError("Cấu hình ứng dụng không hợp lệ. Vui lòng cài đặt lại ứng dụng.")
    data["agencyOrigin"] = data["agencyOrigin"].rstrip("/")
    if data.get("channel", "stable") not in ("stable", "beta"):
        raise RuntimeError("Kênh phát hành không hợp lệ.")
    _manifest_path = path.resolve()
    if not getattr(sys, "frozen", False):
        os.environ["SLIVE_AGENCY_MANIFEST"] = str(_manifest_path)
        os.environ.pop("SLIVE_AGENCY", None)
    _cache = data
    return data

def api_origin(fallback="https://slive.vn"):
    return manifest().get("agencyOrigin") or fallback

def profile_key():
    return manifest().get("agencyId", "platform") + "-" + SERVICE_TYPE + "-" + manifest().get("channel", "stable")

def profile_dir():
    path = Path(os.environ.get("LOCALAPPDATA") or Path.home()) / "SLive" / profile_key()
    path.mkdir(parents=True, exist_ok=True)
    return path

def display_name():
    return manifest().get("name") or ("SLive Thường" if SERVICE_TYPE == "live_noface" else "SLive VIP")

def local_port(default):
    if not manifest():
        return default
    return 20000 + (int(hashlib.sha256(profile_key().encode()).hexdigest()[:6], 16) % 10000) * 3 + (default % 3)

TRUSTED_DOWNLOAD_DOMAINS = (
    "objects.githubusercontent.com",
    "github-releases.githubusercontent.com",
    "github.com",
    "r2.dev",
    "r2.cloudflarestorage.com",
    "amazonaws.com",
)

class SafeRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        old, new = urlsplit(req.full_url), urlsplit(newurl)
        if (old.scheme, old.netloc) != (new.scheme, new.netloc):
            new_host = new.netloc.split(":")[0].lower()
            if any(new_host == d or new_host.endswith("." + d) for d in TRUSTED_DOWNLOAD_DOMAINS):
                # Xóa token nhạy cảm trước khi chuyển hướng sang kho lưu trữ/CDN bên ngoài
                for h in list(req.headers.keys()):
                    if h.lower() in ("authorization", "cookie"):
                        req.headers.pop(h, None)
                return super().redirect_request(req, fp, code, msg, headers, newurl)
            raise RuntimeError("Không chuyển yêu cầu đăng nhập sang máy chủ khác.")
        return super().redirect_request(req, fp, code, msg, headers, newurl)

DEFAULT_USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"

def open_request(request, timeout=15):
    if isinstance(request, str):
        request = urllib.request.Request(request)
    if not request.has_header('User-agent'):
        request.add_header('User-Agent', DEFAULT_USER_AGENT)
    if not request.has_header('Accept'):
        request.add_header('Accept', 'application/json, text/plain, */*')
    return urllib.request.build_opener(SafeRedirect(), urllib.request.HTTPSHandler(context=ssl.create_default_context())).open(request, timeout=timeout)

def open_download_request(request, timeout=60):
    if isinstance(request, str):
        request = urllib.request.Request(request)
    if not request.has_header('User-agent') and not request.has_header('User-Agent'):
        request.add_header('User-Agent', DEFAULT_USER_AGENT)
    ctx = ssl.create_default_context()
    return urllib.request.build_opener(urllib.request.HTTPSHandler(context=ctx)).open(request, timeout=timeout)

def site_config(force=False):
    global _site, _site_at
    if _site is not None and not force and time.monotonic() - _site_at < 60:
        return _site
    m = manifest()
    try:
        req = urllib.request.Request(api_origin() + "/api/config.json", headers={"Accept": "application/json"})
        with open_request(req, timeout=5) as response:
            site = json.load(response)
        if m and site.get("agencyId") != m["agencyId"]:
            raise RuntimeError("AGENCY_MISMATCH: origin identity does not match.")
        for key in ("logo", "favicon"):
            if site.get(key):
                site[key] = urljoin(api_origin() + "/", site[key])
        _site, _site_at = site, time.monotonic()
        return site
    except Exception as exc:
        if isinstance(exc, RuntimeError) and "AGENCY_MISMATCH" in str(exc):
            raise
        if m:
            logo_name = m.get("logo") or "agency-logo.png"
            logo_url = f"/{logo_name}" if (root_dir() / logo_name).is_file() or (root_dir() / "web" / logo_name).is_file() else "/favicon.png"
            _site = {
                "name": m.get("name", "SLive"),
                "agencyId": m.get("agencyId", ""),
                "logo": logo_url,
                "favicon": logo_url,
                "branding": {
                    "siteName": m.get("name", "SLive"),
                    "logoUrl": logo_url,
                    "faviconUrl": logo_url
                }
            }
            _site_at = time.monotonic()
            return _site
        raise

def validate_release(release):
    expected = manifest()
    if not expected:
        return release
    for field, value in (("agencyId", expected["agencyId"]), ("serviceType", SERVICE_TYPE), ("channel", expected.get("channel", "stable"))):
        if release.get(field) != value:
            raise RuntimeError("Bản cập nhật không phù hợp với ứng dụng này.")
    if not re.fullmatch(r"\d+\.\d+\.\d+", str(release.get("version", ""))):
        raise RuntimeError("Phiên bản cập nhật không hợp lệ.")
    if not re.fullmatch(r"[a-fA-F0-9]{64}", str(release.get("sha256", ""))):
        raise RuntimeError("Thiếu SHA-256 bản cập nhật.")
    target_url = str(release.get("downloadUrl") or release.get("url") or "")
    url = urlsplit(target_url)
    if url.scheme != "https" or not url.hostname or url.username or url.password or not (url.path.lower().endswith(".exe") or url.path.lower().endswith(".zip")):
        raise RuntimeError("Bản cập nhật cần bộ cài EXE hoặc bản vá ZIP qua HTTPS.")
    return release

def latest_release():
    from urllib.parse import urlencode
    data = manifest()
    if not data:
        return None
    site_config()
    query = urlencode({"agencyId": data["agencyId"], "serviceType": SERVICE_TYPE, "channel": data.get("channel", "stable")})
    with open_request(urllib.request.Request(api_origin() + "/api/releases/latest?" + query)) as response:
        release = json.load(response).get("release")
    return validate_release(release) if release else None


def device_id():
    import uuid
    path = profile_dir() / "device-id"
    if not path.exists():
        try:
            with path.open("x", encoding="utf-8") as stream:
                stream.write(uuid.uuid4().hex)
        except FileExistsError:
            pass
    value = path.read_text(encoding="utf-8").strip()
    if not re.fullmatch(r"[a-f0-9]{32}", value):
        raise RuntimeError("Mã thiết bị không hợp lệ.")
    return value


def newer_version(candidate, installed):
    def parts(value):
        if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", str(value)):
            raise RuntimeError("Phiên bản không hợp lệ.")
        return tuple(int(part) for part in value.split("."))
    return parts(candidate) > parts(installed)


def installed_version():
    candidates = [
        root_dir() / "app" / "version.json",
        root_dir() / "version.json",
        Path(__file__).resolve().parent / "version.json"
    ]
    versions = []
    for cand in candidates:
        if cand.is_file():
            try:
                ver = str(json.loads(cand.read_text(encoding="utf-8-sig")).get("version") or "").strip()
                if re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", ver):
                    versions.append(ver)
            except Exception:
                pass
    if versions:
        return max(versions, key=lambda v: tuple(int(x) for x in v.split(".")))
    return "0.0.0"


def local_logo_path():
    data = manifest()
    if not data:
        return None
    base = _manifest_path.parent if _manifest_path else root_dir()
    name = data.get("logo") or data.get("logoPath")
    slug = data.get("slug")
    candidates = []
    if name:
        candidates.extend([root_dir() / name, base / name, root_dir() / "agencies" / name])
    if slug:
        candidates.extend([root_dir() / f"{slug}.png", base / f"{slug}.png"])
    candidates.extend([
        root_dir() / "agency-logo.png",
        root_dir() / "agency.png",
        base / "agency-logo.png",
        base / "agency.png",
    ])
    return next((path.resolve() for path in candidates if path.is_file()), None)


def render_loading_html(template):
    """Brand the local splash before any AI service or network call is ready."""
    import base64
    import html
    import mimetypes
    name = html.escape(display_name(), quote=True)
    logo = local_logo_path()
    # HTML passed to pywebview has no reliable relative base URL; embed the
    # image so the branded and default splash both render offline.
    splash_logo = logo or (root_dir() / "web" / "favicon.png")
    if not splash_logo.is_file():
        splash_logo = root_dir() / "icon.ico"
    logo_url = ""
    if splash_logo.is_file():
        mime = mimetypes.guess_type(str(splash_logo))[0] or "image/png"
        logo_url = "data:" + mime + ";base64," + base64.b64encode(splash_logo.read_bytes()).decode("ascii")
    # Older/loading templates use several capitalizations of the same brand.
    branded = re.sub(r"(?i)slive\s+vip", lambda _match: name, template)
    branded = re.sub(r"(?i)slive\s+thường", lambda _match: name, branded)
    return branded.replace('favicon.png', logo_url)


def window_icon_path():
    data = manifest()
    if not data:
        cand = root_dir() / "icon.ico"
        return cand if cand.is_file() else None

    base = _manifest_path.parent if _manifest_path else root_dir()
    slug = data.get("slug")
    candidates = []
    if slug:
        candidates.extend([
            root_dir() / f"{slug}.ico",
            root_dir() / "agencies" / f"{slug}.ico",
            base / f"{slug}.ico",
            root_dir() / "web" / f"{slug}.ico",
        ])
    candidates.extend([
        root_dir() / "agency.ico",
        base / "agency.ico",
        root_dir() / "web" / "agency.ico",
    ])
    for cand in candidates:
        if cand.is_file():
            return cand

    logo = local_logo_path()
    if logo and logo.is_file():
        try:
            from PIL import Image, ImageOps
            icon = profile_dir() / "agency.ico"
            with Image.open(logo) as image:
                icon.parent.mkdir(parents=True, exist_ok=True)
                ImageOps.pad(image.convert("RGBA"), (256, 256)).save(
                    icon, format="ICO", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
                )
            return icon
        except Exception:
            pass

    cand = root_dir() / "icon.ico"
    return cand if cand.is_file() else None
