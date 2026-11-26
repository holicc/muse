//! 暂停与降帧策略：电池供电时降帧，全屏应用在前台时暂停。
//!
//! 熄屏、锁屏、被完全遮挡时，Wayland 合成器不再发 frame callback，winit 会收到 Occluded，
//! 这两种情况由各自的前端处理，这里只管需要主动查询的状态。

use std::time::{Duration, Instant};

use muse_core::config::Config;

/// 多久重新查询一次电源和全屏状态。
pub const CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// 画面静止（字都显示完、没有粒子）时的刷新间隔。
const IDLE_FRAME: Duration = Duration::from_millis(500);

pub struct Pacer {
    fps: u32,
    battery_fps: u32,
    pause_on_fullscreen: bool,
    on_battery: bool,
    fullscreen: bool,
    next_check: Instant,
}

impl Pacer {
    pub fn new(config: &Config) -> Self {
        let mut p = Self {
            fps: config.fps.max(1),
            battery_fps: config.battery_fps,
            pause_on_fullscreen: config.pause_on_fullscreen,
            on_battery: false,
            fullscreen: false,
            next_check: Instant::now(),
        };
        p.refresh();
        p
    }

    /// 到时间就重新查询系统状态，返回状态是否有变化。
    pub fn refresh(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next_check {
            return false;
        }
        self.next_check = now + CHECK_INTERVAL;
        let before = (self.on_battery, self.fullscreen);
        self.on_battery = on_battery();
        self.fullscreen = self.pause_on_fullscreen && fullscreen_app_in_front();
        let changed = before != (self.on_battery, self.fullscreen);
        if changed {
            eprintln!(
                "电源状态: {}{}",
                if self.on_battery { "电池" } else { "插电" },
                if self.fullscreen {
                    "，全屏应用在前台，暂停"
                } else {
                    ""
                }
            );
        }
        changed
    }

    /// 下一次检查系统状态的时刻，暂停期间靠它唤醒。
    pub fn next_check(&self) -> Instant {
        self.next_check
    }

    /// 两帧之间的间隔；None 表示应当暂停。
    pub fn frame_interval(&self, animating: bool) -> Option<Duration> {
        if self.fullscreen {
            return None;
        }
        let fps = if self.on_battery {
            self.battery_fps
        } else {
            self.fps
        };
        if fps == 0 {
            return None;
        }
        let frame = Duration::from_secs_f64(1.0 / fps as f64);
        Some(if animating {
            frame
        } else {
            frame.max(IDLE_FRAME)
        })
    }
}

// ───────────── 电池 ─────────────

#[cfg(target_os = "linux")]
fn on_battery() -> bool {
    // 有电池且所有交流电源都不在线才算用电池；台式机没有电池，永远是插电。
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return false;
    };
    let read = |p: &std::path::Path, f: &str| {
        std::fs::read_to_string(p.join(f))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let (mut has_battery, mut mains_online, mut has_mains) = (false, false, false);
    for e in entries.flatten() {
        let p = e.path();
        match read(&p, "type").as_str() {
            "Battery" if read(&p, "scope") != "Device" => has_battery = true,
            "Mains" | "USB" => {
                has_mains = true;
                mains_online |= read(&p, "online") == "1";
            }
            _ => {}
        }
    }
    has_battery && has_mains && !mains_online
}

#[cfg(target_os = "macos")]
fn on_battery() -> bool {
    // pmset 第一行形如 "Now drawing from 'Battery Power'"。
    std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("'Battery Power'"))
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn on_battery() -> bool {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // 安全性：传入有效的结构体指针。ACLineStatus 0 = 断开交流电，255 = 未知。
    unsafe { GetSystemPowerStatus(&mut s) != 0 && s.ACLineStatus == 0 }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn on_battery() -> bool {
    false
}

// ───────────── 全屏 ─────────────

#[cfg(target_os = "windows")]
fn fullscreen_app_in_front() -> bool {
    use windows_sys::Win32::UI::Shell::{
        QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
        SHQueryUserNotificationState,
    };
    let mut state = 0;
    // 安全性：传入有效的输出指针。
    let ok = unsafe { SHQueryUserNotificationState(&mut state) } == 0;
    ok && matches!(
        state,
        QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE
    )
}

/// 其他平台靠窗口遮挡事件（macOS、X11）或合成器停发帧回调（Wayland）。
#[cfg(not(target_os = "windows"))]
fn fullscreen_app_in_front() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pacer(fps: u32, battery_fps: u32, on_battery: bool, fullscreen: bool) -> Pacer {
        Pacer {
            fps,
            battery_fps,
            pause_on_fullscreen: true,
            on_battery,
            fullscreen,
            next_check: Instant::now() + CHECK_INTERVAL,
        }
    }

    #[test]
    fn intervals_follow_power_state() {
        assert_eq!(
            pacer(30, 10, false, false).frame_interval(true),
            Some(Duration::from_secs_f64(1.0 / 30.0))
        );
        assert_eq!(
            pacer(30, 10, true, false).frame_interval(true),
            Some(Duration::from_millis(100))
        );
        assert_eq!(pacer(30, 0, true, false).frame_interval(true), None);
        assert_eq!(pacer(30, 10, false, true).frame_interval(true), None);
        assert_eq!(
            pacer(30, 10, false, false).frame_interval(false),
            Some(IDLE_FRAME)
        );
    }
}
