// 和 muse 的交互：连接、读取全部数据、保存、响应外部修改。

import { changes, draftFromState, get, set } from './store';
import type { State } from './types';

export const call = <T = unknown>(method: string, params?: unknown) => window.muse.call<T>(method, params);

let booted = false;

export async function boot() {
  if (!booted) {
    booted = true;
    set({ theme: await window.muse.theme() });
    window.muse.onTheme((t) => set({ theme: t }));
    window.muse.onEvent(onEvent);
  }
  set({ phase: 'connecting', error: null });
  if (!(await window.muse.connect())) {
    set({ phase: 'offline' });
    return;
  }
  await load();
}

/** 读 muse 的全部数据，草稿重置为已保存的状态。 */
export async function load(keepView = false) {
  try {
    const [state, display] = await Promise.all([call<State>('state'), window.muse.display()]);
    const draft = draftFromState(state);
    const s = get();
    set({
      state,
      display,
      saved: draft,
      draft,
      past: [],
      future: [],
      index: keepView ? Math.min(s.index, state.poems.length - 1) : state.status.index,
      selected: keepView ? s.selected : null,
      charMode: keepView ? s.charMode : false,
      selectedChar: keepView ? s.selectedChar : null,
      phase: 'ready',
      error: null,
    });
  } catch (e) {
    set({ phase: 'error', error: String((e as Error).message ?? e) });
  }
}

export async function startMuse() {
  set({ phase: 'connecting', error: null });
  try {
    await window.muse.start();
    await load();
  } catch (e) {
    set({ phase: 'offline', error: String((e as Error).message ?? e) });
  }
}

function onEvent(msg: { event: string; data?: any }) {
  const s = get();
  if (msg.event === 'disconnected') {
    if (s.phase === 'ready') set({ phase: 'offline', error: 'muse 已经退出，修改还在窗口里，重新启动后可以继续保存。' });
    return;
  }
  // 别处（MCP、命令行）改了文件：没有未保存的修改时直接刷新，有的话保持草稿，保存时以这里为准。
  if (msg.event === 'changed' && msg.data?.cause !== 'self' && s.phase === 'ready') {
    if (changes().count === 0 && s.save !== 'saving') void load(true);
  }
}

let resetTimer: ReturnType<typeof setTimeout> | undefined;

/** 把草稿交给 muse 保存。 */
export async function save() {
  const s = get();
  if (!s.draft || !s.state || s.save === 'saving') return;
  const c = changes();
  if (!c.count) return;
  const params: Record<string, unknown> = {};
  if (c.arrangement) params.arrangement = s.draft.arrangement;
  if (c.settings.length) {
    // 字体选「自动」要写成 "auto"，否则只是删掉 settings.toml 里的项，会回落到 config.toml 写的字体。
    params.settings = Object.fromEntries(
      c.settings.map((k) => [k, k === 'font' ? (s.draft!.settings.font ?? 'auto') : (s.draft!.settings[k] ?? null)]),
    );
  }
  if (c.styles.length) {
    params.styles = Object.fromEntries(c.styles.map((n) => [n, s.draft!.styles[n]]));
  }
  set({ save: 'saving', saveError: null });
  try {
    await call('save', params);
    // 让壁纸显示正在编辑的这首，保存的效果马上能看到。
    void call('goto', { pack: s.state.poems[s.index].name }).catch(() => {});
    set({ saved: s.draft, save: 'saved' });
    clearTimeout(resetTimer);
    resetTimer = setTimeout(() => {
      if (get().save === 'saved') set({ save: 'idle' });
    }, 1200);
  } catch (e) {
    set({ save: 'failed', saveError: String((e as Error).message ?? e) });
  }
}

/** 放弃修改，关窗。 */
export function quit() {
  void window.muse.quit();
}
