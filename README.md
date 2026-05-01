<p align="center">
  <img src="assets/xsay-logo.png" alt="xsay" width="128" height="128">
</p>

<h1 align="center">xsay</h1>

<p align="center">
  <b>中文优先的离线 AI 语音输入工具</b> · 按住 F2 录音，自动识别并粘贴到光标处<br>
  Offline Chinese-first voice-to-text for Linux / macOS / Windows.
</p>

<p align="center">
  <a href="#安装--install"><img alt="Platforms" src="https://img.shields.io/badge/platforms-linux%20%7C%20macOS%20%7C%20windows-informational"></a>
  <a href="#许可证--license"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue"></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-2024-orange">
</p>

---

## 特性 · Features

- **完全离线** — 所有模型本地推理，不联网、不上传任何音频
- **中文优先 ASR** — 只保留 SenseVoice Small / SenseVoice Small FP32 / Paraformer-zh 三个 sherpa-onnx 模型
- **三种触发方式** — 默认按住 F2 说话，也可切点按切换或绑 GNOME/KDE 系统快捷键调用 `xsay toggle`
- **中文/粤语/中英夹杂** — 默认固定中文，必要时可切自动检测或粤语
- **幻觉过滤** — 识别静音直接返回空白；过滤 Whisper 训练数据里的"謝謝大家收看 / 字幕志愿者 XXX"和 SenseVoice 的填充词
- **X11 / Wayland 同一条路** — Linux 全平台 evdev 直接读 `/dev/input/event*`，绕开 X11 XRecord（之前的死锁源）；arboard 走原生剪贴板协议，uinput 虚拟键盘做 Wayland-native 自动粘贴
- **GPU 加速可选** — `--features cuda | vulkan | metal | hipblas`，按硬件选
- **底部常驻浮层 + StatusNotifierItem 托盘** — 屏幕底部小图标常驻，录音/识别/输入时放大为 120×120 动画话筒；托盘走 ksni（Linux）/ Shell_NotifyIcon（Windows），不依赖 GTK
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
| **xsay-linux-x64-cpu** | Linux x64 | 无 | 通用首选 |
| xsay-linux-x64-vulkan | Linux x64 | NVIDIA / AMD / Intel GPU | 有显卡、想跑大模型 |
| xsay-macos-arm64-metal | macOS Apple Silicon | Apple GPU | M 系列 Mac |

Linux 裸二进制运行时依赖（`apt install`）：`libc6 libstdc++6 libgcc-s1 libx11-6 libxtst6 libasound2 libxdo3`。GNOME 顶栏托盘还需要 `gnome-shell-extension-appindicator`（KDE / Cinnamon / Xfce 原生支持，无需扩展）。

### 或：用 .deb 包安装

下载 `xsay-linux-x64-<version>-1.deb` 后，推荐用 `apt` 安装本地包：

```bash
sudo apt install ./xsay-linux-x64-*.deb
```

如果这个 `.deb` 是打包前运行过 `./packaging/deb/vendor-runtime-libs.sh`
生成的离线包，也可以直接用 `dpkg` 安装：

```bash
sudo dpkg -i ./xsay-linux-x64-*.deb
```

`.deb` 会声明 X11/audio/xdo 等运行时依赖；推荐用
`sudo apt install ./xsay_*.deb`，apt 会自动补齐。`postinst` 还会把执行
`apt install` 的当前用户加入 `input` 组，让内置 evdev 热键监听器能直
接读 `/dev/input/event*` —— 注销重登一次后才生效。`./packaging/deb/vendor-runtime-libs.sh` 打包前跑过的话，会把运行库随包
放到 `/usr/lib/xsay/`，方便离线安装。不要只拷贝 `/usr/bin/xsay` 这个
启动入口。

### 或：从源码构建

见下面 [构建 · Build from source](#构建--build-from-source)。

## 快速开始 · Quick start

1. **启动 xsay**（浮层 + 托盘常驻）
   ```bash
   xsay &
   ```
   `.deb` 装的话，第一次启动前要 `newgrp input` 或注销重登一次让 input 组权限生效（用于 evdev 监听 `/dev/input/event*` 和 `/dev/uinput` 自动粘贴）。

2. **打开设置**：点屏幕底部的浮层图标，或右键托盘 → 「打开设置」
   - **模型** 标签页：点击 `SenseVoice Small` 右边 **安装**，等下载完（约 230 MB）
   - **快捷键** 标签页：默认按住 **F2** 说话；不想用内置监听器也可绑系统快捷键到 `xsay toggle`
   - **常规** 标签页：选择识别语言、粘贴快捷键等

3. **说话**
   - 按住 F2 → 屏幕底部图标放大成 120×120 录音指示
   - 说话
   - 松开或停顿后自动识别 → 文字自动粘贴到光标处

### 给 GNOME/KDE 用户：外部触发（可选）

默认走 evdev 内置监听器（X11 / Wayland 都能用，不依赖 X server XRecord，
也不会占用主线程 X 锁）。如果不想加 input 组，或者想用一个独立按键，
可以让**系统**派发快捷键，调用 `xsay toggle`：

1. 系统设置 → 键盘 → 自定义快捷键 → 新建
2. **名称**：xsay（任取）
3. **命令**：`/usr/bin/xsay toggle`
4. **快捷键**：任意组合

GNOME 自定义快捷键只在按下时触发，没法做"长按说话"。要按住 = 边说边录的体验，
配合 `xbindkeys` 把按下绑到 `xsay press`、松开绑到 `xsay release`。或者直接
用默认的 evdev 内置监听器，按住 F2 就行。

## 模型选择 · Models

xsay 当前面向中文输入服务，只在设置 → 模型 里提供三个 Sherpa ONNX 模型。切换后端**无需重启**。

| 模型 | 大小 | 特点 |
|---|---|---|
| **SenseVoice Small (int8)** | 234 MB | 阿里开源，中文精度超 Whisper-Large，5x 快，支持中/英/日/韩/粤 |
| SenseVoice Small FP32 | 894 MB | 同上，非量化版，内存更多 |
| Paraformer-zh | 950 MB | 达摩院中文专用，非自回归 CTC，低延迟 |

选择建议：
- 默认 **SenseVoice Small (int8)**：速度 + 精度最平衡
- 纯中文写作 → **Paraformer-zh**：延迟低，标点好
- 想减少量化损失 → **SenseVoice Small FP32**：内存更多，速度较慢

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
- **两者都试**：先发 Ctrl+V 再发 Ctrl+Shift+V，最大兼容性

默认使用 **Ctrl+Shift+V**，优先支持 Claude Code CLI、Codex CLI、GNOME Terminal 等终端输入。

## 配置文件 · Configuration

所有设置持久化在 `~/.config/xsay/config.toml`。UI 里的改动会自动写回。完整配置示例：

```toml
[hotkey]
key = "F2"                    # 按键名
modifiers = []                # 修饰键，例如 ["ctrl", "shift"]
mode = "hold"                 # 默认按住说话，松开后识别并输出
internal_listener = true      # 默认启用 evdev 内置监听；置 false 只用系统快捷键调用 xsay toggle

[audio]
silence_threshold = 0.01      # 静音检测阈值
silence_frames = 24           # 约 1.5 秒的静音触发识别
max_record_seconds = 30       # 最长录音

[model]
hf_repo = "k2-fsa/sherpa-onnx"
hf_filename = "sensevoice"    # sensevoice / sensevoice-fp32 / paraformer

[transcription]
language = "zh"               # "zh" / "auto" / "yue"
translate = false             # true = 强制输出英文
n_threads = 4                 # CPU 推理线程数
backend = "sensevoice"        # sensevoice / sensevoice-fp32 / paraformer

[overlay]
position = "bottom-center"    # top-left / top-center / top-right /
                              # bottom-left / bottom-center / bottom-right / center
opacity = 0.9

[injection]
method = "clipboard"          # "clipboard" (CJK 推荐) / "type"
clipboard_delay_ms = 120
paste_shortcut = "ctrl-shift-v" # "ctrl-v" / "ctrl-shift-v" / "both"
```

## CLI 参考

```bash
xsay                   # 启动守护进程（浮层 + 托盘常驻）
xsay toggle            # 切换录音（发 IPC 给运行中的守护进程，给系统快捷键绑用）
xsay press             # 强制开始录音（配合 xbindkeys/evdev 在 key-down 触发）
xsay release           # 强制停止 + 转写（配合 xbindkeys/evdev 在 key-up 触发）
xsay cancel            # 中止当前会话
xsay show              # 唤起设置窗口
xsay quit              # 优雅退出守护进程
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
    libx11-dev libxtst-dev libasound2-dev libxdo-dev libclang-dev

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
cargo build --release
./packaging/deb/vendor-runtime-libs.sh
cargo deb --no-build
# 在线安装推荐用 apt 自动补齐运行依赖
sudo apt install ./target/debian/xsay_*.deb
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
| 按快捷键没反应、设置面板看到"内置键盘监听不可用" | 用户没在 input 组。`sudo usermod -aG input $USER` + 注销重登（或 `newgrp input`），让 evdev 能开 `/dev/input/event*` |
| 托盘图标不显示 | GNOME 需要 `gnome-shell-extension-appindicator` 扩展。守护进程仍在跑，可以点屏幕底部浮层或 `xsay show` 唤起设置 |
| 识别很慢 | 优先使用 SenseVoice Small；Paraformer 冷启动较慢但预热后延迟低 |
| 识别出奇怪的话（"謝謝大家收看"等）| 麦克风无信号时 ASR 会瞎编；xsay 有 RMS 门和黑名单，但也可能漏。看日志 `grep "peak RMS" /tmp/xsay.log` 确认麦克是否在工作 |
| 重复词被吞（"好的好的"只出一次）| 0.1.30 起放宽了重复检测（≥6 次同字才丢）；如果还是吞，看 `grep -i "Skipping" /tmp/xsay.log` 看哪条规则触发了 |
| 没自动粘贴 | 完成 [Wayland 自动粘贴](#wayland-自动粘贴) 的 udev + input 组设置 + 注销重登 |
| GNOME 任务栏 dock 右键"退出"没反应 | 0.1.26 起 dock-quit 已修；旧版升级到最新即可 |

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
