//! 统一接口的方法：命令行、托盘、设计窗口、MCP 共用。传输层见 [`crate::control`]。
//!
//! | 方法 | 参数 | 结果 |
//! |---|---|---|
//! | `status` | | 当前状态 |
//! | `next` / `prev` | | |
//! | `goto` | `{pack: 名字或序号}` | |
//! | `pause` | `{paused?: bool}`，不给时切换 | |
//! | `quit` / `reload` | | |
//! | `state` | | 设计窗口需要的全部数据 |
//! | `poems` | | 所有诗 |
//! | `style.get` | `{pack}` | 这首诗的样式 |
//! | `style.set` | `{pack, style}` | 整个替换 |
//! | `style.patch` | `{pack, colors?, wallpaper?, offsets?, sizes?, opacity?}`，值为 null 表示删掉 | 新样式 |
//! | `settings.get` | | settings.toml 的内容、生效的配置、每项来源 |
//! | `settings.set` | `{values: {键: 值或 null}}` | |
//! | `arrangement.get` / `arrangement.set` | `{x?, y?, scale?, reset?}` | 整首位置和字号倍数 |
//! | `save` | `{arrangement?, settings?, styles?: {名字: 样式}}` | 一次保存多项，撤销时一起撤销 |
//! | `undo` | | 撤销最近一次修改 |
//! | `history` | | 最近的修改 |
//! | `subscribe` | | 之后这个连接会收到事件 |
//! | `fonts` | | 系统里能显示中文的字体 |
//! | `font.locate` | `{font?}` | 字体对应的文件（设计窗口预览换字体用） |
//!
//! 每次写入前把原文件备份到状态目录的 `backups/`，并记进撤销历史；写完通知壁纸重新读取。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use muse_core::config::{self, Arrangement, Config, Wallpaper};
use muse_core::style::{PaintDoc, PoemStyle, StyleDoc};
use muse_core::{LayoutStyle, LoadedPack, Orientation, ParticlePreset, TextPosition};
use muse_render::Placement;
use serde_json::{Map, Value, json};

use crate::control::{Command, Hub};

/// 接口线程需要的、运行期间不变或由前端更新的信息。
pub struct Context {
    pub overrides: crate::Overrides,
    pub packs: Arc<[LoadedPack]>,
    /// 当前字体的文件（设计窗口加载同样的字体），重新读取配置后由前端更新。
    pub font: Mutex<Value>,
}

/// 一次修改：改之前各文件的内容（None 表示原来没有这个文件）。
struct Change {
    description: String,
    time: u64,
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
}

/// 修改历史，用来撤销。
#[derive(Default)]
pub struct Store {
    history: Vec<Change>,
}

const MAX_HISTORY: usize = 200;
const MAX_BACKUPS: usize = 100;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn snapshot(paths: &[PathBuf]) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    paths
        .iter()
        .map(|p| (p.clone(), std::fs::read(p).ok()))
        .collect()
}

fn restore(files: &[(PathBuf, Option<Vec<u8>>)]) -> Result<(), String> {
    let mut errors = Vec::new();
    for (p, data) in files {
        let r = match data {
            Some(d) => p
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|_| std::fs::write(p, d)),
            None => match std::fs::remove_file(p) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
        };
        if let Err(e) = r {
            errors.push(format!("{}: {e}", p.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("恢复文件失败: {}", errors.join("；")))
    }
}

/// 把改之前的文件复制到状态目录的 backups/ 下，只保留最近的若干份。
fn backup(files: &[(PathBuf, Option<Vec<u8>>)]) {
    let Some(root) = config::state_dir().map(|d| d.join("backups")) else {
        return;
    };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    // 同一毫秒里的两次修改分开放。
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = root.join(format!("{ms}-{seq:04}"));
    for (i, (p, data)) in files.iter().enumerate() {
        let Some(data) = data else { continue };
        let name = p
            .file_name()
            .map_or("file".into(), |n| n.to_string_lossy().into_owned());
        let parent = p
            .parent()
            .and_then(|d| d.file_name())
            .map_or(String::new(), |n| format!("{}-", n.to_string_lossy()));
        let _ = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join(format!("{i}-{parent}{name}")), data));
    }
    if let Ok(entries) = std::fs::read_dir(&root) {
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        dirs.sort();
        let excess = dirs.len().saturating_sub(MAX_BACKUPS);
        for d in &dirs[..excess] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// 改动哪些东西，用于通知订阅者。
#[derive(Default)]
struct Touched {
    styles: Vec<String>,
    settings: bool,
    arrangement: bool,
}

/// 在一次事务里写文件：先备份，出错时恢复原样；成功后记进撤销历史、通知壁纸重新读取。
fn transaction(
    hub: &Hub,
    description: String,
    paths: Vec<PathBuf>,
    touched: Touched,
    write: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let mut store = hub.store.lock().unwrap_or_else(|e| e.into_inner());
    let before = snapshot(&paths);
    if let Err(e) = write() {
        restore(&before)?;
        return Err(e);
    }
    if snapshot(&paths) == before {
        return Ok(());
    }
    backup(&before);
    store.history.push(Change {
        description,
        time: now_secs(),
        files: before,
    });
    if store.history.len() > MAX_HISTORY {
        store.history.remove(0);
    }
    drop(store);
    notify(hub, &touched, "change");
    Ok(())
}

fn notify(hub: &Hub, t: &Touched, cause: &str) {
    hub.remote.send(Command::Reload);
    hub.broadcast(
        "changed",
        json!({
            "styles": t.styles,
            "settings": t.settings,
            "arrangement": t.arrangement,
            "cause": cause,
        }),
    );
}

// ───────────── 参数 ─────────────

fn param<'a>(params: &'a Value, key: &str) -> Option<&'a Value> {
    params.get(key).filter(|v| !v.is_null())
}

fn str_param<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    param(params, key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("缺少参数 {key}"))
}

fn find_pack(hub: &Hub, name: &str) -> Result<usize, String> {
    hub.ctx
        .packs
        .iter()
        .position(|p| muse_render::pack_name(p) == name)
        .ok_or_else(|| format!("没有叫 {name} 的诗词包；用 poems 查看所有诗"))
}

fn pack_param(hub: &Hub, params: &Value) -> Result<String, String> {
    let name = str_param(params, "pack")?;
    find_pack(hub, name)?;
    Ok(name.to_string())
}

// ───────────── 输出 ─────────────

pub fn status_json(hub: &Hub) -> Value {
    let s = hub.status();
    let packs = &hub.ctx.packs;
    let p = &packs[s.index.min(packs.len() - 1)];
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "paused": s.paused,
        "index": s.index,
        "total": packs.len(),
        "pack": muse_render::pack_name(p),
        "title": p.pack.poem.title,
        "author": p.pack.poem.author,
    })
}

fn path_str(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

fn poem_json(p: &LoadedPack) -> Value {
    let poem = &p.pack.poem;
    json!({
        "name": muse_render::pack_name(p),
        "dir": path_str(&p.dir),
        "title": poem.title,
        "author": poem.author,
        "dynasty": poem.dynasty,
        "lines": poem.lines,
        "source": poem.source,
        "style": p.pack.text.style.map(style_str),
        "orientation": match p.pack.text.orientation {
            Orientation::Vertical => "vertical",
            Orientation::Horizontal => "horizontal",
        },
        "position": match p.pack.text.position {
            TextPosition::Left => "left",
            TextPosition::Center => "center",
            TextPosition::Right => "right",
        },
        "particles": p.pack.particles.as_ref().map(|x| match x.preset {
            ParticlePreset::Petals => "petals",
            ParticlePreset::Snow => "snow",
            ParticlePreset::Rain => "rain",
            ParticlePreset::Fireflies => "fireflies",
            ParticlePreset::MoonlightDust => "moonlight_dust",
        }),
        "layers": p.pack.layers.iter().map(|l| path_str(&p.dir.join(&l.image))).collect::<Vec<_>>(),
    })
}

fn style_str(s: LayoutStyle) -> &'static str {
    match s {
        LayoutStyle::Magazine => "magazine",
        LayoutStyle::Classic => "classic",
    }
}

fn wallpaper_str(w: &Wallpaper) -> String {
    match w {
        Wallpaper::Auto => "auto".into(),
        Wallpaper::Pack => "pack".into(),
        Wallpaper::File(p) => path_str(p),
    }
}

/// 生效的配置。
fn config_json(c: &Config) -> Value {
    json!({
        "packs": c.packs.as_deref().map(path_str),
        "font": c.font,
        "wallpaper": wallpaper_str(&c.wallpaper),
        "text_color": c.text_color.rgb().map_or("auto".to_string(), |v| format!("#{v:06x}")),
        "style": style_str(c.style),
        "text_scale": c.text_scale,
        "start": c.start,
        "single": c.single,
        "fps": c.fps,
        "battery_fps": c.battery_fps,
        "pause_on_fullscreen": c.pause_on_fullscreen,
        "hold": c.hold,
        "char_interval": c.char_interval,
        "tray": c.tray,
    })
}

fn arrangement_json(a: &Arrangement) -> Value {
    json!({
        "x": a.placement.map(|p| p.x),
        "y": a.placement.map(|p| p.y),
        "scale": a.scale,
    })
}

fn sources_json(l: &crate::LayeredConfig) -> Value {
    l.sources
        .iter()
        .map(|(k, s)| (k.clone(), Value::from(*s)))
        .collect::<Map<_, _>>()
        .into()
}

/// 字体文件信息：设计窗口按它加载同样的字体。
pub fn font_json(font: &muse_render::Font) -> Value {
    sources_to_json(&font.description, &font.sources)
}

fn sources_to_json(description: &str, sources: &[Option<(PathBuf, u32)>; 3]) -> Value {
    let mut files: Vec<String> = Vec::new();
    let mut faces = Vec::new();
    for src in sources {
        let Some((path, index)) = src else {
            return json!({ "description": description, "files": [], "faces": [] });
        };
        let p = path_str(path);
        let i = files.iter().position(|f| *f == p).unwrap_or_else(|| {
            files.push(p);
            files.len() - 1
        });
        faces.push(json!([i, index]));
    }
    json!({ "description": description, "files": files, "faces": faces })
}

fn state_json(hub: &Hub) -> Result<Value, String> {
    let layered = crate::load_layered(&hub.ctx.overrides)?;
    let desktop = crate::wallpaper::current();
    let global = match &layered.config.wallpaper {
        Wallpaper::Auto => desktop.clone(),
        Wallpaper::Pack => None,
        Wallpaper::File(p) => Some(p.clone()),
    };
    let styles: Map<String, Value> = hub
        .ctx
        .packs
        .iter()
        .map(|p| {
            let name = muse_render::pack_name(p);
            let doc = PoemStyle::load(&name).to_doc();
            (name, serde_json::to_value(doc).unwrap_or(Value::Null))
        })
        .collect();
    let font = hub
        .ctx
        .font
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Ok(json!({
        "status": status_json(hub),
        "config": config_json(&layered.config),
        "sources": sources_json(&layered),
        "warnings": layered.warnings,
        "paths": {
            "config": config::config_path().as_deref().map(path_str),
            "settings": config::settings_path().as_deref().map(path_str),
            "colors_dir": config::config_dir().map(|d| path_str(&d.join("poems"))),
            "layout_dir": config::state_dir().map(|d| path_str(&d.join("poems"))),
        },
        "poems": hub.ctx.packs.iter().map(poem_json).collect::<Vec<_>>(),
        "styles": styles,
        "arrangement": arrangement_json(&Arrangement::load()),
        "font": font,
        "wallpaper": {
            "global": global.as_deref().map(path_str),
            "desktop": desktop.as_deref().map(path_str),
        },
    }))
}

// ───────────── 写操作 ─────────────

fn style_paths(name: &str) -> Result<Vec<PathBuf>, String> {
    Ok(vec![
        PoemStyle::colors_path(name).ok_or("取不到配置目录")?,
        PoemStyle::layout_path(name).ok_or("取不到状态目录")?,
    ])
}

fn parse_style(v: &Value) -> Result<PoemStyle, String> {
    let doc: StyleDoc =
        serde_json::from_value(v.clone()).map_err(|e| format!("样式格式不对: {e}"))?;
    PoemStyle::from_doc(&doc)
}

fn write_style(name: &str, st: &PoemStyle) -> Result<(), String> {
    st.save_colors(name)?;
    st.save_layout(name)
}

/// 合并补丁：值为 null 的键删掉，其余覆盖。
fn patch_map<T: serde::de::DeserializeOwned>(
    map: &mut BTreeMap<String, T>,
    patch: Option<&Value>,
    what: &str,
) -> Result<(), String> {
    let Some(patch) = patch else { return Ok(()) };
    let obj = patch
        .as_object()
        .ok_or_else(|| format!("{what} 要是对象"))?;
    for (k, v) in obj {
        if v.is_null() {
            map.remove(k);
        } else {
            let val = serde_json::from_value(v.clone()).map_err(|e| format!("{what}.{k}: {e}"))?;
            map.insert(k.clone(), val);
        }
    }
    Ok(())
}

fn apply_patch(doc: &mut StyleDoc, params: &Value) -> Result<(), String> {
    patch_map::<PaintDoc>(&mut doc.colors, params.get("colors"), "colors")?;
    patch_map::<[f64; 2]>(&mut doc.offsets, params.get("offsets"), "offsets")?;
    patch_map::<f64>(&mut doc.sizes, params.get("sizes"), "sizes")?;
    patch_map::<f64>(&mut doc.opacity, params.get("opacity"), "opacity")?;
    if let Some(w) = params.get("wallpaper") {
        doc.wallpaper = match w {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            _ => return Err("wallpaper 要是字符串或 null".into()),
        };
    }
    Ok(())
}

/// JSON 值转成配置里的 TOML 值；小数项给整数时也按小数存。
fn toml_value(key: &str, v: &Value) -> Result<toml::Value, String> {
    let float_keys = ["text_scale", "hold", "char_interval"];
    Ok(match v {
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Number(n) if float_keys.contains(&key) => {
            toml::Value::Float(n.as_f64().ok_or_else(|| format!("{key}: 不是有效的数"))?)
        }
        Value::Number(n) => match n.as_i64() {
            Some(i) => toml::Value::Integer(i),
            None => toml::Value::Float(n.as_f64().ok_or_else(|| format!("{key}: 不是有效的数"))?),
        },
        _ => return Err(format!("{key}: 只支持字符串、数字、true / false")),
    })
}

fn settings_changes(v: &Value) -> Result<BTreeMap<String, Option<toml::Value>>, String> {
    let obj = v.as_object().ok_or("settings 要是对象")?;
    obj.iter()
        .map(|(k, v)| {
            let val = if v.is_null() {
                None
            } else {
                Some(toml_value(k, v)?)
            };
            Ok((k.clone(), val))
        })
        .collect()
}

fn write_settings(changes: &BTreeMap<String, Option<toml::Value>>) -> Result<(), String> {
    let cfg = config::config_path().ok_or("取不到配置目录")?;
    let set = config::settings_path().ok_or("取不到状态目录")?;
    config::update_settings(&cfg, &set, changes)
}

/// 按参数算出新的整首位置和字号倍数。
fn arrangement_from(v: &Value) -> Result<Option<Arrangement>, String> {
    if v.as_str() == Some("reset") || v.get("reset").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    let mut a = Arrangement::load();
    let num = |k: &str| v.get(k).and_then(Value::as_f64).map(|x| x as f32);
    match (num("x"), num("y")) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => {
            a.placement = Some(Placement {
                x: x.clamp(0.0, 1.0),
                y: y.clamp(0.0, 1.0),
            })
        }
        // x、y 都明确给了 null 才恢复默认位置；没给就保留。
        (None, None)
            if v.get("x").is_some_and(Value::is_null) && v.get("y").is_some_and(Value::is_null) =>
        {
            a.placement = None
        }
        (None, None) => {}
        _ => return Err("x 和 y 要一起给".into()),
    }
    if let Some(s) = num("scale") {
        if !(0.1..=10.0).contains(&s) {
            return Err(format!("scale 要在 0.1 到 10 之间，现在是 {s}"));
        }
        a.scale = s;
    }
    Ok(Some(a))
}

fn write_arrangement(a: Option<Arrangement>) -> Result<(), String> {
    match a {
        Some(a) => a.save().map(|_| ()),
        None => muse_core::style::remove_file(&Arrangement::path().ok_or("取不到状态目录")?),
    }
}

fn save(hub: &Hub, params: &Value) -> Result<Value, String> {
    let mut paths = Vec::new();
    let mut touched = Touched::default();
    let arrangement = match param(params, "arrangement") {
        Some(v) => {
            paths.push(Arrangement::path().ok_or("取不到状态目录")?);
            touched.arrangement = true;
            // 先校验参数，真正的读改写放进事务的锁里。
            arrangement_from(v)?;
            Some(v.clone())
        }
        None => None,
    };
    let settings = match param(params, "settings") {
        Some(v) => {
            paths.push(config::settings_path().ok_or("取不到状态目录")?);
            touched.settings = true;
            Some(settings_changes(v)?)
        }
        None => None,
    };
    let mut styles = Vec::new();
    if let Some(obj) = param(params, "styles") {
        let obj = obj.as_object().ok_or("styles 要是对象")?;
        for (name, v) in obj {
            find_pack(hub, name)?;
            let st = parse_style(v).map_err(|e| format!("{name}: {e}"))?;
            paths.extend(style_paths(name)?);
            touched.styles.push(name.clone());
            styles.push((name.clone(), st));
        }
    }
    let n = touched.styles.len();
    let description = match (n, touched.settings, touched.arrangement) {
        (0, true, false) => "修改全局设置".to_string(),
        (0, false, true) => "调整整首位置".to_string(),
        (1, false, false) => format!("修改 {} 的样式", touched.styles[0]),
        _ => "保存设计窗口的修改".to_string(),
    };
    transaction(hub, description, paths, touched, || {
        if let Some(v) = &arrangement {
            write_arrangement(arrangement_from(v)?)?;
        }
        if let Some(c) = &settings {
            write_settings(c)?;
        }
        for (name, st) in &styles {
            write_style(name, st)?;
        }
        Ok(())
    })?;
    Ok(Value::Null)
}

fn undo(hub: &Hub) -> Result<Value, String> {
    let mut store = hub.store.lock().unwrap_or_else(|e| e.into_inner());
    // 恢复成功才从历史里拿掉，失败时这条还在，可以再试。
    let last = store.history.last().ok_or("没有可以撤销的修改")?;
    restore(&last.files)?;
    let change = store.history.pop().expect("上面已确认有");
    drop(store);
    notify(
        hub,
        &Touched {
            styles: Vec::new(),
            settings: true,
            arrangement: true,
        },
        "undo",
    );
    Ok(json!({ "undone": change.description }))
}

fn history(hub: &Hub) -> Value {
    let store = hub.store.lock().unwrap_or_else(|e| e.into_inner());
    store
        .history
        .iter()
        .rev()
        .map(|c| json!({ "description": c.description, "time": c.time }))
        .collect::<Vec<_>>()
        .into()
}

/// 处理一个请求。
pub fn handle(hub: &Hub, method: &str, params: &Value) -> Result<Value, String> {
    let send = |c| {
        hub.remote.send(c);
        Ok(Value::Null)
    };
    match method {
        "status" | "subscribe" => Ok(status_json(hub)),
        "next" => send(Command::Next),
        "prev" => send(Command::Prev),
        "goto" => {
            let i = match param(params, "pack") {
                Some(Value::String(s)) => find_pack(hub, s)?,
                Some(Value::Number(n)) => n
                    .as_u64()
                    .map(|n| n as usize)
                    .filter(|&n| n < hub.ctx.packs.len())
                    .ok_or("序号超出范围")?,
                _ => return Err("缺少参数 pack".into()),
            };
            send(Command::Goto(i))
        }
        "pause" => match param(params, "paused").and_then(Value::as_bool) {
            Some(p) => send(Command::SetPaused(p)),
            None => send(Command::TogglePause),
        },
        "quit" => send(Command::Quit),
        "reload" => send(Command::Reload),
        "state" => state_json(hub),
        "poems" => Ok(hub
            .ctx
            .packs
            .iter()
            .map(poem_json)
            .collect::<Vec<_>>()
            .into()),
        "style.get" => {
            let name = pack_param(hub, params)?;
            Ok(serde_json::to_value(PoemStyle::load(&name).to_doc()).unwrap_or(Value::Null))
        }
        "style.set" => {
            let name = pack_param(hub, params)?;
            let st = parse_style(param(params, "style").unwrap_or(&json!({})))?;
            let paths = style_paths(&name)?;
            let touched = Touched {
                styles: vec![name.clone()],
                ..Default::default()
            };
            transaction(
                hub,
                format!("修改 {name} 的样式"),
                paths,
                touched,
                || write_style(&name, &st),
            )?;
            Ok(Value::Null)
        }
        "style.patch" => {
            let name = pack_param(hub, params)?;
            let paths = style_paths(&name)?;
            let touched = Touched {
                styles: vec![name.clone()],
                ..Default::default()
            };
            // 读、改、写都在事务的锁里，和别的写操作不会互相覆盖。
            let mut out = None;
            transaction(
                hub,
                format!("修改 {name} 的样式"),
                paths,
                touched,
                || {
                    let mut doc = PoemStyle::load(&name).to_doc();
                    apply_patch(&mut doc, params)?;
                    let st = PoemStyle::from_doc(&doc)?;
                    write_style(&name, &st)?;
                    out = Some(st.to_doc());
                    Ok(())
                },
            )?;
            Ok(serde_json::to_value(out.unwrap_or_default()).unwrap_or(Value::Null))
        }
        "settings.get" => {
            let layered = crate::load_layered(&hub.ctx.overrides)?;
            let values = config::settings_path()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|t| t.parse::<toml::Table>().ok())
                .map(|t| serde_json::to_value(t).unwrap_or(Value::Null))
                .unwrap_or_else(|| json!({}));
            Ok(json!({
                "values": values,
                "effective": config_json(&layered.config),
                "sources": sources_json(&layered),
                "warnings": layered.warnings,
            }))
        }
        "settings.set" => {
            let changes = settings_changes(param(params, "values").ok_or("缺少参数 values")?)?;
            let path = config::settings_path().ok_or("取不到状态目录")?;
            let touched = Touched {
                settings: true,
                ..Default::default()
            };
            transaction(
                hub,
                "修改全局设置".into(),
                vec![path],
                touched,
                || write_settings(&changes),
            )?;
            Ok(Value::Null)
        }
        "arrangement.get" => Ok(arrangement_json(&Arrangement::load())),
        "arrangement.set" => {
            arrangement_from(params)?;
            let path = Arrangement::path().ok_or("取不到状态目录")?;
            let touched = Touched {
                arrangement: true,
                ..Default::default()
            };
            transaction(
                hub,
                "调整整首位置".into(),
                vec![path],
                touched,
                || write_arrangement(arrangement_from(params)?),
            )?;
            Ok(arrangement_json(&Arrangement::load()))
        }
        "save" => save(hub, params),
        "fonts" => Ok(json!({ "families": muse_render::cjk_families() })),
        "font.locate" => {
            let spec = param(params, "font")
                .and_then(Value::as_str)
                .filter(|f| *f != "auto");
            let (desc, sources) = muse_render::locate_font(spec)?;
            Ok(sources_to_json(&desc, &sources))
        }
        "undo" => undo(hub),
        "history" => Ok(history(hub)),
        "" => Err("缺少 method".into()),
        m => Err(format!("不认识的方法 {m}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{Hub, channel_pair};

    fn read(p: Option<PathBuf>) -> String {
        std::fs::read_to_string(p.unwrap()).unwrap_or_default()
    }

    /// 只有这一个测试改环境变量，用临时目录代替真实的配置、状态、主目录。
    #[test]
    fn writes_validates_and_undoes() {
        let root = std::env::temp_dir().join(format!("muse-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["config", "state", "home"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // 安全性：测试里只有这里改环境变量。
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
            std::env::set_var("XDG_STATE_HOME", root.join("state"));
            std::env::set_var("HOME", root.join("home"));
        }
        let packs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        let (packs, _) = muse_core::load_all(&packs_dir).unwrap();
        let (remote, inbox) = channel_pair();
        let hub = Hub::new(
            remote,
            Context {
                overrides: crate::Overrides::default(),
                packs: packs.into(),
                font: Mutex::new(json!({})),
            },
        );
        let call = |m: &str, p: Value| handle(&hub, m, &p);

        let st = call(
            "style.patch",
            json!({ "pack": "jing-ye-si", "colors": { "title": "#ff0000", "line.1": ["#000", "#fff8"] },
                    "sizes": { "seal": 1.5 }, "offsets": { "title": [0.1, 0.0] } }),
        );
        // #fff8 不是合法写法（只支持 3、6、8 位），整个补丁被拒绝。
        assert!(st.is_err(), "{st:?}");
        assert!(PoemStyle::colors_path("jing-ye-si").is_some_and(|p| !p.exists()));

        let st = call(
            "style.patch",
            json!({ "pack": "jing-ye-si", "colors": { "title": "#ff0000", "line.1": ["#000", "#ffffff80"] },
                    "sizes": { "seal": 1.5 }, "offsets": { "title": [0.1, 0.0] }, "wallpaper": "pack" }),
        )
        .unwrap();
        assert_eq!(st["colors"]["title"], "#ff0000");
        let colors = read(PoemStyle::colors_path("jing-ye-si"));
        assert!(
            colors.contains("title = \"#ff0000\"") && colors.contains("wallpaper = \"pack\""),
            "{colors}"
        );
        let layout = read(PoemStyle::layout_path("jing-ye-si"));
        assert!(
            layout.contains("[sizes]") && layout.contains("seal"),
            "{layout}"
        );
        assert_eq!(inbox.try_recv(), Some(Command::Reload));

        // 删掉一项。
        call(
            "style.patch",
            json!({ "pack": "jing-ye-si", "colors": { "title": null } }),
        )
        .unwrap();
        assert!(!read(PoemStyle::colors_path("jing-ye-si")).contains("title"));

        // 设置：写错的整体拒绝，config.toml 不动。
        assert!(call("settings.set", json!({ "values": { "fps": 500 } })).is_err());
        assert!(call("settings.set", json!({ "values": { "nope": 1 } })).is_err());
        call(
            "settings.set",
            json!({ "values": { "fps": 60, "text_scale": 1 } }),
        )
        .unwrap();
        let s = call("settings.get", Value::Null).unwrap();
        assert_eq!(s["effective"]["fps"], 60);
        assert_eq!(s["sources"]["fps"], "settings");
        assert!(!root.join("config/muse/config.toml").exists());

        // 一次保存多项，一次撤销全部撤回。
        call(
            "save",
            json!({ "arrangement": { "x": 0.3, "y": 0.5, "scale": 1.2 },
                    "settings": { "battery_fps": 0 },
                    "styles": { "jing-ye-si": { "colors": { "color": "#123456" } } } }),
        )
        .unwrap();
        assert!(read(Arrangement::path()).contains("x = 0.3000"));
        // 只改字号倍数不会清掉位置（MCP 只给 scale 时）。
        call("arrangement.set", json!({ "scale": 1.1 })).unwrap();
        assert!(read(Arrangement::path()).contains("x = 0.3000"));
        call("undo", Value::Null).unwrap();
        assert_eq!(
            call("history", Value::Null)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            4
        );
        let u = call("undo", Value::Null).unwrap();
        assert_eq!(u["undone"], "保存设计窗口的修改");
        assert!(Arrangement::path().is_some_and(|p| !p.exists()));
        assert!(!read(config::settings_path()).contains("battery_fps"));
        let st = call("style.get", json!({ "pack": "jing-ye-si" })).unwrap();
        assert_eq!(st["colors"]["line.1"], json!(["#000000", "#ffffff80"]));
        assert!(st["colors"].get("color").is_none());

        // 撤销到底，文件全部回到原样（没有文件）。
        while call("undo", Value::Null).is_ok() {}
        assert!(PoemStyle::colors_path("jing-ye-si").is_some_and(|p| !p.exists()));
        assert!(config::settings_path().is_some_and(|p| !p.exists()));
        // 改之前已有文件的修改都有备份：删标题、批量保存、只改字号倍数各一份（新建文件、撤销不备份）。
        assert_eq!(
            root.join("state/muse/backups").read_dir().unwrap().count(),
            3
        );

        let state = call("state", Value::Null).unwrap();
        assert_eq!(
            state["poems"].as_array().unwrap().len(),
            hub.ctx.packs.len()
        );
        assert!(call("goto", json!({ "pack": "nope" })).is_err());
        call("goto", json!({ "pack": "jing-ye-si" })).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }
}
