"""CPU-only contract checks; these do not establish GPU image/gesture quality."""
import unittest
from unittest.mock import patch
from pathlib import Path
import tempfile
import importlib.util
import shutil
import subprocess
import wave
import engine


class EngineTests(unittest.TestCase):
    def test_every_requested_second_has_sufficient_wan_frames(self):
        for seconds in range(2, 11):
            frames = engine.frame_budget(seconds)
            self.assertEqual((frames - 1) % 4, 0)
            self.assertGreaterEqual(frames / 25, seconds)
            self.assertLess(frames / 25 - seconds, 0.16)
        for bad in (0, 1, 11, True, 2.5, "5"):
            with self.assertRaises(ValueError):
                engine.frame_budget(bad)

    def test_missing_operator_installation_fails_without_downloading(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(RuntimeError, "MODEL_NOT_INSTALLED"):
                engine.EchoEngine(root, root)

    def test_subprocess_uses_no_shell_no_stdin_offline_and_timeout(self):
        instance = object.__new__(engine.EchoEngine)
        with patch.dict(engine.os.environ, {"CONCAT_LIPSYNC_TOKEN": "private-server-token"}):
            with patch.object(engine.subprocess, "run") as run:
                instance.run(["python", "infer_flash.py"], cwd=Path("operator-root"), timeout=1500)
        kwargs = run.call_args.kwargs
        self.assertNotIn("shell", kwargs)
        self.assertTrue(kwargs["check"])
        self.assertEqual(kwargs["timeout"], 1500)
        self.assertEqual(kwargs["env"]["HF_HUB_OFFLINE"], "1")
        self.assertEqual(kwargs["stdin"], engine.subprocess.DEVNULL)
        self.assertNotIn("CONCAT_LIPSYNC_TOKEN", kwargs["env"])

    @unittest.skipUnless(shutil.which("ffmpeg") and shutil.which("ffprobe") and importlib.util.find_spec("PIL"), "needs installed FFmpeg/Pillow, no model download")
    def test_real_media_duration_and_silence_with_synthetic_inference_fixture(self):
        from PIL import Image
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            instance = object.__new__(engine.EchoEngine)
            instance.root = root
            instance.models = root
            actual_run = instance.run
            budgets = []

            def synthetic_inference(args, cwd=None, timeout=90):
                if len(args) > 1 and args[1] == "infer_flash.py":
                    # Only replace the model for CPU tests. Encoding/probing and
                    # requested duration use the production adapter unchanged.
                    frames = int(args[args.index("--video_length") + 1])
                    budgets.append(frames)
                    folder = Path(args[args.index("--save_path") + 1])
                    folder.mkdir()
                    actual_run(["ffmpeg", "-nostdin", "-v", "error", "-f", "lavfi", "-i",
                                "color=c=blue:s=128x128:r=25", "-frames:v", str(frames),
                                "-c:v", "libx264", str(folder / "reference_output.mp4")])
                else:
                    if "-vn" in args:
                        self.assertEqual(args[args.index("-f") + 1], "wav")
                    actual_run(args, cwd=cwd, timeout=timeout)

            instance.run = synthetic_inference
            for seconds in (2, 7, 10):
                work = root / str(seconds)
                work.mkdir()
                photo, audio = work / "input.png", work / "input.wav"
                Image.new("RGB", (128, 128), "blue").save(photo)
                with wave.open(str(audio), "wb") as wav:
                    wav.setnchannels(1)
                    wav.setsampwidth(2)
                    wav.setframerate(16000)
                    wav.writeframes(b"\0\0" * 16000)
                output = instance.render(photo, audio, 1, seconds, work)
                self.assertEqual(output[4:8], b"ftyp")
                info = subprocess.run(["ffprobe", "-v", "error", "-show_format", "-show_streams", "-of", "json", str(work / "motion.mp4")], capture_output=True, check=True)
                import json
                value = json.loads(info.stdout)
                self.assertAlmostEqual(float(value["format"]["duration"]), seconds, places=2)
                self.assertFalse(any(s["codec_type"] == "audio" for s in value["streams"]))
            self.assertEqual(budgets, [engine.frame_budget(s) for s in (2, 7, 10)])


if __name__ == "__main__":
    unittest.main()
