# README 图片

原始截图来自当前源码编译的 Windows 原生程序，由 `scripts/capture-docs.py` 通过 WebView 截图接口直接采集；没有重新绘制界面。截图使用独立数据库中的固定示例记录（Claude 88.8 t/s、Codex 77.7 t/s）和独立配置，不采集用户数据库或桌面内容。

- `taskbar.png`：双应用任务栏监控条，使用深色背景展示透明窗口。
- `floating-dark.png` / `floating-light.png`：深浅色桌面悬浮条。
- `settings-display.png` / `settings-preferences.png`：显示和偏好设置页。
- `capture.json`：版本、采集时间、源码提交及界面源文件摘要。
- `product-showcase.png`：以当前截图为素材制作的 README 首屏展示图，使用内置 ImageGen 进行排版。准确的界面细节以原始截图为准。
- `product-showcase-prompt.txt`：展示图使用的完整提示词。

截图应随界面变化重新采集；更新原始截图后也应同步更新首屏展示图。
