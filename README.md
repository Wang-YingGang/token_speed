<p align="center">
  <img src="assets/icon-source.png" width="72" alt="Token Speed 图标" />
</p>

<h1 align="center">Token Speed</h1>

<p align="center"><strong>把 Claude / Codex 每轮回答的输出速度，放到 Windows 任务栏。</strong></p>

<p align="center">
  <a href="https://github.com/Wang-YingGang/token_speed/releases/latest">下载最新版</a> ·
  <a href="#产品截图">产品截图</a> ·
  <a href="#三步开始使用">开始使用</a>
</p>

![Token Speed 产品展示：Claude / Codex 输出速度、深浅色悬浮条与显示设置](docs/images/product-showcase.png)

Token Speed 是一款 Windows 桌面小工具：在任务栏或桌面悬浮条中显示 **Claude 和 Codex 最近完成的一轮请求的输出速度**，旁边保留各自最近 20 轮的速度曲线。写代码、切换应用时，抬眼就能看上一轮输出有多快。

它从 **CC Switch 的本地用量数据库**只读获取记录。适合已经通过 CC Switch 使用 Claude / Codex、希望随手查看生成速度的用户。

> **速度在请求完成后更新。** `t/s` 表示每秒输出的 token 数。显示保留一段时间后会回到“待机”，鼠标悬停仍可查看上次结果。

## 下载

前往 **[Releases 下载最新版](https://github.com/Wang-YingGang/token_speed/releases/latest)**，选择适合的 Windows x64 文件：

| 文件 | 使用方式 |
| --- | --- |
| `Token-Speed_*_windows-x64-setup.exe` | 安装版，运行安装程序后启动 Token Speed。 |
| `Token-Speed_*_windows-x64-portable.zip` | 便携版，解压后运行 `token-speed.exe`。 |
| `SHA256SUMS.txt` | 安装包和便携版的 SHA256 校验值。 |

## 三步开始使用

1. **准备数据**：使用 CC Switch，并让 Claude 或 Codex 产生成功的请求用量记录。默认数据库位置为 `%USERPROFILE%\.cc-switch\cc-switch.db`。
2. **启动 Token Speed**：运行安装版或便携版。默认在主任务栏通知区域左侧显示 Claude / Codex 监控条，托盘中也会出现应用图标。
3. **完成一轮请求**：新记录写入后，监控条会显示本轮输出速度并更新曲线。单击监控条或托盘图标即可打开设置，切换为桌面悬浮条，或只显示一个应用。

## 产品截图

以下截图从 **v0.6.0 当前源码编译的原生程序**重新采集，更新于 **2026-10-30**。速度与曲线使用固定示例数据；监控条已放大展示。首图为这些截图的产品展示排版。

### 任务栏监控条

Claude 与 Codex 各占一行，同时显示本轮速度与历史走势。默认宽度为 142 个逻辑像素。

<p align="center">
  <img src="docs/images/taskbar.png" width="426" alt="当前版本任务栏监控条：Claude 88.8 t/s、Codex 77.7 t/s，以及各自最近 20 轮曲线。深色背景，放大展示。" />
</p>

### 桌面悬浮条

监控条也可以放在桌面上，自由拖动，并按桌面配色选择深色或浅色背景。

<table>
  <tr><th>深色</th><th>浅色</th></tr>
  <tr>
    <td><img src="docs/images/floating-dark.png" width="360" alt="深色桌面悬浮条，显示 Claude 和 Codex 的完成速度及曲线" /></td>
    <td><img src="docs/images/floating-light.png" width="360" alt="浅色桌面悬浮条，与深色模式显示相同的示例记录" /></td>
  </tr>
</table>

### 设置

“显示”页调整监控位置、应用、字号和曲线；“偏好”页调整悬浮外观、置顶、位置锁定和开机启动。

<table>
  <tr><th>显示</th><th>偏好</th></tr>
  <tr>
    <td><img src="docs/images/settings-display.png" width="344" alt="当前版本的显示设置：显示位置、Claude + Codex、文字大小、速度曲线、保留时间与位置微调" /></td>
    <td><img src="docs/images/settings-preferences.png" width="344" alt="当前版本的偏好设置：主题、背景不透明度、始终置顶、位置锁定和开机启动" /></td>
  </tr>
</table>

## 可以怎样调整

| 功能 | 可调整内容 |
| --- | --- |
| 显示方式 | 主任务栏嵌入 / 桌面悬浮条。 |
| 应用 | 同时显示 Claude + Codex，或只显示其中一个；单应用模式隐藏应用名称。 |
| 速度曲线 | 各自最近 20 轮记录，从左到右由旧到新；Y 轴自动缩放；宽度 24–80 px，也可关闭。 |
| 文字和外观 | 字号 10–16 px；悬浮条支持深浅主题、30%–100% 背景不透明度和置顶。 |
| 速度保留时间 | 15 / 30 / 60 秒，默认 15 秒；过期后显示“待机”。 |
| 位置 | 拖动文字区移动；任务栏支持输入偏移与 ±1 / ±5 px 微调；支持锁定位置。 |
| 日常使用 | 自动保存设置与位置、开机启动、托盘显示 / 隐藏和退出。 |

单击监控条或托盘图标打开设置；按 `Esc`、单击关闭按钮或其他窗口收起设置。悬停监控条可查看上次速度、记录时间和模型。

## 速度和状态怎么读

**输出速度 = 输出 token 数 ÷ 生成耗时（秒）。** 优先用“请求总延迟 − 首 token 等待时间”计算生成耗时；数据不完整时，依次使用记录中的耗时字段或总延迟。

例如，一轮请求输出 800 个 token，总延迟 12 秒，首 token 等待 2 秒，显示速度就是 `800 ÷ (12 − 2) = 80 t/s`。曲线仅保留成功且有输出的请求记录。

| 显示状态 | 含义 |
| --- | --- |
| `88.8 t/s` 等数值 | 最近完成的一轮请求的输出速度，仍在保留时间内。 |
| `待机` | 正在等待新的完成记录；悬停仍可查看上次结果。 |
| `暂无` | 当前应用没有可显示的成功输出记录。 |
| `等待数据库` / `读取失败` | 尚未找到数据库，或当前数据库无法读取；可悬停设置页底部状态查看原因。 |

**“待机”不能判断 AI 请求是否正在生成。** 本工具读取的是 CC Switch 已完成请求记录，无法测量当前正在生成中的逐 token 速度。

## 数据与配置

程序以只读方式打开 CC Switch 数据库，不修改其中的用量记录。应用设置默认保存在 `%APPDATA%\TokenSpeed\settings.json`；旧设置自动补齐新增字段，保存的屏幕不可用时会将悬浮窗口移回可见区域。

自定义路径可通过环境变量指定：

| 环境变量 | 用途 |
| --- | --- |
| `CC_SWITCH_DB` | 指定 CC Switch 用量数据库路径。 |
| `TOKEN_SPEED_SETTINGS` | 指定设置文件路径。 |
| `TOKEN_SPEED_EXE` | 为原生集成测试指定待测程序。 |

例如，在 PowerShell 中指定其他数据库后启动便携版：

```powershell
$env:CC_SWITCH_DB = "D:\CCSwitch\cc-switch.db"
.\token-speed.exe
```

## 本地开发

项目基于 Rust + Tauri 2，前端使用原生 HTML / CSS / JavaScript。准备 Node.js、Rust 和 Windows 桌面构建环境后运行：

```powershell
npm ci
npm run dev
```

测试和打包：

```powershell
npm test
npm run build
```

构建产物位于 `src-tauri/target/release/token-speed.exe` 和 `src-tauri/target/release/bundle/nsis/`。

<details>
<summary>原生集成验证与截图更新</summary>

原生集成验证使用隔离的 WebView 配置目录、数据库副本和设置文件，覆盖真实窗口拖动、模式切换和重启恢复：

```powershell
python -m pip install --target integration/.deps -r requirements.txt
python prepare_integration_db.py
cargo build --manifest-path src-tauri/Cargo.toml --locked
npm run test:native
```

README 原始截图使用独立的示例数据库和配置，重新编译当前代码后可采集：

```powershell
cargo build --manifest-path src-tauri/Cargo.toml --locked
python scripts/capture-docs.py
```

截图写入 `docs/images/`，版本、采集时间和源码摘要记录在 `docs/images/capture.json`。展示图的制作说明见 [图片说明](docs/images/README.md)。

`integration/` 保存可重建的测试数据和 WebView 缓存，`src-tauri/target/` 保存构建产物，均不提交。临时调试文件放入已忽略的 `scratch/`、`tmp/` 或 `temp/` 目录。

图标源文件为 `assets/icon-source.png`，设计提示词见 `assets/icon-design.md`；运行 `python generate_icon.py` 重建图标。

</details>

<details>
<summary>自动发布</summary>

推送 `v*` 版本标签后，GitHub Actions 会校验版本、运行测试、构建 Windows x64 安装包，再发布到 Releases。附件包括安装包、便携版 ZIP 和 SHA256 校验文件。带预发布后缀的标签（如 `v0.7.0-beta.1`）会发布为 Pre-release。

发布前保持 `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` 和 `src-tauri/tauri.conf.json` 中的应用版本一致，提交并推送后创建同版本标签：

```powershell
node scripts/validate-release.mjs v0.7.0
git tag -a v0.7.0 -m "Token Speed v0.7.0"
git push origin v0.7.0
```

失败后可在 Actions 页面重新运行，或手动运行 Release 工作流并填写已存在的版本标签。发布使用 GitHub 自动提供的仓库令牌，无需额外 Secret。

</details>

## 许可

[MIT](LICENSE)
