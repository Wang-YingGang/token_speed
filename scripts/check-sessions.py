"""Verify session output inside the existing speed bar, using isolated fixtures."""
import asyncio
import base64
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

PORT = 9337
WORK = ROOT / "integration" / "sessions" / str(time.time_ns())
WORK.mkdir(parents=True)
CODEX = WORK / "codex"
CLAUDE = WORK / "claude"
CODEX.mkdir()
CLAUDE.mkdir()
NOW = int(time.time() * 1000)


def event(kind, **payload):
    return {"type": "event_msg", "timestamp": int(time.time() * 1000), "payload": {"type": kind, **payload}}


def meta(ident, project):
    return {"type": "session_meta", "payload": {"id": ident, "cwd": f"E:/work/{project}"}}


def reply(ident, text):
    return {"type": "response_item", "timestamp": int(time.time() * 1000), "payload": {
        "type": "message", "role": "assistant", "phase": "commentary", "id": ident,
        "content": [{"type": "output_text", "text": text}]}}


def write(path, rows, mode="w"):
    with path.open(mode, encoding="utf-8", newline="\n") as handle:
        for row in rows:
            handle.write(json.dumps(row, ensure_ascii=False) + "\n")


async def cdp(url, method, params=None):
    async with websockets.connect(url, origin=f"http://127.0.0.1:{PORT}", max_size=8 * 1024 * 1024) as socket:
        await socket.send(json.dumps({"id": 1, "method": method, "params": params or {}}))
        while True:
            result = json.loads(await socket.recv())
            if result.get("id") == 1:
                if "error" in result:
                    raise RuntimeError(result["error"])
                return result.get("result", {})


def js(url, expression):
    result = asyncio.run(cdp(url, "Runtime.evaluate", {"expression": expression, "returnByValue": True, "awaitPromise": True}))
    if "exceptionDetails" in result:
        raise RuntimeError(result["exceptionDetails"])
    return result.get("result", {}).get("value")


def invoke(url, name, args=None):
    return js(url, f"window.__TAURI__.core.invoke({json.dumps(name)}, {json.dumps(args or {})})")


def socket_for(label):
    with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json", timeout=1) as response:
        pages = json.load(response)
    for page in pages:
        if page.get("type") == "page":
            url = page["webSocketDebuggerUrl"]
            if js(url, "window.__TAURI__?.window.getCurrentWindow().label") == label:
                return url


def wait_for(check, description, timeout=12):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        value = check()
        if value:
            return value
        time.sleep(.2)
    raise AssertionError(description)


def screenshot(url, name):
    data = asyncio.run(cdp(url, "Page.captureScreenshot", {"format": "png"}))
    (WORK / f"{name}.png").write_bytes(base64.b64decode(data["data"]))


codex_file = CODEX / "live.jsonl"
write(codex_file, [meta("codex-live", "token_speed"), event("task_started", turn_id="turn-live"),
    event("item_completed", item={"type": "AgentMessage", "id": "first", "phase": "commentary", "content": [{"type": "text", "text": "正在检查会话日志，准备运行测试。"}]}),
    reply("first", "正在检查会话日志，准备运行测试。"),
    event("item_completed", item={"type": "CommandExecution", "id": "command", "command": "cargo test", "aggregated_output": "20 passed; 0 failed"})])
write(CODEX / "completed.jsonl", [meta("codex-done", "example_app"), event("task_started", turn_id="done"), reply("done", "配置修改完成，验证通过。"), event("task_complete", turn_id="done")])
write(CODEX / "stale.jsonl", [meta("codex-stale", "old_task"), {"type":"event_msg","timestamp":NOW-12*60*1000,"payload":{"type":"task_started","turn_id":"stale"}}])
write(CLAUDE / "live.jsonl", [
    {"type":"user","sessionId":"claude-live","uuid":"user","cwd":"E:/work/myproject","timestamp":NOW,"message":{"content":"开始测试"}},
    {"type":"assistant","uuid":"assistant","timestamp":NOW,"message":{"stop_reason":"tool_use","content":[{"type":"text","text":"已经找到配置问题，正在验证修复。"},{"type":"tool_use","id":"bash","name":"Bash"}]}},
    {"type":"user","timestamp":NOW,"message":{"content":[{"type":"tool_result","tool_use_id":"bash","content":"tests passed"}]}}])

db = WORK / "usage.db"
with sqlite3.connect(db) as connection:
    connection.execute("CREATE TABLE proxy_request_logs (request_id TEXT, app_type TEXT, model TEXT, output_tokens INTEGER, latency_ms INTEGER, first_token_ms INTEGER, duration_ms INTEGER, created_at INTEGER, status_code INTEGER)")
    for app in ("claude", "codex"):
        connection.execute("INSERT INTO proxy_request_logs VALUES (?,?,?,800,11000,1000,10000,?,200)", (app, app, "demo", int(time.time())))
settings = WORK / "settings.json"
settings.write_text(json.dumps({"windowMode":"floating","floatingX":800,"floatingY":700}), encoding="utf-8")
env = os.environ.copy()
env.update(CC_SWITCH_DB=str(db), TOKEN_SPEED_SETTINGS=str(settings), TOKEN_SPEED_CODEX_SESSIONS=str(CODEX), TOKEN_SPEED_CLAUDE_PROJECTS=str(CLAUDE),
           WEBVIEW2_USER_DATA_FOLDER=str(WORK / "webview"), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f"--remote-debugging-port={PORT}")
exe = Path(os.environ.get("TOKEN_SPEED_EXE", str(ROOT / "src-tauri/target/debug/token-speed.exe")))
with (WORK / "process.log").open("w", encoding="utf-8") as output:
    process = subprocess.Popen([str(exe)], cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
    try:
        def ready():
            if process.poll() is not None:
                raise RuntimeError("Native app exited before becoming ready")
            try:
                return socket_for("main")
            except (OSError, RuntimeError):
                return None
        main = wait_for(ready, "main window did not load", 25)
        wait_for(lambda: len(invoke(main, "get_sessions")["sessions"]) == 4, "fixture sessions not discovered")
        preview = "document.querySelector('.codex .session-output')"
        inline = "document.querySelector('.codex .session-inline')"
        wait_for(lambda: "20 passed" in js(main, f"{preview}.textContent"), "tool output missing from speed bar")
        assert "80.0" in js(main, "document.querySelector('.codex .taskbar-speed').textContent")
        assert js(main, "document.querySelectorAll('.chart-axis').length") == 2
        assert js(main, "document.querySelector('.codex').dataset.sessionStatus") == "running"
        assert "tests passed" in js(main, "document.querySelector('.claude .session-output').textContent")
        assert socket_for("sessions") is None, "an unexpected session panel exists"
        assert invoke(main, "get_monitor_geometry")["width"] == 402
        assert len(next(s for s in invoke(main, "get_sessions")["sessions"] if s["id"] == "codex-live")["entries"]) == 2
        write(codex_file, [reply("preview", "正在检查会话日志，准备运行测试。")], "a")
        write(CLAUDE / "live.jsonl", [{"type":"assistant","uuid":"preview","timestamp":int(time.time()*1000),"message":{"stop_reason":"tool_use","content":[{"type":"text","text":"已经找到配置问题，正在验证修复。"}]}}], "a")
        wait_for(lambda: "正在检查" in js(main, f"{preview}.textContent"), "assistant reply missing from speed bar")
        screenshot(main, "01-inline-floating")
        # The taskbar shows output in the same two rows with speed and chart.
        invoke(main, "set_monitor_settings", {"patch":{"windowMode":"taskbar"}})
        wait_for(lambda: invoke(main, "get_monitor_geometry")["width"] == 370, "taskbar width is incorrect")
        assert js(main, "document.querySelector('.session-inline').getBoundingClientRect().width") == 220
        assert js(main, "[...document.querySelectorAll('.session-inline')].every(e=>{let a=e.getBoundingClientRect(), b=document.querySelector('#monitor').getBoundingClientRect();return a.right<=b.right&&a.top>=b.top&&a.bottom<=b.bottom})")
        screenshot(main, "02-inline-taskbar")
        # Text is rendered literally; an apparent HTML tag cannot execute.
        text = '<img src=x onerror="window.injected=true"> 增量输出已到达'
        write(codex_file, [reply("next", text)], "a")
        wait_for(lambda: "增量输出已到达" in js(main, f"{preview}.textContent"), "appended reply not shown inline")
        assert not js(main, "document.querySelector('.session-output img') !== null || window.injected === true")
        wait_for(lambda: js(main, f"{preview}.classList.contains('is-scrolling')"), "long inline text is not scrolling")
        js(main, f"window.savedOutput={preview}")
        write(codex_file, [event("token_count")], "a")
        time.sleep(1.1)
        assert js(main, f"window.savedOutput === {preview}"), "heartbeat restarted the text animation"
        # Partial UTF-8/JSON must wait for a newline and survive an unchanged mtime.
        raw = (json.dumps(reply("partial", "中文半行续写成功"), ensure_ascii=False) + "\n").encode()
        index = raw.index("中".encode()) + 1
        stat = codex_file.stat()
        with codex_file.open("ab") as handle:
            handle.write(raw[:index])
        os.utime(codex_file, ns=(stat.st_atime_ns, stat.st_mtime_ns))
        time.sleep(1.1)
        assert "增量输出已到达" in js(main, f"{preview}.textContent")
        with codex_file.open("ab") as handle:
            handle.write(raw[index:])
        os.utime(codex_file, ns=(stat.st_atime_ns, stat.st_mtime_ns))
        wait_for(lambda: "中文半行续写成功" in js(main, f"{preview}.textContent"), "partial line lost")
        # A newly discovered session joins the row's rotation, not another window.
        write(CODEX / "new.jsonl", [meta("new-session", "new_project"), event("task_started", turn_id="new"), reply("hello", "新会话已发现")])
        wait_for(lambda: len(invoke(main, "get_sessions")["sessions"]) == 5, "new session not discovered")
        wait_for(lambda: "/2" in js(main, "document.querySelector('.codex .session-project').textContent"), "session count missing")
        before_switch = js(main, f"{inline}.dataset.sessionId")
        js(main, f"{inline}.dispatchEvent(new WheelEvent('wheel',{{deltaY:100,bubbles:true,cancelable:true}}))")
        wait_for(lambda: js(main, f"{inline}.dataset.sessionId") != before_switch, "wheel did not switch session")
        assert "/2" in js(main, "document.querySelector('.codex .session-project').textContent")
        js(main, f"{inline}.dispatchEvent(new WheelEvent('wheel',{{deltaY:-100,bubbles:true,cancelable:true}}))")
        wait_for(lambda: js(main, f"{inline}.dataset.sessionId") == before_switch, "reverse wheel did not switch back")
        # The automatic rotation works without opening another window.
        asyncio.run(cdp(main, "Input.dispatchMouseEvent", {"type":"mouseMoved","x":-1,"y":-1}))
        wait_for(lambda: js(main, f"{inline}.dataset.sessionId") != before_switch, "automatic session rotation did not run", 10)
        # Complete the selected round; another running session immediately takes over.
        write(codex_file, [event("task_complete", turn_id="turn-live")], "a")
        wait_for(lambda: js(main, f"{inline}.dataset.sessionId") == "new-session", "completed round remained selected")
        # Left click still opens the existing settings window.
        js(main, "document.querySelector('#compact-main').click()")
        config = wait_for(lambda: socket_for("settings"), "settings window not reachable")
        assert js(config, "window.__TAURI__.window.getCurrentWindow().isVisible()")
        invoke(main, "set_monitor_settings", {"patch":{"showSessionOutput":False}})
        wait_for(lambda: js(main, "document.querySelectorAll('.session-inline').length") == 0, "inline output toggle failed")
        assert invoke(main, "get_monitor_geometry")["width"] == 142
        invoke(main, "set_monitor_settings", {"patch":{"showSessionOutput":True,"sessionOutputWidth":300,"displayMode":"codex","windowMode":"floating","fontSize":16,"chartWidth":80}})
        wait_for(lambda: js(main, "document.querySelectorAll('.taskbar-row').length") == 1, "single app mode failed")
        assert js(main, "document.querySelector('.session-inline').getBoundingClientRect().width") == 300
        assert js(main, "document.querySelector('#monitor').scrollWidth <= document.querySelector('#monitor').clientWidth"), "inline content overflows"
        screenshot(main, "03-single-app-large-font")
        invoke(main, "set_monitor_settings", {"patch":{"showSessionOutput":True,"sessionOutputWidth":220,"displayMode":"both","fontSize":11,"chartWidth":40}})
        invoke(main, "hide_settings")
        # Text boxes can fit inside the bar while the glyphs themselves are
        # clipped. Check font bounds against every vertical clipping ancestor.
        write(CODEX / "new.jsonl", [meta("new-session", "projectmp client pgyjq"), event("task_started", turn_id="glyphs"), reply("glyphs", "pgyjq 中文输出：字母下沿应完整显示")])
        wait_for(lambda: "pgyjq" in js(main, "document.querySelector('.codex .session-project').textContent"), "descender fixture not loaded")
        glyph_check = """(() => {
            const failures=[];
            for(const e of document.querySelectorAll('.session-project,.session-output')) {
                if(!e.textContent) continue;
                const range=document.createRange(); range.selectNodeContents(e);
                const text=range.getBoundingClientRect();
                for(let clip=e; clip; clip=clip.parentElement) {
                    if(!['hidden','clip','auto','scroll'].includes(getComputedStyle(clip).overflowY)) continue;
                    const box=clip.getBoundingClientRect();
                    if(text.top<box.top-.5 || text.bottom>box.bottom+.5) failures.push({
                        text:e.textContent,element:e.className,clip:clip.className||clip.tagName,
                        textTop:text.top,textBottom:text.bottom,clipTop:box.top,clipBottom:box.bottom});
                }
            }
            return failures;
        })()"""
        for mode in ("codex", "both"):
            for font in (10, 11, 16):
                invoke(main, "set_monitor_settings", {"patch":{"displayMode":mode,"fontSize":font}})
                js(main, "document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))")
                failures=js(main,glyph_check)
                screenshot(main, f"05-glyphs-{mode}-{font}")
                assert not failures, {"mode":mode,"font":font,"clippedGlyphs":failures}
        invoke(main, "set_monitor_settings", {"patch":{"displayMode":"codex","fontSize":11}})
        screenshot(main, "06-descenders-fixed")
        # Removing feeds returns the inline region to its waiting state.
        for path in list(CODEX.glob("*.jsonl")) + list(CLAUDE.glob("*.jsonl")):
            path.unlink()
        wait_for(lambda: not invoke(main, "get_sessions")["sessions"], "deleted feeds not removed")
        wait_for(lambda: "等待会话输出" in js(main, f"{preview}.textContent"), "inline empty state not shown")
        screenshot(main, "04-inline-empty")
        print("Native inline session tests passed: speed + chart + output, taskbar/floating, append, deduplication, partial UTF-8, unchanged mtime, scrolling, stable animation, wheel switching, completion, width, toggle, single-app mode and empty state.")
        print(f"Artifacts: {WORK}")
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
