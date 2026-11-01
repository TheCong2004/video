import subprocess
import time
import urllib.request
import json
import asyncio
import websockets

edge_path = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
proc = subprocess.Popen([
    edge_path,
    "--headless",
    "--remote-debugging-port=9222",
    "file:///d:/capcutpolot/concat/resources/postiz-web/index.html"
])

time.sleep(2.5)

async def test_react():
    with urllib.request.urlopen("http://localhost:9222/json") as response:
        pages = json.loads(response.read().decode())
    
    target_page = [p for p in pages if "postiz-web" in p.get("url", "")][0]
    ws_url = target_page.get("webSocketDebuggerUrl")
    
    async with websockets.connect(ws_url) as ws:
        expr = """
        (function() {
            var scripts = Array.from(document.scripts).map(s => s.src);
            var loadedScripts = scripts.filter(s => s.indexOf('_next') !== -1);
            return {
                totalScripts: scripts.length,
                loadedNextScripts: loadedScripts.length,
                hasWebpack: typeof window.__webpack_require__ !== 'undefined',
                hasNext: typeof window.__NEXT_DATA__ !== 'undefined',
                bodyText: document.body.innerText.substring(0, 300)
            };
        })()
        """
        await ws.send(json.dumps({
            "id": 10,
            "method": "Runtime.evaluate",
            "params": {"expression": expr, "returnByValue": True}
        }))
        res = await ws.recv()
        data = json.loads(res)
        print("RESULT:", json.dumps(data.get("result", {}).get("result", {}).get("value", {}), indent=2))

try:
    asyncio.run(test_react())
finally:
    proc.terminate()

