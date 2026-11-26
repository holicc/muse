//! 每首诗单独的样式：颜色、背景、部件的位置 / 字号 / 透明度。
//!
//! - 颜色和背景：配置目录 `poems/<诗词包目录名>.toml`。用户可以手写，设计窗口、MCP 也会改它；
//!   程序写入时保留原有的注释和格式。
//! - 部件的位置、字号、透明度：状态目录 `poems/<诗词包目录名>.toml`，由程序维护。
//!
//! 颜色文件示例：
//!
//! ```toml
//! color = "#e8ebf5"                  # 整首（标题、正文、署名），也可以写渐变
//! title = "#c0473a"
//! "line.1" = ["#ffffff", "#7a88b8"]  # 渐变：顺着阅读方向从第一个颜色过渡到最后一个
//! "line.2.3" = "#ffd700cc"           # 第 2 列第 3 个字；8 位写法带透明度
//! seal = "#b3382c"                   # 印章底色
//! wallpaper = "pack"                 # 这首诗的背景：auto / pack / 图片路径；不写跟随全局
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{Wallpaper, config_dir, expand_home, state_dir};
use crate::layout::Part;

/// 一个颜色：sRGB 0xRRGGBB 加不透明度（0–255）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub rgb: u32,
    pub a: u8,
}

impl Color {
    pub const fn opaque(rgb: u32) -> Self {
        Self { rgb, a: 255 }
    }

    /// 不透明度 0..1。
    pub fn alpha(self) -> f32 {
        f32::from(self.a) / 255.0
    }

    /// `#rrggbb`，带透明度时 `#rrggbbaa`。
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:06x}", self.rgb)
        } else {
            format!("#{:06x}{:02x}", self.rgb, self.a)
        }
    }
}

/// 解析 `#rgb`、`#rrggbb` 或 `#rrggbbaa`。
pub fn parse_color(s: &str) -> Result<Color, String> {
    let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
    let full: String = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect(),
        _ => hex.to_string(),
    };
    let bad = || format!("颜色要写成 #rrggbb 或 #rrggbbaa，现在是 {s}");
    if !full.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    match full.len() {
        6 => Ok(Color::opaque(
            u32::from_str_radix(&full, 16).map_err(|_| bad())?,
        )),
        8 => {
            let v = u32::from_str_radix(&full, 16).map_err(|_| bad())?;
            Ok(Color {
                rgb: v >> 8,
                a: (v & 0xff) as u8,
            })
        }
        _ => Err(bad()),
    }
}

/// 解析不带透明度的 `#rrggbb` 或 `#rgb`（全局字色用）。
pub fn parse_hex(s: &str) -> Result<u32, String> {
    match parse_color(s) {
        Ok(c) if c.a == 255 && s.trim().trim_start_matches('#').len() != 8 => Ok(c.rgb),
        _ => Err(format!("颜色要写成 #rrggbb，现在是 {s}")),
    }
}

/// 一种颜色或一段渐变。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Paint {
    Solid(Color),
    /// 至少两个颜色，均匀分布。
    Gradient(Vec<Color>),
}

impl Paint {
    /// 在 `t`（0..1）处的颜色。
    pub fn at(&self, t: f32) -> Color {
        match self {
            Self::Solid(c) => *c,
            Self::Gradient(stops) => {
                let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
                let i = (t.floor() as usize).min(stops.len() - 2);
                lerp(stops[i], stops[i + 1], t - i as f32)
            }
        }
    }

    /// 所有色标（纯色时一个）。
    pub fn stops(&self) -> &[Color] {
        match self {
            Self::Solid(c) => std::slice::from_ref(c),
            Self::Gradient(v) => v,
        }
    }

    fn from_stops(stops: Vec<Color>) -> Result<Self, String> {
        match stops.len() {
            0 => Err("渐变至少要一个颜色".into()),
            1 => Ok(Self::Solid(stops[0])),
            _ => Ok(Self::Gradient(stops)),
        }
    }

    fn parse(v: &toml::Value) -> Result<Self, String> {
        match v {
            toml::Value::String(s) => parse_color(s).map(Self::Solid),
            toml::Value::Array(a) => a
                .iter()
                .map(|v| v.as_str().ok_or("渐变里要写颜色字符串".to_string()))
                .map(|s| s.and_then(parse_color))
                .collect::<Result<Vec<_>, _>>()
                .and_then(Self::from_stops),
            _ => Err("要写 \"#rrggbb\" 或 [\"#rrggbb\", \"#rrggbb\"]".into()),
        }
    }

    fn to_doc(&self) -> PaintDoc {
        match self {
            Self::Solid(c) => PaintDoc::One(c.to_hex()),
            Self::Gradient(v) => PaintDoc::Many(v.iter().map(|c| c.to_hex()).collect()),
        }
    }

    fn from_doc(d: &PaintDoc) -> Result<Self, String> {
        match d {
            PaintDoc::One(s) => parse_color(s).map(Self::Solid),
            PaintDoc::Many(v) => v
                .iter()
                .map(|s| parse_color(s))
                .collect::<Result<Vec<_>, _>>()
                .and_then(Self::from_stops),
        }
    }
}

fn lerp(a: Color, b: Color, t: f32) -> Color {
    let ch = |c: u32, s: u32| ((c >> s) & 0xff) as f32;
    let rgb = [16, 8, 0].iter().fold(0, |acc, &s| {
        let v = ch(a.rgb, s) + (ch(b.rgb, s) - ch(a.rgb, s)) * t;
        acc | ((v.round().clamp(0.0, 255.0) as u32) << s)
    });
    let alpha = f32::from(a.a) + (f32::from(b.a) - f32::from(a.a)) * t;
    Color {
        rgb,
        a: alpha.round().clamp(0.0, 255.0) as u8,
    }
}

/// 部件字号倍数的范围。
pub const PART_SIZE_RANGE: (f32, f32) = (0.2, 5.0);

/// 一首诗的样式。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoemStyle {
    /// 部件相对默认排版的位移（屏幕高度为 1）。
    pub offsets: HashMap<Part, (f32, f32)>,
    /// 部件字号相对整首的倍数，1 为跟随整首。
    pub sizes: HashMap<Part, f32>,
    /// 部件的不透明度 0..1，1 为默认。
    pub opacity: HashMap<Part, f32>,
    /// 整首（标题、正文、署名）的颜色。
    pub color: Option<Paint>,
    /// 部件的颜色。
    pub parts: HashMap<Part, Paint>,
    /// 单个字的颜色：(部件, 部件里第几个字，从 0 开始)。
    pub chars: HashMap<(Part, usize), Color>,
    /// 这首诗的背景；None 跟随全局设置。
    pub wallpaper: Option<Wallpaper>,
}

/// 所有诗的样式，按诗词包目录名索引。
pub type Styles = HashMap<String, PoemStyle>;

/// 对外接口（设计窗口、MCP）里一首诗的样式，键名和文件里一致。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StyleDoc {
    /// `color`、`title`、`line.1`、`line.2.3` 等 → 颜色或渐变。
    #[serde(default)]
    pub colors: BTreeMap<String, PaintDoc>,
    /// `auto`、`pack` 或图片路径；不写跟随全局。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
    #[serde(default)]
    pub offsets: BTreeMap<String, [f64; 2]>,
    #[serde(default)]
    pub sizes: BTreeMap<String, f64>,
    #[serde(default)]
    pub opacity: BTreeMap<String, f64>,
}

/// 一个颜色或一组渐变色标。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PaintDoc {
    One(String),
    Many(Vec<String>),
}

/// 颜色文件里的键：整首、部件或单个字。
enum ColorKey {
    Poem,
    Part(Part),
    Char(Part, usize),
}

fn parse_color_key(k: &str) -> Option<ColorKey> {
    if k == "color" {
        return Some(ColorKey::Poem);
    }
    if let Some(p) = Part::parse(k) {
        return Some(ColorKey::Part(p));
    }
    // 单个字：部件名后面加 .第几个字，如 line.2.3、title.1。
    let (p, i) = k.rsplit_once('.')?;
    let i: usize = i.parse().ok()?;
    Some(ColorKey::Char(Part::parse(p)?, i.checked_sub(1)?))
}

const KEY_HINT: &str = "不认识的名字；可以写 color、wallpaper、title、line.N、byline、seal、rule、masthead、issue、source、watermark，单个字在后面加 .N";

fn wallpaper_to_string(w: &Wallpaper) -> String {
    match w {
        Wallpaper::Auto => "auto".into(),
        Wallpaper::Pack => "pack".into(),
        Wallpaper::File(p) => p.to_string_lossy().into_owned(),
    }
}

/// 颜色文件里的背景：相对路径按配置目录解析。
fn parse_wallpaper(s: &str) -> Wallpaper {
    match Wallpaper::from(s.to_string()) {
        Wallpaper::File(p) if p.is_relative() => {
            Wallpaper::File(config_dir().map_or(p.clone(), |d| d.join(&p)))
        }
        w => w,
    }
}

impl PoemStyle {
    pub fn colors_path(name: &str) -> Option<PathBuf> {
        config_dir().map(|d| d.join("poems").join(format!("{name}.toml")))
    }

    /// 部件位置、字号、透明度的文件。
    pub fn layout_path(name: &str) -> Option<PathBuf> {
        state_dir().map(|d| d.join("poems").join(format!("{name}.toml")))
    }

    /// 读取这首诗的样式；文件写错的地方打印提示后跳过。
    pub fn load(name: &str) -> Self {
        let mut s = Self::default();
        if let Some(p) = Self::colors_path(name) {
            s.load_colors(&p);
        }
        if let Some(p) = Self::layout_path(name) {
            s.load_layout(&p);
        }
        s
    }

    fn load_colors(&mut self, path: &Path) {
        let Some(table) = read_table(path) else {
            return;
        };
        for e in self.apply_color_table(&table) {
            eprintln!("{}: {e}", path.display());
        }
    }

    /// 把颜色表里的项读进来，返回写错的项（已跳过）。
    fn apply_color_table(&mut self, table: &toml::Table) -> Vec<String> {
        let mut errors = Vec::new();
        for (k, v) in table {
            if k == "wallpaper" {
                match v.as_str() {
                    Some(s) if !s.trim().is_empty() => self.wallpaper = Some(parse_wallpaper(s)),
                    _ => errors.push(format!("{k}: 要写 \"auto\"、\"pack\" 或图片路径")),
                }
                continue;
            }
            let Some(key) = parse_color_key(k) else {
                errors.push(format!("{k}: {KEY_HINT}"));
                continue;
            };
            let paint = match Paint::parse(v) {
                Ok(p) => p,
                Err(e) => {
                    errors.push(format!("{k}: {e}"));
                    continue;
                }
            };
            match key {
                ColorKey::Poem => self.color = Some(paint),
                ColorKey::Part(p) => {
                    self.parts.insert(p, paint);
                }
                ColorKey::Char(p, i) => match paint {
                    Paint::Solid(c) => {
                        self.chars.insert((p, i), c);
                    }
                    Paint::Gradient(_) => errors.push(format!("{k}: 单个字只能写一个颜色")),
                },
            }
        }
        errors
    }

    fn load_layout(&mut self, path: &Path) {
        let Some(table) = read_table(path) else {
            return;
        };
        let warn = |k: &str| eprintln!("{}: {k}: 格式不对，忽略", path.display());
        let section = |name: &str| table.get(name).and_then(|v| v.as_table());
        for (k, v) in section("offsets").into_iter().flatten() {
            let xy = v.as_array().and_then(|a| match a.as_slice() {
                [x, y] => Some((num(x)?, num(y)?)),
                _ => None,
            });
            match (Part::parse(k), xy) {
                (Some(p), Some((x, y))) if x.is_finite() && y.is_finite() => {
                    self.offsets.insert(p, clamp_offset(x, y));
                }
                _ => warn(&format!("offsets.{k}")),
            }
        }
        for (name, map, range) in [
            ("sizes", &mut self.sizes, PART_SIZE_RANGE),
            ("opacity", &mut self.opacity, (0.0, 1.0)),
        ] {
            for (k, v) in section(name).into_iter().flatten() {
                match (Part::parse(k), num(v)) {
                    (Some(p), Some(x)) if x.is_finite() => {
                        map.insert(p, x.clamp(range.0, range.1));
                    }
                    _ => warn(&format!("{name}.{k}")),
                }
            }
        }
    }

    /// 转成对外接口的格式。
    pub fn to_doc(&self) -> StyleDoc {
        let mut colors = BTreeMap::new();
        if let Some(c) = &self.color {
            colors.insert("color".to_string(), c.to_doc());
        }
        for (p, paint) in &self.parts {
            colors.insert(p.key(), paint.to_doc());
        }
        for ((p, i), c) in &self.chars {
            colors.insert(format!("{}.{}", p.key(), i + 1), PaintDoc::One(c.to_hex()));
        }
        // 文件里只记到小数点后 4 位，转成 f64 时去掉 f32 的尾数噪声。
        let r = |v: f32, d: f64| (f64::from(v) * d).round() / d;
        let keyed = |m: &HashMap<Part, f32>| m.iter().map(|(p, v)| (p.key(), r(*v, 1e3))).collect();
        StyleDoc {
            colors,
            wallpaper: self.wallpaper.as_ref().map(wallpaper_to_string),
            offsets: self
                .offsets
                .iter()
                .map(|(p, (x, y))| (p.key(), [r(*x, 1e4), r(*y, 1e4)]))
                .collect(),
            sizes: keyed(&self.sizes),
            opacity: keyed(&self.opacity),
        }
    }

    /// 从对外接口的格式读入；有写错的项时整体拒绝，返回所有错误。
    pub fn from_doc(doc: &StyleDoc) -> Result<Self, String> {
        let mut s = Self::default();
        let mut errors = Vec::new();
        for (k, v) in &doc.colors {
            let paint = match Paint::from_doc(v) {
                Ok(p) => p,
                Err(e) => {
                    errors.push(format!("{k}: {e}"));
                    continue;
                }
            };
            match parse_color_key(k) {
                Some(ColorKey::Poem) => s.color = Some(paint),
                Some(ColorKey::Part(p)) => {
                    s.parts.insert(p, paint);
                }
                Some(ColorKey::Char(p, i)) => match paint {
                    Paint::Solid(c) => {
                        s.chars.insert((p, i), c);
                    }
                    Paint::Gradient(_) => errors.push(format!("{k}: 单个字只能写一个颜色")),
                },
                None => errors.push(format!("{k}: {KEY_HINT}")),
            }
        }
        match doc.wallpaper.as_deref().map(str::trim) {
            Some("") => errors.push("wallpaper: 不能为空".into()),
            Some(w) => s.wallpaper = Some(Wallpaper::from(w.to_string())),
            None => {}
        }
        let part = |k: &str, errors: &mut Vec<String>| {
            let p = Part::parse(k);
            if p.is_none() {
                errors.push(format!("{k}: 不认识的部件名"));
            }
            p
        };
        for (k, [x, y]) in &doc.offsets {
            if let Some(p) = part(k, &mut errors) {
                if x.is_finite() && y.is_finite() {
                    s.offsets.insert(p, clamp_offset(*x as f32, *y as f32));
                } else {
                    errors.push(format!("offsets.{k}: 不是有效的数"));
                }
            }
        }
        for (name, src, dst, range) in [
            ("sizes", &doc.sizes, &mut s.sizes, PART_SIZE_RANGE),
            ("opacity", &doc.opacity, &mut s.opacity, (0.0, 1.0)),
        ] {
            for (k, v) in src {
                if let Some(p) = part(k, &mut errors) {
                    // 按 f32 比较：0.2（f64）转成 f32 正好等于下限，不会被误判超出范围。
                    let x = *v as f32;
                    if (range.0..=range.1).contains(&x) {
                        dst.insert(p, x);
                    } else {
                        errors.push(format!(
                            "{name}.{k}: 要在 {} 到 {} 之间，现在是 {v}",
                            range.0, range.1
                        ));
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(s)
        } else {
            Err(errors.join("；"))
        }
    }

    /// 写回颜色文件：只改动变了的项，保留其他内容、注释和格式；没有任何内容时删掉文件。
    pub fn save_colors(&self, name: &str) -> Result<(), String> {
        let path = Self::colors_path(name).ok_or("取不到配置目录")?;
        let old = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("{}: 读取失败: {e}", path.display())),
        };
        let text = self.render_colors(&old)?;
        if text.trim().is_empty() {
            return remove_file(&path);
        }
        write_file(&path, &text)
    }

    /// 在原有颜色文件文本上更新颜色和背景，返回新文本。
    fn render_colors(&self, old: &str) -> Result<String, String> {
        let mut doc: toml_edit::DocumentMut = old
            .parse()
            .map_err(|e| format!("颜色文件格式错误，先手动修好再保存: {e}"))?;
        let want = self.to_doc();
        let mut want_items: BTreeMap<String, toml_edit::Item> = want
            .colors
            .iter()
            .map(|(k, v)| (k.clone(), paint_item(v)))
            .collect();
        if let Some(w) = &want.wallpaper {
            want_items.insert("wallpaper".into(), toml_edit::value(w.as_str()));
        }
        // 认得的键：不再需要的删掉，值没变的不动（保留原来的写法和注释）。
        let existing: Vec<String> = doc.iter().map(|(k, _)| k.to_string()).collect();
        for k in existing {
            let known = k == "wallpaper" || parse_color_key(&k).is_some();
            if !known {
                continue;
            }
            match want_items.remove(&k) {
                // 原来的值写错了（加载时已跳过），保留原样让用户自己改，不悄悄删掉。
                None if !old_value_valid(&k, doc.get(&k)) => {}
                None => {
                    doc.remove(&k);
                }
                Some(item) => {
                    let (a, b) = (doc.get(&k).and_then(item_to_value), item_to_value(&item));
                    // 颜色按解析后的值比较，#FFF 和 #ffffff 算同一个，原来的写法不动。
                    let same = match (&a, &b) {
                        (Some(toml::Value::String(a)), Some(toml::Value::String(b)))
                            if k == "wallpaper" =>
                        {
                            parse_wallpaper(a) == parse_wallpaper(b)
                        }
                        (Some(a), Some(b)) if k != "wallpaper" => Paint::parse(a)
                            .ok()
                            .is_some_and(|x| Paint::parse(b) == Ok(x)),
                        _ => a == b,
                    };
                    if !same {
                        doc[&k] = item;
                    }
                }
            }
        }
        for (k, item) in want_items {
            doc[&k] = item;
        }
        Ok(doc.to_string())
    }

    /// 写部件位置、字号、透明度；全是默认值时删掉文件。
    pub fn save_layout(&self, name: &str) -> Result<(), String> {
        let path = Self::layout_path(name).ok_or("取不到状态目录")?;
        match self.render_layout() {
            None => remove_file(&path),
            Some(text) => write_file(&path, &text),
        }
    }

    fn render_layout(&self) -> Option<String> {
        if self.offsets.is_empty() && self.sizes.is_empty() && self.opacity.is_empty() {
            return None;
        }
        let mut text = String::from(
            "# muse 设计窗口保存的部件位置（屏幕高度为 1）、字号倍数、不透明度。删掉这个文件就恢复默认。\n",
        );
        let mut offsets: Vec<_> = self.offsets.iter().collect();
        offsets.sort_by_key(|(p, _)| **p);
        if !offsets.is_empty() {
            text += "\n[offsets]\n";
            for (p, (x, y)) in offsets {
                text += &format!("\"{}\" = [{x:.4}, {y:.4}]\n", p.key());
            }
        }
        for (name, map) in [("sizes", &self.sizes), ("opacity", &self.opacity)] {
            let mut v: Vec<_> = map.iter().collect();
            v.sort_by_key(|(p, _)| **p);
            if !v.is_empty() {
                text += &format!("\n[{name}]\n");
                for (p, x) in v {
                    text += &format!("\"{}\" = {x:.3}\n", p.key());
                }
            }
        }
        Some(text)
    }
}

/// 颜色文件里原有的一项能不能正常读出来。
fn old_value_valid(key: &str, item: Option<&toml_edit::Item>) -> bool {
    let Some(v) = item.and_then(item_to_value) else {
        return false;
    };
    if key == "wallpaper" {
        return v.as_str().is_some_and(|s| !s.trim().is_empty());
    }
    match (parse_color_key(key), Paint::parse(&v)) {
        (Some(ColorKey::Char(..)), Ok(Paint::Gradient(_))) => false,
        (Some(_), Ok(_)) => true,
        _ => false,
    }
}

fn paint_item(p: &PaintDoc) -> toml_edit::Item {
    match p {
        PaintDoc::One(s) => toml_edit::value(s.as_str()),
        PaintDoc::Many(v) => {
            let arr: toml_edit::Array = v.iter().map(String::as_str).collect();
            toml_edit::value(arr)
        }
    }
}

/// 比较两个值时忽略写法差异（引号、空格、注释）。
fn item_to_value(i: &toml_edit::Item) -> Option<toml::Value> {
    let v = i.as_value()?;
    let text = format!("x = {}", v.clone().decorated("", ""));
    text.parse::<toml::Table>().ok()?.remove("x")
}

/// 位移限制在一屏以内。
fn clamp_offset(x: f32, y: f32) -> (f32, f32) {
    (x.clamp(-4.0, 4.0), y.clamp(-1.0, 1.0))
}

fn num(v: &toml::Value) -> Option<f32> {
    v.as_float()
        .map(|f| f as f32)
        .or_else(|| v.as_integer().map(|i| i as f32))
}

fn read_table(path: &Path) -> Option<toml::Table> {
    let text = std::fs::read_to_string(path).ok()?;
    match text.parse::<toml::Table>() {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("{}: 格式错误，忽略: {e}", path.display());
            None
        }
    }
}

/// 写文件：先写临时文件再改名，中途出错不会留下半个文件。
pub fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("{}: 创建目录失败: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: 保存失败: {e}", path.display())
        })
}

pub fn remove_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: 删除失败: {e}", path.display()))
        }
        _ => Ok(()),
    }
}

/// 读取这些诗的样式。
pub fn load_all<'a>(names: impl IntoIterator<Item = &'a str>) -> Styles {
    names
        .into_iter()
        .map(|n| (n.to_string(), PoemStyle::load(n)))
        .collect()
}

/// 背景路径里的 `~`。
pub fn expand_wallpaper(w: Wallpaper) -> Wallpaper {
    match w {
        Wallpaper::File(p) => Wallpaper::File(expand_home(&p)),
        w => w,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, text: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("muse-style-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    fn solid(rgb: u32) -> Paint {
        Paint::Solid(Color::opaque(rgb))
    }

    #[test]
    fn parses_colors_gradients_and_alpha() {
        let p = tmp(
            "colors.toml",
            r##"
            color = "#fff"
            title = "#c0473a"
            "line.1" = ["#000000", "#ffffff00"]
            "line.2.3" = "#ffd70080"
            wallpaper = "pack"
            nonsense = "#000"
            "##,
        );
        let mut s = PoemStyle::default();
        s.load_colors(&p);
        assert_eq!(s.color, Some(solid(0xffffff)));
        assert_eq!(s.parts[&Part::Title], solid(0xc0473a));
        let g = &s.parts[&Part::Line(0)];
        assert_eq!(g.at(0.0), Color::opaque(0));
        assert_eq!(
            g.at(1.0),
            Color {
                rgb: 0xffffff,
                a: 0
            }
        );
        assert_eq!(
            g.at(0.5),
            Color {
                rgb: 0x808080,
                a: 128
            }
        );
        assert_eq!(
            s.chars[&(Part::Line(1), 2)],
            Color {
                rgb: 0xffd700,
                a: 0x80
            }
        );
        assert_eq!(s.wallpaper, Some(Wallpaper::Pack));
        assert_eq!(s.chars.len(), 1);
    }

    #[test]
    fn color_parsing() {
        assert_eq!(parse_color("#FA0"), Ok(Color::opaque(0xffaa00)));
        assert_eq!(parse_color("#11223344").unwrap().to_hex(), "#11223344");
        assert_eq!(parse_color("#112233").unwrap().to_hex(), "#112233");
        assert!(parse_color("#12345").is_err());
        assert!(parse_color("#gggggg").is_err());
        assert_eq!(parse_hex("#c8a96e"), Ok(0xc8a96e));
        assert!(parse_hex("#c8a96e80").is_err());
    }

    #[test]
    fn three_stop_gradient() {
        let g = Paint::Gradient(vec![
            Color::opaque(0xff0000),
            Color::opaque(0x00ff00),
            Color::opaque(0x0000ff),
        ]);
        assert_eq!(g.at(0.5), Color::opaque(0x00ff00));
        assert_eq!(g.at(1.0), Color::opaque(0x0000ff));
    }

    #[test]
    fn layout_file_round_trip() {
        let s = PoemStyle {
            offsets: [(Part::Line(1), (0.1, -0.05)), (Part::Title, (1.0, 0.0))].into(),
            sizes: [(Part::Seal, 1.5)].into(),
            opacity: [(Part::Watermark, 0.5)].into(),
            ..Default::default()
        };
        let p = tmp("layout.toml", &s.render_layout().unwrap());
        let mut back = PoemStyle::default();
        back.load_layout(&p);
        assert_eq!(back, s);
        assert_eq!(PoemStyle::default().render_layout(), None);

        let p = tmp(
            "bad-layout.toml",
            "[offsets]\nbad = [0]\n\"line.1\" = [0, 9]\n[sizes]\ntitle = 99\n",
        );
        let mut s = PoemStyle::default();
        s.load_layout(&p);
        assert_eq!(s.offsets[&Part::Line(0)], (0.0, 1.0));
        assert_eq!(s.sizes[&Part::Title], PART_SIZE_RANGE.1);
        assert_eq!(s.offsets.len(), 1);
    }

    #[test]
    fn doc_round_trip_and_validation() {
        let s = PoemStyle {
            offsets: [(Part::Byline, (0.02, 0.0))].into(),
            sizes: [(Part::Title, 1.2)].into(),
            opacity: [(Part::Seal, 0.8)].into(),
            color: Some(solid(0x112233)),
            parts: [(
                Part::Line(2),
                Paint::Gradient(vec![
                    Color::opaque(0),
                    Color {
                        rgb: 0xffffff,
                        a: 0x40,
                    },
                ]),
            )]
            .into(),
            chars: [((Part::Title, 0), Color::opaque(0xff0000))].into(),
            wallpaper: Some(Wallpaper::Pack),
        };
        let doc = s.to_doc();
        assert_eq!(doc.colors["title.1"], PaintDoc::One("#ff0000".into()));
        assert_eq!(PoemStyle::from_doc(&doc).unwrap(), s);

        let mut bad = StyleDoc::default();
        bad.colors
            .insert("nope".into(), PaintDoc::One("#fff".into()));
        bad.colors.insert(
            "title.2".into(),
            PaintDoc::Many(vec!["#000".into(), "#fff".into()]),
        );
        bad.sizes.insert("title".into(), 50.0);
        // 下限本身是合法的。
        let mut edge = StyleDoc::default();
        edge.sizes.insert("seal".into(), 0.2);
        assert!(PoemStyle::from_doc(&edge).is_ok());
        let e = PoemStyle::from_doc(&bad).unwrap_err();
        assert!(
            e.contains("nope") && e.contains("title.2") && e.contains("sizes.title"),
            "{e}"
        );
    }

    #[test]
    fn writing_colors_keeps_comments_and_unknown_lines() {
        let old = "# 我的配色\ncolor = \"#FFF\"  # 白\ntitle = \"#c0473a\"\nmystery = 1\n";
        let mut s = PoemStyle::default();
        s.apply_color_table(&old.parse().unwrap());
        // 改标题、删整首色（这里先确认整首色写法不变时不动）、加一列渐变。
        s.parts.insert(Part::Title, solid(0x000000));
        s.parts.insert(
            Part::Line(0),
            Paint::Gradient(vec![Color::opaque(1), Color::opaque(2)]),
        );
        let text = s.render_colors(old).unwrap();
        assert!(text.contains("# 我的配色"), "{text}");
        assert!(text.contains("color = \"#FFF\"  # 白"), "{text}");
        assert!(text.contains("title = \"#000000\""), "{text}");
        assert!(text.contains("mystery = 1"), "{text}");
        assert!(
            text.contains("\"line.1\" = [\"#000001\", \"#000002\"]"),
            "{text}"
        );

        s.color = None;
        let text = s.render_colors(&text).unwrap();
        assert!(!text.contains("#FFF"), "{text}");

        // 写错的项（加载时跳过了）保存时原样保留；背景按解析后的路径比较，写法不变。
        let old = "byline = \"#zzz\"\n\"title.1\" = [\"#000\", \"#fff\"]\nwallpaper = \"pack\"\n";
        let mut s = PoemStyle::default();
        s.apply_color_table(&old.parse().unwrap());
        assert_eq!(s.render_colors(old).unwrap(), old);
        assert!(
            PoemStyle::default()
                .render_colors("")
                .unwrap()
                .trim()
                .is_empty()
        );
    }

    #[test]
    fn part_keys_round_trip() {
        for p in [
            Part::Title,
            Part::Line(0),
            Part::Line(11),
            Part::Seal,
            Part::Source,
        ] {
            assert_eq!(Part::parse(&p.key()), Some(p));
        }
        assert_eq!(Part::parse("line.0"), None);
    }
}
