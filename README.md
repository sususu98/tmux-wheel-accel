# tmux-wheel-accel

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)

tmux-wheel-accel 是专为 tmux 打造的硬件级设备感知（Device-Aware）与物理惯性连击阶梯（Inertia Streak Tiering）滚轮变速原生程序。

通过在 macOS 系统底层（CoreGraphics EventTap）实时感知输入设备类型，彻底解耦 **Apple Magic Trackpad（触控板）** 与 **Logitech G502 / MX Master（鼠标滚轮）**，使两套设备分别运行完全独立的加速曲线。

同时针对 G502 物理特性，采用连续惯性阶梯判定，严格区分 **刻度齿轮模式（Detent Mode）** 与 **无极飞轮模式（Free-Spin Mode）**：
- 刻度模式：手指拨动 1~7 齿（streak <= 7），严格限制在 1~8 行，绝不暴冲；
- 无极飞轮：飞轮物理惯性持续自由旋转（streak >= 8 升至 28 行，streak >= 16 升至 64 行），释放极致狂暴起飞。

支持通过 `~/.config/tmux-wheel-accel/config.toml` 独立调优鼠标与触控板参数，具备自动后台守护与修改即时热重载（Hot-Reload）特性。

---

## 核心特性

- 物理级设备自动感知（Device-Aware）：系统底层实时捕获 `kCGScrollWheelEventIsContinuous` 标志位，精准识别当前是触控板在滑还是 G502 鼠标在滚，零延迟自动切换专属配置。
- 物理惯性连击阶梯（Inertia Streak Tiering）：
  - 刻度单拨（streak = 1）：严格 1 行，逐行精读；
  - 刻度普通拨轮（streak 2~3）：3 行，平稳浏览；
  - 刻度快速划动（streak 4~7）：8 行，跨段落快速翻阅；
  - 无极飞轮高速（streak 8~15）：28 行，飞轮初段高速；
  - 无极飞轮极速起飞（streak >= 16）：64 行，全速穿梭长日志。
- 触控板专属配置（[trackpad]）：专为 macOS 触控板手势调校，慢滑逐行精读（1 行），快划平缓封顶（4~6 行），彻底消灭触控板暴冲。
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

# 连续滚动判定窗口超时 (毫秒，默认 60.0ms)
streak_timeout_ms = 60.0

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
# 2. Logitech G502 鼠标专属配置 (刻度精准，飞轮狂飙)
# ====================================================
[mouse]
# 刻度单拨 / 慢拨精读 (单格滚动 streak = 1)
notch_lines = 1

# 刻度模式普通中速拨轮 (连续拨 2~3 格)
normal_lines = 3

# 刻度模式快速划动 (手指用力划 4~7 格)
fast_lines = 8

# 无极飞轮中高速旋转 (惯性持续旋转 streak 8 ~ 15)
high_lines = 28

# G502 物理无极飞轮全力狂转 (惯性持续旋转 streak >= 16) -> 64 行极速起飞！
freespin_lines = 64
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
