"""Private Concat loopback adapter. Reuses avatar_prepare; never calls Promax.

The native parent supplies a per-launch credential over stdin, not argv/logs.
Only the local desktop owns this service; browsers and remote hosts are rejected.
"""
from __future__ import annotations

import hmac
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import threading
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit
from uuid import uuid4

import avatar_prepare

ROUTE = "/api/v1/concat/avatars"  # Preserve the already-tested native wire format.
UUID = re.compile(r"[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}\Z")
MAX_VIDEO = 25 * 1024 * 1024


def native_path(value):
    # Keep Rust's extended Win32 paths interoperable with Python subprocesses.
    value = str(value)
    return value[4:] if os.name == "nt" and value.startswith("\\\\?\\") else value


def terminate(child):
    if child.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run([str(Path(os.environ.get("SystemRoot", "C:/Windows")) / "System32/taskkill.exe"),
                        "/PID", str(child.pid), "/T", "/F"], stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=10, creationflags=subprocess.CREATE_NO_WINDOW)
    else:
        import signal
        os.killpg(child.pid, signal.SIGKILL)
    child.wait(timeout=10)


class Library:
    def __init__(self, root):
        self.root = Path(root).resolve()
        self.root.mkdir(parents=True, exist_ok=True)
        self.lock = threading.Lock()
        self.active = None
        self.cached_readiness = None
        self.children = set()
        self.children_lock = threading.Lock()
        self.idle = threading.Event()
        self.idle.set()
        for job in self.jobs():
            if job["status"] == "processing":
                job.update(status="failed", error_code="PROCESS_INTERRUPTED")
                (self.root / (job["id"] + ".upload")).unlink(missing_ok=True)
                partial = self.root / job["id"]
                if partial.is_dir():
                    shutil.rmtree(partial)
                self.save(job)

    def jobs(self):
        rows = []
        for path in self.root.glob("*.json"):
            if UUID.fullmatch(path.stem):
                row = json.loads(path.read_text(encoding="utf-8"))
                if row.get("id") != path.stem:
                    raise ValueError("INVALID_JOB")
                rows.append(row)
        return rows

    def save(self, job):
        path = self.root / (job["id"] + ".json")
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps(job, ensure_ascii=False), encoding="utf-8")
        temporary.replace(path)

    def archive(self, identity):
        """Caller holds lock. Move only owned job paths; never delete original media."""
        if not UUID.fullmatch(identity):
            raise ValueError("INVALID_JOB")
        job = next((row for row in self.jobs() if row["id"] == identity), None)
        if job is None:
            return False
        if job["status"] == "processing":
            raise ValueError("JOB_BUSY")
        sources = [(self.root / identity, "media"),
                   (self.root / (identity + ".upload"), "upload"),
                   (self.root / (identity + ".json"), "record.json")]
        trash = self.root / ".trash"
        # Reject symlinks/junctions before moving anything outside this library.
        if trash.resolve() != trash or any(path.resolve() != path for path, _ in sources):
            raise ValueError("UNSAFE_JOB_PATH")
        trash.mkdir(exist_ok=True)
        destination = trash / identity
        destination.mkdir()  # Never overwrite an existing archived copy.
        moved = []
        try:
            for source, name in sources:
                if source.exists():
                    target = destination / name
                    source.rename(target)
                    moved.append((source, target))
        except Exception:
            for source, target in reversed(moved):
                target.rename(source)
            destination.rmdir()
            raise
        return True

    def readiness(self):
        if self.cached_readiness is None or not self.cached_readiness["ready"]:
            # Imports/model check stay in a short-lived process, not the HTTP thread.
            options = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {"start_new_session": True}
            child = subprocess.Popen([native_path(sys.executable), "-B", native_path(Path(__file__).with_name("avatar_prepare.py")), "--check"],
                                     # Never inherit the parent-watchdog stdin pipe.
                                     stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, **options)
            with self.children_lock:
                self.children.add(child)
            try:
                raw, _ = child.communicate(timeout=15)
                if child.returncode or len(raw) > 16384:
                    raise ValueError("RUNTIME_CHECK_FAILED")
                self.cached_readiness = json.loads(raw)
            except Exception as error:
                failure = {"check_returncode": child.poll(), "check_error": type(error).__name__}
                terminate(child)
                self.cached_readiness = {"ready": False, "missing": ["python_runtime"], "engine": "slive-1.0.18",
                                         "capabilities": {"face_preparation": False, "source_motion": False,
                                                          "realtime_lipsync": False, "voice_clone": False}}
                self.cached_readiness.update(failure)
            finally:
                with self.children_lock:
                    self.children.discard(child)
            # Safe diagnostic receipt: booleans/missing component names only.
            (self.root / "_runtime-status.json").write_text(json.dumps(self.cached_readiness), encoding="utf-8")
        return self.cached_readiness

    def close(self):
        with self.children_lock:
            children = list(self.children)
        for child in children:
            try:
                terminate(child)
            except (OSError, subprocess.SubprocessError):
                pass
        self.idle.wait(timeout=15)

    def prepare(self, job, source):
        output = self.root / job["id"]
        try:
            options = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {"start_new_session": True}
            child = subprocess.Popen([native_path(sys.executable), "-B", native_path(Path(__file__).with_name("avatar_prepare.py")),
                                      "--source", str(source), "--output", str(output)],
                                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **options)
            with self.children_lock:
                self.children.add(child)
            try:
                if child.wait(timeout=600):
                    raise ValueError("PREPARATION_FAILED")
            finally:
                terminate(child)
                with self.children_lock:
                    self.children.discard(child)
            result = json.loads((output / "result.json").read_text(encoding="utf-8"))
            frames, seconds = result["frames"], result["duration_seconds"]
            if not 2 <= frames <= 750 or not 1 <= seconds <= 30:
                raise ValueError("INVALID_PREPARATION")
            job.update(status="ready", frames=frames, duration_seconds=seconds,
                       source_video_url=f"{ROUTE}/{job['id']}/source-video", preview_url=f"{ROUTE}/{job['id']}/preview")
            job["capabilities"].update(source_motion=True, face_prepared=True)
        except Exception:
            # UUID is generated here, never a caller-provided filesystem path.
            if output.is_dir():
                shutil.rmtree(output)
            job.update(status="failed", error_code="PREPARATION_FAILED")
        finally:
            try:
                self.save(job)
            finally:
                try:
                    source.unlink(missing_ok=True)
                finally:
                    self.active = None
                    self.lock.release()
                    self.idle.set()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass  # No personal filenames, credentials or raw engine diagnostics.

    def reply(self, status, value, mime="application/json"):
        body = json.dumps(value, ensure_ascii=False).encode() if mime == "application/json" else value
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.end_headers()
        self.wfile.write(body)

    def handle_request(self):
        self.connection.settimeout(30)
        expected_host = f"127.0.0.1:{self.server.server_port}"
        if (self.headers.get("Origin") is not None or self.headers.get("Host") != expected_host
                or not hmac.compare_digest(self.headers.get("Authorization", ""), "Bearer " + self.server.token)):
            return self.reply(403, {"ok": False})
        parsed = urlsplit(self.path)
        path = parsed.path
        library = self.server.library
        if self.command == "GET" and path == ROUTE + "/capabilities":
            return self.reply(200, {"ok": True, "data": library.readiness()})
        if self.command == "GET" and path == ROUTE:
            return self.reply(200, {"ok": True, "data": library.jobs()})
        tail = path.removeprefix(ROUTE + "/").split("/") if path.startswith(ROUTE + "/") else []
        if self.command == "DELETE" and len(tail) == 1 and UUID.fullmatch(tail[0]):
            if not library.lock.acquire(blocking=False):
                return self.reply(400, {"ok": False})
            try:
                if not library.archive(tail[0]):
                    return self.reply(404, {"ok": False})
                return self.reply(200, {"ok": True, "data": {"id": tail[0], "deleted": True, "archived": True}})
            finally:
                library.lock.release()
        if self.command == "GET" and len(tail) in (1, 2) and UUID.fullmatch(tail[0]):
            job = next((row for row in library.jobs() if row["id"] == tail[0]), None)
            if job is not None and len(tail) == 1:
                return self.reply(200, {"ok": True, "data": job})
            if job is not None and job["status"] == "ready" and tail[1] in ("source-video", "preview"):
                filename, mime = ("source.mp4", "video/mp4") if tail[1] == "source-video" else ("preview.png", "image/png")
                media = library.root / job["id"] / filename
                if media.stat().st_size <= MAX_VIDEO:
                    return self.reply(200, media.read_bytes(), mime)
        if self.command == "POST" and path == ROUTE + "/import-video":
            query = parse_qs(parsed.query, strict_parsing=True)
            name = query.get("name", [""])[0].strip()
            mime = self.headers.get("Content-Type")
            if (set(query) != {"name"} or len(query["name"]) != 1 or not 1 <= len(name) <= 90
                    or any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in name)
                    or mime not in ("video/mp4", "video/webm") or self.headers.get("Transfer-Encoding")):
                return self.reply(400, {"ok": False})
            size = int(self.headers.get("Content-Length", "0"))
            if not 1 <= size <= MAX_VIDEO:
                return self.reply(413, {"ok": False})
            if not library.lock.acquire(blocking=False):
                return self.reply(503, {"ok": False})
            accepted = False
            source = None
            try:
                jobs = library.jobs()
                if len(jobs) >= 100 or sum(row["status"] != "failed" for row in jobs) >= 5:
                    return self.reply(400, {"ok": False})
                if not library.readiness()["ready"]:
                    return self.reply(503, {"ok": False})
                data = self.rfile.read(size)
                valid = len(data) >= 12 and data[4:8] == b"ftyp" if mime == "video/mp4" else data[:4] == b"\x1aE\xdf\xa3"
                if len(data) != size or not valid:
                    return self.reply(400, {"ok": False})
                identity = str(uuid4())
                source = library.root / (identity + ".upload")
                source.write_bytes(data)
                job = dict(id=identity, name=name, status="processing", created_at=datetime.now(timezone.utc).isoformat(),
                           error_code=None, frames=0, duration_seconds=0, source_video_url=None, preview_url=None,
                           capabilities=dict(source_motion=False, face_prepared=False, realtime_lipsync=False, voice_clone=False))
                library.save(job)
                library.active = identity
                library.idle.clear()
                receipt = dict(job, capabilities=dict(job["capabilities"]))
                threading.Thread(target=library.prepare, args=(job, source), daemon=True).start()
                accepted = True
                return self.reply(202, {"ok": True, "data": receipt})
            finally:
                if not accepted:
                    if source is not None:
                        source.unlink(missing_ok=True)
                    library.lock.release()
        return self.reply(404, {"ok": False})

    def dispatch(self):
        try:
            self.handle_request()
        except (BrokenPipeError, ConnectionError):
            pass
        except Exception:
            self.reply(400, {"ok": False})

    do_GET = dispatch
    do_POST = dispatch
    do_DELETE = dispatch


def main():
    config = json.loads(sys.stdin.readline(16384))
    token = config["token"]
    if not re.fullmatch(r"[a-f0-9]{64}", token):
        raise ValueError("INVALID_SESSION")
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    server.token = token
    server.library = Library(config["data"])
    endpoint = Path(config["receipt"])
    temporary = endpoint.with_suffix(".tmp")
    temporary.write_text(json.dumps({"port": server.server_port}), encoding="utf-8")
    temporary.replace(endpoint)
    # Parent pipe closes on normal shutdown or crash, even on non-Windows hosts.
    def parent_closed():
        sys.stdin.read()
        server.library.close()
        os._exit(0)
    threading.Thread(target=parent_closed, daemon=True).start()
    server.serve_forever(poll_interval=0.2)


if __name__ == "__main__":
    main()
