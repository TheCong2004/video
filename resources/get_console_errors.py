import subprocess
import time
import urllib.request
import json
import asyncio
import sys

# Kiem tra websockets package
try:
    import websockets
except ImportError:
    subprocess.check_call([sys.executable, "-m", "pip", "install", "websockets"])
    import websockets

edge_path = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
proc = subprocess.Popen([
    edge_path,
    "--headless",
    "--remote-debugging-port=9222",
    "file:///d:/capcutpolot/concat/resources/postiz-web/index.html"
])

time.sleep(2)

async def check_logs():
    with urllib.request.urlopen("http://localhost:9222/json") as response:
        pages = json.loads(response.read().decode())
    
    target_page = None
    for p in pages:
        if "postiz-web" in p.get("url", ""):
            target_page = p
            break
            
    if not target_page:
        print("Target page not found!")
        return

    ws_url = target_page.get("webSocketDebuggerUrl")
    print("Connecting to ws:", ws_url)
    
    async with websockets.connect(ws_url) as ws:
        # Enable console and runtime
        await ws.send(json.dumps({"id": 1, "method": "Console.enable"}))
        await ws.send(json.dumps({"id": 2, "method": "Runtime.enable"}))
        await ws.send(json.dumps({"id": 3, "method": "Log.enable"}))
        
        # Reload page to catch all initial errors
        await ws.send(json.dumps({"id": 4, "method": "Page.reload"}))
        
        start = time.time()
        while time.time() - start < 5:
            try:
                msg = await asyncio.wait_for(ws.recv(), timeout=1.0)
                data = json.loads(msg)
                method = data.get("method", "")
                if "Console" in method or "exception" in method.lower() or "log" in method.lower():
                    print("LOG:", json.dumps(data, indent=2))
            except asyncio.TimeoutError:
                pass

try:
    asyncio.run(check_logs())
finally:
    proc.terminate()

