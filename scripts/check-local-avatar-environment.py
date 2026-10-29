"""Diagnose the native launcher's restricted environment without printing secrets."""
import os
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
keys = ("PATH", "SystemRoot", "WINDIR", "TEMP", "TMP", "CUDA_VISIBLE_DEVICES")
env = {key: os.environ[key] for key in keys if key in os.environ}
env.update(PYTHONPATH=str(root / ".runtime/slive-venv/Lib/site-packages"),
           PYTHONUTF8="1", PYTHONDONTWRITEBYTECODE="1",
           CONCAT_SLIVE_S3FD=str(root / ".runtime/slive-models/s3fd.pth"))
result = subprocess.run([sys.executable, "-B", str(root / "integrations/slive/runtime/avatar_prepare.py"), "--check"],
                        env=env, capture_output=True, timeout=30)
print(result.stdout.decode("utf-8", errors="replace"))
raise SystemExit(result.returncode)
