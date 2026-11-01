import subprocess
import time
import urllib.request
import json

edge_path = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
proc = subprocess.Popen([
    edge_path,
    "--headless",
    "--remote-debugging-port=9222",
    "file:///d:/capcutpolot/concat/resources/postiz-web/index.html"
])

time.sleep(2)
try:
    with urllib.request.urlopen("http://localhost:9222/json") as response:
        pages = json.loads(response.read().decode())
        print("Connected pages:", len(pages))
        for p in pages:
            print(" -", p.get("title"), p.get("url"))
finally:
    proc.terminate()

