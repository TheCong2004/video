import re

index_path = r"d:\capcutpolot\concat\resources\postiz-web\index.html"
with open(index_path, "r", encoding="utf-8") as f:
    html = f.read()

# 1. Xoa bo tat ca thuoc tinh crossorigin="", crossorigin, crossOrigin trong the script va link
html = re.sub(r'\s+crossorigin="[^"]*"', '', html)
html = re.sub(r'\s+crossorigin=\'[^\']*\'', '', html)
html = re.sub(r'\s+crossorigin', '', html)
html = re.sub(r'\s+crossOrigin', '', html)

with open(index_path, "w", encoding="utf-8") as f:
    f.write(html)

print("Removed all crossorigin attributes!")

