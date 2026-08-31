# tmux-wheel-accel

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)

tmux-wheel-accel 是专为 tmux 打造的微秒级 6档自适应动态滚轮变速箱（Velocity-based Dynamic Acceleration）原生程序。

特别针对罗技 Logitech G502 / MX Master 系列物理无极/疾速滚轮（Free-Spin / HyperScroll）以及 Apple Magic Trackpad 触控板深度调校，解决终端环境下“慢拨跳多行、快转太慢、滑动延迟溜冰、停下反弹、触控板用力一划飞上千行”等痛点。

支持通过 `~/.config/tmux-wheel-accel/config.toml` 自由配置核心参数，具备微秒级共享内存二进制缓存与修改即时热重载（Hot-Reload）特性。

---

## 核心特性

- 6 档自适应变速箱：从 1档逐行精读（1 行）、2档触控板温和巡航（1 行），到 6档无极飞轮红线疾速起飞（24 行/次），无缝自适应升档。
- 触控板连击深度熔断保护（high_gear_min_streak）：触控板即使快速用力轻拂（10~15 个微事件），也会被严格锁在 1~3档（1~2 行），绝不会发生“划一下飞掉 1500 行”的失控现象。
- 微秒级极速执行：纯 Rust 原生零依赖构建，单次执行耗时低于 0.05ms，具备 64 字节内存二进制缓存，无锁、无阻塞、零延迟。
- 消灭首格滚轮延迟：Root 表瞬发启动，进入 copy-mode 的第一微秒立即同步执行滚动，消灭首格被吞的滞后感。
- 随时热重载配置：在 `~/.config/tmux-wheel-accel/config.toml` 中自定义各档位步长与判定阈值，保存文件即刻生效，无需重启 tmux。
- 按 Pane 独立隔离：状态存储于 /tmp 独立二进制映射中，多 Pane、多窗口、多会话并发滚动互不干扰。

---

## 6 档位自适应变速表

| 档位 | 适用场景与动作 | 时间间隔 ($\Delta t$) | 连击深度门槛 | 单次跳行步长 | 体验特性 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 1 档 | 单格慢拨 / 逐行精读 | $\ge 50\text{ms}$ | 任意 | 1 行 | 精准核对单行代码、无误跳 |
| 2 档 | 触摸板手势 / 慢速连续巡航 | $12\text{ms} \sim 50\text{ms}$ | 任意 | 1 行 | 触控板双指滑动细腻、平缓可控 |
| 3 档 | 较快翻阅代码 | $7\text{ms} \sim 12\text{ms}$ | $\ge 4$ | 2 行 | 轻松翻阅 |
| 4 档 | 快速连续拨轮 | $4\text{ms} \sim 7\text{ms}$ | $\ge 4$ | 4 行 | 快速看长函数 |
| 5 档 | G502 无极飞轮中高速 | $2.5\text{ms} \sim 4\text{ms}$ | $\ge 20$ (长旋飞轮) | 10 行 | 飞速滑过几百行 |
| 6 档 | G502 物理飞轮红线狂转 | $< 2.5\text{ms}$ (超高频) | $\ge 20$ (长旋飞轮) | 24 行 | 真正 6 档红线起飞，瞬间翻越长日志 |

> 触控板一次完整滑动手势通常仅产生 10~15 个微事件，会被严格限制在 1~3 档（最多走 1~2 行），因此滑动极为克制平稳；只有 G502 物理飞轮持续飞旋超过 20 次连击，才会进入 5档与 6档。

---

## 与 macOS 驱动层（如 Mos）的协同机制

在 macOS 环境下使用 Mos（平滑滚动工具）时，建议 Mos 负责系统/驱动层，tmux-wheel-accel 负责终端/字符层，两者精密协同：

1. 终端应用例外分流（Per-App Smooth Disable）：
   - 对 Chrome、Safari、微信等 GUI 应用：Mos 保持 `smooth: true`，提供像素级平滑流动体验；
   - 对终端应用（如 iTerm2、Ghostty、Alacritty）：在 Mos 中配置例外规则 `smooth: false`（关闭平滑插值）；
   - 目的：避免 Mos 向终端发送 60Hz 模拟动画事件流导致的“画面溜冰、输入延迟、停不下来”现象，让鼠标硬件信号 1:1 直接送达终端。

2. 金属滚轮微回弹过滤（DeadZone Tuning）：
   - 在 Mos 全局及应用规则中将滚动死区（`deadZone`）从默认 `1.0` 调整至 `2.0 ~ 2.5`；
   - 目的：G502 金属滚轮较重，手指抬起时机械刻度槽复位会产生微小的反向抖动（Recoil）。Mos 在 EventTap 系统最前沿直接将反向微抖动过滤，彻底消除向上滚动停下时的回弹跳跃。

3. 鼠标与触控板方向解耦（Reverse Decoupling）：
   - 在 Mos 例外规则中保留 `reverse: true`；
   - 目的：macOS 默认将外接鼠标与触控板方向强行绑定。Mos 在系统层独立反转外接鼠标方向，同时保持触控板原生的自然滑动方向。

### 分工链路架构

```text
G502 硬件输入 / 触控板
       │
       ▼
[macOS 系统驱动层 - Mos]
       ├─ 过滤 G502 机械反弹 (deadZone: 2.5)
       ├─ 解耦鼠标与触控板滚动方向 (reverse: true)
       └─ 针对终端直通无平滑 (smooth: false)
       │
       ▼
[终端模拟器 - iTerm2 / Ghostty]
       │
       ▼
[tmux 字符终端层 - tmux-wheel-accel]
       ├─ 触控板与慢速手势：严格锁定 1 行微操
       └─ G502 无极飞轮狂转：智能升至 6 档 (24 行) 起飞
```

---

## 配置文件说明 (`~/.config/tmux-wheel-accel/config.toml`)

程序首次运行时会自动在 `~/.config/tmux-wheel-accel/config.toml` 生成配置文件：

```toml
# ~/.config/tmux-wheel-accel/config.toml
# 6档自适应智能滚轮变速箱配置
# 保存此文件后无需重启 tmux，下次滚动滚轮立即热重载生效！

# 连续滚动判定阈值 (毫秒，两次事件间隔超过此值重置连击)
streak_timeout_ms = 50.0

# 加速起步保护次数（前 N 次滚动严格保持 1档/2档，防误触）
min_streak_for_accel = 4

# 进入 5档/6档 极速飞轮所需的持续连击门槛（默认 20 次）
# 触控板划一下通常只有 10~15 个微事件，会被安全锁在 1~3档（绝不会飞掉上千行）；
# 只有 G502 物理飞轮持续长旋超过 20 次连击，才会平滑升入 5档/6档
high_gear_min_streak = 20

# ----------------------------------------------------
# 6 档位跳行步长配置 (Gear 1 ~ 6)
# ----------------------------------------------------

# 1档: 单格慢拨 / 逐行精读 (时间间隔 >= 50ms)
gear1_lines = 1

# 2档: 触摸板平稳手势 / 中慢速巡航 (时间间隔 12ms ~ 50ms)
# 触摸板滑动主要落在此档，锁定 1 行保证极致细腻不偏快
gear2_lines = 1

# 3档: 较快翻阅代码 (时间间隔 7ms ~ 12ms)
gear3_lines = 2

# 4档: 快速拨轮翻段落 (时间间隔 4ms ~ 7ms)
gear4_lines = 4

# 5档: G502 无极飞轮中高速旋转 (时间间隔 2.5ms ~ 4ms, 需连击 >= high_gear_min_streak)
gear5_lines = 10

# 6档: G502 物理无极飞轮全力狂转 / 疾速起飞 (时间间隔 < 2.5ms, 需连击 >= high_gear_min_streak)
gear6_lines = 24
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

# 1. 首格滚轮零延迟响应（进入 copy-mode 瞬间立即启动 6档变速箱）
bind-key -T root WheelUpPane \
  if-shell -F "#{||:#{alternate_on},#{pane_in_mode},#{mouse_any_flag}}" \
    "send-keys -M" \
    "copy-mode -e; run-shell -b \"~/.local/bin/tmux-wheel-accel \\\"#{pane_id}\\\" up\""

# 2. 6档自适应智能滚轮加速（微秒级 Rust 原生程序）
bind-key -T copy-mode WheelUpPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" up"
bind-key -T copy-mode WheelDownPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" down"
bind-key -T copy-mode-vi WheelUpPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" up"
bind-key -T copy-mode-vi WheelDownPane select-pane \; run-shell -b "~/.local/bin/tmux-wheel-accel \"#{pane_id}\" down"

# 3. 极速涡轮修饰键（可选）
bind-key -T copy-mode S-WheelUpPane select-pane \; send-keys -X -N 8 scroll-up
bind-key -T copy-mode S-WheelDownPane select-pane \; send-keys -X -N 8 scroll-down
bind-key -T copy-mode-vi S-WheelUpPane select-pane \; send-keys -X -N 8 scroll-up
bind-key -T copy-mode-vi S-WheelDownPane select-pane \; send-keys -X -N 8 scroll-down

bind-key -T copy-mode M-WheelUpPane select-pane \; send-keys -X halfpage-up
bind-key -T copy-mode M-WheelDownPane select-pane \; send-keys -X halfpage-down
bind-key -T copy-mode-vi M-WheelUpPane select-pane \; send-keys -X halfpage-up
bind-key -T copy-mode-vi M-WheelDownPane select-pane \; send-keys -X halfpage-down
```

### 3. 热重载生效

在终端执行：
```bash
tmux source-file ~/.tmux.conf
```

---

## License

[MIT License](LICENSE) © 2026 sususu98
