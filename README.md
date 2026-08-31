# tmux-wheel-accel ⚡️

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)

**tmux-wheel-accel** 是一个专为 `tmux` 打造的微秒级动态滚轮加速度（Velocity-based Dynamic Acceleration）原生程序。

特别针对 **罗技 Logitech G502 / MX Master 系列的无极/疾速滚轮（Free-Spin / HyperScroll）** 以及 **Apple Magic Trackpad 触控板** 深度调校，彻底解决在 tmux / Terminal 下“慢滚太快、快转太慢、卡顿延迟、溜冰反弹”等痛点。

---

## ✨ 核心特性

- ⚡️ **微秒级执行**：纯 Rust 原生零依赖编译，单次执行开销 < 0.05ms，无锁、无阻塞、零延迟。
- 🎯 **慢拨逐行精读**：单格慢拨或刻度精读（$\Delta t \ge 130\text{ms}$）时，严格单行（`1 行`）精准跳动，彻底告别误翻与回弹。
- 🚀 **无极飞轮极速升档**：当拨动 G502 / MX Master 物理飞轮（$\Delta t < 18\text{ms}$）时，根据物理旋转频率与连击动态倍增至 `16 ~ 46 行/次`，轻轻一拨瞬间翻阅数千行日志。
- 🛡 **触控板绝佳适配**：双指轻滑平滑细腻，不再因终端高频事件一滑冲顶。
- 🔒 **按 Pane 独立隔离**：状态存储于 `/tmp` 内存映射，多 Pane、多窗口、多会话并发滚动完全互不干扰。

---

## 📈 动态自适应速度曲线

| 滚动动作 | 触发时间差 ($\Delta t$) | 单次跳行步长 | 体验与场景 |
| :--- | :--- | :--- | :--- |
| **单格刻度 / 逐行精读** | $\ge 50\text{ms}$ | **1 行** | 精准核对单行代码、无误跳 |
| **触摸板连续平滑手势** | $12\text{ms} \sim 40\text{ms}$ | **1 行**（严格锁定） | **触控板双指滑动细腻、平缓可控、不偏快** |
| **中高转速飞轮** | $4.5\text{ms} \sim 7.5\text{ms}$ | **3 ~ 6 行** | 平稳升档 |
| **G502 无极飞轮疾速狂转** | $< 4.5\text{ms}$（超高频物理飞轮） | **6 ~ 14 行** | 飞轮疾速翻越几千行历史 |

---

## 🚀 安装与使用

### 1. 编译并安装

```bash
git clone https://github.com/sususu98/tmux-wheel-accel.git
cd tmux-wheel-accel
./install.sh
```

或者手动使用 Cargo：
```bash
cargo build --release
cp target/release/tmux-wheel-accel ~/.local/bin/
chmod +x ~/.local/bin/tmux-wheel-accel
```

### 2. 配置 `~/.tmux.conf`

在你的 `~/.tmux.conf` 中加入以下绑定：

```tmux
# 启用鼠标支持
set -g mouse on

# 智能动态滚轮加速度（Rust 原生微秒级加速器）
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
或在 tmux 内按下 `Prefix + :` 输入 `source-file ~/.tmux.conf`。

---

## 💡 macOS 配合 Mos 最佳实践

如果你在 macOS 上使用 **Mos**（平滑滚动工具）：
1. 建议在 Mos 的 **偏好设置 -> 例外应用** 中将终端（如 `iTerm2` / `Ghostty` / `Alacritty`）加入例外；
2. **关闭终端的平滑滚动 (`smooth: false`)**，保留自然滚动方向 (`reverse: true`)；
3. 将 Mos 的滚动死区（`deadZone`）设为 `2.0 ~ 2.5`，可彻底消除 G502 重金属滚轮手指离开时的微反弹。

---

## 📄 License

[MIT License](LICENSE) © 2026 sususu98
