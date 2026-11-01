with open(r"d:\capcutpolot\concat\resources\postiz-web\index.html", "r", encoding="utf-8") as f:
    html = f.read()

import re

matches = re.findall(r'<div[^>]*class="[^"]*(?:spin|loading|loader|rounded-full)[^"]*"[^>]*>', html)
print(f"Found {len(matches)} loader-like divs:")
for m in matches[:15]:
    print(m)

