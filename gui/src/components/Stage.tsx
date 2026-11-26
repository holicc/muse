// 画布区：wasm 渲染的预览，以及悬停框、选中框、虚影框、参考线、浮标等叠加层和全部画布交互。

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { AnimatePresence, animate, motion } from 'motion/react';
import { Canvas, canvas as liveCanvas, setCanvas } from '../lib/preview';
import { call } from '../lib/api';
import { editArrangement, editStyle, get, set, useStore, without } from '../lib/store';
import { partName, sortParts } from '../lib/parts';
import type { CharRect, PartRect } from '../lib/types';
import { Keycap } from './controls';

type Rect = [number, number, number, number];

interface Guide {
  axis: 'x' | 'y';
  at: number; // 画布 CSS 像素
  from: number;
  to: number;
  label: string;
}

interface Drag {
  part: string | null; // null 为整首
  startX: number;
  startY: number;
  dx: number;
  dy: number;
  moved: boolean;
  /** 吸附中的目标（滞回：进入 6px，离开 9px）。 */
  snapX: number | null;
  snapY: number | null;
  guides: Guide[];
  over: boolean;
  origin: Rect | null;
}

const SNAP_IN = 6;
const SNAP_OUT = 9;
/** 安全边距：屏幕高度的 3%（和排版的边距一致）。 */
const SAFE = 0.03;

const spring = (k: number, z: number) => ({ type: 'spring' as const, stiffness: k, damping: 2 * z * Math.sqrt(k), mass: 1 });

export function Stage({ sidebarWidth }: { sidebarWidth: number }) {
  const state = useStore((s) => s.state);
  const draft = useStore((s) => s.draft);
  const index = useStore((s) => s.index);
  const selected = useStore((s) => s.selected);
  const charMode = useStore((s) => s.charMode);
  const selectedChar = useStore((s) => s.selectedChar);
  const display = useStore((s) => s.display);

  const areaRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const cv = useRef<Canvas | null>(null);
  const [size, setSize] = useState({ w: 0, h: 0, area: { w: 0, h: 0 } });
  const [actual, setActual] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rects, setRects] = useState<PartRect[]>([]);
  const [chars, setChars] = useState<CharRect[]>([]);
  const [hover, setHover] = useState<string | null>(null);
  const [hoverChar, setHoverChar] = useState<number | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [hud, setHud] = useState<{ text: string; value: number; key?: string } | null>(null);
  const [fading, setFading] = useState(false);
  const dragRef = useRef<Drag | null>(null);
  const hoverTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const hudTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const intro = useRef<{ speed: number; until: number } | null>(null);
  const shownIndex = useRef<number>(-1);
  const dpr = window.devicePixelRatio || 1;

  const screenW = display?.width ?? 1920;
  const screenH = display?.height ?? 1080;
  const logicalW = screenW / (display?.scale ?? 1);

  // ── 尺寸：按屏幕比例放进可用区域，不超过实际像素 ──
  useLayoutEffect(() => {
    const el = areaRef.current;
    if (!el) return;
    const fit = () => {
      const r = el.getBoundingClientRect();
      const W = window.innerWidth;
      const margin = W >= 1600 ? 64 : W >= 1280 ? 48 + ((W - 1280) / 320) * 8 : W >= 1100 ? 32 + ((W - 1100) / 180) * 8 : 24;
      const availW = r.width - 2 * margin;
      const availH = r.height - 34 - margin;
      const scale = actual ? 1 : Math.min(availW / logicalW, availH / (logicalW * (screenH / screenW)), 1);
      const w = Math.max(1, Math.round(logicalW * scale));
      const h = Math.max(1, Math.round(w * (screenH / screenW)));
      setSize({ w, h, area: { w: r.width, h: r.height } });
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, [logicalW, screenW, screenH, actual, sidebarWidth]);

  const refresh = useCallback(() => {
    const c = cv.current;
    if (!c?.ready) return;
    const ps = c.parts();
    setRects(ps.map((p) => ({ ...p, rect: p.rect.map((v) => v / dpr) as Rect })));
    const keys = sortParts(ps.map((p) => p.part));
    const s = get();
    set({ parts: keys.join() === s.parts.join() ? s.parts : keys, canvasTick: s.canvasTick + 1 });
    setChars(c.chars().map((p) => ({ ...p, rect: p.rect.map((v) => v / dpr) as Rect })));
  }, [dpr]);

  // ── 创建渲染器 ──
  useEffect(() => {
    if (!state || !draft || !canvasRef.current || cv.current || !size.w) return;
    const c = new Canvas(canvasRef.current);
    canvasRef.current.width = Math.round(size.w * dpr);
    canvasRef.current.height = Math.round(size.h * dpr);
    c.init(state, draft, index)
      .then(() => {
        cv.current = c;
        setCanvas(c);
        shownIndex.current = index;
        c.revealAll();
        setReady(true);
        refresh();
      })
      .catch((e) => setError(String(e?.message ?? e)));
  }, [state, draft, index, size.w, size.h, dpr, refresh]);

  useEffect(() => () => setCanvas(null), []);

  // ── 尺寸变化 ──
  useEffect(() => {
    const c = cv.current;
    if (!c || !canvasRef.current || !size.w) return;
    canvasRef.current.width = Math.round(size.w * dpr);
    canvasRef.current.height = Math.round(size.h * dpr);
    c.resize(canvasRef.current.width, canvasRef.current.height);
    c.revealAll();
    refresh();
  }, [size.w, size.h, dpr, ready, refresh]);

  // ── 草稿变了：更新画面 ──
  useEffect(() => {
    const c = cv.current;
    if (!c || !state || !draft || shownIndex.current !== index) return;
    let alive = true;
    c.update(state, draft, index)
      .then(() => {
        if (!alive) return;
        if (!intro.current) c.revealAll();
        refresh();
      })
      .catch((e) => window.muse.log(`更新画布失败: ${e?.message ?? e}`));
    return () => {
      alive = false;
    };
  }, [draft, state, index, ready, refresh]);

  // ── 换字体 ──
  const font = draft?.settings.font ?? null;
  useEffect(() => {
    const c = cv.current;
    if (!c || !ready) return;
    call<{ files: string[]; faces: [number, number][] }>('font.locate', { font })
      .then((f) => (f.files.length ? c.setFont(f) : undefined))
      .then(() => refresh())
      .catch(() => {});
  }, [font, ready, refresh]);

  // ── 换诗：旧的淡出，新的快速晕开（15ms/字，最多 600ms） ──
  useEffect(() => {
    const c = cv.current;
    if (!c || !state || !draft || shownIndex.current === index) return;
    setFading(true);
    const t = setTimeout(async () => {
      await c.update(state, draft, index);
      await c.setPack(state, draft, index);
      shownIndex.current = index;
      c.revealAll();
      const revealed = c.time();
      const n = state.poems[index].lines.join('').length + 8;
      c.seek(0);
      const speed = Math.max(1, revealed / Math.min(0.6, 0.015 * n));
      intro.current = { speed, until: revealed };
      setFading(false);
      refresh();
    }, 180);
    return () => clearTimeout(t);
  }, [index]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── 每帧：粒子照常动，诗停在全部显示完的样子 ──
  useEffect(() => {
    let raf = 0;
    let last = performance.now();
    const loop = (now: number) => {
      const c = cv.current;
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      if (c?.ready) {
        if (intro.current) {
          c.advance(dt * intro.current.speed);
          if (c.time() >= intro.current.until) {
            intro.current = null;
            c.revealAll();
          }
        } else {
          c.advance(dt);
          c.revealAll();
        }
        try {
          c.render();
        } catch {
          /* 画布尺寸为 0 等，下一帧再画 */
        }
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  // ── 坐标换算 ──
  const toScreen = (cssPx: number) => (cssPx / size.h) * screenH;
  const rectOf = (part: string | null): Rect | null => {
    if (part === null) {
      const b = cv.current?.mainBounds();
      return b ? (b.map((v) => v / dpr) as Rect) : null;
    }
    return rects.find((r) => r.part === part)?.rect ?? null;
  };
  const local = (e: { clientX: number; clientY: number }) => {
    const r = canvasRef.current!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top] as const;
  };
  const inside = (r: Rect | null, x: number, y: number) => !!r && x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3];

  const poem = state?.poems[index];
  const name = poem?.name ?? '';

  // ── 吸附 ──
  const snap = (d: Drag, rawDx: number, rawDy: number, alt: boolean) => {
    const o = d.origin;
    const W = size.w, H = size.h;
    if (!o) return { dx: rawDx, dy: rawDy, guides: [] as Guide[], snapX: null, snapY: null, over: false };
    const moved: Rect = [o[0] + rawDx, o[1] + rawDy, o[2] + rawDx, o[3] + rawDy];
    const safe = SAFE * H;
    // 超出安全区的部分按 0.35 倍跟随。
    const damp = (lo: number, hi: number, min: number, max: number, delta: number) => {
      if (lo < min) return delta + (min - lo) * 0.65;
      if (hi > max) return delta - (hi - max) * 0.65;
      return delta;
    };
    let dx = damp(moved[0], moved[2], safe, W - safe, rawDx);
    let dy = damp(moved[1], moved[3], safe, H - safe, rawDy);
    const over = dx !== rawDx || dy !== rawDy;
    const guides: Guide[] = [];
    let snapX: number | null = null, snapY: number | null = null;
    if (!alt && !over) {
      type T = { v: number; label: string; span?: [number, number] };
      const tx: T[] = [], ty: T[] = [];
      if (d.part === null) {
        tx.push({ v: W / 2, label: '画面中线' }, { v: W / 3, label: '左三分线' }, { v: (2 * W) / 3, label: '右三分线' }, { v: safe, label: `安全边距 ${Math.round(toScreen(safe))}` }, { v: W - safe, label: `安全边距 ${Math.round(toScreen(safe))}` });
        ty.push({ v: H / 2, label: '画面中线' }, { v: H / 3, label: '上三分线' }, { v: (2 * H) / 3, label: '下三分线' }, { v: safe, label: `安全边距 ${Math.round(toScreen(safe))}` }, { v: H - safe, label: `安全边距 ${Math.round(toScreen(safe))}` });
      } else {
        for (const r of rects) {
          if (r.part === d.part || r.part === 'watermark') continue;
          const n = partName(r.part, poem);
          const [x0, y0, x1, y1] = r.rect;
          tx.push({ v: x0, label: n, span: [y0, y1] }, { v: (x0 + x1) / 2, label: n, span: [y0, y1] }, { v: x1, label: n, span: [y0, y1] });
          ty.push({ v: y0, label: n, span: [x0, x1] }, { v: (y0 + y1) / 2, label: n, span: [x0, x1] }, { v: y1, label: n, span: [x0, x1] });
        }
        tx.push({ v: W / 2, label: '画面中线' });
        ty.push({ v: H / 2, label: '画面中线' });
      }
      const pick = (edges: number[], targets: T[], prev: number | null, names: string[]) => {
        let best: { diff: number; t: T; e: number } | null = null;
        for (const t of targets)
          edges.forEach((e, ei) => {
            const diff = t.v - e;
            const limit = prev !== null && Math.abs(prev - t.v) < 0.5 ? SNAP_OUT : SNAP_IN;
            if (Math.abs(diff) <= limit && (!best || Math.abs(diff) < Math.abs(best.diff))) best = { diff, t, e: ei };
          });
        return best ? { ...(best as { diff: number; t: T; e: number }), kind: names[(best as { e: number }).e] } : null;
      };
      const ex = pick([moved[0], (moved[0] + moved[2]) / 2, moved[2]], tx, d.snapX, ['左对齐', '居中对齐', '右对齐']);
      const ey = pick([moved[1], (moved[1] + moved[3]) / 2, moved[3]], ty, d.snapY, ['顶端对齐', '垂直居中', '底端对齐']);
      const fy = ey ? ey.diff : 0, fx = ex ? ex.diff : 0;
      if (ex) {
        dx = rawDx + ex.diff;
        snapX = ex.t.v;
        const span = ex.t.span;
        guides.push({
          axis: 'x',
          at: ex.t.v,
          from: span ? Math.min(span[0], moved[1] + fy) - 12 : 0,
          to: span ? Math.max(span[1], moved[3] + fy) + 12 : H,
          label: span ? `${ex.kind} · ${ex.t.label}` : `${ex.t.label} · ${ex.kind}`,
        });
      }
      if (ey) {
        dy = rawDy + ey.diff;
        snapY = ey.t.v;
        const span = ey.t.span;
        guides.push({
          axis: 'y',
          at: ey.t.v,
          from: span ? Math.min(span[0], moved[0] + fx) - 12 : 0,
          to: span ? Math.max(span[1], moved[2] + fx) + 12 : W,
          label: span ? `${ey.kind} · ${ey.t.label}` : `${ey.t.label} · ${ey.kind}`,
        });
      }
    }
    return { dx, dy, guides, snapX, snapY, over };
  };

  /** 拖到哪里合法：整块回到安全区内。 */
  const legal = (d: Drag, dx: number, dy: number) => {
    const o = d.origin;
    if (!o) return [dx, dy];
    const safe = SAFE * size.h;
    const fix = (lo: number, hi: number, min: number, max: number, v: number) =>
      lo < min ? v + (min - lo) : hi > max ? v - (hi - max) : v;
    return [fix(o[0] + dx, o[2] + dx, safe, size.w - safe, dx), fix(o[1] + dy, o[3] + dy, safe, size.h - safe, dy)];
  };

  /** 把拖动结果写进草稿（偏移按屏幕高度归一）。 */
  const commit = (d: Drag, dx: number, dy: number) => {
    const s = get();
    if (!s.state || !s.draft) return;
    if (d.part) {
      const nx = dx / size.h, ny = dy / size.h;
      editStyle(name, (st) => {
        const [ox, oy] = st.offsets[d.part!] ?? [0, 0];
        return { ...st, offsets: { ...st.offsets, [d.part!]: [round4(ox + nx), round4(oy + ny)] } };
      });
    } else {
      const p = cv.current?.placement();
      if (!p) return;
      editArrangement({
        x: round4(Math.max(0, Math.min(1, p[0] + dx / size.w))),
        y: round4(Math.max(0, Math.min(1, p[1] + dy / size.h))),
      });
    }
  };

  const setLiveDrag = (part: string | null, dx: number, dy: number) => cv.current?.drag(part, dx * dpr, dy * dpr);

  // ── 指针 ──
  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || !ready) return;
    const [x, y] = local(e);
    const s = get();
    // 单字模式：点字选字。
    if (s.charMode && s.selected) {
      const hit = chars.find((c) => c.part === s.selected && inside(c.rect, x, y));
      if (hit) {
        set({ selectedChar: hit.index });
        return;
      }
    }
    let part: string | null = null;
    if (s.selected === 'watermark' && inside(rectOf('watermark'), x, y) && !e.shiftKey) part = 'watermark';
    else if (!e.shiftKey) part = cv.current?.partAt(x * dpr, y * dpr) ?? null;
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      /* 合成事件没有真实指针 */
    }
    const d: Drag = { part, startX: x, startY: y, dx: 0, dy: 0, moved: false, snapX: null, snapY: null, guides: [], over: false, origin: rectOf(part) };
    dragRef.current = d;
    if (part !== s.selected) set({ selected: part, charMode: false, selectedChar: null });
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const [x, y] = local(e);
    const d = dragRef.current;
    if (d) {
      const rx = x - d.startX, ry = y - d.startY;
      if (!d.moved && Math.hypot(rx, ry) < 3) return;
      const r = snap(d, rx, ry, e.altKey);
      const next = { ...d, moved: true, dx: r.dx, dy: r.dy, guides: r.guides, snapX: r.snapX, snapY: r.snapY, over: r.over };
      dragRef.current = next;
      setDrag(next);
      setLiveDrag(d.part, r.dx, r.dy);
      return;
    }
    const s = get();
    if (s.charMode && s.selected) {
      const hit = chars.find((c) => c.part === s.selected && inside(c.rect, x, y));
      setHoverChar(hit ? hit.index : null);
    }
    const p = e.shiftKey ? null : cv.current?.partAt(x * dpr, y * dpr) ?? null;
    if (p !== hover) {
      clearTimeout(hoverTimer.current);
      if (p === null) setHover(null);
      else hoverTimer.current = setTimeout(() => setHover(p), 60);
    }
  };

  const finish = (d: Drag) => {
    dragRef.current = null;
    if (!d.moved) {
      setDrag(null);
      return;
    }
    const [lx, ly] = legal(d, d.dx, d.dy);
    if (lx === d.dx && ly === d.dy) {
      commit(d, d.dx, d.dy);
      setDrag(null);
      return;
    }
    // 超出安全区：弹回最近的合法位置。
    const from = { x: d.dx, y: d.dy };
    setDrag({ ...d, guides: [] });
    animate(0, 1, {
      ...spring(420, 0.72),
      onUpdate: (t) => setLiveDrag(d.part, from.x + (lx - from.x) * t, from.y + (ly - from.y) * t),
      onComplete: () => {
        commit(d, lx, ly);
        setDrag(null);
      },
    });
  };

  const onPointerUp = () => {
    const d = dragRef.current;
    if (d) finish(d);
  };

  // Esc 取消拖动：回到原位。
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const d = dragRef.current;
      if (e.key !== 'Escape' || !d?.moved) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      dragRef.current = null;
      const from = { x: d.dx, y: d.dy };
      animate(1, 0, {
        duration: 0.24,
        ease: [0.45, 0, 0.25, 1],
        onUpdate: (t) => setLiveDrag(d.part, from.x * t, from.y * t),
        onComplete: () => {
          setLiveDrag(null, 0, 0);
          setDrag(null);
        },
      });
    };
    window.addEventListener('keydown', key, true);
    return () => window.removeEventListener('keydown', key, true);
  });

  // ── 滚轮：整首字号（以指针为锚点），⌘/Ctrl 只调选中部件 ──
  const showHud = (text: string, value: number, key?: string) => {
    setHud({ text, value, key });
    clearTimeout(hudTimer.current);
    hudTimer.current = setTimeout(() => setHud(null), 800);
  };
  const onWheel = (e: React.WheelEvent) => {
    const s = get();
    if (!s.draft || !ready) return;
    const notches = -Math.sign(e.deltaY) * Math.min(3, Math.max(1, Math.abs(e.deltaY) / 100));
    if ((e.ctrlKey || e.metaKey) && s.selected) {
      const part = s.selected;
      const cur = s.draft.styles[name]?.sizes[part] ?? 1;
      const next = round3(Math.max(0.3, Math.min(3, cur * 1.04 ** notches)));
      editStyle(name, (st) => ({ ...st, sizes: Math.abs(next - 1) < 1e-3 ? without(st.sizes, part) : { ...st.sizes, [part]: next } }), `wheel-size-${part}`);
      showHud(`${partName(part, poem)} · 字号`, next, '⌘');
      return;
    }
    const ts = s.draft.settings.text_scale as number;
    const a = s.draft.arrangement;
    const k0 = a.scale;
    const k1 = Math.max(0.3 / ts, Math.min(3 / ts, k0 * 1.05 ** notches));
    const ratio = k1 / k0;
    const p = liveCanvas?.placement();
    const [x, y] = local(e);
    const patch: Partial<typeof a> = { scale: round3(k1) };
    if (p) {
      const px = x / size.w, py = y / size.h;
      patch.x = round4(Math.max(0, Math.min(1, px + (p[0] - px) * ratio)));
      patch.y = round4(Math.max(0, Math.min(1, py + (p[1] - py) * ratio)));
    }
    editArrangement(patch, 'wheel-scale');
    showHud('整体字号', ts * k1);
  };

  const onDoubleClick = (e: React.MouseEvent) => {
    const [x, y] = local(e);
    const p = cv.current?.partAt(x * dpr, y * dpr) ?? null;
    if (p) {
      // 侧栏收起时双击部件重新展开并定位到它。
      set({ selected: p, sidebar: true, tab: 'inspector' });
    } else {
      setActual((v) => !v);
    }
  };

  // ── 叠加层 ──
  const st = draft?.styles[name];
  const selRect = selected ? rectOf(selected) : null;
  const dragging = drag?.moved ? drag : null;
  const shift = (r: Rect | null, part: string | null): Rect | null => {
    if (!r || !dragging) return r;
    const hit = dragging.part === null ? part !== null && isMainPart(part) : dragging.part === part;
    return hit ? [r[0] + dragging.dx, r[1] + dragging.dy, r[2] + dragging.dx, r[3] + dragging.dy] : r;
  };
  const coloredChars = st ? chars.filter((c) => st.colors[`${c.part}.${c.index + 1}`] !== undefined) : [];
  const wholeRect = dragging && dragging.part === null ? shift(rectOf(null), 'title') : null;

  const statusScale = Math.round((size.w / logicalW) * 100);
  useEffect(() => {
    set({ previewScale: statusScale });
  }, [statusScale]);

  return (
    <div ref={areaRef} className={`stage${actual ? ' actual' : ''}`}>
      {error && (
        <div className="stage-error">
          <b>画布无法显示</b>
          <span>{error}</span>
          <span className="muted">需要 WebGPU；可以在 muse 里照常保存，效果以桌面壁纸为准。</span>
        </div>
      )}
      <div className="screen-wrap" style={{ width: size.w, height: size.h }}>
        <div className="screen" style={{ width: size.w, height: size.h }}>
          <canvas
            ref={canvasRef}
            style={{ width: size.w, height: size.h, opacity: fading ? 0.35 : 1, transform: fading ? 'translateY(8px)' : 'none' }}
            className="screen-canvas"
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerLeave={() => {
              clearTimeout(hoverTimer.current);
              setHover(null);
              setHoverChar(null);
            }}
            onWheel={onWheel}
            onDoubleClick={onDoubleClick}
          />
          <div className="overlays">
            {/* 超出安全区：斜纹 */}
            {dragging?.over && <SafeZones w={size.w} h={size.h} safe={SAFE * size.h} />}

            {/* 拖整首：每个部件一个浅框，外面一个总框 */}
            {dragging && dragging.part === null &&
              rects.filter((r) => isMainPart(r.part)).map((r) => <span key={r.part} className="ov-mini" style={box(shift(r.rect, r.part)!, 2)} />)}
            {wholeRect && <Frame rect={wholeRect} label="整首" kind="select" />}

            {/* 虚影框 */}
            {dragging?.origin && dragging.part !== null && <span className="ov-ghost" style={box(dragging.origin, 3)} />}

            {/* 悬停框 */}
            <AnimatePresence>
              {hover && hover !== selected && !dragging && !charMode && rectOf(hover) && (
                <Frame key={'h' + hover} rect={rectOf(hover)!} label={partName(hover, poem)} kind="hover" />
              )}
            </AnimatePresence>

            {/* 选中框 */}
            {selRect && !(dragging && dragging.part === null) && (
              <Frame rect={shift(selRect, selected)!} label={selected ? partName(selected, poem) : ''} kind="select" lifted={!!dragging} extra={selected === 'watermark' && !dragging ? 'Tab 选中 · 可拖动' : charMode ? '单字上色' : undefined} />
            )}

            {/* 单字：悬停框、已上色的点 */}
            {charMode && selected && hoverChar !== null && (() => {
              const c = chars.find((x) => x.part === selected && x.index === hoverChar);
              return c ? (
                <span className="ov-char" style={box(c.rect, 1)}>
                  <span className="ov-chip ov-char-chip">{c.ch} · 点按上色</span>
                </span>
              ) : null;
            })()}
            {charMode && selected && selectedChar !== null && (() => {
              const c = chars.find((x) => x.part === selected && x.index === selectedChar);
              return c ? <span className="ov-char on" style={box(c.rect, 1)} /> : null;
            })()}
            {coloredChars.map((c) => (
              <span key={`${c.part}.${c.index}`} className="ov-dot" style={{ left: c.rect[2] + 1, top: (c.rect[1] + c.rect[3]) / 2 - 2 }} />
            ))}

            {/* 参考线 */}
            {dragging?.guides.map((g, i) => <GuideLine key={i} g={g} />)}

            {/* 拖动中的位置 */}
            {dragging && (() => {
              const r = dragging.part ? shift(dragging.origin, dragging.part) : wholeRect;
              if (!r) return null;
              const label = dragging.over
                ? '超出安全区 · 阻尼 0.35×'
                : dragging.part
                  ? `X ${signed(toScreen(dragging.dx))} · Y ${signed(toScreen(dragging.dy))}`
                  : `X ${Math.round(toScreen((r[0] + r[2]) / 2))} · Y ${Math.round(toScreen((r[1] + r[3]) / 2))}`;
              return (
                <span className={`ov-chip ov-pos${dragging.over ? ' guide' : ''}`} style={{ left: r[0], top: r[3] + 8 }}>
                  {label}
                </span>
              );
            })()}

            {/* 滚轮浮标 */}
            <AnimatePresence>
              {hud && (
                <motion.div
                  className="hud"
                  initial={{ opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0, transition: { duration: 0.12, ease: [0.22, 1, 0.36, 1] } }}
                  exit={{ opacity: 0, transition: { duration: 0.2 } }}
                >
                  {hud.key && <Keycap>{hud.key}</Keycap>}
                  <span>{hud.text}</span>
                  <span className="mono">{hud.key ? `${Math.round(hud.value * 100)}%` : `×${hud.value.toFixed(2)}`}</span>
                  <span className="hud-bar" style={{ width: hud.key ? 48 : 64 }}>
                    <span style={{ width: `${Math.min(100, ((hud.value - 0.3) / 2.7) * 100)}%` }} />
                  </span>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </div>
      </div>
      {!ready && !error && <div className="stage-loading">正在准备画布…</div>}
    </div>
  );
}

function isMainPart(p: string) {
  return !['masthead', 'issue', 'source', 'watermark'].includes(p);
}

const round4 = (v: number) => Math.round(v * 10000) / 10000;
const round3 = (v: number) => Math.round(v * 1000) / 1000;
const signed = (v: number) => (v >= 0 ? '+' : '') + Math.round(v);

function box(r: Rect, pad: number): React.CSSProperties {
  return { left: r[0] - pad, top: r[1] - pad, width: r[2] - r[0] + 2 * pad, height: r[3] - r[1] + 2 * pad };
}

function Frame({ rect, label, kind, lifted, extra }: { rect: Rect; label: string; kind: 'hover' | 'select'; lifted?: boolean; extra?: string }) {
  const pad = 4;
  const style = box(rect, pad);
  if (kind === 'hover') {
    return (
      <motion.span
        className="ov-hover"
        style={style}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1, transition: { duration: 0.09, ease: 'linear' } }}
        exit={{ opacity: 0, transition: { duration: 0.12, ease: 'linear' } }}
      >
        <motion.span
          className="ov-chip ov-label"
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0, transition: { delay: 0.04, duration: 0.12, ease: [0.22, 1, 0.36, 1] } }}
        >
          {label}
        </motion.span>
      </motion.span>
    );
  }
  return (
    <motion.span
      className={`ov-select${lifted ? ' lifted' : ''}`}
      initial={false}
      animate={style as any}
      transition={lifted ? { duration: 0 } : { duration: 0.22, ease: [0.45, 0, 0.25, 1] }}
    >
      <i className="cm tl" />
      <i className="cm tr" />
      <i className="cm bl" />
      <i className="cm br" />
      {label && (
        <span className="ov-labels">
          <span className="ov-chip sel">{label}</span>
          {extra && <span className="ov-chip">{extra}</span>}
        </span>
      )}
    </motion.span>
  );
}

function GuideLine({ g }: { g: Guide }) {
  const style: React.CSSProperties =
    g.axis === 'x'
      ? { left: g.at, top: g.from, height: g.to - g.from, width: 1 }
      : { top: g.at, left: g.from, width: g.to - g.from, height: 1 };
  return (
    <motion.span
      className="ov-guide"
      style={{ ...style, transformOrigin: 'center' }}
      initial={{ scaleX: g.axis === 'y' ? 0 : 1, scaleY: g.axis === 'x' ? 0 : 1 }}
      animate={{ scaleX: 1, scaleY: 1, transition: { duration: 0.12, ease: [0.22, 1, 0.36, 1] } }}
    >
      <span className={`ov-chip guide ov-guide-label ${g.axis}`}>{g.label}</span>
    </motion.span>
  );
}

function SafeZones({ w, h, safe }: { w: number; h: number; safe: number }) {
  return (
    <>
      <span className="ov-unsafe" style={{ left: 0, top: 0, width: w, height: safe }} />
      <span className="ov-unsafe" style={{ left: 0, top: h - safe, width: w, height: safe }} />
      <span className="ov-unsafe" style={{ left: 0, top: safe, width: safe, height: h - 2 * safe }} />
      <span className="ov-unsafe" style={{ left: w - safe, top: safe, width: safe, height: h - 2 * safe }} />
    </>
  );
}

/** 当前页的部件，按设计稿顺序（检查器列表、Tab 切换用）。 */
export function currentParts(): string[] {
  return get().parts;
}
