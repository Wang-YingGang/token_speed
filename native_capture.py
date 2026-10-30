from pathlib import Path
import ctypes
from ctypes import wintypes
import os
import subprocess
import time
from PIL import Image

ROOT = Path(__file__).resolve().parent
EXE = ROOT / "src-tauri" / "target" / "debug" / "token-speed.exe"
DB = ROOT / "integration" / "cc-switch.db"
OUT = ROOT / "integration" / "print-window.png"
user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
user32.SetProcessDPIAware()

class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG),
        ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG),
        ("biYPelsPerMeter", wintypes.LONG), ("biClrUsed", wintypes.DWORD), ("biClrImportant", wintypes.DWORD),
    ]

class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]

def find_window():
    found = []
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def callback(hwnd, _):
        length = user32.GetWindowTextLengthW(hwnd)
        title = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, title, length + 1)
        if "Token Speed" in title.value and user32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True
    user32.EnumWindows(callback_type(callback), 0)
    return found[0] if found else None

def print_window(hwnd):
    rect = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    width, height = rect.right - rect.left, rect.bottom - rect.top
    window_dc = user32.GetWindowDC(hwnd)
    memory_dc = gdi32.CreateCompatibleDC(window_dc)
    bitmap = gdi32.CreateCompatibleBitmap(window_dc, width, height)
    previous = gdi32.SelectObject(memory_dc, bitmap)
    result = user32.PrintWindow(hwnd, memory_dc, 2)
    info = BITMAPINFO()
    info.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    info.bmiHeader.biWidth = width
    info.bmiHeader.biHeight = -height
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    buffer = ctypes.create_string_buffer(width * height * 4)
    rows = gdi32.GetDIBits(memory_dc, bitmap, 0, height, buffer, ctypes.byref(info), 0)
    gdi32.SelectObject(memory_dc, previous)
    gdi32.DeleteObject(bitmap)
    gdi32.DeleteDC(memory_dc)
    user32.ReleaseDC(hwnd, window_dc)
    print(f"PrintWindow={result}, rows={rows}, size={width}x{height}")
    if not result or not rows:
        raise RuntimeError("PrintWindow 捕获失败")
    image = Image.frombuffer("RGBA", (width, height), buffer, "raw", "BGRA", 0, 1)
    image.save(OUT)
    print(OUT)

env = os.environ.copy()
env["CC_SWITCH_DB"] = str(DB)
process = subprocess.Popen([str(EXE)], cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
try:
    hwnd = None
    for _ in range(30):
        time.sleep(0.25)
        hwnd = find_window()
        if hwnd:
            break
    if not hwnd:
        raise RuntimeError("未找到窗口")
    time.sleep(1)
    print_window(hwnd)
finally:
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)
    if process.stdout:
        print("应用输出：", process.stdout.read())
