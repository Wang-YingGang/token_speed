"""Capture the current native Tauri UI with isolated, synthetic demo data."""

from pathlib import Path
import argparse
import asyncio
import base64
import hashlib
import json
import os
import sqlite3
import subprocess
import sys
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "integration" / ".deps"))
import websockets


async def capture(executable, port):
    output = ROOT / "docs" / "images"
    output.mkdir(parents=True, exist_ok=True)
    work = ROOT / "integration" / "docs-capture" / str(time.time_ns())
    work.mkdir(parents=True)
    database = work / "demo.db"
    settings_file = work / "settings.json"
    settings_file.write_text(json.dumps({"idleAfterSeconds": 60}), encoding="utf-8")
    with sqlite3.connect(database) as connection:
        connection.execute("""CREATE TABLE proxy_request_logs (
            request_id TEXT, app_type TEXT, model TEXT, input_tokens INTEGER,
            output_tokens INTEGER, latency_ms INTEGER, first_token_ms INTEGER,
            duration_ms INTEGER, status_code INTEGER, is_streaming INTEGER,
            data_source TEXT, created_at INTEGER)""")

    counter = 0

    async def cdp(socket_url, method, params=None):
        nonlocal counter
        counter += 1
        request_id = counter
        async with websockets.connect(socket_url, origin=f"http://127.0.0.1:{port}") as socket:
            await socket.send(json.dumps({"id": request_id, "method": method, "params": params or {}}))
            while True:
                message = json.loads(await socket.recv())
                if message.get("id") == request_id:
                    if "error" in message:
                        raise RuntimeError(message["error"])
                    return message.get("result", {})

    async def evaluate(socket_url, expression):
        result = await cdp(socket_url, "Runtime.evaluate", {
            "expression": expression, "returnByValue": True, "awaitPromise": True,
        })
        if "exceptionDetails" in result:
            raise RuntimeError(result["exceptionDetails"])
        return result.get("result", {}).get("value")

    async def invoke(socket_url, command, args=None):
        return await evaluate(socket_url, f"window.__TAURI__.core.invoke({json.dumps(command)}, {json.dumps(args or {})})")

    async def configure(**patch):
        result = await invoke(main, "set_monitor_settings", {"patch": patch})
        await asyncio.sleep(0.35)
        return result

    async def screenshot(socket_url, filename, width, height, background=None):
        await cdp(socket_url, "Emulation.setDeviceMetricsOverride", {
            "width": width, "height": height, "deviceScaleFactor": 3, "mobile": False,
        })
        await cdp(socket_url, "Emulation.setDefaultBackgroundColorOverride", {
            "color": background or {"r": 0, "g": 0, "b": 0, "a": 0},
        })
        if background:
            color = f"rgb({background['r']}, {background['g']}, {background['b']})"
            await evaluate(socket_url, f"document.body.style.backgroundColor = {json.dumps(color)}")
        else:
            await evaluate(socket_url, "document.body.style.backgroundColor = ''")
        await evaluate(socket_url, "document.fonts.ready.then(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))")
        result = await cdp(socket_url, "Page.captureScreenshot", {
            "format": "png", "clip": {"x": 0, "y": 0, "width": width, "height": height, "scale": 3},
        })
        (output / filename).write_bytes(base64.b64decode(result["data"]))
        print(f"Captured {filename}: {width} x {height} CSS pixels at 3x")

    environment = os.environ.copy()
    environment.update({
        "CC_SWITCH_DB": str(database),
        "TOKEN_SPEED_SETTINGS": str(settings_file),
        "WEBVIEW2_USER_DATA_FOLDER": str(work / "webview"),
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS": f"--remote-debugging-port={port}",
    })
    with (work / "app.log").open("w", encoding="utf-8") as log:
        process = subprocess.Popen([str(executable)], cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT)
        main = None
        try:
            sockets = {}
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError("Demo app exited; inspect " + str(work / "app.log"))
                try:
                    with urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=1) as response:
                        pages = json.load(response)
                    for page in pages:
                        socket_url = page.get("webSocketDebuggerUrl")
                        if not socket_url or page.get("type") != "page":
                            continue
                        label = await evaluate(socket_url, "window.__TAURI__?.window.getCurrentWindow().label")
                        if label:
                            sockets[label] = socket_url
                    if "main" in sockets and "settings" in sockets:
                        break
                except (OSError, ValueError, RuntimeError):
                    pass
                await asyncio.sleep(0.2)
            else:
                raise RuntimeError("Timed out waiting for native WebViews")
            main, settings = sockets["main"], sockets["settings"]
            # Wait for the empty snapshot before adding completions, so the demo
            # follows the same recent-record behavior as the running product.
            await asyncio.sleep(1)
            now = int(time.time())
            series = {
                "claude": [42, 48, 45, 62, 52, 71, 68, 55, 75, 61, 82, 73, 69, 84, 76, 81, 77, 86, 79, 88.8],
                "codex": [48, 55, 51, 65, 58, 68, 64, 72, 59, 67, 74, 69, 76, 72, 81, 75, 79, 73, 82, 77.7],
            }
            with sqlite3.connect(database) as connection:
                for app_type, speeds in series.items():
                    for index, speed in enumerate(speeds):
                        connection.execute(
                            "INSERT INTO proxy_request_logs VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                            (f"demo-{app_type}-{index}", app_type, "demo-model", 120,
                             round(speed * 10), 11000, 1000, 10000, 200, 1, "demo", now - (19-index)*60),
                        )
            for _ in range(40):
                text = await evaluate(main, "document.querySelector('#display-slot').innerText")
                if "88.8" in text and "77.7" in text:
                    break
                await asyncio.sleep(0.2)
            else:
                raise RuntimeError("Demo completion records were not rendered")

            # Confirm the executable really embeds the current checked-in UI.
            for socket_url in (main, settings):
                for filename in ("app.js", "styles.css"):
                    live = await evaluate(socket_url, f"fetch({json.dumps('./' + filename)}).then(response => response.text())")
                    expected = (ROOT / "ui" / filename).read_text(encoding="utf-8")
                    if live.replace("\r\n", "\n") != expected.replace("\r\n", "\n"):
                        raise RuntimeError(f"Executable embeds stale {filename}; rebuild before capturing")

            current_layout = await evaluate(settings, "Boolean(document.querySelector('#display-tab') && document.querySelector('#preferences-tab') && !document.querySelector('#overview-view'))")
            if not current_layout:
                raise RuntimeError("The current settings layout was not rendered")

            await configure(windowMode="taskbar", displayMode="both", fontSize=11, chartWidth=40, idleAfterSeconds=60)
            await screenshot(main, "taskbar.png", 142, 48, {"r": 25, "g": 27, "b": 34, "a": 1})
            await configure(windowMode="floating", theme="dark", fontSize=13, chartWidth=64, opacity=100)
            geometry = await invoke(main, "get_monitor_geometry")
            await screenshot(main, "floating-dark.png", round(geometry["width"]), 48)
            await configure(theme="light")
            await screenshot(main, "floating-light.png", round(geometry["width"]), 48)

            await invoke(main, "show_settings")
            await evaluate(settings, "document.querySelector('#display-tab').click()")
            await screenshot(settings, "settings-display.png", 344, 510)
            await evaluate(settings, "document.querySelector('#preferences-tab').click()")
            await screenshot(settings, "settings-preferences.png", 344, 510)

            version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
            manifest = {
                "version": version,
                "sourceCommit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "capturedAt": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
                "data": "Synthetic demo records; no user database or desktop content is captured.",
                "sourceHashes": {name: hashlib.sha256((ROOT / "ui" / name).read_bytes()).hexdigest() for name in ("app.js", "styles.css", "index.html")},
            }
            (output / "capture.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        finally:
            if process.poll() is None and main:
                try:
                    await invoke(main, "quit_app")
                except Exception:
                    pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, default=ROOT / "src-tauri" / "target" / "debug" / "token-speed.exe")
    parser.add_argument("--port", type=int, default=9346)
    args = parser.parse_args()
    if not args.exe.is_file():
        parser.error("Build the current debug executable first: cargo build --manifest-path src-tauri/Cargo.toml --locked")
    asyncio.run(capture(args.exe.resolve(), args.port))
