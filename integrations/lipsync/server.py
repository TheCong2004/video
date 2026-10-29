"""Concat private GPU endpoint. Buffered utterances, not a WebRTC stream.

Bind loopback behind an SSH tunnel/TLS proxy. One owner, one GPU job at a time.
No URLs, arbitrary paths, browser requests, automatic downloads or retry queue.
"""
from __future__ import annotations

import base64
import binascii
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading

MAX_VIDEO = 25 * 1024 * 1024
MAX_AUDIO = 5 * 1024 * 1024
MAX_BODY = 41 * 1024 * 1024
MAX_RESULT = 60 * 1024 * 1024
TYPES = {"audio/wav": ".wav", "audio/x-wav": ".wav", "audio/mpeg": ".mp3",
         "audio/mp3": ".mp3", "audio/ogg": ".ogg", "audio/flac": ".flac",
         "audio/aac": ".aac", "audio/mp4": ".m4a"}


def decode_media(value, maximum):
    if not isinstance(value, str) or len(value) > (maximum + 2) // 3 * 4:
        raise ValueError("INPUT_INVALID")
    try:
        decoded = base64.b64decode(value, validate=True)
    except (ValueError, binascii.Error):
        raise ValueError("INPUT_INVALID") from None
    if not decoded or len(decoded) > maximum:
        raise ValueError("INPUT_INVALID")
    return decoded


class Service:
    def __init__(self, root, token, engine, motion=None):
        if not isinstance(token, str) or not 32 <= len(token) <= 256 or not token.isascii() or not token.isalnum():
            raise ValueError("Use a random alphanumeric token of 32-256 characters")
        self.root = Path(root).resolve()
        self.root.mkdir(parents=True, exist_ok=True)
        self.token = token
        self.engine = engine
        self.motion = motion
        self.busy = threading.Lock()


def make_server(service, address):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass  # Never log credentials, prompts, paths or media.

        def reply(self, status, value):
            # Drain bounded rejected uploads before closing HTTP/1.0. Otherwise
            # Windows can reset the socket and discard the 4xx response itself.
            try:
                length = int(self.headers.get("Content-Length", "0"))
            except ValueError:
                length = 0
            if 0 < length <= MAX_BODY and not getattr(self, "body_consumed", False):
                remaining = length
                while remaining:
                    chunk = self.rfile.read(min(65536, remaining))
                    if not chunk:
                        break
                    remaining -= len(chunk)
                self.body_consumed = True
            body = json.dumps(value).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def authorized(self):
            self.connection.settimeout(30)
            if self.headers.get("Origin") or self.headers.get("Sec-Fetch-Site"):
                self.reply(403, {"error": "BROWSER_REJECTED"})
                return False
            if not hmac.compare_digest(self.headers.get("Authorization", "").encode("utf-8"), ("Bearer " + service.token).encode("ascii")):
                self.reply(401, {"error": "AUTH_REQUIRED"})
                return False
            return True

        def do_GET(self):
            if not self.authorized():
                return
            if self.path == "/v1/motion/health":
                ready = service.motion is not None
                return self.reply(200 if ready else 503, {"version": 1, "ready": ready,
                    "model": "echomimic-v3-flash", "mode": "offline-gesture",
                    "min_seconds": 2, "max_seconds": 10, "busy": service.busy.locked()})
            if self.path != "/v1/lipsync/health":
                return self.reply(404, {"error": "NOT_FOUND"})
            ready = service.engine is not None
            self.reply(200 if ready else 503, {"version": 1, "ready": ready, "model": "musetalk-1.5",
                       "mode": "buffered-utterance", "fps": 25, "face_resolution": 256,
                       "max_source_seconds": 10, "max_audio_seconds": 15, "busy": service.busy.locked()})

        def do_POST(self):
            if not self.authorized():
                return
            if self.path == "/v1/motion/render":
                return self.motion_render()
            if self.path != "/v1/lipsync/render":
                return self.reply(404, {"error": "NOT_FOUND"})
            if service.engine is None:
                return self.reply(503, {"error": "MODEL_UNAVAILABLE"})
            if not service.busy.acquire(blocking=False):
                return self.reply(409, {"error": "GPU_BUSY"})
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if self.headers.get("Transfer-Encoding") or not 0 < length <= MAX_BODY:
                    return self.reply(413, {"error": "BODY_LIMIT"})
                if self.headers.get("Content-Type") != "application/json":
                    return self.reply(415, {"error": "MEDIA_TYPE"})
                raw = self.rfile.read(length)
                self.body_consumed = True
                if len(raw) != length:
                    raise ValueError("INPUT_INVALID")
                payload = json.loads(raw)
                if not isinstance(payload, dict) or set(payload) != {"video", "audio", "audio_type"}:
                    raise ValueError("INPUT_INVALID")
                video = decode_media(payload["video"], MAX_VIDEO)
                audio = decode_media(payload["audio"], MAX_AUDIO)
                suffix = TYPES.get(payload["audio_type"])
                if suffix is None or video[4:8] != b"ftyp":
                    raise ValueError("INPUT_INVALID")
                if suffix == ".wav" and not (audio.startswith(b"RIFF") and audio[8:12] == b"WAVE"):
                    raise ValueError("INPUT_INVALID")
                with tempfile.TemporaryDirectory(prefix="reply-", dir=service.root) as temporary:
                    work = Path(temporary)
                    source, speech = work / "source.mp4", work / ("speech" + suffix)
                    source.write_bytes(video)
                    speech.write_bytes(audio)
                    result, metrics = service.engine.render(source, speech, work)
                    if not 12 <= len(result) <= MAX_RESULT or result[4:8] != b"ftyp":
                        raise RuntimeError("INVALID_RENDER")
                self.send_response(200)
                self.send_header("Content-Type", "video/mp4")
                self.send_header("Content-Length", str(len(result)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("X-Render-Seconds", str(round(metrics["render_seconds"], 3)))
                self.send_header("X-Audio-Seconds", str(round(metrics["audio_seconds"], 3)))
                self.end_headers()
                self.wfile.write(result)
            except (ValueError, TypeError, KeyError):
                self.reply(400, {"error": "INPUT_INVALID"})
            except (BrokenPipeError, ConnectionError, TimeoutError):
                pass
            except Exception as error:
                print("LIPSYNC_RENDER_FAILED", type(error).__name__, flush=True)
                self.reply(503, {"error": "RENDER_FAILED"})
            finally:
                service.busy.release()

        def motion_render(self):
            if service.motion is None:
                return self.reply(503, {"error": "MODEL_UNAVAILABLE"})
            if not service.busy.acquire(blocking=False):
                return self.reply(409, {"error": "GPU_BUSY"})
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if self.headers.get("Transfer-Encoding") or not 0 < length <= 13 * 1024 * 1024:
                    return self.reply(413, {"error": "BODY_LIMIT"})
                if self.headers.get("Content-Type") != "application/json":
                    return self.reply(415, {"error": "MEDIA_TYPE"})
                raw = self.rfile.read(length)
                self.body_consumed = True
                if len(raw) != length:
                    raise ValueError("INPUT_INVALID")
                data = json.loads(raw)
                if not isinstance(data, dict) or set(data) != {"image", "audio", "audio_type", "role", "seconds", "consent"}:
                    raise ValueError("INPUT_INVALID")
                if data["consent"] is not True or type(data["role"]) is not int or not 0 <= data["role"] <= 2:
                    raise ValueError("INPUT_INVALID")
                if type(data["seconds"]) is not int or not 2 <= data["seconds"] <= 10:
                    raise ValueError("INPUT_INVALID")
                image = decode_media(data["image"], 4 * 1024 * 1024)
                audio = decode_media(data["audio"], MAX_AUDIO)
                suffix = TYPES.get(data["audio_type"])
                if not image.startswith(b"\x89PNG\r\n\x1a\n") or suffix is None:
                    raise ValueError("INPUT_INVALID")
                with tempfile.TemporaryDirectory(prefix="motion-", dir=service.root) as temporary:
                    work = Path(temporary)
                    photo, speech = work / "image.png", work / ("speech" + suffix)
                    photo.write_bytes(image)
                    speech.write_bytes(audio)
                    result = service.motion.render(photo, speech, data["role"], data["seconds"], work)
                    if not 12 <= len(result) <= MAX_VIDEO or result[4:8] != b"ftyp":
                        raise RuntimeError("INVALID_RENDER")
                self.send_response(200)
                self.send_header("Content-Type", "video/mp4")
                self.send_header("Content-Length", str(len(result)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.write(result)
            except (ValueError, TypeError, KeyError):
                self.reply(400, {"error": "INPUT_INVALID"})
            except (BrokenPipeError, ConnectionError, TimeoutError):
                pass
            except Exception as error:
                print("MOTION_RENDER_FAILED", type(error).__name__, flush=True)
                self.reply(503, {"error": "RENDER_FAILED"})
            finally:
                service.busy.release()

    class BoundedServer(ThreadingHTTPServer):
        daemon_threads = True
        request_queue_size = 4
        slots = threading.BoundedSemaphore(6)

        def process_request(self, request, address):
            if not self.slots.acquire(blocking=False):
                self.shutdown_request(request)
                return
            try:
                super().process_request(request, address)
            except Exception:
                self.slots.release()
                raise

        def process_request_thread(self, request, address):
            try:
                super().process_request_thread(request, address)
            finally:
                self.slots.release()

    return BoundedServer(address, Handler)


if __name__ == "__main__":
    # The model environment is operator-installed; missing weights fail startup.
    motion = None
    engine = None
    if os.environ.get("CONCAT_GPU_MODE") == "motion":
        import sys
        sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "motion"))
        from engine import EchoEngine
        motion = EchoEngine(os.environ["CONCAT_ECHO_ROOT"], os.environ["CONCAT_ECHO_MODELS"])
    else:
        from musetalk_engine import MuseTalkEngine
        engine = MuseTalkEngine(Path(os.environ["CONCAT_MUSETALK_ROOT"]))
    app = Service(Path(os.environ.get("CONCAT_LIPSYNC_DATA", "./lipsync-data")),
                  os.environ["CONCAT_LIPSYNC_TOKEN"], engine, motion=motion)
    http = make_server(app, ("127.0.0.1", int(os.environ.get("CONCAT_LIPSYNC_PORT", "8766"))))
    print("MOTION_RUNTIME_READY model=echomimic-v3-flash" if motion else "LIPSYNC_READY model=musetalk-1.5 mode=buffered-utterance", flush=True)
    http.serve_forever()
