// 检查器和全局设置共用的小工具。

import { useEffect, useState } from 'react';
import { canvas } from './preview';
import { useStore } from './store';
import type { AutoColor } from './types';

const urls = new Map<string, Promise<string | null>>();

/** 本地图片转成页面能显示的地址（缩略图用）。 */
export function useImageUrl(path: string | null): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!path) return setUrl(null);
    let alive = true;
    if (!urls.has(path)) {
      urls.set(
        path,
        window.muse
          .readFile(path)
          .then((b) => URL.createObjectURL(new Blob([b as BlobPart])))
          .catch(() => null),
      );
    }
    urls.get(path)!.then((u) => alive && setUrl(u));
    return () => {
      alive = false;
    };
  }, [path]);
  return url;
}

/** 画布算出的自动配色依据，草稿或当前诗变了之后刷新。 */
export function useAutoColor(): AutoColor {
  const tick = useStore((s) => s.canvasTick);
  const [a, setA] = useState<AutoColor>({ luma: 0.8, light: true, ink: '#2A2623', on_wallpaper: false });
  useEffect(() => {
    const v = canvas?.autoColor();
    if (v) setA((old) => (JSON.stringify(old) === JSON.stringify(v) ? old : v));
  }, [tick]);
  return a;
}

export const fileName = (p: string | null) => (p ? p.split(/[\\/]/).pop() ?? p : '');
