import os

app_dir = r"d:\capcutpolot\postiz-app\apps\frontend\src\app"
for root, dirs, files in os.walk(app_dir):
    for f in files:
        if "launch" in f.lower() or "page" in f.lower():
            print(os.path.relpath(os.path.join(root, f), app_dir))

