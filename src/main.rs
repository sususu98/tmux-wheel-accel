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
    if dt_ms >= 60.0 {
        // 单格慢拨 / 逐行精读 (1 行)
        1
    } else if dt_ms >= 25.0 {
        // 正常中速滑屏 / 触控板平稳连续滑动 (1 ~ 2 行)
        if streak > 6 {
            2
        } else {
            1
        }
    } else if dt_ms >= 12.0 {
        // 较快拨动 (2 ~ 4 行)
        2 + (streak.min(4) / 2)
    } else if dt_ms >= 6.0 {
        // 快速用力拨轮 (4 ~ 8 行)
        4 + streak.min(4)
    } else {
        // G502 无极飞轮 / MX Master 疾速高频狂转 (< 6ms 超高频) (8 ~ 18 行)
        8 + (streak.min(5) * 2)
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

        let lines = if state.dir_char == dir_char && dt_ms < 70.0 {
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
