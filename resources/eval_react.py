import asyncio
import websockets
import json
import urllib.request

async def test_react():
    with urllib.request.urlopen("http://localhost:9222/json") as response:
        pages = json.loads(response.read().decode())
    
    target_page = [p for p in pages if "postiz-web" in p.get("url", "")][0]
    ws_url = target_page.get("webSocketDebuggerUrl")
    
    async with websockets.connect(ws_url) as ws:
        # Check if React has mounted
        expr = """
        ({
            hasReact: !!window.__NEXT_DATA__ || !!window.React,
            scriptsCount: document.scripts.length,
            bodyHtmlLength: document.body.innerHTML.length,
            title: document.title,
            pathname: window.location.pathname
        })
        """
        await ws.send(json.dumps({
            "id": 10,
            "method": "Runtime.evaluate",
            "params": {"expression": expr, "returnByValue": True}
        }))
        res = await ws.recv()
        print("React state:", json.dumps(json.loads(res), indent=2))

asyncio.run(test_react())

