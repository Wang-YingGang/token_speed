"""Rebuild platform icon formats from the approved raster source."""
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parent
subprocess.run(["npx.cmd", "tauri", "icon", str(root / "assets" / "icon-source.png")], cwd=root, check=True)
shutil.copyfile(root / "src-tauri" / "icons" / "32x32.png", root / "ui" / "app-icon.png")
