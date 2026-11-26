// 设计窗口的状态：从 muse 读到的数据、正在编辑的草稿、撤销 / 重做、界面状态。
// 所有修改先进草稿，只在画布上生效；点「保存」时一次交给 muse 写盘。

import { useSyncExternalStore } from 'react';
import type { Arrangement, Display, SettingKey, State, StyleDoc } from './types';
import { emptyStyle } from './types';

export interface Draft {
  styles: Record<string, StyleDoc>;
  settings: Record<SettingKey, any>;
  arrangement: Arrangement;
}

export type SaveState = 'idle' | 'saving' | 'saved' | 'failed';
export type Tab = 'inspector' | 'global';

export interface Store {
  phase: 'connecting' | 'offline' | 'loading' | 'ready' | 'error';
  error: string | null;
  state: State | null;
  display: Display | null;
  saved: Draft | null;
  draft: Draft | null;
  past: Draft[];
  future: Draft[];
  /** 正在编辑第几首。 */
  index: number;
  /** 选中的部件（如 line.2）；null 为整首。 */
  selected: string | null;
  /** 单字模式下选中部件里第几个字。 */
  charMode: boolean;
  selectedChar: number | null;
  tab: Tab;
  sidebar: boolean;
  save: SaveState;
  saveError: string | null;
  theme: 'light' | 'dark';
  shortcuts: boolean;
  /** 预览相对屏幕逻辑尺寸的比例（%），状态栏显示。 */
  previewScale: number;
  /** 当前页的部件（按设计稿顺序），画布每次重排后更新。 */
  parts: string[];
  /** 画布每次重排后加一，依赖画布数据（自动配色等）的组件据此刷新。 */
  canvasTick: number;
}

const SETTING_KEYS: SettingKey[] = [
  'wallpaper',
  'font',
  'style',
  'text_scale',
  'text_color',
  'hold',
  'char_interval',
  'fps',
  'battery_fps',
  'pause_on_fullscreen',
  'tray',
];

let store: Store = {
  phase: 'connecting',
  error: null,
  state: null,
  display: null,
  saved: null,
  draft: null,
  past: [],
  future: [],
  index: 0,
  selected: null,
  charMode: false,
  selectedChar: null,
  tab: 'inspector',
  sidebar: true,
  save: 'idle',
  saveError: null,
  theme: 'light',
  shortcuts: false,
  previewScale: 100,
  parts: [],
  canvasTick: 0,
};

const listeners = new Set<() => void>();

export function get(): Store {
  return store;
}

export function set(patch: Partial<Store>) {
  store = { ...store, ...patch };
  listeners.forEach((l) => l());
}

export function useStore<T>(pick: (s: Store) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => pick(store),
  );
}

export function draftFromState(s: State): Draft {
  const settings = {} as Record<SettingKey, any>;
  for (const k of SETTING_KEYS) settings[k] = (s.config as any)[k];
  const styles: Record<string, StyleDoc> = {};
  for (const p of s.poems) styles[p.name] = { ...emptyStyle(), ...(s.styles[p.name] ?? {}) };
  return { styles, settings, arrangement: { ...s.arrangement } };
}

// ───────────── 修改与撤销 ─────────────

let lastKey: string | null = null;
let lastTime = 0;

/**
 * 改草稿。`key` 相同且间隔很短的连续修改（拖滑块、拖色板）合并成一步撤销。
 */
export function edit(fn: (d: Draft) => Draft, key: string | null = null) {
  const s = store;
  if (!s.draft) return;
  const next = fn(s.draft);
  if (next === s.draft) return;
  const now = performance.now();
  const merge = key !== null && key === lastKey && now - lastTime < 800;
  lastKey = key;
  lastTime = now;
  set({
    draft: next,
    past: merge ? s.past : [...s.past.slice(-199), s.draft],
    future: [],
    save: s.save === 'saved' ? 'idle' : s.save,
  });
}

export function undo() {
  const s = store;
  if (!s.draft || !s.past.length) return;
  lastKey = null;
  set({ draft: s.past[s.past.length - 1], past: s.past.slice(0, -1), future: [s.draft, ...s.future] });
}

export function redo() {
  const s = store;
  if (!s.draft || !s.future.length) return;
  lastKey = null;
  set({ draft: s.future[0], future: s.future.slice(1), past: [...s.past, s.draft] });
}

/** 改当前这首诗的样式。 */
export function editStyle(name: string, fn: (st: StyleDoc) => StyleDoc, key: string | null = null) {
  edit((d) => {
    const cur = d.styles[name] ?? emptyStyle();
    const next = fn(cur);
    if (next === cur) return d;
    return { ...d, styles: { ...d.styles, [name]: next } };
  }, key);
}

export function editSetting(k: SettingKey, v: any, key: string | null = null) {
  edit((d) => (d.settings[k] === v ? d : { ...d, settings: { ...d.settings, [k]: v } }), key);
}

export function editArrangement(a: Partial<Arrangement>, key: string | null = null) {
  edit((d) => ({ ...d, arrangement: { ...d.arrangement, ...a } }), key);
}

/** 从对象里删掉一个键（返回新对象）。 */
export function without<T extends Record<string, any>>(o: T, k: string): T {
  if (!(k in o)) return o;
  const { [k]: _, ...rest } = o;
  return rest as T;
}

// ───────────── 和已保存的比较 ─────────────

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

function normStyle(st: StyleDoc | undefined): StyleDoc {
  const s = { ...emptyStyle(), ...(st ?? {}) };
  const sort = (o: Record<string, any>) => Object.fromEntries(Object.entries(o).sort(([a], [b]) => a.localeCompare(b)));
  return {
    colors: sort(s.colors),
    offsets: sort(s.offsets),
    sizes: sort(s.sizes),
    opacity: sort(s.opacity),
    ...(s.wallpaper ? { wallpaper: s.wallpaper } : {}),
  };
}

/** 草稿和已保存的差别：哪些诗的样式、哪些设置、整首位置。 */
export function changes(d: Draft | null = store.draft, saved: Draft | null = store.saved) {
  const out = { styles: [] as string[], settings: [] as SettingKey[], arrangement: false, count: 0 };
  if (!d || !saved) return out;
  for (const name of Object.keys(d.styles)) {
    const a = normStyle(d.styles[name]), b = normStyle(saved.styles[name]);
    if (!same(a, b)) {
      out.styles.push(name);
      for (const sec of ['colors', 'offsets', 'sizes', 'opacity'] as const) {
        const keys = new Set([...Object.keys(a[sec]), ...Object.keys(b[sec])]);
        for (const k of keys) if (!same(a[sec][k], b[sec][k])) out.count++;
      }
      if (a.wallpaper !== b.wallpaper) out.count++;
    }
  }
  for (const k of SETTING_KEYS) {
    if (!same(d.settings[k], saved.settings[k])) {
      out.settings.push(k);
      out.count++;
    }
  }
  if (!same(d.arrangement, saved.arrangement)) {
    out.arrangement = true;
    out.count++;
  }
  return out;
}

export function useDirty(): number {
  return useStore((s) => changes(s.draft, s.saved).count);
}
