//! 用户设置：`config.toml`，命令行参数会覆盖这里的值。
//!
//! 位置：Linux `$XDG_CONFIG_HOME/muse/`（默认 `~/.config/muse/`），
//! macOS `~/Library/Application Support/muse/`，Windows `%APPDATA%\muse\`。

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::LayoutStyle;
use crate::scene::{Placement, Timing};

pub const CONFIG_FILE: &str = "config.toml";

/// 字的颜色。配置里写 `"auto"` 或 `"#rrggbb"`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum TextColor {
    /// 按背景自动选。
    #[default]
    Auto,
    /// 固定颜色，sRGB 0xRRGGBB。
    Rgb(u32),
}

impl TextColor {
    /// 固定颜色时返回 0xRRGGBB。
    pub fn rgb(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::Rgb(c) => Some(c),
        }
    }
}

impl TryFrom<String> for TextColor {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let t = s.trim();
        if t == "auto" {
            return Ok(Self::Auto);
        }
        if let Ok(v) = crate::style::parse_hex(t) {
            return Ok(Self::Rgb(v));
        }
        Err(format!(
            "text_color 要是 auto 或 #rrggbb 格式的颜色，现在是 {s}"
        ))
    }
}

/// 背景用哪张图。配置里写 `"auto"`、`"pack"` 或图片路径。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(from = "String")]
pub enum Wallpaper {
    /// 当前桌面壁纸，找不到时用诗词包自带的背景。
    #[default]
    Auto,
    /// 诗词包自带的背景。
    Pack,
    /// 指定的图片。
    File(PathBuf),
}

impl From<String> for Wallpaper {
    fn from(s: String) -> Self {
        match s.as_str() {
            "auto" => Self::Auto,
            "pack" => Self::Pack,
            _ => Self::File(expand_home(Path::new(&s))),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// 诗词包目录。不填时依次找当前目录的 `packs`、程序旁边的 `packs`、配置目录下的 `packs`。
    pub packs: Option<PathBuf>,
    /// 字体：字体文件路径，或系统里的字体名（如 "LXGW WenKai"）。不填自动找中文宋体。
    pub font: Option<String>,
    /// 诗画在什么背景上：当前桌面壁纸（默认）、诗词包自带的背景，或指定的图片。
    pub wallpaper: Wallpaper,
    /// 字的颜色：auto 按背景自动配色，或 "#rrggbb"。
    pub text_color: TextColor,
    /// 版式：magazine（杂志风）或 classic（传统）。诗词包里写了 text.style 时以包为准。
    pub style: LayoutStyle,
    /// 整体字号倍数，1.0 为默认大小。
    pub text_scale: f32,
    /// 从哪个包（目录名）开始。
    pub start: Option<String>,
    /// 只显示一首，不轮换。
    pub single: bool,
    /// 插电时的帧率。
    pub fps: u32,
    /// 用电池时的帧率，0 表示用电池时暂停动画。
    pub battery_fps: u32,
    /// 全屏应用在前台时暂停（Windows、macOS 有效；Wayland 由合成器决定是否回调）。
    pub pause_on_fullscreen: bool,
    /// 每首诗全部显示后停留的秒数。
    pub hold: f32,
    /// 逐字出现的间隔秒数。
    pub char_interval: f32,
    /// 显示托盘图标。
    pub tray: bool,
}

impl Default for Config {
    fn default() -> Self {
        let t = Timing::default();
        Self {
            packs: None,
            font: None,
            wallpaper: Wallpaper::Auto,
            text_color: TextColor::Auto,
            style: LayoutStyle::default(),
            text_scale: 1.0,
            start: None,
            single: false,
            fps: 30,
            battery_fps: 15,
            pause_on_fullscreen: true,
            hold: t.hold,
            char_interval: t.char_interval,
            tray: true,
        }
    }
}

/// 程序写的全局设置文件（状态目录），覆盖 `config.toml` 里的同名项。
pub const SETTINGS_FILE: &str = "settings.toml";

/// 配置里的每一项来自哪里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Default,
    /// 用户的 `config.toml`。
    Config,
    /// 设计窗口、MCP 写的 `settings.toml`。
    Settings,
}

/// 读取后的配置，以及每一项的来源。
#[derive(Debug, Clone)]
pub struct Layered {
    pub config: Config,
    /// 写在 config.toml 或 settings.toml 里的键及其来源；没列出的是默认值。
    pub sources: std::collections::BTreeMap<String, Source>,
    /// settings.toml 里写错、被跳过的项。
    pub warnings: Vec<String>,
}

/// 读一个 TOML 文件为表；文件不存在时为空表。
fn read_table(path: &Path) -> Result<toml::Table, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => t
            .parse::<toml::Table>()
            .map_err(|e| format!("{}: 格式错误: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(e) => Err(format!("{}: 读取失败: {e}", path.display())),
    }
}

/// 把表里的相对路径按 `base` 解析成绝对路径，`~` 换成主目录。
fn resolve_paths(table: &mut toml::Table, base: &Path) {
    for key in ["packs", "wallpaper"] {
        if let Some(toml::Value::String(s)) = table.get_mut(key) {
            if key == "wallpaper" && matches!(s.as_str(), "auto" | "pack") {
                continue;
            }
            let mut p = expand_home(Path::new(s.as_str()));
            if p.is_relative() {
                p = base.join(p);
            }
            *s = p.to_string_lossy().into_owned();
        }
    }
    // font 可能是字体名，只有按这个目录解析后确实存在的文件才当作相对路径。
    if let Some(toml::Value::String(f)) = table.get_mut("font") {
        let rel = base.join(expand_home(Path::new(f.as_str())));
        if Path::new(f.as_str()).is_relative() && rel.is_file() {
            *f = rel.to_string_lossy().into_owned();
        }
    }
}

impl Config {
    /// 读取配置文件；文件不存在时返回默认值。
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut table = read_table(path)?;
        resolve_paths(&mut table, path.parent().unwrap_or(Path::new(".")));
        Self::from_table(table).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn from_table(table: toml::Table) -> Result<Self, String> {
        let mut c: Self = toml::Value::Table(table)
            .try_into()
            .map_err(|e: toml::de::Error| format!("格式错误: {}", e.message()))?;
        // font = "auto" 表示自动选字体（settings.toml 用它覆盖 config.toml 里写的字体）。
        if c.font.as_deref() == Some("auto") {
            c.font = None;
        }
        c.validate()?;
        Ok(c)
    }

    /// 读 `config.toml`，再用 `settings.toml` 覆盖。config.toml 写错时报错；
    /// settings.toml 是程序写的，写错的项跳过并记下提示。
    pub fn load_layered(config: &Path, settings: &Path) -> Result<Layered, String> {
        let mut table = read_table(config)?;
        resolve_paths(&mut table, config.parent().unwrap_or(Path::new(".")));
        let mut cfg =
            Self::from_table(table.clone()).map_err(|e| format!("{}: {e}", config.display()))?;
        let mut sources: std::collections::BTreeMap<String, Source> =
            table.keys().map(|k| (k.clone(), Source::Config)).collect();
        let mut warnings = Vec::new();
        let mut extra = match read_table(settings) {
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("{e}，忽略整个文件"));
                toml::Table::new()
            }
        };
        resolve_paths(&mut extra, settings.parent().unwrap_or(Path::new(".")));
        for (k, v) in extra {
            let mut candidate = table.clone();
            candidate.insert(k.clone(), v);
            match Self::from_table(candidate.clone()) {
                Ok(c) => {
                    cfg = c;
                    table = candidate;
                    sources.insert(k, Source::Settings);
                }
                Err(e) => warnings.push(format!("{}: {k}: {e}，忽略这一项", settings.display())),
            }
        }
        Ok(Layered {
            config: cfg,
            sources,
            warnings,
        })
    }

    fn validate(&self) -> Result<(), String> {
        if !(1..=120).contains(&self.fps) {
            return Err(format!("fps 必须在 1 到 120 之间，现在是 {}", self.fps));
        }
        if self.battery_fps > 120 {
            return Err(format!(
                "battery_fps 不能超过 120，现在是 {}",
                self.battery_fps
            ));
        }
        if !(0.3..=3.0).contains(&self.text_scale) {
            return Err(format!(
                "text_scale 必须在 0.3 到 3 之间，现在是 {}",
                self.text_scale
            ));
        }
        if !(0.0..=3600.0).contains(&self.hold) {
            return Err(format!(
                "hold 必须在 0 到 3600 秒之间，现在是 {}",
                self.hold
            ));
        }
        if !(0.02..=10.0).contains(&self.char_interval) {
            return Err(format!(
                "char_interval 必须在 0.02 到 10 秒之间，现在是 {}",
                self.char_interval
            ));
        }
        Ok(())
    }

    pub fn timing(&self) -> Timing {
        Timing {
            hold: self.hold,
            char_interval: self.char_interval,
            ..Timing::default()
        }
    }
}

/// `config.toml` 的位置。
pub fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join(CONFIG_FILE))
}

/// `settings.toml` 的位置。
pub fn settings_path() -> Option<PathBuf> {
    state_dir().map(|d| d.join(SETTINGS_FILE))
}

/// 改 `settings.toml`：值为 None 的项删掉（回到 config.toml 或默认值）。
/// 先和 config.toml 合起来校验，有错整体不写。保留文件里原有的注释和格式。
pub fn update_settings(
    config: &Path,
    settings: &Path,
    changes: &std::collections::BTreeMap<String, Option<toml::Value>>,
) -> Result<(), String> {
    let old = match std::fs::read_to_string(settings) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: 读取失败: {e}", settings.display())),
    };
    let mut doc: toml_edit::DocumentMut = old.parse().map_err(|e| {
        format!(
            "{}: 格式错误，先手动修好或删掉再改设置: {e}",
            settings.display()
        )
    })?;
    for (k, v) in changes {
        match v {
            None => {
                doc.remove(k);
            }
            Some(v) => {
                let item: toml_edit::Value = v
                    .to_string()
                    .parse()
                    .map_err(|e| format!("{k}: 值无效: {e}"))?;
                doc[k.as_str()] = toml_edit::Item::Value(item);
            }
        }
    }
    let text = doc.to_string();
    // 合起来校验：改动的项必须都能生效。
    let mut table = read_table(config)?;
    resolve_paths(&mut table, config.parent().unwrap_or(Path::new(".")));
    let mut extra: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: 格式错误: {e}", settings.display()))?;
    resolve_paths(&mut extra, settings.parent().unwrap_or(Path::new(".")));
    for (k, v) in &extra {
        let mut candidate = table.clone();
        candidate.insert(k.clone(), v.clone());
        match Config::from_table(candidate.clone()) {
            Ok(_) => table = candidate,
            Err(e) if changes.contains_key(k) => return Err(format!("{k}: {e}")),
            Err(_) => {}
        }
    }
    if text.trim().is_empty() {
        return crate::style::remove_file(settings);
    }
    crate::style::write_file(settings, &text)
}

/// 把开头的 `~` 换成主目录。
pub fn expand_home(p: &Path) -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|h| !h.is_empty());
    match (p.strip_prefix("~"), home) {
        (Ok(rest), Some(h)) => PathBuf::from(h).join(rest),
        _ => p.to_path_buf(),
    }
}

/// 配置目录，取不到（没有 HOME 之类）时返回 None。
pub fn config_dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(target_os = "windows") {
        env("APPDATA")?
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support")
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?
    };
    Some(base.join("muse"))
}

/// 程序自己记的状态放在哪：Linux `$XDG_STATE_HOME/muse/`（默认 `~/.local/state/muse/`），
/// 其他平台和配置目录相同。
pub fn state_dir() -> Option<PathBuf> {
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        return config_dir();
    }
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|h| PathBuf::from(h).join(".local/state"))
        })
        .map(|d| d.join("muse"))
}

/// 在「调整位置」里拖出来的位置和缩放，所有诗共用。单独存一个文件，不改用户的 config.toml。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrangement {
    /// None 时按诗词包的 `text.position`。
    pub placement: Option<Placement>,
    /// 在配置的 `text_scale` 上再乘的倍数。
    pub scale: f32,
}

impl Default for Arrangement {
    fn default() -> Self {
        Self {
            placement: None,
            scale: 1.0,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArrangementFile {
    x: Option<f32>,
    y: Option<f32>,
    #[serde(default = "one")]
    scale: f32,
}

fn one() -> f32 {
    1.0
}

impl Arrangement {
    pub const FILE: &str = "arrangement.toml";

    pub fn path() -> Option<PathBuf> {
        state_dir().map(|d| d.join(Self::FILE))
    }

    /// 读取保存的位置；没有文件时返回默认值，文件坏了打印提示后也用默认值。
    pub fn load() -> Self {
        Self::path().map_or_else(Self::default, |p| Self::load_from(&p))
    }

    fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match toml::from_str::<ArrangementFile>(&text) {
            Ok(f) => {
                let ok = |v: f32| v.is_finite().then(|| v.clamp(0.0, 1.0));
                Self {
                    placement: match (f.x.and_then(ok), f.y.and_then(ok)) {
                        (Some(x), Some(y)) => Some(Placement { x, y }),
                        _ => None,
                    },
                    // 和整体字号相乘后再夹到 0.3–3，这里只挡住离谱的值（和接口一致）。
                    scale: if f.scale.is_finite() {
                        f.scale.clamp(0.1, 10.0)
                    } else {
                        1.0
                    },
                }
            }
            Err(e) => {
                eprintln!("{}: 格式错误，忽略: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        let path = Self::path().ok_or("取不到保存位置的目录")?;
        self.save_to(&path)?;
        Ok(path)
    }

    fn save_to(&self, path: &Path) -> Result<(), String> {
        let mut text =
            String::from("# muse 调整位置时保存的，删掉这个文件就恢复诗词包的默认位置。\n");
        if let Some(p) = self.placement {
            text += &format!("x = {:.4}\ny = {:.4}\n", p.x, p.y);
        }
        text += &format!("scale = {:.3}\n", self.scale);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("{}: 创建目录失败: {e}", dir.display()))?;
        }
        std::fs::write(path, text).map_err(|e| format!("{}: 保存失败: {e}", path.display()))
    }
}

/// 找诗词包目录：配置指定的优先，否则按约定位置找第一个存在的。
pub fn find_packs_dir(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = configured {
        return Some(p.to_path_buf());
    }
    let mut candidates = vec![PathBuf::from("packs")];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        candidates.push(dir.join("packs"));
        // macOS .app 包里放在 Contents/Resources。
        candidates.push(dir.join("../Resources/packs"));
    }
    if let Some(dir) = config_dir() {
        candidates.push(dir.join("packs"));
    }
    candidates.into_iter().find(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("muse-config-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(CONFIG_FILE);
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn missing_file_gives_defaults() {
        let c = Config::load(Path::new("/nonexistent/muse/config.toml")).unwrap();
        assert_eq!(c.fps, 30);
        assert!(c.tray);
        assert_eq!(c.wallpaper, Wallpaper::Auto);
    }

    #[test]
    fn relative_paths_resolve_against_config_dir() {
        let p = write(
            "rel",
            "packs = 'my-packs'\nwallpaper = 'bg.jpg'\nfps = 24\nfont = 'LXGW WenKai'\nstyle = 'classic'\n",
        );
        let c = Config::load(&p).unwrap();
        assert_eq!(c.packs.unwrap(), p.parent().unwrap().join("my-packs"));
        assert_eq!(
            c.wallpaper,
            Wallpaper::File(p.parent().unwrap().join("bg.jpg"))
        );
        assert_eq!(c.fps, 24);
        // 不存在的相对路径当作字体名，原样保留。
        assert_eq!(c.font.as_deref(), Some("LXGW WenKai"));
        assert_eq!(c.style, LayoutStyle::Classic);
    }

    #[test]
    fn arrangement_round_trip() {
        let p = write("arr", "").with_file_name(Arrangement::FILE);
        let _ = std::fs::remove_file(&p);
        assert_eq!(Arrangement::load_from(&p), Arrangement::default());
        let a = Arrangement {
            placement: Some(Placement { x: 0.25, y: 0.6 }),
            scale: 1.2,
        };
        a.save_to(&p).unwrap();
        assert_eq!(Arrangement::load_from(&p), a);
        std::fs::write(&p, "x = 2\ny = 0.5\nscale = 9\n").unwrap();
        let b = Arrangement::load_from(&p);
        assert_eq!(b.placement, Some(Placement { x: 1.0, y: 0.5 }));
        assert_eq!(b.scale, 9.0);
    }

    #[test]
    fn settings_override_config_and_bad_items_are_skipped() {
        let cfg = write("layer", "fps = 24\nstyle = 'classic'\n");
        let set = cfg.with_file_name(SETTINGS_FILE);
        std::fs::write(&set, "fps = 60\ntext_scale = 9\nwallpaper = 'pack'\n").unwrap();
        let l = Config::load_layered(&cfg, &set).unwrap();
        assert_eq!(l.config.fps, 60);
        assert_eq!(l.config.style, LayoutStyle::Classic);
        assert_eq!(l.config.text_scale, 1.0);
        assert_eq!(l.config.wallpaper, Wallpaper::Pack);
        assert_eq!(l.sources["fps"], Source::Settings);
        assert_eq!(l.sources["style"], Source::Config);
        assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);

        // settings.toml 写 font = "auto" 能盖过 config.toml 里写的字体。
        let cfg2 = write("fontauto", "font = 'X'\n");
        let set2 = cfg2.with_file_name(SETTINGS_FILE);
        std::fs::write(&set2, "font = 'auto'\n").unwrap();
        assert_eq!(
            Config::load_layered(&cfg2, &set2).unwrap().config.font,
            None
        );

        let mut changes = std::collections::BTreeMap::new();
        changes.insert("text_scale".to_string(), None);
        changes.insert("battery_fps".to_string(), Some(toml::Value::Integer(0)));
        update_settings(&cfg, &set, &changes).unwrap();
        let text = std::fs::read_to_string(&set).unwrap();
        assert!(
            text.contains("battery_fps = 0") && !text.contains("text_scale"),
            "{text}"
        );
        changes.clear();
        changes.insert("fps".to_string(), Some(toml::Value::Integer(500)));
        assert!(update_settings(&cfg, &set, &changes).is_err());
        changes.clear();
        changes.insert("bogus".to_string(), Some(toml::Value::Integer(1)));
        assert!(update_settings(&cfg, &set, &changes).is_err());
        assert_eq!(std::fs::read_to_string(&set).unwrap(), text);
    }

    #[test]
    fn rejects_bad_values_and_unknown_keys() {
        let c = Config::load(&write("pack", "wallpaper = 'pack'\n")).unwrap();
        assert_eq!(c.wallpaper, Wallpaper::Pack);
        let c = Config::load(&write("color", "text_color = '#C8a96e'\n")).unwrap();
        assert_eq!(c.text_color, TextColor::Rgb(0xc8a96e));
        assert_eq!(
            TextColor::try_from("#fa0".to_string()),
            Ok(TextColor::Rgb(0xffaa00))
        );
        let e = Config::load(&write("badcolor", "text_color = 'red'\n")).unwrap_err();
        assert!(e.contains("text_color"), "{e}");
        let e = Config::load(&write("fps", "fps = 0\n")).unwrap_err();
        assert!(e.contains("fps"), "{e}");
        let e = Config::load(&write("unk", "fsp = 30\n")).unwrap_err();
        assert!(e.contains("fsp"), "{e}");
    }
}
