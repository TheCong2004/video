import os

frontend_dir = r"d:\capcutpolot\postiz-app\apps\frontend\src"
for root, _, files in os.walk(frontend_dir):
    for f in files:
        if f.endswith((".ts", ".tsx")):
            p = os.path.join(root, f)
            with open(p, "r", encoding="utf-8", errors="ignore") as fp:
                txt = fp.read()
                if "LoadingComponent" in txt or "components/layout/loading" in txt:
                    print("Used in:", f, "path:", os.path.relpath(p, frontend_dir))
                    for line in txt.split("\n"):
                        if "LoadingComponent" in line or "loading" in line:
                            print("   ", line.strip())

