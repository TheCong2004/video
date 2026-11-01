import os
import re

frontend_dir = r"d:\capcutpolot\postiz-app\apps\frontend\src"
for root, _, files in os.walk(frontend_dir):
    for f in files:
        if "launches" in f.lower() or "post" in f.lower():
            p = os.path.join(root, f)
            with open(p, "r", encoding="utf-8", errors="ignore") as fp:
                txt = fp.read()
                if "fetch" in txt or "useQuery" in txt or "axios" in txt:
                    print(f"File: {f}")
                    for line in txt.split("\n"):
                        if any(k in line for k in ["fetch", "useQuery", "axios", "useFetch", "api"]):
                            if len(line.strip()) < 120:
                                print("   ", line.strip())

