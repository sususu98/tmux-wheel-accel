# tmux-wheel-accel ⚡️

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)

**tmux-wheel-accel** 是一个专为 `tmux` 打造的微秒级 **6档自适应智能滚轮变速箱（6-Gear Velocity Acceleration）** 原生程序。

特别针对 **罗技 Logitech G502 / MX Master 系列的无极/疾速滚轮（Free-Spin / HyperScroll）** 以及 **Apple Magic Trackpad 触控板** 深度调校，彻底解决在 tmux / Terminal 下“慢滚太快、快转太慢、卡顿延迟、溜冰反弹、缺少高速档”等痛点。

支持通过 **`~/.config/tmux-wheel-accel/config.toml`** 自由微调 6 档位的每一个参数，具备**微秒级内存二进制缓存与修改即时热重载（Hot-Reload）**特性！

---

## ✨ 核心特性

- 🏎 **6 档自适应变速箱**：从 1档逐行精读（2 行），到 2档触控板稳速巡航（2 行），再到 6档无极飞轮红线疾速起飞（32 行/次），无缝线性升档。
- ⚡️ **微秒级极速执行**：单次执行开销 < 0.05ms，具备 56 字节内存二进制缓存，无锁、无阻塞、零延迟。
- 🚀 **消灭首格滚轮延迟**：Root 表瞬发启动，进入 `copy-mode` 的第一微秒立即同步滚动，告别首格被吞的迟钝感。
- ⚙️ **随时热重载配置**：支持在 `~/.config/tmux-wheel-accel/config.toml` 中自定义各档位步长，**保存文件即刻生效，无需重启 tmux**。
- 🛡 **触控板绝佳适配**：触摸板手势区间（$\Delta t \ge 12\text{ms}$）严格锁定在 2档（2 行），双指轻滑平滑细腻，不再因终端高频事件一滑冲顶。
- 🔒 **按 Pane 独立隔离**：状态存储于 `/tmp` 内存映射，多 Pane、多窗口、多会话并发滚动完全互不干扰。

---

## 🏎 6 档位自适应变速表

| 档位 | 适用场景与动作 | 时间间隔 ($\Delta t$) | 单次跳行步长 | 体验特性 |
| :--- | :--- | :--- | :--- | :--- |
| **1 档** | 单格慢拨 / 逐行精读 | $\ge 50\text{ms}$ | **2 行** | 精准核对单行代码、无误跳 |
| **2 档** | 触摸板手势 / 慢速连续巡航 | $12\text{ms} \sim 50\text{ms}$ | **2 行** | **触控板双指滑动细腻、平缓可控** |
| **3 档** | 正常翻阅代码 | $7\text{ms} \sim 12\text{ms}$ | **4 行** | 轻松翻阅 |
| **4 档** | 快速连续拨轮 | $4\text{ms} \sim 7\text{ms}$ | **8 行** | 快速看长函数 |
| **5 档** | G502 无极飞轮中高速 | $2\text{ms} \sim 4\text{ms}$ | **16 行** | 飞速滑过几百行 |
| **6 档** | **G502 物理飞轮红线狂转** | $< 2\text{ms}$ (超高频) | **32 行** | **真正 6 档红线起飞，瞬间翻越几千行！** |

---

## ⚙️ 配置文件说明 (`~/.config/tmux-wheel-accel/config.toml`)

程序运行时会自动在 `~/.config/tmux-wheel-accel/config.toml` 生成带有完整注释的配置文件：

```toml
# ~/.config/tmux-wheel-accel/config.toml
# 6档自适应智能滚轮变速箱配置
# 保存此文件后无需重启 tmux，下次滚动滚轮立即热重载生效！

# 连续滚动判定阈值 (毫秒，两次事件间隔超过此值重置连击)
streak_timeout_ms = 50.0

# 加速起步保护次数（前 N 次滚动严格保持 1档/2档，防误触）
min_streak_for_accel = 4

# ----------------------------------------------------
# 6 档位跳行步长配置 (Gear 1 ~ 6)
# ----------------------------------------------------

# 1档: 单格慢拨 / 逐行精读 (时间间隔 >= 50ms)
gear1_lines = 2

# 2档: 触摸板平稳手势 / 中慢速巡航 (时间间隔 12ms ~ 50ms)
# 触摸板滑动主要落在此档，锁定 2 行保证绝不偏快
gear2_lines = 2

# 3档: 较快翻阅代码 (时间间隔 7ms ~ 12ms)
gear3_lines = 4

# 4档: 快速拨轮翻段落 (时间间隔 4ms ~ 7ms)
gear4_lines = 8

# 5档: G502 无极飞轮中高速旋转 (时间间隔 2ms ~ 4ms)
gear5_lines = 16

# 6档: G502 物理无极飞轮全力狂转 / 疾速起飞 (时间间隔 < 2ms 超高频)
gear6_lines = 32
```

---

## 🚀 安装与使用

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
```

### 3. 热重载生效

在终端执行：
```bash
tmux source-file ~/.tmux.conf
```

---

## 📄 License

[MIT License](LICENSE) © 2026 sususu98
