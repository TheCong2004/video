import atexit
import json
import multiprocessing
import os
import socket
import subprocess
import sys
import time
import urllib.request
import traceback
import tempfile
import threading
from pathlib import Path

os.environ.setdefault("PYTHONUTF8", "1")
os.environ.setdefault("PYTHONIOENCODING", "utf-8")

def _fix_windows_dll_search_path():
    if sys.platform == "win32":
        import ctypes
        app_base = Path(sys.executable).parent if getattr(sys, "frozen", False) else Path(__file__).resolve().parent
        candidate_dirs = [
            app_base / "_internal" / "torch" / "lib",
            app_base / "torch" / "lib",
            app_base / "_internal",
            app_base,
        ]
        for cdir in candidate_dirs:
            if cdir.is_dir():
                cdir_str = str(cdir)
                try:
                    os.add_dll_directory(cdir_str)
                except Exception:
                    pass
                try:
                    ctypes.windll.kernel32.SetDllDirectoryW(cdir_str)
                except Exception:
                    pass

def _prefer_disk_modules():
    """Ưu tiên nạp các file .py ngoài đĩa (app.py, server/routes.py, llm.py...)
    trước khi tìm trong binary freeze PyInstaller để hỗ trợ hot patch."""
    try:
        app_base = Path(sys.executable).parent if getattr(sys, "frozen", False) else Path(__file__).resolve().parent
        if str(app_base) not in sys.path:
            sys.path.insert(0, str(app_base))
        from importlib.machinery import PathFinder
        current_meta = [f for f in sys.meta_path if f is not PathFinder]
        sys.meta_path = [PathFinder] + current_meta
    except Exception:
        pass

_fix_windows_dll_search_path()
_prefer_disk_modules()

import agency_runtime

from runtime_config import (
    application_dir,
    apply_runtime_environment,
    load_config,
    refresh_remote_runtime_config,
)
from process_supervisor import ProcessSupervisor
from runtime_paths import (
    avatar_path,
    face_detector_path,
    logs_dir,
    model_path,
    resources_dir,
    tts_cache_dir,
)


def _preload_webview_dlls():
    try:
        import sys, os
        app_base = Path(sys.executable).parent if getattr(sys, "frozen", False) else Path(__file__).resolve().parent
        candidate_dirs = [
            app_base,
            app_base / "_internal",
            app_base / "_internal" / "webview" / "lib",
            app_base / "webview" / "lib",
            app_base / "lib",
        ]
        for cdir in candidate_dirs:
            if cdir.is_dir():
                dll_core = cdir / "Microsoft.Web.WebView2.Core.dll"
                if dll_core.is_file():
                    try:
                        import clr
                        clr.AddReference(str(dll_core))
                    except Exception:
                        pass
                dll_wf = cdir / "Microsoft.Web.WebView2.WinForms.dll"
                if dll_wf.is_file():
                    try:
                        import clr
                        clr.AddReference(str(dll_wf))
                    except Exception:
                        pass
    except Exception:
        pass

_preload_webview_dlls()


APP_DIR = application_dir()
os.chdir(APP_DIR)
CONFIG = load_config(APP_DIR)
refresh_remote_runtime_config(CONFIG)
apply_runtime_environment(CONFIG, APP_DIR)

if (APP_DIR / "ffmpeg").is_dir():
    os.environ["PATH"] = str(APP_DIR / "ffmpeg") + os.pathsep + os.environ.get("PATH", "")
elif (APP_DIR / "ffmpeg.exe").is_file():
    os.environ["PATH"] = str(APP_DIR) + os.pathsep + os.environ.get("PATH", "")


def safe_print(*args):
    try:
        print(*args, flush=True)
    except Exception:
        pass


def subprocess_command(flag):
    if getattr(sys, "frozen", False):
        return [sys.executable, flag]
    return [sys.executable, str(Path(__file__).resolve()), flag]


def run_tts_service():
    import uvicorn
    from tts_server.app import app
    uvicorn.run(app, host=str(CONFIG["tts_host"]), port=int(CONFIG["tts_port"]),
                log_level="warning", access_log=False)


def run_engine_service():
    cmd = [
        "app.py", "--model", str(CONFIG["model"]), "--avatar_id", str(CONFIG["avatar_id"]),
        "--transport", str(CONFIG["transport"]), "--tts", str(CONFIG["tts"]),
        "--TTS_SERVER", str(CONFIG["tts_server"]), "--REF_FILE", str(CONFIG["tts_voice"]),
        "--listenport", str(CONFIG["port"])
    ]
    if "batch_size" in CONFIG:
        cmd.extend(["--batch_size", str(CONFIG["batch_size"])])
    if "camera_index" in CONFIG:
        cmd.extend(["--camera_index", str(CONFIG["camera_index"])])
    sys.argv = cmd
    from app import main as engine_main
    engine_main()


def run_avatar_generator():
    if len(sys.argv) < 4:
        raise RuntimeError("Thiếu tham số tạo avatar.")
    video_path, avatar_id = sys.argv[2], sys.argv[3]
    generator_args = sys.argv[4:]
    sys.argv = ["genavatar.py", "--video_path", video_path, "--avatar_id", avatar_id,
                *generator_args]
    import runpy
    runpy.run_module("avatars.wav2lip.genavatar", run_name="__main__")


def run_bootstrap_service():
    from resource_manager import run_bootstrap
    run_bootstrap(str(CONFIG["host"]), int(CONFIG["port"]))


def dispatch_subprocess():
    if len(sys.argv) < 2:
        return False
    if sys.argv[1] == "--run-tts":
        run_tts_service()
        return True
    if sys.argv[1] == "--run-engine":
        run_engine_service()
        return True
    if sys.argv[1] == "--run-genavatar":
        run_avatar_generator()
        return True
    if sys.argv[1] == "--run-bootstrap":
        run_bootstrap_service()
        return True
    if sys.argv[1] == "--run-tiktok":
        username = sys.argv[2] if len(sys.argv) > 2 else ""
        sessionid = sys.argv[3] if len(sys.argv) > 3 else "current"
        from live import start_tiktok_listener
        start_tiktok_listener(username, sessionid)
        return True
    return False


def validate_dependencies():
    required = ["torch", "aiohttp", "aiortc", "av", "cv2", "fastapi", "uvicorn", "vieneu"]
    missing = []
    for package in required:
        try:
            __import__(package)
        except ImportError:
            missing.append(package)
    if missing:
        raise RuntimeError("Thiếu thư viện: " + ", ".join(missing))
def validate_resources():
    model = model_path()
    if not model.is_file():
        raise RuntimeError(f"Thiếu file trọng số mô hình Wav2Lip (models/wav2lip384.pth hoặc models/wav2lip.pth)")
    avatar_id = str(CONFIG.get("avatar_id", "")).strip()
    if avatar_id.lower() not in ("webcam", "camera", "livecam") and not avatar_id.lower().startswith("webcam"):
        avatar = avatar_path(avatar_id)
        required_avatar = [avatar / "coords.pkl", avatar / "full_imgs", avatar / "face_imgs"]
        missing_assets = [str(path) for path in required_avatar if not path.exists()]
        if missing_assets:
            raise RuntimeError("Avatar chưa đầy đủ: " + ", ".join(missing_assets))
    cache = tts_cache_dir() / "hub"
    model_cache = cache / "models--pnnbao-ump--VieNeu-TTS-v3-Turbo" / "snapshots"
    codec_cache = cache / "models--OpenMOSS-Team--MOSS-Audio-Tokenizer-Nano-ONNX" / "snapshots"
    if not face_detector_path().is_file():
        raise RuntimeError("Thiếu mô hình nhận diện khuôn mặt S3FD.")
    if not model_cache.is_dir() or not codec_cache.is_dir():
        raise RuntimeError("Thiếu cache VieNeu-TTS hoặc MOSS codec. Không thể chạy TTS offline.")


def resources_installed():
    try:
        from resource_manager import ResourceManager
        res = ResourceManager(APP_DIR, install_root=resources_dir()).status()["installed"]
        if not res:
            try:
                error_log = logs_dir() / "resources-check-error.log"
                error_log.parent.mkdir(parents=True, exist_ok=True)
                with error_log.open("a", encoding="utf-8") as f:
                    f.write(f"--- status['installed'] is False at {time.ctime()} ---\n")
                    status_info = ResourceManager(APP_DIR, install_root=resources_dir()).status()
                    f.write(json.dumps(status_info, indent=2, ensure_ascii=False) + "\n")
            except Exception:
                pass
        return res
    except Exception as exc:
        try:
            error_log = logs_dir() / "resources-check-error.log"
            error_log.parent.mkdir(parents=True, exist_ok=True)
            with error_log.open("a", encoding="utf-8") as f:
                f.write(f"--- Exception at {time.ctime()} ---\n")
                traceback.print_exc(file=f)
        except Exception:
            pass
        return False


def port_available(host, port):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        try:
            sock.bind((host, int(port)))
            return True
        except OSError:
            return False


def json_health(url):
    try:
        with urllib.request.urlopen(url, timeout=2) as response:
            body = json.loads(response.read().decode("utf-8"))
            return response.status == 200 and body.get("status") == "ready"
    except Exception:
        return False


def web_health(url):
    try:
        with urllib.request.urlopen(url, timeout=2) as response:
            return response.status == 200
    except Exception:
        return False


processes = []
log_files = []
process_supervisor = None
cleanup_lock = threading.Lock()
cleanup_complete = False


class DesktopApi:
    def __init__(self):
        self._avatar_window = None
        self._main_window = None
        self._lock = threading.Lock()

    def open_avatar_window(self):
        with self._lock:
            if self._avatar_window is not None:
                try:
                    if hasattr(self._avatar_window, 'native') and self._avatar_window.native is None:
                        raise RuntimeError("Avatar window native disposed")
                    self._avatar_window.show()
                    self._avatar_window.restore()
                    return True
                except Exception:
                    self._avatar_window = None

            import webview
            target = f"http://{CONFIG['host']}:{CONFIG['port']}/avatar-live.html"
            try:
                self._avatar_window = webview.create_window(
                    "Avatar Live", url=target, width=1600, height=900,
                    min_size=(960, 540), resizable=True, maximized=True,
                    background_color="#000000", zoomable=False,
                )
                def _on_closed():
                    with self._lock:
                        self._avatar_window = None
                self._avatar_window.events.closed += _on_closed
            except Exception:
                self._avatar_window = None
            return self._avatar_window is not None

    def check_for_update(self):
        try:
            from update_manager import check_update
            return {"success": True, **check_update()}
        except Exception as exc:
            return {"success": False, "error": str(exc)}

    def get_app_version(self):
        try:
            from update_manager import current_version
            return {"success": True, "version": current_version()}
        except Exception as exc:
            return {"success": False, "error": str(exc)}

    def install_update(self, info):
        try:
            import importlib
            import agency_runtime
            import update_manager
            try:
                importlib.reload(agency_runtime)
                importlib.reload(update_manager)
            except Exception:
                pass
            from update_manager import check_update, download_update, launch_updater
            verified_info = check_update()
            if not verified_info.get("available"):
                raise RuntimeError("Không có phiên bản mới để cài đặt.")

            def _on_progress(data):
                if self._main_window is not None:
                    js(self._main_window, "onUpdateProgress", data)

            installer = download_update(verified_info, progress=_on_progress)
            url_str = str(verified_info.get("url") or "")
            is_patch = url_str.lower().endswith(".zip") or str(verified_info.get("type") or "").lower() == "patch"
            if is_patch:
                return {"success": True, "is_patch": True, "version": verified_info.get("version")}
            else:
                launch_updater(installer, verified_info)
                if self._main_window is not None:
                    threading.Timer(0.5, self._main_window.destroy).start()
                return {"success": True, "is_patch": False, "version": verified_info.get("version")}
        except Exception as exc:
            return {"success": False, "error": str(exc)}

    def restart_app(self):
        try:
            exe = sys.executable
            if getattr(sys, "frozen", False):
                target_cmd = f'timeout /t 1 /nobreak >nul & start "" "{exe}"'
            else:
                target_cmd = f'timeout /t 1 /nobreak >nul & "{exe}" "{Path(__file__).resolve()}"'
            subprocess.Popen(f'cmd /c "{target_cmd}"', shell=True, cwd=str(APP_DIR))
            if self._main_window is not None:
                threading.Timer(0.3, self._main_window.destroy).start()
            return {"success": True}
        except Exception as exc:
            return {"success": False, "error": str(exc)}


def writable_log_dir():
    candidates = [
        logs_dir(),
        APP_DIR / "logs",
        Path(tempfile.gettempdir()) / "SLiveFace" / "logs",
    ]
    for directory in candidates:
        try:
            directory.mkdir(parents=True, exist_ok=True)
            probe = directory / ".write-test"
            probe.write_text("ok", encoding="utf-8")
            probe.unlink()
            return directory
        except Exception:
            continue
    return APP_DIR


def terminate_process(proc):
    if not proc or proc.poll() is not None:
        return
    try:
        if os.name == "nt":
            subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        else:
            proc.terminate()
            proc.wait(timeout=5)
    except Exception:
        try:
            proc.kill()
        except Exception:
            pass


import resource_manager
from resource_manager import ResourceManager, run_bootstrap

def kill_process_tree(proc):
    if not proc:
        return
    try:
        if os.name == "nt" and hasattr(proc, "pid"):
            subprocess.run(f"taskkill /F /T /PID {proc.pid}", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        elif hasattr(proc, "kill"):
            proc.kill()
    except Exception:
        pass


def kill_stale_app_processes():
    if agency_runtime.manifest():
        return  # Only ProcessSupervisor may stop this agency\'s child processes.
    if os.name == "nt":
        current_pid = os.getpid()
        try:
            for port in [8006, 8010]:
                out = subprocess.check_output(f"netstat -ano | findstr :{port}", shell=True, text=True, errors="ignore")
                for line in out.strip().splitlines():
                    parts = line.split()
                    if len(parts) >= 5 and parts[1].endswith(f":{port}"):
                        pid = parts[-1]
                        if pid.isdigit() and int(pid) != current_pid:
                            subprocess.run(f"taskkill /F /PID {pid}", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except Exception:
            pass

def cleanup():
    global cleanup_complete
    with cleanup_lock:
        if cleanup_complete:
            return
        cleanup_complete = True
        for proc in reversed(processes):
            kill_process_tree(proc)
        processes.clear()
        if process_supervisor is not None:
            try:
                process_supervisor.close()
            except Exception:
                pass
        kill_stale_app_processes()
        for handle in log_files:
            try:
                handle.close()
            except Exception:
                pass
        log_files.clear()


def js(window, function, *args):
    script = f"{function}({','.join(json.dumps(arg, ensure_ascii=False) for arg in args)})"
    try:
        window.evaluate_js(script)
    except Exception:
        pass


def start_process(flag, log_name):
    with cleanup_lock:
        if cleanup_complete:
            raise RuntimeError("Ứng dụng đang đóng, không khởi động thêm dịch vụ.")
        local = writable_log_dir()
        handle = (local / log_name).open("w", encoding="utf-8")
        log_files.append(handle)
        flags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        proc = subprocess.Popen(subprocess_command(flag), cwd=APP_DIR, env=os.environ.copy(),
                                stdout=handle, stderr=subprocess.STDOUT, creationflags=flags)
        processes.append(proc)
        if process_supervisor is not None:
            try:
                process_supervisor.add(proc)
            except Exception as exc:
                safe_print(f"Cannot attach process {proc.pid} to Windows Job: {exc}")
        return proc, local / log_name


def wait_for_service(proc, predicate, timeout, window, status, start_percent, end_percent, log_path):
    start_time = time.monotonic()
    deadline = start_time + timeout
    current_p = start_percent
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"Dịch vụ dừng đột ngột. Xem log: {log_path}")
        elapsed = time.monotonic() - start_time
        target_p = start_percent + int((elapsed / max(1, timeout)) * (end_percent - start_percent))
        current_p = min(end_percent - 1, max(current_p, target_p))
        js(window, "updateStatus", status, current_p)
        if predicate():
            js(window, "updateStatus", status, end_percent)
            time.sleep(0.2)
            return
        time.sleep(0.35)
        current_p = min(end_percent - 1, current_p + 1)
    raise RuntimeError(f"Quá thời gian khởi động. Xem log: {log_path}")


def initialize(window):
    try:
        # Chờ Edge WebView2 nạp xong giao diện để tránh xung đột luồng vẽ (Deadlock)
        js(window, "updateStatus", "Đang kiểm tra hệ thống...", 5)
        time.sleep(1.2)
        js(window, "updateStatus", "Đang kiểm tra tệp tin tài nguyên...", 8)
        validate_dependencies()
        if not resources_installed():
            if not port_available(str(CONFIG["host"]), CONFIG["port"]):
                raise RuntimeError(f"Port {CONFIG['port']} is already in use.")
            js(window, "updateStatus", "Đang mở trình cài tài nguyên...", 5)
            bootstrap_proc, bootstrap_log = start_process("--run-bootstrap", "resources.log")
            target = f"http://{CONFIG['host']}:{CONFIG['port']}/dashboard.html"
            login_url = f"http://{CONFIG['host']}:{CONFIG['port']}/login.html"
            wait_for_service(bootstrap_proc, lambda: web_health(login_url), 30, window,
                             "Đang kết nối trang đăng nhập...", 5, 10, bootstrap_log)
            window.load_url(target)
            bootstrap_proc.wait()
            if not resources_installed():
                raise RuntimeError(f"Cài tài nguyên chưa hoàn tất. Xem log: {bootstrap_log}")
            js(window, "updateStatus", "Tài nguyên đã sẵn sàng. Đang khởi động AI...", 10)

        validate_resources()
        if not port_available(str(CONFIG["tts_host"]), CONFIG["tts_port"]):
            raise RuntimeError(f"Cổng TTS {CONFIG['tts_port']} đang được chương trình khác sử dụng.")
        if not port_available(str(CONFIG["host"]), CONFIG["port"]):
            raise RuntimeError(f"Cổng WebRTC {CONFIG['port']} đang được chương trình khác sử dụng.")

        js(window, "updateStatus", "Đang nạp mô hình giọng nói...", 12)
        tts_proc, tts_log = start_process("--run-tts", "tts.log")
        wait_for_service(tts_proc, lambda: json_health(CONFIG["tts_server"] + "/"),
                         CONFIG["tts_startup_timeout_seconds"], window,
                         "Đang nạp mô hình giọng nói (lần đầu có thể mất vài phút)...", 12, 58, tts_log)

        js(window, "updateStatus", "Giọng nói đã sẵn sàng. Đang nạp mô hình hình ảnh và avatar...", 62)
        engine_proc, engine_log = start_process("--run-engine", "engine.log")
        target = f"http://{CONFIG['host']}:{CONFIG['port']}/dashboard.html"
        public_health = f"http://{CONFIG['host']}:{CONFIG['port']}/login.html"
        wait_for_service(engine_proc, lambda: web_health(public_health), 180, window,
                         "Đang nạp mô hình hình ảnh và avatar...", 62, 96, engine_log)
        js(window, "updateStatus", "Khởi động hoàn tất", 100)
        time.sleep(0.35)
        window.load_url(target)
    except Exception as exc:
        safe_print(exc)
        cleanup()
        js(window, "showError", str(exc))


def main():
    global process_supervisor
    multiprocessing.freeze_support()
    if dispatch_subprocess():
        return
    kill_stale_app_processes()
    if agency_runtime.manifest():
        import socket
        for port in (CONFIG["port"], CONFIG["tts_port"]):
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
                try:
                    probe.bind(("127.0.0.1", int(port)))
                except OSError as error:
                    raise RuntimeError("Cổng ứng dụng đang được sử dụng. Hãy đóng phiên app cũ trước.") from error
    process_supervisor = ProcessSupervisor()
    if os.name == "nt":
        try:
            import ctypes
            slug = agency_runtime.agency_slug() or "slive_vip"
            ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(f"com.sliveai.{slug}.app")
        except Exception:
            pass
    try:
        import webview
        desktop_api = DesktopApi()
        loading = agency_runtime.render_loading_html((APP_DIR / "web" / "loading.html").read_text(encoding="utf-8"))
        window = webview.create_window(agency_runtime.display_name(), html=loading, width=1440, height=900,
                                       min_size=(1050, 700), resizable=True, js_api=desktop_api)
        desktop_api._main_window = window

        def on_closing():
            try:
                import urllib.request
                req = urllib.request.Request(f"http://{CONFIG['host']}:{CONFIG['port']}/api/resources/status")
                with urllib.request.urlopen(req, timeout=1.2) as resp:
                    data = json.loads(resp.read().decode('utf-8'))
                    if data.get("progress", {}).get("running"):
                        if os.name == "nt":
                            import ctypes
                            MB_YESNO = 0x00000004
                            MB_ICONWARNING = 0x00000030
                            IDYES = 6
                            res = ctypes.windll.user32.MessageBoxW(
                                0,
                                f"Hệ thống đang tải tài nguyên {agency_runtime.display_name()}.\n\nNếu đóng ứng dụng lúc này, việc tải sẽ bị gián đoạn và bạn sẽ phải tải lại.\n\nBạn có chắc chắn muốn đóng ứng dụng không?",
                                "Cảnh báo - " + agency_runtime.display_name(),
                                MB_YESNO | MB_ICONWARNING
                            )
                            if res != IDYES:
                                return False
            except Exception:
                pass
            return True

        window.events.closing += on_closing
        window.events.closed += cleanup
        icon = agency_runtime.window_icon_path() if agency_runtime.manifest() else agency_runtime.root_dir() / "icon.ico"
        webview.start(initialize, window, debug=False, icon=str(icon) if icon and icon.is_file() else None, **({"storage_path": str(agency_runtime.profile_dir() / "webview"), "private_mode": False} if agency_runtime.manifest() else {}))
    except Exception as exc:
        print(f"Webview startup failed, using browser fallback: {exc}", flush=True)
        try:
            import webbrowser
            webbrowser.open(f"http://{CONFIG['host']}:{CONFIG['port']}/login.html")
            return
        except Exception:
            pass
    finally:
        cleanup()


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        try:
            (writable_log_dir() / "launcher-crash.log").write_text(
                traceback.format_exc(), encoding="utf-8")
        except Exception:
            pass
        try:
            import webbrowser
            webbrowser.open(f"http://{CONFIG['host']}:{CONFIG['port']}/login.html")
        except Exception:
            pass
