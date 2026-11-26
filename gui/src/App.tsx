// 设计窗口：顶栏、画布、侧栏（检查器 / 全局设置）、状态栏，以及全局快捷键。

import { useEffect, useState } from 'react';
import { AnimatePresence, motion } from 'motion/react';
import { TopBar } from './components/TopBar';
import { Stage } from './components/Stage';
import { Inspector } from './components/Inspector';
import { GlobalPanel } from './components/GlobalPanel';
import { Button, Keycap, Segmented } from './components/controls';
import { boot, save, startMuse } from './lib/api';
import { get, redo, set, undo, useStore } from './lib/store';
import { cycleParts, goPoem, nudge, resetPoem } from './lib/actions';

function useWindowSize() {
  const [s, setS] = useState({ w: window.innerWidth, h: window.innerHeight });
  useEffect(() => {
    const r = () => setS({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener('resize', r);
    return () => window.removeEventListener('resize', r);
  }, []);
  return s;
}

export function App() {
  const phase = useStore((s) => s.phase);
  const theme = useStore((s) => s.theme);
  useEffect(() => {
    void boot();
  }, []);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  if (phase !== 'ready') return <Offline />;
  return <Editor />;
}

function Offline() {
  const phase = useStore((s) => s.phase);
  const error = useStore((s) => s.error);
  return (
    <div className="offline">
      <span className="brand big">诗</span>
      {phase === 'connecting' || phase === 'loading' ? (
        <p className="muted">正在连接 muse…</p>
      ) : (
        <>
          <h1>{phase === 'error' ? '读取数据失败' : 'muse 没有在运行'}</h1>
          <p className="muted">设计窗口通过正在运行的壁纸读取和保存设置。{phase === 'offline' && '启动壁纸后就能继续。'}</p>
          {error && <p className="error-text">{error}</p>}
          <div className="offline-actions">
            {phase === 'offline' && (
              <Button kind="primary" onClick={() => startMuse()}>
                启动壁纸
              </Button>
            )}
            <Button kind="secondary" onClick={() => boot()}>
              重新连接
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

function Editor() {
  const { w, h } = useWindowSize();
  const sidebar = useStore((s) => s.sidebar);
  const tab = useStore((s) => s.tab);
  const selected = useStore((s) => s.selected);
  const shortcuts = useStore((s) => s.shortcuts);
  const drawerMode = w < 1100;
  const width = w >= 1600 ? 360 : w >= 1280 ? 320 : w >= 1100 ? 288 : 300;
  const [drawer, setDrawer] = useState(false);
  const compact = w < 1100;

  // 小窗口：选中部件时自动打开抽屉。
  useEffect(() => {
    if (drawerMode && selected !== null) setDrawer(true);
  }, [selected, drawerMode]);

  useKeys(drawerMode && drawer, () => setDrawer(false));

  const docked = !drawerMode && sidebar;
  const showDrawer = drawerMode ? drawer : false;
  return (
    <div className="app">
      <TopBar compact={compact} />
      <div className="body">
        <main className="main">
          <Stage sidebarWidth={docked ? width : 0} />
          <StatusBar mode={w < 1100 ? 'min' : w < 1280 ? 'short' : 'full'} hidden={h < 680} />
          <AnimatePresence>{shortcuts && <ShortcutCard />}</AnimatePresence>
        </main>
        <AnimatePresence initial={false}>
          {docked && (
            <motion.aside
              key="side"
              className="sidebar"
              initial={{ width: 0 }}
              animate={{ width, transition: { duration: 0.28, ease: [0.22, 1, 0.36, 1] } }}
              exit={{ width: 0, transition: { duration: 0.22, ease: [0.45, 0, 0.25, 1], delay: 0.06 } }}
            >
              <Side width={width} tab={tab} />
            </motion.aside>
          )}
        </AnimatePresence>
        <AnimatePresence>
          {showDrawer && (
            <>
              <motion.div key="scrim" className="scrim" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onClick={() => setDrawer(false)} />
              <motion.aside
                key="drawer"
                className="sidebar drawer"
                style={{ width: 300 }}
                initial={{ x: 24, opacity: 0 }}
                animate={{ x: 0, opacity: 1, transition: { duration: 0.24, ease: [0.22, 1, 0.36, 1] } }}
                exit={{ x: 24, opacity: 0, transition: { duration: 0.16, ease: [0.4, 0, 1, 1] } }}
              >
                <Side width={300} tab={tab} />
              </motion.aside>
            </>
          )}
        </AnimatePresence>
        {drawerMode && !drawer && (
          <button type="button" className="drawer-handle" onClick={() => setDrawer(true)} aria-label="显示侧栏">
            ‹
          </button>
        )}
      </div>
    </div>
  );
}

function Side({ width, tab }: { width: number; tab: 'inspector' | 'global' }) {
  return (
    <motion.div className="side-inner" style={{ width }} initial={{ opacity: 0 }} animate={{ opacity: 1, transition: { delay: 0.06, duration: 0.16 } }} exit={{ opacity: 0, transition: { duration: 0.1 } }}>
      <div className="side-tabs">
        <Segmented
          height={32}
          options={[
            { value: 'inspector', label: '检查器', icon: 'layers' },
            { value: 'global', label: '全局设置', icon: 'grid' },
          ]}
          value={tab}
          onChange={(v) => set({ tab: v })}
        />
      </div>
      <div className="side-scroll">{tab === 'inspector' ? <Inspector /> : <GlobalPanel />}</div>
      <div className="side-fade" />
    </motion.div>
  );
}

function StatusBar({ mode, hidden }: { mode: 'full' | 'short' | 'min'; hidden: boolean }) {
  const draft = useStore((s) => s.draft);
  const display = useStore((s) => s.display);
  const scale = useStore((s) => s.previewScale);
  if (!draft || hidden) return null;
  const k = (draft.settings.text_scale as number) * draft.arrangement.scale;
  const hints = [
    <span key="1">拖动部件 <b>单独移动</b></span>,
    <span key="2">空白处 / <Keycap>⇧</Keycap> 拖动 <b>整首</b></span>,
    <span key="3">滚轮 <b>整首字号</b></span>,
    <span key="4"><Keycap>⌘/Ctrl</Keycap> + 滚轮 <b>选中部件字号</b></span>,
    <span key="5"><Keycap>Alt</Keycap> 暂停吸附</span>,
  ];
  const shown = mode === 'full' ? hints : mode === 'short' ? hints.slice(0, 3) : [];
  return (
    <div className="statusbar">
      <div className="status-hints">
        {shown}
        <button type="button" className="status-help" onClick={() => set({ shortcuts: !get().shortcuts })}>
          <Keycap>?</Keycap> {mode === 'min' ? '快捷键' : '全部快捷键'}
        </button>
      </div>
      <div className="status-info mono">
        <span>字号 ×{k.toFixed(2)}</span>
        <span>预览 {scale}%</span>
        <span>
          {`${display?.width}×${display?.height} · 主显示器`}
        </span>
      </div>
    </div>
  );
}

const SHORTCUTS: [string, string][] = [
  ['Tab ⇧Tab', '逐个选中部件（含水印）'],
  ['← ↑ → ↓', '微移 1px，⇧ 10px'],
  ['⌘/Ctrl 滚轮', '调选中部件字号'],
  ['Enter ⌘S', '保存'],
  ['N ⇧N', '下一首 / 上一首'],
  ['R', '复位本首'],
  ['⇧R', '全部复位…（需确认）'],
  ['⌘Z', '撤销'],
  ['⌘⇧Z', '重做'],
  ['⌘\\', '显示 / 收起侧栏'],
  ['Esc', '取消拖动 · 退出单字 · 取消选中'],
  ['?', '显示此卡片'],
];

function ShortcutCard() {
  return (
    <motion.div
      className="shortcut-card"
      initial={{ opacity: 0, scale: 0.98 }}
      animate={{ opacity: 1, scale: 1, transition: { duration: 0.16, ease: [0.22, 1, 0.36, 1] } }}
      exit={{ opacity: 0, transition: { duration: 0.1 } }}
    >
      <div className="sc-title">快捷键</div>
      <div className="sc-grid">
        {SHORTCUTS.map(([k, d]) => (
          <div key={k} className="sc-row">
            <span className="sc-key mono">{k}</span>
            <span className="sc-desc">{d}</span>
          </div>
        ))}
      </div>
      <div className="sc-foot">Esc 不关闭窗口，关窗只能点「取消」。输入框聚焦时 Enter 只确认数值。</div>
    </motion.div>
  );
}

/** 全局快捷键。输入框聚焦时不处理（数值框自己处理回车、方向键）。 */
function useKeys(drawerOpen: boolean, closeDrawer: () => void) {
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) return;
      const mod = e.metaKey || e.ctrlKey;
      const s = get();
      if (s.phase !== 'ready') return;
      const k = e.key;
      const handled = () => {
        e.preventDefault();
        e.stopPropagation();
      };
      if (mod && k.toLowerCase() === 'z') {
        handled();
        if (e.shiftKey) redo();
        else undo();
      } else if (mod && k.toLowerCase() === 'y') {
        handled();
        redo();
      } else if (mod && k.toLowerCase() === 's') {
        handled();
        void save();
      } else if (mod && k === '\\') {
        handled();
        set({ sidebar: !s.sidebar });
      } else if (mod) {
        return;
      } else if (k === 'Enter') {
        if (t?.tagName === 'BUTTON') return;
        handled();
        void save();
      } else if (k === 'Tab') {
        handled();
        cycleParts(e.shiftKey ? -1 : 1);
      } else if (k.startsWith('Arrow')) {
        handled();
        const d = e.shiftKey ? 10 : 1;
        nudge(k === 'ArrowLeft' ? -d : k === 'ArrowRight' ? d : 0, k === 'ArrowUp' ? -d : k === 'ArrowDown' ? d : 0);
      } else if (k === 'n' || k === 'N') {
        handled();
        goPoem(e.shiftKey ? -1 : 1);
      } else if (k === 'r' && !e.shiftKey) {
        handled();
        resetPoem();
      } else if (k === 'R' && e.shiftKey) {
        handled();
        // 全部复位要确认：打开顶栏复位按钮旁的确认框。
        window.dispatchEvent(new Event('muse:reset-all'));
      } else if (k === '?') {
        handled();
        set({ shortcuts: !s.shortcuts });
      } else if (k === 'Escape') {
        handled();
        if (s.shortcuts) set({ shortcuts: false });
        else if (s.charMode) set({ charMode: false, selectedChar: null });
        else if (drawerOpen) closeDrawer();
        else if (s.selected !== null) set({ selected: null });
      }
    };
    window.addEventListener('keydown', down);
    return () => window.removeEventListener('keydown', down);
  }, [drawerOpen, closeDrawer]);
}
