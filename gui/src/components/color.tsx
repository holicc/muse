// 颜色编辑：来源阶梯、纯色 / 渐变、取色器（饱和度明度面板、色相、透明度、吸管）、中国传统色。

import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Button, Icon, Segmented, TextField } from './controls';
import { PALETTE, contrastOn, hsvToRgb, mix, paletteName, parseHex, rgbToHsv, toHex } from '../lib/color';
import type { PaintDoc } from '../lib/types';

export function Chip({ color, size = 12, radius = 3 }: { color: string; size?: number; radius?: number }) {
  return (
    <span className="chip" style={{ width: size, height: size, borderRadius: radius, background: color }}>
      <span style={{ borderRadius: radius }} />
    </span>
  );
}

/** 未设置颜色时的斜纹小块。 */
export function HatchChip({ size = 12 }: { size?: number }) {
  return <span className="chip chip-hatch" style={{ width: size, height: size }} />;
}

export type Layer = '单字' | '部件' | '整首' | '全局' | '自动' | '默认';

/**
 * 颜色来源阶梯：实底朱红 = 生效层；划线 = 已设置但被覆盖；斜纹 = 未设置。点任一层可跳到该层编辑。
 */
export function Ladder({
  layers,
  active,
  onJump,
  children,
}: {
  layers: { name: Layer; color: string | null }[];
  active: Layer;
  onJump?: (l: Layer) => void;
  children: React.ReactNode;
}) {
  return (
    <div className="ladder">
      <span className="ladder-title">颜色来源 · 优先级从左到右</span>
      <div className="ladder-row">
        {layers.map((l, i) => {
          const state = l.name === active ? 'active' : l.color ? 'over' : 'unset';
          return (
            <span key={l.name} style={{ display: 'contents' }}>
              {i > 0 && <span className="ladder-sep">›</span>}
              <button type="button" className={`ladder-cell ${state}`} onClick={() => onJump?.(l.name)}>
                {l.color && state !== 'unset' && <Chip color={l.color} size={10} radius={2} />}
                <span className="ladder-name">{l.name}</span>
              </button>
            </span>
          );
        })}
      </div>
      <div className="ladder-note">{children}</div>
    </div>
  );
}

/** 一个颜色值的完整编辑器：取色面板 + HEX/透明度 + 色板。 */
export function Picker({
  value,
  onChange,
  bgLuma,
  showPanel = true,
}: {
  value: string;
  onChange: (hex: string, done: boolean) => void;
  bgLuma: number;
  showPanel?: boolean;
}) {
  const rgba = parseHex(value) ?? { r: 0, g: 0, b: 0, a: 1 };
  const [hsv, setHsv] = useState(() => rgbToHsv(rgba));
  // 外部改了颜色（点色板、撤销）时同步；保留色相，免得灰色时色相跳回 0。
  const lastOut = useRef(value);
  useEffect(() => {
    if (value.toUpperCase() !== lastOut.current.toUpperCase()) {
      const [h, s, v] = rgbToHsv(rgba);
      setHsv(([h0]) => [s === 0 ? h0 : h, s, v]);
      lastOut.current = value;
    }
  }, [value]); // eslint-disable-line react-hooks/exhaustive-deps
  const emit = (h: number, s: number, v: number, a: number, done: boolean) => {
    setHsv([h, s, v]);
    const hex = toHex(hsvToRgb(h, s, v, a));
    lastOut.current = hex;
    onChange(hex, done);
  };
  const [h, s, v] = hsv;
  const a = rgba.a;
  const track = (el: HTMLElement, e: React.PointerEvent, fn: (x: number, y: number, done: boolean) => void) => {
    el.setPointerCapture(e.pointerId);
    const at = (ev: { clientX: number; clientY: number }, done: boolean) => {
      const r = el.getBoundingClientRect();
      fn(
        Math.max(0, Math.min(1, (ev.clientX - r.left) / r.width)),
        Math.max(0, Math.min(1, (ev.clientY - r.top) / r.height)),
        done,
      );
    };
    at(e, false);
    const move = (ev: PointerEvent) => at(ev, false);
    const up = (ev: PointerEvent) => {
      at(ev, true);
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };
  const pick = async () => {
    const ED = (window as any).EyeDropper;
    if (!ED) return;
    try {
      const r = await new ED().open();
      const c = parseHex(r.sRGBHex);
      if (c) emit(...rgbToHsv(c), a, true);
    } catch {
      /* 按了 Esc */
    }
  };
  const solid = toHex({ ...rgba, a: 1 }, false);
  return (
    <div className="picker">
      {showPanel && (
        <>
          <div
            className="sv"
            style={{ background: `linear-gradient(to top,#000,rgba(0,0,0,0)), linear-gradient(to right,#fff,hsl(${h},100%,50%))` }}
            onPointerDown={(e) => track(e.currentTarget, e, (x, y, done) => emit(h, x, 1 - y, a, done))}
          >
            <span className="sv-ring" />
            <span className="sv-cursor" style={{ left: `calc(${s * 100}% - 7px)`, top: `calc(${(1 - v) * 100}% - 7px)`, background: solid }} />
          </div>
          <div className="picker-row">
            <Button kind="secondary" icon="eyedropper" aria-label="吸取颜色" className="btn-square" onClick={pick} />
            <div className="tracks">
              <div className="track hue" onPointerDown={(e) => track(e.currentTarget, e, (x, _y, done) => emit(x * 359.9, s, v, a, done))}>
                <span className="track-thumb" style={{ left: `calc(${(h / 360) * 100}% - 6px)`, background: `hsl(${h},100%,50%)` }} />
              </div>
              <div className="track alpha" onPointerDown={(e) => track(e.currentTarget, e, (x, _y, done) => emit(h, s, v, Math.round(x * 100) / 100, done))}>
                <span className="track-fill" style={{ background: `linear-gradient(to right, rgba(0,0,0,0), ${solid})` }} />
                <span className="track-thumb" style={{ left: `calc(${a * 100}% - 6px)`, background: solid }} />
              </div>
            </div>
          </div>
        </>
      )}
      <div className="picker-row fields">
        <TextField
          prefix="HEX"
          width={130}
          value={toHex(rgba)}
          validate={(t) => parseHex(t) !== null}
          onCommit={(t) => {
            const c = parseHex(t)!;
            emit(...rgbToHsv(c), c.a, true);
          }}
        />
        <AlphaField value={a} onChange={(na, done) => emit(h, s, v, na, done)} />
      </div>
      <Palette value={value} bgLuma={bgLuma} onPick={(hex) => {
        const c = parseHex(hex)!;
        emit(...rgbToHsv(c), a, true);
      }} />
    </div>
  );
}

function AlphaField({ value, onChange }: { value: number; onChange: (a: number, done: boolean) => void }) {
  const [text, setText] = useState<string | null>(null);
  const shown = text ?? String(Math.round(value * 100));
  const commit = () => {
    if (text !== null) {
      const n = parseFloat(text);
      if (Number.isFinite(n)) onChange(Math.max(0, Math.min(100, n)) / 100, true);
    }
    setText(null);
  };
  return (
    <label className="field" style={{ width: 124 }}>
      <span className="field-prefix static">A</span>
      <input
        className="field-value"
        value={shown}
        onFocus={(e) => {
          setText(shown);
          e.currentTarget.select();
        }}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Enter') e.currentTarget.blur();
        }}
      />
      <span className="field-unit">%</span>
    </label>
  );
}

/** 中国传统色；在当前背景上对比度不足的右下角折角（仍可用）。 */
export function Palette({ value, onPick, bgLuma }: { value: string; onPick: (hex: string) => void; bgLuma: number }) {
  const cur = value.toUpperCase().slice(0, 7);
  return (
    <div className="palette">
      <div className="palette-head">
        <span>中国传统色</span>
        <span className="palette-legend">
          <span className="fold-sample" />
          当前背景上对比不足
        </span>
      </div>
      <div className="palette-grid">
        {PALETTE.map((p) => {
          const low = contrastOn(p.hex, bgLuma) < 1.8;
          const on = p.hex === cur;
          return (
            <PaletteCell key={p.hex} name={p.name} hex={p.hex} on={on} low={low} onPick={() => onPick(p.hex)} />
          );
        })}
      </div>
    </div>
  );
}

function PaletteCell({ name, hex, on, low, onPick }: { name: string; hex: string; on: boolean; low: boolean; onPick: () => void }) {
  const [hover, setHover] = useState(false);
  return (
    <button
      type="button"
      className={`swatch${on ? ' on' : ''}`}
      aria-label={`${name} ${hex}`}
      onClick={onPick}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
    >
      <span className="swatch-color" style={{ background: hex }}>
        <span className="swatch-ring" />
        {low && <span className="fold" />}
      </span>
      <span className="swatch-name">{name}</span>
      <AnimatePresence>
        {hover && (
          <motion.span className="tip swatch-tip" initial={{ opacity: 0, y: 4 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.12 }}>
            {name}
            <span className="mono" style={{ opacity: 0.75 }}>{hex}</span>
          </motion.span>
        )}
      </AnimatePresence>
    </button>
  );
}

// ───────────── 渐变 ─────────────


/**
 * 渐变色标条：色标为带尖角的小旗，选中的描朱红；沿阅读方向排列，左端 = 第一个字。
 * 点按色条添加，向下拖离删除。色标均匀分布（文件格式里只记颜色），拖动只改顺序。
 */
export function GradientBar({
  stops,
  selected,
  onSelect,
  onChange,
}: {
  stops: string[];
  selected: number;
  onSelect: (i: number) => void;
  onChange: (stops: string[], done: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const [GRAD_W, setW] = useState(288);
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setW(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const [hoverX, setHoverX] = useState<number | null>(null);
  const [drag, setDrag] = useState<{ i: number; x: number; away: boolean } | null>(null);
  const pos = (i: number, n = stops.length) => (n > 1 ? i / (n - 1) : 0);
  const css = stops.length > 1 ? stops.map((c, i) => `${c} ${pos(i) * 100}%`).join(', ') : `${stops[0]} 0%, ${stops[0]} 100%`;
  const xOf = (e: { clientX: number }) => {
    const r = ref.current!.getBoundingClientRect();
    return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
  };
  const addAt = (t: number) => {
    // 在点到的位置插入一个色标，颜色取那里的渐变色。
    const n = stops.length;
    const k = Math.min(n - 1, Math.max(1, Math.round(t * (n - 1) + 0.5)));
    const left = stops[k - 1], right = stops[k];
    const local = (t - pos(k - 1)) / (pos(k) - pos(k - 1) || 1);
    const next = [...stops.slice(0, k), mix(left, right, Math.max(0, Math.min(1, local))), ...stops.slice(k)];
    onChange(next, true);
    onSelect(k);
  };
  return (
    <div className="grad" ref={box} style={{ height: drag?.away ? 86 : 50 }}>
      <div
        ref={ref}
        className="grad-bar"
        onPointerMove={(e) => setHoverX(xOf(e))}
        onPointerLeave={() => setHoverX(null)}
        onClick={(e) => addAt(xOf(e))}
      >
        <span className="grad-checker" />
        <span className="grad-fill" style={{ background: `linear-gradient(to right, ${css})` }} />
      </div>
      {hoverX !== null && !drag && (
        <span className="grad-add" style={{ left: hoverX * GRAD_W - 8 }}>
          <Icon name="plus" size={12} />
        </span>
      )}
      {stops.map((c, i) => {
        const dragging = drag?.i === i;
        const left = dragging ? drag.x * GRAD_W - 8 : pos(i) * GRAD_W - 8;
        return (
          <motion.span
            key={i + c}
            className={`flag${i === selected ? ' on' : ''}`}
            style={{ left, top: dragging && drag.away ? 62 : 26, opacity: dragging && drag.away ? 0.55 : 1 }}
            initial={{ scale: 0.6, opacity: 0 }}
            animate={{ scale: 1, opacity: dragging && drag.away ? 0.55 : 1 }}
            transition={{ type: 'spring', stiffness: 600, damping: 30 }}
            onPointerDown={(e) => {
              e.stopPropagation();
              e.currentTarget.setPointerCapture(e.pointerId);
              onSelect(i);
              setDrag({ i, x: pos(i), away: false });
            }}
            onPointerMove={(e) => {
              if (!drag || drag.i !== i) return;
              const r = ref.current!.getBoundingClientRect();
              setDrag({ i, x: xOf(e), away: e.clientY - r.bottom > 30 && stops.length > 2 });
            }}
            onPointerUp={() => {
              if (!drag) return;
              if (drag.away) {
                const next = stops.filter((_, k) => k !== i);
                onChange(next, true);
                onSelect(Math.max(0, i - 1));
              } else {
                // 拖过别的色标就换顺序。
                const n = stops.length;
                const to = Math.round(drag.x * (n - 1));
                if (to !== i) {
                  const next = [...stops];
                  const [m] = next.splice(i, 1);
                  next.splice(to, 0, m);
                  onChange(next, true);
                  onSelect(to);
                }
              }
              setDrag(null);
            }}
          >
            <span className="flag-notch" />
            <span className="flag-body" />
            <span className="flag-color" style={{ background: c }} />
            {dragging && !drag.away && <span className="flag-tip">{Math.round(drag.x * 100)}%</span>}
          </motion.span>
        );
      })}
      {drag?.away && (
        <span className="grad-delete">
          <Icon name="trash" size={12} />
          松手删除
        </span>
      )}
    </div>
  );
}

/**
 * 纯色 / 渐变编辑：`paint` 为空时表示未设置（显示 `fallback`）。
 */
export function PaintEditor({
  paint,
  fallback,
  fallbackLabel,
  onChange,
  bgLuma,
  allowGradient = true,
}: {
  paint: PaintDoc | undefined;
  fallback: string;
  fallbackLabel: string;
  onChange: (p: PaintDoc | undefined, done: boolean) => void;
  bgLuma: number;
  allowGradient?: boolean;
}) {
  const stops = paint === undefined ? null : Array.isArray(paint) ? paint : [paint];
  const gradient = Array.isArray(paint) && paint.length > 1;
  const [sel, setSel] = useState(0);
  const cur = stops ? stops[Math.min(sel, stops.length - 1)] : fallback;
  const setStop = (hex: string, done: boolean) => {
    if (!stops) return onChange(hex, done);
    if (!gradient) return onChange(hex, done);
    const next = [...stops];
    next[Math.min(sel, next.length - 1)] = hex;
    onChange(next, done);
  };
  return (
    <div className="paint">
      {allowGradient && (
        <Segmented
          options={[
            { value: 'solid', label: '纯色' },
            { value: 'gradient', label: '渐变' },
          ]}
          value={gradient ? 'gradient' : 'solid'}
          onChange={(m) => {
            // 切回纯色取第一个色标；切到渐变从当前色出发再加一个。
            if (m === 'solid') onChange(stops ? stops[0] : undefined, true);
            else {
              const base = stops ? stops[0] : fallback;
              onChange([base, mix(base, '#FFFFFF', 0.5)], true);
              setSel(1);
            }
          }}
        />
      )}
      {gradient && stops ? (
        <>
          <div className="grad-head">
            <span>
              <Icon name="direction" size={12} />
              沿阅读方向 · 自上而下
            </span>
            <span>点按色条添加 · 拖离删除</span>
          </div>
          <GradientBar stops={stops} selected={Math.min(sel, stops.length - 1)} onSelect={setSel} onChange={onChange} />
          <div className="stop-row">
            <span className="mono stop-index">
              {Math.min(sel, stops.length - 1) + 1} / {stops.length}
            </span>
            <Chip color={cur} size={24} radius={4} />
            <span className="stop-name">{paletteName(cur) ?? toHex(parseHex(cur) ?? { r: 0, g: 0, b: 0, a: 1 })}</span>
            <Button
              kind="icon"
              icon="trash"
              aria-label="删除色标"
              disabled={stops.length <= 2}
              onClick={() => {
                const i = Math.min(sel, stops.length - 1);
                onChange(stops.filter((_, k) => k !== i), true);
                setSel(Math.max(0, i - 1));
              }}
            />
          </div>
          <Picker value={cur} onChange={setStop} bgLuma={bgLuma} />
        </>
      ) : (
        <SolidEditor value={stops ? stops[0] : null} fallback={fallback} fallbackLabel={fallbackLabel} onChange={onChange} bgLuma={bgLuma} />
      )}
    </div>
  );
}

/** 纯色：未设置时显示回落的颜色，点「设定颜色」才展开取色器。 */
function SolidEditor({
  value,
  fallback,
  fallbackLabel,
  onChange,
  bgLuma,
}: {
  value: string | null;
  fallback: string;
  fallbackLabel: string;
  onChange: (p: PaintDoc | undefined, done: boolean) => void;
  bgLuma: number;
}) {
  if (value === null) {
    return (
      <div className="solid-row">
        <Chip color={fallback} size={28} radius={5} />
        <span className="field field-static" style={{ width: 150 }}>
          <span className="field-value muted">
            {fallbackLabel} · {fallback.toUpperCase()}
          </span>
        </span>
        <Button kind="secondary" icon="plus" onClick={() => onChange(fallback.toUpperCase(), true)}>
          设定颜色
        </Button>
      </div>
    );
  }
  const name = paletteName(value);
  const a = parseHex(value)?.a ?? 1;
  return (
    <>
      <div className="solid-row">
        <Chip color={value} size={28} radius={5} />
        <div className="solid-meta">
          <span className="solid-name">{name ?? '自定义'}</span>
          <span className="mono muted">
            {value.toUpperCase().slice(0, 7)} · {Math.round(a * 100)}%
          </span>
        </div>
        <Button kind="ghost" icon="restore" onClick={() => onChange(undefined, true)}>
          清除
        </Button>
      </div>
      <Picker value={value} onChange={(hex, done) => onChange(hex, done)} bgLuma={bgLuma} />
    </>
  );
}
