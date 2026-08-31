use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

extern "C" {
    fn getuid() -> u32;
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

/// Calculate dynamic scroll jump lines based on event delta time (ms) and consecutive streak
#[inline]
fn calculate_lines(dt_ms: f64, streak: u32) -> u32 {
    // 1. 触摸板与普通中慢速滚动保护：
    // 触摸板连续手势的频率通常在 12ms ~ 40ms (30~80Hz)
    // 只要时间间隔 >= 7.5ms，或者连击处于前段，绝对严格锁定为 1 行！
    if dt_ms >= 7.5 || streak < 5 {
        return 1;
    }

    // 2. 只有当物理无极滚轮狂转 (< 7.5ms，通常 2ms ~ 5ms，> 150Hz 超高频物理飞轮)
    // 且持续旋转时，才平滑升档加速：
    if dt_ms >= 4.5 {
        // 中高转速飞轮 (3 ~ 6 行)
        3 + ((streak - 4).min(3))
    } else {
        // G502 极致极速狂转飞轮 (< 4.5ms) (6 ~ 14 行)
        6 + ((streak - 4).min(4) * 2)
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

        let lines = if state.dir_char == dir_char && dt_ms < 60.0 {
            state.streak = state.streak.saturating_add(1);
            calculate_lines(dt_ms, state.streak)
        } else {
            state.streak = 0;
            1
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
            .args(["send-keys", "-t", &pane, "-X", "-N", "1", dir_str])
            .status();
    }
}
