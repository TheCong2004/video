"""Offline contract/security tests: never load a model or contact a provider."""
import base64
import http.client
import json
from pathlib import Path
import tempfile
import threading
import unittest

import server


class FakeEngine:
    def render(self, video, audio, work):
        assert video.read_bytes()[4:8] == b"ftyp"
        assert audio.read_bytes().startswith(b"RIFF")
        return b"\0\0\0\x18ftypisomfixture", {"render_seconds": 0.1, "audio_seconds": 1.0}


class FakeMotion:
    def render(self, image, audio, role, seconds, work):
        assert image.read_bytes().startswith(b"\x89PNG")
        assert role == 1 and seconds == 7
        return b"\0\0\0\x18ftypisomfixture"


class ContractTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.app = server.Service(Path(self.temp.name), "t" * 48, FakeEngine())
        self.http = server.make_server(self.app, ("127.0.0.1", 0))
        self.thread = threading.Thread(target=self.http.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.http.shutdown()
        self.http.server_close()
        self.thread.join()
        self.temp.cleanup()

    def request(self, method, route, body=None, **headers):
        conn = http.client.HTTPConnection(*self.http.server_address, timeout=2)
        conn.request(method, route, body=body, headers=headers)
        response = conn.getresponse()
        result = response.status, response.read()
        conn.close()
        return result

    def payload(self):
        return json.dumps({"video": base64.b64encode(b"\0\0\0\x18ftypisomfixture").decode(),
                           "audio": base64.b64encode(b"RIFF\0\0\0\0WAVEfixture").decode(),
                           "audio_type": "audio/wav"})

    def test_auth_origin_and_unready_fail_closed(self):
        self.assertEqual(self.request("GET", "/v1/lipsync/health")[0], 401)
        self.assertEqual(self.request("GET", "/v1/lipsync/health", Authorization="Bearer " + "t" * 48,
                                      Origin="https://example.invalid")[0], 403)
        self.app.engine = None
        status, body = self.request("GET", "/v1/lipsync/health", Authorization="Bearer " + "t" * 48)
        self.assertEqual(status, 503)
        self.assertFalse(json.loads(body)["ready"])

    def test_only_exact_authenticated_route_and_valid_media(self):
        auth = {"Authorization": "Bearer " + "t" * 48, "Content-Type": "application/json"}
        self.assertEqual(self.request("POST", "/v1/lipsync/render", "{}", **auth)[0], 400)
        self.assertEqual(self.request("POST", "/v1/lipsync/render?url=http://evil", self.payload(), **auth)[0], 404)
        payload = json.loads(self.payload())
        payload["audio"] = "not-base64"
        self.assertEqual(self.request("POST", "/v1/lipsync/render", json.dumps(payload), **auth)[0], 400)
        status, output = self.request("POST", "/v1/lipsync/render", self.payload(), **auth)
        self.assertEqual(status, 200)
        self.assertEqual(output[4:8], b"ftyp")
        self.assertEqual(list(Path(self.temp.name).iterdir()), [])

    def test_single_flight_no_implicit_queue_or_retry(self):
        self.app.busy.acquire()
        try:
            self.assertEqual(self.request("POST", "/v1/lipsync/render", self.payload(),
                                         Authorization="Bearer " + "t" * 48,
                                         **{"Content-Type": "application/json"})[0], 409)
        finally:
            self.app.busy.release()

    def test_rejects_missing_secret(self):
        with self.assertRaises(ValueError):
            server.Service(Path(self.temp.name), "", FakeEngine())

    def test_motion_installed_state_duration_consent_and_atomic_upload(self):
        auth = {"Authorization": "Bearer " + "t" * 48, "Content-Type": "application/json"}
        self.assertEqual(self.request("GET", "/v1/motion/health", **auth)[0], 503)
        self.app.motion = FakeMotion()
        status, body = self.request("GET", "/v1/motion/health", **auth)
        self.assertEqual(status, 200)
        self.assertEqual(json.loads(body)["model"], "echomimic-v3-flash")
        data = {"image": base64.b64encode(b"\x89PNG\r\n\x1a\nfixture").decode(),
                "audio": base64.b64encode(b"RIFF\0\0\0\0WAVEfixture").decode(),
                "audio_type": "audio/wav", "role": 1, "seconds": 7, "consent": True}
        for key, value in [("seconds", 0), ("seconds", 11), ("seconds", True), ("role", -1), ("role", True), ("consent", False)]:
            invalid = dict(data, **{key: value})
            self.assertEqual(self.request("POST", "/v1/motion/render", json.dumps(invalid), **auth)[0], 400)
        self.assertEqual(self.request("POST", "/v1/motion/render?url=evil", json.dumps(data), **auth)[0], 404)
        self.app.busy.acquire()
        try:
            self.assertEqual(self.request("POST", "/v1/motion/render", json.dumps(data), **auth)[0], 409)
        finally:
            self.app.busy.release()
        status, output = self.request("POST", "/v1/motion/render", json.dumps(data), **auth)
        self.assertEqual(status, 200)
        self.assertEqual(output[4:8], b"ftyp")
        self.assertEqual(list(Path(self.temp.name).iterdir()), [])


if __name__ == "__main__":
    unittest.main()
