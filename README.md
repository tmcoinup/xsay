# xsay

AI 语音输入工具。按住快捷键录音，松开后自动转写并写入当前输入框。离线识别，支持中英文。

## 特性

- **按住说话** 或 **点按切换** 两种触发模式
- **停顿自动识别**：说话中途停顿 ~1.5 秒自动出字，不用松开键
- **Esc 取消**：误按随时撤销
- **Wayland + X11**：Linux 下通过 evdev 监听硬件按键（需加入 `input` 组）
- **系统托盘**：后台常驻，右键菜单打开设置
- **悬浮指示**：屏幕一角的 90×30 徽章，录音时变成动画话筒
- **模型可选**：Tiny / Base / Small / Medium / Large v3，UI 内下载、暂停、切换
- **历史记录**：识别结果保存在 `~/.cache/xsay/history.jsonl`
- **开机自启动**：设置里一键开关

## 系统要求

| 平台 | 热键后端 | 说明 |
|---|---|---|
| Linux X11 | rdev | 开箱即用 |
| Linux Wayland | evdev | 需 `sudo usermod -aG input $USER` 并重新登录 |
| macOS | rdev | 首次使用需在"系统偏好设置 → 辅助功能"授权 |
| Windows | rdev | 原生支持 |

Linux 运行时依赖：`libx11-6 libxtst6 libasound2 libgtk-3-0 libayatana-appindicator3-1 libxdo3`

## 构建

```bash
# Ubuntu / Debian 构建依赖
sudo apt install build-essential cmake pkg-config \
    libx11-dev libxtst-dev libasound2-dev libxdo-dev \
    libgtk-3-dev libayatana-appindicator3-dev libclang-dev

cargo build --release
./target/release/xsay
```

首次构建会花 3–5 分钟编译 whisper.cpp。

## 使用

```bash
xsay                   # 启动（托盘常驻）
xsay --config          # 打印配置文件路径
xsay --list-devices    # 列出麦克风设备
xsay --download-model  # 手动下载默认模型（默认是 Medium，1.5 GB）
xsay --help
```

启动后在屏幕一角看到 `⚙ xsay` 徽章，点击打开设置；或点击系统托盘图标。

## 配置

配置文件：`~/.config/xsay/config.toml`，UI 里的改动会自动写回。

```toml
[hotkey]
key = "F9"              # F1..F12, a..z, Space, Home, End 等
modifiers = []          # "ctrl", "alt", "shift", "super"
mode = "hold"           # "hold" 按住说话 / "toggle" 点按切换

[audio]
silence_threshold = 0.01
silence_frames = 24     # 约 1.5 秒
max_record_seconds = 30

[model]
hf_repo = "ggerganov/whisper.cpp"
hf_filename = "ggml-medium.bin"

[transcription]
language = "auto"       # "zh", "en", "ja", "ko", ..., "auto"
translate = false
n_threads = 4

[overlay]
position = "top-right"  # top-left, bottom-left, bottom-right, center

[injection]
method = "clipboard"    # "clipboard" (CJK 推荐) 或 "type"
clipboard_delay_ms = 80
```

## 打包成 .deb

```bash
cargo install cargo-deb
cargo deb
```

生成的 `.deb` 在 `target/debian/` 下，包含元数据里声明的运行时依赖。

安装：`sudo dpkg -i target/debian/xsay_0.1.0_amd64.deb`

## 目录

- 配置：`~/.config/xsay/config.toml`
- 模型缓存：`~/.cache/xsay/models/`
- 历史记录：`~/.cache/xsay/history.jsonl`
- 自启动（Linux）：`~/.config/autostart/xsay.desktop`

## 许可证

MIT
