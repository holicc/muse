//! 设计窗口（Electron 应用）的启动。
//!
//! 安装包里设计窗口和 muse 放在一起：muse 在 `resources/bin/` 下，设计窗口的可执行文件在安装目录的根上
//! （macOS 在 `Contents/MacOS/`）。开发时从仓库的 `gui/` 目录用 electron 运行。
//! 也可以用环境变量 `MUSE_GUI` 指定设计窗口的可执行文件。

use std::path::PathBuf;
use std::process::Command;

use crate::Overrides;

/// 安装包里设计窗口可执行文件的名字。
#[cfg(target_os = "windows")]
const NAMES: &[&str] = &["muse-studio.exe"];
#[cfg(target_os = "macos")]
const NAMES: &[&str] = &["Muse", "muse-studio"];
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const NAMES: &[&str] = &["muse-studio"];

enum Target {
    /// 打包好的可执行文件。
    Exe(PathBuf),
    /// 开发时仓库里的 gui 目录，用 electron 运行。
    Dev(PathBuf),
}

fn find() -> Option<Target> {
    if let Some(p) = std::env::var_os("MUSE_GUI").filter(|v| !v.is_empty()) {
        return Some(Target::Exe(PathBuf::from(p)));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let roots = [
        dir.to_path_buf(),
        // resources/bin/muse → 安装目录
        dir.join("../.."),
        // macOS：Contents/Resources/bin/muse → Contents/MacOS
        dir.join("../../MacOS"),
    ];
    for root in &roots {
        for name in NAMES {
            let p = root.join(name);
            if p.is_file() {
                return Some(Target::Exe(p));
            }
        }
    }
    // 开发：target/debug/muse 往上找仓库里的 gui/。
    dir.ancestors()
        .map(|d| d.join("gui"))
        .find(|g| g.join("package.json").is_file() && g.join("dist").is_dir())
        .map(Target::Dev)
}

/// 打开设计窗口（不等它退出）。
pub fn launch(overrides: &Overrides) -> Result<(), String> {
    let target = find().ok_or(
        "找不到设计窗口：安装包里应和 muse 放在一起；开发时先在 gui/ 里运行 npm run build。\
         也可以用环境变量 MUSE_GUI 指定它的路径",
    )?;
    let mut cmd = match &target {
        Target::Exe(p) => Command::new(p),
        Target::Dev(dir) => {
            let mut c = Command::new("electron");
            c.arg(dir);
            c
        }
    };
    if let Ok(exe) = std::env::current_exe() {
        cmd.env("MUSE_BIN", exe);
    }
    cmd.env("MUSE_SOCKET", crate::control::address());
    // 设计窗口要启动壁纸时沿用同样的命令行选项。
    cmd.env(
        "MUSE_ARGS",
        serde_json::to_string(&overrides.to_args()).unwrap_or_default(),
    );
    cmd.spawn()
        .map(|mut child| {
            // 等它退出，免得在 Unix 上留下僵尸进程。
            let _ = std::thread::Builder::new()
                .name("muse-gui-wait".into())
                .spawn(move || child.wait());
        })
        .map_err(|e| match target {
            Target::Exe(p) => format!("{}: 启动设计窗口失败: {e}", p.display()),
            Target::Dev(_) => format!("启动 electron 失败（开发时要把 electron 放进 PATH）: {e}"),
        })
}

/// 托盘菜单里打开设计窗口，失败只打印提示。
pub fn spawn_from_tray(overrides: &Overrides) {
    if let Err(e) = launch(overrides) {
        eprintln!("{e}");
    }
}

/// 设计窗口找得到吗（托盘里据此决定这一项能不能点）。
pub fn available() -> bool {
    find().is_some()
}
