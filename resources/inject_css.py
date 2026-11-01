import os
import glob
import re

postiz_dir = r"d:\capcutpolot\concat\resources\postiz-web"
css_files = glob.glob(os.path.join(postiz_dir, "_next", "static", "chunks", "*.css"))

print(f"Found {len(css_files)} CSS files.")
all_css = []
for fpath in css_files:
    fname = os.path.basename(fpath)
    with open(fpath, "r", encoding="utf-8") as f:
        content = f.read()
    print(f"Loaded CSS: {fname} ({len(content)} chars)")
    all_css.append(f"/* === {fname} === */\n" + content)

combined_css = "\n".join(all_css)

custom_css = """
/* ========================================================== */
/* POSTIZ NATIVE CONCAT THEME & LOGO REMOVAL                  */
/* ========================================================== */

/* 1. XOA HOAN TOAN VA TRIET DE MOI LOGO */
img[src*="logo"],
img[src*="Logo"],
img[alt*="logo"],
img[alt*="Logo"],
svg[id*="logo"],
svg[id*="Logo"],
svg[class*="logo"],
.logo,
[class*="logo-"],
[id*="logo-"],
a[href="/launches"] > svg,
a[href="/"] > svg,
a[href="/"] > img,
nav > div:first-child > div:first-child,
div[class*="logo"],
.cursor-pointer[data-state],
svg.lucide-sparkles + svg,
a[href="/"] svg {
    display: none !important;
    visibility: hidden !important;
    opacity: 0 !important;
    pointer-events: none !important;
}

/* 2. Fix layout, font chu va dark mode dong bo Concat Studio */
html, body {
    background-color: #0d0f18 !important;
    color: #f1f5f9 !important;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif !important;
    margin: 0 !important;
    padding: 0 !important;
}

/* 3. Style cho Left Sidebar Postiz */
#left-menu, nav, aside {
    background-color: #111420 !important;
    border-right: 1px solid #1e2436 !important;
}

/* 4. Tinh chinh menu items */
a, button {
    text-decoration: none !important;
    font-family: inherit !important;
}

/* 5. Bang, Calendar va Card Dark Mode */
.dark, [class*="bg-white"] {
    background-color: #0d0f18 !important;
    color: #f1f5f9 !important;
}

[class*="text-gray-900"], [class*="text-black"] {
    color: #f1f5f9 !important;
}

[class*="border-gray-200"], [class*="border-gray-100"] {
    border-color: #1e2436 !important;
}
"""

index_path = os.path.join(postiz_dir, "index.html")
with open(index_path, "r", encoding="utf-8") as f:
    html = f.read()

# Dam bao html tag co class="dark"
if '<html' in html and 'class="dark"' not in html:
    html = re.sub(r'<html([^>]*)>', r'<html class="dark"\1>', html, count=1)

# Xoa cac the style bundle cu
html = re.sub(r'<style id="postiz-inlined-tailwind-bundle">.*?</style>', '', html, flags=re.DOTALL)
html = re.sub(r'<style id="nexpulse-clean-style">.*?</style>', '', html, flags=re.DOTALL)
html = re.sub(r'<style id="postiz-complete-css-bundle">.*?</style>', '', html, flags=re.DOTALL)

# Insert the combined bundle
style_block = f'<style id="postiz-complete-css-bundle">\n{combined_css}\n{custom_css}\n</style>'
html = html.replace('</head>', f'{style_block}\n</head>')

with open(index_path, "w", encoding="utf-8") as f:
    f.write(html)

print(f"Successfully injected CSS into {index_path}. File size: {os.path.getsize(index_path)} bytes")

