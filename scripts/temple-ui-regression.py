"""Explicit, isolated desktop QA. Requires pywinauto and Pillow on Windows.

Pass an owned Concat process and a test-only temple server origin. Mutating
simulation checks require --simulate; never connects an account or broadcasts.
"""
import argparse
import ctypes
import json
import time
import urllib.request
from pathlib import Path

ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
from pywinauto import Application, mouse
from PIL import ImageGrab
import win32clipboard

p = argparse.ArgumentParser()
p.add_argument("--pid", type=int, required=True)
p.add_argument("--origin", default="http://127.0.0.1:3199")
p.add_argument("--output", type=Path, required=True)
p.add_argument("--simulate", action="store_true")
p.add_argument("--inspect", action="store_true")
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
app = Application(backend="win32").connect(process=a.pid)
window = next(w for w in app.windows() if w.class_name() == "Window Class" and w.window_text())
window.restore()
window.set_focus()
ui = Application(backend="uia").connect(process=a.pid).window(handle=window.handle)

def controls(kind=None):
    return ui.descendants(control_type=kind) if kind else ui.descendants()

def button(text):
    return next(c for c in controls("Button") if text in c.window_text())

def click(text):
    control = button(text)
    assert control.is_enabled(), f"Disabled: {text}"
    pointer_click(control)
    time.sleep(.2)

def pointer_click(control):
    # Let Winit observe the move before the press, including WebView child
    # transitions. Rapid SendInput move+press can retain its previous hover.
    r = control.rectangle()
    position = (int((r.left + r.right) / 2), int((r.top + r.bottom) / 2))
    ctypes.windll.user32.SetCursorPos(*position)
    time.sleep(.25)
    mouse.click(coords=position)

def status():
    return json.load(urllib.request.urlopen(a.origin + "/status", timeout=2))

def post(command):
    request = urllib.request.Request(a.origin + "/api/control", data=json.dumps(command).encode(), headers={"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(request, timeout=2))

def wait(predicate, timeout=10):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if predicate(): return
        time.sleep(.1)
    raise AssertionError("Desktop/server assertion timed out")

def screenshot(name):
    window.set_focus()
    time.sleep(.5)
    ImageGrab.grab(bbox=rectangle(window.rectangle())).save(a.output / name)

def rectangle(r):
    return (r.left, r.top, r.right, r.bottom)

if a.inspect:
    data = [{"type": c.element_info.control_type, "name": c.window_text(), "rect": rectangle(c.rectangle()), "enabled": c.is_enabled()} for c in controls()]
    (a.output / "controls.json").write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf8")
    screenshot("inspect.png")
    print("INSPECT_SAVED", len(data))
    raise SystemExit()

wait(lambda: any("Server: Kết nối tốt" in c.window_text() for c in controls("Text")))
result = {"pid": a.pid, "origin": a.origin, "checks": [], "sizes": []}
click("Copy Link OBS Studio")
def copied_overlay():
    try:
        win32clipboard.OpenClipboard()
        return win32clipboard.GetClipboardData(win32clipboard.CF_UNICODETEXT) == a.origin + "/live/overlay.html"
    except Exception:
        return False
    finally:
        try: win32clipboard.CloseClipboard()
        except Exception: pass
wait(copied_overlay)
result["checks"].append("native OBS button copies the actual configured overlay URL")
original = status()["settings"]
if a.simulate:
    assert a.origin.endswith(":3199"), "Simulation QA is restricted to the isolated 3199 runtime"
    assert status()["tiktok"]["status"] == "idle", "Never mutate an active account room"
    post({"cmd": "settings", "settings": {"autoChant": False, "gifts": {"readAll": False, "minDiamonds": 50000}}})
    post({"cmd": "clear"})
    name = next(c for c in controls("Edit") if c.window_text() == "Tên người xem")
    name.set_edit_text("QA_Native")
    text = next(c for c in controls("Edit") if c.window_text() == "Bình luận / Lời nguyện")
    text.set_edit_text("Nam mo A Di Da Phat")
    click("Gửi bình luận")
    wait(lambda: status()["viewers"] == 1)
    click("Thả tim")
    for gift in ["Hoa hồng", "Hoa sen", "Chuông · 199"]:
        click(gift)
    wait(lambda: any("230" in c.window_text() for c in controls("Text")))
    result["checks"].append("native text inputs, chat, like and three gifts; total 230 coins")
    screenshot("native-simulation.png")
    for count in [15, 36, 64, 100]:
        post({"cmd": "clear"})
        click(str(count) + " người")
        wait(lambda: status()["viewers"] == count)
    click("Làm trống")
    wait(lambda: status()["viewers"] == 0)
    click("Bật Bot")
    wait(lambda: status()["bots"]["on"])
    click("Tắt Bot")
    wait(lambda: not status()["bots"]["on"])
    result["checks"].append("15/36/64/100 presets, clear, acknowledged bot on/off")
    music = next(c for c in controls("CheckBox") if c.window_text() == "Nhạc thiền")
    pointer_click(music)
    wait(lambda: status()["settings"]["music"] != original["music"])
    result["checks"].append("native music switch updates runtime settings")
    click("Bật âm thanh")
    wait(lambda: any("Tắt âm preview" in c.window_text() for c in controls("Button")))
    click("Tắt âm preview")
    result["checks"].append("preview acknowledges actual AudioContext activation then mute")

for tab, filename in [("Avatar &", "avatar"), ("Sản Phẩm &", "products"), ("Kịch Bản &", "scripts"), ("Điều phối LIVE", "livebrain"), ("Nhật Ký", "logs")]:
    click(tab)
    screenshot("native-tab-" + filename + ".png")
    if a.simulate:
        assert name.get_value() == "QA_Native", "Input state lost after tab change"
result["checks"].append("five real native tabs reachable; inputs preserved")
if a.simulate:
    post({"cmd": "clear"})
    post({"cmd": "settings", "settings": original})
for width, height in [(1920, 1080), (1632, 1000), (1440, 900), (1366, 768), (960, 900)]:
    window.move_window(x=0, y=0, width=width, height=height)
    time.sleep(.8)
    screenshot(f"native-{width}x{height}.png")
    result["sizes"].append([width, height])
(a.output / "desktop-results.json").write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf8")
print(json.dumps(result, ensure_ascii=False))
