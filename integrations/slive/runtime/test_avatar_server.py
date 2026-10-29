import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import unittest
from urllib.error import HTTPError
from urllib.request import Request, urlopen
from unittest.mock import patch

from avatar_server import Handler, Library, ROUTE, ThreadingHTTPServer


class LocalAvatarTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory(prefix="concat-local-test-")
        self.library = Library(self.folder.name)
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.library = self.library
        self.server.token = "a" * 64
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}{ROUTE}"

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.library.close()
        self.folder.cleanup()

    def request(self, suffix="", data=None, **headers):
        request = Request(self.url + suffix, data=data,
                          headers={"Authorization": "Bearer " + self.server.token, **headers})
        return urlopen(request, timeout=30)

    def test_auth_origin_host_and_traversal_fail_closed(self):
        for headers in ({"Authorization": "wrong"}, {"Origin": "https://example.invalid"}, {"Host": "example.invalid"}):
            with self.subTest(headers=headers), self.assertRaises(HTTPError) as result:
                self.request(**headers)
            self.assertEqual(result.exception.code, 403)
        with self.assertRaises(HTTPError) as result:
            self.request("/../avatar_server.py")
        self.assertEqual(result.exception.code, 404)

    def test_readiness_and_empty_library(self):
        with self.request() as response:
            self.assertEqual(json.load(response), {"ok": True, "data": []})
        with patch.object(self.library, "readiness", return_value={"ready": False, "missing": ["torch"]}):
            with self.request("/capabilities") as response:
                self.assertFalse(json.load(response)["data"]["ready"])

    def test_invalid_uploads_do_not_create_jobs(self):
        self.library.cached_readiness = {"ready": True, "missing": []}
        for suffix, body, mime in [("?name=MC", b"invalid", "video/mp4"),
                                   ("?name=MC&path=elsewhere", b"0000ftyp", "video/mp4"),
                                   ("?name=MC", b"0000ftyp", "text/plain")]:
            with self.assertRaises(HTTPError) as result:
                self.request("/import-video" + suffix, body, **{"Content-Type": mime})
            self.assertEqual(result.exception.code, 400)
        self.assertEqual(self.library.jobs(), [])
        self.assertFalse(self.library.lock.locked())

    def test_busy_upload_is_not_repeated(self):
        self.library.lock.acquire()
        try:
            with self.assertRaises(HTTPError) as result:
                self.request("/import-video?name=MC", b"0000ftyp", **{"Content-Type": "video/mp4"})
            self.assertEqual(result.exception.code, 503)
        finally:
            self.library.lock.release()

    def test_interrupted_jobs_are_failed_on_restart(self):
        job = {"id": "d21a4803-1415-4621-96d0-64555c5a040d", "status": "processing"}
        self.library.save(job)
        upload = Path(self.folder.name) / (job["id"] + ".upload")
        upload.write_bytes(b"interrupted copy, not the original video")
        restarted = Library(self.folder.name)
        self.assertEqual(restarted.jobs()[0]["error_code"], "PROCESS_INTERRUPTED")
        self.assertFalse(upload.exists())

    def test_delete_is_scoped_to_validated_job_and_not_while_busy(self):
        identity = "d21a4803-1415-4621-96d0-64555c5a040d"
        self.library.save({"id": identity, "status": "failed"})
        request = Request(self.url + "/" + identity, method="DELETE",
                          headers={"Authorization": "Bearer " + self.server.token})
        self.library.lock.acquire()
        try:
            with self.assertRaises(HTTPError) as result:
                urlopen(request, timeout=5)
            self.assertEqual(result.exception.code, 400)
        finally:
            self.library.lock.release()
        with urlopen(request, timeout=5) as response:
            self.assertTrue(json.load(response)["data"]["deleted"])
        self.assertEqual(self.library.jobs(), [])

    def test_delete_archives_only_selected_duplicate_and_preserves_original(self):
        identity = "d21a4803-1415-4621-96d0-64555c5a040d"
        other = "d21a4803-1415-4621-96d0-64555c5a040e"
        for key in (identity, other):
            self.library.save({"id": key, "name": "same name", "status": "ready"})
            folder = self.library.root / key
            folder.mkdir()
            (folder / "source.mp4").write_bytes(b"copied video")
        original = Path(self.folder.name) / "user-original.mp4"
        original.write_bytes(b"original outside managed job")
        request = Request(self.url + "/" + identity, method="DELETE",
                          headers={"Authorization": "Bearer " + self.server.token})
        with urlopen(request, timeout=5) as response:
            receipt = json.load(response)["data"]
        self.assertTrue(receipt["archived"])
        self.assertEqual([row["id"] for row in self.library.jobs()], [other])
        self.assertEqual(original.read_bytes(), b"original outside managed job")
        self.assertEqual((self.library.root / other / "source.mp4").read_bytes(), b"copied video")
        archive = self.library.root / ".trash" / identity
        self.assertEqual(json.loads((archive / "record.json").read_text())["id"], identity)
        self.assertEqual((archive / "media/source.mp4").read_bytes(), b"copied video")
        restarted = Library(self.folder.name)
        self.assertEqual([row["id"] for row in restarted.jobs()], [other])
        with self.assertRaises(HTTPError) as result:
            urlopen(request, timeout=5)
        self.assertEqual(result.exception.code, 404)

    def test_delete_rejects_browser_origin(self):
        identity = "d21a4803-1415-4621-96d0-64555c5a040d"
        self.library.save({"id": identity, "status": "failed"})
        request = Request(self.url + "/" + identity, method="DELETE",
                          headers={"Authorization": "Bearer " + self.server.token, "Origin": "https://example.invalid"})
        with self.assertRaises(HTTPError) as result:
            urlopen(request, timeout=5)
        self.assertEqual(result.exception.code, 403)
        self.assertEqual(len(self.library.jobs()), 1)

    def test_archive_failure_restores_job_and_copied_video(self):
        identity = "d21a4803-1415-4621-96d0-64555c5a040d"
        self.library.save({"id": identity, "status": "ready"})
        folder = self.library.root / identity
        folder.mkdir()
        (folder / "source.mp4").write_bytes(b"fixture")
        rename = Path.rename
        def fail_record(source, target):
            if source == self.library.root / (identity + ".json"):
                raise OSError("fixture failure")
            return rename(source, target)
        with patch.object(Path, "rename", fail_record), self.assertRaises(OSError):
            self.library.archive(identity)
        self.assertEqual(self.library.jobs()[0]["id"], identity)
        self.assertEqual((folder / "source.mp4").read_bytes(), b"fixture")
        self.assertFalse((self.library.root / ".trash" / identity).exists())

    def test_worker_releases_slot_even_when_result_cannot_be_saved(self):
        identity = "d21a4803-1415-4621-96d0-64555c5a040d"
        source = Path(self.folder.name) / (identity + ".upload")
        source.write_bytes(b"fixture")
        self.library.lock.acquire()
        self.library.idle.clear()
        with patch("avatar_server.subprocess.Popen", side_effect=OSError("fixture")), \
                patch.object(self.library, "save", side_effect=OSError("fixture")):
            with self.assertRaises(OSError):
                self.library.prepare({"id": identity}, source)
        self.assertFalse(source.exists())
        self.assertFalse(self.library.lock.locked())
        self.assertTrue(self.library.idle.is_set())

    @unittest.skipUnless(os.environ.get("CONCAT_LOCAL_AVATAR_REAL_TEST"), "explicit real-engine opt-in")
    def test_real_preparation_without_promax(self):
        root = Path(__file__).resolve().parents[3]
        sample = root / "src/crates/concat/assets/avatars/newscaster.mp4"
        video = Path(self.folder.name) / "fixture.mp4"
        subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-i", str(sample), "-t", "2",
                        "-an", "-c:v", "libx264", str(video)], check=True, timeout=30)
        with self.request("/import-video?name=Local%20smoke", video.read_bytes(), **{"Content-Type": "video/mp4"}) as response:
            self.assertEqual(response.status, 202)
            identity = json.load(response)["data"]["id"]
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline:
            with self.request("/" + identity) as response:
                job = json.load(response)["data"]
            if job["status"] != "processing":
                break
            time.sleep(0.5)
        self.assertEqual(job["status"], "ready")
        self.assertEqual(job["frames"], 50)
        with self.request("/" + identity + "/source-video") as response:
            self.assertEqual(response.read()[4:8], b"ftyp")
        with self.request("/" + identity + "/preview") as response:
            self.assertEqual(response.read()[:8], b"\x89PNG\r\n\x1a\n")


if __name__ == "__main__":
    unittest.main()
