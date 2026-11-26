import { createRoot } from 'react-dom/client';
import { MotionConfig, MotionGlobalConfig } from 'motion/react';
import { App } from './App';
import * as store from './lib/store';
import * as preview from './lib/preview';
import * as api from './lib/api';

// 开发验收脚本通过它操作界面。
(window as any).__muse = store;
(window as any).__preview = preview;
(window as any).__api = api;
// 截图验收时窗口隐藏，动画帧被节流，直接跳到动画终点。
if ((window as any).muse?.shot) {
  MotionGlobalConfig.skipAnimations = true;
  document.documentElement.classList.add('shot');
}
import './styles/tokens.css';
import './styles/app.css';

createRoot(document.getElementById('root')!).render(
  // 系统开了「减少动态效果」时，位移动画换成短淡入淡出。
  <MotionConfig reducedMotion="user">
    <App />
  </MotionConfig>,
);
