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
    /// 单格慢拨与逐行精读的基础行数（默认 1 行）
    pub base_lines: u32,
    /// 连续滚动连击超时时间（毫秒，默认 50.0ms，超过此间隔重置连击）
    pub streak_timeout_ms: f64,
    /// 进入加速所需的最小连击次数（默认 5 次，防误触）
    pub min_streak_for_accel: u32,
    /// 触摸板与慢速保护阈值（毫秒，默认 7.5ms，>= 此值严格锁定为 base_lines）
    pub trackpad_lock_threshold_ms: f64,
    /// 中转速飞轮阈值（毫秒，默认 4.5ms）
    pub medium_accel_threshold_ms: f64,
    /// 中转速飞轮基础行数（默认 3 行）
    pub medium_accel_base_lines: u32,
    /// 中转速飞轮最大额外加成行数（默认 3 行）
    pub medium_accel_max_boost: u32,
    /// 疾速狂转飞轮基础行数（默认 6 行）
    pub high_accel_base_lines: u32,
    /// 疾速狂转飞轮最大额外加成行数（默认 8 行）
    pub high_accel_max_boost: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_lines: 1,
            streak_timeout_ms: 50.0,
            min_streak_for_accel: 5,
            trackpad_lock_threshold_ms: 7.5,
            medium_accel_threshold_ms: 4.5,
            medium_accel_base_lines: 3,
            medium_accel_max_boost: 3,
            high_accel_base_lines: 6,
            high_accel_max_boost: 8,
        }
    }
}

const DEFAULT_CONFIG_TOML: &str = r#"# ~/.config/tmux-wheel-accel/config.toml
# tmux-wheel-accel 核心参数配置文件
# 修改此文件后无需重启 tmux，下次滚动滚轮立即热加载生效！

# 单格慢拨与逐行精读的基础行数（默认 1 行）
base_lines = 1

# 连续滚动连击超时时间 (毫秒)
# 两次事件间隔超过此值时，判定为全新的单次滚动，重置连击计数器
streak_timeout_ms = 50.0

# 进入加速档位所需的最小连续触发次数（防误触保护，默认 5 次）
min_streak_for_accel = 5

# 触摸板与慢速手势保护阈值 (毫秒)
# 触摸板滑动间隔通常在 12ms ~ 40ms，事件间隔 >= 该值时严格锁定为 base_lines
trackpad_lock_threshold_ms = 7.5

# 中转速飞轮阈值 (毫秒)
medium_accel_threshold_ms = 4.5

# 中转速飞轮基础行数与最大额外加成
medium_accel_base_lines = 3
medium_accel_max_boost = 3

# G502 / MX Master 物理飞轮疾速狂转 (< medium_accel_threshold_ms) 基础行数与最大加成
high_accel_base_lines = 6
high_accel_max_boost = 8
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

    // 2. 检查二进制缓存是否存在且有效 (直接读取 48 字节内存结构，耗时 < 1 微秒)
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

/// Calculate dynamic scroll jump lines based on event delta time (ms), streak, and config
#[inline]
fn calculate_lines(dt_ms: f64, streak: u32, cfg: &Config) -> u32 {
    // 触摸板与慢速手势保护
    if dt_ms >= cfg.trackpad_lock_threshold_ms || streak < cfg.min_streak_for_accel {
        return cfg.base_lines;
    }

    let extra_streak = streak.saturating_sub(cfg.min_streak_for_accel);

    if dt_ms >= cfg.medium_accel_threshold_ms {
        // 中速飞轮
        cfg.medium_accel_base_lines + extra_streak.min(cfg.medium_accel_max_boost)
    } else {
        // G502 / MX Master 疾速狂转飞轮
        cfg.high_accel_base_lines + (extra_streak.min(cfg.high_accel_max_boost / 2) * 2)
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

        let lines = if state.dir_char == dir_char && dt_ms < config.streak_timeout_ms {
            state.streak = state.streak.saturating_add(1);
            calculate_lines(dt_ms, state.streak, &config)
        } else {
            state.streak = 0;
            config.base_lines
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

        // 执行 tmux 滚动指令
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", &pane, "-X", "-N", &lines.to_string(), dir_str])
            .status();
    } else {
        // Fallback: 默认单行滚动
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", &pane, "-X", "-N", &config.base_lines.to_string(), dir_str])
            .status();
    }
}
