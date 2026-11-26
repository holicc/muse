// muse 的全部着色器：背景、图层、文字、粒子。
// 坐标约定：CPU 传入的都是像素坐标，原点左上。

struct Globals {
    screen: vec2f,
    time: f32,
    particle_kind: u32,
    bg_top: vec4f,
    bg_bottom: vec4f,
    // rgb 雾色，a 雾的浓度
    mist: vec4f,
    particle: vec4f,
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var samp: sampler;
@group(1) @binding(0) var tex: texture_2d<f32>;

fn to_clip(p: vec2f) -> vec4f {
    return vec4f(p.x / g.screen.x * 2.0 - 1.0, 1.0 - p.y / g.screen.y * 2.0, 0.0, 1.0);
}

// 两个三角形拼成的单位方块，返回 [-1, 1]。
fn corner(i: u32) -> vec2f {
    var c = array<vec2f, 6>(
        vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(-1.0, 1.0),
        vec2f(-1.0, 1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
    );
    return c[i];
}

fn hash(p: vec2f) -> f32 {
    let q = fract(p * vec2f(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    // 五次插值，二阶导连续，雾在格子边界处不会出现亮暗带。
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    return mix(mix(hash(i), hash(i + vec2f(1.0, 0.0)), u.x),
               mix(hash(i + vec2f(0.0, 1.0)), hash(i + vec2f(1.0, 1.0)), u.x), u.y);
}

fn fbm(p0: vec2f) -> f32 {
    var p = p0;
    var v = 0.0;
    var a = 0.5;
    for (var i = 0; i < 4; i++) {
        v += a * noise(p);
        // 每层旋转一点，打散轴对齐的痕迹。
        p = mat2x2f(1.6, 1.2, -1.2, 1.6) * p + vec2f(17.0, 9.0);
        a *= 0.5;
    }
    return v;
}

// ───────────── 背景 ─────────────

struct BgOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn bg_vs(@builtin(vertex_index) i: u32) -> BgOut {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: BgOut;
    o.pos = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

@fragment
fn bg_fs(in: BgOut) -> @location(0) vec4f {
    let aspect = g.screen.x / g.screen.y;
    var col = mix(g.bg_top.rgb, g.bg_bottom.rgb, smoothstep(0.0, 1.0, in.uv.y));
    // 缓慢飘动的雾，越往下越浓。
    let p = vec2f(in.uv.x * aspect, in.uv.y) * 2.2 + vec2f(g.time * 0.012, g.time * 0.003);
    let m = smoothstep(0.35, 0.85, fbm(p)) * g.mist.a * (0.4 + 0.6 * in.uv.y);
    col = mix(col, g.mist.rgb, m);
    // 宣纸般的细颗粒，避免大面积渐变出现色带。
    let grain = hash(in.pos.xy) - 0.5;
    col += grain * 0.012;
    // 暗角。
    let d = in.uv - 0.5;
    col *= 1.0 - dot(d, d) * 0.35;
    return vec4f(col, 1.0);
}

// ───────────── 图层图片 ─────────────

struct ImageInst {
    @location(0) rect: vec4f,   // x0 y0 x1 y1，像素
    @location(1) alpha: f32,
}

struct ImageOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) alpha: f32,
}

@vertex
fn image_vs(@builtin(vertex_index) i: u32, inst: ImageInst) -> ImageOut {
    let c = corner(i) * 0.5 + 0.5;
    var o: ImageOut;
    o.pos = to_clip(mix(inst.rect.xy, inst.rect.zw, c));
    o.uv = c;
    o.alpha = inst.alpha;
    return o;
}

@fragment
fn image_fs(in: ImageOut) -> @location(0) vec4f {
    let c = textureSample(tex, samp, in.uv);
    return vec4f(c.rgb, c.a * in.alpha);
}

// ───────────── 文字 ─────────────

struct GlyphInst {
    @location(0) center: vec2f,
    @location(1) half_size: vec2f,
    @location(2) uv0: vec2f,
    @location(3) uv1: vec2f,
    @location(4) color: vec4f,
    // x: 显现进度 0..1，y: 最大晕染半径（纹素）
    @location(5) params: vec2f,
}

struct GlyphOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
    @location(2) params: vec2f,
}

@vertex
fn glyph_vs(@builtin(vertex_index) i: u32, inst: GlyphInst) -> GlyphOut {
    let c = corner(i);
    let p = inst.params.x;
    // 墨刚落下时略大，随后收拢。
    let grow = 1.0 + 0.10 * (1.0 - p) * (1.0 - p);
    var o: GlyphOut;
    o.pos = to_clip(inst.center + c * inst.half_size * grow);
    o.uv = mix(inst.uv0, inst.uv1, c * 0.5 + 0.5);
    o.color = inst.color;
    o.params = inst.params;
    return o;
}

@fragment
fn glyph_fs(in: GlyphOut) -> @location(0) vec4f {
    let p = in.params.x;
    let texel = 1.0 / vec2f(textureDimensions(tex));
    let r = pow(1.0 - p, 1.6) * in.params.y;
    // 两圈采样近似高斯模糊，模拟墨在纸上晕开再收拢。
    var cov = textureSample(tex, samp, in.uv).r * 2.0;
    var w = 2.0;
    for (var k = 0; k < 8; k++) {
        let a = f32(k) * 0.7853982 + 0.3;
        let d = vec2f(cos(a), sin(a));
        cov += textureSample(tex, samp, in.uv + d * r * texel).r;
        cov += textureSample(tex, samp, in.uv + d * r * 0.5 * texel).r * 1.5;
        w += 2.5;
    }
    cov /= w;
    // 晕染时边缘偏淡、中心偏浓。
    let ink = smoothstep(0.0, 1.0, p);
    let a = clamp(cov * (1.0 + (1.0 - p) * 0.8), 0.0, 1.0) * ink * in.color.a;
    return vec4f(in.color.rgb, a);
}

// ───────────── 细线、印章 ─────────────

struct ShapeInst {
    @location(0) rect: vec4f,
    @location(1) color: vec4f,
    @location(2) color2: vec4f,
    // x: 渐变方向（0 横向，1 纵向），y、z: 渐变起止坐标（像素）
    @location(3) grad: vec3f,
    @location(4) kind: f32,
}

struct ShapeOut {
    @builtin(position) pos: vec4f,
    @location(0) color: vec4f,
    @location(1) local: vec2f,
    @location(2) kind: f32,
    @location(3) color2: vec4f,
    @location(4) grad: vec3f,
}

@vertex
fn shape_vs(@builtin(vertex_index) i: u32, inst: ShapeInst) -> ShapeOut {
    let c = corner(i) * 0.5 + 0.5;
    var o: ShapeOut;
    o.pos = to_clip(mix(inst.rect.xy, inst.rect.zw, c));
    o.color = inst.color;
    o.color2 = inst.color2;
    o.grad = inst.grad;
    o.local = mix(inst.rect.xy, inst.rect.zw, c);
    o.kind = inst.kind;
    return o;
}

@fragment
fn shape_fs(in: ShapeOut) -> @location(0) vec4f {
    let along = select(in.local.x, in.local.y, in.grad.x > 0.5);
    let t = clamp((along - in.grad.y) / max(in.grad.z - in.grad.y, 0.001), 0.0, 1.0);
    let color = mix(in.color, in.color2, t);
    var a = color.a;
    if in.kind > 0.5 {
        // 印泥不匀：细碎的斑驳，让印章不像一块纯色方块。
        let n = noise(in.local * 0.35) * 0.6 + noise(in.local * 1.7) * 0.4;
        a *= smoothstep(0.12, 0.4, n) * 0.25 + 0.75;
    }
    return vec4f(color.rgb, a);
}

// ───────────── 粒子 ─────────────

struct ParticleInst {
    @location(0) center: vec2f,
    @location(1) half_size: vec2f,
    // x: 旋转（弧度），y: 不透明度
    @location(2) params: vec2f,
}

struct ParticleOut {
    @builtin(position) pos: vec4f,
    @location(0) local: vec2f,
    @location(1) alpha: f32,
}

@vertex
fn particle_vs(@builtin(vertex_index) i: u32, inst: ParticleInst) -> ParticleOut {
    let c = corner(i);
    let s = sin(inst.params.x);
    let k = cos(inst.params.x);
    let v = c * inst.half_size;
    let rotated = vec2f(v.x * k - v.y * s, v.x * s + v.y * k);
    var o: ParticleOut;
    o.pos = to_clip(inst.center + rotated);
    o.local = c;
    o.alpha = inst.params.y;
    return o;
}

@fragment
fn particle_fs(in: ParticleOut) -> @location(0) vec4f {
    let q = in.local;
    let len = length(q);
    var a = 0.0;
    var col = g.particle.rgb;
    switch g.particle_kind {
        // 花瓣：椭圆，顶端一个小缺口。
        case 0u: {
            let e = length(vec2f(q.x / 0.62, q.y * 0.95 + 0.05));
            let notch = length(vec2f(q.x, q.y + 1.05)) - 0.32;
            let edge = fwidth(e) * 1.5;
            a = (1.0 - smoothstep(1.0 - edge, 1.0, e)) * smoothstep(-0.02, 0.06, notch);
            col = mix(col, vec3f(1.0), 0.45 * (1.0 - e));
        }
        // 雪：柔边圆点。
        case 1u: {
            a = 1.0 - smoothstep(0.25, 1.0, len);
        }
        // 雨：细长的线，两头渐隐。
        case 2u: {
            a = (1.0 - smoothstep(0.0, 1.0, abs(q.x))) * (1.0 - abs(q.y));
        }
        // 萤火：亮核加光晕。
        case 3u: {
            a = exp(-len * len * 9.0) + 0.35 * exp(-len * len * 2.5);
            col = mix(col, vec3f(1.0, 1.0, 0.85), exp(-len * len * 30.0));
        }
        // 月光微尘。
        default: {
            a = exp(-len * len * 4.5);
        }
    }
    // 光晕在方块边缘之前衰减到零，免得看出方形的边。
    a *= 1.0 - smoothstep(0.75, 1.0, len);
    return vec4f(col, a * in.alpha * g.particle.a);
}
