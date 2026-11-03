"""Check floating taskbar placement without relying on desktop input focus."""
import asyncio
import base64
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "integration" / ".deps"))
import websockets
from websockets.exceptions import ConnectionClosed

PORT = 9338
WORK = ROOT / "integration" / "floating-position" / str(time.time_ns())
WORK.mkdir(parents=True)
SETTINGS = WORK / "settings.json"
SETTINGS.write_text(json.dumps({"windowMode":"floating","displayMode":"codex","showSessionOutput":False,"floatingX":100,"floatingY":100}), encoding="utf-8")
DB = WORK / "usage.db"
with sqlite3.connect(DB) as connection:
    connection.execute("CREATE TABLE proxy_request_logs (request_id TEXT, app_type TEXT, model TEXT, output_tokens INTEGER, latency_ms INTEGER, first_token_ms INTEGER, duration_ms INTEGER, created_at INTEGER, status_code INTEGER)")

env = os.environ.copy()
env.update(CC_SWITCH_DB=str(DB), TOKEN_SPEED_SETTINGS=str(SETTINGS),
    TOKEN_SPEED_CODEX_SESSIONS=str(WORK / "empty-codex"), TOKEN_SPEED_CLAUDE_PROJECTS=str(WORK / "empty-claude"),
    WEBVIEW2_USER_DATA_FOLDER=str(WORK / "webview"), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f"--remote-debugging-port={PORT}")
EXE = Path(os.environ.get("TOKEN_SPEED_EXE", str(ROOT / "src-tauri/target/debug/token-speed.exe")))
user32 = ctypes.windll.user32
user32.SetProcessDPIAware()
user32.FindWindowW.restype = wintypes.HWND
user32.GetParent.restype = wintypes.HWND
user32.IsWindowVisible.argtypes = [wintypes.HWND]
user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
user32.SetWindowPos.argtypes = [wintypes.HWND,wintypes.HWND,ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.c_int,wintypes.UINT]
user32.GetWindow.restype = wintypes.HWND
user32.GetWindow.argtypes = [wintypes.HWND,wintypes.UINT]
user32.GetWindowLongPtrW.restype = ctypes.c_ssize_t
user32.GetWindowLongPtrW.argtypes = [wintypes.HWND,ctypes.c_int]
user32.GetForegroundWindow.restype = wintypes.HWND
user32.SetForegroundWindow.argtypes = [wintypes.HWND]
user32.GetClassNameW.argtypes = [wintypes.HWND,wintypes.LPWSTR,ctypes.c_int]
user32.WindowFromPoint.restype = wintypes.HWND
user32.WindowFromPoint.argtypes = [wintypes.POINT]
user32.GetAncestor.restype = wintypes.HWND
user32.GetAncestor.argtypes = [wintypes.HWND,wintypes.UINT]


async def cdp(url, method, params=None):
    async with websockets.connect(url, origin=f"http://127.0.0.1:{PORT}") as socket:
        await socket.send(json.dumps({"id":1,"method":method,"params":params or {}}))
        while True:
            response = json.loads(await socket.recv())
            if response.get("id") == 1:
                if "error" in response:
                    raise RuntimeError(response["error"])
                return response.get("result", {})


def js(url, expression):
    result = asyncio.run(cdp(url,"Runtime.evaluate",{"expression":expression,"returnByValue":True,"awaitPromise":True}))
    if "exceptionDetails" in result:
        raise RuntimeError(result["exceptionDetails"])
    return result.get("result",{}).get("value")


def invoke(url, name, args=None):
    return js(url, f"window.__TAURI__.core.invoke({json.dumps(name)},{json.dumps(args or {})})")


def rect(hwnd):
    value = wintypes.RECT()
    assert user32.GetWindowRect(hwnd,ctypes.byref(value))
    return value.left,value.top,value.right,value.bottom


def move(hwnd, target):
    assert user32.SetWindowPos(hwnd,None,*target,0,0,0x001|0x004|0x010)


def above_taskbar(hwnd, taskbar):
    cursor=hwnd
    for _ in range(4096):
        cursor=user32.GetWindow(cursor,3)  # GW_HWNDPREV
        if not cursor: return True
        if cursor==taskbar: return False
    raise AssertionError("Unexpected cycle in native window order")


def check_taskbar_order(hwnd, url, taskbar, expected_position):
    for _ in range(3):
        user32.SetForegroundWindow(taskbar)
        assert user32.SetWindowPos(taskbar,wintypes.HWND(-1),0,0,0,0,0x001|0x002|0x010)
        foreground=user32.GetForegroundWindow()
        started=time.monotonic()
        while not above_taskbar(hwnd,taskbar) and time.monotonic()-started < .75:
            time.sleep(.01)
        elapsed=time.monotonic()-started
        assert above_taskbar(hwnd,taskbar), "activated taskbar covered the topmost floating window"
        bounds=rect(hwnd)
        hit=user32.WindowFromPoint(wintypes.POINT((bounds[0]+bounds[2])//2,(bounds[1]+bounds[3])//2))
        assert user32.GetAncestor(hit,2)==hwnd, "taskbar still covers the floating window at its screen position"
        assert user32.GetForegroundWindow()==foreground, "restoring topmost order stole foreground focus"
        assert rect(hwnd)[:2]==expected_position, "restoring topmost order moved the floating window"
        assert user32.GetWindowLongPtrW(hwnd,-20)&8
        print(f"Taskbar z-order restored without activation in {elapsed*1000:.0f} ms")
    # An explicit opt-out must stop the z-order guard, even while locked.
    invoke(url,"set_monitor_settings",{"patch":{"alwaysOnTop":False}})
    assert user32.SetWindowPos(taskbar,wintypes.HWND(-1),0,0,0,0,0x001|0x002|0x010)
    time.sleep(1.1)
    assert not user32.GetWindowLongPtrW(hwnd,-20)&8
    assert not above_taskbar(hwnd,taskbar), "disabled topmost setting was overridden"
    invoke(url,"set_monitor_settings",{"patch":{"alwaysOnTop":True}})


def secondary_taskbars():
    found=[]
    def visit(hwnd,_):
        name=ctypes.create_unicode_buffer(100)
        user32.GetClassNameW(hwnd,name,100)
        if name.value=="Shell_SecondaryTrayWnd" and user32.IsWindowVisible(hwnd): found.append(hwnd)
        return True
    callback=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)(visit)
    user32.EnumWindows(callback,0)
    return found


def native_window(pid):
    found=[]
    def visit(hwnd, _):
        owner=wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd,ctypes.byref(owner))
        if owner.value == pid:
            title=ctypes.create_unicode_buffer(100)
            user32.GetWindowTextW(hwnd,title,100)
            if title.value == "Token Speed": found.append(hwnd)
        return True
    callback=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)(visit)
    user32.EnumWindows(callback,0)
    return found[0] if found else None


log=(WORK / "process.log").open("w",encoding="utf-8")
def launch():
    process=subprocess.Popen([str(EXE)],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT)
    try:
        for _ in range(100):
            if process.poll() is not None: raise RuntimeError("App exited during startup")
            try:
                hwnd=native_window(process.pid)
                with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json",timeout=1) as response:
                    pages=json.load(response)
                for page in pages:
                    url=page.get("webSocketDebuggerUrl")
                    if hwnd and user32.IsWindowVisible(hwnd) and page.get("type")=="page" and js(url,"window.__TAURI__?.window.getCurrentWindow().label")=="main":
                        return process,hwnd,url
            except (OSError,RuntimeError,ConnectionClosed): pass
            time.sleep(.15)
        raise RuntimeError("Native window did not become ready")
    except Exception:
        process.terminate();process.wait(timeout=5);raise


def stop(process):
    if process.poll() is None:
        process.terminate()
        process.wait(timeout=5)


process=None
try:
    process,hwnd,url=launch()
    bar=rect(user32.FindWindowW("Shell_TrayWnd",None))
    window=rect(hwnd)
    target=(bar[0]+200,bar[1]+max(0,((bar[3]-bar[1])-(window[3]-window[1]))//2))
    move(hwnd,target)
    assert rect(hwnd)[:2]==target
    for _ in range(20):
        cfg=invoke(url,"get_taskbar_settings")
        if (cfg["floatingX"],cfg["floatingY"])==target: break
        time.sleep(.05)
    assert (cfg["floatingX"],cfg["floatingY"])==target, "native move event was not received"
    for patch in ({"opacity":64},{"theme":"light"},{"fontSize":16},{"showSessionOutput":True,"sessionOutputWidth":260},{"positionLocked":True}):
        invoke(url,"set_monitor_settings",{"patch":patch})
        time.sleep(.1)
        assert rect(hwnd)[:2]==target, {"patch":patch,"expected":target,"actual":rect(hwnd)}
    saved=json.loads(SETTINGS.read_text(encoding="utf-8"))
    assert (saved["floatingX"],saved["floatingY"])==target
    stop(process)
    process,hwnd,url=launch()
    assert rect(hwnd)[:2]==target, {"restart_expected":target,"actual":rect(hwnd)}
    assert not user32.GetParent(hwnd), "floating window became embedded"
    check_taskbar_order(hwnd,url,user32.FindWindowW("Shell_TrayWnd",None),target)
    for taskbar in secondary_taskbars():
        bounds=rect(taskbar)
        size=rect(hwnd)
        secondary_position=(bounds[0]+200,bounds[1]+max(0,((bounds[3]-bounds[1])-(size[3]-size[1]))//2))
        move(hwnd,secondary_position)
        invoke(url,"set_monitor_settings",{"patch":{"positionLocked":True}})
        check_taskbar_order(hwnd,url,taskbar,secondary_position)
        print(f"Secondary taskbar verified at {secondary_position}")
    move(hwnd,target)
    invoke(url,"set_monitor_settings",{"patch":{"positionLocked":True}})
    screenshot=asyncio.run(cdp(url,"Page.captureScreenshot",{"format":"png"}))
    (WORK/"restored-on-taskbar.png").write_bytes(base64.b64decode(screenshot["data"]))
    # Quitting immediately after moving must also persist the latest coordinates.
    target=(target[0]+31,target[1])
    move(hwnd,target)
    try:
        invoke(url,"quit_app")
    except (OSError,RuntimeError,ConnectionClosed):
        pass
    process.wait(timeout=5)
    saved=json.loads(SETTINGS.read_text(encoding="utf-8"))
    assert (saved["floatingX"],saved["floatingY"])==target
    process,hwnd,url=launch()
    assert rect(hwnd)[:2]==target
    print(f"PASS: floating taskbar placement survives settings/restart/quit; taskbar activation restores z-order without stealing focus; disabling topmost is respected. Final position: {target}")
    print(f"Artifacts: {WORK}")
finally:
    if process is not None: stop(process)
    log.close()
