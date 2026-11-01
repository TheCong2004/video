with open(r"d:\capcutpolot\postiz-app\apps\frontend\.next\server\app\launches.html", "r", encoding="utf-8") as f:
    txt = f.read()

import re

# Tim cac doan co "borderTopColor" hoac "612bd3" hoac "spin"
matches = [m.start() for m in re.finditer(r'612bd3', txt)]
print(f"Found '612bd3' in launches.html at: {matches}")
for idx in matches:
    print(txt[max(0, idx-100):min(len(txt), idx+200)])

