import agency_runtime
import asyncio
import hashlib
import json
import os
import shutil
import ssl
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

from aiohttp import web

from runtime_config import application_dir, load_config
from runtime_paths import installed_resources_path, required_resource_path, resources_dir
from server.central_auth import CentralAuth
from ssl_utils import create_download_ssl_context


class CustomRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        new_req = super().redirect_request(req, fp, code, msg, headers, newurl)
        if new_req:
            for k, v in req.headers.items():
                if k.lower() in ("user-agent", "accept", "accept-language"):
                    new_req.add_header(k, v)
        return new_req


class ResourceManager:
    def __init__(self, app_dir=None, install_root=None, manifest_path=None):
        self.app_dir = Path(app_dir or application_dir()).resolve()
        self.install_root = Path(install_root or resources_dir()).resolve()
        self.config = load_config(self.app_dir)
        if not manifest_path:
            cust_manifest = self.app_dir / "customer-resource-manifest.json"
            if cust_manifest.is_file():
                manifest_path = cust_manifest
            else:
                manifest_path = self.app_dir / "resource-manifest.json"
        self.manifest_path = Path(manifest_path)
        self.manifest = self._read_json(self.manifest_path, {"bundles": []})
        self.is_bootstrap_manifest = self.manifest_path.name == "customer-resource-manifest.json"
        if agency_runtime.manifest() and not self.is_bootstrap_manifest:
            self.manifest["bundles"] = [bundle for bundle in self.manifest.get("bundles", [])
                                       if bundle.get("install_root") in ("resources", "data")]
        self.ssl_context = ssl.create_default_context() if agency_runtime.manifest() else create_download_ssl_context()
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPSHandler(context=self.ssl_context),
            CustomRedirectHandler()
        )
        self.lock = threading.RLock()
        self.launch_requested = threading.Event()
        self.cancel_requested = threading.Event()
        self.auto_launch_on_complete = False
        self.bundle_progress = {}
        self.progress = {
            "running": False, "percent": 0, "downloaded": 0, "total": 0,
            "speed_mb": 0.0, "speed_text": "-- MB/s",
            "eta_seconds": 0, "eta_text": "--",
            "stage": "Sẵn sàng",
            "current": "Sẵn sàng thiết lập",
            "error": "", "complete": False, "pending_ids": [],
        }

    def _installed_versions(self):
        return self._read_json(installed_resources_path(), {"bundles": {}})

    def _record_installed(self, bundle):
        state = self._installed_versions()
        state.setdefault("bundles", {})[str(bundle.get("id"))] = {
            "version": str(bundle.get("version") or "1"),
            "release": str(self.manifest.get("release") or ""),
        }
        installed_resources_path().parent.mkdir(parents=True, exist_ok=True)
        temporary = installed_resources_path().with_suffix(".tmp")
        temporary.write_text(json.dumps(state, ensure_ascii=False, indent=2), encoding="utf-8")
        temporary.replace(installed_resources_path())

    @staticmethod
    def _read_json(path, default):
        try:
            return json.loads(path.read_text(encoding="utf-8-sig"))
        except Exception:
            return default

    def get_bundle_install_root(self, bundle):
        root_type = bundle.get("install_root")
        if agency_runtime.manifest() and not getattr(self, "is_bootstrap_manifest", False) and root_type not in ("resources", "data"):
            raise RuntimeError("Cần cập nhật ứng dụng bằng bộ cài phù hợp.")
        base_dir = self.app_dir.parent if self.app_dir.name.lower() == "app" else self.app_dir
        if root_type == "resources":
            return base_dir / "resources"
        elif root_type == "data":
            return base_dir / "data"
        return base_dir / "app"

    def _bundle_required_files_present(self, bundle):
        required = bundle.get("required") or []
        install_root = self.get_bundle_install_root(bundle)
        if not required:
            return False
        for item in required:
            if item in ("SliveVip.exe", "Live_AI_LipSync.exe"):
                slive = install_root / "SliveVip.exe"
                live = install_root / "Live_AI_LipSync.exe"
                has_valid = (slive.is_file() and slive.stat().st_size >= 20_000_000) or \
                            (live.is_file() and live.stat().st_size >= 20_000_000)
                if not has_valid:
                    return False
                internal_dir = install_root / "_internal"
                if internal_dir.is_dir() and not any(internal_dir.glob("python*.dll")):
                    return False
                if not internal_dir.is_dir() and max(slive.stat().st_size if slive.is_file() else 0, live.stat().st_size if live.is_file() else 0) < 50_000_000:
                    return False
                continue
            p = required_resource_path(install_root, item)
            if not p.exists():
                return False
            # Ensure compiled PE executable is not truncated or corrupted (< 20MB)
            if p.suffix.lower() == ".exe" and p.stat().st_size < 20_000_000:
                return False
        return True

    def _bundle_installed(self, bundle):
        # extractall() can create all required files before it has finished
        # extracting the rest of the archive. Do not report "installed" while
        # the worker is still running or the frontend will freeze at 99%.
        bundle_id = str(bundle.get("id"))
        with self.lock:
            installing = bool(self.progress["running"])
            pending_ids = self.progress.get("pending_ids") or []
        if installing and bundle_id in pending_ids:
            return False
        return self._bundle_is_current(bundle)

    def _bundle_is_current(self, bundle):
        bundle_id = str(bundle.get("id"))
        expected = str(bundle.get("version") or "1")
        installed = self._installed_versions().get("bundles", {}).get(bundle_id)
        if installed and str(installed.get("version") or "") == expected:
            if self._bundle_required_files_present(bundle):
                return True
            return False
        if self._bundle_required_files_present(bundle):
            self._record_installed(bundle)
            return True
        return False

    def _sync_runtime_brand_assets(self, install_dir):
        try:
            icon_src = self.app_dir / "icon.ico"
            if icon_src.is_file():
                shutil.copy2(icon_src, install_dir / "icon.ico")
            web_src = self.app_dir / "web"
            if web_src.is_dir():
                shutil.copytree(web_src, install_dir / "web", dirs_exist_ok=True)
            for fname in ["agency.json", "agency-logo.png", "agency.png", "smlive.png", "icon.ico"]:
                fpath = self.app_dir / fname
                if fpath.is_file():
                    shutil.copy2(fpath, install_dir / fname)
                    if (install_dir / "web").is_dir():
                        shutil.copy2(fpath, install_dir / "web" / fname)
            # Ensure SliveVip.exe is the active executable (without running CopyIcons)
            slive_exe = install_dir / "SliveVip.exe"
            live_exe = install_dir / "Live_AI_LipSync.exe"
            if live_exe.is_file() and live_exe.stat().st_size >= 20_000_000:
                if not slive_exe.is_file() or slive_exe.stat().st_size < 20_000_000:
                    shutil.copy2(live_exe, slive_exe)
            elif slive_exe.is_file() and slive_exe.stat().st_size >= 20_000_000:
                if not live_exe.is_file() or live_exe.stat().st_size < 20_000_000:
                    shutil.copy2(slive_exe, live_exe)
            slive_cfg = install_dir / "SliveVip.exe.config"
            live_cfg = install_dir / "Live_AI_LipSync.exe.config"
            if live_cfg.is_file() and not slive_cfg.is_file():
                shutil.copy2(live_cfg, slive_cfg)
        except Exception:
            pass

    @staticmethod
    def _friendly_bundle_name(index, item):
        name = item.get("name")
        if name and any(kw in name for kw in ["Gói Nhân", "Mô Hình", "Bộ Thư Viện", "Bộ Nhận Diện", "Kho Dữ Liệu"]):
            return name
        bundle_id = str(item.get("id") or "").lower()
        bundle_type = str(item.get("type") or "").lower()
        if bundle_type == "app-runtime" or "runtime" in bundle_id:
            brand = agency_runtime.display_name() if agency_runtime.manifest() else "Slive VIP"
            return f"Gói Hệ Thống & Vận Hành {brand}"
        if "wav2lip" in bundle_id or "lip" in bundle_id:
            return "Mô Hình Khẩu Hình & Khớp Môi AI"
        if "vieneu-tts" in bundle_id or "tts" in bundle_id or "voice" in bundle_id or bundle_type == "tts":
            return "Bộ Thư Viện Giọng Đọc & Phát Âm AI"
        if "s3fd" in bundle_id or "face" in bundle_id:
            return "Bộ Nhận Diện Khuôn Mặt & Chuyển Động"
        if "avatar" in bundle_id:
            return "Kho Dữ Liệu Avatar & Nhân Vật Live VIP"
        return name or f"Gói Tính Năng Nâng Cao {index}"

    def status(self):
        with self.lock:
            internal_progress = dict(self.progress)
            bp_map = {k: dict(v) for k, v in self.bundle_progress.items()}

        bundles = []
        for index, item in enumerate(self.manifest.get("bundles") or [], 1):
            bundle_id = str(item.get("id") or "")
            installed = self._bundle_installed(item)
            bp = bp_map.get(bundle_id, {})
            b_total = int(item.get("size") or 0)
            if installed:
                b_percent = 100
                b_downloaded = b_total
                b_status = "installed"
                b_stage = "Hoàn thành"
                b_speed = ""
            else:
                b_percent = bp.get("percent", 0)
                b_downloaded = bp.get("downloaded", 0)
                b_status = bp.get("status", "pending")
                b_stage = bp.get("stage", "Chờ thiết lập")
                b_speed = bp.get("speed_text", "")

            bundle = {
                "id": bundle_id,
                "name": self._friendly_bundle_name(index, item),
                "size": b_total,
                "installed": installed,
                "percent": b_percent,
                "downloaded": b_downloaded,
                "status": b_status,
                "stage": b_stage,
                "speed_text": b_speed,
            }
            bundles.append(bundle)
        installed = bool(bundles) and all(item["installed"] for item in bundles)

        progress = {
            key: internal_progress.get(key)
            for key in ("running", "percent", "downloaded", "total", "complete",
                        "speed_mb", "speed_text", "eta_seconds", "eta_text", "stage", "current", "pending_ids")
        }
        if internal_progress.get("error"):
            progress["current"] = "Thiết lập dữ liệu chưa thành công"
            progress["stage"] = "Gặp sự cố kết nối"
            progress["error"] = internal_progress.get("error") or "Không thể thiết lập dữ liệu. Vui lòng kiểm tra mạng và thử lại."
        elif internal_progress.get("running"):
            progress["current"] = internal_progress.get("current") or "Đang tải và thiết lập dữ liệu Slive VIP..."
            progress["stage"] = internal_progress.get("stage") or "Đang tiến hành..."
            progress["error"] = ""
        elif internal_progress.get("complete"):
            progress["current"] = "Đã hoàn tất thiết lập Không gian Slive VIP"
            progress["stage"] = "Hoàn tất"
            progress["percent"] = 100
            progress["eta_text"] = "Hoàn thành"
            progress["speed_text"] = "0 MB/s"
            progress["error"] = ""
        else:
            progress["current"] = "Sẵn sàng thiết lập"
            progress["stage"] = "Chờ bắt đầu"
            progress["error"] = ""
        return {"installed": installed, "bundles": bundles, "progress": progress}

    def _bundle_url(self, bundle):
        url = str(bundle.get("url") or "").strip()
        if not url:
            raise RuntimeError(f"Chưa cấu hình URL tải cho {bundle.get('name') or bundle.get('id')}.")
        if urllib.parse.urlparse(url).scheme:
            return url
        base = str(self.config.get("resource_base_url") or "").rstrip("/") + "/"
        if base == "/":
            raise RuntimeError("Thiếu resource_base_url trong config.json.")
        return urllib.parse.urljoin(base, url)

    def _set_progress(self, **values):
        with self.lock:
            self.progress.update(values)

    @staticmethod
    def _safe_extract(archive, destination):
        destination = destination.resolve()
        for member in archive.infolist():
            target = (destination / member.filename).resolve()
            if os.path.commonpath([str(destination), str(target)]) != str(destination):
                raise RuntimeError("Gói tài nguyên chứa đường dẫn không an toàn.")
        archive.extractall(destination)

    def _install(self, target_bundle_id=None):
        manifest_order = [str(b.get("id")) for b in self.manifest.get("bundles", [])]
        pending = [
            b for b in self.manifest.get("bundles", [])
            if not self._bundle_is_current(b)
        ]
        if target_bundle_id:
            pending = [b for b in pending if str(b.get("id")) == str(target_bundle_id)]
        else:
            pending.sort(key=lambda b: manifest_order.index(str(b.get("id"))) if str(b.get("id")) in manifest_order else 999)
        pending_ids = [str(b.get("id")) for b in pending]
        self._set_progress(pending_ids=pending_ids)
        if not pending:
            self._set_progress(running=False, percent=100, complete=True, error="")
            if self.auto_launch_on_complete:
                self.launch_requested.set()
            return
        
        total = sum(max(1, int(b.get("size") or 0)) for b in pending)
        completed_bytes = 0
        completed_bytes_lock = threading.Lock()
        
        active_downloads = {}
        active_downloads_lock = threading.Lock()
        base_root = self.app_dir.parent if self.app_dir.name.lower() == "app" else self.app_dir
        downloads_dir = base_root / "downloads"
        downloads_dir.mkdir(parents=True, exist_ok=True)
        
        session_start = time.monotonic()
        errors = []
        install_succeeded = False
        
        # Khởi tạo trạng thái cho từng gói theo thứ tự từ trên xuống
        with self.lock:
            for b in self.manifest.get("bundles") or []:
                bid = str(b.get("id"))
                bsize = int(b.get("size") or 0)
                if self._bundle_installed(b):
                    self.bundle_progress[bid] = {
                        "percent": 100, "downloaded": bsize, "total": bsize,
                        "status": "installed", "stage": "Hoàn thành", "speed_text": ""
                    }
                else:
                    self.bundle_progress[bid] = {
                        "percent": 0, "downloaded": 0, "total": bsize,
                        "status": "pending", "stage": "Chờ thiết lập", "speed_text": ""
                    }
        
        def download_and_install_bundle(bundle):
            nonlocal completed_bytes
            bundle_id = str(bundle.get("id"))
            bundle_size = int(bundle.get("size") or 0)
            sources = bundle.get("parts") or [bundle]
            part_files = []
            bundle_completed_parts_bytes = 0

            with self.lock:
                if bundle_id in self.bundle_progress:
                    self.bundle_progress[bundle_id]["status"] = "downloading"
                    self.bundle_progress[bundle_id]["stage"] = "Đang thiết lập"
            
            try:
                for part_index, source in enumerate(sources, 1):
                    if self.cancel_requested.is_set():
                        raise RuntimeError("Hủy tải tài nguyên theo yêu cầu người dùng.")

                    url = self._bundle_url(source)
                    parsed_url = urllib.parse.urlparse(url)
                    part_name = os.path.basename(parsed_url.path)
                    if not part_name:
                        part_name = f"{bundle_id}_part_{part_index}.zip"
                    part_file = downloads_dir / part_name
                    part_files.append(part_file)
                    
                    part_size = int(source.get("size") or 0)
                    part_expected_sha = str(source.get("sha256") or "").lower().strip()
                    primary_url = url
                    if "https://" in url[8:]:
                        direct_url = "https://" + url[8:].split("https://", 1)[1]
                        candidate_urls = [direct_url, primary_url]
                    else:
                        candidate_urls = [primary_url]

                    initial_existing_bytes = part_file.stat().st_size if part_file.exists() else 0
                    part_download_retries = 0

                    while part_download_retries < 15:
                        if self.cancel_requested.is_set():
                            raise RuntimeError("Hủy tải tài nguyên theo yêu cầu người dùng.")

                        downloaded_bytes = part_file.stat().st_size if part_file.exists() else 0
                        if part_size > 0 and downloaded_bytes >= part_size:
                            part_digest = hashlib.sha256()
                            with part_file.open("rb") as f:
                                while True:
                                    chunk = f.read(1024 * 1024)
                                    if not chunk:
                                        break
                                    part_digest.update(chunk)
                            if not part_expected_sha or part_digest.hexdigest().lower() == part_expected_sha:
                                with completed_bytes_lock:
                                    completed_bytes += part_size
                                break
                            else:
                                part_file.unlink(missing_ok=True)
                                downloaded_bytes = 0
                                initial_existing_bytes = 0

                        response = None
                        mode = "ab" if downloaded_bytes > 0 else "wb"
                        part_start_time = time.monotonic()
                        
                        try:
                            for attempt_url in candidate_urls:
                                try:
                                    headers = {
                                        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
                                        "Accept": "*/*",
                                    }
                                    request = urllib.request.Request(attempt_url, headers=headers)
                                    if downloaded_bytes > 0:
                                        request.add_header("Range", f"bytes={downloaded_bytes}-")
                                    response = self.opener.open(request, timeout=60)
                                    if downloaded_bytes > 0 and response.status != 206:
                                        downloaded_bytes = 0
                                        initial_existing_bytes = 0
                                        mode = "wb"
                                        part_file.unlink(missing_ok=True)
                                    break
                                except urllib.error.HTTPError as he:
                                    if he.code == 416:
                                        # Requested Range Not Satisfiable: corrupt or completed part file, reset
                                        part_file.unlink(missing_ok=True)
                                        downloaded_bytes = 0
                                        initial_existing_bytes = 0
                                        mode = "wb"
                                        request = urllib.request.Request(attempt_url, headers=headers)
                                        response = self.opener.open(request, timeout=60)
                                        break
                                    if attempt_url == candidate_urls[-1]:
                                        raise
                                except Exception:
                                    if attempt_url == candidate_urls[-1]:
                                        raise

                            with part_file.open(mode) as output:
                                while True:
                                    if self.cancel_requested.is_set():
                                        output.flush()
                                        output.close()
                                        raise RuntimeError("Hủy tải tài nguyên theo yêu cầu người dùng.")

                                    chunk = response.read(1024 * 1024)
                                    if not chunk:
                                        break
                                    output.write(chunk)
                                    
                                    # Track download progress of active part
                                    current_part_bytes = output.tell()
                                    downloaded_this_session = max(0, current_part_bytes - initial_existing_bytes)
                                    
                                    part_url = source.get("url") or url
                                    with active_downloads_lock:
                                        active_downloads[part_url] = downloaded_this_session
                                        total_active = sum(active_downloads.values())
                                        current = completed_bytes + total_active
                                        
                                    part_elapsed = max(0.1, time.monotonic() - part_start_time)
                                    part_speed_mb = (downloaded_this_session / part_elapsed) / (1024 * 1024)
                                    current_mb = current / (1024 * 1024)
                                    total_mb = total / (1024 * 1024)

                                    session_elapsed = max(0.2, time.monotonic() - session_start)
                                    overall_speed_mb = max(0.1, current / session_elapsed / (1024 * 1024))
                                    display_speed = (part_speed_mb * 0.7 + overall_speed_mb * 0.3)

                                    remaining_bytes = max(0, total - current)
                                    eta_seconds = int(remaining_bytes / (display_speed * 1024 * 1024)) if display_speed > 0.05 else 0
                                    if eta_seconds > 0:
                                        if eta_seconds < 60:
                                            eta_text = f"Khoảng {eta_seconds} giây"
                                        else:
                                            mins = eta_seconds // 60
                                            secs = eta_seconds % 60
                                            eta_text = f"Khoảng {mins} phút {secs:02d} giây"
                                    else:
                                        eta_text = "Đang tính toán..."

                                    speed_text = f"{display_speed:.1f} MB/s"
                                    friendly_name = self._friendly_bundle_name(part_index, bundle)
                                    label = f" (Phần {part_index}/{len(sources)})" if len(sources) > 1 else ""
                                    status_text = f"Đang thiết lập {friendly_name}{label}: {current_mb:.1f} / {total_mb:.1f} MB ({speed_text})"
                                    self._set_progress(
                                        current=status_text,
                                        stage="Đang thiết lập...",
                                        downloaded=current,
                                        total=total,
                                        speed_mb=round(display_speed, 1),
                                        speed_text=speed_text,
                                        eta_seconds=eta_seconds,
                                        eta_text=eta_text,
                                        percent=min(97, int(current * 100 / max(1, total)))
                                    )

                                    # Cập nhật riêng cho gói hiện tại
                                    bundle_current_bytes = min(bundle_size, bundle_completed_parts_bytes + downloaded_bytes + downloaded_this_session)
                                    bundle_percent = min(98, int(bundle_current_bytes * 100 / max(1, bundle_size))) if bundle_size > 0 else 0
                                    with self.lock:
                                        if bundle_id in self.bundle_progress:
                                            self.bundle_progress[bundle_id].update({
                                                "downloaded": bundle_current_bytes,
                                                "percent": bundle_percent,
                                                "status": "downloading",
                                                "stage": f"Đang thiết lập {bundle_percent}%",
                                                "speed_text": speed_text
                                            })
                        except Exception as exc:
                            if self.cancel_requested.is_set():
                                raise
                            part_download_retries += 1
                            if part_download_retries >= 15:
                                raise RuntimeError("Kết nối tải dữ liệu bị gián đoạn sau nhiều lần thử. Vui lòng kiểm tra mạng và thử lại.")
                            time.sleep(2)
                            continue
                    
                    # Mark this part's dynamic download tracking as completed
                    part_url = source.get("url") or url
                    with active_downloads_lock:
                        if part_url in active_downloads:
                            active_downloads[part_url] = 0
                    part_stat_len = part_file.stat().st_size if part_file.exists() else 0
                    with completed_bytes_lock:
                        completed_bytes += part_stat_len - downloaded_bytes
                    bundle_completed_parts_bytes += part_stat_len
                        
                if self.cancel_requested.is_set():
                    raise RuntimeError("Hủy tải tài nguyên theo yêu cầu người dùng.")

                target = downloads_dir / f"{bundle_id}.zip"
                overall_pct = min(99, int(completed_bytes * 100 / max(1, total)))
                self._set_progress(
                    current="Đang đồng bộ và sắp xếp dữ liệu...",
                    stage="Đang thiết lập...",
                    percent=overall_pct,
                    eta_text="Sắp hoàn tất"
                )
                with target.open("wb") as merged:
                    for part_file in part_files:
                        with part_file.open("rb") as pf:
                            shutil.copyfileobj(pf, merged)
                            
                expected_sha = str(bundle.get("sha256") or "").lower().strip()
                if expected_sha:
                    self._set_progress(
                        current="Đang kiểm tra tính toàn vẹn dữ liệu...",
                        stage="Đang thiết lập...",
                        percent=overall_pct,
                        eta_text="Sắp hoàn tất"
                    )
                    digest = hashlib.sha256()
                    with target.open("rb") as f:
                        while True:
                            chunk = f.read(1024 * 1024)
                            if not chunk:
                                break
                            digest.update(chunk)
                    if digest.hexdigest().lower() != expected_sha:
                        target.unlink(missing_ok=True)
                        raise RuntimeError("Dữ liệu tải về chưa trọn vẹn do đường truyền. Vui lòng bấm thử lại.")
                        
                self._set_progress(
                    current=f"Đang thiết lập {self._friendly_bundle_name(0, bundle)}...",
                    stage="Đang thiết lập...",
                    percent=overall_pct,
                    eta_text="Khoảng vài giây"
                )
                with self.lock:
                    if bundle_id in self.bundle_progress:
                        self.bundle_progress[bundle_id].update({
                            "downloaded": bundle_size,
                            "percent": 99,
                            "status": "extracting",
                            "stage": "Đang thiết lập",
                            "speed_text": ""
                        })

                install_dir = self.get_bundle_install_root(bundle)
                install_dir.mkdir(parents=True, exist_ok=True)
                with zipfile.ZipFile(target) as archive:
                    self._safe_extract(archive, install_dir)
                if not self._bundle_required_files_present(bundle):
                    raise RuntimeError("Thiết lập dữ liệu chưa đầy đủ. Vui lòng bấm thử lại.")
                if str(bundle.get("type") or "").lower() == "app-runtime" or "runtime" in bundle_id:
                    self._sync_runtime_brand_assets(install_dir)
                self._record_installed(bundle)
                target.unlink(missing_ok=True)
                for part_file in part_files:
                    part_file.unlink(missing_ok=True)

                with self.lock:
                    if bundle_id in self.bundle_progress:
                        self.bundle_progress[bundle_id].update({
                            "downloaded": bundle_size,
                            "percent": 100,
                            "status": "installed",
                            "stage": "Hoàn thành",
                            "speed_text": ""
                        })
                    
            except Exception as exc:
                errors.append(exc)
                raise

        try:
            self._set_progress(running=True, percent=0, downloaded=0, total=total,
                               complete=False, error="")
            
            for b in pending:
                if self.cancel_requested.is_set():
                    break
                curr_id = str(b.get("id"))
                friendly_curr = self._friendly_bundle_name(0, b)
                with self.lock:
                    if curr_id in self.bundle_progress:
                        self.bundle_progress[curr_id].update({
                            "status": "downloading",
                            "stage": "Đang thiết lập"
                        })
                self._set_progress(
                    pending_ids=[curr_id],
                    current=f"Đang thiết lập {friendly_curr}...",
                    stage="Đang thiết lập..."
                )
                download_and_install_bundle(b)
                if errors:
                    raise errors[0]
                
            if not self.cancel_requested.is_set():
                install_succeeded = True
                self._set_progress(running=True, percent=99, downloaded=total, total=total,
                                   current="Hoàn thành thiết lập toàn bộ tài nguyên!", complete=False, error="")
        except Exception as exc:
            if self.cancel_requested.is_set():
                with self.lock:
                    self.progress.update({
                        "running": False,
                        "error": "",
                        "current": "Đã hủy tải tài nguyên",
                        "stage": "Đã dừng",
                        "complete": False
                    })
                    for bid, bp in self.bundle_progress.items():
                        if bp.get("status") in ("downloading", "pending"):
                            bp["status"] = "canceled"
                            bp["stage"] = "Đã hủy"
                return

            import traceback
            try:
                log_p = (self.app_dir.parent if self.app_dir.name.lower() == "app" else self.app_dir) / "setup-error.log"
                with log_p.open("a", encoding="utf-8") as f:
                    f.write(f"\n[{time.strftime('%Y-%m-%d %H:%M:%S')}] INSTALL ERROR:\n{traceback.format_exc()}\n")
            except Exception:
                pass
            reason = exc.reason if isinstance(exc, urllib.error.URLError) else exc
            if isinstance(reason, ssl.SSLCertVerificationError):
                message = (
                    "Không xác minh được chứng chỉ HTTPS. Hãy bật tự động ngày giờ Windows, "
                    "cập nhật chứng chỉ gốc hoặc tắt tạm chế độ quét HTTPS của antivirus/proxy rồi thử lại."
                )
            else:
                message = str(exc)
            self._set_progress(running=False, error=message, current="Cài đặt thất bại")
        finally:
            pass

        if install_succeeded:
            self._set_progress(running=False, percent=100, downloaded=total, total=total,
                               current="Hoàn thành thiết lập toàn bộ tài nguyên!", stage="Hoàn thành",
                               complete=True, error="", pending_ids=[])

    def cancel(self):
        with self.lock:
            if not self.progress.get("running"):
                return False
            self.cancel_requested.set()
            self.progress["current"] = "Đang dừng tiến trình tải..."
            self.progress["stage"] = "Đã yêu cầu hủy"
        return True

    def start(self, target_bundle_id=None):
        with self.lock:
            if self.progress.get("running"):
                return False
            self.cancel_requested.clear()
            self.progress.update(
                running=True,
                error="",
                complete=False,
                stage="Đang thiết lập...",
                current="Đang thiết lập tài nguyên Slive Vip...",
                percent=0
            )
        threading.Thread(target=self._install, args=(target_bundle_id,), daemon=True).start()
        return True


def run_bootstrap(host, port, manager=None):
    manager = manager or ResourceManager()
    manager.auto_launch_on_complete = False
    auth = CentralAuth(str(manager.app_dir))

    def bootstrap_status():
        status = manager.status()
        status["mode"] = "bootstrap"
        return status

    async def resources_status(_request):
        return web.json_response(bootstrap_status())

    async def resources_install(request):
        bundle_id = None
        try:
            body = await request.json()
            bundle_id = body.get("bundle_id")
        except Exception:
            pass
        manager.start(bundle_id)
        return web.json_response(bootstrap_status())

    async def resources_cancel(_request):
        manager.cancel()
        return web.json_response(bootstrap_status())

    async def resources_launch(_request):
        status = bootstrap_status()
        if not status["installed"]:
            return web.json_response({"success": False, "error": "Tài nguyên chưa được cài đầy đủ."}, status=409)
        import asyncio
        asyncio.get_event_loop().call_later(0.5, manager.launch_requested.set)
        return web.json_response({"success": True})

    async def dashboard_page(_request):
        return web.FileResponse(manager.app_dir / "web" / "dashboard.html")

    async def index_page(_request):
        return web.FileResponse(manager.app_dir / "web" / "dashboard.html")

    async def resources_page(_request):
        raise web.HTTPFound("/dashboard.html")

    async def app_exit(_request):
        import asyncio
        asyncio.get_event_loop().call_later(0.3, lambda: os._exit(0))
        return web.json_response({"success": True})

    async def app_version(_request):
        from runtime_config import get_app_version
        v = get_app_version(manager.app_dir)
        return web.json_response({"success": True, "version": v})

    async def serve():
        app = web.Application(middlewares=[auth.middleware])
        app["central_auth"] = auth
        auth.setup_routes(app)
        app.router.add_get("/api/resources/status", resources_status)
        app.router.add_post("/api/resources/install", resources_install)
        app.router.add_post("/api/resources/cancel", resources_cancel)
        app.router.add_post("/api/resources/launch", resources_launch)
        app.router.add_post("/api/app/exit", app_exit)
        app.router.add_get("/api/app/version", app_version)
        app.router.add_get("/", index_page)
        app.router.add_get("/index.html", index_page)
        app.router.add_get("/dashboard.html", dashboard_page)
        app.router.add_get("/resources.html", resources_page)
        app.router.add_static("/", path=str(manager.app_dir / "web"))
        runner = web.AppRunner(app)
        await runner.setup()
        site = web.TCPSite(runner, host, int(port))
        await site.start()
        while not manager.launch_requested.is_set():
            await asyncio.sleep(0.25)
        # Give the web client a moment to receive the success response
        await asyncio.sleep(0.4)
        # Fast exit the subprocess to let the launcher proceed instantly
        os._exit(0)

    asyncio.run(serve())
