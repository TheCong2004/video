with open(r"d:\capcutpolot\concat\resources\postiz-web\index.html", "r", encoding="utf-8") as f:
    html = f.read()

import re
svgs = re.findall(r"<svg[^>]*>.*?</svg>", html, re.DOTALL)
print(svgs[1])

