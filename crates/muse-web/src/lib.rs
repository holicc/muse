//! 设计窗口的画布：muse-render 编译成 wasm，在 WebGPU 画布上画出和桌面壁纸一样的画面。
//!
//! 网页读不了磁盘，字体和图片由 Electron 主进程读出来传进来：
//! 先 `new Builder()`，`add_font_file` / `add_image` 放进数据，再 `build(canvas, 设置)` 得到 `Preview`。
//! 坐标都是画布像素。

#![cfg(target_arch = "wasm32")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use muse_core::LoadedPack;
use muse_render::{Font, ImageCache, Options, Part, Placement, PoemStyle, Renderer, StyleDoc};
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::prelude::*;

/// 设置，键名和接口 `state` 里的一致。
#[derive(Deserialize)]
struct Settings {
    #[serde(default)]
    style: Option<String>,
    /// 实际字号倍数（全局字号 × 整首字号）。
    #[serde(default = "one")]
    text_scale: f32,
    #[serde(default)]
    placement: Option<[f32; 2]>,
    /// "#rrggbb"；None 自动配色。
    #[serde(default)]
    text_color: Option<String>,
    /// 全局背景图；None 用诗词包自带。
    #[serde(default)]
    wallpaper: Option<String>,
    #[serde(default)]
    desktop: Option<String>,
    #[serde(default)]
    hold: Option<f32>,
    #[serde(default)]
    char_interval: Option<f32>,
    #[serde(default)]
    styles: HashMap<String, StyleDoc>,
}

fn one() -> f32 {
    1.0
}

#[derive(Deserialize)]
struct PackSource {
    name: String,
    toml: String,
}

#[derive(Deserialize)]
struct Init {
    packs: Vec<PackSource>,
    #[serde(default)]
    start: usize,
    #[serde(flatten)]
    settings: Settings,
}

fn err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// 按设置算出渲染选项。写错的样式报错（设计窗口只会传校验过的）。
fn options(s: &Settings, start: usize) -> Result<Options, JsValue> {
    let mut styles = HashMap::new();
    for (name, doc) in &s.styles {
        let st = PoemStyle::from_doc(doc).map_err(|e| err(format!("{name}: {e}")))?;
        styles.insert(name.clone(), st);
    }
    let mut timing = muse_core::scene::Timing::default();
    if let Some(h) = s.hold {
        timing.hold = h;
    }
    if let Some(c) = s.char_interval {
        timing.char_interval = c;
    }
    let text_color = match s.text_color.as_deref() {
        None | Some("auto") => None,
        Some(c) => Some(muse_core::style::parse_hex(c).map_err(err)?),
    };
    Ok(Options {
        timing,
        start,
        single: true,
        style: match s.style.as_deref() {
            Some("classic") => muse_core::LayoutStyle::Classic,
            _ => muse_core::LayoutStyle::Magazine,
        },
        text_scale: s.text_scale.clamp(0.3, 3.0),
        wallpaper: s.wallpaper.as_ref().map(PathBuf::from),
        desktop: s.desktop.as_ref().map(PathBuf::from),
        placement: s.placement.map(|[x, y]| Placement { x, y }),
        text_color,
        styles: Arc::new(styles),
    })
}

/// 收集字体和图片数据，然后创建画布。
#[wasm_bindgen]
#[derive(Default)]
pub struct Builder {
    fonts: Vec<Vec<u8>>,
    images: ImageCache,
}

#[wasm_bindgen]
impl Builder {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        console_error_panic_hook::set_once();
        Self::default()
    }

    /// 加一个字体文件，返回它的序号（`build` 的 faces 里用）。
    pub fn add_font_file(&mut self, data: Vec<u8>) -> usize {
        self.fonts.push(data);
        self.fonts.len() - 1
    }

    /// 放进一张图，`path` 和设置、诗词包里写的路径一致。
    pub fn add_image(&self, path: String, data: Vec<u8>) -> Result<(), JsValue> {
        self.images
            .insert_encoded(PathBuf::from(path), &data)
            .map_err(err)
    }

    /// 创建画布。`faces` 是细、常规、粗三个字重 `[[文件序号, 文件里第几个字体], ...]` 的 JSON，
    /// `init` 是 `{packs: [{name, toml}], start, ...设置}` 的 JSON。
    pub async fn build(
        self,
        canvas: web_sys::HtmlCanvasElement,
        faces: String,
        init: String,
    ) -> Result<Preview, JsValue> {
        let faces: [(usize, u32); 3] = serde_json::from_str(&faces).map_err(err)?;
        let init: Init = serde_json::from_str(&init).map_err(err)?;
        let font = Font::from_files(self.fonts, faces, "font").map_err(err)?;
        let mut packs = Vec::new();
        for p in init.packs {
            let pack = toml::from_str(&p.toml).map_err(|e| err(format!("{}: {e}", p.name)))?;
            packs.push(LoadedPack {
                dir: PathBuf::from(&p.name),
                pack,
            });
        }
        if packs.is_empty() {
            return Err(err("没有诗词包"));
        }
        let options = options(&init.settings, init.start)?;
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| err(format!("创建画布失败: {e}")))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| err(format!("没有可用的 WebGPU: {e}")))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| err(format!("创建显卡设备失败: {e}")))?;
        // 网页画布不能直接用 sRGB 格式，用 sRGB 视图写入，颜色处理和桌面端一致。
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats[0];
        let view_format = format.add_srgb_suffix();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![view_format],
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        let mut renderer = Renderer::new(
            device.clone(),
            queue.clone(),
            view_format,
            Arc::new(font),
            Arc::from(packs),
            options,
            self.images.clone(),
        );
        renderer.resize(width, height);
        Ok(Preview {
            surface,
            config,
            device,
            queue,
            renderer,
            view_format,
            images: self.images,
            fonts: Vec::new(),
        })
    }
}

#[wasm_bindgen]
pub struct Preview {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    view_format: wgpu::TextureFormat,
    images: ImageCache,
    /// 换字体时先放进来的字体文件。
    fonts: Vec<Vec<u8>>,
}

#[wasm_bindgen]
impl Preview {
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.renderer.resize(width, height);
    }

    /// 换设置（JSON，同 `build` 的设置部分），停在当前这首。
    pub fn set_options(&mut self, settings: String) -> Result<(), JsValue> {
        let s: Settings = serde_json::from_str(&settings).map_err(err)?;
        let o = options(&s, self.renderer.options().start)?;
        self.renderer.reconfigure(&o);
        Ok(())
    }

    pub fn add_image(&self, path: String, data: Vec<u8>) -> Result<(), JsValue> {
        self.images
            .insert_encoded(PathBuf::from(path), &data)
            .map_err(err)
    }

    /// 这张图已经放进来了吗（切诗时只保留当前和下一首的图，其余会被释放）。
    pub fn has_image(&self, path: String) -> bool {
        self.images.contains(std::path::Path::new(&path))
    }

    /// 第 `index` 首诗要用、还没放进来的图片路径（JSON 数组）。设置变了之后也要再问一次。
    pub fn missing_images(&self, index: usize) -> String {
        let packs = self.renderer.packs();
        let Some(pack) = packs.get(index % packs.len().max(1)) else {
            return "[]".into();
        };
        let missing: Vec<String> = muse_render::layer_paths(pack, self.renderer.options())
            .into_iter()
            .filter(|p| !self.images.contains(p))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        json!(missing).to_string()
    }

    /// 换字体：先 `add_font_file`，再 `set_font(faces)`。
    pub fn add_font_file(&mut self, data: Vec<u8>) -> usize {
        self.fonts.push(data);
        self.fonts.len() - 1
    }

    pub fn set_font(&mut self, faces: String) -> Result<(), JsValue> {
        let faces: [(usize, u32); 3] = serde_json::from_str(&faces).map_err(err)?;
        let files = std::mem::take(&mut self.fonts);
        let font = Font::from_files(files, faces, "font").map_err(err)?;
        self.renderer.set_font(Arc::new(font));
        Ok(())
    }

    pub fn set_pack(&mut self, index: usize) {
        self.renderer.set_pack(index);
    }

    pub fn current_index(&self) -> usize {
        self.renderer.current_index()
    }

    pub fn advance(&mut self, dt: f32) {
        self.renderer.advance(dt);
    }

    /// 停在全诗刚好显示完的时刻。
    pub fn reveal_all(&mut self) {
        self.renderer.reveal_all();
    }

    pub fn seek(&mut self, t: f32) {
        self.renderer.seek(t);
    }

    pub fn time(&self) -> f32 {
        self.renderer.time()
    }

    /// 这一点落在哪个部件上（如 `line.2`），没有返回空串。不含背景大字。
    pub fn part_at(&self, x: f32, y: f32) -> String {
        self.renderer
            .part_at(x, y)
            .map(|p| p.key())
            .unwrap_or_default()
    }

    /// 拖动中的偏移（像素）；`part` 为空串表示整首。松手后用 `set_options` 定下来。
    pub fn set_drag(&mut self, part: &str, dx: f32, dy: f32) {
        self.renderer.set_drag(Part::parse(part), dx, dy);
    }

    /// 当前页每个部件的外接框（像素）：`[{part, rect: [x0, y0, x1, y1]}]`。
    pub fn parts(&self) -> String {
        let v: Vec<_> = self
            .renderer
            .part_bounds()
            .into_iter()
            .map(|(p, r)| json!({ "part": p.key(), "rect": r }))
            .collect();
        json!(v).to_string()
    }

    /// 当前页每个字：`[{part, index, ch, rect}]`，index 是部件里第几个字（从 0 开始）。
    pub fn chars(&self) -> String {
        let v: Vec<_> = self
            .renderer
            .chars()
            .into_iter()
            .map(|(p, i, c, r)| json!({ "part": p.key(), "index": i, "ch": c.to_string(), "rect": r }))
            .collect();
        json!(v).to_string()
    }

    /// 诗主体的外接框（像素），没有时为 null。
    pub fn main_bounds(&self) -> String {
        json!(self.renderer.main_bounds()).to_string()
    }

    /// 诗主体现在的中心（0..1），整首拖动后据此算新位置。
    pub fn placement(&self) -> String {
        json!(self.renderer.placement().map(|p| [p.x, p.y])).to_string()
    }

    /// 自动配色的依据：`{luma, light, ink: "#rrggbb", on_wallpaper}`。
    pub fn auto_color(&self) -> String {
        let a = self.renderer.auto_color();
        json!({
            "luma": a.luma,
            "light": a.light,
            "ink": format!("#{:06x}", a.ink),
            "on_wallpaper": a.on_wallpaper,
        })
        .to_string()
    }

    /// 长诗分页：`[第几页, 共几页]`，从 0 开始。
    pub fn page(&self) -> String {
        let (p, n) = self.renderer.page();
        json!([p, n]).to_string()
    }

    pub fn render(&mut self) -> Result<(), JsValue> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            other => return Err(err(format!("取不到画布纹理: {other:?}"))),
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        self.renderer.render(&view);
        self.queue.present(frame);
        Ok(())
    }
}
