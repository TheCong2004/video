"""Compile a clean reusable core into a new directory, without publishing."""
import json
import os
import shutil
import subprocess
import sys
import uuid
from pathlib import Path
import agency_runtime

ROOT = Path(__file__).resolve().parent

def build_core(version):
    destination = ROOT / "build-agency-core" / version / uuid.uuid4().hex
    distribution = destination / "dist"
    env = {**os.environ, "SLIVE_AGENCY_CORE_BUILD": "1"}
    def compile_spec(name):
        subprocess.run([sys.executable, "-m", "PyInstaller", "--noconfirm",
                        "--distpath", str(distribution), "--workpath", str(destination / "work" / name),
                        str(ROOT / (name + ".spec"))], cwd=ROOT, env=env, check=True)
    if agency_runtime.SERVICE_TYPE == "live_noface":
        compile_spec("SLiveNoFace")
        core = distribution / "SLiveNoFace"
    else:
        compile_spec("Live_AI_LipSync")
        compile_spec("SLiveFace_Setup")
        core = distribution / "SliveVip"
        # Copy the entire runtime, including native libraries, not an older EXE.
        shutil.copytree(distribution / "Live_AI_LipSync", core / "app")
        shutil.copy2(core / "app" / "Live_AI_LipSync.exe", core / "app" / "SliveVip.exe")
        for folder in ("web", "tiktok_pin_extension"):
            if (ROOT / folder).is_dir():
                shutil.copytree(ROOT / folder, core / folder, dirs_exist_ok=True)
        for name in ("customer-resource-manifest.json", "resource-manifest.json", "config.example.json"):
            if (ROOT / name).is_file():
                shutil.copy2(ROOT / name, core / name)
                if name.endswith("resource-manifest.json"):
                    manifest = json.loads((core / name).read_text(encoding="utf-8-sig"))
                    manifest["bundles"] = [bundle for bundle in manifest.get("bundles", [])
                                           if bundle.get("install_root") in ("resources", "data")]
                    (core / name).write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
        (core / "app" / "version.json").write_text(json.dumps({"version": version}), encoding="utf-8")
    dotnet = """<?xml version="1.0" encoding="utf-8"?>
<configuration><startup useLegacyV2RuntimeActivationPolicy="true">
<supportedRuntime version="v4.0" sku=".NETFramework,Version=v4.7.2" />
</startup><runtime><loadFromRemoteSources enabled="true" /></runtime></configuration>"""
    for executable in list(core.glob("*.exe")) + list((core / "app").glob("*.exe")):
        executable.with_name(executable.name + ".config").write_text(dotnet, encoding="utf-8")
    (core / "version.json").write_text(json.dumps({"version": version}), encoding="utf-8")
    (core / "agency-runtime.version").write_text("1", encoding="utf-8")
    return core
