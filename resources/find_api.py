import os
import re

frontend_dir = r"d:\capcutpolot\postiz-app\apps\frontend\src"
endpoints = set()
for root, _, files in os.walk(frontend_dir):
    for f in files:
        if f.endswith((".ts", ".tsx")):
            p = os.path.join(root, f)
            with open(p, "r", encoding="utf-8", errors="ignore") as fp:
                txt = fp.read()
                matches = re.findall(r"['\"`](/api/[a-zA-Z0-9_/-]+)['\"`]", txt)
                endpoints.update(matches)

print("Found endpoints count:", len(endpoints))
for ep in sorted(list(endpoints))[:30]:
    print(" -", ep)

