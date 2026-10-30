# Token Speed

Windows 上的 Claude / Codex 输出速度监控，基于 Rust + Tauri 2。只读 CC Switch 的用量数据库，显示最近完成的一轮请求及最近 20 轮曲线。

## 使用

- 左键单击托盘图标、任务栏监控或悬浮条，直接打开设置。设置分为“显示”和“偏好”两页。
- 点击其他应用、窗口外部，或按 Esc / 右上角关闭按钮，收起设置。
- 默认双应用条宽 142 个逻辑像素，单应用条宽 104px。单应用模式默认隐藏应用名称。曲线宽度可在 24–80px 之间调整，也可关闭。
- 曲线 Y 轴根据各自最近 20 轮速度自动选择刻度，例如 0 / 50 / 100 t/s；横轴从左到右由旧到新。
- 新记录到达后短暂显示完成速度，默认 15 秒后变为“待机”。可改为 30 / 60 秒。鼠标悬停查看上次速度、记录时间和模型。待机表示等待新的完成记录，不能据此判断是否有正在进行的 AI 请求。
- 水平偏移支持直接输入与 ±1 / ±5px 微调。0 是通知区域左侧，负数左移，正数右移；设置页显示当前宽度与屏幕 X 坐标。
- 拖动文字区可移动监控：任务栏内左右移动，悬浮条在桌面自由移动。位置和设置自动保存。
- 支持双应用 / 单应用、10–16px 字号、深浅色悬浮背景、背景不透明度、置顶、位置锁定和开机启动。
- 托盘菜单提供显示 / 隐藏、设置与退出。

数值单位为输出 token / 秒，优先以总延迟减首 token 延迟计算生成耗时。数据来自 CC Switch 已完成请求记录，不能测量正在生成中的实时 token 速度。

配置保存在 `%APPDATA%\TokenSpeed\settings.json`。旧版本设置自动补齐新增字段。保存的屏幕不可用时，悬浮窗口会回到可见区域。

## 开发和验证

```powershell
npm install
npm run dev
npm test
npm run build
```

原生集成验证使用隔离的 WebView 配置目录、数据库副本和设置文件，测试真实窗口拖动、模式切换和重启恢复，不修改正在使用的 CC Switch 数据库或应用设置：

```powershell
python -m pip install --target integration/.deps websockets pillow
python prepare_integration_db.py
cargo build --manifest-path src-tauri/Cargo.toml
npm run test:native
```

`integration/` 存放可重新生成的联调数据库、截图和 WebView 缓存，`src-tauri/target/` 存放构建产物，均不提交。临时调试文件统一放入已忽略的 `scratch/`、`tmp/` 或 `temp/` 目录。

可通过 `TOKEN_SPEED_EXE` 指定要验证的 EXE。`CC_SWITCH_DB` 可指定用量数据库；`TOKEN_SPEED_SETTINGS` 可指定设置文件。默认数据库为 `%USERPROFILE%\.cc-switch\cc-switch.db`。

构建产物：`src-tauri/target/release/token-speed.exe` 和 `src-tauri/target/release/bundle/nsis/Token Speed_0.6.0_x64-setup.exe`。

图标源文件为 `assets/icon-source.png`，设计提示词见 `assets/icon-design.md`；运行 `python generate_icon.py` 重建所有图标格式。

## 自动发布

推送 `v*` 版本标签后，GitHub Actions 会校验版本、运行 Rust / 前端测试、构建 Windows x64 安装包，然后发布到仓库的 Releases。附件包括 NSIS 安装包、便携版 ZIP 和 `SHA256SUMS.txt`。带预发布后缀的标签（如 `v0.7.0-beta.1`）会发布为 Pre-release。

发布前保持 `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 和 `src-tauri/tauri.conf.json` 的应用版本一致，提交并推送后创建同版本标签：

```powershell
node scripts/validate-release.mjs v0.7.0
git tag -a v0.7.0 -m "Token Speed v0.7.0"
git push origin v0.7.0
```

首次版本为 `v0.6.0`。失败后可在 Actions 页面重新运行，或手动运行 Release 工作流并填写已存在的版本标签。发布使用 GitHub 自动提供的仓库令牌，无需配置额外 Secret。
