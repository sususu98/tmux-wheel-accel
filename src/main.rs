use serde::{Deserialize, Serialize};
use std::env;
use std::ffi::c_void;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[link(name = "ApplicationServices", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn getuid() -> u32;
    fn kill(pid: i32, sig: i32) -> i32;

    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void,
        user_info: *mut c_void,
    ) -> *mut c_void;

    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: *mut c_void,
        order: isize,
    ) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CFRunLoopRun();

    static kCFRunLoopCommonModes: *const c_void;
}

const K_CG_SESSION_EVENT_TAP: u32 = 1;
const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
const K_CG_EVENT_SCROLL_WHEEL: u32 = 22;
const K_CG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS: u32 = 88;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TrackpadConfig {
    /// 慢速滑动 (>= 14ms, 60Hz 帧)
    pub slow_lines: u32,
    /// 中速滑动 (8ms ~ 14ms)
    pub medium_lines: u32,
    /// 快速轻拂 (4ms ~ 8ms)
    pub fast_lines: u32,
    /// 极速狂划封顶步长 (< 4ms)
    pub max_lines: u32,
}

impl Default for TrackpadConfig {
    fn default() -> Self {
        Self {
            slow_lines: 1,
            medium_lines: 2,
            fast_lines: 4,
            max_lines: 6,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MouseConfig {
    /// 刻度单拨 (>= 40ms)
    pub notch_lines: u32,
    /// 普通中速拨轮 (15ms ~ 40ms)
    pub normal_lines: u32,
    /// 快速拨轮 (7ms ~ 15ms)
    pub fast_lines: u32,
    /// 无极飞轮中高速 (3ms ~ 7ms)
    pub high_lines: u32,
    /// G502 物理无极飞轮全力狂转 (< 3ms)
    pub freespin_lines: u32,
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            notch_lines: 1,
            normal_lines: 4,
            fast_lines: 12,
            high_lines: 28,
            freespin_lines: 64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub debug: bool,
    pub streak_timeout_ms: f64,
    pub debounce_min_dt_ms: f64,
    pub trackpad: TrackpadConfig,
    pub mouse: MouseConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            debug: true,
            streak_timeout_ms: 50.0,
            debounce_min_dt_ms: 2.0,
            trackpad: TrackpadConfig::default(),
            mouse: MouseConfig::default(),
        }
    }
}

const DEFAULT_CONFIG_TOML: &str = r#"# ~/.config/tmux-wheel-accel/config.toml
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
"#;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct State {
    last_ts_micros: u64,
    streak: u32,
    dir_char: u8,
}

fn get_now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

fn get_config_path() -> PathBuf {
    if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(config_home).join("tmux-wheel-accel/config.toml")
    } else if let Ok(home) = env::var("HOME") {
        PathBuf::from(home).join(".config/tmux-wheel-accel/config.toml")
    } else {
        PathBuf::from("/tmp/tmux-wheel-accel-config.toml")
    }
}

fn load_config() -> Config {
    let config_path = get_config_path();
    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&config_path, DEFAULT_CONFIG_TOML);
    }

    fs::read_to_string(&config_path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}

fn log_debug(msg: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/tmux-wheel-accel.log")
    {
        let _ = writeln!(file, "{}", msg);
    }
}

// ----------------------------------------------------
// 后台设备感知守护进程 (CoreGraphics EventTap)
// ----------------------------------------------------

extern "C" fn tap_callback(
    _proxy: *mut c_void,
    event_type: u32,
    event: *mut c_void,
    _user_info: *mut c_void,
) -> *mut c_void {
    if event_type == K_CG_EVENT_SCROLL_WHEEL {
        let is_continuous =
            unsafe { CGEventGetIntegerValueField(event, K_CG_SCROLL_WHEEL_EVENT_IS_CONTINUOUS) };
        let device_type: u8 = if is_continuous != 0 { 1 } else { 0 }; // 1 = Trackpad, 0 = Mouse

        let uid = unsafe { getuid() };
        let dev_file = format!("/tmp/tmux-wheel-accel-{}/device_type.bin", uid);
        let _ = fs::write(&dev_file, [device_type]);
    }
    event
}

fn run_daemon() {
    let uid = unsafe { getuid() };
    let state_dir = format!("/tmp/tmux-wheel-accel-{}", uid);
    let _ = fs::create_dir_all(&state_dir);
    let pid_file = format!("{}/daemon.pid", state_dir);
    let _ = fs::write(&pid_file, std::process::id().to_string());

    let mask = 1u64 << K_CG_EVENT_SCROLL_WHEEL;
    let tap = unsafe {
        CGEventTapCreate(
            K_CG_SESSION_EVENT_TAP,
            K_CG_HEAD_INSERT_EVENT_TAP,
            K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
            mask,
            tap_callback,
            std::ptr::null_mut(),
        )
    };

    if tap.is_null() {
        eprintln!("Failed to create event tap.");
        return;
    }

    let source = unsafe { CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0) };
    unsafe {
        let rl = CFRunLoopGetCurrent();
        CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        CFRunLoopRun();
    }
}

/// 确保后台设备感知守护进程处于运行状态
fn ensure_daemon_running() {
    let uid = unsafe { getuid() };
    let state_dir = format!("/tmp/tmux-wheel-accel-{}", uid);
    let pid_file = format!("{}/daemon.pid", state_dir);

    let is_alive = if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe { kill(pid, 0) == 0 }
        } else {
            false
        }
    } else {
        false
    };

    if !is_alive {
        if let Ok(self_exe) = env::current_exe() {
            let _ = Command::new(self_exe).arg("--daemon").spawn();
        }
    }
}

/// 获取当前滚动的物理设备类型 (0 = Mouse G502, 1 = Magic Trackpad)
fn get_current_device_type(state_dir: &str) -> u8 {
    let dev_file = format!("{}/device_type.bin", state_dir);
    if let Ok(bytes) = fs::read(&dev_file) {
        if !bytes.is_empty() {
            return bytes[0];
        }
    }
    1 // 默认作为触控板温和模式
}

/// 触控板计算逻辑
#[inline]
fn calculate_trackpad_lines(dt_ms: f64, cfg: &TrackpadConfig) -> (u32, &'static str) {
    if dt_ms >= 14.0 {
        (cfg.slow_lines, "慢滑")
    } else if dt_ms >= 8.0 {
        (cfg.medium_lines, "中速")
    } else if dt_ms >= 4.0 {
        (cfg.fast_lines, "快划")
    } else {
        (cfg.max_lines, "极速封顶")
    }
}

/// 鼠标计算逻辑
#[inline]
fn calculate_mouse_lines(dt_ms: f64, cfg: &MouseConfig) -> (u32, &'static str) {
    if dt_ms >= 40.0 {
        (cfg.notch_lines, "刻度单拨")
    } else if dt_ms >= 15.0 {
        (cfg.normal_lines, "普通拨轮")
    } else if dt_ms >= 7.0 {
        (cfg.fast_lines, "快速拨轮")
    } else if dt_ms >= 3.0 {
        (cfg.high_lines, "飞轮高速")
    } else {
        (cfg.freespin_lines, "无极飞轮狂转起飞")
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() >= 2 && args[1] == "--daemon" {
        run_daemon();
        return;
    }

    if args.len() < 3 {
        return;
    }

    let pane = &args[1];
    let dir = &args[2];

    let dir_char = if dir.starts_with('u') || dir.starts_with('U') {
        b'u'
    } else {
        b'd'
    };
    let dir_str = if dir_char == b'u' { "scroll-up" } else { "scroll-down" };

    let clean_pane = pane.trim_start_matches('%');
    let uid = unsafe { getuid() };
    let state_dir = format!("/tmp/tmux-wheel-accel-{}", uid);
    let _ = fs::create_dir_all(&state_dir);
    let state_path = format!("{}/pane_{}.bin", state_dir, clean_pane);

    // 确保后台设备感知守护进程存活
    ensure_daemon_running();

    let config = load_config();
    let is_trackpad = get_current_device_type(&state_dir) == 1;

    let now_micros = get_now_micros();
    let mut state = State::default();

    if let Ok(mut file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .open(&state_path)
    {
        let mut buf = [0u8; std::mem::size_of::<State>()];
        if file.read_exact(&mut buf).is_ok() {
            state = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const State) };
        }

        let dt_ms = if state.last_ts_micros > 0 && now_micros >= state.last_ts_micros {
            (now_micros - state.last_ts_micros) as f64 / 1000.0
        } else {
            1000.0
        };

        // 亚毫秒级去抖（仅针对触控板同帧重复包）
        if is_trackpad && state.dir_char == dir_char && dt_ms < config.debounce_min_dt_ms {
            if config.debug {
                log_debug(&format!(
                    "[DEBOUNCE: Trackpad] pane: {}, dir: {}, dt: {:.2}ms < {:.2}ms",
                    pane, dir_str, dt_ms, config.debounce_min_dt_ms
                ));
            }
            return;
        }

        let (lines, stage) = if is_trackpad {
            let (l, s) = calculate_trackpad_lines(dt_ms, &config.trackpad);
            (l, s)
        } else {
            let (l, s) = calculate_mouse_lines(dt_ms, &config.mouse);
            (l, s)
        };

        state.last_ts_micros = now_micros;
        state.dir_char = dir_char;

        let _ = file.seek(SeekFrom::Start(0));
        let slice = unsafe {
            std::slice::from_raw_parts(
                &state as *const State as *const u8,
                std::mem::size_of::<State>(),
            )
        };
        let _ = file.write_all(slice);

        if config.debug {
            log_debug(&format!(
                "[SCROLL: {}] pane: {}, dir: {}, dt: {:>6.2}ms, mode: {} -> scroll {} lines",
                if is_trackpad { "Trackpad" } else { "Mouse G502" },
                pane,
                dir_str,
                dt_ms,
                stage,
                lines
            ));
        }

        // 执行 tmux 滚动指令
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", &pane, "-X", "-N", &lines.to_string(), dir_str])
            .status();
    }
}
