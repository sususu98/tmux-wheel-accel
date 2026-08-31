# tmux-wheel-accel

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)

tmux-wheel-accel 是专为 tmux 打造的硬件级设备感知（Device-Aware）动态滚轮变速原生程序。

通过在 macOS 系统底层（CoreGraphics EventTap）实时感知输入设备类型，彻底解耦 **Apple Magic Trackpad（触控板）** 与 **Logitech G502 / MX Master（鼠标滚轮）**，使两套设备分别运行完全独立的加速曲线。

支持通过 `~/.config/tmux-wheel-accel/config.toml` 独立调优鼠标与触控板参数，具备自动后台守护与修改即时热重载（Hot-Reload）特性。

---

## 核心特性

- 物理级设备自动感知（Device-Aware）：系统底层实时捕获 `kCGScrollWheelEventIsContinuous` 标志位，精准识别当前是触控板在滑还是 G502 鼠标在滚，零延迟自动切换专属配置。
- 触控板专属配置（[trackpad]）：专为 macOS 触控板手势调校，慢滑逐行精读（1 行），快划平缓封顶（4~6 行），彻底消灭触控板暴冲。
- 鼠标专属配置（[mouse]）：专为 G502 / MX Master 调校，刻度单拨精准（1 行），物理无极飞轮全力狂转时可达 32 行极速起飞。
- 亚毫秒同帧去抖（debounce_min_dt_ms = 2.0ms）：精准过滤 macOS/iTerm2 同帧内的微秒级重复子包。
- 微秒级极速执行：纯 Rust 原生构建，无锁、无阻塞、零延迟。
- 消灭首格滚轮延迟：Root 表瞬发启动，进入 copy-mode 的第一微秒立即同步执行滚动，消灭首格被吞的滞后感。
- 随时热重载配置：在 `~/.config/tmux-wheel-accel/config.toml` 中自定义两套独立曲线，保存文件即刻生效，无需重启 tmux。

---

## 配置文件说明 (`~/.config/tmux-wheel-accel/config.toml`)

程序运行时会自动生成独立的双设备配置文件：

```toml
# ~/.config/tmux-wheel-accel/config.toml
# 鼠标与触控板物理级独立配置
# 保存此文件后无需重启 tmux，下次滚动滚轮立即热重载生效！

# 调试日志开关 (true 时实时写入 /tmp/tmux-wheel-accel.log)
debug = true

# 连续滚动判定阈值 (毫秒，两次事件间隔超过此值重置连击)
streak_timeout_ms = 50.0

# 亚毫秒级同帧子包去抖阈值 (毫秒，默认 2.0ms)
debounce_min_dt_ms = 2.0

# ====================================================
# 1. Apple Magic Trackpad 触控板专属配置 (温和细腻，绝不暴冲)
# ====================================================
[trackpad]
# 慢滑 / 精读 (时间间隔 >= 14ms，60Hz 慢帧)
slow_lines = 1

# 中速滑动 (时间间隔 8ms ~ 14ms)
medium_lines = 2

# 快速轻拂 (时间间隔 4ms ~ 8ms)
fast_lines = 4

# 极速狂划封顶步长 (时间间隔 < 4ms，绝对不失控)
max_lines = 6

# ====================================================
# 2. Logitech G502 鼠标专属配置 (慢拨精准，飞轮狂飙)
# ====================================================
[mouse]
# 刻度单拨 / 慢拨精读 (时间间隔 >= 40ms)
notch_lines = 1

# 普通中速拨轮 (时间间隔 15ms ~ 40ms)
normal_lines = 3

# 快速拨轮翻段落 (时间间隔 7ms ~ 15ms)
fast_lines = 8

# 无极飞轮中高速旋转 (时间间隔 3ms ~ 7ms)
high_lines = 16

# G502 物理无极飞轮全力狂转 (< 3ms 超高频) -> 极速起飞！
freespin_lines = 32
```

---

## 硬件分流架构

```text
               ┌───────────────────────────────┐
               │ 物理输入 (Trackpad / Mouse)   │
               └──────────────┬────────────────┘
                              │
                              ▼
               ┌───────────────────────────────┐
               │ macOS CoreGraphics EventTap   │
               │ (自动识别 isContinuous 标志位) │
               └──────────────┬────────────────┘
                              │
             ┌────────────────┴────────────────┐
             ▼                                 ▼
   [检测到: 触控板手势]               [检测到: 实体鼠标]
             │                                 │
             ▼                                 ▼
   [应用 trackpad 配置]               [应用 mouse 配置]
   ├─ 慢滑: 1 行                      ├─ 刻度: 1 行
   ├─ 中速: 2 行                      ├─ 拨轮: 3 ~ 8 行
   └─ 快划封顶: 4 ~ 6 行              └─ 飞轮狂转: 16 ~ 32 行
             │                                 │
             └────────────────┬────────────────┘
                              │
                              ▼
               ┌───────────────────────────────┐
               │ tmux 字符终端执行 (0.05ms)    │
               └───────────────────────────────┘
```

---

## 安装与使用

### 1. 编译并安装

```bash
git clone https://github.com/sususu98/tmux-wheel-accel.git
cd tmux-wheel-accel
./install.sh
```

### 2. 配置 `~/.tmux.conf`

在你的 `~/.tmux.conf` 中加入以下绑定：

```tmux
# 启用鼠标支持
set -g mouse on

# 1. 首格滚轮零延迟响应（进入 copy-mode 瞬间立即启动变速箱）
bind-key -T root WheelUpPane \
  if-shell -F "#{||:#{alternate_on},#{pane_in_mode},#{mouse_any_flag}}" \
    "send-keys -M" \
    "copy-mode -e; run-shell -b \"~/.local/bin/tmux-wheel-accel \\\"#{pane_id}\\\" up\""

# 2. 智能设备感知滚轮加速（微秒级 Rust 原生程序）
bind-key -T copy-mode WheelUpPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" up"
bind-key -T copy-mode WheelDownPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" down"
bind-key -T copy-mode-vi WheelUpPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" up"
bind-key -T copy-mode-vi WheelDownPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" down"
```

### 3. 热重载生效

在终端执行：
```bash
tmux source-file ~/.tmux.conf
```

---

## License

[MIT License](LICENSE) © 2026 sususu98
