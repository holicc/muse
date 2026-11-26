//! 找当前桌面壁纸的图片文件，`wallpaper = "auto"` 时把诗画在它上面。
//!
//! - Linux：没有统一接口，按常见的壁纸程序依次找：Noctalia、swww/awww、hyprpaper、swaybg、
//!   waypaper、KDE Plasma、feh。
//! - macOS：NSWorkspace 给出的主屏幕桌面图片。
//! - Windows：系统设置里的桌面壁纸。

use std::path::PathBuf;

/// 当前桌面壁纸。找不到或文件不存在时返回 None。
pub fn current() -> Option<PathBuf> {
    platform::current().filter(|p| p.is_file())
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use muse_core::config::expand_home;

    pub fn current() -> Option<PathBuf> {
        let finders: [fn() -> Option<PathBuf>; 7] =
            [noctalia, swww, hyprpaper, swaybg, waypaper, plasma, feh];
        finders.iter().find_map(|f| f().filter(|p| p.is_file()))
    }

    fn home() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
    }

    fn xdg(var: &str, fallback: &str) -> Option<PathBuf> {
        std::env::var_os(var)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(fallback)))
    }

    fn read(path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    /// 运行一个命令取标准输出，命令不存在或失败时返回 None。
    fn output(cmd: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(cmd).args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Noctalia：界面里换的壁纸记在状态文件里（按显示器），配置文件里是默认壁纸。
    fn noctalia() -> Option<PathBuf> {
        let state = xdg("XDG_STATE_HOME", ".local/state")?.join("noctalia/settings.toml");
        let config = xdg("XDG_CONFIG_HOME", ".config")?.join("noctalia/config.toml");
        [state, config].iter().find_map(|f| {
            let v: toml::Table = read(f)?.parse().ok()?;
            noctalia_path(&v)
        })
    }

    fn noctalia_path(v: &toml::Table) -> Option<PathBuf> {
        let w = v.get("wallpaper")?.as_table()?;
        let path = |t: &toml::Value| t.get("path")?.as_str().map(PathBuf::from);
        let monitors = w.get("monitors").and_then(|m| m.as_table());
        monitors
            .and_then(|m| m.values().find_map(path))
            .or_else(|| w.get("last").and_then(path))
            .or_else(|| w.get("default").and_then(path))
            .map(|p| expand_home(&p))
    }

    /// swww（新版改名 awww）：`显示器: ..., currently displaying: image: /路径`。
    fn swww() -> Option<PathBuf> {
        let out = output("swww", &["query"]).or_else(|| output("awww", &["query"]))?;
        out.lines().find_map(|l| {
            l.split_once("image: ")
                .map(|(_, p)| PathBuf::from(p.trim()))
        })
    }

    /// hyprpaper：`显示器 = /路径`。
    fn hyprpaper() -> Option<PathBuf> {
        let out = output("hyprctl", &["hyprpaper", "listactive"])?;
        out.lines()
            .find_map(|l| l.split_once(" = ").map(|(_, p)| PathBuf::from(p.trim())))
    }

    /// swaybg：从正在运行的进程参数里找 `-i`/`--image`。
    fn swaybg() -> Option<PathBuf> {
        std::fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
            let raw = std::fs::read(e.path().join("cmdline")).ok()?;
            let args: Vec<_> = raw
                .split(|&b| b == 0)
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            if Path::new(args.first()?).file_name()? != "swaybg" {
                return None;
            }
            swaybg_image(&args)
        })
    }

    fn swaybg_image(args: &[String]) -> Option<PathBuf> {
        args.iter().enumerate().find_map(|(i, a)| {
            if a == "-i" || a == "--image" {
                args.get(i + 1).map(PathBuf::from)
            } else {
                a.strip_prefix("--image=").map(PathBuf::from)
            }
        })
    }

    /// waypaper：`~/.config/waypaper/config.ini` 里的 `wallpaper = 路径`。
    fn waypaper() -> Option<PathBuf> {
        let text = read(&xdg("XDG_CONFIG_HOME", ".config")?.join("waypaper/config.ini"))?;
        text.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == "wallpaper").then(|| expand_home(Path::new(v.trim())))
        })
    }

    /// KDE Plasma：桌面小程序配置里的 `Image=file:///路径`。
    fn plasma() -> Option<PathBuf> {
        let f = xdg("XDG_CONFIG_HOME", ".config")?.join("plasma-org.kde.plasma.desktop-appletsrc");
        let p = read(&f)?.lines().find_map(|l| {
            let v = l.strip_prefix("Image=")?;
            Some(PathBuf::from(v.strip_prefix("file://").unwrap_or(v)))
        })?;
        if !p.is_dir() {
            return Some(p);
        }
        // 默认壁纸是一个壁纸包目录，图在 contents/images/ 下按分辨率放了几张，取最大的文件。
        std::fs::read_dir(p.join("contents/images"))
            .ok()?
            .flatten()
            .filter_map(|e| Some((e.metadata().ok()?.len(), e.path())))
            .filter(|(_, p)| p.is_file())
            .max_by_key(|(n, _)| *n)
            .map(|(_, p)| p)
    }

    /// feh：`~/.fehbg` 最后一个单引号括起来的参数。
    fn feh() -> Option<PathBuf> {
        let text = read(&home()?.join(".fehbg"))?;
        let line = text.lines().rev().find(|l| l.contains("feh"))?;
        let parts: Vec<_> = line.split('\'').collect();
        (parts.len() >= 3).then(|| PathBuf::from(parts[parts.len() - 2]))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn noctalia_prefers_monitor_then_last_then_default() {
            let t: toml::Table = r#"
                [wallpaper.default]
                path = "/d.png"
                [wallpaper.last]
                path = "/l.png"
                [wallpaper.monitors.eDP-1]
                path = "/m.png"
            "#
            .parse()
            .unwrap();
            assert_eq!(noctalia_path(&t), Some(PathBuf::from("/m.png")));
            let t: toml::Table = "[wallpaper.default]\npath = '/d.png'\n".parse().unwrap();
            assert_eq!(noctalia_path(&t), Some(PathBuf::from("/d.png")));
            assert_eq!(noctalia_path(&toml::Table::new()), None);
        }

        #[test]
        fn swaybg_args() {
            let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            assert_eq!(
                swaybg_image(&a(&["swaybg", "-m", "fill", "-i", "/w.jpg"])),
                Some(PathBuf::from("/w.jpg"))
            );
            assert_eq!(
                swaybg_image(&a(&["swaybg", "--image=/w.jpg"])),
                Some(PathBuf::from("/w.jpg"))
            );
            assert_eq!(swaybg_image(&a(&["swaybg", "-c", "#000"])), None);
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::path::PathBuf;

    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    pub fn current() -> Option<PathBuf> {
        // 安全性：都是 AppKit / Foundation 的公开方法，返回的对象由自动释放池或单例持有，
        // 这里只在当前调用内使用。
        unsafe {
            let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let screen: *mut AnyObject = msg_send![class!(NSScreen), mainScreen];
            if ws.is_null() || screen.is_null() {
                return None;
            }
            let url: *mut AnyObject = msg_send![&*ws, desktopImageURLForScreen: &*screen];
            if url.is_null() {
                return None;
            }
            let path: *mut AnyObject = msg_send![&*url, path];
            if path.is_null() {
                return None;
            }
            let s: *const c_char = msg_send![&*path, UTF8String];
            if s.is_null() {
                return None;
            }
            Some(PathBuf::from(
                CStr::from_ptr(s).to_string_lossy().into_owned(),
            ))
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETDESKWALLPAPER, SystemParametersInfoW,
    };

    pub fn current() -> Option<PathBuf> {
        let mut buf = [0u16; 1024];
        // 安全性：缓冲区长度如实传入，系统只会写入这么多字符。
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETDESKWALLPAPER,
                buf.len() as u32,
                buf.as_mut_ptr().cast(),
                0,
            )
        };
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        (ok != 0 && len > 0).then(|| std::ffi::OsString::from_wide(&buf[..len]).into())
    }
}
