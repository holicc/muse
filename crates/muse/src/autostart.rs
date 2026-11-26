//! 开机自启：`muse autostart on|off|status`。
//!
//! - Linux：`~/.config/autostart/muse.desktop`（XDG 自启动，KDE、GNOME、systemd 会话都认）
//! - macOS：`~/Library/LaunchAgents/io.github.muse.plist`
//! - Windows：注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//!
//! 记录的是当前程序的绝对路径和诗词包目录，移动程序后要重新执行 `muse autostart on`。

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

/// 开启自启，`packs` 是要写进启动命令的诗词包目录。返回给用户看的说明。
pub fn enable(packs: &Path) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("取不到程序路径: {e}"))?;
    let packs = std::fs::canonicalize(packs)
        .map_err(|e| format!("{}: 诗词包目录无效: {e}", packs.display()))?;
    imp::enable(&exe, &packs)
}

pub fn disable() -> Result<String, String> {
    imp::disable()
}

pub fn status() -> Result<String, String> {
    imp::status()
}

#[cfg(unix)]
fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "没有 HOME 环境变量".to_string())
}

#[cfg(unix)]
fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("{}: 创建目录失败: {e}", dir.display()))?;
    }
    std::fs::write(path, content).map_err(|e| format!("{}: 写入失败: {e}", path.display()))
}

#[cfg(unix)]
fn remove_file(path: &Path) -> Result<String, String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(format!("已关闭开机自启（删除了 {}）", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("本来就没有开机自启".into()),
        Err(e) => Err(format!("{}: 删除失败: {e}", path.display())),
    }
}

#[cfg(unix)]
fn file_status(path: &Path) -> Result<String, String> {
    Ok(if path.is_file() {
        format!("已开启：{}", path.display())
    } else {
        "未开启".into()
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::path::{Path, PathBuf};

    use super::{file_status, home, remove_file, write_file};

    fn entry() -> Result<PathBuf, String> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .map_or_else(|| home().map(|h| h.join(".config")), Ok)?;
        Ok(base.join("autostart/muse.desktop"))
    }

    /// desktop entry 的 Exec 字段：含空格、引号等的参数要加双引号并转义。
    fn quote(s: &str) -> String {
        if s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
        {
            return s.to_string();
        }
        let mut q = String::from("\"");
        for c in s.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                q.push('\\');
            }
            q.push(c);
        }
        q.push('"');
        q
    }

    pub fn enable(exe: &Path, packs: &Path) -> Result<String, String> {
        let path = entry()?;
        let exec = format!(
            "{} run --packs {}",
            quote(&exe.to_string_lossy()),
            quote(&packs.to_string_lossy())
        );
        write_file(
            &path,
            &format!(
                "[Desktop Entry]\nType=Application\nName=muse\nComment=古诗词动态壁纸\n\
                 Exec={exec}\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n"
            ),
        )?;
        let mut msg = format!("已开启开机自启：{}", path.display());
        // niri、Sway 等不一定处理 XDG 自启动，提示用户在合成器配置里加。
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        if ["niri", "sway", "Hyprland", "river"]
            .iter()
            .any(|d| desktop.contains(d))
        {
            msg.push_str(&format!(
                "\n{desktop} 不一定会执行 XDG 自启动项；如果开机后没有启动，在合成器配置里加上启动命令：\n  {exec}"
            ));
        }
        Ok(msg)
    }

    pub fn disable() -> Result<String, String> {
        remove_file(&entry()?)
    }

    pub fn status() -> Result<String, String> {
        file_status(&entry()?)
    }

    #[cfg(test)]
    mod tests {
        use super::quote;

        #[test]
        fn quotes_desktop_exec_arguments() {
            assert_eq!(quote("/usr/bin/muse"), "/usr/bin/muse");
            assert_eq!(quote("/home/a b/packs"), "\"/home/a b/packs\"");
            assert_eq!(quote("/x/$y\"z"), "\"/x/\\$y\\\"z\"");
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};

    use super::{file_status, home, remove_file, write_file};

    const LABEL: &str = "io.github.muse";

    fn plist() -> Result<PathBuf, String> {
        Ok(home()?.join(format!("Library/LaunchAgents/{LABEL}.plist")))
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    pub fn enable(exe: &Path, packs: &Path) -> Result<String, String> {
        let path = plist()?;
        let args = [
            exe.to_string_lossy().into_owned(),
            "run".into(),
            "--packs".into(),
            packs.to_string_lossy().into_owned(),
        ];
        let args: String = args
            .iter()
            .map(|a| format!("        <string>{}</string>\n", xml_escape(a)))
            .collect();
        write_file(
            &path,
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
{args}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
            ),
        )?;
        Ok(format!("已开启开机自启：{}", path.display()))
    }

    pub fn disable() -> Result<String, String> {
        remove_file(&plist()?)
    }

    pub fn status() -> Result<String, String> {
        file_status(&plist()?)
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::path::Path;
    use std::process::Command;

    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

    fn reg(args: &[&str]) -> Result<std::process::Output, String> {
        Command::new("reg")
            .args(args)
            .output()
            .map_err(|e| format!("执行 reg 失败: {e}"))
    }

    pub fn enable(exe: &Path, packs: &Path) -> Result<String, String> {
        let value = format!("\"{}\" run --packs \"{}\"", exe.display(), packs.display());
        let out = reg(&["add", KEY, "/v", "muse", "/t", "REG_SZ", "/d", &value, "/f"])?;
        if out.status.success() {
            Ok("已开启开机自启".into())
        } else {
            Err(format!(
                "写注册表失败: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    pub fn disable() -> Result<String, String> {
        let out = reg(&["delete", KEY, "/v", "muse", "/f"])?;
        Ok(if out.status.success() {
            "已关闭开机自启".into()
        } else {
            "本来就没有开机自启".into()
        })
    }

    pub fn status() -> Result<String, String> {
        let out = reg(&["query", KEY, "/v", "muse"])?;
        Ok(if out.status.success() {
            "已开启".into()
        } else {
            "未开启".into()
        })
    }
}
