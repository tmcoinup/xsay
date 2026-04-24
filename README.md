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

### 识别结果如何进入光标处

xsay 识别完成后 **一定**会把文字复制到系统剪贴板，并弹一条桌面通知。然后：

- **X11 会话 / XWayland 应用（终端、旧版 Firefox/Electron 等）**：xsay 顺便合成一次 Ctrl+V，**自动粘贴，无需任何配置**。
- **Wayland 会话 + 原生 Wayland 应用（GNOME 新应用、GTK4、新版 Electron 等）**：合成按键**进不去**（这是 Wayland 安全模型的限制，不是 xsay 的 bug）。**按一次 Ctrl+V 即可**。

这是所有 Linux 离线语音输入工具的现状。我们没有凭空发明一个简单方案 —— 没有。

### 可选：Wayland 下也自动粘贴（ydotool）

想省掉 Ctrl+V 的那一下，可以装 ydotool。它通过 `/dev/uinput` 合成按键，在 compositor 之下，所有 app 都能收到。**装好后 xsay 自动使用，无需改配置**；没装也不影响使用。

简易设置（Ubuntu/Debian）：

```bash
sudo apt install ydotool
# 允许 input 组访问 uinput 设备
echo 'KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"' \
    | sudo tee /etc/udev/rules.d/60-uinput.rules
echo uinput | sudo tee /etc/modules-load.d/uinput.conf
sudo udevadm control --reload-rules && sudo modprobe uinput
# 系统级 ydotoold（最简单）
sudo systemctl enable --now ydotoold
```

然后**注销再登录**（让新组权限生效）。验证：

```bash
ydotool key 29:1 47:1 47:0 29:0   # 应在光标处粘贴当前剪贴板
```

报 `failed to open uinput device` = udev 规则没生效，重启或检查你在 `input` 组里没（`id | grep input`）。

## 选哪个版本下载

| 发行版 | 系统 | 硬件 | 运行时依赖 | 速度 |
|---|---|---|---|---|
| **xsay-linux-x64-cpu**（推荐给大多数人） | Linux x64 | 任意 | 无（见上表） | Base/Small 模型流畅 |
| xsay-linux-x64-vulkan | Linux x64 | NVIDIA / AMD / Intel GPU | Vulkan 驱动（绝大多数发行版默认已装） | Medium/Large 可实时 |
| xsay-macos-arm64-metal | macOS Apple Silicon | Apple GPU | 无（Metal 随系统） | Medium/Large 可实时 |

**选择原则**：
1. 不确定 → 选 **cpu** 版。零外部依赖，Base 模型（147 MB）在现代 CPU 上已经能做到交互级速度
2. 有 NVIDIA/AMD 显卡、想用 Medium/Large → **vulkan** 版。**不需要装 CUDA**，大多数 Linux 发行版的显卡驱动已自带 Vulkan loader
3. NVIDIA 且已装 CUDA toolkit → 可自行 `cargo build --release --features cuda` 编译 CUDA 版（略快于 Vulkan）

## 从源码构建

```bash
# Ubuntu / Debian 公共构建依赖
sudo apt install build-essential cmake pkg-config \
    libx11-dev libxtst-dev libasound2-dev libxdo-dev \
    libgtk-3-dev libayatana-appindicator3-dev libclang-dev

# 默认 CPU 版（所有人）
cargo build --release

# Vulkan 版（额外依赖：Vulkan 头文件 + GLSL 编译器）
sudo apt install libvulkan-dev glslang-tools
cargo build --release --features vulkan

# CUDA 版（额外依赖：CUDA toolkit 含 nvcc）
cargo build --release --features cuda

# 或一键多版本构建（脚本封装了上面几条命令）
./build.sh cpu           # 只 cpu
./build.sh cpu vulkan    # cpu + vulkan
./build.sh all           # 当前平台支持的全部变体
```

输出到 `dist/xsay-<variant>-<os>-<arch>`。首次构建会花 3–5 分钟编译 whisper.cpp。

GPU 特性是 **互斥**的（whisper.cpp 只 link 一个 GGML backend），一次构建只能选一个。

## 使用

```bash
xsay                   # 启动（托盘常驻）
xsay toggle            # 发 IPC 命令给已运行的 daemon：切换录音（用于系统自定义快捷键）
xsay cancel            # 发 IPC 命令给已运行的 daemon：中止当前会话
xsay --config          # 打印配置文件路径
xsay --list-devices    # 列出麦克风设备
xsay --download-model  # 手动下载默认模型
xsay --help
```

### 推荐绑定系统快捷键（Flameshot 方式）

Wayland 下应用无法可靠地捕获 Super 键，但系统设置里的"自定义快捷键"可以。绑定 `xsay toggle` 到 Super+Z（或任意组合）：

- GNOME：设置 → 键盘 → 查看和自定义快捷键 → 自定义快捷键 → 新建
- 命令填你本机 xsay 的绝对路径 + `toggle`，例如 `/usr/local/bin/xsay toggle`

按快捷键 → 开始录音 → 再按 / 停顿 1.5 秒 → 自动识别并粘贴到光标处。

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
