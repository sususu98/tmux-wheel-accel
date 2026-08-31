use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

extern "C" {
    fn getuid() -> u32;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Config {
    /// 调试日志开关（true 时向 /tmp/tmux-wheel-accel.log 输出实时判定日志）
    pub debug: bool,
    /// 连续滚动连击超时时间（毫秒，默认 50.0ms）
    pub streak_timeout_ms: f64,
    /// 亚毫秒级同帧子包去抖阈值（毫秒，默认 2.0ms）
    pub debounce_min_dt_ms: f64,
    /// 触发加速所需的起步连击保护次数（默认 3 次）
    pub min_streak_for_accel: u32,
    /// 进入 5档/6档 极速起飞所需的连击门槛（默认 8 次）
    pub high_gear_min_streak: u32,

    /// 1档：慢拨 / 逐行精读（>= 30ms，默认 1 行）
    pub gear1_lines: u32,
    /// 2档：触控板平稳慢速滑动（18ms ~ 30ms，默认 1 行）
    pub gear2_lines: u32,
    /// 3档：中速翻阅（10ms ~ 18ms，默认 3 行）
    pub gear3_lines: u32,
    /// 4档：快速划动 / 快速拨轮（6ms ~ 10ms，默认 6 行）
    pub gear4_lines: u32,
    /// 5档：高速飞划 / 飞轮高速（3ms ~ 6ms，默认 12 行）
    pub gear5_lines: u32,
    /// 6档：触控板用力快划 / G502 物理飞轮全力狂转（< 3ms，默认 24 行）
    pub gear6_lines: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            debug: true,
            streak_timeout_ms: 50.0,
            debounce_min_dt_ms: 2.0,
            min_streak_for_accel: 3,
            high_gear_min_streak: 8,
            gear1_lines: 1,
            gear2_lines: 1,
            gear3_lines: 3,
            gear4_lines: 6,
            gear5_lines: 12,
            gear6_lines: 24,
        }
    }
}

const DEFAULT_CONFIG_TOML: &str = r#"# ~/.config/tmux-wheel-accel/config.toml
# 宽动态范围 6档自适应智能滚轮变速箱配置
# 保存此文件后无需重启 tmux，下次滚动滚轮立即热重载生效！

# 调试日志开关 (true 时实时写入 /tmp/tmux-wheel-accel.log)
debug = true

# 连续滚动判定阈值 (毫秒，两次事件间隔超过此值重置连击)
streak_timeout_ms = 50.0

# 亚毫秒级同帧子包去抖阈值 (毫秒，默认 2.0ms)
debounce_min_dt_ms = 2.0

# 加速起步保护次数（默认 3 次）
min_streak_for_accel = 3

# 进入 5档/6档 高速爆发所需的连击门槛（默认 8 次）
high_gear_min_streak = 8

# ----------------------------------------------------
# 6 档位跳行步长配置 (Gear 1 ~ 6)
# ----------------------------------------------------

# 1档: 单格慢拨 / 逐行精读 (时间间隔 >= 30ms) -> 绝对慢，严格 1 行
gear1_lines = 1

# 2档: 触控板平稳慢滑 (时间间隔 18ms ~ 30ms) -> 依然保持 1 行，逐行细腻
gear2_lines = 1

# 3档: 中速翻阅代码 (时间间隔 10ms ~ 18ms) -> 3 行
gear3_lines = 3

# 4档: 快速划动手势 / 快速拨轮 (时间间隔 6ms ~ 10ms) -> 6 行
gear4_lines = 6

# 5档: 高速飞划 / 飞轮高速旋转 (时间间隔 3ms ~ 6ms, 需连击 >= 8) -> 12 行
gear5_lines = 12

# 6档: 触控板用力快划 / G502 物理飞轮狂转 (时间间隔 < 3ms, 需连击 >= 8) -> 24 行爆发起飞！
gear6_lines = 24
"#;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CachedConfigHeader {
    mtime_sec: i64,
    mtime_nsec: i64,
    config: Config,
}

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

/// Load configuration with sub-microsecond binary cache and automatic hot-reloading on file modification
fn load_config(state_dir: &str) -> Config {
    let config_path = get_config_path();
    let cache_path = format!("{}/cached_config.bin", state_dir);

    // 1. 如果配置文件不存在，自动创建默认 config.toml
    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&config_path, DEFAULT_CONFIG_TOML);
    }

    let meta = match fs::metadata(&config_path) {
        Ok(m) => m,
        Err(_) => return Config::default(),
    };
    let mtime_sec = meta.mtime();
    let mtime_nsec = meta.mtime_nsec();

    // 2. 检查二进制缓存是否存在且有效
    if let Ok(mut cache_file) = OpenOptions::new().read(true).open(&cache_path) {
        let mut buf = [0u8; std::mem::size_of::<CachedConfigHeader>()];
        if cache_file.read_exact(&mut buf).is_ok() {
            let cached: CachedConfigHeader =
                unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const CachedConfigHeader) };
            if cached.mtime_sec == mtime_sec && cached.mtime_nsec == mtime_nsec {
                return cached.config;
            }
        }
    }

    // 3. 缓存失效或不存在：解析 TOML 配置文件并写入高速二进制缓存
    let config: Config = fs::read_to_string(&config_path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default();

    let new_cache = CachedConfigHeader {
        mtime_sec,
        mtime_nsec,
        config,
    };

    if let Ok(mut cache_file) = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&cache_path)
    {
        let slice = unsafe {
            std::slice::from_raw_parts(
                &new_cache as *const CachedConfigHeader as *const u8,
                std::mem::size_of::<CachedConfigHeader>(),
            )
        };
        let _ = cache_file.write_all(slice);
    }

    config
}

/// 6 档位宽动态范围自适应变速计算，返回 (行数, 档位)
#[inline]
fn calculate_lines(dt_ms: f64, streak: u32, cfg: &Config) -> (u32, u8) {
    if dt_ms >= 30.0 || streak < cfg.min_streak_for_accel {
        (cfg.gear1_lines, 1)
    } else if dt_ms >= 18.0 {
        (cfg.gear2_lines, 2)
    } else if dt_ms >= 10.0 {
        (cfg.gear3_lines, 3)
    } else if dt_ms >= 6.0 {
        (cfg.gear4_lines, 4)
    } else {
        if streak >= cfg.high_gear_min_streak {
            if dt_ms >= 3.0 {
                (cfg.gear5_lines, 5)
            } else {
                (cfg.gear6_lines, 6)
            }
        } else {
            (cfg.gear4_lines, 4)
        }
    }
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

fn main() {
    let mut args = env::args().skip(1);
    let pane = match args.next() {
        Some(p) if !p.is_empty() => p,
        _ => return,
    };
    let dir = match args.next() {
        Some(d) if !d.is_empty() => d,
        _ => return,
    };

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

    // 加载配置（具备微秒级二进制缓存 + 配置文件修改即时热重载）
    let config = load_config(&state_dir);

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

        // 亚毫秒级去抖
        if state.dir_char == dir_char && dt_ms < config.debounce_min_dt_ms {
            if config.debug {
                log_debug(&format!(
                    "[DEBOUNCE] pane: {}, dir: {}, dt: {:.2}ms < {:.2}ms (duplicate sub-packet dropped)",
                    pane, dir_str, dt_ms, config.debounce_min_dt_ms
                ));
            }
            return;
        }

        let (lines, gear) = if state.dir_char == dir_char && dt_ms < config.streak_timeout_ms {
            state.streak = state.streak.saturating_add(1);
            calculate_lines(dt_ms, state.streak, &config)
        } else {
            state.streak = 0;
            (config.gear1_lines, 1)
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
                "[SCROLL] pane: {}, dir: {}, dt: {:>6.2}ms, streak: {:>2}, gear: {}档 -> scroll {} lines",
                pane, dir_str, dt_ms, state.streak, gear, lines
            ));
        }

        // 执行 tmux 滚动指令
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", &pane, "-X", "-N", &lines.to_string(), dir_str])
            .status();
    } else {
        // Fallback: 默认走 1档
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", &pane, "-X", "-N", &config.gear1_lines.to_string(), dir_str])
            .status();
    }
}
