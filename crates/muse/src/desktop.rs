//! 把普通 winit 窗口变成桌面层窗口（X11、macOS、Windows）。
//!
//! - X11：窗口类型设为 `_NET_WM_WINDOW_TYPE_DESKTOP`，窗口管理器会把它放在最底层。
//! - macOS：窗口层级设为桌面层，忽略鼠标，出现在所有空间。
//! - Windows：挂到桌面图标后面的 WorkerW 窗口下面（和 Wallpaper Engine 同样的做法）。

use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowAttributes};

/// 覆盖整个显示器、无边框的窗口属性。
pub fn attributes(monitor: &MonitorHandle) -> WindowAttributes {
    let pos: PhysicalPosition<i32> = monitor.position();
    let size: PhysicalSize<u32> = monitor.size();
    let attrs = Window::default_attributes()
        .with_title("muse")
        .with_decorations(false)
        .with_resizable(false)
        .with_position(pos)
        .with_inner_size(size);
    platform::attributes(attrs)
}

/// 窗口创建后做的平台处理。
pub fn attach(window: &Window, monitor: &MonitorHandle) -> Result<(), String> {
    platform::attach(window, monitor)
}

/// 退出前恢复桌面（Windows 需要让系统重画原来的壁纸）。
pub fn restore() {
    platform::restore();
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use winit::monitor::MonitorHandle;
    use winit::platform::x11::{WindowAttributesExtX11, WindowType};
    use winit::window::{Window, WindowAttributes};

    pub fn attributes(attrs: WindowAttributes) -> WindowAttributes {
        attrs.with_x11_window_type(vec![WindowType::Desktop])
    }

    pub fn attach(_: &Window, _: &MonitorHandle) -> Result<(), String> {
        Ok(())
    }

    pub fn restore() {}
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::monitor::MonitorHandle;
    use winit::window::{Window, WindowAttributes};

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWindowLevelForKey(key: i32) -> i32;
    }
    const DESKTOP_WINDOW_LEVEL_KEY: i32 = 2;

    // NSWindowCollectionBehavior
    const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
    const STATIONARY: usize = 1 << 4;
    const IGNORES_CYCLE: usize = 1 << 6;

    pub fn attributes(attrs: WindowAttributes) -> WindowAttributes {
        attrs
    }

    pub fn attach(window: &Window, _: &MonitorHandle) -> Result<(), String> {
        let handle = window
            .window_handle()
            .map_err(|e| format!("取不到窗口句柄: {e}"))?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            return Err("不是 AppKit 窗口".into());
        };
        // 安全性：ns_view 是 winit 创建、在窗口存活期间有效的 NSView；以下都是 NSWindow 的公开方法，
        // 必须在主线程调用（winit 的事件回调就在主线程）。
        unsafe {
            let view: &AnyObject = h.ns_view.cast().as_ref();
            let ns_window: *mut AnyObject = msg_send![view, window];
            let Some(ns_window) = ns_window.as_ref() else {
                return Err("NSView 没有所属窗口".into());
            };
            let level = CGWindowLevelForKey(DESKTOP_WINDOW_LEVEL_KEY) as isize;
            let _: () = msg_send![ns_window, setLevel: level];
            let _: () = msg_send![ns_window, setIgnoresMouseEvents: Bool::YES];
            let _: () = msg_send![ns_window, setHasShadow: Bool::NO];
            let behavior = CAN_JOIN_ALL_SPACES | STATIONARY | IGNORES_CYCLE;
            let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
        }
        Ok(())
    }

    pub fn restore() {}
}

#[cfg(target_os = "windows")]
mod platform {
    use std::ffi::c_void;
    use std::ptr::null;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowExW, FindWindowW, GetSystemMetrics, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN, SMTO_NORMAL, SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER, SWP_NOACTIVATE,
        SWP_NOZORDER, SWP_SHOWWINDOW, SendMessageTimeoutW, SetParent, SetWindowPos,
        SystemParametersInfoW,
    };
    use winit::monitor::MonitorHandle;
    use winit::platform::windows::WindowAttributesExtWindows;
    use winit::window::{Window, WindowAttributes};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn attributes(attrs: WindowAttributes) -> WindowAttributes {
        attrs.with_skip_taskbar(true)
    }

    /// 找到桌面图标后面那层 WorkerW。
    unsafe fn find_wallpaper_parent() -> HWND {
        unsafe {
            let progman = FindWindowW(wide("Progman").as_ptr(), null());
            if progman.is_null() {
                return std::ptr::null_mut();
            }
            // 让 Progman 在图标层后面生成一个 WorkerW。
            let mut result = 0usize;
            SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, SMTO_NORMAL, 1000, &mut result);

            unsafe extern "system" fn find(hwnd: HWND, out: LPARAM) -> i32 {
                unsafe {
                    let defview = FindWindowExW(
                        hwnd,
                        std::ptr::null_mut(),
                        wide("SHELLDLL_DefView").as_ptr(),
                        null(),
                    );
                    if !defview.is_null() {
                        // 旧版本：WorkerW 是装着图标那个窗口的下一个兄弟窗口。
                        *(out as *mut HWND) = FindWindowExW(
                            std::ptr::null_mut(),
                            hwnd,
                            wide("WorkerW").as_ptr(),
                            null(),
                        );
                    }
                }
                TRUE
            }
            let mut workerw: HWND = std::ptr::null_mut();
            EnumWindows(Some(find), &mut workerw as *mut HWND as LPARAM);
            if workerw.is_null() {
                // Windows 11 24H2 起 WorkerW 变成 Progman 的子窗口。
                workerw = FindWindowExW(
                    progman,
                    std::ptr::null_mut(),
                    wide("WorkerW").as_ptr(),
                    null(),
                );
            }
            if workerw.is_null() { progman } else { workerw }
        }
    }

    pub fn attach(window: &Window, monitor: &MonitorHandle) -> Result<(), String> {
        let handle = window
            .window_handle()
            .map_err(|e| format!("取不到窗口句柄: {e}"))?;
        let RawWindowHandle::Win32(h) = handle.as_raw() else {
            return Err("不是 Win32 窗口".into());
        };
        let hwnd = h.hwnd.get() as *mut c_void;
        // 安全性：hwnd 是 winit 创建的有效窗口；其余句柄来自系统查询，空值已检查。
        unsafe {
            let parent = find_wallpaper_parent();
            if parent.is_null() {
                return Err("找不到桌面窗口（Progman），无法嵌入壁纸层".into());
            }
            if SetParent(hwnd, parent).is_null() {
                return Err("无法把窗口挂到桌面层（SetParent 失败）".into());
            }
            // 子窗口坐标相对父窗口，父窗口覆盖整个虚拟屏幕。
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let pos = monitor.position();
            let size = monitor.size();
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                pos.x - vx,
                pos.y - vy,
                size.width as i32,
                size.height as i32,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        Ok(())
    }

    /// 我们的窗口销毁后桌面会留着最后一帧，重新设置一次当前壁纸让系统重画。
    pub fn restore() {
        let mut path = [0u16; 1024];
        // 安全性：缓冲区长度和传入的大小一致。
        unsafe {
            if SystemParametersInfoW(
                SPI_GETDESKWALLPAPER,
                path.len() as u32,
                path.as_mut_ptr().cast(),
                0,
            ) != 0
            {
                SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, path.as_mut_ptr().cast(), 0);
            }
        }
    }
}
