// 全局设置：默认背景、字体、版式、字号倍数、全局字色、节奏、性能与系统。
// 改动写进 settings.toml，覆盖 config.toml 的同名项；config.toml 不会被改。

import { useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Hint, Icon, NumberField, Row, Section, Segmented, Slider, Switch } from './controls';
import { Picker } from './color';
import { BgChoices } from './Inspector';
import { call } from '../lib/api';
import { editSetting, set, useStore } from '../lib/store';
import { fileName, useAutoColor } from '../lib/hooks';
import { canvas } from '../lib/preview';

export function GlobalPanel() {
  const state = useStore((s) => s.state)!;
  const draft = useStore((s) => s.draft)!;
  const display = useStore((s) => s.display);
  const index = useStore((s) => s.index);
  const auto = useAutoColor();
  const s = draft.settings;
  const own = state.poems.filter((p) => draft.styles[p.name]?.wallpaper !== undefined);
  const lockedPoems = state.poems.filter((p) => p.style !== null);
  const color = s.text_color as string;
  const totalChars = state.poems[0]?.lines.join('').length ?? 20;
  return (
    <div className="panel">
      <Section no="01" title="默认背景" aside={<span className="section-aside">单首可覆盖</span>}>
        <BgChoices value={s.wallpaper} desktop={state.wallpaper.desktop} preset={state.poems[index]?.particles ?? null} onChange={(w) => editSetting('wallpaper', w)} />
        <Hint>
          {s.wallpaper === 'auto'
            ? state.wallpaper.desktop
              ? `已识别：${display?.label ?? '主显示器'} · ${fileName(state.wallpaper.desktop)} · ${display?.width ?? ''}×${display?.height ?? ''}${auto.on_wallpaper ? ` · 诗所在区域亮度 ${Math.round(auto.luma * 100)}%` : ''}`
              : '没有识别到当前桌面壁纸，用诗词包自带的背景。'
            : s.wallpaper === 'pack'
              ? '每首诗用诗词包自带的背景。'
              : `图片：${s.wallpaper}`}
        </Hint>
        {own.length > 0 && (
          <div className="info-bar">
            <span>
              <Icon name="image" size={14} />
              {own.length} 首诗单独设置了背景，不受此处影响
            </span>
            <a
              href="#"
              onClick={(e) => {
                e.preventDefault();
                const i = state.poems.indexOf(own[0]);
                set({ index: i, tab: 'inspector', selected: null });
              }}
            >
              查看 ›
            </a>
          </div>
        )}
      </Section>
      <Section no="02" title="字体">
        <FontPicker value={s.font} />
      </Section>
      <Section no="03" title="版式">
        <Segmented
          options={[
            { value: 'magazine', label: '杂志风' },
            { value: 'classic', label: '传统' },
          ]}
          value={s.style}
          onChange={(v) => editSetting('style', v)}
        />
        {lockedPoems.length > 0 && (
          <Hint>
            <span className="inline-icon">
              <Icon name="lock" size={12} />
            </span>
            部分诗由诗词包指定版式，不受此设置影响
          </Hint>
        )}
      </Section>
      <Section no="04" title="字号倍数">
        <div className="scale-row">
          <div className="scale-slider">
            <Slider value={s.text_scale} min={0.3} max={3} width={196} onChange={(v, done) => editSetting('text_scale', round2(v), done ? null : 'text_scale')} />
            <div className="ticks mono">
              <span>0.3</span>
              <span style={{ left: `${((1 - 0.3) / 2.7) * 100}%` }}>1.0</span>
              <span>3.0</span>
            </div>
          </div>
          <NumberField value={s.text_scale} width={68} unit="×" step={0.01} min={0.3} max={3} format={(v) => v.toFixed(2)} onChange={(v, done) => editSetting('text_scale', round2(v), done ? null : 'text_scale')} />
        </div>
      </Section>
      <Section no="05" title="全局字色">
        <Segmented
          options={[
            { value: 'auto', label: '自动' },
            { value: 'fixed', label: '固定颜色' },
          ]}
          value={color === 'auto' ? 'auto' : 'fixed'}
          onChange={(v) => editSetting('text_color', v === 'auto' ? 'auto' : auto.ink.toUpperCase())}
        />
        {color === 'auto' ? (
          <Hint>
            按诗所在区域的明暗自动选择 · 当前 {auto.light ? '偏亮 → 墨色' : '偏暗 → 月白'} <span className="mono">{auto.ink.toUpperCase()}</span>
          </Hint>
        ) : (
          <Picker value={color} bgLuma={auto.luma} onChange={(hex, done) => editSetting('text_color', hex.slice(0, 7), done ? null : 'text_color')} />
        )}
      </Section>
      <Section no="06" title="节奏">
        <Row label="每首停留">
          <NumberField value={s.hold} width={72} unit="秒" min={0} max={3600} onChange={(v, done) => editSetting('hold', v, done ? null : 'hold')} />
          <span className="row-note">之后切换下一首</span>
        </Row>
        <Row label="逐字间隔">
          <NumberField value={s.char_interval * 1000} width={84} unit="毫秒" min={20} max={10000} step={10} onChange={(v, done) => editSetting('char_interval', round3(v / 1000), done ? null : 'char_interval')} />
          <span className="row-note">整首约 {(totalChars * s.char_interval + 1.8).toFixed(1)} 秒写完</span>
        </Row>
      </Section>
      <Section no="07" title="性能与系统">
        <Fps label="插电" value={s.fps} presets={[30, 60, 120]} onChange={(v, done) => editSetting('fps', v, done ? null : 'fps')} />
        <Fps label="电池" value={s.battery_fps} presets={[0, 15, 30, 60]} onChange={(v, done) => editSetting('battery_fps', v, done ? null : 'battery_fps')} />
        <Hint>
          {s.battery_fps === 0 && <>使用电池时停止动画。</>}
          默认：插电 30 · 电池 15
        </Hint>
        <div className="switch-row">
          <span>全屏应用在前台时暂停</span>
          <Switch label="全屏应用在前台时暂停" on={s.pause_on_fullscreen} onChange={(v) => editSetting('pause_on_fullscreen', v)} />
        </div>
        <div className="switch-row">
          <span>在托盘显示图标</span>
          <Switch label="在托盘显示图标" on={s.tray} onChange={(v) => editSetting('tray', v)} />
        </div>
        {state.warnings.length > 0 && <Hint>{state.warnings.join('；')}</Hint>}
      </Section>
    </div>
  );
}

/** 帧率：预设档位 + 自定义（1–120）；电池档的 0 显示为「暂停」。 */
function Fps({ label, value, presets, onChange }: { label: string; value: number; presets: number[]; onChange: (v: number, done: boolean) => void }) {
  const [custom, setCustom] = useState(!presets.includes(value));
  useEffect(() => {
    if (!presets.includes(value)) setCustom(true);
  }, [value]); // eslint-disable-line react-hooks/exhaustive-deps
  const seg = custom ? 'custom' : String(value);
  return (
    <div className="fps">
      <div className="fps-row">
        <span className="fps-label">{label}</span>
        <Segmented
          height={26}
          className="grow"
          options={[...presets.map((p) => ({ value: String(p), label: p === 0 ? '暂停' : String(p) })), { value: 'custom', label: '自定义' }]}
          value={seg}
          onChange={(v) => {
            if (v === 'custom') {
              setCustom(true);
              if (value === 0) onChange(20, true);
            } else {
              setCustom(false);
              onChange(Number(v), true);
            }
          }}
        />
      </div>
      <AnimatePresence initial={false}>
        {custom && (
          <motion.div className="fps-custom" initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={{ duration: 0.22, ease: [0.45, 0, 0.25, 1] }}>
            <NumberField value={value} width={84} unit="fps" min={1} max={120} onChange={onChange} />
            <span className="row-note">1 – 120</span>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

/** 字体：列出系统里能显示中文的字体，悬停即时预览到画布，移开还原。 */
function FontPicker({ value }: { value: string | null }) {
  const [open, setOpen] = useState(false);
  const [fonts, setFonts] = useState<string[] | null>(null);
  const [q, setQ] = useState('');
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (open && !fonts) call<{ families: string[] }>('fonts').then((r) => setFonts(r.families)).catch(() => setFonts([]));
  }, [open, fonts]);
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => !ref.current?.contains(e.target as Node) && close();
    const key = (e: KeyboardEvent) => e.key === 'Escape' && (e.stopPropagation(), close());
    window.addEventListener('pointerdown', down, true);
    window.addEventListener('keydown', key, true);
    return () => {
      window.removeEventListener('pointerdown', down, true);
      window.removeEventListener('keydown', key, true);
    };
  });
  const list = useMemo(() => (fonts ?? []).filter((f) => f.toLowerCase().includes(q.toLowerCase())), [fonts, q]);
  const preview = (f: string | null) =>
    call<{ files: string[]; faces: [number, number][] }>('font.locate', { font: f })
      .then((r) => (r.files.length ? canvas?.setFont(r) : undefined))
      .catch(() => {});
  const close = () => {
    setOpen(false);
    void preview(value);
  };
  const label = value ?? '自动（中文宋体）';
  return (
    <div className="font-picker" ref={ref}>
      <button type="button" className={`dropdown${open ? ' open' : ''}`} onClick={() => (open ? close() : setOpen(true))}>
        <span className="dropdown-label">
          {label}
          <span className="dropdown-sample" style={{ fontFamily: value ? `"${value}"` : undefined }}>
            空山新雨后
          </span>
        </span>
        <Icon name={open ? 'up' : 'down'} size={14} />
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            className="menu font-menu"
            initial={{ opacity: 0, scale: 0.98 }}
            animate={{ opacity: 1, scale: 1, transition: { duration: 0.16, ease: [0.22, 1, 0.36, 1] } }}
            exit={{ opacity: 0, transition: { duration: 0.1 } }}
          >
            <input className="font-search" autoFocus placeholder={`搜索系统字体（共 ${fonts?.length ?? '…'} 款）`} value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
            <div className="font-group">适合诗词</div>
            <div className="font-list" onPointerLeave={() => void preview(value)}>
              <button type="button" className={`font-item${value === null ? ' on' : ''}`} onPointerEnter={() => void preview(null)} onClick={() => (editSetting('font', null), setOpen(false))}>
                <span className="font-names">
                  <span className="font-name">自动</span>
                  <span className="font-family mono">中文宋体优先</span>
                </span>
              </button>
              {list.map((f) => (
                <button
                  key={f}
                  type="button"
                  className={`font-item${f === value ? ' on' : ''}`}
                  onPointerEnter={() => void preview(f)}
                  onClick={() => {
                    editSetting('font', f);
                    setOpen(false);
                  }}
                >
                  <span className="font-names">
                    <span className="font-name">{f}</span>
                  </span>
                  <span className="font-sample" style={{ fontFamily: `"${f}"` }}>
                    空山新雨后，天气晚来秋
                  </span>
                </button>
              ))}
              {fonts && list.length === 0 && <div className="font-empty">没有匹配的字体</div>}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

const round2 = (v: number) => Math.round(v * 100) / 100;
const round3 = (v: number) => Math.round(v * 1000) / 1000;
