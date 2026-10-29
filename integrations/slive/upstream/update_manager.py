"""Version check and verified installer download for the desktop app."""
from __future__ import annotations

import agency_runtime
import hashlib
import json
import os
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

from runtime_config import CENTRAL_API_URL
from runtime_paths import installed_program_dir, program_dir, updater_dir, updates_dir


def version_tuple(value):
    parts = []
    for item in str(value or "0").strip().lstrip("v").split("."):
        try:
            parts.append(int(item))
        except ValueError:
            parts.append(0)
    return tuple(parts)


def current_version():
    if agency_runtime.manifest():
        return agency_runtime.installed_version()
    try:
        payload = json.loads((program_dir() / "version.json").read_text(encoding="utf-8-sig"))
        return str(payload.get("version") or "0.0.0")
    except Exception:
        return "0.0.0"


def check_update(timeout=10):
    if agency_runtime.manifest():
        release = agency_runtime.latest_release()
        if not release: return {"available": False}
        return {**release, "available": agency_runtime.newer_version(release["version"], current_version()),
                "currentVersion": current_version()}
    from ssl_utils import create_download_ssl_context
    url = f"{CENTRAL_API_URL}/api/app/version?app_id=sliveface&app=sliveface&channel=stable&platform=windows-x64"
    request = urllib.request.Request(url, headers={"Accept": "application/json", "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"})
    context = create_download_ssl_context()
    with urllib.request.urlopen(request, timeout=timeout, context=context) as response:
        payload = json.loads(response.read().decode("utf-8"))
    if isinstance(payload.get("data"), dict):
        payload = payload["data"]
    latest = str(payload.get("version") or "0.0.0")
    download_url = str(payload.get("url") or "")
    download_sha256 = str(payload.get("sha256") or "").lower()
    if download_url and f"v{current_version()}" in download_url:
        latest = current_version()
    release_ready = (
        urllib.parse.urlparse(download_url).scheme.lower() == "https"
        and len(download_sha256) == 64
        and all(char in "0123456789abcdef" for char in download_sha256)
    )
    return {
        "available": release_ready and version_tuple(latest) > version_tuple(current_version()),
        "currentVersion": current_version(),
        "version": latest,
        "url": download_url,
        "sha256": download_sha256,
        "mandatory": bool(payload.get("mandatory")),
        "notes": str(payload.get("notes") or ""),
    }


def _sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for chunk in iter(lambda: handle.read(4 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download_update(info, progress=None):
    from ssl_utils import create_download_ssl_context
    agency_runtime.validate_release(info)
    if not info.get("available") or not info.get("url") or not info.get("sha256"):
        raise RuntimeError("Thông tin cập nhật chưa đầy đủ.")
    if urllib.parse.urlparse(str(info["url"])).scheme.lower() != "https":
        raise RuntimeError("Link cập nhật phải sử dụng HTTPS.")
    if len(str(info["sha256"]).strip()) != 64:
        raise RuntimeError("SHA-256 của bản cập nhật không hợp lệ.")
    updates_dir().mkdir(parents=True, exist_ok=True)
    url_str = str(info.get("url") or "")
    is_patch = url_str.lower().endswith(".zip") or str(info.get("type") or "").lower() == "patch"
    ext = ".zip" if is_patch else ".exe"
    destination = updates_dir() / f"SLiveFace-{info['version']}{ext}"
    temporary = destination.with_suffix(".download")
    request = urllib.request.Request(info["url"], headers={"User-Agent": "SLiveFace/Desktop"})
    context = create_download_ssl_context()
    start_time = time.monotonic()
    last_emit = 0.0
    with agency_runtime.open_download_request(request, timeout=60) as response, temporary.open("wb") as output:
        total = int(response.headers.get("Content-Length") or 0)
        done = 0
        while True:
            chunk = response.read(64 * 1024)
            if not chunk:
                break
            output.write(chunk)
            done += len(chunk)
            now = time.monotonic()
            if progress and (now - last_emit >= 0.08 or done >= total):
                last_emit = now
                elapsed = max(0.001, now - start_time)
                speed_bps = done / elapsed
                rem_bytes = max(0, total - done)
                eta_sec = int(rem_bytes / speed_bps) if speed_bps > 0 else 0
                pct = round((done * 100 / total), 1) if total > 0 else 0
                progress({
                    "done": done,
                    "total": total,
                    "percent": pct,
                    "speedMb": round(speed_bps / (1024 * 1024), 2),
                    "doneMb": round(done / (1024 * 1024), 2),
                    "totalMb": round(total / (1024 * 1024), 2),
                    "etaSec": eta_sec
                })
    if _sha256(temporary).lower() != info["sha256"].lower():
        temporary.unlink(missing_ok=True)
        raise RuntimeError("Bản cập nhật chưa trọn vẹn do lỗi đường truyền. Vui lòng thử lại.")
    temporary.replace(destination)

    # Nếu là bản vá vi sai (.zip), giải nén áp dụng trực tiếp ngay lập tức
    if is_patch:
        apply_patch(destination)
    return destination


def apply_patch(patch_path):
    """Áp dụng bản vá vi sai (.zip) siêu nhanh mà không cần chạy lại installer."""
    import zipfile
    import shutil
    target_dir = program_dir()
    agency = agency_runtime.manifest()

    with zipfile.ZipFile(patch_path, "r") as archive:
        namelist = set(archive.namelist())

        if agency:
            # 1. Nếu app đang chạy là App đại lý:
            # Bản vá BẮT BUỘC phải là bản vá dành riêng cho đại lý đó (chứa agency.json và trùng agencyId)
            # Ngăn chặn việc áp dụng nhầm bản patch chung của Slive (làm mất thương hiệu đại lý)
            if "agency.json" not in namelist:
                raise RuntimeError("Bản vá không chứa thông tin đại lý đã xác thực. Không thể áp dụng patch chung.")
            try:
                patch_agency = json.loads(archive.read("agency.json").decode("utf-8-sig"))
                curr_agency_id = str(agency.get("agencyId", "")).strip().lower()
                patch_agency_id = str(patch_agency.get("agencyId", "")).strip().lower()
                if patch_agency_id != curr_agency_id:
                    raise RuntimeError(f"Bản vá này thuộc đại lý khác ({patch_agency.get('name')}), không áp dụng được cho {agency.get('name')}.")
            except Exception as exc:
                if isinstance(exc, RuntimeError):
                    raise
                raise RuntimeError(f"Không thể xác thực thông tin đại lý trong bản vá: {exc}")
        else:
            # 2. Ngược lại nếu app đang chạy là bản Slive gốc:
            # Không cho phép áp dụng bản vá của đại lý lên bản Slive gốc
            if "agency.json" in namelist or "agency.required" in namelist:
                raise RuntimeError("Không thể áp dụng bản vá của đại lý cho bản Slive gốc.")

        for member in archive.infolist():
            dest_file = (target_dir / member.filename).resolve()
            if os.path.commonpath([str(target_dir), str(dest_file)]) != str(target_dir):
                continue
            if member.is_dir():
                dest_file.mkdir(parents=True, exist_ok=True)
            else:
                dest_file.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(member) as src, dest_file.open("wb") as dst:
                    shutil.copyfileobj(src, dst)

        # Đảm bảo đồng bộ version.json và agency.json ở cả user_root / root_dir nếu cấu trúc thư mục có cấp app/
        try:
            root = agency_runtime.root_dir()
            if root != target_dir:
                for sync_name in ("version.json", "agency.json"):
                    if sync_name in namelist and (root / sync_name).is_file():
                        with archive.open(sync_name) as src, (root / sync_name).open("wb") as dst:
                            shutil.copyfileobj(src, dst)
        except Exception:
            pass

    return True


def launch_updater(installer, info):
    url_str = str(info.get("url") or "")
    is_patch = url_str.lower().endswith(".zip") or str(info.get("type") or "").lower() == "patch"
    if is_patch:
        # Bản vá đã được giải nén thành công qua apply_patch, không cần khởi chạy SLiveFaceUpdater.exe
        return
    if agency_runtime.manifest():
        agency_runtime.validate_release(info)
        if _sha256(installer).lower() != info["sha256"].lower():
            raise RuntimeError("Bản cập nhật sai SHA-256.")
        subprocess.Popen([str(installer), "/SILENT", "/DIR=" + str(installed_program_dir())], close_fds=True)
        return
    candidates = [
        updater_dir() / "SLiveFaceUpdater.exe",
        program_dir() / "SLiveFaceUpdater.exe",
    ]
    helper = next((item for item in candidates if item.is_file()), None)
    if helper:
        command = [str(helper)]
    elif not getattr(sys, "frozen", False):
        command = [sys.executable, str(program_dir() / "updater.py")]
    else:
        raise RuntimeError("Thiếu trình cập nhật hệ thống.")
    command += [
        "--installer", str(installer),
        "--sha256", str(info["sha256"]),
        "--app-exe", str(installed_program_dir() / "SLiveFace_Setup.exe"),
        "--wait-pid", str(os.getpid()),
    ]
    subprocess.Popen(command, cwd=str(program_dir()), close_fds=True)
