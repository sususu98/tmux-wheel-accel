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
    /// 连续滚动连击超时时间（毫秒，默认 50.0ms）
    pub streak_timeout_ms: f64,
    /// 触发加速所需的起步连击保护次数（默认 4 次）
    pub min_streak_for_accel: u32,

    /// 1档：单格慢拨 / 逐行精读（>= 50ms，默认 2 行）
    pub gear1_lines: u32,
    /// 2档：触摸板平稳手势 / 慢速连续巡航（12ms ~ 50ms，默认 2 行）
    pub gear2_lines: u32,
    /// 3档：正常中速看代码（7ms ~ 12ms，默认 4 行）
    pub gear3_lines: u32,
    /// 4档：快速连续拨轮（4ms ~ 7ms，默认 8 行）
    pub gear4_lines: u32,
    /// 5档：无极飞轮中高速（2ms ~ 4ms，默认 16 行）
    pub gear5_lines: u32,
    /// 6档：G502 物理无极飞轮红线极速起飞（< 2ms 超高频，默认 32 行）
    pub gear6_lines: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            streak_timeout_ms: 50.0,
            min_streak_for_accel: 4,
            gear1_lines: 2,
            gear2_lines: 2,
            gear3_lines: 4,
            gear4_lines: 8,
            gear5_lines: 16,
            gear6_lines: 32,
        }
    }
}

const DEFAULT_CONFIG_TOML: &str = r#"# ~/.config/tmux-wheel-accel/config.toml
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

    // 2. 检查二进制缓存是否存在且有效 (读取 56 字节内存结构，耗时 < 1 微秒)
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

/// 6 档位自适应变速计算
#[inline]
fn calculate_lines(dt_ms: f64, streak: u32, cfg: &Config) -> u32 {
    // 1档 / 起步保护
    if dt_ms >= 50.0 || streak < cfg.min_streak_for_accel {
        return cfg.gear1_lines;
    }

    if dt_ms >= 12.0 {
        // 2档: 触摸板手势 / 巡航慢速 (锁定 2 行)
        cfg.gear2_lines
    } else if dt_ms >= 7.0 {
        // 3档: 较快翻阅
        cfg.gear3_lines
    } else if dt_ms >= 4.0 {
        // 4档: 快速拨轮
        cfg.gear4_lines
    } else if dt_ms >= 2.0 {
        // 5档: 无极飞轮高速
        cfg.gear5_lines
    } else {
        // 6档: G502 无极飞轮红线极速起飞！
        cfg.gear6_lines
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
            config.gear1_lines
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
        // Fallback: 默认走 1档
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", &pane, "-X", "-N", &config.gear1_lines.to_string(), dir_str])
            .status();
    }
}
