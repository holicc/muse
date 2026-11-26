// 部件名、部件顺序、颜色来源的计算。

import type { Draft } from './store';
import type { PaintDoc, Poem, State, StyleDoc } from './types';
import { SEAL_DEFAULT } from './color';

/** 部件的中文名。 */
export function partName(key: string, poem?: Poem): string {
  const m = /^line\.(\d+)$/.exec(key);
  if (m) return `正文 · 第 ${m[1]} ${poem?.orientation === 'horizontal' ? '行' : '列'}`;
  return (
    {
      title: '标题',
      byline: '署名',
      seal: '印章',
      rule: '分栏细线',
      masthead: '刊头',
      issue: '期号',
      source: '出处',
      watermark: '水印',
    } as Record<string, string>
  )[key] ?? key;
}

const ORDER = ['title', 'line', 'byline', 'seal', 'rule', 'masthead', 'issue', 'source', 'watermark'];

/** 按设计稿的顺序排部件：标题、正文各列、署名、印章、细线、刊头、期号、出处、水印。 */
export function sortParts(keys: string[]): string[] {
  const rank = (k: string) => {
    const base = k.startsWith('line.') ? 'line' : k;
    return ORDER.indexOf(base) * 1000 + (k.startsWith('line.') ? parseInt(k.slice(5)) : 0);
  };
  return [...new Set(keys)].sort((a, b) => rank(a) - rank(b));
}

/** 整首颜色作用于标题、正文、署名。 */
export function inPoemColor(part: string) {
  return part === 'title' || part === 'byline' || part.startsWith('line.');
}

export const firstColor = (p: PaintDoc | undefined): string | null =>
  p === undefined ? null : Array.isArray(p) ? p[0] : p;

/** 全局字色（固定颜色时）。 */
export function globalColor(d: Draft): string | null {
  return d.settings.text_color === 'auto' ? null : (d.settings.text_color as string);
}

/** 部件的显示色：部件色 > 整首色 > 全局 > 自动（印章为默认朱红）。 */
export function partColor(st: StyleDoc, part: string, d: Draft, auto: string): string {
  const own = firstColor(st.colors[part]);
  if (own) return own;
  if (part === 'seal') return SEAL_DEFAULT;
  if (inPoemColor(part)) {
    const poem = firstColor(st.colors.color);
    if (poem) return poem;
  }
  return globalColor(d) ?? auto;
}

/** 部件里有几个字单独上了色。 */
export function charColorCount(st: StyleDoc, part: string): number {
  const re = new RegExp(`^${part.replace('.', '\\.')}\\.\\d+$`);
  return Object.keys(st.colors).filter((k) => re.test(k)).length;
}

/** 是不是单个字的颜色键（如 title.1、line.2.3）。 */
export function isCharKey(k: string) {
  return /^(title|byline|seal|rule|masthead|issue|source|watermark)\.\d+$/.test(k) || /^line\.\d+\.\d+$/.test(k);
}

export function allCharColorCount(st: StyleDoc): number {
  return Object.keys(st.colors).filter(isCharKey).length;
}

export function poemOf(state: State, index: number): Poem {
  return state.poems[Math.max(0, Math.min(index, state.poems.length - 1))];
}
