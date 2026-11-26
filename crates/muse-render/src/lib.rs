//! 用 wgpu 把诗词包画出来。和窗口系统无关：调用方给 `Device`、`Queue` 和目标纹理。

mod font;
mod images;

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use muse_core::scene::{
    self, Layout, LayoutOptions, ParticleSystem, Role, ShapeKind, Timeline, Timing, is_main,
};
use muse_core::{LayoutStyle, LoadedPack, ParticlePreset};

use font::{Atlas, Weight};
pub use font::{Font, cjk_families, locate as locate_font};
pub use images::ImageCache;
pub use muse_core::config::Wallpaper;
pub use muse_core::scene::{Part, Placement};
pub use muse_core::style::{Color, Paint, PoemStyle, StyleDoc, Styles};

// ───────────── 配色 ─────────────

/// 一首诗的配色，按粒子预设（即季节、时辰）决定。颜色都是 sRGB。
#[derive(Debug, Clone, Copy)]
struct Palette {
    bg_top: u32,
    bg_bottom: u32,
    mist: (u32, f32),
    ink: u32,
    /// 署名、注释、细线。
    accent: u32,
    /// 印章底色。
    seal: u32,
    particle: (u32, f32),
}

fn palette(preset: Option<ParticlePreset>) -> Palette {
    match preset {
        Some(ParticlePreset::Petals) => Palette {
            bg_top: 0xf4e6e1,
            bg_bottom: 0xe8d2c3,
            mist: (0xffffff, 0.35),
            ink: 0x2a221e,
            accent: 0x8a3b2c,
            seal: 0xb3382c,
            particle: (0xf0a3b4, 0.9),
        },
        Some(ParticlePreset::Snow) => Palette {
            bg_top: 0xcfd6de,
            bg_bottom: 0xedf0f2,
            mist: (0xffffff, 0.4),
            ink: 0x1d2830,
            accent: 0x55687a,
            seal: 0xb3382c,
            particle: (0xffffff, 0.95),
        },
        Some(ParticlePreset::Rain) => Palette {
            bg_top: 0x4f5a64,
            bg_bottom: 0x7f8a92,
            mist: (0xc0c8ce, 0.25),
            ink: 0xeef1f2,
            accent: 0xc3ccd2,
            seal: 0xb3382c,
            particle: (0xe0e8ef, 1.0),
        },
        Some(ParticlePreset::Fireflies) => Palette {
            bg_top: 0x09111f,
            bg_bottom: 0x16252c,
            mist: (0x2b4148, 0.3),
            ink: 0xe9e2cc,
            accent: 0xb8a874,
            seal: 0xc0473a,
            particle: (0xd6f07a, 1.0),
        },
        Some(ParticlePreset::MoonlightDust) => Palette {
            bg_top: 0x0c142c,
            bg_bottom: 0x253050,
            mist: (0x7a88b8, 0.18),
            ink: 0xe8ebf5,
            accent: 0xa7b2d4,
            seal: 0xc0473a,
            particle: (0xe6ecff, 0.9),
        },
        None => Palette {
            bg_top: 0xf0e9db,
            bg_bottom: 0xe2d7c2,
            mist: (0xffffff, 0.3),
            ink: 0x222020,
            accent: 0x8b2e24,
            seal: 0xb3382c,
            particle: (0xffffff, 0.0),
        },
    }
}

/// 画在用户壁纸上时的配色：按文字区域的亮度选深色字或浅色字，粒子颜色沿用诗词包的。
/// `bg_top` 是印章字的纸色。
fn wallpaper_palette(light: bool, preset: Option<ParticlePreset>) -> Palette {
    let base = palette(preset);
    if light {
        Palette {
            bg_top: 0xf6f1e7,
            bg_bottom: 0xf6f1e7,
            ink: 0x1f1b18,
            accent: 0x6b3a2e,
            seal: 0xb3382c,
            ..base
        }
    } else {
        Palette {
            bg_top: 0x101216,
            bg_bottom: 0x101216,
            ink: 0xf3efe6,
            accent: 0xd9cdb5,
            seal: 0xb8402f,
            ..base
        }
    }
}

/// 用户指定了字色时，标题、正文用它，署名、刊头等小字和细线也用它（按角色自带的透明度显得淡一些）。
/// 印章保持原来的朱红。
fn with_text_color(p: Palette, color: Option<u32>) -> Palette {
    match color {
        Some(c) => Palette {
            ink: c,
            accent: c,
            ..p
        },
        None => p,
    }
}

/// 每个字的颜色（线性 RGBA）：单字设置 > 部件设置 > 整首设置 > 配色。
/// 不透明度 = 角色自带的 × 颜色自带的 × 部件的。
fn char_colors(layout: &Layout, pal: &Palette, style: Option<&PoemStyle>) -> Vec<[f32; 4]> {
    let defaults = layout.chars.iter().map(|c| role_style(c.role, pal).1);
    let Some(st) = style else {
        return defaults.collect();
    };
    let opacity = |p: &Part| st.opacity.get(p).copied().unwrap_or(1.0);
    // 部件里第几个字、共几个字；整首里（标题、正文、署名）第几个字、共几个字。
    let mut in_part: std::collections::HashMap<Part, usize> = Default::default();
    let mut index = Vec::with_capacity(layout.chars.len());
    let poem = |r: Role| matches!(r, Role::Title | Role::Body | Role::Byline);
    let mut in_poem = 0;
    for c in &layout.chars {
        let n = in_part.entry(c.part).or_default();
        index.push((*n, in_poem));
        *n += 1;
        if poem(c.role) {
            in_poem += 1;
        }
    }
    let t = |i: usize, n: usize| {
        if n > 1 {
            i as f32 / (n - 1) as f32
        } else {
            0.0
        }
    };
    layout
        .chars
        .iter()
        .zip(index)
        .zip(defaults)
        .map(|((c, (i, k)), def)| {
            let rgb = st
                .chars
                .get(&(c.part, i))
                .copied()
                .or_else(|| st.parts.get(&c.part).map(|p| p.at(t(i, in_part[&c.part]))))
                .or_else(|| {
                    st.color
                        .as_ref()
                        .filter(|_| poem(c.role))
                        .map(|p| p.at(t(k, in_poem)))
                });
            let mut col = match rgb {
                // 印章里的字默认是纸色，只有单独给它设了颜色才换。
                Some(_) if c.role == Role::Seal && !st.chars.contains_key(&(c.part, i)) => def,
                Some(color) => linear(color.rgb, def[3] * color.alpha()),
                None => def,
            };
            col[3] *= opacity(&c.part);
            col
        })
        .collect()
}

/// 用户壁纸缩小后的亮度图（线性亮度），用来判断文字区域是亮是暗。
struct Backdrop {
    luma: Vec<f32>,
    w: usize,
    h: usize,
    aspect: f32,
}

impl Backdrop {
    fn new(img: &image::RgbaImage) -> Self {
        let (w, h) = img.dimensions();
        let k = 256.0 / w.max(h) as f32;
        let (tw, th) = (
            ((w as f32 * k).round() as u32).max(1),
            ((h as f32 * k).round() as u32).max(1),
        );
        let small = image::imageops::thumbnail(img, tw, th);
        let luma = small
            .pixels()
            .map(|p| {
                let [r, g, b, _] = p.0;
                luminance(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
            })
            .collect();
        Self {
            luma,
            w: tw as usize,
            h: th as usize,
            aspect: w as f32 / h as f32,
        }
    }

    /// 屏幕上一块区域（像素）里的平均亮度。图片按 cover 铺满 `screen`。
    fn mean(&self, rect: [f32; 4], screen: (f32, f32)) -> f32 {
        let [x0, y0, x1, y1] = cover_rect(self.aspect, 1.0, screen, 0.0);
        let (iw, ih) = (x1 - x0, y1 - y0);
        let gx = |x: f32| (((x - x0) / iw * self.w as f32) as isize).clamp(0, self.w as isize - 1);
        let gy = |y: f32| (((y - y0) / ih * self.h as f32) as isize).clamp(0, self.h as isize - 1);
        let (mut sum, mut n) = (0.0, 0.0);
        for y in gy(rect[1])..=gy(rect[3]) {
            for x in gx(rect[0])..=gx(rect[2]) {
                let v = self.luma[y as usize * self.w + x as usize];
                sum += v;
                n += 1.0;
            }
        }
        sum / n
    }
}

/// 图片按 cover 铺满屏幕时的位置（像素），`scale` 大于 1 时多放大一点留出视差漂移的余量。
fn cover_rect(aspect: f32, scale: f32, (w, h): (f32, f32), drift: f32) -> [f32; 4] {
    let (iw, ih) = if aspect > w / h {
        (h * aspect * scale, h * scale)
    } else {
        (w * scale, w / aspect * scale)
    };
    let x0 = (w - iw) / 2.0 + drift;
    let y0 = (h - ih) / 2.0;
    [x0, y0, x0 + iw, y0 + ih]
}

/// 长诗分页时，每页停留时间按页数分摊（至少 12 秒），免得一首长诗占屏太久。
fn page_timing(t: Timing, pages: usize) -> Timing {
    if pages <= 1 {
        return t;
    }
    Timing {
        hold: (t.hold / pages as f32).max(12.0).min(t.hold),
        ..t
    }
}

/// 每种角色的字重和颜色（线性 RGBA）。
fn role_style(role: Role, pal: &Palette) -> (Weight, [f32; 4]) {
    let dark = luminance(pal.bg_top) < 0.3;
    match role {
        Role::Title => (Weight::Heavy, linear(pal.ink, 1.0)),
        Role::Body => (Weight::Regular, linear(pal.ink, 1.0)),
        Role::Byline => (Weight::Regular, linear(pal.accent, 0.92)),
        Role::Caption => (Weight::Light, linear(pal.accent, 0.8)),
        // 阴文印章：字用纸色。
        Role::Seal => (Weight::Heavy, linear(pal.bg_top, 0.95)),
        Role::Watermark => (
            Weight::Heavy,
            // 在线性空间里混合，深色背景上的浅色字看起来会亮很多，所以透明度要更低。
            linear(pal.ink, if dark { 0.007 } else { 0.035 }),
        ),
    }
}

/// 按 WCAG 对比度，在这个亮度的背景上深色字是否比浅色字更清楚。
fn prefers_dark_ink(luma: f32) -> bool {
    (luma + 0.05) / 0.05 >= 1.05 / (luma + 0.05)
}

fn same_timing(a: Timing, b: Timing) -> bool {
    a.char_interval == b.char_interval
        && a.line_pause == b.line_pause
        && a.char_fade == b.char_fade
        && a.hold == b.hold
        && a.fade_out == b.fade_out
        && a.gap == b.gap
}

/// 自动配色的依据，设计窗口显示用。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoColor {
    /// 诗所在区域的亮度（线性 0..1）；诗词包自带背景时为背景的平均亮度。
    pub luma: f32,
    /// 背景偏亮，用深色字。
    pub light: bool,
    /// 自动选出的字色 0xRRGGBB。
    pub ink: u32,
    /// 画在用户壁纸上（否则是诗词包自带的背景）。
    pub on_wallpaper: bool,
}

fn luminance(hex: u32) -> f32 {
    let [r, g, b, _] = linear(hex, 1.0);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// sRGB 十六进制转线性 RGBA。
fn linear(hex: u32, alpha: f32) -> [f32; 4] {
    let c = |v: u32| {
        let s = (v & 0xff) as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    [c(hex >> 16), c(hex >> 8), c(hex), alpha]
}

// ───────────── GPU 数据 ─────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    screen: [f32; 2],
    time: f32,
    particle_kind: u32,
    bg_top: [f32; 4],
    bg_bottom: [f32; 4],
    mist: [f32; 4],
    particle: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ImageInst {
    rect: [f32; 4],
    alpha: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlyphInst {
    center: [f32; 2],
    half_size: [f32; 2],
    uv0: [f32; 2],
    uv1: [f32; 2],
    color: [f32; 4],
    params: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShapeInst {
    rect: [f32; 4],
    /// 渐变起点、终点的颜色（纯色时相同）。
    color: [f32; 4],
    color2: [f32; 4],
    /// 渐变沿哪个方向（0 横向，1 纵向）和起止坐标（像素）。
    grad: [f32; 3],
    /// 0 细线，1 印章（边缘做一点斑驳）。
    kind: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParticleInst {
    center: [f32; 2],
    half_size: [f32; 2],
    params: [f32; 2],
}

struct Pipelines {
    bg: wgpu::RenderPipeline,
    image: wgpu::RenderPipeline,
    glyph: wgpu::RenderPipeline,
    particle: wgpu::RenderPipeline,
    shape: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
}

impl Pipelines {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("muse"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let plain_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("plain"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let textured_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("textured"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

        let make = |name: &str,
                    layout: &wgpu::PipelineLayout,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(&format!("{name}_vs")),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(&format!("{name}_fs")),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let instance =
            |stride: usize, attrs: &'static [wgpu::VertexAttribute]| wgpu::VertexBufferLayout {
                array_stride: stride as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: attrs,
            };
        let alpha = Some(wgpu::BlendState::ALPHA_BLENDING);

        const IMAGE_ATTRS: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32];
        const GLYPH_ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4, 5 => Float32x2
        ];
        const SHAPE_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x3, 4 => Float32
        ];
        const PARTICLE_ATTRS: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x2];

        Self {
            bg: make("bg", &plain_layout, &[], None),
            image: make(
                "image",
                &textured_layout,
                &[Some(instance(
                    std::mem::size_of::<ImageInst>(),
                    &IMAGE_ATTRS,
                ))],
                alpha,
            ),
            glyph: make(
                "glyph",
                &textured_layout,
                &[Some(instance(
                    std::mem::size_of::<GlyphInst>(),
                    &GLYPH_ATTRS,
                ))],
                alpha,
            ),
            particle: make(
                "particle",
                &plain_layout,
                &[Some(instance(
                    std::mem::size_of::<ParticleInst>(),
                    &PARTICLE_ATTRS,
                ))],
                alpha,
            ),
            shape: make(
                "shape",
                &plain_layout,
                &[Some(instance(
                    std::mem::size_of::<ShapeInst>(),
                    &SHAPE_ATTRS,
                ))],
                alpha,
            ),
            globals,
            globals_group,
            texture_layout,
        }
    }
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    (w, h): (u32, u32),
    format: wgpu::TextureFormat,
    data: &[u8],
) -> wgpu::BindGroup {
    let bpp = format.block_copy_size(None).unwrap_or(4);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * bpp),
            rows_per_image: Some(h),
        },
        texture.size(),
    );
    let view = texture.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    })
}

// ───────────── 场景 ─────────────

/// 这首诗要用到的图片（用户壁纸或诗词包的图层）。
pub fn layer_paths(pack: &LoadedPack, options: &Options) -> Vec<std::path::PathBuf> {
    if let Some(w) = options.wallpaper_for(pack) {
        return vec![w];
    }
    pack.pack
        .layers
        .iter()
        .take(MAX_LAYERS)
        .map(|l| pack.dir.join(&l.image))
        .collect()
}

/// 后台解码下一首的图层。
fn prefetch_next(images: &ImageCache, packs: &[LoadedPack], current: usize, options: &Options) {
    if packs.len() > 1 {
        images.prefetch(layer_paths(&packs[(current + 1) % packs.len()], options));
    }
}

struct Layer {
    group: wgpu::BindGroup,
    aspect: f32,
    depth: f32,
    /// cover 之外再放大的倍数，给视差漂移留余量；用户壁纸为 1，原样铺满。
    scale: f32,
}

/// 当前这首诗在 GPU 上的全部状态。
struct Scene {
    palette: Palette,
    /// 长诗分成的各页；短诗只有一页。
    pages: Vec<Layout>,
    /// 各页不含部件字号、位移的默认排版，整首定位以它为准。
    base: Vec<Layout>,
    /// 这首诗用的用户壁纸（None 为诗词包自带的背景），用来判断要不要重建场景。
    wallpaper: Option<std::path::PathBuf>,
    page: usize,
    /// 当前页的排版和时间线。
    timeline: Timeline,
    layout: Layout,
    particles: Option<ParticleSystem>,
    layers: Vec<Layer>,
    /// 画在用户壁纸上时有值，排版变化后据此重新配色。
    backdrop: Option<Backdrop>,
    /// 随屏幕尺寸变化重建。
    atlas_group: Option<wgpu::BindGroup>,
    /// 每个字在图集里的位置和字格偏移；字体里没有的字为 None。
    glyphs: Vec<Option<GlyphInst>>,
}

/// 渲染选项。
#[derive(Debug, Clone)]
pub struct Options {
    pub timing: Timing,
    /// 从第几首开始。
    pub start: usize,
    /// 只显示一首，不轮换。
    pub single: bool,
    /// 默认版式（诗词包里写了 text.style 时以包为准）。
    pub style: LayoutStyle,
    /// 整体字号倍数。
    pub text_scale: f32,
    /// 全局背景：用户的壁纸，设了就替代诗词包自带的背景（每首诗可以单独改）。
    pub wallpaper: Option<std::path::PathBuf>,
    /// 当前桌面壁纸，某首诗单独选了 auto 时用；没找到时为 None。
    pub desktop: Option<std::path::PathBuf>,
    /// 用户拖出来的诗的位置；None 时按诗词包的设置。
    pub placement: Option<Placement>,
    /// 用户指定的字色（0xRRGGBB）；None 时自动配色。
    pub text_color: Option<u32>,
    /// 每首诗的部件位置和颜色，按诗词包目录名索引。
    pub styles: Arc<Styles>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            timing: Timing::default(),
            start: 0,
            single: false,
            style: LayoutStyle::default(),
            text_scale: 1.0,
            wallpaper: None,
            desktop: None,
            placement: None,
            text_color: None,
            styles: Arc::default(),
        }
    }
}

/// 诗词包的名字（目录名），样式按它索引。
pub fn pack_name(pack: &LoadedPack) -> String {
    pack.dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Options {
    fn style(&self, pack: &LoadedPack) -> Option<&PoemStyle> {
        self.styles.get(&pack_name(pack))
    }

    /// 这首诗用哪张图做背景；None 用诗词包自带的。
    pub fn wallpaper_for(&self, pack: &LoadedPack) -> Option<std::path::PathBuf> {
        match self.style(pack).and_then(|s| s.wallpaper.as_ref()) {
            None => self.wallpaper.clone(),
            Some(Wallpaper::Pack) => None,
            Some(Wallpaper::Auto) => self.desktop.clone(),
            Some(Wallpaper::File(p)) => Some(p.clone()),
        }
    }

    /// 排版：返回默认排版，以及应用了这首诗的部件字号、位移之后的排版。
    fn pages(&self, pack: &LoadedPack, aspect: f32, index: usize) -> (Vec<Layout>, Vec<Layout>) {
        let base = scene::layout(&pack.pack, aspect, self.layout(index));
        let mut pages = base.clone();
        if let Some(st) = self.style(pack) {
            for p in &mut pages {
                p.scale_parts(&st.sizes);
                p.offset_parts(&st.offsets);
            }
        }
        (base, pages)
    }

    fn layout(&self, index: usize) -> LayoutOptions {
        LayoutOptions {
            style: self.style,
            scale: self.text_scale,
            issue: index + 1,
            placement: self.placement,
        }
    }
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: Pipelines,
    font: Arc<Font>,
    packs: Arc<[LoadedPack]>,
    options: Options,
    images: ImageCache,
    width: u32,
    height: u32,
    current: usize,
    /// 拖动中的临时偏移（像素）：拖的是哪部分（None 为整块诗主体）和位移。
    drag: (Option<Part>, f32, f32),
    /// 调整位置时鼠标所指的部件，画一个框。
    highlight: Option<Part>,
    /// 当前这首诗已经播放的秒数。
    t: f32,
    /// 总时间，驱动雾气等全局动画。
    clock: f32,
    scene: Scene,
    glyph_buf: wgpu::Buffer,
    particle_buf: wgpu::Buffer,
    image_buf: wgpu::Buffer,
    shape_buf: wgpu::Buffer,
    shape_data: Vec<ShapeInst>,
    glyph_data: Vec<GlyphInst>,
    particle_data: Vec<ParticleInst>,
}

const MAX_PARTICLES: usize = 512;
const MAX_LAYERS: usize = 16;
const MAX_SHAPES: usize = 96;

impl Renderer {
    /// `packs` 不能为空。
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        font: Arc<Font>,
        packs: Arc<[LoadedPack]>,
        options: Options,
        images: ImageCache,
    ) -> Self {
        assert!(!packs.is_empty(), "至少需要一首诗");
        let pipelines = Pipelines::new(&device, format);
        let buf = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let particle_buf = buf(
            "particles",
            MAX_PARTICLES * std::mem::size_of::<ParticleInst>(),
        );
        let image_buf = buf("layers", MAX_LAYERS * std::mem::size_of::<ImageInst>());
        let shape_buf = buf("shapes", MAX_SHAPES * std::mem::size_of::<ShapeInst>());
        let glyph_buf = buf("glyphs", 64 * std::mem::size_of::<GlyphInst>());
        let current = options.start % packs.len();
        let scene = Self::build_scene(
            &device,
            &queue,
            &pipelines,
            &images,
            &packs[current],
            current,
            &options,
            16.0 / 9.0,
        );
        prefetch_next(&images, &packs, current, &options);
        Self {
            device,
            queue,
            pipelines,
            font,
            packs,
            options,
            images,
            width: 0,
            height: 0,
            current,
            drag: (None, 0.0, 0.0),
            highlight: None,
            t: 0.0,
            clock: 0.0,
            scene,
            glyph_buf,
            particle_buf,
            image_buf,
            shape_buf,
            shape_data: Vec::new(),
            glyph_data: Vec::new(),
            particle_data: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_scene(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &Pipelines,
        images: &ImageCache,
        pack: &LoadedPack,
        index: usize,
        options: &Options,
        aspect: f32,
    ) -> Scene {
        let preset = pack.pack.particles.as_ref().map(|p| p.preset);
        let (base, pages) = options.pages(pack, aspect, index);
        let layout = pages[0].clone();
        let timeline = Timeline::new(&layout.columns, page_timing(options.timing, pages.len()));
        let seed = pack.pack.poem.title.chars().map(|c| c as u64).sum::<u64>();
        let particles = preset.map(|p| ParticleSystem::new(p, aspect, seed));
        let mut layers = Vec::new();
        let mut backdrop = None;
        // 用户壁纸加载失败时退回诗词包自带的背景。
        let wallpaper_path = options.wallpaper_for(pack);
        let wallpaper = wallpaper_path.as_ref().and_then(|w| match images.get(w) {
            Ok(img) => Some(img),
            Err(e) => {
                eprintln!("{e}");
                None
            }
        });
        let sources: Vec<_> = match wallpaper {
            Some(img) => {
                backdrop = Some(Backdrop::new(&img));
                vec![(Ok(img), 0.0, 1.0)]
            }
            None => pack
                .pack
                .layers
                .iter()
                .take(MAX_LAYERS)
                .map(|l| (images.get(&pack.dir.join(&l.image)), l.depth, 1.06))
                .collect(),
        };
        for (img, depth, scale) in sources {
            let img = match img {
                Ok(img) => img,
                Err(e) => {
                    eprintln!("{e}");
                    continue;
                }
            };
            // 超过显卡纹理上限时再缩一次，否则创建纹理会直接报错退出。
            let max = device.limits().max_texture_dimension_2d;
            let img = if img.width() > max || img.height() > max {
                Arc::new(
                    image::DynamicImage::ImageRgba8((*img).clone())
                        .resize(max, max, image::imageops::FilterType::Triangle)
                        .to_rgba8(),
                )
            } else {
                img
            };
            let (w, h) = img.dimensions();
            layers.push(Layer {
                group: upload_texture(
                    device,
                    queue,
                    &pipelines.texture_layout,
                    (w, h),
                    wgpu::TextureFormat::Rgba8UnormSrgb,
                    &img,
                ),
                aspect: w as f32 / h as f32,
                depth,
                scale,
            });
        }
        Scene {
            palette: with_text_color(palette(preset), options.text_color),
            pages,
            base,
            wallpaper: wallpaper_path,
            page: 0,
            timeline,
            layout,
            particles,
            layers,
            backdrop,
            atlas_group: None,
            glyphs: Vec::new(),
        }
    }

    /// 当前这首诗。
    pub fn current_pack(&self) -> &LoadedPack {
        &self.packs[self.current]
    }

    /// 跳到当前诗的第 `t` 秒（用于截图和调试）。
    pub fn seek(&mut self, t: f32) {
        self.t = t.max(0.0);
        self.clock = self.t;
    }

    /// 切到下一首。
    pub fn next_pack(&mut self) {
        self.set_pack(self.current + 1);
    }

    /// 切到上一首。
    pub fn prev_pack(&mut self) {
        self.set_pack(self.current + self.packs.len() - 1);
    }

    /// 切到第 `index` 首（超出时取余），从头播放。
    pub fn set_pack(&mut self, index: usize) {
        self.current = index % self.packs.len();
        self.t = 0.0;
        self.reload_scene();
    }

    /// 当前是第几首（从 0 开始）。
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// 所有诗。
    pub fn packs(&self) -> &[LoadedPack] {
        &self.packs
    }

    /// 当前的设置。
    pub fn options(&self) -> &Options {
        &self.options
    }

    /// 当前这首诗已经播放的秒数。
    pub fn time(&self) -> f32 {
        self.t
    }

    fn reload_scene(&mut self) {
        let aspect = self.aspect();
        self.scene = Self::build_scene(
            &self.device,
            &self.queue,
            &self.pipelines,
            &self.images,
            &self.packs[self.current],
            self.current,
            &self.options,
            aspect,
        );
        if self.width > 0 {
            self.rebuild_glyphs();
        }
        // 当前这首已经上传到显卡，内存里只留下一首的图，并开始预先解码。
        let o = &self.options;
        let mut keep = layer_paths(&self.packs[(self.current + 1) % self.packs.len()], o);
        keep.extend(layer_paths(&self.packs[self.current], o));
        self.images.retain(&keep);
        prefetch_next(&self.images, &self.packs, self.current, o);
    }

    fn aspect(&self) -> f32 {
        if self.height == 0 {
            16.0 / 9.0
        } else {
            self.width as f32 / self.height as f32
        }
    }

    /// 目标尺寸变化时调用（物理像素）。
    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        let aspect = self.aspect();
        if let Some(p) = &mut self.scene.particles {
            p.set_aspect(aspect);
        }
        self.relayout();
    }

    /// 重新排当前这首诗（尺寸、位置、字号变了之后）。
    fn relayout(&mut self) {
        // 分页数可能变，尽量停在原来的页上。
        (self.scene.base, self.scene.pages) =
            self.options
                .pages(&self.packs[self.current], self.aspect(), self.current);
        self.scene.page = self.scene.page.min(self.scene.pages.len() - 1);
        self.set_page(self.scene.page);
        if self.width > 0 {
            self.rebuild_glyphs();
        }
    }

    /// 换诗的位置和字号，立刻重排。
    pub fn set_arrangement(&mut self, placement: Option<Placement>, text_scale: f32) {
        self.options.placement = placement;
        self.options.text_scale = text_scale.clamp(0.3, 3.0);
        self.drag = (None, 0.0, 0.0);
        self.relayout();
    }

    /// 诗主体现在的中心位置（归一化）。
    /// 不含部件字号和位移：整首定位在这之前，这里要和它对得上，否则拖过部件后整首会跳。
    pub fn placement(&self) -> Option<Placement> {
        let (w, h) = (self.width.max(1) as f32, self.height.max(1) as f32);
        let b = self.scene.base.get(self.scene.page)?.main_bounds()?;
        Some(Placement {
            x: (b.0 + b.2) / 2.0 * h / w,
            y: (b.1 + b.3) / 2.0,
        })
    }

    /// 拖动中的临时偏移（像素），只移动画面不重排；松手后用 `set_arrangement` / `set_styles` 定下来。
    /// `part` 为 None 时拖整块诗主体。
    pub fn set_drag(&mut self, part: Option<Part>, dx: f32, dy: f32) {
        self.drag = (part, dx, dy);
    }

    /// 换成新的设置（壁纸收到 reload、设计窗口改了设置时用），停在当前这首、当前时刻。
    /// 背景或节奏变了才重建场景，否则只重排。
    pub fn reconfigure(&mut self, options: &Options) {
        let timing_changed = !same_timing(self.options.timing, options.timing);
        let start = self.options.start;
        self.options = options.clone();
        self.options.start = start;
        self.drag = (None, 0.0, 0.0);
        let wallpaper = self.options.wallpaper_for(&self.packs[self.current]);
        if timing_changed || wallpaper != self.scene.wallpaper {
            let t = self.t;
            self.reload_scene();
            self.t = t.min(self.scene.timeline.duration());
        } else {
            self.relayout();
        }
    }

    /// 换所有诗的样式（部件位置、字号、颜色、背景），立刻重排。
    pub fn set_styles(&mut self, styles: Arc<Styles>) {
        let mut o = self.options.clone();
        o.styles = styles;
        self.reconfigure(&o);
    }

    /// 换字体。
    pub fn set_font(&mut self, font: Arc<Font>) {
        self.font = font;
        if self.width > 0 {
            self.rebuild_glyphs();
        }
    }

    /// 当前页每个部件的外接框（像素，x0 y0 x1 y1），含背景大字。
    pub fn part_bounds(&self) -> Vec<(Part, [f32; 4])> {
        let h = self.height.max(1) as f32;
        let l = &self.scene.layout;
        l.parts()
            .into_iter()
            .filter_map(|p| {
                let b = l.part_bounds(p)?;
                Some((p, [b.0 * h, b.1 * h, b.2 * h, b.3 * h]))
            })
            .collect()
    }

    /// 当前页每个字：部件、部件里第几个字、字、字格（像素）。
    pub fn chars(&self) -> Vec<(Part, usize, char, [f32; 4])> {
        let h = self.height.max(1) as f32;
        let mut n: std::collections::HashMap<Part, usize> = Default::default();
        self.scene
            .layout
            .chars
            .iter()
            .map(|c| {
                let i = n.entry(c.part).or_default();
                let k = *i;
                *i += 1;
                let r = c.size / 2.0;
                (
                    c.part,
                    k,
                    c.ch,
                    [(c.x - r) * h, (c.y - r) * h, (c.x + r) * h, (c.y + r) * h],
                )
            })
            .collect()
    }

    /// 诗主体的外接框（像素）。
    pub fn main_bounds(&self) -> Option<[f32; 4]> {
        let h = self.height.max(1) as f32;
        let b = self.scene.layout.main_bounds()?;
        Some([b.0 * h, b.1 * h, b.2 * h, b.3 * h])
    }

    /// 自动配色的依据：诗所在区域的亮度（0..1，线性）和按它选出的字色。
    pub fn auto_color(&self) -> AutoColor {
        let preset = self.scene.particles.as_ref().map(|p| p.preset);
        match self.backdrop_luma() {
            Some(luma) => {
                let light = prefers_dark_ink(luma);
                AutoColor {
                    luma,
                    light,
                    ink: wallpaper_palette(light, preset).ink,
                    on_wallpaper: true,
                }
            }
            None => {
                let pal = palette(preset);
                let luma = (luminance(pal.bg_top) + luminance(pal.bg_bottom)) / 2.0;
                AutoColor {
                    luma,
                    light: luma >= 0.18,
                    ink: pal.ink,
                    on_wallpaper: false,
                }
            }
        }
    }

    /// 长诗分页：(当前第几页, 共几页)，从 0 开始。
    pub fn page(&self) -> (usize, usize) {
        (self.scene.page, self.scene.pages.len())
    }

    /// 屏幕上这一点（像素）落在哪个部件上。
    pub fn part_at(&self, x: f32, y: f32) -> Option<Part> {
        let h = self.height.max(1) as f32;
        self.scene.layout.part_at(x / h, y / h, 0.006)
    }

    /// 给这个部件画一个框（None 不画）。
    pub fn set_highlight(&mut self, part: Option<Part>) {
        self.highlight = part;
    }

    /// 屏幕高度（像素），用来把拖动距离换成排版单位。
    pub fn height(&self) -> u32 {
        self.height
    }

    /// 这一位移（像素）作用在哪些东西上。
    fn drag_for(&self, part: Part, role: Role) -> (f32, f32) {
        let (target, dx, dy) = self.drag;
        let hit = match target {
            Some(p) => p == part,
            None => is_main(role),
        };
        if hit { (dx, dy) } else { (0.0, 0.0) }
    }

    /// 停在全诗刚好显示完的时刻（调整位置时用）。
    pub fn reveal_all(&mut self) {
        self.t = self.scene.timeline.revealed_at();
    }

    /// 按当前屏幕尺寸重新光栅化字形。
    fn rebuild_glyphs(&mut self) {
        self.fit_backdrop();
        let h = self.height as f32;
        let max_dim = self.device.limits().max_texture_dimension_2d;
        // 单个字形不超过纹理上限的三分之一（背景大字在超高分辨率下会很大）。
        let px_of = |size: f32| ((size * h).round() as u32).clamp(4, max_dim / 3);
        let pal = self.scene.palette;
        let colors = char_colors(
            &self.scene.layout,
            &pal,
            self.options.style(&self.packs[self.current]),
        );
        let wanted: Vec<_> = self
            .scene
            .layout
            .chars
            .iter()
            .map(|c| (c.ch, px_of(c.size), role_style(c.role, &pal).0))
            .collect();
        let atlas = Atlas::build(&self.font, &wanted, max_dim);
        let (aw, ah) = (atlas.width as f32, atlas.height as f32);
        self.scene.atlas_group = Some(upload_texture(
            &self.device,
            &self.queue,
            &self.pipelines.texture_layout,
            (atlas.width, atlas.height),
            wgpu::TextureFormat::R8Unorm,
            &atlas.pixels,
        ));
        self.scene.glyphs = self
            .scene
            .layout
            .chars
            .iter()
            .zip(colors)
            .map(|(c, color)| {
                let px = px_of(c.size);
                let weight = role_style(c.role, &pal).0;
                let slot = atlas.slots.get(&(c.ch, px, weight))?;
                let pad = *atlas.padding.get(&px)? as f32;
                Some(GlyphInst {
                    center: [c.x * h + slot.offset.0, c.y * h + slot.offset.1],
                    half_size: [slot.w as f32 / 2.0, slot.h as f32 / 2.0],
                    uv0: [slot.x as f32 / aw, slot.y as f32 / ah],
                    uv1: [(slot.x + slot.w) as f32 / aw, (slot.y + slot.h) as f32 / ah],
                    color,
                    params: [0.0, pad * 0.8],
                })
            })
            .collect();
        let needed = self.scene.glyphs.len().max(1) * std::mem::size_of::<GlyphInst>();
        if self.glyph_buf.size() < needed as u64 {
            self.glyph_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("glyphs"),
                size: needed.next_power_of_two() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
    }

    /// 选字色：画在用户壁纸上时按当前排版下诗所在区域的明暗选，否则用诗词包的配色。
    fn fit_backdrop(&mut self) {
        let preset = self.scene.particles.as_ref().map(|p| p.preset);
        self.scene.palette = match self.backdrop_luma() {
            Some(mean) => wallpaper_palette(prefers_dark_ink(mean), preset),
            None if self.scene.backdrop.is_some() => wallpaper_palette(true, preset),
            None => palette(preset),
        };
        self.scene.palette = with_text_color(self.scene.palette, self.options.text_color);
    }

    /// 用户壁纸上诗主体所在区域的平均亮度；不是画在用户壁纸上、或这一页没有主体时为 None。
    fn backdrop_luma(&self) -> Option<f32> {
        let bd = self.scene.backdrop.as_ref()?;
        let (w, h) = (self.width as f32, self.height as f32);
        // 刊头、期号这些角落小字和背景大字不算，只看诗的主体。
        let mut rect = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for c in &self.scene.layout.chars {
            if matches!(c.role, Role::Caption | Role::Watermark) {
                continue;
            }
            let r = c.size / 2.0;
            rect = [
                rect[0].min((c.x - r) * h),
                rect[1].min((c.y - r) * h),
                rect[2].max((c.x + r) * h),
                rect[3].max((c.y + r) * h),
            ];
        }
        (rect[0] <= rect[2]).then(|| bd.mean(rect, (w, h)))
    }

    /// 推进 `dt` 秒，到时间就换下一首。
    pub fn advance(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 1.0);
        self.t += dt;
        self.clock += dt;
        if let Some(p) = &mut self.scene.particles {
            p.update(dt);
        }
        if self.t >= self.scene.timeline.duration() {
            self.t = 0.0;
            if self.scene.page + 1 < self.scene.pages.len() {
                self.set_page(self.scene.page + 1);
                self.rebuild_glyphs();
            } else if self.options.single || self.packs.len() == 1 {
                if self.scene.pages.len() > 1 {
                    self.set_page(0);
                    self.rebuild_glyphs();
                }
            } else {
                self.next_pack();
            }
        }
    }

    /// 切到第 `page` 页的排版和时间线（不重建字形，调用方按需重建）。
    fn set_page(&mut self, page: usize) {
        let s = &mut self.scene;
        s.page = page;
        s.layout = s.pages[page].clone();
        s.timeline = Timeline::new(
            &s.layout.columns,
            page_timing(self.options.timing, s.pages.len()),
        );
    }

    /// 是否还有动画在进行。全诗显示完、又没有粒子时可以降低帧率。
    pub fn is_animating(&self) -> bool {
        if self.scene.particles.is_some() {
            return true;
        }
        let tl = &self.scene.timeline;
        let n = self.scene.layout.chars.len();
        let last_done = n == 0 || tl.char_progress(n - 1, self.t) >= 1.0;
        !(last_done && tl.fade_out_factor(self.t) >= 1.0)
    }

    /// 画一帧到 `target`。调用前至少 `resize` 过一次。
    pub fn render(&mut self, target: &wgpu::TextureView) {
        let (w, h) = (self.width as f32, self.height as f32);
        let pal = self.scene.palette;
        let kind = match self.scene.particles.as_ref().map(|p| p.preset) {
            Some(ParticlePreset::Petals) | None => 0,
            Some(ParticlePreset::Snow) => 1,
            Some(ParticlePreset::Rain) => 2,
            Some(ParticlePreset::Fireflies) => 3,
            Some(ParticlePreset::MoonlightDust) => 4,
        };
        let globals = Globals {
            screen: [w, h],
            time: self.clock,
            particle_kind: kind,
            bg_top: linear(pal.bg_top, 1.0),
            bg_bottom: linear(pal.bg_bottom, 1.0),
            mist: linear(pal.mist.0, pal.mist.1),
            particle: linear(pal.particle.0, pal.particle.1),
        };
        self.queue
            .write_buffer(&self.pipelines.globals, 0, bytemuck::bytes_of(&globals));

        // 图层：铺满屏幕（cover），按深度缓慢左右漂移形成视差。
        let images: Vec<ImageInst> = self
            .scene
            .layers
            .iter()
            .map(|l| {
                let drift = (self.clock * 0.05).sin() * l.depth * w * 0.02;
                ImageInst {
                    rect: cover_rect(l.aspect, l.scale, (w, h), drift),
                    alpha: 1.0,
                }
            })
            .collect();
        if !images.is_empty() {
            self.queue
                .write_buffer(&self.image_buf, 0, bytemuck::cast_slice(&images));
        }

        self.glyph_data.clear();
        for (i, g) in self.scene.glyphs.iter().enumerate() {
            let Some(g) = g else { continue };
            let p = self.scene.timeline.char_progress(i, self.t);
            if p <= 0.0 {
                continue;
            }
            let mut g = *g;
            g.params[0] = p;
            let c = &self.scene.layout.chars[i];
            let (dx, dy) = self.drag_for(c.part, c.role);
            g.center[0] += dx;
            g.center[1] += dy;
            self.glyph_data.push(g);
        }
        if !self.glyph_data.is_empty() {
            self.queue
                .write_buffer(&self.glyph_buf, 0, bytemuck::cast_slice(&self.glyph_data));
        }

        // 细线从一端生长出来；印章随印章字一起淡入。
        self.shape_data.clear();
        for sh in self.scene.layout.shapes.iter().take(MAX_SHAPES) {
            let p = self.scene.timeline.char_progress(sh.start_char, self.t);
            if p <= 0.0 {
                continue;
            }
            let ease = 1.0 - (1.0 - p).powi(3);
            let role = self
                .scene
                .layout
                .chars
                .get(sh.start_char)
                .map_or(Role::Caption, |c| c.role);
            let (dx, dy) = self.drag_for(sh.part, role);
            let style = self.options.style(&self.packs[self.current]);
            let paint = style.and_then(|st| st.parts.get(&sh.part));
            let opacity = style
                .and_then(|st| st.opacity.get(&sh.part))
                .copied()
                .unwrap_or(1.0);
            let full = [
                sh.x0 * h + dx,
                sh.y0 * h + dy,
                sh.x1 * h + dx,
                sh.y1 * h + dy,
            ];
            let vertical = full[2] - full[0] < full[3] - full[1];
            let mut drawn = full;
            let (default, alpha, kind) = match sh.kind {
                ShapeKind::Rule => {
                    // 至少一个物理像素宽，从一端生长出来。
                    if vertical {
                        drawn[2] = drawn[2].max(drawn[0] + 1.0);
                        drawn[3] = drawn[1] + (drawn[3] - drawn[1]) * ease;
                    } else {
                        drawn[3] = drawn[3].max(drawn[1] + 1.0);
                        drawn[2] = drawn[0] + (drawn[2] - drawn[0]) * ease;
                    }
                    let fade = self.scene.timeline.fade_out_factor(self.t);
                    (pal.accent, 0.55 * fade, 0.0)
                }
                ShapeKind::Seal => (pal.seal, 0.9 * p, 1.0),
            };
            let solid = [Color::opaque(default)];
            let stops = paint.map_or(&solid[..], Paint::stops);
            let color = |c: Color| linear(c.rgb, alpha * c.alpha() * opacity);
            // 印章的渐变从上到下；细线顺着它的方向。多段渐变拆成几段，每段两色。
            let axis = if vertical || sh.kind == ShapeKind::Seal {
                1
            } else {
                0
            };
            let (lo, hi) = (full[axis], full[axis + 2]);
            let n = stops.len().max(2) - 1;
            for k in 0..n {
                let (a, b) = (
                    stops[k.min(stops.len() - 1)],
                    stops[(k + 1).min(stops.len() - 1)],
                );
                let (s0, s1) = (
                    lo + (hi - lo) * k as f32 / n as f32,
                    lo + (hi - lo) * (k + 1) as f32 / n as f32,
                );
                let mut rect = drawn;
                rect[axis] = rect[axis].max(s0);
                rect[axis + 2] = rect[axis + 2].min(s1);
                if rect[axis] >= rect[axis + 2] || self.shape_data.len() >= MAX_SHAPES {
                    continue;
                }
                self.shape_data.push(ShapeInst {
                    rect,
                    color: color(a),
                    color2: color(b),
                    grad: [axis as f32, s0, s1],
                    kind,
                });
            }
        }
        // 调整位置时鼠标所指部件的框。
        if let Some(b) = self
            .highlight
            .and_then(|p| self.scene.layout.part_bounds(p))
        {
            let (dx, dy) = match self.drag {
                (Some(p), dx, dy) if Some(p) == self.highlight => (dx, dy),
                _ => (0.0, 0.0),
            };
            let pad = 0.008 * h;
            let (x0, y0, x1, y1) = (
                b.0 * h - pad + dx,
                b.1 * h - pad + dy,
                b.2 * h + pad + dx,
                b.3 * h + pad + dy,
            );
            let color = linear(pal.accent, 0.8);
            let t = 1.5;
            for rect in [
                [x0, y0, x1, y0 + t],
                [x0, y1 - t, x1, y1],
                [x0, y0, x0 + t, y1],
                [x1 - t, y0, x1, y1],
            ] {
                if self.shape_data.len() < MAX_SHAPES {
                    self.shape_data.push(ShapeInst {
                        rect,
                        color,
                        color2: color,
                        grad: [0.0, 0.0, 1.0],
                        kind: 0.0,
                    });
                }
            }
        }
        if !self.shape_data.is_empty() {
            self.queue
                .write_buffer(&self.shape_buf, 0, bytemuck::cast_slice(&self.shape_data));
        }

        self.particle_data.clear();
        if let Some(sys) = &self.scene.particles {
            for p in sys.particles.iter().take(MAX_PARTICLES) {
                let s = p.size * h;
                let (half, rot) = match sys.preset {
                    ParticlePreset::Rain => ([s * 0.035, s], -p.vx.atan2(p.vy)),
                    ParticlePreset::Petals => ([s, s], p.rotation),
                    ParticlePreset::Fireflies => ([s * 3.0, s * 3.0], 0.0),
                    _ => ([s, s], 0.0),
                };
                // 花瓣翻转：沿一个轴压扁，看起来像在空中翻动。
                let half = if sys.preset == ParticlePreset::Petals {
                    [
                        half[0] * (0.35 + 0.65 * (p.rotation * 1.7 + p.phase).cos().abs()),
                        half[1],
                    ]
                } else {
                    half
                };
                self.particle_data.push(ParticleInst {
                    center: [p.x * h, p.y * h],
                    half_size: [half[0].max(0.75), half[1].max(0.75)],
                    params: [rot, p.alpha],
                });
            }
        }
        if !self.particle_data.is_empty() {
            self.queue.write_buffer(
                &self.particle_buf,
                0,
                bytemuck::cast_slice(&self.particle_data),
            );
        }

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("muse"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.pipelines.globals_group, &[]);

            pass.set_pipeline(&self.pipelines.bg);
            pass.draw(0..3, 0..1);

            if !images.is_empty() {
                pass.set_pipeline(&self.pipelines.image);
                pass.set_vertex_buffer(0, self.image_buf.slice(..));
                for (i, l) in self.scene.layers.iter().enumerate() {
                    pass.set_bind_group(1, &l.group, &[]);
                    let i = i as u32;
                    pass.draw(0..6, i..i + 1);
                }
            }

            // 粒子在文字后面，免得遮住字。
            if !self.particle_data.is_empty() {
                pass.set_pipeline(&self.pipelines.particle);
                pass.set_vertex_buffer(0, self.particle_buf.slice(..));
                pass.draw(0..6, 0..self.particle_data.len() as u32);
            }

            if !self.shape_data.is_empty() {
                pass.set_pipeline(&self.pipelines.shape);
                pass.set_vertex_buffer(0, self.shape_buf.slice(..));
                pass.draw(0..6, 0..self.shape_data.len() as u32);
            }

            if let (Some(atlas), false) = (&self.scene.atlas_group, self.glyph_data.is_empty()) {
                pass.set_pipeline(&self.pipelines.glyph);
                pass.set_bind_group(1, atlas, &[]);
                pass.set_vertex_buffer(0, self.glyph_buf.slice(..));
                pass.draw(0..6, 0..self.glyph_data.len() as u32);
            }
        }
        self.queue.submit([encoder.finish()]);
    }

    /// 离屏画一帧并读回 RGBA8（sRGB）像素，用于截图。
    /// 渲染器必须以 `Rgba8UnormSrgb` 格式创建。
    pub fn capture(&mut self, width: u32, height: u32) -> Vec<u8> {
        self.resize(width, height);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.render(&texture.create_view(&Default::default()));

        let row = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        buffer.map_async(wgpu::MapMode::Read, .., |r| r.expect("读回截图失败"));
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("等待 GPU 失败");
        let mapped = buffer.get_mapped_range(..).expect("映射截图缓冲失败");
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let start = (y * row) as usize;
            out.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
        }
        out
    }
}
