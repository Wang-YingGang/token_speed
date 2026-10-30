from pathlib import Path
import ctypes
from ctypes import wintypes
import json
import os
import subprocess
import time
import urllib.request

root = Path(__file__).resolve().parent
env = os.environ.copy()
env["CC_SWITCH_DB"] = str(root / "integration" / "cc-switch.db")
env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "--remote-debugging-port=9334"
process = subprocess.Popen([str(root / "src-tauri" / "target" / "debug" / "token-speed.exe")], cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
try:
    time.sleep(4)
    print("进程状态：", process.poll())
    user32 = ctypes.windll.user32
    user32.FindWindowW.restype = wintypes.HWND
    user32.GetParent.restype = wintypes.HWND
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def callback(hwnd, _):
        length = user32.GetWindowTextLengthW(hwnd)
        title = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, title, length + 1)
        class_name = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, class_name, 256)
        if "Token" in title.value or "token" in title.value or "Tauri" in class_name.value or "Chrome" in class_name.value:
            rect = wintypes.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(rect))
            print("窗口", hwnd, repr(title.value), class_name.value, "visible", user32.IsWindowVisible(hwnd), "parent", user32.GetParent(hwnd), "rect", (rect.left, rect.top, rect.right, rect.bottom))
        return True
    user32.EnumWindows(callback_type(callback), 0)
    taskbar = user32.FindWindowW("Shell_TrayWnd", None)
    print("任务栏", taskbar, "子窗口：")
    user32.EnumChildWindows(taskbar, callback_type(callback), 0)
    try:
        with urllib.request.urlopen("http://127.0.0.1:9334/json", timeout=2) as response:
            print("CDP", json.dumps(json.load(response), ensure_ascii=False, indent=2))
    except Exception as error:
        print("CDP 错误", error)
finally:
    if process.poll() is None:
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)
    print("输出", process.stdout.read() if process.stdout else "")
