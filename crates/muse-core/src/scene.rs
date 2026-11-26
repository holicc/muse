//! 和平台、GPU 无关的场景逻辑：时间线、粒子模拟。排版在 [`crate::layout`]。
//!
//! 渲染层只管把这里算出来的结果画出来，所以这部分都可以直接单测。

use crate::ParticlePreset;
pub use crate::layout::{
    Column, Layout, LayoutOptions, Part, PlacedChar, Placement, Role, Shape, ShapeKind, is_main,
    layout,
};

// ───────────── 时间线 ─────────────

/// 一首诗从出现到淡出的节奏，单位秒。
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// 相邻两个字开始晕开的间隔。
    pub char_interval: f32,
    /// 换句时额外停顿。
    pub line_pause: f32,
    /// 单个字从开始晕开到完全清晰。
    pub char_fade: f32,
    /// 全诗显示完后停留多久。
    pub hold: f32,
    /// 整首淡出。
    pub fade_out: f32,
    /// 淡出后到下一首开始的空白。
    pub gap: f32,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            char_interval: 0.35,
            line_pause: 0.9,
            char_fade: 1.8,
            hold: 90.0,
            fade_out: 4.0,
            gap: 2.0,
        }
    }
}

/// 一首诗在时间线上的安排：每个字何时开始晕开，整首何时结束。
#[derive(Debug, Clone)]
pub struct Timeline {
    timing: Timing,
    /// 按排版顺序（标题、诗句、署名）每个字的开始时间。
    starts: Vec<f32>,
    /// 最后一个字完全显示的时刻。
    revealed_at: f32,
}

impl Timeline {
    /// `columns` 是排版后按出现顺序的各组字。
    pub fn new(columns: &[Column], timing: Timing) -> Self {
        let mut starts = Vec::new();
        let mut t = 0.0;
        for (i, c) in columns.iter().enumerate() {
            if i > 0 {
                t += timing.line_pause;
            }
            if c.together {
                starts.extend(std::iter::repeat_n(t, c.len));
                t += timing.char_interval;
            } else {
                for _ in 0..c.len {
                    starts.push(t);
                    t += timing.char_interval;
                }
            }
        }
        let revealed_at = starts.last().map_or(0.0, |s| s + timing.char_fade);
        Self {
            timing,
            starts,
            revealed_at,
        }
    }

    /// 最后一个字完全显示的时刻。
    pub fn revealed_at(&self) -> f32 {
        self.revealed_at
    }

    /// 整首诗一轮（含淡出和空白）的时长。
    pub fn duration(&self) -> f32 {
        self.revealed_at + self.timing.hold + self.timing.fade_out + self.timing.gap
    }

    /// 第 `i` 个字在时刻 `t` 的显现进度，0 未出现，1 完全清晰。
    pub fn char_progress(&self, i: usize, t: f32) -> f32 {
        let start = self.starts.get(i).copied().unwrap_or(0.0);
        let p = ((t - start) / self.timing.char_fade).clamp(0.0, 1.0);
        p * self.fade_out_factor(t)
    }

    /// 整首淡出系数，1 正常显示，0 已消失。
    pub fn fade_out_factor(&self, t: f32) -> f32 {
        let begin = self.revealed_at + self.timing.hold;
        1.0 - ((t - begin) / self.timing.fade_out).clamp(0.0, 1.0)
    }
}

// ───────────── 粒子 ─────────────

/// 固定种子的小随机数发生器，避免为这点用途引入依赖。
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// [0, 1) 均匀分布。
    pub fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

/// 一个粒子。坐标同排版，屏幕高度为 1，原点左上。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub size: f32,
    pub rotation: f32,
    pub spin: f32,
    /// 用于闪烁、摇摆的相位。
    pub phase: f32,
    /// 当前不透明度。
    pub alpha: f32,
}

#[derive(Debug, Clone)]
pub struct ParticleSystem {
    pub preset: ParticlePreset,
    pub particles: Vec<Particle>,
    aspect: f32,
    time: f32,
    rng: Rng,
}

impl ParticleSystem {
    pub fn new(preset: ParticlePreset, aspect: f32, seed: u64) -> Self {
        let count = match preset {
            ParticlePreset::Petals => 36,
            ParticlePreset::Snow => 140,
            ParticlePreset::Rain => 220,
            ParticlePreset::Fireflies => 28,
            ParticlePreset::MoonlightDust => 90,
        };
        let mut sys = Self {
            preset,
            particles: Vec::with_capacity(count),
            aspect,
            time: 0.0,
            rng: Rng::new(seed),
        };
        for _ in 0..count {
            let p = sys.spawn(true);
            sys.particles.push(p);
        }
        sys
    }

    pub fn set_aspect(&mut self, aspect: f32) {
        if aspect > self.aspect {
            // 屏幕变宽时把粒子按比例铺开，避免右侧一片空白。
            let k = aspect / self.aspect;
            for p in &mut self.particles {
                p.x *= k;
            }
        }
        self.aspect = aspect;
    }

    /// 生成一个粒子。`anywhere` 为真时在全屏随机位置（初始化用），否则从入场边生成。
    fn spawn(&mut self, anywhere: bool) -> Particle {
        let r = &mut self.rng;
        let w = self.aspect;
        let mut p = Particle {
            x: r.range(-0.1, w + 0.1),
            y: r.range(-0.1, 1.1),
            vx: 0.0,
            vy: 0.0,
            size: 0.0,
            rotation: r.range(0.0, std::f32::consts::TAU),
            spin: 0.0,
            phase: r.range(0.0, std::f32::consts::TAU),
            alpha: 1.0,
        };
        match self.preset {
            ParticlePreset::Petals => {
                p.size = r.range(0.010, 0.020);
                p.vx = r.range(0.010, 0.035);
                p.vy = r.range(0.025, 0.050);
                p.spin = r.range(-1.2, 1.2);
                p.alpha = r.range(0.65, 0.95);
                if !anywhere {
                    // 风从左上吹来，从上边或左边入场。
                    if r.next_f32() < 0.7 {
                        p.y = -0.05;
                    } else {
                        p.x = -0.05;
                    }
                }
            }
            ParticlePreset::Snow => {
                p.size = r.range(0.002, 0.007);
                p.vy = p.size * 9.0 + r.range(0.01, 0.02);
                p.vx = r.range(-0.006, 0.006);
                p.alpha = r.range(0.5, 0.95);
                if !anywhere {
                    p.y = -0.02;
                }
            }
            ParticlePreset::Rain => {
                p.size = r.range(0.025, 0.06);
                p.vy = r.range(0.9, 1.3);
                p.vx = p.vy * 0.12;
                p.alpha = r.range(0.15, 0.35);
                if !anywhere {
                    p.y = -0.08;
                    p.x = r.range(-0.2, w);
                }
            }
            ParticlePreset::Fireflies => {
                p.size = r.range(0.004, 0.008);
                p.y = r.range(0.35, 1.0);
                p.vx = r.range(-0.012, 0.012);
                p.vy = r.range(-0.008, 0.004);
            }
            ParticlePreset::MoonlightDust => {
                p.size = r.range(0.0015, 0.004);
                p.vx = r.range(-0.004, 0.004);
                p.vy = r.range(-0.006, -0.001);
            }
        }
        p
    }

    /// 推进 `dt` 秒。
    pub fn update(&mut self, dt: f32) {
        // 长时间暂停后恢复时不要一步跳太远。
        let dt = dt.clamp(0.0, 0.1);
        self.time += dt;
        let t = self.time;
        let w = self.aspect;
        for i in 0..self.particles.len() {
            let p = &mut self.particles[i];
            match self.preset {
                ParticlePreset::Petals => {
                    let sway = (t * 0.9 + p.phase).sin() * 0.02;
                    p.x += (p.vx + sway) * dt;
                    p.y += p.vy * dt;
                    p.rotation += p.spin * dt;
                }
                ParticlePreset::Snow => {
                    p.x += (p.vx + (t * 0.5 + p.phase).sin() * 0.006) * dt;
                    p.y += p.vy * dt;
                }
                ParticlePreset::Rain => {
                    p.x += p.vx * dt;
                    p.y += p.vy * dt;
                }
                ParticlePreset::Fireflies => {
                    p.x += (p.vx + (t * 0.37 + p.phase).sin() * 0.01) * dt;
                    p.y += (p.vy + (t * 0.29 + p.phase * 1.3).cos() * 0.008) * dt;
                    p.alpha = 0.5 + 0.5 * (t * 1.6 + p.phase * 3.0).sin().max(0.0).powf(0.6);
                }
                ParticlePreset::MoonlightDust => {
                    p.x += (p.vx + (t * 0.2 + p.phase).sin() * 0.002) * dt;
                    p.y += p.vy * dt;
                    p.alpha = 0.35 + 0.35 * (t * 0.8 + p.phase).sin();
                }
            }
            let out = p.y > 1.1 || p.y < -0.15 || p.x > w + 0.15 || p.x < -0.25;
            if out {
                let anywhere = matches!(
                    self.preset,
                    ParticlePreset::Fireflies | ParticlePreset::MoonlightDust
                );
                self.particles[i] = self.spawn(anywhere);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_reveals_in_order_and_fades_out() {
        let col = |len, together| Column { len, together };
        let tl = Timeline::new(&[col(2, false), col(3, false)], Timing::default());
        let t = Timing::default();
        assert_eq!(tl.char_progress(0, 0.0), 0.0);
        assert!(tl.char_progress(0, 1.0) > tl.char_progress(1, 1.0));
        // 第二列第一个字要等换句停顿。
        let second_col = 2.0 * t.char_interval + t.line_pause;
        assert_eq!(tl.char_progress(2, second_col - 0.01), 0.0);
        assert!(tl.char_progress(2, second_col + 0.1) > 0.0);
        let end = tl.duration() - t.gap;
        assert_eq!(tl.char_progress(4, end), 0.0);
        assert!(tl.duration() > t.hold);

        // 整组同时出现。
        let tl = Timeline::new(&[col(3, true), col(1, false)], Timing::default());
        assert_eq!(tl.char_progress(0, 0.5), tl.char_progress(2, 0.5));
    }

    #[test]
    fn particles_stay_near_screen() {
        for preset in [
            ParticlePreset::Petals,
            ParticlePreset::Snow,
            ParticlePreset::Rain,
            ParticlePreset::Fireflies,
            ParticlePreset::MoonlightDust,
        ] {
            let mut sys = ParticleSystem::new(preset, 1.6, 7);
            let n = sys.particles.len();
            for _ in 0..3000 {
                sys.update(1.0 / 30.0);
            }
            assert_eq!(sys.particles.len(), n);
            assert!(
                sys.particles.iter().all(|p| p.x > -0.3
                    && p.x < 1.8
                    && p.y > -0.2
                    && p.y < 1.2
                    && p.alpha.is_finite()),
                "{preset:?}"
            );
        }
    }
}
