// Windows 上不弹出控制台窗口（发布版）；调试版保留控制台方便看日志。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api;
mod app;
mod autostart;
mod control;
mod desktop;
mod gpu;
mod gui;
mod mcp;
mod power;
mod tray;
mod wallpaper;
#[cfg(target_os = "linux")]
mod wayland;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use muse_core::LoadedPack;
use muse_core::config::{self, Config};
use muse_render::{Font, ImageCache, Options};

const USAGE: &str = "\
用法:
  muse run [选项]                  作为桌面壁纸运行
  muse preview [选项]              在窗口里预览
  muse snapshot <输出.png> [选项]  离屏渲染一帧保存为 PNG
  muse check [诗词包目录]          校验诗词包
  muse arrange                     打开设计窗口：调整位置、颜色、字号和全局设置
  muse next | prev | pause | quit  控制正在运行的壁纸
  muse goto <诗词包>               切到这首诗
  muse status                      正在显示哪首诗、是否暂停
  muse reload                      手动改了配置或颜色文件后让壁纸重新读取
  muse undo                        撤销最近一次修改（设计窗口、MCP 做的修改）
  muse mcp                         作为 MCP 服务运行（stdio），让 AI 控制壁纸
  muse autostart on|off|status     开机自启
  muse config                      显示配置文件位置和当前设置

选项（覆盖配置文件）:
  --packs <目录>    诗词包目录
  --pack <名字>     从这个包（目录名）开始
  --single          只显示这一首，不轮换
  --font <字体>     字体文件路径或系统字体名，默认从系统里找中文宋体
  --wallpaper <背景> auto（当前桌面壁纸，默认）、pack（诗词包自带）或图片路径
  --text-color <颜色> 字的颜色：auto（自动，默认）或 #rrggbb
  --style <版式>    magazine（杂志风，默认）或 classic（传统）
  --scale <倍数>    字号倍数，默认 1.0
  --fps <帧率>      插电时的帧率，默认 30
  --no-tray         不显示托盘图标
  --time <秒>       snapshot 截取的时刻，默认 20
  --size <宽x高>    snapshot 的尺寸，默认 1920x1080

预览窗口里：N 下一首，空格暂停，Esc 退出。";

/// 命令行参数。没给的项用配置文件里的值。
struct Args {
    overrides: Overrides,
    time: f32,
    size: (u32, u32),
    positional: Vec<String>,
}

/// 命令行里覆盖配置的选项，优先级最高。
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    packs: Option<PathBuf>,
    pack: Option<String>,
    single: bool,
    font: Option<String>,
    wallpaper: Option<config::Wallpaper>,
    text_color: Option<config::TextColor>,
    style: Option<muse_core::LayoutStyle>,
    scale: Option<f32>,
    fps: Option<u32>,
    no_tray: bool,
}

impl Overrides {
    /// 转回命令行参数（启动设计窗口时原样传给它再启动的壁纸）。
    pub fn to_args(&self) -> Vec<String> {
        let mut v = Vec::new();
        let mut push = |k: &str, val: String| {
            v.push(k.to_string());
            v.push(val);
        };
        if let Some(p) = &self.packs {
            push("--packs", p.to_string_lossy().into_owned());
        }
        if let Some(p) = &self.pack {
            push("--pack", p.clone());
        }
        if let Some(f) = &self.font {
            push("--font", f.clone());
        }
        if let Some(w) = &self.wallpaper {
            push(
                "--wallpaper",
                match w {
                    config::Wallpaper::Auto => "auto".into(),
                    config::Wallpaper::Pack => "pack".into(),
                    config::Wallpaper::File(p) => p.to_string_lossy().into_owned(),
                },
            );
        }
        if let Some(c) = self.text_color {
            push(
                "--text-color",
                c.rgb().map_or("auto".into(), |v| format!("#{v:06x}")),
            );
        }
        if let Some(st) = self.style {
            push(
                "--style",
                match st {
                    muse_core::LayoutStyle::Magazine => "magazine".into(),
                    muse_core::LayoutStyle::Classic => "classic".into(),
                },
            );
        }
        if let Some(sc) = self.scale {
            push("--scale", sc.to_string());
        }
        if let Some(f) = self.fps {
            push("--fps", f.to_string());
        }
        if self.single {
            v.push("--single".into());
        }
        if self.no_tray {
            v.push("--no-tray".into());
        }
        v
    }
}

fn parse_args(raw: &[String]) -> Result<Args, String> {
    let mut a = Args {
        overrides: Overrides::default(),
        time: 20.0,
        size: (1920, 1080),
        positional: Vec::new(),
    };
    let mut it = raw.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} 后面缺少参数"))
        };
        match arg.as_str() {
            "--packs" => a.overrides.packs = Some(value("--packs")?.into()),
            "--pack" => a.overrides.pack = Some(value("--pack")?),
            "--single" => a.overrides.single = true,
            "--font" => a.overrides.font = Some(value("--font")?),
            "--wallpaper" => a.overrides.wallpaper = Some(value("--wallpaper")?.into()),
            "--text-color" => {
                a.overrides.text_color = Some(config::TextColor::try_from(value("--text-color")?)?)
            }
            "--style" => {
                a.overrides.style = Some(match value("--style")?.as_str() {
                    "magazine" => muse_core::LayoutStyle::Magazine,
                    "classic" => muse_core::LayoutStyle::Classic,
                    v => return Err(format!("--style 只能是 magazine 或 classic，不认识 {v}")),
                });
            }
            "--scale" => {
                a.overrides.scale = Some(
                    value("--scale")?
                        .parse()
                        .ok()
                        .filter(|s: &f32| (0.3..=3.0).contains(s))
                        .ok_or("--scale 要是 0.3 到 3 之间的数")?,
                );
            }
            "--no-tray" => a.overrides.no_tray = true,
            "--fps" => {
                a.overrides.fps = Some(
                    value("--fps")?
                        .parse()
                        .ok()
                        .filter(|f| (1..=120).contains(f))
                        .ok_or("--fps 要是 1 到 120 之间的整数")?,
                );
            }
            "--time" => {
                a.time = value("--time")?
                    .parse()
                    .ok()
                    .filter(|t: &f32| (0.0..=3600.0).contains(t))
                    .ok_or("--time 要是 0 到 3600 之间的秒数")?;
            }
            "--size" => {
                let v = value("--size")?;
                a.size = v
                    .split_once(['x', 'X'])
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .filter(|&(w, h): &(u32, u32)| w > 0 && h > 0 && w <= 8192 && h <= 8192)
                    .ok_or_else(|| format!("--size 格式应为 宽x高，现在是 {v}"))?;
            }
            s if s.starts_with("--") => return Err(format!("不认识的选项 {s}")),
            _ => a.positional.push(arg.clone()),
        }
    }
    Ok(a)
}

fn config_path() -> Option<PathBuf> {
    config::config_path()
}

/// 读出的配置和每一项的来源（config / settings / cli）。
pub struct LayeredConfig {
    pub config: Config,
    pub sources: std::collections::BTreeMap<String, &'static str>,
    pub warnings: Vec<String>,
}

/// 读 config.toml、settings.toml，再用命令行参数覆盖。
pub fn load_layered(args: &Overrides) -> Result<LayeredConfig, String> {
    let layered = match (config::config_path(), config::settings_path()) {
        (Some(c), Some(s)) => Config::load_layered(&c, &s)?,
        (Some(c), None) => config::Layered {
            config: Config::load(&c)?,
            sources: Default::default(),
            warnings: Vec::new(),
        },
        _ => config::Layered {
            config: Config::default(),
            sources: Default::default(),
            warnings: Vec::new(),
        },
    };
    let mut sources: std::collections::BTreeMap<String, &'static str> = layered
        .sources
        .iter()
        .map(|(k, s)| {
            let s = match s {
                config::Source::Default => "default",
                config::Source::Config => "config",
                config::Source::Settings => "settings",
            };
            (k.clone(), s)
        })
        .collect();
    let mut c = layered.config;
    let mut cli = |k: &str| {
        sources.insert(k.to_string(), "cli");
    };
    if args.packs.is_some() {
        cli("packs");
    }
    if args.font.is_some() {
        cli("font");
    }
    if args.wallpaper.is_some() {
        cli("wallpaper");
    }
    if args.text_color.is_some() {
        cli("text_color");
    }
    if args.style.is_some() {
        cli("style");
    }
    if args.scale.is_some() {
        cli("text_scale");
    }
    if args.pack.is_some() {
        cli("start");
    }
    if args.fps.is_some() {
        cli("fps");
    }
    if args.no_tray {
        cli("tray");
    }
    if args.single {
        cli("single");
    }
    apply_overrides(&mut c, args);
    Ok(LayeredConfig {
        config: c,
        sources,
        warnings: layered.warnings,
    })
}

/// 读配置并用命令行参数覆盖，打印 settings.toml 里被跳过的项。
fn load_config(args: &Args) -> Result<Config, String> {
    let l = load_layered(&args.overrides)?;
    for w in &l.warnings {
        eprintln!("{w}");
    }
    Ok(l.config)
}

fn apply_overrides(c: &mut Config, args: &Overrides) {
    if args.packs.is_some() {
        c.packs.clone_from(&args.packs);
    }
    if args.font.is_some() {
        c.font.clone_from(&args.font);
    }
    if let Some(w) = &args.wallpaper {
        c.wallpaper = w.clone();
    }
    if let Some(t) = args.text_color {
        c.text_color = t;
    }
    if let Some(style) = args.style {
        c.style = style;
    }
    if let Some(scale) = args.scale {
        c.text_scale = scale;
    }
    if args.pack.is_some() {
        c.start.clone_from(&args.pack);
    }
    c.single |= args.single;
    if let Some(fps) = args.fps {
        c.fps = fps;
    }
    if args.no_tray {
        c.tray = false;
    }
}

fn packs_dir(c: &Config) -> Result<PathBuf, String> {
    config::find_packs_dir(c.packs.as_deref()).ok_or_else(|| {
        "找不到诗词包目录：用 --packs 指定，或在配置文件里写 packs = \"...\"（muse config 查看位置）"
            .to_string()
    })
}

/// 一次运行需要的全部输入，各前端共用。
pub struct Session {
    pub font: Arc<Font>,
    pub packs: Arc<[LoadedPack]>,
    pub options: Options,
    pub config: Config,
    pub overrides: Overrides,
    pub images: ImageCache,
    pub remote: control::Remote,
    pub inbox: control::Inbox,
    /// 作为壁纸运行时的对外接口；预览、截图时为 None。
    pub hub: Option<Arc<control::Hub>>,
    /// 正在显示第几首；新建窗口（换显示器、显卡重建）时从这里接着显示。
    pub current: std::cell::Cell<usize>,
}

impl Session {
    /// 新建一个渲染器，停在正在显示的那首。
    pub fn renderer(
        &self,
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> muse_render::Renderer {
        let mut r = muse_render::Renderer::new(
            device,
            queue,
            format,
            self.font.clone(),
            self.packs.clone(),
            self.options.clone(),
            self.images.clone(),
        );
        if r.current_index() != self.current.get() {
            r.set_pack(self.current.get());
        }
        r
    }

    /// 托盘图标：朱红印章上一个「诗」字。
    pub fn icon(&self) -> tray::IconImage {
        tray::IconImage(self.font.seal_icon(
            '诗',
            tray::ICON_SIZE,
            [0xb0, 0x30, 0x24],
            [0xfb, 0xf3, 0xe6],
        ))
    }
}

/// 「调整位置」保存的位置，以及乘上保存的缩放后的字号倍数。
pub fn arrangement(config: &Config) -> (Option<muse_render::Placement>, f32) {
    let a = config::Arrangement::load();
    (a.placement, (config.text_scale * a.scale).clamp(0.3, 3.0))
}

/// 读取每首诗的样式（颜色文件和部件位置）。
fn load_styles(packs: &[LoadedPack]) -> muse_render::Styles {
    let names: Vec<String> = packs.iter().map(muse_render::pack_name).collect();
    muse_core::style::load_all(names.iter().map(String::as_str))
}

impl Session {
    /// 前端换了诗或暂停状态后告诉接口（订阅者会收到事件）。
    pub fn report(&self, index: usize, paused: bool) {
        self.current.set(index);
        if let Some(h) = &self.hub {
            h.set_status(index, paused);
        }
    }
}

/// 重新读取配置、位置和每首诗的样式（收到 reload 时）。字体换了时返回新字体。
pub fn reload_session(s: &mut Session) -> Option<Arc<Font>> {
    let config = match load_layered(&s.overrides) {
        Ok(l) => {
            for w in &l.warnings {
                eprintln!("{w}");
            }
            l.config
        }
        Err(e) => {
            // config.toml 写错时沿用原来的设置，只重读位置和样式。
            eprintln!("{e}，沿用原来的设置");
            s.config.clone()
        }
    };
    let mut new_font = None;
    if config.font != s.config.font {
        match Font::load(config.font.as_deref()) {
            Ok(f) => {
                eprintln!("字体: {}", f.description);
                s.font = Arc::new(f);
                new_font = Some(s.font.clone());
                if let Some(h) = &s.hub {
                    *h.ctx.font.lock().unwrap_or_else(|e| e.into_inner()) = api::font_json(&s.font);
                }
            }
            Err(e) => eprintln!("{e}，沿用原来的字体"),
        }
    }
    // 字体没换成功时记住原来的，下次 reload 还会再试新字体。
    let mut config = config;
    if new_font.is_none() && config.font != s.config.font {
        config.font = s.config.font.clone();
    }
    s.options = build_options(&config, &s.packs, &s.images, s.options.start);
    s.config = config;
    new_font
}

/// 按配置算出渲染选项：背景、位置、字号、每首诗的样式。
fn build_options(
    config: &Config,
    packs: &[LoadedPack],
    images: &ImageCache,
    start: usize,
) -> Options {
    let desktop = wallpaper::current();
    // 找到了但解不了码（比如 macOS 的 HEIC 动态壁纸）就当作没有，用诗词包自带的背景。
    let decodable = |p: PathBuf| match images.get(&p) {
        Ok(_) => Some(p),
        Err(e) => {
            eprintln!("{e}，用诗词包自带的背景");
            None
        }
    };
    let wallpaper = match &config.wallpaper {
        config::Wallpaper::Pack => None,
        config::Wallpaper::File(p) if p.is_file() => decodable(p.clone()),
        config::Wallpaper::File(p) => {
            eprintln!("找不到壁纸图片 {}，用诗词包自带的背景", p.display());
            None
        }
        config::Wallpaper::Auto => match &desktop {
            Some(p) => {
                eprintln!("背景: 当前桌面壁纸 {}", p.display());
                decodable(p.clone())
            }
            None => {
                eprintln!("背景: 没找到当前桌面壁纸，用诗词包自带的背景");
                None
            }
        },
    };
    let (placement, text_scale) = arrangement(config);
    Options {
        start,
        single: config.single,
        timing: config.timing(),
        style: config.style,
        text_scale,
        wallpaper,
        desktop,
        placement,
        text_color: config.text_color.rgb(),
        styles: Arc::new(load_styles(packs)),
    }
}

/// 加载字体和诗词包，确定起始的那首。
fn prepare(config: Config, overrides: Overrides) -> Result<Session, String> {
    // 命令行明确写错了才报错退出；settings.toml 里的值（可能是没挂载的盘）只提示，用诗词包背景。
    if let Some(config::Wallpaper::File(p)) = &overrides.wallpaper
        && !p.is_file()
    {
        return Err(format!("找不到壁纸图片 {}", p.display()));
    }
    let dir = packs_dir(&config)?;
    let (packs, errors) = muse_core::load_all(&dir).map_err(|e| e.to_string())?;
    for e in &errors {
        eprintln!("跳过 {e}");
    }
    if packs.is_empty() {
        return Err(format!("{} 下没有可用的诗词包", dir.display()));
    }
    let start = match &config.start {
        Some(name) => match packs
            .iter()
            .position(|p| p.dir.file_name().is_some_and(|n| n == name.as_str()))
        {
            Some(i) => i,
            None if overrides.pack.is_some() => return Err(format!("找不到诗词包 {name}")),
            None => {
                eprintln!("找不到诗词包 {name}，从第一首开始");
                0
            }
        },
        None => 0,
    };
    let images = ImageCache::new();
    let options = build_options(&config, &packs, &images, start);
    let font = Font::load(config.font.as_deref())?;
    eprintln!("字体: {}", font.description);
    let (remote, inbox) = control::channel_pair();
    Ok(Session {
        font: Arc::new(font),
        packs: packs.into(),
        options,
        config,
        overrides,
        images,
        remote,
        inbox,
        hub: None,
        current: std::cell::Cell::new(start),
    })
}

fn main() -> ExitCode {
    // 发布版没有自己的控制台；从 cmd、PowerShell 里运行时挂到父进程的控制台上，命令输出才看得到。
    #[cfg(all(windows, not(debug_assertions)))]
    // 安全性：没有参数需要校验，失败（比如从资源管理器双击启动）时什么也不做。
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = raw.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    if matches!(cmd.as_str(), "-h" | "--help" | "help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let args = match parse_args(rest) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let result = match cmd.as_str() {
        "check" => return check(&args),
        "preview" => load_config(&args)
            .and_then(|c| prepare(c, args.overrides.clone()))
            .and_then(|s| app::run(app::Mode::Preview, s)),
        "arrange" => gui::launch(&args.overrides),
        "snapshot" => snapshot(&args),
        "run" => run_wallpaper(&args),
        "mcp" => mcp::run(),
        "next" | "prev" | "pause" | "quit" | "reload" => {
            control::call(cmd, serde_json::Value::Null).map(|_| ())
        }
        "goto" => match args.positional.first() {
            Some(p) => control::call("goto", serde_json::json!({ "pack": p })).map(|_| ()),
            None => Err("goto 后面要写诗词包的目录名".into()),
        },
        "status" => control::call("status", serde_json::Value::Null).map(|v| {
            println!(
                "{}《{}》{} · 第 {}/{} 首{}",
                v["author"].as_str().unwrap_or(""),
                v["title"].as_str().unwrap_or(""),
                v["pack"].as_str().unwrap_or(""),
                v["index"].as_u64().unwrap_or(0) + 1,
                v["total"].as_u64().unwrap_or(0),
                if v["paused"].as_bool() == Some(true) {
                    "（已暂停）"
                } else {
                    ""
                }
            );
        }),
        "undo" => control::call("undo", serde_json::Value::Null).map(|v| {
            println!("已撤销：{}", v["undone"].as_str().unwrap_or(""));
        }),
        "autostart" => autostart(&args),
        "config" => show_config(&args),
        _ => Err(format!("不认识的命令 {cmd}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_wallpaper(args: &Args) -> Result<(), String> {
    let mut session = prepare(load_config(args)?, args.overrides.clone())?;
    let hub = control::Hub::new(
        session.remote.clone(),
        api::Context {
            overrides: args.overrides.clone(),
            packs: session.packs.clone(),
            font: std::sync::Mutex::new(api::font_json(&session.font)),
        },
    );
    hub.set_status(session.options.start, false);
    control::listen(hub.clone())?;
    session.hub = Some(hub);
    let result = run_frontend(session);
    control::cleanup();
    result
}

/// Linux 上有 Wayland 就用 layer-shell，否则走 X11；其他平台用桌面层窗口。
fn run_frontend(session: Session) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) {
        return wayland::run(session);
    }
    app::run(app::Mode::Desktop, session)
}

fn autostart(args: &Args) -> Result<(), String> {
    let msg = match args.positional.first().map(String::as_str) {
        Some("on") => {
            let c = load_config(args)?;
            autostart::enable(&packs_dir(&c)?)?
        }
        Some("off") => autostart::disable()?,
        Some("status") | None => autostart::status()?,
        Some(other) => return Err(format!("autostart 只接受 on、off、status，不认识 {other}")),
    };
    println!("{msg}");
    Ok(())
}

fn show_config(args: &Args) -> Result<(), String> {
    match config_path() {
        Some(p) => println!(
            "配置文件: {}{}",
            p.display(),
            if p.is_file() {
                ""
            } else {
                "（不存在，使用默认值）"
            }
        ),
        None => println!("取不到配置目录，使用默认值"),
    }
    if let Some(p) = config::settings_path() {
        println!(
            "设计窗口的设置: {}{}",
            p.display(),
            if p.is_file() {
                "（覆盖 config.toml 的同名项）"
            } else {
                "（还没有）"
            }
        );
    }
    let c = load_config(args)?;
    let packs = packs_dir(&c).map_or_else(|e| e, |p| p.display().to_string());
    println!("诗词包目录: {packs}");
    println!("字体: {}", c.font.as_ref().map_or("自动", String::as_str));
    println!(
        "字色: {}",
        c.text_color
            .rgb()
            .map_or("自动".to_string(), |v| format!("#{v:06x}"))
    );
    println!(
        "背景: {}",
        match &c.wallpaper {
            config::Wallpaper::Pack => "诗词包自带".to_string(),
            config::Wallpaper::File(p) => p.display().to_string(),
            config::Wallpaper::Auto => match wallpaper::current() {
                Some(p) => format!("当前桌面壁纸（{}）", p.display()),
                None => "当前桌面壁纸（没找到，用诗词包自带的背景）".to_string(),
            },
        }
    );
    println!(
        "版式: {}，字号倍数: {}",
        match c.style {
            muse_core::LayoutStyle::Magazine => "magazine（杂志风）",
            muse_core::LayoutStyle::Classic => "classic（传统）",
        },
        c.text_scale
    );
    println!("帧率: 插电 {}，电池 {}", c.fps, c.battery_fps);
    println!(
        "全屏时暂停: {}",
        if c.pause_on_fullscreen { "是" } else { "否" }
    );
    println!("每首停留: {} 秒，逐字间隔: {} 秒", c.hold, c.char_interval);
    println!("托盘: {}", if c.tray { "显示" } else { "不显示" });
    let a = config::Arrangement::load();
    let where_ =
        config::Arrangement::path().map_or(String::new(), |p| format!("（{}）", p.display()));
    match a.placement {
        Some(p) => println!(
            "位置: 中心在屏幕 {:.0}%, {:.0}%，字号再乘 {:.2}{where_}",
            p.x * 100.0,
            p.y * 100.0,
            a.scale
        ),
        None => println!("位置: 按诗词包设置，字号再乘 {:.2}{where_}", a.scale),
    }
    Ok(())
}

fn snapshot(args: &Args) -> Result<(), String> {
    let out = args
        .positional
        .first()
        .ok_or("snapshot 需要输出文件名")?
        .clone();
    let mut session = prepare(load_config(args)?, args.overrides.clone())?;
    session.options.single = true;
    let (_, device, queue) = gpu::init(None)?;
    let mut r = muse_render::Renderer::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        session.font,
        session.packs,
        session.options,
        session.images,
    );
    let (w, h) = args.size;
    r.resize(w, h);
    // 逐帧推进，让粒子处于自然的分布状态。
    let step = 1.0 / 30.0;
    let mut t = 0.0;
    while t + step <= args.time {
        r.advance(step);
        t += step;
    }
    let pixels = r.capture(w, h);
    let img = image::RgbaImage::from_raw(w, h, pixels).ok_or("截图尺寸不对")?;
    img.save(&out)
        .map_err(|e| format!("{out}: 保存失败: {e}"))?;
    println!("已保存 {out}");
    Ok(())
}

/// 校验目录下所有诗词包，供编写自定义包时使用。
fn check(args: &Args) -> ExitCode {
    let root = match args.positional.first() {
        Some(p) => PathBuf::from(p),
        None => match load_config(args).and_then(|c| packs_dir(&c)) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        },
    };
    let (packs, errors) = match muse_core::load_all(&root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    for p in &packs {
        let poem = &p.pack.poem;
        println!("ok    {}（{}·{}）", poem.title, poem.dynasty, poem.author);
    }
    for e in &errors {
        eprintln!("error {e}");
    }
    println!("{} 个通过，{} 个失败", packs.len(), errors.len());
    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
