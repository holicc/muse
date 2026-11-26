// 颜色换算：HEX（3/6/8 位）、HSV、对比度。

export interface RGBA {
  r: number;
  g: number;
  b: number;
  /** 0..1 */
  a: number;
}

export function parseHex(s: string): RGBA | null {
  let h = s.trim().replace(/^#/, '');
  if (!/^[0-9a-fA-F]+$/.test(h)) return null;
  if (h.length === 3) h = h.split('').map((c) => c + c).join('');
  if (h.length !== 6 && h.length !== 8) return null;
  const n = (i: number) => parseInt(h.slice(i, i + 2), 16);
  return { r: n(0), g: n(2), b: n(4), a: h.length === 8 ? n(6) / 255 : 1 };
}

export function toHex(c: RGBA, withAlpha = true): string {
  const x = (v: number) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, '0');
  const a = Math.round(c.a * 255);
  return ('#' + x(c.r) + x(c.g) + x(c.b) + (withAlpha && a < 255 ? x(a) : '')).toUpperCase();
}

export function rgbToHsv({ r, g, b }: RGBA): [number, number, number] {
  const R = r / 255, G = g / 255, B = b / 255;
  const max = Math.max(R, G, B), min = Math.min(R, G, B), d = max - min;
  let h = 0;
  if (d) {
    if (max === R) h = ((G - B) / d) % 6;
    else if (max === G) h = (B - R) / d + 2;
    else h = (R - G) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return [h, max ? d / max : 0, max];
}

export function hsvToRgb(h: number, s: number, v: number, a = 1): RGBA {
  const c = v * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = v - c;
  const [r, g, b] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return { r: (r + m) * 255, g: (g + m) * 255, b: (b + m) * 255, a };
}

/** WCAG 相对亮度（线性）。 */
export function luminance(c: RGBA): number {
  const f = (v: number) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b);
}

/** 颜色在给定亮度背景上的对比度。 */
export function contrastOn(hex: string, bgLuma: number): number {
  const c = parseHex(hex);
  if (!c) return 21;
  const l = luminance(c);
  const [hi, lo] = l > bgLuma ? [l, bgLuma] : [bgLuma, l];
  return (hi + 0.05) / (lo + 0.05);
}

/** 两个颜色之间插值（sRGB），给渐变色条显示用。 */
export function mix(a: string, b: string, t: number): string {
  const x = parseHex(a), y = parseHex(b);
  if (!x || !y) return a;
  return toHex({ r: x.r + (y.r - x.r) * t, g: x.g + (y.g - x.g) * t, b: x.b + (y.b - x.b) * t, a: x.a + (y.a - x.a) * t });
}

/** 设计稿的中国传统色。 */
export const PALETTE: { name: string; hex: string }[] = [
  { name: '朱砂', hex: '#FF461F' },
  { name: '胭脂', hex: '#9D2933' },
  { name: '檀色', hex: '#B36D61' },
  { name: '藤黄', hex: '#FFB61E' },
  { name: '秋香', hex: '#D9B611' },
  { name: '松花绿', hex: '#057748' },
  { name: '天水碧', hex: '#5AA4AE' },
  { name: '石青', hex: '#1685A9' },
  { name: '黛蓝', hex: '#425066' },
  { name: '月白', hex: '#D6ECF0' },
  { name: '象牙白', hex: '#FFFBF0' },
  { name: '墨色', hex: '#50616D' },
];

export function paletteName(hex: string): string | null {
  const h = hex.toUpperCase().slice(0, 7);
  return PALETTE.find((p) => p.hex === h)?.name ?? null;
}

/** 印章默认朱红、自动墨色。 */
export const SEAL_DEFAULT = '#B8352A';
