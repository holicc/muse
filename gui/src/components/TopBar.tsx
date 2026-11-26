// 顶栏：品牌、切诗、未保存提示、复位（拆分按钮 + 菜单）、取消、保存、收起侧栏。

import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { Button, Icon, Keycap, Popover } from './controls';
import { quit, save } from '../lib/api';
import { changes, get, set, useDirty, useStore } from '../lib/store';
import { resetAll, resetPoem } from '../lib/actions';

export function TopBar({ compact }: { compact: boolean }) {
  const state = useStore((s) => s.state);
  const index = useStore((s) => s.index);
  const saveState = useStore((s) => s.save);
  const saveError = useStore((s) => s.saveError);
  const sidebar = useStore((s) => s.sidebar);
  const dirty = useDirty();
  const [menu, setMenu] = useState(false);
  const [confirmAll, setConfirmAll] = useState(false);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const caretRef = useRef<HTMLButtonElement>(null);
  const resetRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  // 窗口的关闭按钮和「取消」一样：有未保存的修改时先确认。
  useEffect(() => {
    window.muse.onCloseRequest(() => (changes().count > 0 ? setConfirmCancel(true) : quit()));
    const all = () => setConfirmAll(true);
    window.addEventListener('muse:reset-all', all);
    return () => window.removeEventListener('muse:reset-all', all);
  }, []);
  if (!state) return <header className="topbar" />;
  const poem = state.poems[index];
  const n = state.poems.length;
  const go = (d: number) => set({ index: (index + d + n) % n, charMode: false, selectedChar: null });
  const saved = saveState === 'saved';

  return (
    <header className="topbar">
      <div className="tb-left">
        <span className="brand">诗</span>
        {!compact && <span className="brand-name">调整与设计</span>}
      </div>
      <div className="tb-center">
        <Button kind="icon" icon="left" aria-label="上一首" onClick={() => go(-1)} />
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={poem.name}
            className="tb-poem"
            initial={{ opacity: 0, x: 8 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -8 }}
            transition={{ duration: 0.2, ease: [0.45, 0, 0.25, 1] }}
          >
            <span className="tb-title">{poem.title}</span>
            <span className="tb-author">
              {poem.dynasty} · {poem.author}
            </span>
            <span className="tb-count mono">
              {String(index + 1).padStart(2, '0')} / {String(n).padStart(2, '0')}
            </span>
          </motion.div>
        </AnimatePresence>
        <Button kind="icon" icon="right" aria-label="下一首" onClick={() => go(1)} />
      </div>
      <div className="tb-right">
        <AnimatePresence mode="wait" initial={false}>
          {saveState === 'failed' ? (
            <motion.span key="f" className="tb-status error" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
              {saveError ? `无法写入配置，修改已保留（${saveError}）·` : '无法写入配置，修改已保留 ·'}
              <a href="#" onClick={(e) => (e.preventDefault(), save())}>
                重试
              </a>
            </motion.span>
          ) : saved ? (
            <motion.span key="s" className="tb-status" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
              <Icon name="check" size={14} />
              已保存 · 壁纸已更新
            </motion.span>
          ) : dirty > 0 ? (
            <motion.span key="d" className="tb-status" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
              <span className="dot" />
              未保存的修改
            </motion.span>
          ) : null}
        </AnimatePresence>
        <div className="split" ref={resetRef}>
          <Button
            kind="ghost"
            icon="reset"
            title="复位本首（R）：本首所有部件的位置、字号、透明度恢复默认"
            disabled={saved}
            onClick={resetPoem}
          >
            {compact ? undefined : '复位'}
          </Button>
          <span className="split-div" />
          <button
            ref={caretRef}
            type="button"
            className={`btn btn-icon split-caret${menu ? ' open' : ''}`}
            aria-label="复位选项"
            aria-haspopup="menu"
            aria-expanded={menu}
            disabled={saved}
            onClick={() => setMenu((v) => !v)}
          >
            <Icon name="down" />
          </button>
        </div>
        <Popover open={menu} onClose={() => setMenu(false)} anchor={resetRef.current} width={260} role="menu">
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            onClick={() => {
              setMenu(false);
              resetPoem();
            }}
          >
            <span className="menu-text">
              <span className="menu-title">复位本首</span>
              <span className="menu-sub">本首所有部件的位置、字号、透明度恢复默认</span>
            </span>
            <Keycap>R</Keycap>
          </button>
          <div className="menu-div" />
          <button
            type="button"
            role="menuitem"
            className="menu-item"
            onClick={() => {
              setMenu(false);
              setConfirmAll(true);
            }}
          >
            <span className="menu-text">
              <span className="menu-title">全部复位…</span>
              <span className="menu-sub">{n} 首诗全部恢复默认，需要确认</span>
            </span>
            <Keycap>⇧R</Keycap>
          </button>
        </Popover>
        <ConfirmAll open={confirmAll} onClose={() => setConfirmAll(false)} anchor={resetRef.current} count={n} />
        <button
          ref={cancelRef}
          type="button"
          className="btn btn-secondary btn-h28"
          disabled={saved}
          onClick={() => (dirty > 0 ? setConfirmCancel(true) : quit())}
        >
          <span>取消</span>
        </button>
        <ConfirmCancel open={confirmCancel} onClose={() => setConfirmCancel(false)} anchor={cancelRef.current} count={dirty} />
        <SaveButton />
        <span className="tb-div" />
        <Button
          kind="icon"
          icon="sidebar"
          aria-label={sidebar ? '收起侧栏' : '显示侧栏'}
          className={sidebar ? 'pressed' : ''}
          onClick={() => set({ sidebar: !get().sidebar })}
        />
      </div>
    </header>
  );
}

/** 保存：成功时「已存」印章盖下，失败时左右轻抖。 */
function SaveButton() {
  const s = useStore((x) => x.save);
  const dirty = useDirty();
  return (
    <motion.button
      type="button"
      className="btn btn-primary btn-h28 save-btn"
      disabled={dirty === 0 && s !== 'saved'}
      onClick={() => save()}
      animate={s === 'failed' ? { x: [0, 3, -3, 2, 0] } : { x: 0 }}
      transition={{ duration: 0.24, ease: 'linear' }}
    >
      <AnimatePresence mode="wait" initial={false}>
        {s === 'saved' ? (
          <motion.span
            key="saved"
            className="stamp"
            initial={{ scale: 1.25, rotate: -6, opacity: 0 }}
            animate={{ scale: 1, rotate: 0, opacity: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.24, ease: [0.45, 0, 0.25, 1] } }}
            transition={{ type: 'spring', stiffness: 520, damping: 2 * 0.6 * Math.sqrt(520) }}
          >
            <Icon name="check" size={14} />
            已存
            <motion.i className="ink-ring" initial={{ scale: 1, opacity: 0.4 }} animate={{ scale: 1.6, opacity: 0 }} transition={{ duration: 0.36, ease: [0.22, 1, 0.36, 1] }} />
          </motion.span>
        ) : (
          <motion.span key="label" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.12 }}>
            {s === 'saving' ? '保存中' : '保存'}
          </motion.span>
        )}
      </AnimatePresence>
    </motion.button>
  );
}

function ConfirmCancel({ open, onClose, anchor, count }: { open: boolean; onClose: () => void; anchor: HTMLElement | null; count: number }) {
  return (
    <Popover open={open} onClose={onClose} anchor={anchor} width={268} align="right" role="alertdialog">
      <div className="confirm">
        <div className="confirm-title">放弃修改？</div>
        <div className="confirm-body">有 {count} 处调整未保存，取消后将还原到上次保存的样子。</div>
        <div className="confirm-actions">
          <button type="button" className="btn btn-secondary btn-h28" autoFocus onClick={onClose}>
            <span>继续编辑</span>
          </button>
          <button type="button" className="btn btn-primary btn-h28" onClick={() => quit()}>
            <span>放弃修改</span>
          </button>
        </div>
      </div>
    </Popover>
  );
}

function ConfirmAll({ open, onClose, anchor, count }: { open: boolean; onClose: () => void; anchor: HTMLElement | null; count: number }) {
  return (
    <Popover open={open} onClose={onClose} anchor={anchor} width={300} role="alertdialog">
      <div className="confirm">
        <div className="confirm-title">将 {count} 首诗全部复位？</div>
        <div className="confirm-body">每首诗所有部件的位置、字号、透明度和整首位置都恢复默认；保存前可以用 ⌘Z 撤销。</div>
        <div className="confirm-actions">
          <button type="button" className="btn btn-secondary btn-h28" autoFocus onClick={onClose}>
            <span>取消</span>
          </button>
          <button
            type="button"
            className="btn btn-primary btn-h28"
            onClick={() => {
              onClose();
              resetAll();
            }}
          >
            <span>全部复位</span>
          </button>
        </div>
      </div>
    </Popover>
  );
}

