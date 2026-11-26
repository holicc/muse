//! 字体查找与字形光栅化。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont as _, VariableFont as _};

/// 按顺序尝试的系统字体：先宋体类（衬线），没有再退到黑体类。
const PREFERRED_FAMILIES: &[&str] = &[
    "Noto Serif CJK SC",
    "Source Han Serif SC",
    "Source Han Serif CN",
    "Songti SC",
    "STSong",
    "SimSun",
    "Noto Serif SC",
    "Noto Sans CJK SC",
    "Source Han Sans SC",
    "PingFang SC",
    "Microsoft YaHei",
    "WenQuanYi Zen Hei",
];

/// 用来判断一个字体能不能显示汉字的探测字。
const PROBE: char = '诗';

/// 字重。可变字体直接调 wght 轴；非可变字体按字重找同一家族的其他字体文件，找不到就用同一个。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Light,
    Regular,
    Heavy,
}

impl Weight {
    const ALL: [Weight; 3] = [Weight::Light, Weight::Regular, Weight::Heavy];

    fn wght(self) -> f32 {
        match self {
            Weight::Light => 300.0,
            Weight::Regular => 500.0,
            Weight::Heavy => 800.0,
        }
    }

    fn css(self) -> fontdb::Weight {
        match self {
            Weight::Light => fontdb::Weight::LIGHT,
            Weight::Regular => fontdb::Weight::NORMAL,
            Weight::Heavy => fontdb::Weight::BOLD,
        }
    }
}

pub struct Font {
    /// 按 Weight::ALL 的顺序。字体数据在程序运行期间一直需要，加载后不释放。
    faces: [FontRef<'static>; 3],
    /// 人可读的来源，用于日志。
    pub description: String,
    /// 每个字重来自哪个字体文件、文件里第几个字体（细、常规、粗）。设计窗口按它加载同样的字体。
    pub sources: FaceSources,
}

impl Font {
    /// `spec` 可以是字体文件路径（.ttc 取第一个字体），也可以是系统字体名；不指定时自动找中文宋体。
    pub fn load(spec: Option<&str>) -> Result<Self, String> {
        let Some(spec) = spec else {
            return Self::find_system();
        };
        let path = Path::new(spec);
        if path.is_file() {
            let data = std::fs::read(path)
                .map_err(|e| format!("{}: 读取字体失败: {e}", path.display()))?;
            let mut f = Self::from_bytes(data, 0, spec)?;
            let src = Some((path.to_path_buf(), 0));
            f.sources = [src.clone(), src.clone(), src];
            return Ok(f);
        }
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self::load_family(&db, spec)?.ok_or_else(|| {
            format!("找不到字体「{spec}」：既不是存在的文件，也不是系统里的字体名（可以用 fc-list 查看字体名）")
        })
    }

    /// 从字体文件的内容加载，`index` 是文件里第几个字体，给读不了磁盘的环境（网页）用。
    pub fn from_bytes(data: Vec<u8>, index: u32, description: &str) -> Result<Self, String> {
        let face = leak_face(data, index, description)?;
        Ok(Self::from_faces(
            [face.clone(), face.clone(), face],
            description.to_string(),
        ))
    }

    /// 三个字重分别取自 `files` 里的哪个文件、第几个字体（同一个文件只传一次，省内存）。
    pub fn from_files(
        files: Vec<Vec<u8>>,
        faces: [(usize, u32); 3],
        description: &str,
    ) -> Result<Self, String> {
        let datas: Vec<&'static [u8]> = files
            .into_iter()
            .map(|d| &*Box::leak(d.into_boxed_slice()))
            .collect();
        let load = |(file, index): (usize, u32)| -> Result<FontRef<'static>, String> {
            let data = datas
                .get(file)
                .ok_or_else(|| format!("{description}: 字体文件序号 {file} 超出范围"))?;
            let f = FontRef::try_from_slice_and_index(data, index)
                .map_err(|e| format!("{description}: 无法解析字体: {e}"))?;
            if f.glyph_id(PROBE).0 == 0 {
                return Err(format!("{description}: 字体不含汉字"));
            }
            Ok(f)
        };
        Ok(Self::from_faces(
            [load(faces[0])?, load(faces[1])?, load(faces[2])?],
            description.to_string(),
        ))
    }

    fn face(&self, w: Weight) -> &FontRef<'static> {
        &self.faces[w as usize]
    }

    /// 每个字重一份实例；可变字体在这里设置 wght。
    fn from_faces(faces: [FontRef<'static>; 3], description: String) -> Self {
        let mut faces = faces;
        for (f, w) in faces.iter_mut().zip(Weight::ALL) {
            f.set_variation(b"wght", w.wght());
        }
        Self {
            faces,
            description,
            sources: [None, None, None],
        }
    }

    /// 按家族名加载三种字重。家族不存在返回 Ok(None)。
    fn load_family(db: &fontdb::Database, name: &str) -> Result<Option<Self>, String> {
        let query = |w: Weight| {
            db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(name)],
                weight: w.css(),
                ..Default::default()
            })
        };
        let Some(regular) = query(Weight::Regular) else {
            return Ok(None);
        };
        // 同一个字体（可变字体常见）只加载一次。
        let mut loaded: Vec<(fontdb::ID, FontRef<'static>)> = Vec::new();
        let mut get = |id: fontdb::ID| -> Result<FontRef<'static>, String> {
            if let Some((_, f)) = loaded.iter().find(|(i, _)| *i == id) {
                return Ok(f.clone());
            }
            let f = db
                .with_face_data(id, |data, index| leak_face(data.to_vec(), index, name))
                .ok_or_else(|| format!("{name}: 读取字体失败"))??;
            loaded.push((id, f.clone()));
            Ok(f)
        };
        let source = |id: fontdb::ID| match db.face_source(id) {
            Some((fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _), i)) => Some((p, i)),
            _ => None,
        };
        let regular_face = get(regular)?;
        let mut faces = [regular_face.clone(), regular_face.clone(), regular_face];
        let mut sources = [source(regular), source(regular), source(regular)];
        for w in [Weight::Light, Weight::Heavy] {
            if let Some(id) = query(w) {
                faces[w as usize] = get(id)?;
                sources[w as usize] = source(id);
            }
        }
        let mut f = Self::from_faces(faces, name.to_string());
        f.sources = sources;
        Ok(Some(f))
    }

    /// 画一个方形印章图标（托盘用）：圆角底色上居中一个字。返回 RGBA8，边长 `size`。
    pub fn seal_icon(&self, ch: char, size: u32, bg: [u8; 3], fg: [u8; 3]) -> Vec<u8> {
        let n = size as usize;
        let mut cov = vec![0f32; n * n];
        let f = self.face(Weight::Heavy);
        let px = size as f32 * 0.78;
        let id = f.glyph_id(ch);
        let glyph = id.with_scale_and_position(em_scale(f, px), ab_glyph::point(0.0, 0.0));
        if let Some(o) = f.outline_glyph(glyph) {
            let b = o.px_bounds();
            let ox = (size as f32 - b.width()) / 2.0;
            let oy = (size as f32 - b.height()) / 2.0;
            o.draw(|x, y, c| {
                let (x, y) = ((x as f32 + ox) as usize, (y as f32 + oy) as usize);
                if x < n && y < n {
                    cov[y * n + x] = c.clamp(0.0, 1.0);
                }
            });
        }
        let r = size as f32 * 0.18;
        let mut out = Vec::with_capacity(n * n * 4);
        for y in 0..n {
            for x in 0..n {
                // 圆角矩形的覆盖率，边缘一像素抗锯齿。
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let dx = (r - cx).max(cx - (size as f32 - r)).max(0.0);
                let dy = (r - cy).max(cy - (size as f32 - r)).max(0.0);
                let a = (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
                let c = cov[y * n + x];
                for k in 0..3 {
                    out.push((bg[k] as f32 * (1.0 - c) + fg[k] as f32 * c) as u8);
                }
                out.push((a * 255.0) as u8);
            }
        }
        out
    }

    fn find_system() -> Result<Self, String> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        for name in PREFERRED_FAMILIES {
            if let Ok(Some(f)) = Self::load_family(&db, name) {
                return Ok(f);
            }
        }
        // 首选列表都没有时，退而求其次找任何带简体中文名的字体。
        let fallback: Vec<String> = db
            .faces()
            .filter(|f| {
                f.families
                    .iter()
                    .any(|(_, lang)| *lang == fontdb::Language::Chinese_PeoplesRepublicOfChina)
            })
            .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
            .collect();
        for name in fallback {
            if let Ok(Some(f)) = Self::load_family(&db, &name) {
                return Ok(f);
            }
        }
        Err("系统里找不到可用的中文字体，请用 --font 指定字体文件或字体名".into())
    }
}

/// 细、常规、粗三个字重各来自哪个字体文件、文件里第几个字体。
pub type FaceSources = [Option<(PathBuf, u32)>; 3];

/// 系统里能显示中文的字体家族（设计窗口的字体列表）。按名字排序，首选的宋体排在前面。
pub fn cjk_families() -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let chinese = |l: fontdb::Language| {
        matches!(
            l,
            fontdb::Language::Chinese_PeoplesRepublicOfChina
                | fontdb::Language::Chinese_Taiwan
                | fontdb::Language::Chinese_HongKongSAR
                | fontdb::Language::Chinese_Singapore
                | fontdb::Language::Chinese_MacaoSAR
        )
    };
    let mut names: Vec<String> = db
        .faces()
        .filter(|f| {
            f.families.iter().any(|(_, l)| chinese(*l))
                || f.families.iter().any(|(n, _)| {
                    ["CJK", "Han ", "Song", "Kai", "Hei", "WenKai", "Ming"]
                        .iter()
                        .any(|k| n.contains(k))
                })
        })
        // 第一个名字通常是英文名，和 fc-list、配置文件里的写法一致。
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .collect();
    names.sort();
    names.dedup();
    let rank = |n: &String| {
        PREFERRED_FAMILIES
            .iter()
            .position(|p| p == n)
            .unwrap_or(usize::MAX)
    };
    names.sort_by_key(rank);
    names
}

/// 找到字体对应的文件（不加载字体数据），返回描述和细、常规、粗三个字重的来源。
/// `spec` 为 None 时按自动选择的顺序找。
pub fn locate(spec: Option<&str>) -> Result<(String, FaceSources), String> {
    if let Some(s) = spec {
        let p = Path::new(s);
        if p.is_file() {
            let src = Some((p.to_path_buf(), 0));
            return Ok((s.to_string(), [src.clone(), src.clone(), src]));
        }
    }
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let find = |name: &str| {
        let query = |w: Weight| {
            db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(name)],
                weight: w.css(),
                ..Default::default()
            })
        };
        let source = |id: fontdb::ID| match db.face_source(id) {
            Some((fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _), i)) => Some((p, i)),
            _ => None,
        };
        let regular = query(Weight::Regular)?;
        let r = source(regular);
        let pick = |w: Weight| query(w).and_then(source).or_else(|| r.clone());
        Some([pick(Weight::Light), r.clone(), pick(Weight::Heavy)])
    };
    match spec {
        Some(name) => find(name)
            .map(|s| (name.to_string(), s))
            .ok_or_else(|| format!("找不到字体「{name}」")),
        None => PREFERRED_FAMILIES
            .iter()
            .find_map(|n| find(n).map(|s| (n.to_string(), s)))
            .ok_or_else(|| "系统里找不到可用的中文字体".to_string()),
    }
}

/// 让一个字身（em）正好是 `px` 像素。ab_glyph 的 PxScale 指的是上升加下降的总高度，
/// 中文字体这个值约为 1.4 em，直接用会让字比格子小三成。
fn em_scale(f: &FontRef<'_>, px: f32) -> PxScale {
    let upem = f.units_per_em().unwrap_or(1000.0);
    PxScale::from(px * f.height_unscaled() / upem)
}

/// 解析字体并让数据常驻（字体在程序整个运行期间都要用，多个字重实例共用一份数据）。
fn leak_face(data: Vec<u8>, index: u32, description: &str) -> Result<FontRef<'static>, String> {
    // 先检查，通过了才让数据常驻，免得探测失败的字体白占内存。
    let probe = FontRef::try_from_slice_and_index(&data, index)
        .map_err(|e| format!("{description}: 无法解析字体: {e}"))?;
    if probe.glyph_id(PROBE).0 == 0 {
        return Err(format!("{description}: 字体不含汉字"));
    }
    let data: &'static [u8] = Box::leak(data.into_boxed_slice());
    FontRef::try_from_slice_and_index(data, index)
        .map_err(|e| format!("{description}: 无法解析字体: {e}"))
}

/// 字形在图集里的位置，以及相对字格中心的偏移。
#[derive(Debug, Clone, Copy)]
pub struct GlyphSlot {
    /// 图集里的像素矩形（含留白）。
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// 位图中心相对字格中心的偏移，像素。
    pub offset: (f32, f32),
}

/// 图集里一个字形：字、像素字号、字重。
pub type GlyphKey = (char, u32, Weight);

/// 一张单通道字形图集，留白足够让晕染模糊不被裁掉。
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub slots: HashMap<GlyphKey, GlyphSlot>,
    /// 每个字形四周的留白，像素。
    pub padding: HashMap<u32, u32>,
}

impl Atlas {
    /// 光栅化一组字形。重复项和字体里没有的字会被跳过。
    /// `max_dim` 是显卡支持的最大纹理边长，放不下的字形会被跳过（不画），而不是报错退出。
    pub fn build(font: &Font, wanted: &[GlyphKey], max_dim: u32) -> Self {
        struct Raster {
            key: GlyphKey,
            w: u32,
            h: u32,
            pad: u32,
            data: Vec<u8>,
            offset: (f32, f32),
        }
        let mut seen = std::collections::HashSet::new();
        let mut rasters = Vec::new();
        for &key in wanted {
            let (ch, px, weight) = key;
            if !seen.insert(key) {
                continue;
            }
            let f = font.face(weight);
            let id = f.glyph_id(ch);
            if id.0 == 0 {
                continue;
            }
            let scale = em_scale(f, px as f32);
            let scaled = f.as_scaled(scale);
            let glyph = id.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
            let Some(outline) = f.outline_glyph(glyph) else {
                continue;
            };
            let b = outline.px_bounds();
            // 晕染模糊需要的留白；背景大字不需要按比例留那么多。
            let pad = (px as f32 * 0.22).min(48.0).ceil() as u32 + 2;
            let gw = b.width().ceil() as u32;
            let gh = b.height().ceil() as u32;
            let (w, h) = (gw + pad * 2, gh + pad * 2);
            let mut data = vec![0u8; (w * h) as usize];
            outline.draw(|x, y, c| {
                let i = ((y + pad) * w + x + pad) as usize;
                if let Some(d) = data.get_mut(i) {
                    *d = (c.clamp(0.0, 1.0) * 255.0) as u8;
                }
            });
            // 汉字的字面框中心约在基线上方 0.38em、步进宽度的一半处。
            let cell_cx = scaled.h_advance(id) / 2.0;
            let cell_cy = -0.38 * px as f32;
            let bmp_cx = b.min.x + b.width() / 2.0;
            let bmp_cy = b.min.y + b.height() / 2.0;
            rasters.push(Raster {
                key,
                w,
                h,
                pad,
                data,
                offset: (bmp_cx - cell_cx, bmp_cy - cell_cy),
            });
        }

        // 按高度排序后逐行摆放。
        rasters.sort_by_key(|r| std::cmp::Reverse(r.h));
        let total: u32 = rasters.iter().map(|r| r.w * r.h).sum();
        let width = ((total as f32).sqrt() * 1.15).ceil().max(64.0) as u32;
        let max_dim = max_dim.clamp(64, 8192);
        rasters.retain(|r| r.w <= max_dim && r.h <= max_dim);
        let width = width
            .max(rasters.iter().map(|r| r.w).max().unwrap_or(1))
            .min(max_dim);
        let mut placed = Vec::with_capacity(rasters.len());
        let (mut x, mut y, mut row_h) = (0u32, 0u32, 0u32);
        for r in &rasters {
            if x + r.w > width {
                x = 0;
                y += row_h;
                row_h = 0;
            }
            placed.push((x, y));
            x += r.w;
            row_h = row_h.max(r.h);
        }
        // 超出高度上限的字形丢掉。
        let fits: Vec<bool> = rasters
            .iter()
            .zip(&placed)
            .map(|(r, &(_, py))| py + r.h <= max_dim)
            .collect();
        if fits.contains(&false) {
            eprintln!("字形图集超出显卡纹理上限，部分字不显示");
        }
        let mut keep = fits.iter();
        rasters.retain(|_| *keep.next().unwrap_or(&false));
        let mut keep = fits.iter();
        placed.retain(|_| *keep.next().unwrap_or(&false));
        let height = rasters
            .iter()
            .zip(&placed)
            .map(|(r, &(_, py))| py + r.h)
            .max()
            .unwrap_or(1)
            .max(1);

        let mut pixels = vec![0u8; (width * height) as usize];
        let mut slots = HashMap::new();
        let mut padding = HashMap::new();
        for (r, &(px, py)) in rasters.iter().zip(&placed) {
            for row in 0..r.h {
                let src = &r.data[(row * r.w) as usize..((row + 1) * r.w) as usize];
                let dst = ((py + row) * width + px) as usize;
                pixels[dst..dst + r.w as usize].copy_from_slice(src);
            }
            slots.insert(
                r.key,
                GlyphSlot {
                    x: px,
                    y: py,
                    w: r.w,
                    h: r.h,
                    offset: r.offset,
                },
            );
            padding.insert(r.key.1, r.pad);
        }
        Self {
            width,
            height,
            pixels,
            slots,
            padding,
        }
    }
}
