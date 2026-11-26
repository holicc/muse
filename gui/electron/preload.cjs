// 页面能调用的主进程功能，全部经 contextBridge 暴露为 window.muse。
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('muse', {
  connect: () => ipcRenderer.invoke('api:connect'),
  start: () => ipcRenderer.invoke('api:start'),
  call: (method, params) => ipcRenderer.invoke('api:call', method, params),
  onEvent: (fn) => ipcRenderer.on('api:event', (_e, msg) => fn(msg)),
  readFile: (path) => ipcRenderer.invoke('fs:read', path),
  readText: (path) => ipcRenderer.invoke('fs:readText', path),
  display: () => ipcRenderer.invoke('display'),
  pickImage: () => ipcRenderer.invoke('dialog:image'),
  quit: () => ipcRenderer.invoke('app:quit'),
  onCloseRequest: (fn) => ipcRenderer.on('app:close-request', () => fn()),
  theme: () => ipcRenderer.invoke('theme'),
  onTheme: (fn) => ipcRenderer.on('theme', (_e, t) => fn(t)),
  platform: process.platform,
  shot: !!process.env.MUSE_GUI_SHOT,
  log: (m) => ipcRenderer.send('log', String(m)),
});
