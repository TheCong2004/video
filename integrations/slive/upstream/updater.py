"""Small external process that installs a verified Inno Setup update."""
from __future__ import annotations

import argparse
import hashlib
import os
import subprocess
import sys
import time
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(4 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


import ctypes


def process_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    if sys.platform == "win32":
        try:
            PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
            STILL_ACTIVE = 259
            handle = ctypes.windll.kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
            if not handle:
                return False
            exit_code = ctypes.c_ulong()
            success = ctypes.windll.kernel32.GetExitCodeProcess(handle, ctypes.byref(exit_code))
            ctypes.windll.kernel32.CloseHandle(handle)
            return bool(success and exit_code.value == STILL_ACTIVE)
        except Exception:
            return False
    try:
        os.kill(pid, 0)
        return True
    except Exception:
        return False


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--installer", required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--app-exe", required=True)
    parser.add_argument("--wait-pid", type=int, default=0)
    args = parser.parse_args()

    installer = Path(args.installer).resolve()
    app_exe = Path(args.app_exe).resolve()
    if not installer.is_file():
        raise SystemExit("Không tìm thấy installer cập nhật.")
    if sha256(installer).lower() != args.sha256.strip().lower():
        raise SystemExit("Installer cập nhật sai SHA-256.")

    deadline = time.monotonic() + 90
    while process_alive(args.wait_pid) and time.monotonic() < deadline:
        time.sleep(0.5)
    command = [
        str(installer), "/SP-", "/VERYSILENT", "/SUPPRESSMSGBOXES",
        "/NORESTART", "/CLOSEAPPLICATIONS", "/NORESTARTAPPLICATIONS",
    ]
    result = subprocess.run(command, check=False)
    if result.returncode != 0:
        raise SystemExit(f"Installer kết thúc với mã lỗi {result.returncode}.")
    if not app_exe.is_file():
        raise SystemExit("Cài xong nhưng không tìm thấy ứng dụng.")
    subprocess.Popen([str(app_exe), "--updated"], cwd=str(app_exe.parent))


if __name__ == "__main__":
    main()
