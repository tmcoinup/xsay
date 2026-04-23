# xsay UI 设计稿生成提示词

把下列提示词粘给 Figma AI / Galileo AI / Magic Patterns / v0.dev / Claude / GPT-4V 等工具，生成可直接交给前端的视觉稿。适配 macOS / Windows / Linux 桌面端；深色主题为主、浅色主题可选。

---

## 顶层提示词（推荐直接用）

> **Design a native-desktop settings window for "xsay", an offline AI voice input tool for Linux / macOS / Windows. Users hold a global hotkey (default F9) to record speech; the app transcribes with Whisper and types the result into the focused window.**
>
> **Visual language**: modern, minimal, focused. Deep charcoal background (`#1E1E22`), near-white text (`#E8E8EC`), accent color cyan-blue (`#3DA5FF`) for active/selected state, soft green (`#50DC50`) for success badges, warm amber (`#FFB43C`) for warnings. Rounded 8 px corners on frames and buttons. Inter (or system UI font) 13 px base, 18 px for hero values. Generous vertical rhythm (8 px grid).
>
> **Two artifacts are needed**:
>
> 1. **Floating overlay (always-on-top widget)** — two compact states:
>    - **Idle badge**: 90×30 px rounded pill, semi-transparent dark grey `rgba(30,30,30,0.7)`, single-line label `"⚙ xsay"` in 12 px secondary grey. Clickable; hover reveals a faint cyan border.
>    - **Recording**: 120×120 px rounded square, dark overlay, centered red circle (`#C83232`) with a white microphone glyph and a pulsing concentric ring (animated, 2 s ease-in-out). Below the mic, 10 px label `● REC` in hot red.
>    - **Transcribing**: same 120 px box, centered label `识别中...` in cyan blue, animated ellipsis.
>    - **Injecting**: same box, label `输入中...` in soft green.
>    All four states share the same anchor corner; illustrate a right-top anchoring on a 1920×1080 mockup.
>
> 2. **Settings window** — 620×520 px, non-modal, undecorated-looking but with a subtle drag region. Four tabs along the top as segmented control:
>    **🤖 模型 | ⌨ 快捷键 | ⚙ 常规 | 📜 历史记录**
>
>    Design each of the four tabs as separate frames sharing the same chrome.

---

## 每个 Tab 的细节

### Tab 1 — 🤖 模型

Purpose: let the user pick and manage Whisper model files.

- **Warning banner** at top when no model is present: warm amber background, bold white text `"⚠  当前没有可用模型，xsay 无法识别语音"`, subtle second line `"推荐下载 Medium (1.5 GB，中英文高精度)"`.
- **Vertical list of 5 model cards**, each:
  - Left: radio-button-style selector (filled cyan when active).
  - Right stack:
    - Row 1: **Name** (bold) + size in MB (muted) + short description (muted).
    - Optional tiny chips: `✓ 当前使用` (green) / `↑ 有更新` (yellow) / `✓ 最新` (dim green).
    - Row 2: either a progress bar (downloading), `已下载 X/Y MB，可继续` (paused/partial), or bare `X.Y MB` (ready).
    - Row 3: action pills — `⬇ 下载` / `▶ 继续下载` / `⏸ 暂停` / `✕ 取消` / `✓ 切换使用` / `🗑 删除` / `✕ 删除进度`. Inline, small, text-only buttons with hover fill.
- Model catalogue to render (name — file — size — desc):
  - Tiny — `ggml-tiny.bin` — 75 MB — "最快，精度一般，适合低配设备"
  - Base — `ggml-base.bin` — 147 MB — "快速，精度良好"
  - Small — `ggml-small.bin` — 488 MB — "平衡速度与精度"
  - Medium — `ggml-medium.bin` — 1500 MB — "高精度，推荐使用"
  - Large v3 — `ggml-large-v3.bin` — 3100 MB — "最高精度，速度较慢，需要大量内存"
- **Bottom row**: secondary button `🔄 检查所有模型更新`. Disabled spinner state `🔄 检查中...`.

### Tab 2 — ⌨ 快捷键

- **Hero group**: label `当前快捷键`, below it the chord rendered big (18 px, monospace, cyan): e.g. `Ctrl + Alt + F9`.
- **Capture button**: `  捕捉按键  ` — a wide rounded button (180×30). When armed: `⌨  请按下目标按键...` amber text. Helper caption below: `按下任意功能键 (F1-F12, Home, End 等) 或字母键，按 Esc 取消`.
- **Manual entry row**: label `或手动输入键名：` + single-line 120 px text input + muted hint `如 Pause, ScrollLock, CapsLock`.
- **Trigger mode** radio group (label `触发模式：`, bold):
  - ◯ `按住说话（松开识别）`
  - ◯ `点按切换（再按结束）`
- **Modifier keys** checkbox row (label `修饰键（可选）：`, bold): `☐ Ctrl  ☐ Alt  ☐ Shift  ☐ Super` inline.
- **Actions row** at bottom: primary `💾  保存快捷键` (disabled when no change), secondary `↩  还原`.
- **Tip line** (muted, small): `"提示：按住快捷键录音，松开转写输入。停顿 1.5 秒自动识别。Esc 取消。"`

### Tab 3 — ⚙ 常规

One-column scrollable list of **grouped cards**, each group is a labeled `GroupBox`:

1. **语音识别**
   - 语言 dropdown: 自动检测 / 中文 / English / 日本語 / 한국어 / Français / Deutsch / Español / Русский
   - `☐ 翻译为英文输出`
   - 推理线程 slider (1–16, 显示数值)
2. **文字注入**
   - 方式 dropdown: `剪贴板 (Ctrl+V)` / `键盘模拟`
   - 剪贴板延迟 slider (0–500 ms, 后缀 ms)
   - 说明: "CJK 字符推荐剪贴板方式；慢设备上请调大延迟"
3. **音频与停顿检测**
   - 静音阈值 slider (对数 0.001–0.1, 3 位小数)
   - 停顿长度 slider (8–80 帧, 旁边显示 "约 X.X 秒")
   - 最长录音 slider (5–180 秒)
4. **麦克风** (只读信息)
   - `可用设备 (N)`
   - 每个设备一行 `• 设备名称`
   - 提示: "目前使用系统默认设备，切换设备需在 config.toml 中指定"
5. **系统**
   - `☐ 开机自启动` + 副文 `（登录后自动启动 xsay）`
6. **浮层**
   - 位置 dropdown: 右上角 / 左上角 / 右下角 / 左下角 / 居中

底部浮现绿色小字反馈 `"✓ 已保存并生效"`。

### Tab 4 — 📜 历史记录

- **Header row**: 左 `最近 N 条识别结果` (bold) + 右 `🗑 清空` 按钮（右对齐，条目为零时禁用）
- **Cards stack** (滚动): 每条 = 一个暗色卡片，
  - 顶部行: 左 `2026-04-23 16:42` (monospace, muted) + 右 `📋 复制`（右对齐，小按钮）
  - 正文: 识别出的文字（正常颜色，可能多行）
- **空态**: 居中淡色文字 `"暂无历史记录。识别出的文本会自动保存到这里。"`

---

## 托盘菜单（单独画一个 context-menu 小图）

一个 macOS/Linux 风格的下拉菜单，暗灰背景 `#2A2A2E`，宽 ~160 px：
- `⚙  打开设置`
- ` ─────── ` 分隔符
- `退出 xsay`

---

## 交互细节

- **悬浮徽章**：鼠标移上去光标变成 pointing-hand，点击打开设置窗。
- **录音动画**：脉冲环 `scale(1.0 → 1.3)` + `opacity(0.6 → 0.15)` 约 2 s 循环。
- **模型下载**：进度条带文字 `12.3/147 MB  8%`，暂停/继续无缝切换。
- **保存按钮**：仅当有未保存修改时高亮并可点，否则 40% 透明度。
- **无障碍**：所有图标都带文字标签；键盘 Tab 顺序跟视觉顺序一致。

## 可导出的设计 tokens（如果工具支持）

```json
{
  "color.bg": "#1E1E22",
  "color.bg.card": "#26262B",
  "color.bg.card.selected": "#1E3C1E",
  "color.bg.banner.warning": "#5A2D14",
  "color.text.primary": "#E8E8EC",
  "color.text.secondary": "#A0A0A8",
  "color.text.disabled": "#606068",
  "color.accent": "#3DA5FF",
  "color.success": "#50DC50",
  "color.warning": "#FFB43C",
  "color.danger": "#FF6060",
  "color.rec": "#C83232",
  "radius.card": 6,
  "radius.button": 4,
  "radius.badge": 8,
  "font.base": 13,
  "font.heading": 14,
  "font.hero": 18,
  "space.xs": 2,
  "space.sm": 4,
  "space.md": 8,
  "space.lg": 12,
  "space.xl": 16,
  "grid": 8
}
```

---

## 最小生成清单（面向 AI 设计工具）

1. 一张主图：Idle 徽章 + 录音动画两态并排展示
2. 四张 tab 详情图：模型、快捷键、常规、历史
3. 一张托盘菜单
4. 一张 1920×1080 桌面场景图：悬浮徽章贴在右上角
5. 一张状态机图：Idle → Recording → Transcribing → Injecting（用箭头连接，标注触发条件）

---

## 功能全景（给设计师看的"这个应用都能干啥"）

- 全局快捷键（F9 可改）触发录音，两种触发模式
- 离线 Whisper 模型（5 档可选），UI 内下载/暂停/续传/切换/删除，检查远端更新
- 自动停顿检测（1.5s 默认）边说边出字
- 剪贴板 or 键盘模拟两种注入方式（CJK 友好）
- 多语言（9 种）+ 可选翻译到英文
- 系统托盘驻留 + 右键菜单
- X11 和 Wayland 都支持（Linux）
- 开机自启动开关
- 历史记录本地保存（JSONL）
- 浮层 5 种角落/居中定位
- 配置即时生效，不重启
