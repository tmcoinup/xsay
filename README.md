<p align="center">
  <img src="assets/xsay-logo.png" alt="xsay" width="128" height="128">
</p>

<h1 align="center">xsay</h1>

<p align="center">
  <b>离线 AI 语音输入工具</b> · 按快捷键录音，松开自动识别，直接粘贴到光标处<br>
  Offline voice-to-text for Linux / macOS / Windows. Hold a hotkey, speak,
  release — the transcription types itself wherever your cursor is.
</p>

<p align="center">
  <a href="#安装--install"><img alt="Platforms" src="https://img.shields.io/badge/platforms-linux%20%7C%20macOS%20%7C%20windows-informational"></a>
  <a href="#许可证--license"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-2024-orange">
</p>

---

## 特性 · Features

- **完全离线** — 所有模型本地推理，不联网、不上传任何音频
- **两代 ASR 后端** — Whisper (whisper.cpp) + SenseVoice/Paraformer (sherpa-onnx)，设置里一键切换
- **三种触发方式** — 按住说话（hold）/ 点按切换（toggle）/ 绑 GNOME 系统快捷键调用 `xsay toggle`
- **中英日韩粤自动识别** — 句内混说（"这个 API 怎么 deploy"）也能正确分词
- **幻觉过滤** — 识别静音直接返回空白；过滤 Whisper 训练数据里的"謝謝大家收看 / 字幕志愿者 XXX"和 SenseVoice 的"Okay./Yes./嗯。"填充词
- **Wayland 友好** — evdev 直接读键盘、arboard 走原生剪贴板协议、uinput 级合成 Ctrl+V 自动粘贴，无需 ydotool daemon
- **GPU 加速可选** — `--features cuda | vulkan | metal | hipblas`，按硬件选
- **原生托盘 + 悬浮反馈** — 系统托盘常驻，识别时屏幕一角 120×120 动画话筒
- **历史记录** — 所有识别结果保存在 `~/.cache/xsay/history.jsonl`

## 安装 · Install

### 推荐：下载发行版二进制

到 [Releases](https://github.com/tmcoinup/xsay/releases) 挑对应版本下载，解压后：

```bash
chmod +x xsay-*
./xsay-*
```

| 二进制 | 系统 | GPU 加速 | 对谁合适 |
|---|---|---|---|
| **xsay-linux-x64-cpu** | Linux x64 | 无 | 通用首选，零依赖 |
| xsay-linux-x64-vulkan | Linux x64 | NVIDIA / AMD / Intel GPU | 有显卡、想跑大模型 |
| xsay-macos-arm64-metal | macOS Apple Silicon | Apple GPU | M 系列 Mac |

Linux 运行时依赖（`apt install`）：`libx11-6 libxtst6 libasound2 libgtk-3-0 libayatana-appindicator3-1 libxdo3`

### 或：从源码构建

见下面 [构建 · Build from source](#构建--build-from-source)。

## 快速开始 · Quick start

1. **启动 xsay**（托盘常驻）
   ```bash
   xsay &
   ```

2. **第一次运行自动打开设置窗口（或从托盘打开）**
   - **模型** 标签页：点击 `SenseVoice Small` 右边 **安装**，等下载完（约 230 MB）
   - **快捷键** 标签页：点 **捕捉按键** → 按你想用的组合（如 F2 或 Super+Z）
   - **常规** 标签页：选择识别语言、粘贴快捷键等

3. **说话**
   - 按住（或按下切换）快捷键 → 屏幕底部出现 `● REC`
   - 说话
   - 松开快捷键（或再按一次） → 文字自动粘贴到光标处

### 给 GNOME/KDE 用户：绑系统快捷键更稳（推荐）

Wayland 下应用级按键捕获有限制。最可靠的是让**系统**派发快捷键，然后调用 `xsay toggle`：

1. 系统设置 → 键盘 → 自定义快捷键 → 新建
2. **名称**：xsay（任取）
3. **命令**：`/path/to/xsay toggle`（例如 `/home/你/.local/bin/xsay toggle`，见设置面板"外部触发"卡片里的绝对路径）
4. **快捷键**：按 Super+Z 或任意组合

## 模型选择 · Models

xsay 支持两代 ASR 后端，可在设置 → 模型 里选择。切换后端**无需重启**。

### Whisper (OpenAI) — 通用多语言

| 模型 | 大小 | CPU 实时因子 | 备注 |
|---|---|---|---|
| Tiny | 75 MB | ~0.1x | 最快，精度差 |
| **Base** | 147 MB | ~0.3x | 入门推荐 |
| Small | 488 MB | ~1x | 中档 |
| Medium | 1.5 GB | ~3x | 有 GPU 时推荐 |
| Large v3 | 3.1 GB | ~10x | 必须 GPU |
| **Large v3 Turbo** | 810 MB | ~1x | 精度接近 Large 但 4x 快 |

### Sherpa ONNX — 中文强项

| 模型 | 大小 | 特点 |
|---|---|---|
| **SenseVoice Small (int8)** | 234 MB | 阿里开源，中文精度超 Whisper-Large，5x 快，支持中/英/日/韩/粤 |
| SenseVoice Small FP32 | 894 MB | 同上，非量化版，内存更多 |
| Paraformer-zh | 950 MB | 达摩院中文专用，非自回归 CTC，低延迟 |

选择建议：
- 默认 **SenseVoice Small (int8)**：速度 + 精度最平衡
- 纯中文写作 → **Paraformer-zh**：延迟低，标点好
- 多语言混说 → **SenseVoice** 或 **Whisper Turbo**

## Wayland 自动粘贴

Wayland 会话下，合成按键进不去原生 Wayland 应用（GNOME Terminal / 新版 VS Code 等）。xsay 用 **`/dev/uinput` 虚拟键盘**绕开这个限制，不依赖 ydotool。

需要一次性权限设置（只做一次）：

```bash
# 1. 加入 input 组
sudo usermod -aG input $USER

# 2. 允许 input 组写 /dev/uinput
echo 'KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"' \
    | sudo tee /etc/udev/rules.d/60-xsay-uinput.rules
sudo udevadm control --reload-rules && sudo modprobe uinput

# 3. 注销再登录（让组权限生效）
```

**验证**：在 xsay 日志里看到 `uinput virtual keyboard created for auto-paste` 就是成功了。

如果不做这步，xsay 会把文字复制到剪贴板 + 弹通知，你手动 Ctrl+V 也能用。

### 终端粘贴快捷键

设置 → 常规 → 粘贴快捷键 有三个选项：

- **Ctrl+V**：普通编辑器 / 浏览器
- **Ctrl+Shift+V**：终端（Claude Code CLI、Codex CLI、GNOME Terminal 等）
- **两者都试**（默认）：先发 Ctrl+V 再发 Ctrl+Shift+V，最大兼容性

## 配置文件 · Configuration

所有设置持久化在 `~/.config/xsay/config.toml`。UI 里的改动会自动写回。完整配置示例：

```toml
[hotkey]
key = "F2"                    # 按键名
modifiers = []                # 修饰键，例如 ["ctrl", "shift"]
mode = "hold"                 # "hold" 按住说话 / "toggle" 点按切换

[audio]
silence_threshold = 0.01      # 静音检测阈值
silence_frames = 24           # 约 1.5 秒的静音触发识别
max_record_seconds = 30       # 最长录音

[model]
hf_repo = "ggerganov/whisper.cpp"
hf_filename = "ggml-base.bin" # 或 "sensevoice" 等子目录名

[transcription]
language = "zh"               # "auto" / "zh" / "en" / "ja" / ...
translate = false             # true = 强制输出英文
n_threads = 4                 # CPU 推理线程数
backend = "sensevoice"        # "whisper" / "sensevoice" / "paraformer"

[overlay]
position = "bottom-center"    # top-left / top-center / top-right /
                              # bottom-left / bottom-center / bottom-right / center
opacity = 0.9

[injection]
method = "clipboard"          # "clipboard" (CJK 推荐) / "type"
clipboard_delay_ms = 80
paste_shortcut = "both"       # "ctrl-v" / "ctrl-shift-v" / "both"
```

## CLI 参考

```bash
xsay                   # 启动守护进程（托盘常驻）
xsay toggle            # 切换录音（发 IPC 给运行中的守护进程，给系统快捷键绑用）
xsay cancel            # 中止当前会话
xsay --config          # 打印配置文件路径
xsay --list-devices    # 列出麦克风设备
xsay --download-model  # 手动下载默认模型
xsay --help
```

## 构建 · Build from source

### 构建依赖

```bash
# Ubuntu / Debian
sudo apt install build-essential cmake pkg-config \
    libx11-dev libxtst-dev libasound2-dev libxdo-dev \
    libgtk-3-dev libayatana-appindicator3-dev libclang-dev

# Vulkan GPU 支持（可选，用于 Medium/Large Whisper）
sudo apt install libvulkan-dev glslang-tools glslc

# macOS (用 brew)
brew install cmake pkg-config
```

### 构建命令

```bash
# 默认（CPU-only Whisper + SenseVoice int8 ONNX）
cargo build --release
./target/release/xsay

# 带 Vulkan GPU 加速
cargo build --release --features vulkan

# 带 CUDA GPU 加速（NVIDIA + CUDA toolkit）
cargo build --release --features cuda

# macOS Metal GPU
cargo build --release --features metal

# 最小化 Whisper-only（去掉 sherpa-onnx 50MB 共享库）
cargo build --release --no-default-features

# 一键出多版本（build.sh）
./build.sh cpu              # 只 CPU
./build.sh cpu vulkan       # CPU + Vulkan
./build.sh all              # 本机支持的全部变体
```

构建产物在 `dist/xsay-<variant>-linux-x64`。首次构建会花 3–5 分钟编译 whisper.cpp。

## 打 Debian 包

```bash
cargo install cargo-deb
cargo deb
sudo dpkg -i target/debian/xsay_0.1.0_amd64.deb
```

## 打 Snap（可发布到 Snap Store / Ubuntu 软件中心）

仓库根目录已包含 `snap/snapcraft.yaml`：

```bash
# 装 snapcraft（LXD 多核构建，最省心）
sudo snap install snapcraft --classic
sudo snap install lxd
sudo lxd init --auto

# 构建
snapcraft

# 本地测试
sudo snap install ./xsay_*_amd64.snap --dangerous

# 发布到 Snap Store（需先在 snapcraft.io 注册账号）
snapcraft login
snapcraft register xsay            # 第一次发布抢占 name
snapcraft upload --release=edge ./xsay_*_amd64.snap
# 稳定后改 stable：snapcraft release xsay <rev> stable
```

上线 Ubuntu 软件中心见 [PACKAGING.md](PACKAGING.md)。

## 故障排查 · Troubleshooting

| 症状 | 原因 / 解决 |
|---|---|
| 按快捷键没反应 | Wayland 下 rdev 看不到原生窗口；`sudo usermod -aG input $USER` 让 xsay 用 evdev |
| 识别很慢 | 切换到 SenseVoice Small 或 Whisper Base；有 GPU 就 `--features vulkan` 重建 |
| 识别出奇怪的话（"謝謝大家收看"等）| 麦克风无信号时 Whisper 会瞎编；xsay 有 RMS 门和黑名单，但也可能漏。看日志 `grep "peak RMS" /tmp/xsay.log` 确认麦克是否在工作 |
| 没自动粘贴 | 完成 [Wayland 自动粘贴](#wayland-自动粘贴) 的 udev 设置 + 注销重登 |
| "`Unknown` is not responding" | 旧 bug（主浮层未真正显示）已修，升级到最新版 |

日志：`RUST_LOG=xsay=debug xsay 2>&1 | tee /tmp/xsay.log`

## 目录 · File layout

- 配置：`~/.config/xsay/config.toml`
- 模型缓存：`~/.cache/xsay/models/`
- 历史记录：`~/.cache/xsay/history.jsonl`
- IPC socket：`$XDG_RUNTIME_DIR/xsay.sock`
- Linux 自启动：`~/.config/autostart/xsay.desktop`

## 致谢 · Credits

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) — OpenAI Whisper 的 C++ 推理
- [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) — 多后端 ONNX ASR 运行时
- [SenseVoice](https://github.com/FunAudioLLM/SenseVoice) — 阿里 FunAudioLLM 开源模型
- [Paraformer](https://github.com/modelscope/FunASR) — 达摩院 FunASR
- [eframe / egui](https://github.com/emilk/egui) — Rust GUI 框架

## 许可证 · License

[MIT](LICENSE)
