with open(r"d:\capcutpolot\concat\resources\postiz-web\index.html", "r", encoding="utf-8") as f:
    html = f.read()

import re

# Tim moi the svg co trong file html
svgs = re.findall(r"<svg[^>]*>.*?</svg>", html, re.DOTALL)
print(f"Total SVGs in HTML: {len(svgs)}")
for i, s in enumerate(svgs):
    # Lay svg co kich thuoc hoac path dac biet
    if "animate" in s or "spin" in s or "circle" in s or "rotate" in s:
        print(f"SVG #{i}: {s[:160]}...")
    elif "width" in s and ("60" in s or "48" in s or "40" in s or "32" in s or "50" in s):
        print(f"SVG #{i}: {s[:160]}...")

