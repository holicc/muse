//! 文字排版。坐标、字号都以屏幕高度为 1 的归一化单位，原点左上。
//!
//! 两种版式：
//! - `classic`：传统的居中竖排（或横排），标题、署名小字。
//! - `magazine`：杂志风。大号粗体标题加细线分栏，正文错落留白，署名配朱红印章，
//!   角落有刊头、期号和出处小字，背景一个极淡的超大字作装饰。

use crate::{LayoutStyle, Orientation, Pack, TextPosition};

/// 字在版面里的角色，决定颜色、字重和透明度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Title,
    Body,
    /// 署名、传统版式的标题。
    Byline,
    /// 刊头、期号、出处等小字。
    Caption,
    /// 印章里的字。
    Seal,
    /// 背景装饰大字。
    Watermark,
}

/// 版面上可以单独拖动、单独配色的部件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    /// 标题（折成几列时一起动）。
    Title,
    /// 正文第几列（横排时第几行），从 0 开始，按整首诗算，分页后不重新编号。
    Line(usize),
    /// 署名。
    Byline,
    /// 印章（底色和印章里的字）。
    Seal,
    /// 标题和正文之间的分栏细线。
    Rule,
    /// 刊头「MUSE」。
    Masthead,
    /// 期号。
    Issue,
    /// 底部的署名、出处小字和上面的细线。
    Source,
    /// 背景大字。
    Watermark,
}

impl Part {
    /// 设置文件里的名字：`title`、`line.1`（从 1 开始）、`byline` 等。
    pub fn key(self) -> String {
        match self {
            Self::Title => "title".into(),
            Self::Line(i) => format!("line.{}", i + 1),
            Self::Byline => "byline".into(),
            Self::Seal => "seal".into(),
            Self::Rule => "rule".into(),
            Self::Masthead => "masthead".into(),
            Self::Issue => "issue".into(),
            Self::Source => "source".into(),
            Self::Watermark => "watermark".into(),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Some(match key {
            "title" => Self::Title,
            "byline" => Self::Byline,
            "seal" => Self::Seal,
            "rule" => Self::Rule,
            "masthead" => Self::Masthead,
            "issue" => Self::Issue,
            "source" => Self::Source,
            "watermark" => Self::Watermark,
            _ => {
                let n: usize = key.strip_prefix("line.")?.parse().ok()?;
                Self::Line(n.checked_sub(1)?)
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedChar {
    pub ch: char,
    /// 字格中心。
    pub x: f32,
    pub y: f32,
    /// 字格边长。
    pub size: f32,
    pub role: Role,
    pub part: Part,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    /// 细线，显现时从一端生长出来。
    Rule,
    /// 印章底色。
    Seal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub kind: ShapeKind,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// 跟着第几个字一起显现。
    pub start_char: usize,
    pub part: Part,
}

/// 时间线上的一组字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    pub len: usize,
    /// 整组同时出现（刊头、印章、注释），而不是逐字出现。
    pub together: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub chars: Vec<PlacedChar>,
    /// 按出现顺序分组，组内字在 `chars` 里连续。
    pub columns: Vec<Column>,
    pub shapes: Vec<Shape>,
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutOptions {
    pub style: LayoutStyle,
    /// 整体字号倍数。
    pub scale: f32,
    /// 期号（第几首），杂志版式的刊头用。
    pub issue: usize,
    /// 用户拖出来的位置；None 时按诗词包的 `text.position`。
    pub placement: Option<Placement>,
}

/// 诗主体（标题、正文、署名、印章）外接框的中心，按屏幕宽、高归一到 0..1。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
}

impl Placement {
    /// 靠哪边：决定版面按哪种对齐排、刊头和背景大字放到哪一侧。
    fn side(self) -> TextPosition {
        if self.x < 0.4 {
            TextPosition::Left
        } else if self.x > 0.6 {
            TextPosition::Right
        } else {
            TextPosition::Center
        }
    }
}

/// 诗的主体：不含角落小字和背景大字。拖动、定位都只动这部分。
pub fn is_main(role: Role) -> bool {
    !matches!(role, Role::Caption | Role::Watermark)
}

impl Layout {
    /// 诗主体的外接框 (x0, y0, x1, y1)，没有主体时返回 None。
    pub fn main_bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in self.chars.iter().filter(|c| is_main(c.role)) {
            let h = c.size / 2.0;
            b = (
                b.0.min(c.x - h),
                b.1.min(c.y - h),
                b.2.max(c.x + h),
                b.3.max(c.y + h),
            );
        }
        for s in self.shapes.iter().filter(|s| self.shape_is_main(s)) {
            b = (b.0.min(s.x0), b.1.min(s.y0), b.2.max(s.x1), b.3.max(s.y1));
        }
        (b.0 <= b.2).then_some(b)
    }

    /// 形状跟着哪个字出现就属于哪部分（出处下面的细线属于角落小字）。
    pub fn shape_is_main(&self, s: &Shape) -> bool {
        self.chars
            .get(s.start_char)
            .is_some_and(|c| is_main(c.role))
    }

    /// 按部件平移（单位同排版，屏幕高度为 1）。
    pub fn offset_parts(&mut self, offsets: &std::collections::HashMap<Part, (f32, f32)>) {
        if offsets.is_empty() {
            return;
        }
        for c in &mut self.chars {
            if let Some((dx, dy)) = offsets.get(&c.part) {
                c.x += dx;
                c.y += dy;
            }
        }
        for s in &mut self.shapes {
            if let Some((dx, dy)) = offsets.get(&s.part) {
                s.x0 += dx;
                s.x1 += dx;
                s.y0 += dy;
                s.y1 += dy;
            }
        }
    }

    /// 按部件缩放字号：以部件外接框中心为基准，字的大小和间距一起缩放。
    pub fn scale_parts(&mut self, sizes: &std::collections::HashMap<Part, f32>) {
        for (&part, &k) in sizes {
            if (k - 1.0).abs() < 1e-4 {
                continue;
            }
            let Some(b) = self.part_bounds(part) else {
                continue;
            };
            let (cx, cy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
            for c in self.chars.iter_mut().filter(|c| c.part == part) {
                c.x = cx + (c.x - cx) * k;
                c.y = cy + (c.y - cy) * k;
                c.size *= k;
            }
            for s in self.shapes.iter_mut().filter(|s| s.part == part) {
                s.x0 = cx + (s.x0 - cx) * k;
                s.x1 = cx + (s.x1 - cx) * k;
                s.y0 = cy + (s.y0 - cy) * k;
                s.y1 = cy + (s.y1 - cy) * k;
            }
        }
    }

    /// 页面上有哪些部件（按部件顺序）。
    pub fn parts(&self) -> Vec<Part> {
        let mut parts: Vec<Part> = self.chars.iter().map(|c| c.part).collect();
        parts.extend(self.shapes.iter().map(|s| s.part));
        parts.sort();
        parts.dedup();
        parts
    }

    /// 某个部件的外接框；这一页没有这个部件时返回 None。
    pub fn part_bounds(&self, part: Part) -> Option<(f32, f32, f32, f32)> {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in self.chars.iter().filter(|c| c.part == part) {
            let h = c.size / 2.0;
            b = (
                b.0.min(c.x - h),
                b.1.min(c.y - h),
                b.2.max(c.x + h),
                b.3.max(c.y + h),
            );
        }
        for s in self.shapes.iter().filter(|s| s.part == part) {
            b = (b.0.min(s.x0), b.1.min(s.y0), b.2.max(s.x1), b.3.max(s.y1));
        }
        (b.0 <= b.2).then_some(b)
    }

    /// 点 (x, y) 落在哪个部件上（不含背景大字）；重叠时取面积最小的。`slop` 是细线的点击余量。
    pub fn part_at(&self, x: f32, y: f32, slop: f32) -> Option<Part> {
        self.parts()
            .into_iter()
            .filter(|&p| p != Part::Watermark)
            .filter_map(|p| {
                let b = self.part_bounds(p)?;
                let hit = x >= b.0 - slop && x <= b.2 + slop && y >= b.1 - slop && y <= b.3 + slop;
                hit.then_some((p, (b.2 - b.0 + slop) * (b.3 - b.1 + slop)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p)
    }

    /// 把诗主体平移到 `p`，整块留在屏幕边距以内。
    fn place(&mut self, p: Placement, aspect: f32) {
        let Some(b) = self.main_bounds() else { return };
        let (bw, bh) = (b.2 - b.0, b.3 - b.1);
        let margin = 0.03;
        let clamp = |c: f32, half: f32, max: f32| {
            let (lo, hi) = (margin + half, max - margin - half);
            if lo > hi { max / 2.0 } else { c.clamp(lo, hi) }
        };
        let cx = clamp(p.x * aspect, bw / 2.0, aspect);
        let cy = clamp(p.y, bh / 2.0, 1.0);
        let (dx, dy) = (cx - (b.0 + b.2) / 2.0, cy - (b.1 + b.3) / 2.0);
        let main: Vec<bool> = self.chars.iter().map(|c| is_main(c.role)).collect();
        for (c, _) in self.chars.iter_mut().zip(&main).filter(|(_, m)| **m) {
            c.x += dx;
            c.y += dy;
        }
        for s in &mut self.shapes {
            if main.get(s.start_char).copied().unwrap_or(false) {
                s.x0 += dx;
                s.x1 += dx;
                s.y0 += dy;
                s.y1 += dy;
            }
        }
    }
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            style: LayoutStyle::Magazine,
            scale: 1.0,
            issue: 1,
            placement: None,
        }
    }
}

/// 标点在诗句里不显示，竖排古诗通常不带标点。
fn is_punct(c: char) -> bool {
    matches!(
        c,
        '，' | '。'
            | '、'
            | '；'
            | '：'
            | '？'
            | '！'
            | ','
            | '.'
            | ';'
            | ':'
            | '?'
            | '!'
            | ' '
            | '　'
    )
}

fn clean(s: &str) -> Vec<char> {
    s.chars().filter(|c| !is_punct(*c)).collect()
}

/// 一句最多几个字，超过就均分成几段。
const MAX_SEGMENT: usize = 14;
/// 标题一列（一行）最多几个字，超过折成两列。
const MAX_TITLE: usize = 8;

#[derive(Clone)]
struct Text {
    title: Vec<char>,
    /// 每一列（横排时每一行）：按句中标点拆开、过长的再均分后的结果。
    lines: Vec<Vec<char>>,
    /// `lines[i]` 是不是原诗某一行的结尾，分页时优先在这里断开。
    line_end: Vec<bool>,
    dynasty: Vec<char>,
    author: Vec<char>,
    source: Option<String>,
    /// 分页时的 (第几页, 共几页)，从 1 开始；只有一页时为 None。
    page: Option<(usize, usize)>,
    /// 这一页第一列在整首诗里是第几列。
    first_line: usize,
}

/// 句中标点：一行里出现这些标点就在此断开。
fn is_break(c: char) -> bool {
    matches!(
        c,
        '，' | '。' | '；' | '？' | '！' | '、' | ',' | '.' | ';' | '?' | '!'
    )
}

/// 把一行诗按句中标点拆开，过长的句子均分。
fn segments(line: &str) -> Vec<Vec<char>> {
    let mut out = Vec::new();
    for part in line.split(is_break) {
        let seg = clean(part);
        if seg.is_empty() {
            continue;
        }
        let n = seg.len().div_ceil(MAX_SEGMENT);
        let chunk = seg.len().div_ceil(n);
        out.extend(seg.chunks(chunk).map(<[char]>::to_vec));
    }
    out
}

impl Text {
    fn new(pack: &Pack) -> Self {
        let p = &pack.poem;
        let mut lines = Vec::new();
        let mut line_end = Vec::new();
        for l in &p.lines {
            let segs = segments(l);
            let n = segs.len();
            for (i, seg) in segs.into_iter().enumerate() {
                lines.push(seg);
                line_end.push(i + 1 == n);
            }
        }
        Self {
            title: clean(&p.title),
            lines,
            line_end,
            dynasty: clean(&p.dynasty),
            author: clean(&p.author),
            source: p.source.clone(),
            page: None,
            first_line: 0,
        }
    }

    /// 只含 `range` 这些列的一页。
    fn page(&self, range: std::ops::Range<usize>, page: Option<(usize, usize)>) -> Self {
        // 只拷这一页的列，不克隆整首（超长诗分页时这里会被调用很多次）。
        Self {
            title: self.title.clone(),
            lines: self.lines[range.clone()].to_vec(),
            line_end: self.line_end[range.clone()].to_vec(),
            dynasty: self.dynasty.clone(),
            author: self.author.clone(),
            source: self.source.clone(),
            page,
            first_line: self.first_line + range.start,
        }
    }

    fn longest(&self) -> usize {
        self.lines.iter().map(Vec::len).max().unwrap_or(1).max(1)
    }

    fn byline(&self) -> Vec<char> {
        let mut v = self.dynasty.clone();
        v.push('·');
        v.extend(&self.author);
        v
    }

    /// 标题分成几列（行），每列字数尽量平均。
    fn title_lines(&self) -> Vec<&[char]> {
        if self.title.is_empty() {
            return Vec::new();
        }
        let n = self.title.len().div_ceil(MAX_TITLE);
        self.title.chunks(self.title.len().div_ceil(n)).collect()
    }
}

/// 按包里的设置排版，返回一页或多页。`aspect` 是屏幕宽高比（宽 / 高）。
/// 包里写了 `text.style` 时以包为准，否则用 `opts.style`。
///
/// 诗太长、整首放进一屏字会小于可读下限时分页，每页都带标题和印章，分页尽量落在原诗的行尾、凑成整联。
pub fn layout(pack: &Pack, aspect: f32, opts: LayoutOptions) -> Vec<Layout> {
    let text = Text::new(pack);
    let style = pack.text.style.unwrap_or(opts.style);
    let scale = opts.scale.clamp(0.3, 3.0);
    let orient = pack.text.orientation;
    let pos = opts.placement.map_or(pack.text.position, Placement::side);
    let render = |t: &Text| match (style, orient) {
        (LayoutStyle::Classic, Orientation::Vertical) => classic_vertical(t, pos, aspect, scale),
        (LayoutStyle::Classic, Orientation::Horizontal) => {
            classic_horizontal(t, pos, aspect, scale)
        }
        (LayoutStyle::Magazine, o) => magazine(t, o, pos, aspect, scale, opts.issue),
    };
    let n = text.lines.len();
    if n == 0 {
        return vec![render(&text)];
    }
    // 可读下限：横屏约为屏高的 3%，竖屏屏幕更高，比例相应小一些。用户放大字号时下限也跟着放大。
    let fits_size =
        |range: std::ops::Range<usize>| body_size(&render(&text.page(range, Some((1, 2)))));
    // 下限不能超过一联（两句）单独成页时能排出的字号，否则版面受屏宽限制时会退化成每页一句。
    let min_body =
        (if aspect < 1.0 { 0.019 } else { 0.03 } * scale).min(fits_size(0..n.min(2)) * 0.95);
    let fits = |range: std::ops::Range<usize>| fits_size(range) >= min_body;

    let mut ranges = Vec::new();
    let mut start = 0;
    while start < n {
        let mut end = start + 1;
        while end < n && fits(start..end + 1) {
            end += 1;
        }
        if end < n {
            // 往回找一个更好的断点：原诗行尾且凑成偶数句（整联），其次只要是行尾。
            let good = (start + 1..=end)
                .rev()
                .find(|&e| text.line_end[e - 1] && (e - start) % 2 == 0)
                .or_else(|| (start + 1..=end).rev().find(|&e| text.line_end[e - 1]));
            // 保持原诗一行完整比填满一页更重要，只要不空出一半以上就在行尾断开。
            if let Some(e) = good.filter(|&e| (e - start) * 2 >= end - start) {
                end = e;
            }
        }
        ranges.push(start..end);
        start = end;
    }
    let total = ranges.len();
    ranges
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let mut l = render(&text.page(r, (total > 1).then_some((i + 1, total))));
            if let Some(p) = opts.placement {
                l.place(p, aspect);
            }
            l
        })
        .collect()
}

/// 一页正文的字号；没有正文时视为无限大（不需要分页）。
fn body_size(l: &Layout) -> f32 {
    l.chars
        .iter()
        .find(|c| c.role == Role::Body)
        .map_or(f32::MAX, |c| c.size)
}

/// 逐步构建版面。
struct Builder {
    out: Layout,
    /// 接下来加的字和形状属于哪个部件。
    part: Part,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            out: Layout::default(),
            part: Part::Title,
        }
    }
}

impl Builder {
    /// 一组字，`at(i)` 给出第 i 个字的中心。
    fn group(
        &mut self,
        text: &[char],
        size: f32,
        role: Role,
        together: bool,
        at: impl Fn(usize) -> (f32, f32),
    ) -> usize {
        let start = self.out.chars.len();
        for (i, &ch) in text.iter().enumerate() {
            let (x, y) = at(i);
            self.out.chars.push(PlacedChar {
                ch,
                x,
                y,
                size,
                role,
                part: self.part,
            });
        }
        if !text.is_empty() {
            self.out.columns.push(Column {
                len: text.len(),
                together,
            });
        }
        start
    }

    fn shape(
        &mut self,
        kind: ShapeKind,
        (x0, y0, x1, y1): (f32, f32, f32, f32),
        start_char: usize,
    ) {
        self.out.shapes.push(Shape {
            kind,
            x0,
            y0,
            x1,
            y1,
            start_char,
            part: self.part,
        });
    }

    /// 版面内容的外接框（不含背景大字）。
    fn bounds(&self) -> (f32, f32, f32, f32) {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for c in self.out.chars.iter().filter(|c| c.role != Role::Watermark) {
            let h = c.size / 2.0;
            b = (
                b.0.min(c.x - h),
                b.1.min(c.y - h),
                b.2.max(c.x + h),
                b.3.max(c.y + h),
            );
        }
        for s in &self.out.shapes {
            b = (b.0.min(s.x0), b.1.min(s.y0), b.2.max(s.x1), b.3.max(s.y1));
        }
        b
    }

    /// 以 (ox, oy) 为原点整体缩放、再平移。
    fn transform(&mut self, k: f32, (ox, oy): (f32, f32), (dx, dy): (f32, f32)) {
        for c in &mut self.out.chars {
            c.x = ox + (c.x - ox) * k + dx;
            c.y = oy + (c.y - oy) * k + dy;
            c.size *= k;
        }
        for s in &mut self.out.shapes {
            s.x0 = ox + (s.x0 - ox) * k + dx;
            s.x1 = ox + (s.x1 - ox) * k + dx;
            s.y0 = oy + (s.y0 - oy) * k + dy;
            s.y1 = oy + (s.y1 - oy) * k + dy;
        }
    }

    /// 把版面缩放进 (x0, y0, x1, y1) 区域，再按 `position` 对齐。只缩小不放大。
    fn fit(&mut self, area: (f32, f32, f32, f32), position: TextPosition) {
        let b = self.bounds();
        let (bw, bh) = (b.2 - b.0, b.3 - b.1);
        let (aw, ah) = (area.2 - area.0, area.3 - area.1);
        // 窗口被拉成极窄的细条时也不能出现负的缩放（会镜像翻转）。
        let k = (aw / bw).min(ah / bh).clamp(1e-3, 1.0);
        self.transform(k, (b.0, b.1), (0.0, 0.0));
        let (bw, bh) = (bw * k, bh * k);
        let x = match position {
            TextPosition::Left => area.0,
            TextPosition::Center => area.0 + (aw - bw) / 2.0,
            TextPosition::Right => area.2 - bw,
        };
        let y = area.1 + (ah - bh) / 2.0;
        self.transform(1.0, (0.0, 0.0), (x - b.0, y - b.1));
    }
}

// ───────────── 传统版式 ─────────────

/// 竖排：从右往左，标题在最右（过长时折成几列），署名在最左并下沉。
fn classic_vertical(t: &Text, pos: TextPosition, aspect: f32, scale: f32) -> Layout {
    let longest = t.longest() as f32;
    let size = (0.6 / longest).min(0.075) * scale;
    let small = size * 0.6;
    let pitch = size * 1.75;
    let byline = t.byline();
    let mut b = Builder::default();
    let titles = t.title_lines();
    for (j, tl) in titles.iter().enumerate() {
        let x = -small * 1.4 * j as f32;
        b.part = Part::Title;
        b.group(tl, small, Role::Byline, false, |i| {
            (x, small * (i as f32 + 0.5))
        });
    }
    let x0 = -small * 1.4 * titles.len().saturating_sub(1) as f32;
    for (j, line) in t.lines.iter().enumerate() {
        let x = x0 - pitch * (j as f32 + 1.0);
        b.part = Part::Line(t.first_line + j);
        b.group(line, size, Role::Body, false, |i| {
            (x, size * (i as f32 + 0.5))
        });
    }
    let x = x0 - pitch * (t.lines.len() as f32 + 1.0);
    let y0 = longest * size - byline.len() as f32 * small;
    b.part = Part::Byline;
    b.group(&byline, small, Role::Byline, false, |i| {
        (x, y0 + small * (i as f32 + 0.5))
    });
    b.fit(screen_area(aspect, 0.1, pos), pos);
    b.out
}

/// 横排：从上往下，标题居上（过长时折成几行），署名右对齐在下。
fn classic_horizontal(t: &Text, pos: TextPosition, aspect: f32, scale: f32) -> Layout {
    let longest = t.longest() as f32;
    let size = (0.07 * scale).min(aspect * 0.8 / longest);
    let small = size * 0.6;
    let pitch = size * 1.7;
    let w = longest * size;
    let byline = t.byline();
    let mut b = Builder::default();
    let center = |n: usize, sz: f32| (w - n as f32 * sz) / 2.0;
    let titles = t.title_lines();
    for (j, tl) in titles.iter().enumerate() {
        let x0 = center(tl.len(), small);
        let y = small * 1.4 * j as f32;
        b.part = Part::Title;
        b.group(tl, small, Role::Byline, false, |i| {
            (x0 + small * (i as f32 + 0.5), y)
        });
    }
    let y0 = small * 1.4 * titles.len().saturating_sub(1) as f32;
    for (j, line) in t.lines.iter().enumerate() {
        let x0 = center(line.len(), size);
        let y = y0 + pitch * (j as f32 + 1.0);
        b.part = Part::Line(t.first_line + j);
        b.group(line, size, Role::Body, false, |i| {
            (x0 + size * (i as f32 + 0.5), y)
        });
    }
    let x0 = w - byline.len() as f32 * small;
    let y = y0 + pitch * (t.lines.len() as f32 + 1.0);
    b.part = Part::Byline;
    b.group(&byline, small, Role::Byline, false, |i| {
        (x0 + small * (i as f32 + 0.5), y)
    });
    b.fit(screen_area(aspect, 0.1, pos), pos);
    b.out
}

/// 屏幕上留出边距后的可用区域；靠边对齐时文字块只占大约一半宽度。
fn screen_area(aspect: f32, margin: f32, pos: TextPosition) -> (f32, f32, f32, f32) {
    let x0 = margin * 1.3;
    let x1 = (aspect - margin * 1.3).max(x0 + 0.01);
    let half = (x1 - x0) * 0.55;
    match pos {
        TextPosition::Left => (x0, margin, x0 + half, 1.0 - margin),
        TextPosition::Center => (x0, margin, x1, 1.0 - margin),
        TextPosition::Right => (x1 - half, margin, x1, 1.0 - margin),
    }
}

// ───────────── 杂志版式 ─────────────

const RULE: f32 = 0.0012;

fn magazine(
    t: &Text,
    orient: Orientation,
    pos: TextPosition,
    aspect: f32,
    scale: f32,
    issue: usize,
) -> Layout {
    let margin = 0.075;
    let mut b = Builder::default();
    let portrait = aspect < 1.0;

    // 背景大字：取标题第一个字，最先出现，铺在文字块对面的留白里。
    let mark = t.title.first().copied();
    if let Some(ch) = mark {
        b.part = Part::Watermark;
        b.group(&[ch], 0.0, Role::Watermark, true, |_| (0.0, 0.0));
    }

    match orient {
        Orientation::Vertical => magazine_vertical_block(&mut b, t, scale),
        Orientation::Horizontal => magazine_horizontal_block(&mut b, t, scale),
    }
    // 文字块放进屏幕一侧（竖屏时用整个宽度）。背景大字不计入外接框，fit 后再单独摆放。
    let area = if portrait {
        (margin, margin * 2.2, aspect - margin, 1.0 - margin * 2.2)
    } else {
        let a = screen_area(aspect, margin, pos);
        (a.0, margin * 1.6, a.2, 1.0 - margin * 1.6)
    };
    b.fit(area, if portrait { TextPosition::Center } else { pos });
    let (bx0, _, bx1, _) = b.bounds();
    if mark.is_some() {
        // 对面留白的中心；没有足够留白（竖屏、居中）时放在文字块后面。
        // 放在文字块旁边的留白里（居中时放左边）；竖屏没有留白，只能放在文字块后面。
        let (free0, free1) = match pos {
            _ if portrait => (0.0, aspect),
            TextPosition::Left => (bx1, aspect),
            TextPosition::Right | TextPosition::Center => (0.0, bx0),
        };
        let c = &mut b.out.chars[0];
        c.size = ((free1 - free0) * 0.7).clamp(0.3, 0.72);
        c.x = (free0 + free1) / 2.0;
        c.y = 0.5;
    }

    // 刊头和出处：放在文字块对面的上下角，竖屏时放在上下边。
    let caption = 0.016 * scale.sqrt();
    let side_left = portrait || pos != TextPosition::Left;
    let corner_x = |w: f32| {
        if side_left {
            margin
        } else {
            aspect - margin - w
        }
    };
    let masthead: Vec<char> = "MUSE".chars().collect();
    let issue_text: Vec<char> = match t.page {
        Some((p, n)) => format!("No.{issue:02}  {p}/{n}"),
        None => format!("No.{issue:02}"),
    }
    .chars()
    .collect();
    let track = caption * 1.6;
    let mw = masthead.len() as f32 * track;
    let mx = corner_x(mw);
    let my = margin * 0.9;
    b.part = Part::Masthead;
    b.group(&masthead, caption * 1.3, Role::Caption, true, |i| {
        (mx + track * (i as f32 + 0.5), my)
    });
    let iw = issue_text.len() as f32 * caption * 0.75;
    let ix = corner_x(iw);
    b.part = Part::Issue;
    b.group(&issue_text, caption, Role::Caption, true, |i| {
        (ix + caption * 0.75 * (i as f32 + 0.5), my + caption * 2.2)
    });

    let mut foot: Vec<char> = t.byline();
    if let Some(src) = &t.source {
        foot.extend("　".chars());
        foot.extend(src.chars());
    }
    // 出处很长时缩小，最多占屏宽的 45%。
    let caption = caption.min(aspect * 0.45 / foot.len().max(1) as f32);
    let fw = foot.len() as f32 * caption;
    let fx = corner_x(fw);
    let fy = 1.0 - margin * 0.9;
    let rule_w = (fw * 0.5).max(0.05);
    b.part = Part::Source;
    let start = b.group(&foot, caption, Role::Caption, true, |i| {
        (fx + caption * (i as f32 + 0.5), fy)
    });
    let rx = if side_left { fx } else { fx + fw - rule_w };
    b.shape(
        ShapeKind::Rule,
        (
            rx,
            fy - caption * 1.6,
            rx + rule_w,
            fy - caption * 1.6 + RULE,
        ),
        start,
    );
    b.out
}

/// 竖排杂志文字块，在局部坐标里构建（原点在右上，向左展开），之后整体 fit。
fn magazine_vertical_block(b: &mut Builder, t: &Text, scale: f32) {
    let longest = t.longest() as f32;
    let body = (0.058 * scale).min(0.62 / longest);
    let titles = t.title_lines();
    let title_len = titles.iter().map(|l| l.len()).max().unwrap_or(1);
    let title = (body * 1.9).min(0.72 / title_len as f32);
    let small = (body * 0.46).max(0.014);
    let pitch = body * 2.05;
    // 字距：字身满格，需要留一点空隙，正文比标题松一些。
    let (tt, bt) = (title * 1.05, body * 1.1);

    // 标题：最右边，大号粗体；过长时折成几列。
    let mut title_start = b.out.chars.len();
    for (j, tl) in titles.iter().enumerate() {
        let tx = -title / 2.0 - title * 1.15 * j as f32;
        b.part = Part::Title;
        let s = b.group(tl, title, Role::Title, false, |i| {
            (tx, tt * (i as f32 + 0.5))
        });
        if j == 0 {
            title_start = s;
        }
    }
    let title_cols = titles.len().max(1);

    // 正文：比标题低一截，错落开。
    let rule_x = -title * 1.15 * (title_cols - 1) as f32 - title * 1.35;
    let py = title * 1.25;
    let first_x = rule_x - body * 1.5;
    let mut body_start = None;
    for (j, line) in t.lines.iter().enumerate() {
        let x = first_x - pitch * j as f32;
        b.part = Part::Line(t.first_line + j);
        let s = b.group(line, body, Role::Body, false, |i| {
            (x, py + bt * (i as f32 + 0.5))
        });
        body_start.get_or_insert(s);
    }
    let body_bottom = py + longest * bt;
    let title_bottom = title_len as f32 * tt;
    // 分栏细线：标题和正文之间，贯穿整个文字块的高度。
    let bottom = body_bottom.max(title_bottom);
    b.part = Part::Rule;
    b.shape(
        ShapeKind::Rule,
        (rule_x, 0.0, rule_x + RULE, bottom),
        body_start.unwrap_or(title_start),
    );

    // 署名和印章：最左一列，底部对齐正文。署名只写朝代，作者名在印章里，完整署名在底部小字。
    let last_x = first_x - pitch * (t.lines.len().max(1) - 1) as f32;
    let bx = last_x - pitch * 0.8;
    let (seal_w, seal_h) = seal_size(t.author.len(), small);
    let seal_y0 = bottom - seal_h;
    let byline = t.dynasty.clone();
    let by_y0 = seal_y0 - small * 0.9 - byline.len() as f32 * small;
    b.part = Part::Byline;
    b.group(&byline, small, Role::Byline, false, |i| {
        (bx, by_y0 + small * (i as f32 + 0.5))
    });
    seal(
        b,
        &t.author,
        small,
        (bx - seal_w / 2.0, seal_y0, bx + seal_w / 2.0, bottom),
    );
}

/// 横排杂志文字块：标题大字（过长时折行），下面细线，正文逐行，署名和印章右对齐。
fn magazine_horizontal_block(b: &mut Builder, t: &Text, scale: f32) {
    let longest = t.longest() as f32;
    let body = 0.06 * scale;
    let titles = t.title_lines();
    let title_len = titles.iter().map(|l| l.len()).max().unwrap_or(1);
    let title = (body * 1.7).min(body * longest / title_len as f32 * 1.4);
    let small = (body * 0.46).max(0.014);
    let pitch = body * 1.75;
    let (tt, bt) = (title * 1.05, body * 1.08);
    let width = (longest * bt).max(title_len as f32 * tt);

    let mut title_start = b.out.chars.len();
    for (j, tl) in titles.iter().enumerate() {
        let y = title / 2.0 + title * 1.2 * j as f32;
        b.part = Part::Title;
        let s = b.group(tl, title, Role::Title, false, |i| {
            (tt * (i as f32 + 0.5), y)
        });
        if j == 0 {
            title_start = s;
        }
    }
    let rule_y = title * 1.2 * titles.len().saturating_sub(1) as f32 + title * 1.35;
    let py = rule_y + body * 1.2;
    let mut body_start = None;
    for (j, line) in t.lines.iter().enumerate() {
        let y = py + pitch * j as f32;
        b.part = Part::Line(t.first_line + j);
        let s = b.group(line, body, Role::Body, false, |i| {
            (bt * (i as f32 + 0.5), y)
        });
        body_start.get_or_insert(s);
    }
    b.part = Part::Rule;
    b.shape(
        ShapeKind::Rule,
        (0.0, rule_y, width, rule_y + RULE),
        body_start.unwrap_or(title_start),
    );
    let last_y = py + pitch * (t.lines.len().max(1) - 1) as f32;
    let (seal_w, seal_h) = seal_size(t.author.len(), small);
    let by_y = last_y + body * 0.5 + small * 2.0;
    let seal_x0 = width - seal_w;
    let byline = t.dynasty.clone();
    let bx0 = seal_x0 - small * 0.9 - byline.len() as f32 * small;
    b.part = Part::Byline;
    b.group(&byline, small, Role::Byline, false, |i| {
        (bx0 + small * (i as f32 + 0.5), by_y)
    });
    seal(
        b,
        &t.author,
        small,
        (seal_x0, by_y - seal_h / 2.0, width, by_y + seal_h / 2.0),
    );
}

/// 印章的字号与尺寸：1 字方章，2 字竖长章，3、4 字两列方章。
fn seal_char(small: f32) -> f32 {
    small * 1.15
}

fn seal_size(n: usize, small: f32) -> (f32, f32) {
    let c = seal_char(small);
    let pad = c * 0.28;
    match n {
        0 | 1 => (c + pad * 2.0, c + pad * 2.0),
        2 => (c + pad * 2.0, c * 2.0 + pad * 2.0),
        _ => (c * 2.0 + pad * 2.0, c * 2.0 + pad * 2.0),
    }
}

/// 印章：底色加阴文字，名字按竖排从右往左排进格子。
fn seal(b: &mut Builder, name: &[char], small: f32, rect: (f32, f32, f32, f32)) {
    let name: Vec<char> = name.iter().copied().take(4).collect();
    let c = seal_char(small);
    let pad = c * 0.28;
    let start = b.out.chars.len();
    b.part = Part::Seal;
    b.shape(ShapeKind::Seal, rect, start);
    let (x0, y0, x1, _) = rect;
    let cols = if name.len() > 2 { 2 } else { 1 };
    let per_col = name.len().div_ceil(cols).max(1);
    let n = name.len();
    b.group(&name, c * 0.92, Role::Seal, true, |i| {
        let col = i / per_col;
        let row = i % per_col;
        // 三个字时右列两字、左列一字居中。
        let in_col = if col == 0 {
            per_col.min(n)
        } else {
            n - per_col
        };
        let col_h = in_col as f32 * c;
        let full_h = per_col as f32 * c;
        let x = if cols == 1 {
            (x0 + x1) / 2.0
        } else {
            x1 - pad - c * (col as f32 + 0.5)
        };
        let y = y0 + pad + (full_h - col_h) / 2.0 + c * (row as f32 + 0.5);
        (x, y)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page1(p: &Pack, aspect: f32, opts: LayoutOptions) -> Layout {
        let pages = layout(p, aspect, opts);
        assert_eq!(pages.len(), 1);
        pages.into_iter().next().unwrap()
    }

    fn long_pack(lines: &[&str], title: &str, orientation: &str, style: &str) -> Pack {
        let lines: Vec<String> = lines.iter().map(|l| format!("'{l}'")).collect();
        toml::from_str(&format!(
            "format = 1\n[poem]\ntitle='{title}'\nauthor='白居易'\ndynasty='唐'\n\
             lines=[{}]\nsource='《全唐诗》卷四百三十五'\n\
             [text]\norientation='{orientation}'\nstyle='{style}'\n",
            lines.join(",")
        ))
        .unwrap()
    }

    /// 《长恨歌》开头若干句的长度规律：七言，每行一联。
    fn changhen(couplets: usize) -> Vec<&'static str> {
        std::iter::repeat_n("汉皇重色思倾国，御宇多年求不得。", couplets).collect()
    }

    fn pack(orientation: &str, position: &str, style: &str) -> Pack {
        toml::from_str(&format!(
            "format = 1\n[poem]\ntitle='题都城南庄'\nauthor='崔护'\ndynasty='唐'\n\
             lines=['去年今日此门中，', '人面桃花相映红。']\nsource='《全唐诗》'\n\
             [text]\norientation='{orientation}'\nposition='{position}'\nstyle='{style}'\n"
        ))
        .unwrap()
    }

    fn assert_on_screen(l: &Layout, aspect: f32) {
        for c in l.chars.iter().filter(|c| c.role != Role::Watermark) {
            let h = c.size / 2.0;
            assert!(
                c.x - h >= -1e-4
                    && c.x + h <= aspect + 1e-4
                    && c.y - h >= -1e-4
                    && c.y + h <= 1.0 + 1e-4,
                "{c:?} 超出屏幕 (aspect {aspect})"
            );
        }
        for s in &l.shapes {
            assert!(
                s.x0 >= 0.0 && s.x1 <= aspect && s.y0 >= 0.0 && s.y1 <= 1.0,
                "{s:?}"
            );
        }
        let total: usize = l.columns.iter().map(|c| c.len).sum();
        assert_eq!(total, l.chars.len());
        assert!(l.shapes.iter().all(|s| s.start_char < l.chars.len()));
    }

    #[test]
    fn classic_vertical_goes_right_to_left_without_punctuation() {
        let l = page1(
            &pack("vertical", "right", "classic"),
            16.0 / 9.0,
            LayoutOptions::default(),
        );
        let lens: Vec<usize> = l.columns.iter().map(|c| c.len).collect();
        assert_eq!(lens, vec![5, 7, 7, 4]);
        assert!(!l.chars.iter().any(|c| c.ch == '，' || c.ch == '。'));
        let a = l.chars.iter().find(|c| c.ch == '去').unwrap();
        let b = l.chars.iter().find(|c| c.ch == '红').unwrap();
        assert!(a.x > b.x);
        assert_on_screen(&l, 16.0 / 9.0);
    }

    #[test]
    fn magazine_has_title_rule_seal_and_fits_all_screens() {
        for aspect in [16.0 / 9.0, 21.0 / 9.0, 4.0 / 3.0, 9.0 / 16.0] {
            for o in ["vertical", "horizontal"] {
                for p in ["left", "center", "right"] {
                    for scale in [0.7, 1.0, 1.6] {
                        let opts = LayoutOptions {
                            scale,
                            ..LayoutOptions::default()
                        };
                        let l = page1(&pack(o, p, "magazine"), aspect, opts);
                        assert_on_screen(&l, aspect);
                        let title = l.chars.iter().find(|c| c.role == Role::Title).unwrap();
                        let body = l.chars.iter().find(|c| c.role == Role::Body).unwrap();
                        assert!(title.size > body.size);
                        assert!(l.shapes.iter().any(|s| s.kind == ShapeKind::Seal));
                        assert!(l.shapes.iter().any(|s| s.kind == ShapeKind::Rule));
                        assert_eq!(l.chars.iter().filter(|c| c.role == Role::Seal).count(), 2);
                        assert_eq!(l.chars[0].role, Role::Watermark);
                    }
                }
            }
        }
    }

    #[test]
    fn scale_makes_text_bigger_until_it_must_fit() {
        let p = pack("vertical", "right", "magazine");
        let size = |scale| {
            let opts = LayoutOptions {
                scale,
                ..LayoutOptions::default()
            };
            // 放到很大时允许分页，这里只看第一页。
            layout(&p, 16.0 / 9.0, opts)[0]
                .chars
                .iter()
                .find(|c| c.role == Role::Body)
                .unwrap()
                .size
        };
        assert!(size(1.2) > size(1.0));
        assert!(size(3.0) < 0.2);
    }

    #[test]
    fn pack_style_overrides_option() {
        let l = page1(
            &pack("vertical", "right", "classic"),
            1.6,
            LayoutOptions::default(),
        );
        assert!(l.shapes.is_empty());
    }

    #[test]
    fn splits_lines_at_internal_punctuation_and_wraps_long_segments() {
        assert_eq!(segments("君不见黄河之水天上来，奔流到海不复回。").len(), 2);
        assert_eq!(segments("去年今日此门中，").len(), 1);
        let long = segments("一二三四五六七八九十一二三四五六七八九十");
        assert_eq!(long.len(), 2);
        assert!(long.iter().all(|s| s.len() <= MAX_SEGMENT));
    }

    #[test]
    fn long_poems_paginate_at_couplets_and_stay_readable() {
        for aspect in [16.0 / 9.0, 9.0 / 16.0, 21.0 / 9.0] {
            for o in ["vertical", "horizontal"] {
                for style in ["magazine", "classic"] {
                    let p = long_pack(&changhen(60), "长恨歌", o, style);
                    let pages = layout(&p, aspect, LayoutOptions::default());
                    assert!(pages.len() > 1, "{aspect} {o} {style}");
                    let min = if aspect < 1.0 { 0.019 } else { 0.03 };
                    let mut total = 0;
                    for l in &pages {
                        assert_on_screen(l, aspect);
                        assert!(body_size(l) >= min * 0.999, "{} < {min}", body_size(l));
                        let n = l.chars.iter().filter(|c| c.role == Role::Body).count();
                        // 每页都是整联（每联 14 字）。
                        assert_eq!(n % 14, 0, "{aspect} {o} {style}");
                        total += n;
                    }
                    assert_eq!(total, 60 * 14);
                }
            }
        }
    }

    #[test]
    fn long_title_wraps_and_page_number_shows() {
        let p = long_pack(
            &changhen(40),
            "闻王昌龄左迁龙标遥有此寄",
            "vertical",
            "magazine",
        );
        let pages = layout(&p, 16.0 / 9.0, LayoutOptions::default());
        let l = &pages[0];
        let xs: std::collections::BTreeSet<i32> = l
            .chars
            .iter()
            .filter(|c| c.role == Role::Title)
            .map(|c| (c.x * 1000.0) as i32)
            .collect();
        assert_eq!(xs.len(), 2, "标题应折成两列");
        let caption: String = l
            .chars
            .iter()
            .filter(|c| c.role == Role::Caption)
            .map(|c| c.ch)
            .collect();
        assert!(caption.contains(&format!("1/{}", pages.len())), "{caption}");
    }

    #[test]
    fn never_degrades_to_one_segment_per_page() {
        let quatrain = ["床前明月光，疑是地上霜。", "举头望明月，低头思故乡。"];
        for aspect in [16.0 / 9.0, 9.0 / 16.0, 0.4] {
            for o in ["vertical", "horizontal"] {
                for style in ["magazine", "classic"] {
                    for scale in [1.0, 1.5, 3.0] {
                        let opts = LayoutOptions {
                            scale,
                            ..LayoutOptions::default()
                        };
                        for lines in [&quatrain[..], &changhen(60)[..]] {
                            let p = long_pack(lines, "静夜思", o, style);
                            let pages = layout(&p, aspect, opts);
                            let cols = |l: &Layout| {
                                l.chars.iter().filter(|c| c.role == Role::Body).count()
                            };
                            // 除最后一页外每页至少两句（五言 10 字 / 七言 14 字）。
                            for l in &pages[..pages.len() - 1] {
                                assert!(cols(l) >= 10, "{aspect} {o} {style} {scale}: {}", cols(l));
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn very_narrow_window_keeps_positive_sizes() {
        for aspect in [0.05, 0.1, 0.2] {
            for style in ["magazine", "classic"] {
                let p = long_pack(&changhen(4), "长恨歌", "vertical", style);
                for l in layout(&p, aspect, LayoutOptions::default()) {
                    assert!(
                        l.chars.iter().all(|c| c.size > 0.0 && c.x.is_finite()),
                        "{aspect} {style}"
                    );
                }
            }
        }
    }

    #[test]
    fn placement_moves_main_block_and_stays_on_screen() {
        let aspect = 16.0 / 9.0;
        for style in ["magazine", "classic"] {
            for o in ["vertical", "horizontal"] {
                let p = pack(o, "right", style);
                for (x, y) in [
                    (0.25, 0.5),
                    (0.5, 0.5),
                    (0.75, 0.45),
                    (0.0, 0.0),
                    (1.0, 1.0),
                ] {
                    let opts = LayoutOptions {
                        placement: Some(Placement { x, y }),
                        ..LayoutOptions::default()
                    };
                    let l = page1(&p, aspect, opts);
                    assert_on_screen(&l, aspect);
                    let b = l.main_bounds().unwrap();
                    let (cx, cy) = ((b.0 + b.2) / 2.0 / aspect, (b.1 + b.3) / 2.0);
                    // 没被边距挡住时中心就在指定的位置。
                    let m = 0.031;
                    let free = b.0 > m && b.2 < aspect - m && b.1 > m && b.3 < 1.0 - m;
                    if free {
                        assert!(
                            (cx - x).abs() < 0.01 && (cy - y).abs() < 0.01,
                            "{style} {o} {x},{y}: {cx},{cy}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn parts_are_numbered_across_pages_and_move_alone() {
        let lines = changhen(60);
        let p = long_pack(&lines, "长恨歌", "vertical", "magazine");
        let pages = layout(&p, 16.0 / 9.0, LayoutOptions::default());
        assert!(pages.len() > 1);
        // 每页的正文列号接着上一页往下编。
        let mut next = 0;
        for page in &pages {
            let mut ids: Vec<usize> = page
                .chars
                .iter()
                .filter_map(|c| match c.part {
                    Part::Line(i) => Some(i),
                    _ => None,
                })
                .collect();
            ids.dedup();
            assert_eq!(ids.first(), Some(&next));
            next = ids.last().unwrap() + 1;
        }

        let mut l = page1(
            &pack("vertical", "right", "magazine"),
            16.0 / 9.0,
            LayoutOptions::default(),
        );
        let before = l.clone();
        let offsets = std::collections::HashMap::from([(Part::Line(1), (0.1, -0.05))]);
        l.offset_parts(&offsets);
        for (a, b) in l.chars.iter().zip(&before.chars) {
            if a.part == Part::Line(1) {
                assert!((a.x - b.x - 0.1).abs() < 1e-6 && (a.y - b.y + 0.05).abs() < 1e-6);
            } else {
                assert_eq!(a, b);
            }
        }
        // 点在第二列的字上能选中它，印章和标题也能选中。
        for part in [Part::Line(1), Part::Seal, Part::Title, Part::Byline] {
            let c = l.chars.iter().find(|c| c.part == part).unwrap();
            assert_eq!(l.part_at(c.x, c.y, 0.0), Some(part), "{part:?}");
        }
        assert_eq!(l.part_at(-1.0, -1.0, 0.0), None);
    }

    #[test]
    fn scaling_a_part_keeps_its_center_and_leaves_others() {
        let mut l = page1(
            &pack("vertical", "right", "magazine"),
            16.0 / 9.0,
            LayoutOptions::default(),
        );
        let before = l.clone();
        let b0 = l.part_bounds(Part::Title).unwrap();
        l.scale_parts(&std::collections::HashMap::from([(Part::Title, 1.5)]));
        let b1 = l.part_bounds(Part::Title).unwrap();
        let center = |b: (f32, f32, f32, f32)| ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
        let (c0, c1) = (center(b0), center(b1));
        assert!((c0.0 - c1.0).abs() < 1e-5 && (c0.1 - c1.1).abs() < 1e-5);
        assert!(((b1.3 - b1.1) / (b0.3 - b0.1) - 1.5).abs() < 1e-3);
        for (a, b) in l.chars.iter().zip(&before.chars) {
            if a.part == Part::Title {
                assert!((a.size / b.size - 1.5).abs() < 1e-5);
            } else {
                assert_eq!(a, b);
            }
        }
        assert!(l.parts().contains(&Part::Watermark));
    }
}
