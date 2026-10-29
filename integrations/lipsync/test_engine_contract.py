"""No GPU/weights: validate failure gates and local-only decoder arguments."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import musetalk_engine as engine


class EngineGateTests(unittest.TestCase):
    def test_probe_rejects_nonfinite_duration_and_never_enables_network_protocols(self):
        with patch.object(engine, "command", return_value=SimpleNamespace(stdout=json.dumps({"format": {"duration": "nan"}}).encode())) as call:
            with self.assertRaises(ValueError):
                engine.probe(Path("local.wav"))
            argv = call.call_args.args[0]
            self.assertEqual(argv[argv.index("-protocol_whitelist") + 1], "file,pipe")
            self.assertIn("-format_whitelist", argv)

    def test_overlong_audio_rejected_before_gpu_is_touched(self):
        unloaded = engine.MuseTalkEngine.__new__(engine.MuseTalkEngine)
        with patch.object(engine, "probe", return_value=({}, 16.0)):
            with self.assertRaises(ValueError):
                unloaded.render(Path("source.mp4"), Path("speech.wav"), Path("work"))

    def test_source_limit_rejected_before_avatar_preparation(self):
        unloaded = engine.MuseTalkEngine.__new__(engine.MuseTalkEngine)
        unloaded.avatar = None
        unloaded.source_hash = None
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "source.mp4"
            source.write_bytes(b"test source")
            with patch.object(engine, "probe", return_value=({"streams": [{"codec_type": "video", "width": 1920, "height": 1080}]}, 30.0)):
                with self.assertRaises(ValueError):
                    unloaded.prepare(source, Path(temporary))

    def test_missing_weights_do_not_become_ready_or_download_anything(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(RuntimeError, "MODEL_FILES_MISSING"):
                engine.MuseTalkEngine(Path(temporary))


if __name__ == "__main__":
    unittest.main()
