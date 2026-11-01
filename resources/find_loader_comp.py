import os
import re

frontend_dir = r"d:\capcutpolot\postiz-app\apps\frontend\src"
for root, _, files in os.walk(frontend_dir):
    for f in files:
        if "load" in f.lower() or "spin" in f.lower() or "suspense" in f.lower():
            print("Loader file:", f, "in", os.path.relpath(root, frontend_dir))
            p = os.path.join(root, f)
            with open(p, "r", encoding="utf-8", errors="ignore") as fp:
                print("   ", fp.read()[:300])

