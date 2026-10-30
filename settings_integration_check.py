from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent / "integration" / ".deps"))
import asyncio
import ctypes
from ctypes import wintypes
import json
import os
import sqlite3
import subprocess
import time
import urllib.request
import winreg
from PIL import Image, ImageGrab
import websockets

ROOT = Path(__file__).resolve().parent
EXE = Path(os.environ.get("TOKEN_SPEED_EXE", str(ROOT / "src-tauri" / "target" / "debug" / "token-speed.exe")))
DB = ROOT / "integration" / "settings-check.db"
OUT = ROOT / "integration"
PORT = 9335
user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
user32.FindWindowW.restype = wintypes.HWND
user32.GetParent.restype = wintypes.HWND
user32.SetProcessDPIAware()

class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [("biSize", wintypes.DWORD), ("biWidth", wintypes.LONG), ("biHeight", wintypes.LONG), ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD), ("biSizeImage", wintypes.DWORD), ("biXPelsPerMeter", wintypes.LONG), ("biYPelsPerMeter", wintypes.LONG), ("biClrUsed", wintypes.DWORD), ("biClrImportant", wintypes.DWORD)]
class BITMAPINFO(ctypes.Structure):
    _fields_ = [("bmiHeader", BITMAPINFOHEADER), ("bmiColors", wintypes.DWORD * 3)]

def find_window(title):
    found = []
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def callback(hwnd, _):
        length = user32.GetWindowTextLengthW(hwnd)
        value = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, value, length + 1)
        class_name = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, class_name, 256)
        pid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if value.value == title and class_name.value == "Tauri Window" and pid.value == active_pid and user32.IsWindowVisible(hwnd): found.append(hwnd)
        return True
    cb = callback_type(callback)
    user32.EnumWindows(cb, 0)
    taskbar = user32.FindWindowW("Shell_TrayWnd", None)
    if taskbar: user32.EnumChildWindows(taskbar, cb, 0)
    return found[0] if found else None

def rect(hwnd):
    value = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(value))
    return value.left, value.top, value.right, value.bottom

def capture(hwnd, name):
    left, top, right, bottom = rect(hwnd)
    width, height = right - left, bottom - top
    window_dc = user32.GetWindowDC(hwnd)
    memory_dc = gdi32.CreateCompatibleDC(window_dc)
    bitmap = gdi32.CreateCompatibleBitmap(window_dc, width, height)
    previous = gdi32.SelectObject(memory_dc, bitmap)
    if not user32.PrintWindow(hwnd, memory_dc, 2):
        raise RuntimeError("PrintWindow 捕获失败")
    info = BITMAPINFO()
    info.bmiHeader.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    info.bmiHeader.biWidth = width
    info.bmiHeader.biHeight = -height
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    buffer = ctypes.create_string_buffer(width * height * 4)
    if not gdi32.GetDIBits(memory_dc, bitmap, 0, height, buffer, ctypes.byref(info), 0):
        raise RuntimeError("GetDIBits 捕获失败")
    gdi32.SelectObject(memory_dc, previous)
    gdi32.DeleteObject(bitmap)
    gdi32.DeleteDC(memory_dc)
    user32.ReleaseDC(hwnd, window_dc)
    path = OUT / f"{name}.png"
    Image.frombuffer("RGBA", (width, height), buffer, "raw", "BGRA", 0, 1).save(path)
    if "taskbar" in name:
        user32.SetCursorPos(left + width // 2, 1079)
        time.sleep(0.8)
    else:
        user32.SetForegroundWindow(hwnd)
        time.sleep(0.3)
    screen = ImageGrab.grab(all_screens=True)
    if name == "41-taskbar-transparent-both":
        screen.save(OUT / "40-full-desktop.png")
    screen.crop((left, top, right, bottom)).save(OUT / f"{name}-screen.png")
    print(f"截图 {name}: {width}x{height}")
    return path

async def cdp(socket_url, method, params=None):
    async with websockets.connect(socket_url, origin=f"http://127.0.0.1:{PORT}") as socket:
        await socket.send(json.dumps({"id": 1, "method": method, "params": params or {}}))
        while True:
            message = json.loads(await socket.recv())
            if message.get("id") == 1:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message.get("result", {})

async def evaluate(socket_url, expression):
    result = await cdp(socket_url, "Runtime.evaluate", {"expression": expression, "returnByValue": True, "awaitPromise": True})
    if "exceptionDetails" in result:
        raise RuntimeError(result["exceptionDetails"])
    return result.get("result", {}).get("value")

def page_sockets():
    with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json", timeout=2) as response:
        return [page["webSocketDebuggerUrl"] for page in json.load(response) if page.get("type") == "page"]

async def window_socket(label):
    for socket_url in page_sockets():
        try:
            value = await evaluate(socket_url, "window.__TAURI__?.window.getCurrentWindow().label ?? null")
            if value == label:
                return socket_url
        except Exception:
            pass
    return None

def launch():
    global active_pid
    process = subprocess.Popen([str(EXE)], cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
    active_pid = process.pid
    main = None
    main_socket = None
    for _ in range(75):
        time.sleep(0.2)
        if process.poll() is not None:
            raise RuntimeError(f"应用提前退出：{process.stdout.read()}")
        main = main or find_window("Token Speed")
        try:
            main_socket = main_socket or asyncio.run(window_socket("main"))
        except Exception:
            pass
        if main and main_socket:
            return process, main, main_socket
    process.terminate()
    raise RuntimeError("任务栏窗口未就绪")

def stop(process):
    if process.poll() is None:
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)

import shutil
import base64
SETTINGS = OUT / "settings-check.json"
if SETTINGS.exists(): SETTINGS.unlink()
shutil.copyfile(OUT / "cc-switch.db", DB)
env = os.environ.copy()
env["CC_SWITCH_DB"] = str(DB)
env["WEBVIEW2_USER_DATA_FOLDER"] = str(OUT / "settings-webview")
env["TOKEN_SPEED_SETTINGS"] = str(SETTINGS)
env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={PORT}"
process = None

def js(socket, expression): return asyncio.run(evaluate(socket, expression))
def invoke(socket, command, args=None):
    return js(socket, f"window.__TAURI__.core.invoke({json.dumps(command)}, {json.dumps(args or {})})")
def settings(socket, **patch):
    value = invoke(socket, "set_monitor_settings", {"patch": patch})
    time.sleep(.25)
    return value

def screenshot(socket, name):
    data = asyncio.run(cdp(socket, "Page.captureScreenshot", {"format":"png"}))
    (OUT / f"{name}.png").write_bytes(base64.b64decode(data["data"]))

def change(socket, selector, value):
    js(socket, f"(() => {{const e=document.querySelector({json.dumps(selector)}); if(e.type === 'checkbox') e.checked={json.dumps(value)}; else e.value={json.dumps(value)}; e.dispatchEvent(new Event('change'));}})()")
    time.sleep(.4)
    assert js(socket,"window.__TAURI__.window.getCurrentWindow().isVisible()"), f'Changing {selector} unexpectedly closed settings'

def open_settings(ms):
    invoke(ms,'show_settings'); time.sleep(.25)
    hwnd = find_window('Token Speed 设置')
    return hwnd, asyncio.run(window_socket('settings'))

def click_tray_icon():
    found=[]
    def visit(hwnd,_):
        cls=ctypes.create_unicode_buffer(100);pid=wintypes.DWORD()
        user32.GetClassNameW(hwnd,cls,100);user32.GetWindowThreadProcessId(hwnd,ctypes.byref(pid))
        if pid.value==active_pid and cls.value=='tray_icon_app':found.append(hwnd)
        return True
    cb=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)(visit)
    user32.EnumWindows(cb,0)
    assert found, 'Tray callback window not found'
    class GUID(ctypes.Structure):
        _fields_=[('a',wintypes.DWORD),('b',wintypes.WORD),('c',wintypes.WORD),('d',ctypes.c_ubyte*8)]
    class NID(ctypes.Structure):
        _fields_=[('size',wintypes.DWORD),('hwnd',wintypes.HWND),('id',wintypes.UINT),('guid',GUID)]
    shell=ctypes.windll.shell32
    for ident in range(1,20):
        nid=NID();nid.size=ctypes.sizeof(NID);nid.hwnd=found[0];nid.id=ident;r=wintypes.RECT()
        if shell.Shell_NotifyIconGetRect(ctypes.byref(nid),ctypes.byref(r))==0:
            print('Tray rect:', ident,(r.left,r.top,r.right,r.bottom))
            if r.left>=0 and r.top>=0 and r.right>r.left:
                # Actual screen input if the icon is in the visible notification area.
                user32.SetCursorPos((r.left+r.right)//2,(r.top+r.bottom)//2)
                user32.mouse_event(0x0002,0,0,0,0);user32.mouse_event(0x0004,0,0,0,0)
            else:
                # Hidden overflow icon: deliver Windows' left-release notification
                # to its real native callback, exercising the same tray handler.
                user32.SendMessageW(found[0],6002,ident,0x0202)
            time.sleep(.35)
            if not find_window('Token Speed 设置'):
                user32.SendMessageW(found[0],6002,ident,0x0202)
                time.sleep(.35)
            return
    raise RuntimeError('Tray notification icon is not registered')

def insert_record(ident, speed):
    with sqlite3.connect(DB) as db:
        db.execute("INSERT INTO proxy_request_logs (request_id,app_type,model,input_tokens,output_tokens,latency_ms,first_token_ms,duration_ms,status_code,is_streaming,data_source,created_at) VALUES (?,'codex','integration',100,?,11000,1000,10500,200,1,'integration',?)", (ident,speed*10,int(time.time())))


try:
    process, main, ms = launch(); time.sleep(1.2)
    assert user32.GetParent(main)==user32.FindWindowW('Shell_TrayWnd',None)
    cfg=invoke(ms,'get_taskbar_settings')
    assert cfg['idleAfterSeconds']==15 and cfg['chartWidth']==40
    geometry=invoke(ms,'get_monitor_geometry')
    assert geometry['width']==142, geometry
    print('Compact taskbar width:',geometry['width'])
    gaps=js(ms,"[...document.querySelectorAll('.taskbar-row')].map(r=>r.querySelector('.taskbar-speed').getBoundingClientRect().left-r.querySelector('.taskbar-app-label').getBoundingClientRect().right)")
    assert all(0<=gap<=5 for gap in gaps)
    assert js(ms,"[...document.querySelectorAll('.taskbar-row')].every(r=>r.dataset.status==='idle')")
    assert 't/s' not in js(ms,"document.querySelector('#display-slot').innerText")
    assert js(ms,"document.querySelectorAll('.chart-axis').length")==2
    screenshot(ms,'60-compact-idle-axes')
    # Clicking the monitor opens settings directly.
    js(ms,"document.querySelector('#compact-main').click()")
    time.sleep(.3)
    sh=find_window('Token Speed 设置'); ss=asyncio.run(window_socket('settings'))
    assert sh and ss
    assert js(ss,"!document.querySelector('#settings-panel').hidden && !document.querySelector('#overview-view')")
    assert js(ss,"getComputedStyle(document.querySelector('#settings-panel')).backgroundColor") == 'rgb(36, 36, 38)'
    assert js(ss,"!document.querySelector('#avoid-widgets')")
    layout=js(ss,"(()=>{let e=document.querySelector('.settings-content');return {height:e.clientHeight,content:e.scrollHeight}})()")
    assert layout['content']<=layout['height']+1, layout
    screenshot(ss,'70-settings-display')
    js(ss,"document.querySelector('#preferences-tab').click()")
    screenshot(ss,'71-settings-preferences')
    js(ss,"document.querySelector('#display-tab').click()")
    # Left clicking the registered notification icon opens settings directly.
    invoke(ms,'hide_settings')
    click_tray_icon()
    assert user32.IsWindowVisible(sh), 'Left tray click did not open settings'
    # Explicit close and reopening are reliable.
    js(ss,"document.querySelector('#close-settings').click()")
    time.sleep(.2); assert not user32.IsWindowVisible(sh)
    sh,ss=open_settings(ms)
    # Native select popup must not trigger dismiss-on-blur.
    select_rect=js(ss,"(()=>{let r=document.querySelector('#display-mode').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()")
    for t in ['mousePressed','mouseReleased']:
        asyncio.run(cdp(ss,'Input.dispatchMouseEvent',{'type':t,**select_rect,'button':'left','buttons':1 if t=='mousePressed' else 0,'clickCount':1}))
    time.sleep(.4); assert user32.IsWindowVisible(sh),'Opening a select dismissed settings'
    user32.keybd_event(0x1B,0,0,0); user32.keybd_event(0x1B,0,2,0)
    time.sleep(.2)
    assert user32.IsWindowVisible(sh), 'Closing native dropdown also closed settings'
    # Use a real unrelated native window to verify outside focus closes settings.
    user32.CreateWindowExW.restype=wintypes.HWND
    external=user32.CreateWindowExW(0,'STATIC','Token Speed integration focus target',0x10CF0000,200,200,200,120,None,None,None,None)
    try:
        user32.SetForegroundWindow(external)
        time.sleep(.5)
        assert not user32.IsWindowVisible(sh),'Settings stayed visible after outside focus'
    finally: user32.DestroyWindow(external)
    sh,ss=open_settings(ms)
    # 1px manual adjustment is stable through periodic window maintenance.
    change(ss,'#taskbar-offset',-180)
    cfg=invoke(ms,'get_taskbar_settings'); assert cfg['offset']==-180
    x=rect(main)[0]
    js(ss,"document.querySelector('[data-nudge=\"1\"]').click()")
    time.sleep(.5); assert rect(main)[0]==x+1,(x,rect(main))
    time.sleep(1.1); assert rect(main)[0]==x+1
    change(ss,'#chart-width',60)
    assert invoke(ms,'get_monitor_geometry')['width']==162
    # Completion arrives, and each app's axis auto-scales from its own history.
    insert_record('fresh-speed',777)
    time.sleep(1.2)
    assert '777' in js(ms,"document.querySelector('.taskbar-row.codex').innerText")
    axis=js(ms,"document.querySelector('.codex .taskbar-sparkline').getAttribute('aria-label')")
    assert '1000' in axis and '500' in axis,axis
    screenshot(ms,'62-recent-speed-axes')
    time.sleep(15.5)
    assert js(ms,"document.querySelector('.codex').dataset.status")== 'idle'
    assert '777' in js(ms,"document.querySelector('.codex').title")
    print('New completion shown; after 15 seconds it becomes waiting with last result in tooltip')
    sh,ss=open_settings(ms)
    change(ss,'#window-mode','floating')
    assert not user32.GetParent(main)
    assert user32.GetWindowLongW(main,-20)&8
    # Native drag in both dimensions.
    invoke(ms,'hide_settings'); time.sleep(.3); before=rect(main)
    user32.SetCursorPos(before[0]+60,before[1]+15); user32.mouse_event(0x0002,0,0,0,0)
    time.sleep(.1); user32.SetCursorPos(before[0]+50,before[1]+10); time.sleep(.2)
    user32.SetCursorPos(before[0]-60,before[1]-85); time.sleep(.2); user32.mouse_event(0x0004,0,0,0,0)
    time.sleep(1.2); after=rect(main)
    assert abs(after[0]-before[0])>50 and abs(after[1]-before[1])>50,(before,after,invoke(ms,'get_taskbar_settings'))
    sh,ss=open_settings(ms)
    js(ss,"document.querySelector('#preferences-tab').click()")
    change(ss,'#position-locked',True)
    invoke(ms,'hide_settings'); time.sleep(.3); before=rect(main)
    user32.SetCursorPos(before[0]+60,before[1]+15); user32.mouse_event(0x0002,0,0,0,0)
    user32.SetCursorPos(before[0]-50,before[1]-60); time.sleep(.15); user32.mouse_event(0x0004,0,0,0,0)
    time.sleep(.3); assert rect(main)==before
    sh,ss=open_settings(ms)
    change(ss,'#position-locked',False); change(ss,'#theme','light'); change(ss,'#opacity',65)
    change(ss,'#font-size',16); screenshot(ms,'63-floating-light-axes')
    change(ss,'#always-on-top',False); assert not user32.GetWindowLongW(main,-20)&8
    js(ss,"document.querySelector('#display-tab').click()")
    change(ss,'#show-chart',False); assert js(ms,"document.querySelectorAll('.chart-axis').length")==0
    for mode,count in [('claude',1),('codex',1),('both',2)]:
        change(ss,'#display-mode',mode)
        assert js(ms,"document.querySelectorAll('.taskbar-row').length")==count
        assert js(ms,"document.querySelectorAll('.taskbar-app-label').length") == (2 if mode=='both' else 0)
    change(ss,'#font-size',11)
    change(ss,'#show-chart',True)
    change(ss,'#display-mode','codex')
    assert invoke(ms,'get_monitor_geometry')['width'] == 156 # floating, chart width 60, one app
    screenshot(ms,'73-single-app')
    cfg=invoke(ms,'get_taskbar_settings')
    invalid=js(ms,"window.__TAURI__.core.invoke('set_monitor_settings',{patch:{chartWidth:10}}).then(()=>false,()=>true)")
    assert invalid and invoke(ms,'get_taskbar_settings')==cfg
    time.sleep(1.1); stop(process)
    process,main,ms=launch(); time.sleep(1.2)
    assert invoke(ms,'get_taskbar_settings')==cfg
    assert not user32.GetParent(main)
    settings(ms,windowMode='taskbar',fontSize=11,showChart=True,chartWidth=40)
    assert user32.GetParent(main)==user32.FindWindowW('Shell_TrayWnd',None)
    sh,ss=open_settings(ms)
    asyncio.run(cdp(ss,'Input.dispatchKeyEvent',{'type':'keyDown','key':'Escape','code':'Escape','windowsVirtualKeyCode':27}))
    time.sleep(.3); assert not user32.IsWindowVisible(sh)
    print('PASS: short width, exact 1px position, settings, outside-dismiss, native dropdown, close/Esc, fresh/idle states, axes and scaling, floating drag/lock, appearance, persistence')
finally:
    if process is not None: stop(process)
