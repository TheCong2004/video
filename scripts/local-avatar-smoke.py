"""Real native local smoke. No Promax, Node, TikTok or cloud service is started."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
exe = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else root / "src/target/debug/concat.exe"
artifacts = Path(tempfile.mkdtemp(prefix="local-avatar-smoke-", dir=root / ".runtime"))
(artifacts / "temp").mkdir()
sample = root / "src/crates/concat/assets/avatars/newscaster.mp4"
video = artifacts / "sample.mp4"
ffmpeg_bin = next((root / ".deps/ffmpeg").glob("*/bin"))
subprocess.run([str(ffmpeg_bin / "ffmpeg.exe"), "-nostdin", "-v", "error", "-i", str(sample),
                "-t", "2", "-an", "-c:v", "libx264", str(video)], check=True, timeout=30)
env = os.environ.copy()
for key in list(env):
    if key.startswith("PROMAX_") or key in ("CONCAT_PROMAX_ORIGIN", "CONCAT_PROMAX_AVATAR_TOKEN"):
        del env[key]
env.update(CONCAT_PROMAX_AVATAR_SMOKE_VIDEO=str(video),
           CONCAT_AVATAR_RUNTIME_DIR=str(root),
           LOCALAPPDATA=str(artifacts / "local"), APPDATA=str(artifacts / "roaming"),
           TEMP=str(artifacts / "temp"), TMP=str(artifacts / "temp"))
ort = next((root / ".deps/ort").glob("*/lib"))
env["PATH"] = os.pathsep.join([str(ffmpeg_bin), str(ort), str(exe.parent), env.get("PATH", "")])
result = subprocess.run([str(exe), "--smoke"], env=env, cwd=root, stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT, timeout=620, creationflags=subprocess.CREATE_NO_WINDOW)
output = result.stdout.decode("utf-8", errors="replace")
if result.returncode or "PROMAX_AVATAR_UI_SMOKE=PASS" not in output or "archive=real live_guard=blocked" not in output:
    print(output[-4000:])
    raise SystemExit("LOCAL_AVATAR_UI_SMOKE_FAILED")
jobs = []
archived = []
for directory in [artifacts / "local", artifacts / "roaming"]:
    for path in directory.rglob("library/*.json"):
        job = json.loads(path.read_text(encoding="utf-8"))
        if job.get("status") == "ready":
            jobs.append(job)
    for path in directory.rglob("record.json"):
        if path.parent.parent.name == ".trash":
            job = json.loads(path.read_text(encoding="utf-8"))
            assert (path.parent / "media/source.mp4").is_file()
            archived.append(job)
assert not jobs and len(archived) == 1 and archived[0]["frames"] == 50
assert video.is_file(), "the user-selected original fixture must remain untouched"
print("CONCAT_LOCAL_AVATAR_SMOKE=PASS app_started_runtime=real preparation=50_frames live_motion=real clean_output=matched archive=real original=preserved live_guard=blocked")
print(f"SMOKE_ARTIFACTS={artifacts}")
