// 检查器：没选部件时编辑整首（颜色、本首背景、部件列表、版式与字号），
// 选了部件时编辑部件（颜色、字号与透明度、位置），正文、印章等还可以单字上色。

import { useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Button, Hint, Icon, NumberField, ResetSlot, Row, Section, Segmented, Slider } from './controls';
import { Chip, HatchChip, Ladder, PaintEditor, Picker, type Layer } from './color';
import { editArrangement, editSetting, editStyle, set, useStore, without } from '../lib/store';
import { charColorCount, firstColor, globalColor, inPoemColor, isCharKey, partColor, partName } from '../lib/parts';
import { fileName, useAutoColor, useImageUrl } from '../lib/hooks';
import { wallpaperFor, globalWallpaper, canvas } from '../lib/preview';
import { SEAL_DEFAULT, paletteName } from '../lib/color';
import type { PaintDoc, StyleDoc } from '../lib/types';

export function Inspector() {
  const state = useStore((s) => s.state);
  const draft = useStore((s) => s.draft);
  const index = useStore((s) => s.index);
  const selected = useStore((s) => s.selected);
  const charMode = useStore((s) => s.charMode);
  if (!state || !draft) return null;
  const poem = state.poems[index];
  const key = selected === null ? 'poem' : charMode ? `char-${selected}` : `part-${selected}`;
  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div
        key={key + poem.name}
        className="panel"
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, transition: { duration: 0.08 } }}
        transition={{ duration: 0.16, ease: [0.22, 1, 0.36, 1] }}
      >
        {selected === null ? <PoemPanel /> : charMode ? <CharPanel part={selected} /> : <PartPanel part={selected} />}
      </motion.div>
    </AnimatePresence>
  );
}

function useCtx() {
  const state = useStore((s) => s.state)!;
  const draft = useStore((s) => s.draft)!;
  const index = useStore((s) => s.index);
  const display = useStore((s) => s.display);
  const poem = state.poems[index];
  const st: StyleDoc = draft.styles[poem.name];
  const auto = useAutoColor();
  const edit = (fn: (st: StyleDoc) => StyleDoc, k: string | null = null) => editStyle(poem.name, fn, k);
  return { state, draft, poem, st, auto, edit, display };
}

/** 设颜色：undefined 表示删掉。 */
function setColor(st: StyleDoc, key: string, p: PaintDoc | undefined): StyleDoc {
  return { ...st, colors: p === undefined ? without(st.colors, key) : { ...st.colors, [key]: p } };
}

function Header({ sub, title, onReset, resetDisabled, extra }: { sub: string; title: string; onReset: () => void; resetDisabled: boolean; extra?: React.ReactNode }) {
  return (
    <div className="panel-head">
      <div className="panel-head-text">
        <span className="panel-sub">{sub}</span>
        <span className={sub.startsWith('未选中') ? 'panel-title-poem' : 'panel-title'}>{title}</span>
      </div>
      <div className="panel-head-actions">
        {extra}
        <Button kind="ghost" size={24} icon="restore" disabled={resetDisabled} onClick={onReset}>
          恢复默认
        </Button>
      </div>
    </div>
  );
}

// ───────────── 整首 ─────────────

function PoemPanel() {
  const { draft, poem, st, auto, edit, display } = useCtx();
  const g = globalColor(draft);
  const own = st.colors.color;
  const active: Layer = own ? '整首' : g ? '全局' : '自动';
  const fallback = g ?? auto.ink;
  const locked = poem.style !== null;
  const layout = locked ? poem.style! : (draft.settings.style as string);
  const ts = draft.settings.text_scale as number;
  const a = draft.arrangement;
  const place = a.x != null && a.y != null ? [a.x, a.y] : canvas?.placement() ?? null;
  const W = display?.width ?? 1920, H = display?.height ?? 1080;
  return (
    <>
      <Header
        sub="未选中部件 · 正在编辑"
        title={`整首 · ${poem.title}`}
        resetDisabled={!own && st.wallpaper === undefined}
        onReset={() => edit((s) => ({ ...setColor(s, 'color', undefined), wallpaper: undefined }))}
      />
      <Section no="01" title="整首颜色">
        <Ladder
          active={active}
          layers={[
            { name: '单字', color: null },
            { name: '部件', color: null },
            { name: '整首', color: firstColor(own) },
            { name: '全局', color: g },
            { name: '自动', color: auto.ink },
          ]}
          onJump={(l) => l === '全局' && set({ tab: 'global' })}
        >
          {active === '自动' ? (
            <>
              <b>自动</b>：诗所在区域{auto.light ? '偏亮' : '偏暗'}，使用{auto.light ? '墨色' : '月白'} <span className="mono">{auto.ink.toUpperCase()}</span>。拖动后会重新判断。
            </>
          ) : active === '全局' ? (
            <>
              未设置整首颜色，沿用<b>全局字色</b> <span className="mono">{g!.toUpperCase()}</span>。
            </>
          ) : (
            <>
              颜色来自<b>整首</b>，作用于标题、正文、署名，覆盖了下层的「{g ? '全局' : '自动'}」。
            </>
          )}
        </Ladder>
        <PaintEditor
          paint={own}
          fallback={fallback}
          fallbackLabel={g ? '全局' : '自动'}
          bgLuma={auto.luma}
          onChange={(p, done) => edit((s) => setColor(s, 'color', p), done ? null : 'poem-color')}
        />
      </Section>
      <Section
        no="02"
        title="本首背景"
        aside={<span className="tag">{st.wallpaper === undefined ? '跟随全局' : '本首单独设置'}</span>}
      >
        <PoemBackground />
      </Section>
      <Section no="03" title="部件" aside={<span className="section-aside">点选以单独调整</span>}>
        <PartList />
        <Hint>水印铺满背景，画布上点不中；在此列表中选，或用 Tab / ⇧Tab 切到它，选中后可在画布上拖动。</Hint>
      </Section>
      <Section no="04" title="版式与字号">
        <div className="stack6">
          <div className="lock-row">
            <Segmented
              locked={locked}
              options={[
                { value: 'magazine', label: '杂志风' },
                { value: 'classic', label: '传统' },
              ]}
              value={layout}
              onChange={(v) => editSetting('style', v)}
              className="grow"
            />
            {locked && (
              <span className="lock-tag" title={`诗词包指定为${poem.style === 'magazine' ? '杂志风' : '传统'}；全局「版式」对本首无效。`}>
                <Icon name="lock" size={13} />
                诗词包指定
              </span>
            )}
          </div>
          <Hint>
            {locked
              ? `本首所在诗词包指定了版式，全局设置对它无效。`
              : '诗词包未指定版式 · 这里的选择就是全局设置，会作用于所有未指定的诗。'}
          </Hint>
        </div>
        <Row label="字号">
          <Slider value={a.scale} min={0.3 / ts} max={3 / ts} onChange={(v, done) => editArrangement({ scale: r3(v) }, done ? null : 'poem-scale')} />
          <NumberField value={a.scale} width={68} unit="×" step={0.01} min={0.3 / ts} max={3 / ts} format={(v) => v.toFixed(2)} onChange={(v, done) => editArrangement({ scale: r3(v) }, done ? null : 'poem-scale')} />
          <ResetSlot modified={Math.abs(a.scale - 1) > 1e-3} tip="恢复为 1.00×" onReset={() => editArrangement({ scale: 1 })} />
        </Row>
        <Row label="位置">
          <NumberField
            prefix="X"
            width={89}
            value={place ? place[0] * W : 0}
            min={0}
            max={W}
            onChange={(v, done) => place && editArrangement({ x: r4(v / W), y: r4(place[1]) }, done ? null : 'poem-x')}
          />
          <NumberField
            prefix="Y"
            width={89}
            value={place ? place[1] * H : 0}
            min={0}
            max={H}
            onChange={(v, done) => place && editArrangement({ x: r4(place[0]), y: r4(v / H) }, done ? null : 'poem-y')}
          />
          <ResetSlot modified={a.x != null} tip="恢复诗词包的默认位置" onReset={() => editArrangement({ x: null, y: null })} />
        </Row>
        <Hint>整首字号也可用滚轮调整；整首不单独设透明度。位置和字号所有诗共用。</Hint>
      </Section>
    </>
  );
}

/** 本首背景：跟随全局，或单独选桌面壁纸、诗词包背景、图片。 */
function PoemBackground() {
  const { state, draft, poem, st, edit } = useCtx();
  const follow = st.wallpaper === undefined;
  const g = globalWallpaper(state, draft);
  const thumb = useImageUrl(follow ? g : wallpaperFor(state, draft, poem));
  const desc = (w: string | null, setting: string) =>
    setting === 'pack' || !w ? '诗词包背景' : setting === 'auto' ? `桌面壁纸 · ${fileName(w)}` : `图片 · ${fileName(w)}`;
  return (
    <div className="stack10">
      <div className="ladder">
        <span className="ladder-title">背景来源 · 本首设置优先于全局</span>
        <div className="ladder-row">
          <span className={`ladder-cell ${follow ? 'unset' : 'active'}`}>本首</span>
          <span className="ladder-sep">›</span>
          <span className={`ladder-cell ${follow ? 'active' : 'over'}`}>
            {thumb && follow && <img src={thumb} className="ladder-thumb" alt="" />}
            全局
          </span>
        </div>
      </div>
      <Segmented
        options={[
          { value: 'follow', label: '跟随全局' },
          { value: 'own', label: '本首单独设置' },
        ]}
        value={follow ? 'follow' : 'own'}
        onChange={(v) => edit((s) => (v === 'follow' ? { ...s, wallpaper: undefined } : { ...s, wallpaper: draft.settings.wallpaper === 'pack' ? 'auto' : 'pack' }))}
      />
      {follow ? (
        <div className="bg-card">
          {thumb ? <img src={thumb} alt="" /> : <span className="bg-card-pack" style={{ background: packBg(poem.particles) }} />}
          <div className="bg-card-text">
            <span className="bg-card-title">{desc(g, draft.settings.wallpaper)}</span>
            <span className="bg-card-sub">来自全局设置 · 所有未单独设置的诗共用</span>
          </div>
          <Button kind="icon" size={24} icon="arrow" aria-label="前往全局设置" onClick={() => set({ tab: 'global' })} />
        </div>
      ) : (
        <BgChoices value={st.wallpaper!} desktop={state.wallpaper.desktop} preset={poem.particles} onChange={(w) => edit((s) => ({ ...s, wallpaper: w }))} />
      )}
      <Hint>为这首诗单独设置后，切到它时壁纸随之更换；自动字色会按本首背景中诗所在区域的明暗重新判断。</Hint>
    </div>
  );
}

/** 诗词包自带背景的缩略色（和渲染器按粒子预设选的配色一致）。 */
export function packBg(preset: string | null): string {
  const [a, b] =
    (
      {
        petals: ['#f4e6e1', '#e8d2c3'],
        snow: ['#cfd6de', '#edf0f2'],
        rain: ['#4f5a64', '#7f8a92'],
        fireflies: ['#09111f', '#16252c'],
        moonlight_dust: ['#0c142c', '#253050'],
      } as Record<string, [string, string]>
    )[preset ?? ''] ?? ['#f0e9db', '#e2d7c2'];
  return `linear-gradient(${a}, ${b})`;
}

/** 背景三选一：桌面壁纸 / 诗词包背景 / 选择图片。 */
export function BgChoices({ value, desktop, onChange, preset = null }: { value: string; desktop: string | null; onChange: (w: string) => void; preset?: string | null }) {
  const desk = useImageUrl(desktop);
  const file = value !== 'auto' && value !== 'pack' ? value : null;
  const fileThumb = useImageUrl(file);
  const card = (on: boolean, img: string | null, title: string, sub: string, click: () => void, dashed = false) => (
    <button type="button" className={`bg-choice${on ? ' on' : ''}`} onClick={click}>
      <span className="bg-choice-img">
        {img ? <img src={img} alt="" /> : dashed ? <span className="bg-choice-add"><Icon name="plus" /></span> : <span className="bg-choice-pack" style={{ background: packBg(preset) }} />}
        {on && (
          <span className="bg-choice-check">
            <Icon name="check" size={10} />
          </span>
        )}
      </span>
      <span className="bg-choice-title">{title}</span>
      <span className="bg-choice-sub">{sub}</span>
    </button>
  );
  return (
    <div className="bg-choices">
      {card(value === 'auto', desk, '桌面壁纸', desktop ? '自动识别' : '没有识别到', () => onChange('auto'))}
      {card(value === 'pack', null, '诗词包背景', '随诗变化', () => onChange('pack'))}
      {card(!!file, fileThumb, file ? fileName(file) : '选择图片…', 'PNG / JPG / WEBP', async () => {
        const p = await window.muse.pickImage();
        if (p) onChange(p);
      }, true)}
    </div>
  );
}

/** 部件列表：色块、名字、状态（部件色 / 继承 / 默认朱红 / N 字单独上色）。正文可展开到各列。 */
function PartList() {
  const { st, draft, auto, poem } = useCtx();
  const selected = useStore((s) => s.selected);
  const [open, setOpen] = useState(false);
  const parts = useStore((s) => s.parts);
  const lines = parts.filter((p) => p.startsWith('line.'));
  const others = parts.filter((p) => !p.startsWith('line.'));
  const customized = (p: string) =>
    st.colors[p] !== undefined || st.offsets[p] !== undefined || st.sizes[p] !== undefined || st.opacity[p] !== undefined || charColorCount(st, p) > 0;
  const status = (p: string) => {
    const n = charColorCount(st, p);
    if (p === 'watermark') return '选中后可拖动';
    if (st.colors[p] !== undefined) return '部件色';
    if (n) return `${n} 字单独上色`;
    if (p === 'seal') return '默认朱红';
    return '继承';
  };
  const row = (p: string, label: string, indent = false, chevron?: React.ReactNode) => (
    <button
      key={p}
      type="button"
      className={`part-row${selected === p ? ' on' : ''}${indent ? ' indent' : ''}`}
      onClick={() => set({ selected: p, charMode: false, selectedChar: null })}
    >
      <span className="part-chev">{chevron}</span>
      {p === 'watermark' ? <HatchChip /> : <Chip color={partColor(st, p, draft, auto.ink)} />}
      <span className="part-name">{label}</span>
      {customized(p) && <span className="part-dot" />}
      <span className="part-status">{status(p)}</span>
    </button>
  );
  const lineChars = lines.reduce((n, p) => n + charColorCount(st, p), 0);
  const linesCustom = lines.some(customized);
  const [first, ...rest] = others;
  return (
    <div className="part-list">
      {first === 'title' && row('title', '标题')}
      {lines.length > 0 && (
        <>
          <button type="button" className="part-row" onClick={() => setOpen((v) => !v)}>
            <span className="part-chev">
              <Icon name={open ? 'down' : 'right'} size={12} />
            </span>
            <Chip color={partColor(st, lines[0], draft, auto.ink)} />
            <span className="part-name">
              正文 · {lines.length} {poem.orientation === 'horizontal' ? '行' : '列'}
            </span>
            {linesCustom && <span className="part-dot" />}
            <span className="part-status">{lineChars ? `${lineChars} 字单独上色` : lines.some((p) => st.colors[p]) ? '部件色' : '继承'}</span>
          </button>
          <AnimatePresence initial={false}>
            {open && (
              <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} transition={{ duration: 0.22, ease: [0.45, 0, 0.25, 1] }} style={{ overflow: 'hidden' }}>
                {lines.map((p) => row(p, partName(p, poem).replace('正文 · ', ''), true))}
              </motion.div>
            )}
          </AnimatePresence>
        </>
      )}
      {(first === 'title' ? rest : others).map((p) => row(p, partName(p, poem)))}
    </div>
  );
}

// ───────────── 部件 ─────────────

function partLayers(st: StyleDoc, part: string, draft: ReturnType<typeof useCtx>['draft'], autoInk: string, char?: string) {
  const g = globalColor(draft);
  const seal = part === 'seal';
  const poemColor = inPoemColor(part) ? firstColor(st.colors.color) : null;
  const own = firstColor(st.colors[part]);
  const layers: { name: Layer; color: string | null }[] = [
    { name: '单字', color: char ?? null },
    { name: '部件', color: own },
    { name: '整首', color: seal ? null : poemColor },
    { name: '全局', color: seal ? null : g },
    seal ? { name: '默认', color: SEAL_DEFAULT } : { name: '自动', color: autoInk },
  ];
  const active = layers.find((l) => l.color !== null)!.name;
  return { layers, active };
}

function PartPanel({ part }: { part: string }) {
  const { st, draft, auto, edit, poem, display } = useCtx();
  const parts = useStore((s) => s.parts);
  const { layers, active } = partLayers(st, part, draft, auto.ink);
  const below = layers.find((l) => l.name !== '部件' && l.name !== '单字' && l.color !== null)!;
  const fallback = below.color!;
  const hasChars = part !== 'rule';
  const custom = st.colors[part] !== undefined || st.offsets[part] || st.sizes[part] !== undefined || st.opacity[part] !== undefined || charColorCount(st, part) > 0;
  return (
    <>
      <Header
        sub={`已选部件 · ${parts.indexOf(part) + 1} / ${parts.length}`}
        title={partName(part, poem)}
        resetDisabled={!custom}
        onReset={() =>
          edit((s) => ({
            ...s,
            colors: Object.fromEntries(Object.entries(s.colors).filter(([k]) => k !== part && !(isCharKey(k) && k.startsWith(part + '.')))),
            offsets: without(s.offsets, part),
            sizes: without(s.sizes, part),
            opacity: without(s.opacity, part),
          }))
        }
      />
      {hasChars && (
        <Segmented
          options={[
            { value: 'part', label: part.startsWith('line.') ? '整列颜色' : '整体颜色' },
            { value: 'char', label: '单字上色' },
          ]}
          value="part"
          onChange={(v) => v === 'char' && set({ charMode: true, selectedChar: 0 })}
        />
      )}
      <Section no="01" title="颜色">
        <Ladder layers={layers} active={active} onJump={(l) => l === '全局' ? set({ tab: 'global' }) : l === '整首' ? set({ selected: null }) : undefined}>
          {active === '部件' ? (
            <>
              颜色来自<b>部件</b>，覆盖了下层的「{below.name}」{below.name === '自动' ? (auto.light ? '墨色' : '月白') : ''}。
            </>
          ) : part === 'seal' ? (
            <>
              未设置，使用<b>默认朱红</b>。整首颜色与全局字色不作用于印章。
            </>
          ) : (
            <>
              未设置部件颜色，沿用<b>{below.name}</b> <span className="mono">{fallback.toUpperCase()}</span>。
            </>
          )}
        </Ladder>
        <PaintEditor
          paint={st.colors[part]}
          fallback={fallback}
          fallbackLabel={part === 'seal' ? '默认朱红' : below.name}
          bgLuma={auto.luma}
          onChange={(p, done) => edit((s) => setColor(s, part, p), done ? null : `color-${part}`)}
        />
      </Section>
      <SizeOpacity part={part} title="字号与透明度" note="字号为相对整首的百分比，100% 即跟随整首；也可 ⌘/Ctrl + 滚轮调整。" />
      <Section no="03" title="位置">
        <Row label="偏移">
          {(['X', 'Y'] as const).map((axis, i) => {
            const H = display?.height ?? 1080;
            const o = st.offsets[part] ?? [0, 0];
            return (
              <NumberField
                key={axis}
                prefix={axis}
                width={89}
                value={o[i] * H}
                format={(v) => (Math.round(v) >= 0 ? '+' : '') + Math.round(v)}
                onChange={(v, done) =>
                  edit((s) => {
                    const cur = s.offsets[part] ?? [0, 0];
                    const next: [number, number] = i === 0 ? [r4(v / H), cur[1]] : [cur[0], r4(v / H)];
                    return { ...s, offsets: next[0] === 0 && next[1] === 0 ? without(s.offsets, part) : { ...s.offsets, [part]: next } };
                  }, done ? null : `offset-${part}`)
                }
              />
            );
          })}
          <ResetSlot modified={!!st.offsets[part]} tip="回到默认位置" onReset={() => edit((s) => ({ ...s, offsets: without(s.offsets, part) }))} />
        </Row>
      </Section>
    </>
  );
}

function SizeOpacity({ part, title, note, no = '02' }: { part: string; title: string; note: string; no?: string }) {
  const { st, edit } = useCtx();
  const size = st.sizes[part] ?? 1;
  const op = st.opacity[part] ?? 1;
  const setSize = (v: number, done: boolean) =>
    edit((s) => ({ ...s, sizes: Math.abs(v - 1) < 1e-3 ? without(s.sizes, part) : { ...s.sizes, [part]: r3(v) } }), done ? null : `size-${part}`);
  const setOp = (v: number, done: boolean) =>
    edit((s) => ({ ...s, opacity: Math.abs(v - 1) < 1e-3 ? without(s.opacity, part) : { ...s.opacity, [part]: r3(v) } }), done ? null : `op-${part}`);
  return (
    <Section no={no} title={title}>
      <Row label="字号">
        <Slider value={size} min={0.3} max={3} onChange={setSize} />
        <NumberField value={size * 100} width={68} unit="%" min={30} max={300} onChange={(v, d) => setSize(v / 100, d)} />
        <ResetSlot modified={Math.abs(size - 1) > 1e-3} tip="恢复为 100%（跟随整首）" onReset={() => setSize(1, true)} />
      </Row>
      <Row label="透明度">
        <Slider value={op} min={0} max={1} onChange={setOp} />
        <NumberField value={op * 100} width={68} unit="%" min={0} max={100} onChange={(v, d) => setOp(v / 100, d)} />
        <ResetSlot modified={Math.abs(op - 1) > 1e-3} tip="恢复为 100%" onReset={() => setOp(1, true)} />
      </Row>
      <Hint>{note}</Hint>
    </Section>
  );
}

// ───────────── 单字 ─────────────

function CharPanel({ part }: { part: string }) {
  const { st, draft, auto, edit, poem } = useCtx();
  const selectedChar = useStore((s) => s.selectedChar);
  const parts = useStore((s) => s.parts);
  const chars = (canvas?.chars() ?? []).filter((c) => c.part === part);
  const i = Math.min(selectedChar ?? 0, Math.max(0, chars.length - 1));
  const ch = chars[i]?.ch ?? '';
  const key = `${part}.${i + 1}`;
  const own = st.colors[key] as string | undefined;
  const { layers, active } = partLayers(st, part, draft, auto.ink, own);
  const below = layers.find((l) => l.name !== '单字' && l.color !== null)!;
  const seal = part === 'seal';
  const charFallback = seal ? '#FFF4EC' : below.color!;
  const n = charColorCount(st, part);
  const total = Object.keys(st.colors).filter(isCharKey).length;
  const colorOf = (k: number) => (st.colors[`${part}.${k + 1}`] as string | undefined) ?? (seal ? '#FFF4EC' : below.color!);
  const nextColored = () => {
    const keys = Object.keys(st.colors).filter(isCharKey);
    if (!keys.length) return;
    const cur = keys.indexOf(key);
    const k = keys[(cur + 1) % keys.length];
    const m = /^(.*)\.(\d+)$/.exec(k)!;
    set({ selected: m[1], charMode: true, selectedChar: parseInt(m[2]) - 1 });
  };
  return (
    <>
      <Header
        sub={`已选部件 · ${parts.indexOf(part) + 1} / ${parts.length}`}
        title={partName(part, poem)}
        resetDisabled={n === 0}
        onReset={() => edit((s) => ({ ...s, colors: Object.fromEntries(Object.entries(s.colors).filter(([k]) => !(isCharKey(k) && k.startsWith(part + '.')))) }))}
      />
      <Segmented
        options={[
          { value: 'part', label: part.startsWith('line.') ? '整列颜色' : '整体颜色' },
          { value: 'char', label: '单字上色' },
        ]}
        value="char"
        onChange={(v) => v === 'part' && set({ charMode: false, selectedChar: null })}
      />
      <div className={`char-strip${seal ? ' seal' : ''}`}>
        {chars.map((c, k) => (
          <button
            key={k}
            type="button"
            className={`char-tile${k === i ? ' on' : ''}`}
            style={{ color: colorOf(k), background: seal ? firstColor(st.colors.seal) ?? SEAL_DEFAULT : undefined }}
            onClick={() => set({ selectedChar: k })}
          >
            {c.ch}
            {st.colors[`${part}.${k + 1}`] !== undefined && <i className="char-dot" />}
          </button>
        ))}
      </div>
      <div className="char-strip-foot">
        <span>
          本{part.startsWith('line.') ? '列' : '部件'} {n} 个字单独上色
        </span>
        <Button kind="ghost" size={24} disabled={n === 0} onClick={() => edit((s) => ({ ...s, colors: Object.fromEntries(Object.entries(s.colors).filter(([k]) => !(isCharKey(k) && k.startsWith(part + '.')))) }))}>
          清除本{part.startsWith('line.') ? '列' : '部件'}
        </Button>
      </div>
      <Section no="01" title={`单字 · 「${ch}」`}>
        <Ladder layers={layers} active={active}>
          {own ? (
            <>
              「{ch}」用<b>单字</b>颜色{paletteName(own) ? `（${paletteName(own)}）` : ''}；清除后回落到<b>{seal ? '默认纸色' : below.name}</b>（{seal ? '#FFF4EC' : below.color!.toUpperCase()}）。
            </>
          ) : (
            <>
              「{ch}」没有单独上色，沿用<b>{seal ? '默认纸色' : below.name}</b>。
            </>
          )}
        </Ladder>
        {own ? (
          <>
            <div className="solid-row">
              <Chip color={own} size={28} radius={5} />
              <div className="solid-meta">
                <span className="solid-name">{paletteName(own) ?? '自定义'}</span>
                <span className="mono muted">{own.toUpperCase().slice(0, 7)} · 100%</span>
              </div>
            </div>
            <Picker value={own} bgLuma={auto.luma} onChange={(hex, done) => edit((s) => setColor(s, key, hex), done ? null : `char-${key}`)} />
            <Button kind="secondary" icon="restore" onClick={() => edit((s) => setColor(s, key, undefined))}>
              清除「{ch}」的颜色
            </Button>
          </>
        ) : (
          <PaintEditor
            paint={undefined}
            fallback={charFallback}
            fallbackLabel={seal ? '默认纸色' : below.name}
            bgLuma={auto.luma}
            allowGradient={false}
            onChange={(p, done) => edit((s) => setColor(s, key, p as string | undefined), done ? null : `char-${key}`)}
          />
        )}
      </Section>
      <SizeOpacity part={part} title={`本${part.startsWith('line.') ? '列' : '部件'}字号与透明度`} note={`作用于整${part.startsWith('line.') ? '列' : '个部件'}；单字只能单独设颜色。`} />
      <div className="char-total">
        <span>整首共 {total} 个字单独上色</span>
        {total > 0 && (
          <a href="#" onClick={(e) => (e.preventDefault(), nextColored())}>
            逐个查看 ›
          </a>
        )}
      </div>
    </>
  );
}

const r3 = (v: number) => Math.round(v * 1000) / 1000;
const r4 = (v: number) => Math.round(v * 10000) / 10000;
