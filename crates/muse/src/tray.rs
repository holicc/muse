//! 托盘图标：下一首、暂停 / 继续、打开设计窗口、退出。
//!
//! Linux 用 StatusNotifierItem（D-Bus，KDE、waybar、niri 配合 waybar 等都支持），
//! macOS、Windows 用 tray-icon，必须在主线程、事件循环启动后创建。

use crate::Overrides;
use crate::control::Remote;

/// 托盘里打开设计窗口那一项的文字。
const DESIGN_LABEL: &str = "调整与设计…";

/// 托盘图标的边长。
pub const ICON_SIZE: u32 = 64;

/// 图标像素：RGBA8，边长 ICON_SIZE。
pub struct IconImage(pub Vec<u8>);

#[cfg(target_os = "linux")]
mod imp {
    use ksni::blocking::{Handle, TrayMethods};

    use super::{DESIGN_LABEL, ICON_SIZE, IconImage};
    use crate::Overrides;
    use crate::control::{Command, Remote};

    struct Sni {
        remote: Remote,
        paused: bool,
        icon: Vec<u8>,
        overrides: Overrides,
        design: bool,
    }

    impl ksni::Tray for Sni {
        fn id(&self) -> String {
            "muse".into()
        }

        fn title(&self) -> String {
            "muse 诗词壁纸".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            vec![ksni::Icon {
                width: ICON_SIZE as i32,
                height: ICON_SIZE as i32,
                data: self.icon.clone(),
            }]
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            let item = |label: &str, c: Command| {
                StandardItem {
                    label: label.into(),
                    activate: Box::new(move |t: &mut Self| t.remote.send(c)),
                    ..Default::default()
                }
                .into()
            };
            vec![
                item("下一首", Command::Next),
                item(
                    if self.paused { "继续" } else { "暂停" },
                    Command::TogglePause,
                ),
                StandardItem {
                    label: if self.design {
                        DESIGN_LABEL.into()
                    } else {
                        "调整与设计（没有找到设计窗口）".into()
                    },
                    enabled: self.design,
                    activate: Box::new(|t: &mut Self| crate::gui::spawn_from_tray(&t.overrides)),
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
                item("退出", Command::Quit),
            ]
        }
    }

    pub struct Tray(Handle<Sni>);

    impl Tray {
        pub fn create(
            remote: Remote,
            icon: IconImage,
            overrides: Overrides,
        ) -> Result<Self, String> {
            // RGBA → 网络字节序的 ARGB。
            let argb = icon
                .0
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[r, g, b, a]| [a, r, g, b])
                .collect();
            let sni = Sni {
                remote,
                paused: false,
                icon: argb,
                overrides,
                design: crate::gui::available(),
            };
            sni.spawn()
                .map(Tray)
                .map_err(|e| format!("创建托盘图标失败（需要支持 StatusNotifierItem 的面板）: {e}"))
        }

        pub fn set_paused(&self, paused: bool) {
            self.0.update(|t| t.paused = paused);
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    use super::{DESIGN_LABEL, ICON_SIZE, IconImage};
    use crate::Overrides;
    use crate::control::{Command, Remote};

    pub struct Tray {
        _icon: TrayIcon,
        pause: MenuItem,
    }

    impl Tray {
        pub fn create(
            remote: Remote,
            icon: IconImage,
            overrides: Overrides,
        ) -> Result<Self, String> {
            let next = MenuItem::new("下一首", true, None);
            let pause = MenuItem::new("暂停", true, None);
            let arrange = MenuItem::new(DESIGN_LABEL, crate::gui::available(), None);
            let quit = MenuItem::new("退出", true, None);
            let menu = Menu::new();
            menu.append_items(&[
                &next,
                &pause,
                &arrange,
                &PredefinedMenuItem::separator(),
                &quit,
            ])
            .map_err(|e| format!("创建托盘菜单失败: {e}"))?;
            let ids = [
                (next.id().clone(), Command::Next),
                (pause.id().clone(), Command::TogglePause),
                (quit.id().clone(), Command::Quit),
            ];
            let arrange_id = arrange.id().clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if e.id == arrange_id {
                    crate::gui::spawn_from_tray(&overrides);
                } else if let Some((_, c)) = ids.iter().find(|(id, _)| *id == e.id) {
                    remote.send(*c);
                }
            }));
            let icon = Icon::from_rgba(icon.0, ICON_SIZE, ICON_SIZE)
                .map_err(|e| format!("托盘图标无效: {e}"))?;
            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("muse 诗词壁纸")
                .with_icon(icon)
                .build()
                .map_err(|e| format!("创建托盘图标失败: {e}"))?;
            Ok(Self { _icon: tray, pause })
        }

        pub fn set_paused(&self, paused: bool) {
            self.pause.set_text(if paused { "继续" } else { "暂停" });
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod imp {
    use super::IconImage;
    use crate::Overrides;
    use crate::control::Remote;

    pub struct Tray;

    impl Tray {
        pub fn create(_: Remote, _: IconImage, _: Overrides) -> Result<Self, String> {
            Err("这个平台不支持托盘".into())
        }

        pub fn set_paused(&self, _: bool) {}
    }
}

pub use imp::Tray;

/// 创建托盘；失败只打印提示，不影响壁纸运行。
pub fn create(remote: Remote, icon: IconImage, overrides: Overrides) -> Option<Tray> {
    match Tray::create(remote, icon, overrides) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("{e}");
            None
        }
    }
}
