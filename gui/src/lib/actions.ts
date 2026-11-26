// 顶栏、快捷键、检查器共用的操作。

import { edit, get, set, without } from './store';
import { currentParts } from '../components/Stage';
import type { StyleDoc } from './types';
import { canvas } from './preview';

const clearLayout = (st: StyleDoc): StyleDoc => ({ ...st, offsets: {}, sizes: {}, opacity: {} });

/** 复位本首：本首所有部件的位置、字号、透明度恢复默认（不动颜色和背景）。 */
export function resetPoem() {
  const s = get();
  if (!s.state || !s.draft) return;
  const name = s.state.poems[s.index].name;
  edit((d) => ({ ...d, styles: { ...d.styles, [name]: clearLayout(d.styles[name]) } }));
}

/** 全部复位：每首诗的部件位置、字号、透明度，以及整首位置和字号。 */
export function resetAll() {
  edit((d) => ({
    ...d,
    styles: Object.fromEntries(Object.entries(d.styles).map(([k, v]) => [k, clearLayout(v)])),
    arrangement: { x: null, y: null, scale: 1 },
  }));
}

/** 切到下一首 / 上一首。 */
export function goPoem(delta: number) {
  const s = get();
  if (!s.state) return;
  const n = s.state.poems.length;
  set({ index: (s.index + delta + n) % n, charMode: false, selectedChar: null });
}

/** Tab / ⇧Tab：逐个选中部件（含水印）。 */
export function cycleParts(delta: number) {
  const parts = currentParts();
  if (!parts.length) return;
  const s = get();
  const i = s.selected ? parts.indexOf(s.selected) : -1;
  const next = parts[(i + delta + parts.length + (i < 0 && delta < 0 ? 1 : 0)) % parts.length];
  set({ selected: next, charMode: false, selectedChar: null });
}

/** 方向键微移（屏幕像素），不吸附。没选中部件时移动整首。 */
export function nudge(dx: number, dy: number) {
  const s = get();
  if (!s.state || !s.draft || !s.display) return;
  const name = s.state.poems[s.index].name;
  const H = s.display.height;
  const W = s.display.width;
  if (s.selected) {
    const part = s.selected;
    edit((d) => {
      const st = d.styles[name];
      const [ox, oy] = st.offsets[part] ?? [0, 0];
      const nx = r4(ox + dx / H), ny = r4(oy + dy / H);
      const offsets = Math.abs(nx) < 1e-6 && Math.abs(ny) < 1e-6 ? without(st.offsets, part) : { ...st.offsets, [part]: [nx, ny] as [number, number] };
      return { ...d, styles: { ...d.styles, [name]: { ...st, offsets } } };
    }, `nudge-${part}`);
  } else {
    const a = s.draft.arrangement;
    const p = a.x != null && a.y != null ? [a.x, a.y] : null;
    const base = p ?? canvas?.placement() ?? null;
    if (!base) return;
    edit((d) => ({
      ...d,
      arrangement: { ...d.arrangement, x: r4(clamp01(base[0] + dx / W)), y: r4(clamp01(base[1] + dy / H)) },
    }), 'nudge-whole');
  }
}

const r4 = (v: number) => Math.round(v * 10000) / 10000;
const clamp01 = (v: number) => Math.max(0, Math.min(1, v));
