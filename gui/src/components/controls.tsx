// 设计稿「组件清单」里的基础控件。全部自绘：矩形、圆角、1px 线、实色和少量阴影。

import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { AnimatePresence, motion } from 'motion/react';

// ───────────── 图标（16×16，1.5 线宽） ─────────────

const P: Record<string, ReactNode> = {
  reset: (
    <>
      <path d="M3.2 8a4.8 4.8 0 1 0 1.4-3.4" />
      <path d="M3 2.6v2.6h2.6" />
    </>
  ),
  sidebar: (
    <>
      <rect x="2" y="3" width="12" height="10" rx="1.5" />
      <path d="M10 3v10" />
    </>
  ),
  down: <path d="M4 6.2l4 4 4-4" />,
  up: <path d="M4 9.8l4-4 4 4" />,
  left: <path d="M9.8 4l-4 4 4 4" />,
  right: <path d="M6.2 4l4 4-4 4" />,
  restore: (
    <>
      <path d="M4 6.5h6a3 3 0 0 1 0 6H6" />
      <path d="M6.5 4 4 6.5 6.5 9" />
    </>
  ),
  plus: <path d="M8 3.5v9M3.5 8h9" />,
  trash: <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5" />,
  eyedropper: (
    <>
      <path d="M9.6 3.4l3 3" />
      <path d="M11 2.2a1.6 1.6 0 0 1 2.8 1.1l-.1.5-1.6 1.6-2.4-2.4 1.6-1.6z" />
      <path d="M10 4.8 4 10.8 3.4 13l2.2-.6 6-6" />
    </>
  ),
  lock: (
    <>
      <rect x="3.5" y="7" width="9" height="6.5" rx="1.2" />
      <path d="M5.5 7V5.2a2.5 2.5 0 0 1 5 0V7" />
    </>
  ),
  layers: (
    <>
      <path d="M8 2.5 14 5.5 8 8.5 2 5.5z" />
      <path d="M2 8.2l6 3 6-3" />
      <path d="M2 10.8l6 3 6-3" />
    </>
  ),
  grid: <path d="M5.5 2v12M10.5 2v12M2 5.5h12M2 10.5h12" />,
  check: <path d="M3.5 8.3l3 3 6-6.6" />,
  arrow: <path d="M3 8h10M9 4l4 4-4 4" />,
  image: (
    <>
      <rect x="2" y="3" width="12" height="10" rx="1.5" />
      <circle cx="6" cy="6.5" r="1.2" />
      <path d="M2.5 12l3.5-3.5 2.5 2.5 2-2 3 3" />
    </>
  ),
  direction: <path d="M8 2.5v11M4.5 10 8 13.5 11.5 10" />,
};

export function Icon({ name, size = 16, style }: { name: keyof typeof P | string; size?: number; style?: CSSProperties }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={{ display: 'block', flex: 'none', ...style }}
    >
      {P[name]}
    </svg>
  );
}

// ───────────── 按钮 ─────────────

type Kind = 'primary' | 'secondary' | 'ghost' | 'icon';

export function Button({
  kind = 'secondary',
  icon,
  children,
  size = 28,
  ...rest
}: {
  kind?: Kind;
  icon?: string;
  size?: 24 | 28 | 32;
  children?: ReactNode;
} & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const cls = ['btn', `btn-${kind}`, `btn-h${size}`, icon && children ? 'btn-with-icon' : ''].join(' ');
  return (
    <button type="button" className={cls + (rest.className ? ' ' + rest.className : '')} {...rest}>
      {icon && <Icon name={icon} />}
      {children !== undefined && <span>{children}</span>}
    </button>
  );
}

export function Keycap({ children }: { children: ReactNode }) {
  return <span className="keycap">{children}</span>;
}

// ───────────── 分段控件 ─────────────

export function Segmented<T extends string | number>({
  options,
  value,
  onChange,
  height = 28,
  locked = false,
  className,
}: {
  options: { value: T; label: ReactNode; icon?: string }[];
  value: T;
  onChange?: (v: T) => void;
  height?: 26 | 28 | 32;
  locked?: boolean;
  className?: string;
}) {
  return (
    <div
      className={`seg seg-h${height}${locked ? ' seg-locked' : ''}${className ? ' ' + className : ''}`}
      role="radiogroup"
    >
      {options.map((o) => (
        <button
          key={String(o.value)}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          aria-disabled={locked || undefined}
          className={o.value === value ? 'on' : ''}
          onClick={() => !locked && onChange?.(o.value)}
        >
          {o.icon && <Icon name={o.icon} size={14} />}
          {o.label}
        </button>
      ))}
    </div>
  );
}

// ───────────── 滑条 ─────────────

export function Slider({
  value,
  min,
  max,
  onChange,
  width = 112,
  step,
}: {
  value: number;
  min: number;
  max: number;
  onChange: (v: number, done: boolean) => void;
  width?: number;
  step?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState(false);
  const t = Math.max(0, Math.min(1, (value - min) / (max - min)));
  const fromX = (x: number) => {
    const r = ref.current!.getBoundingClientRect();
    let v = min + Math.max(0, Math.min(1, (x - r.left) / r.width)) * (max - min);
    if (step) v = Math.round(v / step) * step;
    return v;
  };
  return (
    <div
      ref={ref}
      className={`slider${drag ? ' dragging' : ''}`}
      style={{ width }}
      role="slider"
      tabIndex={0}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        setDrag(true);
        onChange(fromX(e.clientX), false);
      }}
      onPointerMove={(e) => drag && onChange(fromX(e.clientX), false)}
      onPointerUp={(e) => {
        setDrag(false);
        onChange(fromX(e.clientX), true);
      }}
      onKeyDown={(e) => {
        const d = (step ?? (max - min) / 100) * (e.shiftKey ? 10 : 1);
        if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') onChange(Math.max(min, value - d), true);
        else if (e.key === 'ArrowRight' || e.key === 'ArrowUp') onChange(Math.min(max, value + d), true);
        else return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      <div className="slider-track" />
      <div className="slider-fill" style={{ width: `${t * 100}%` }} />
      <div className="slider-thumb" style={{ left: `calc(${t * 100}% - ${drag ? 8 : 7}px)` }} />
    </div>
  );
}

// ───────────── 数值框 ─────────────

/**
 * 数值等宽。拖动前缀标签可快速调值（⇧ ×10，Alt ×0.1），聚焦时滚轮 ±1，回车确认。
 * `format` / `parse` 处理显示和输入（如 HEX）。
 */
export function NumberField({
  value,
  onChange,
  prefix,
  unit,
  width = 96,
  step = 1,
  min = -Infinity,
  max = Infinity,
  format = (v: number) => String(Math.round(v)),
  invalid,
}: {
  value: number;
  onChange: (v: number, done: boolean) => void;
  prefix?: string;
  unit?: string;
  width?: number;
  step?: number;
  min?: number;
  max?: number;
  format?: (v: number) => string;
  invalid?: boolean;
}) {
  const [text, setText] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const clamp = (v: number) => Math.max(min, Math.min(max, v));
  const commit = () => {
    if (text === null) return;
    const v = parseFloat(text.replace(/^\+/, ''));
    if (Number.isFinite(v)) onChange(clamp(v), true);
    setText(null);
  };
  const bad = invalid || (text !== null && !Number.isFinite(parseFloat(text.replace(/^\+/, ''))));
  return (
    <label className={`field${dragging ? ' dragging' : ''}${bad ? ' invalid' : ''}`} style={{ width }}>
      {prefix && (
        <span
          className="field-prefix"
          onPointerDown={(e) => {
            e.preventDefault();
            e.currentTarget.setPointerCapture(e.pointerId);
            const start = e.clientX;
            const v0 = value;
            setDragging(true);
            const move = (ev: PointerEvent) => {
              const k = ev.shiftKey ? 10 : ev.altKey ? 0.1 : 1;
              onChange(clamp(v0 + Math.round((ev.clientX - start) / 2) * step * k), false);
            };
            const up = (ev: PointerEvent) => {
              setDragging(false);
              window.removeEventListener('pointermove', move);
              window.removeEventListener('pointerup', up);
              const k = ev.shiftKey ? 10 : ev.altKey ? 0.1 : 1;
              onChange(clamp(v0 + Math.round((ev.clientX - start) / 2) * step * k), true);
            };
            window.addEventListener('pointermove', move);
            window.addEventListener('pointerup', up);
          }}
        >
          {prefix}
        </span>
      )}
      <input
        className="field-value"
        value={text ?? format(value)}
        onFocus={(e) => {
          setText(format(value));
          e.currentTarget.select();
        }}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Enter') {
            commit();
            e.currentTarget.blur();
          } else if (e.key === 'Escape') {
            setText(null);
            e.currentTarget.blur();
          } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            e.preventDefault();
            const v = clamp(value + (e.key === 'ArrowUp' ? 1 : -1) * step * (e.shiftKey ? 10 : 1));
            onChange(v, true);
            setText(format(v));
          }
        }}
        onWheel={(e) => {
          if (document.activeElement !== e.currentTarget) return;
          const v = clamp(value + (e.deltaY < 0 ? 1 : -1) * step);
          onChange(v, true);
          setText(format(v));
        }}
      />
      {unit && <span className="field-unit">{unit}</span>}
    </label>
  );
}

/** 文字框（HEX 等），样式和数值框一致。 */
export function TextField({
  value,
  onCommit,
  prefix,
  width = 104,
  validate,
}: {
  value: string;
  onCommit: (v: string) => void;
  prefix?: string;
  width?: number;
  validate?: (v: string) => boolean;
}) {
  const [text, setText] = useState<string | null>(null);
  const bad = text !== null && validate ? !validate(text) : false;
  const commit = () => {
    if (text !== null && (!validate || validate(text))) onCommit(text);
    setText(null);
  };
  return (
    <label className={`field${bad ? ' invalid' : ''}`} style={{ width }}>
      {prefix && <span className="field-prefix static">{prefix}</span>}
      <input
        className="field-value"
        value={text ?? value}
        spellCheck={false}
        onFocus={(e) => {
          setText(value);
          e.currentTarget.select();
        }}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Enter') e.currentTarget.blur();
          if (e.key === 'Escape') {
            setText(null);
            e.currentTarget.blur();
          }
        }}
      />
    </label>
  );
}

// ───────────── 开关 ─────────────

export function Switch({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button type="button" role="switch" aria-checked={on} aria-label={label} className={`switch${on ? ' on' : ''}`} onClick={() => onChange(!on)}>
      <motion.span className="switch-thumb" layout transition={{ type: 'spring', stiffness: 600, damping: 2 * 0.85 * Math.sqrt(600) }} />
    </button>
  );
}

// ───────────── 提示 ─────────────

export function Tooltip({ text, children, side = 'top' }: { text: ReactNode; children: ReactNode; side?: 'top' | 'right' }) {
  const [show, setShow] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  return (
    <span
      className="tip-anchor"
      onPointerEnter={() => {
        timer.current = setTimeout(() => setShow(true), 300);
      }}
      onPointerLeave={() => {
        clearTimeout(timer.current);
        setShow(false);
      }}
    >
      {children}
      <AnimatePresence>
        {show && (
          <motion.span
            className={`tip tip-${side}`}
            initial={{ opacity: 0, y: side === 'top' ? 4 : 0 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, transition: { duration: 0.1, ease: [0.4, 0, 1, 1] } }}
            transition={{ duration: 0.12, ease: [0.22, 1, 0.36, 1] }}
          >
            {text}
          </motion.span>
        )}
      </AnimatePresence>
    </span>
  );
}

/** 改过的值旁边的「恢复默认」按钮；没改过时留出同样宽的空位。 */
export function ResetSlot({ modified, onReset, tip }: { modified: boolean; onReset: () => void; tip: string }) {
  return (
    <span className="reset-slot">
      <AnimatePresence>
        {modified && (
          <motion.span initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.12 }}>
            <Tooltip text={tip} side="top">
              <Button kind="icon" size={24} icon="restore" aria-label="恢复默认" onClick={onReset} />
            </Tooltip>
          </motion.span>
        )}
      </AnimatePresence>
    </span>
  );
}

// ───────────── 弹层 ─────────────

/** 锚定在触发元素下方的弹层；点外面或 Esc 关闭。 */
export function Popover({
  open,
  onClose,
  anchor,
  children,
  width,
  align = 'left',
  role = 'dialog',
}: {
  open: boolean;
  onClose: () => void;
  anchor: HTMLElement | null;
  children: ReactNode;
  width: number;
  align?: 'left' | 'right';
  role?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      if (ref.current?.contains(e.target as Node) || anchor?.contains(e.target as Node)) return;
      onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('pointerdown', down, true);
    window.addEventListener('keydown', key, true);
    return () => {
      window.removeEventListener('pointerdown', down, true);
      window.removeEventListener('keydown', key, true);
    };
  }, [open, onClose, anchor]);
  const r = anchor?.getBoundingClientRect();
  const style: CSSProperties = r
    ? {
        top: r.bottom + 6,
        width,
        ...(align === 'left' ? { left: Math.max(8, r.left) } : { right: Math.max(8, window.innerWidth - r.right) }),
      }
    : { width };
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          ref={ref}
          className="popover"
          role={role}
          style={{ ...style, transformOrigin: align === 'left' ? 'top left' : 'top right' }}
          initial={{ opacity: 0, scale: 0.98 }}
          animate={{ opacity: 1, scale: 1, transition: { duration: 0.16, ease: [0.22, 1, 0.36, 1] } }}
          exit={{ opacity: 0, scale: 0.98, transition: { duration: 0.1, ease: [0.4, 0, 1, 1] } }}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** 小节标题：编号 · 名称 · 细线 · 右侧附注。 */
export function Section({ no, title, aside, children }: { no: string; title: string; aside?: ReactNode; children: ReactNode }) {
  return (
    <section className="section">
      <div className="section-head">
        <span className="section-no">{no}</span>
        <span className="section-title">{title}</span>
        <span className="section-line" />
        {aside}
      </div>
      {children}
    </section>
  );
}

export function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="row">
      <span className="row-label">{label}</span>
      <div className="row-body">{children}</div>
    </div>
  );
}

export function Hint({ children }: { children: ReactNode }) {
  return <div className="hint">{children}</div>;
}
