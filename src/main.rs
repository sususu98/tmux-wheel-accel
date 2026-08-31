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
    /// 亚毫秒重复子包去抖阈值（毫秒，默认 3.5ms，小于该间隔的同帧重复包直接合并去抖）
    pub debounce_min_dt_ms: f64,
    /// 触发加速所需的起步连击保护次数（默认 4 次）
    pub min_streak_for_accel: u32,
    /// 进入 5档/6档 极速飞轮所需的持续连击门槛（默认 25 次）
    pub high_gear_min_streak: u32,

    /// 1档：单格慢拨 / 逐行精读（>= 45ms，默认 1 行）
    pub gear1_lines: u32,
    /// 2档：触摸板平稳手势 / 慢速连续巡航（12ms ~ 45ms，默认 1 行）
    pub gear2_lines: u32,
    /// 3档：较快翻阅代码（7ms ~ 12ms，默认 2 行）
    pub gear3_lines: u32,
    /// 4档：快速连续拨轮（4.5ms ~ 7ms，默认 4 行）
    pub gear4_lines: u32,
    /// 5档：无极飞轮中高速（3.5ms ~ 4.5ms，需 streak >= high_gear_min_streak，默认 8 行）
    pub gear5_lines: u32,
    /// 6档：G502 物理飞轮全力狂转（需 streak >= high_gear_min_streak，默认 16 行）
    pub gear6_lines: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            streak_timeout_ms: 50.0,
            debounce_min_dt_ms: 3.5,
            min_streak_for_accel: 4,
            high_gear_min_streak: 25,
            gear1_lines: 1,
            gear2_lines: 1,
            gear3_lines: 2,
            gear4_lines: 4,
            gear5_lines: 8,
            gear6_lines: 16,
        }
    }
}

const DEFAULT_CONFIG_TOML: &str = r#"# ~/.config/tmux-wheel-accel/config.toml
# 6档自适应智能滚轮变速箱配置
# 保存此文件后无需重启 tmux，下次滚动滚轮立即热重载生效！

# 连续滚动判定阈值 (毫秒，两次事件间隔超过此值重置连击)
streak_timeout_ms = 50.0

# 亚毫秒级同帧子包去抖阈值 (毫秒，默认 3.5ms)
# macOS / iTerm2 在每个 60Hz 帧内会发出 < 1ms 的成对子包，去抖过滤后可彻底消除触控板暴冲
debounce_min_dt_ms = 3.5

# 加速起步保护次数（前 N 次滚动严格保持 1档/2档，防误触）
min_streak_for_accel = 4

# 进入 5档/6档 极速飞轮所需的持续连击门槛（默认 25 次）
# 触控板划一下通常只有 10~15 个微事件，会被安全锁在 1~3档（绝不会飞掉上千行）；
# 只有 G502 物理飞轮持续长旋超过 25 次连击，才会平滑升入 5档/6档
high_gear_min_streak = 25

# ----------------------------------------------------
# 6 档位跳行步长配置 (Gear 1 ~ 6)
# ----------------------------------------------------

# 1档: 单格慢拨 / 逐行精读 (时间间隔 >= 45ms)
gear1_lines = 1

# 2档: 触摸板平稳手势 / 中慢速巡航 (时间间隔 12ms ~ 45ms)
# 触摸板滑动主要落在此档，锁定 1 行保证极致细腻不偏快
gear2_lines = 1

# 3档: 较快翻阅代码 (时间间隔 7ms ~ 12ms)
gear3_lines = 2

# 4档: 快速拨轮翻段落 (时间间隔 4.5ms ~ 7ms)
gear4_lines = 4

# 5档: G502 无极飞轮中高速旋转 (时间间隔 3.5ms ~ 4.5ms, 需连击 >= high_gear_min_streak)
gear5_lines = 8

# 6档: G502 物理无极飞轮全力狂转 / 疾速起飞 (需连击 >= high_gear_min_streak)
gear6_lines = 16
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

    // 2. 检查二进制缓存是否存在且有效 (读取 64 字节内存结构，耗时 < 1 微秒)
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

/// 6 档位自适应变速计算（去抖 + 速度 dt 与连击深度 streak 双重把关）
#[inline]
fn calculate_lines(dt_ms: f64, streak: u32, cfg: &Config) -> u32 {
    // 1档 / 起步保护
    if dt_ms >= 45.0 || streak < cfg.min_streak_for_accel {
        return cfg.gear1_lines;
    }

    if dt_ms >= 12.0 {
        // 2档: 触摸板常规平稳手势 (严格 1 行)
        cfg.gear2_lines
    } else if dt_ms >= 7.0 {
        // 3档: 较快翻阅 (2 行)
        cfg.gear3_lines
    } else if dt_ms >= 4.5 {
        // 4档: 快速拨轮 (4 行)
        cfg.gear4_lines
    } else {
        // 只有连击深度 >= high_gear_min_streak (G502 持续物理飞轮)，才允许升入 5档 / 6档
        // 触摸板快速划一下只有 10~15 个事件，会被安全限制在 3~4 档
        if streak >= cfg.high_gear_min_streak {
            if dt_ms >= 3.5 {
                cfg.gear5_lines
            } else {
                cfg.gear6_lines
            }
        } else {
            cfg.gear4_lines
        }
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

        // 关键去抖保护：如果同一个方向的事件间隔 < debounce_min_dt_ms (3.5ms)，
        // 说明是 macOS/iTerm2 同一物理帧内丢出来的成对子包，直接去抖忽略，防止微观成对子包造成速度误翻倍！
        if state.dir_char == dir_char && dt_ms < config.debounce_min_dt_ms {
            return;
        }

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
