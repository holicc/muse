// 画布：加载 wasm 渲染器（和桌面壁纸同一套代码），按草稿更新，提供部件框、命中测试。

import type { AutoColor, CharRect, PartRect, Poem, State } from './types';
import type { Draft } from './store';

type Wasm = typeof import('../wasm/muse_web.js');
type Preview = import('../wasm/muse_web.js').Preview;

let wasm: Wasm | null = null;

async function loadWasm(): Promise<Wasm> {
  if (!wasm) {
    const m = await import('../wasm/muse_web.js');
    await m.default();
    wasm = m;
  }
  return wasm;
}

/** 草稿里全局背景对应的图片；null 用诗词包自带。 */
export function globalWallpaper(state: State, d: Draft): string | null {
  const w = d.settings.wallpaper as string;
  if (w === 'auto') return state.wallpaper.desktop;
  if (w === 'pack') return null;
  return w;
}

/** 某首诗实际用的背景图；null 为诗词包自带的背景。 */
export function wallpaperFor(state: State, d: Draft, poem: Poem): string | null {
  const own = d.styles[poem.name]?.wallpaper;
  if (own === undefined) return globalWallpaper(state, d);
  if (own === 'pack') return null;
  if (own === 'auto') return state.wallpaper.desktop;
  return own;
}

function imagesFor(state: State, d: Draft, poem: Poem): string[] {
  const w = wallpaperFor(state, d, poem);
  return w ? [w] : poem.layers;
}

/** 传给渲染器的设置。 */
export function settingsJson(state: State, d: Draft): string {
  const s = d.settings;
  const a = d.arrangement;
  return JSON.stringify({
    style: s.style,
    text_scale: Math.min(3, Math.max(0.3, s.text_scale * a.scale)),
    placement: a.x != null && a.y != null ? [a.x, a.y] : null,
    text_color: s.text_color === 'auto' ? null : s.text_color,
    wallpaper: globalWallpaper(state, d),
    desktop: state.wallpaper.desktop,
    hold: s.hold,
    char_interval: s.char_interval,
    styles: d.styles,
  });
}

export class Canvas {
  private preview: Preview | null = null;
  private failed = new Set<string>();
  /** 字体文件路径 → 是否和当前一致，用来判断要不要换字体。 */
  private fontKey = '';

  constructor(private canvas: HTMLCanvasElement) {}

  get ready() {
    return this.preview !== null;
  }

  async init(state: State, d: Draft, index: number) {
    const m = await loadWasm();
    const b = new m.Builder();
    const font = state.font;
    if (!font.files.length) throw new Error('取不到字体文件，设计窗口无法预览这个字体');
    for (const f of font.files) b.add_font_file(await window.muse.readFile(f));
    this.fontKey = JSON.stringify(font);
    for (const p of imagesFor(state, d, state.poems[index])) {
      try {
        b.add_image(p, await window.muse.readFile(p));
      } catch {
        this.failed.add(p);
      }
    }
    const packs = await Promise.all(
      state.poems.map(async (p) => ({ name: p.name, toml: await window.muse.readText(`${p.dir}/pack.toml`) })),
    );
    const init = { ...JSON.parse(settingsJson(state, d)), packs, start: index };
    this.preview = await b.build(this.canvas, JSON.stringify(font.faces), JSON.stringify(init));
  }

  /** 先把这首诗要用的图片放进去（读不了的跳过，渲染器会退回诗词包背景）。 */
  private async ensureImages(state: State, d: Draft, poem: Poem) {
    const p = this.preview;
    if (!p) return;
    for (const path of imagesFor(state, d, poem)) {
      if (this.failed.has(path) || p.has_image(path)) continue;
      try {
        p.add_image(path, await window.muse.readFile(path));
      } catch {
        this.failed.add(path);
      }
    }
  }

  async update(state: State, d: Draft, index: number) {
    if (!this.preview) return;
    await this.ensureImages(state, d, state.poems[index]);
    this.preview.set_options(settingsJson(state, d));
  }

  /** 换字体：font.locate 的结果。 */
  async setFont(font: { files: string[]; faces: [number, number][] }) {
    const key = JSON.stringify(font);
    if (!this.preview || key === this.fontKey) return;
    for (const f of font.files) this.preview.add_font_file(await window.muse.readFile(f));
    this.preview.set_font(JSON.stringify(font.faces));
    this.fontKey = key;
  }

  async setPack(state: State, d: Draft, index: number) {
    if (!this.preview) return;
    await this.ensureImages(state, d, state.poems[index]);
    this.preview.set_pack(index);
  }

  resize(w: number, h: number) {
    this.preview?.resize(w, h);
  }

  advance(dt: number) {
    this.preview?.advance(dt);
  }
  revealAll() {
    this.preview?.reveal_all();
  }
  seek(t: number) {
    this.preview?.seek(t);
  }
  time(): number {
    return this.preview?.time() ?? 0;
  }
  render() {
    this.preview?.render();
  }
  partAt(x: number, y: number): string | null {
    return this.preview?.part_at(x, y) || null;
  }
  drag(part: string | null, dx: number, dy: number) {
    this.preview?.set_drag(part ?? '', dx, dy);
  }
  parts(): PartRect[] {
    return this.preview ? JSON.parse(this.preview.parts()) : [];
  }
  chars(): CharRect[] {
    return this.preview ? JSON.parse(this.preview.chars()) : [];
  }
  mainBounds(): [number, number, number, number] | null {
    return this.preview ? JSON.parse(this.preview.main_bounds()) : null;
  }
  placement(): [number, number] | null {
    return this.preview ? JSON.parse(this.preview.placement()) : null;
  }
  autoColor(): AutoColor | null {
    return this.preview ? JSON.parse(this.preview.auto_color()) : null;
  }
}

/** 全局只有一个画布，检查器等需要部件框、自动配色时从这里取。 */
export let canvas: Canvas | null = null;
export function setCanvas(c: Canvas | null) {
  canvas = c;
}
