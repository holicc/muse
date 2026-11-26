// muse 设计窗口的 Electron 主进程。
//
// - 连接正在运行的 muse（Linux / macOS 的 Unix socket，Windows 的命名管道），按行收发 JSON，
//   把请求和事件转给页面。muse 没运行时页面会提示，并可以从这里启动它。
// - 页面读不了磁盘：字体、图片、诗词包由这里读出来交给页面里的 wasm 渲染器。
// - 关窗由页面决定（有未保存的修改时先确认）。

const { app, BrowserWindow, ipcMain, dialog, screen, nativeTheme, protocol, net } = require('electron');
const fs = require('fs');
const nodeNet = require('net');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');
const { pathToFileURL } = require('url');

app.commandLine.appendSwitch('enable-unsafe-webgpu');
// 开发验收：MUSE_GUI_SHOT=<输出目录> 时窗口不显示，按 MUSE_GUI_STEPS（JSON 文件）逐步操作并截图，2 倍屏。
// Chromium 的缓存默认放在以 productName 命名的目录，正好是 muse 的配置目录（可能由 home-manager 管理），挪开。
app.setPath('userData', path.join(app.getPath('appData'), 'muse-studio'));

const SHOT = process.env.MUSE_GUI_SHOT;
if (SHOT) app.commandLine.appendSwitch('force-device-scale-factor', '2');
app.commandLine.appendSwitch('enable-features', 'Vulkan,WebGPU');

protocol.registerSchemesAsPrivileged([
  { scheme: 'app', privileges: { standard: true, secure: true, supportFetchAPI: true } },
]);

/** muse 的地址：muse 启动设计窗口时会传进来，否则按同样的规则算。 */
function socketAddress() {
  if (process.env.MUSE_SOCKET) return process.env.MUSE_SOCKET;
  if (process.platform === 'win32') {
    const user = (process.env.USERNAME || 'user').replace(/[^\p{L}\p{N}]/gu, '_');
    return `\\\\.\\pipe\\muse-${user}`;
  }
  const dir = process.env.XDG_RUNTIME_DIR || os.tmpdir();
  return path.join(dir, `muse-${process.getuid()}.sock`);
}

/** muse 可执行文件：启动时传进来的，或安装包里放在 resources/bin 下的。 */
function museBin() {
  if (process.env.MUSE_BIN) return process.env.MUSE_BIN;
  const exe = process.platform === 'win32' ? 'muse.exe' : 'muse';
  const packed = path.join(process.resourcesPath || '', 'bin', exe);
  if (fs.existsSync(packed)) return packed;
  return exe;
}

// ───────────── 和 muse 的连接 ─────────────

let win = null;
let conn = null;
let nextId = 1;
const pending = new Map();

function sendToPage(channel, data) {
  if (win && !win.isDestroyed()) win.webContents.send(channel, data);
}

function connect() {
  return new Promise((resolve, reject) => {
    const s = nodeNet.createConnection(socketAddress());
    let buf = '';
    s.setEncoding('utf8');
    s.once('connect', () => {
      conn = s;
      resolve();
    });
    s.on('data', (chunk) => {
      buf += chunk;
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i);
        buf = buf.slice(i + 1);
        if (!line.trim()) continue;
        let msg;
        try {
          msg = JSON.parse(line);
        } catch {
          continue;
        }
        if (msg.event) {
          sendToPage('api:event', msg);
        } else if (pending.has(msg.id)) {
          const { resolve: ok, reject: fail } = pending.get(msg.id);
          pending.delete(msg.id);
          if (msg.error) fail(new Error(msg.error.message || '未知错误'));
          else ok(msg.result);
        }
      }
    });
    s.on('error', (e) => {
      if (conn !== s) reject(e);
    });
    s.on('close', () => {
      if (conn === s) {
        conn = null;
        for (const { reject: fail } of pending.values()) fail(new Error('muse 已经退出'));
        pending.clear();
        sendToPage('api:event', { event: 'disconnected' });
      }
    });
  });
}

function call(method, params) {
  if (!conn) return Promise.reject(new Error('muse 没有在运行'));
  const id = nextId++;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    conn.write(JSON.stringify({ id, method, params: params ?? null }) + '\n');
  });
}

async function ensureConnected() {
  if (conn) return true;
  try {
    await connect();
    await call('subscribe');
    return true;
  } catch {
    return false;
  }
}

/** 启动壁纸（沿用 muse arrange 时的命令行选项），等它接受连接。 */
async function startMuse() {
  let args = [];
  try {
    args = JSON.parse(process.env.MUSE_ARGS || '[]');
  } catch {}
  const child = spawn(museBin(), ['run', ...args], { detached: true, stdio: 'ignore' });
  let failed = null;
  child.on('error', (e) => (failed = e));
  child.unref();
  for (let i = 0; i < 50; i++) {
    await new Promise((r) => setTimeout(r, 200));
    if (failed) throw new Error(`启动 muse 失败: ${failed.message}`);
    if (await ensureConnected()) return true;
  }
  throw new Error('muse 启动后没有响应');
}

// ───────────── 页面能用的接口 ─────────────

ipcMain.handle('api:connect', () => ensureConnected());
ipcMain.handle('api:start', () => startMuse());
ipcMain.handle('api:call', (_e, method, params) => call(method, params));
ipcMain.handle('fs:read', (_e, file) => fs.promises.readFile(file));
ipcMain.handle('fs:readText', (_e, file) => fs.promises.readFile(file, 'utf8'));
ipcMain.handle('display', () => {
  const d = screen.getPrimaryDisplay();
  return {
    width: Math.round(d.size.width * d.scaleFactor),
    height: Math.round(d.size.height * d.scaleFactor),
    scale: d.scaleFactor,
    label: d.label || '主显示器',
  };
});
ipcMain.handle('dialog:image', async () => {
  const r = await dialog.showOpenDialog(win, {
    title: '选择背景图片',
    properties: ['openFile'],
    filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp'] }],
  });
  return r.canceled ? null : r.filePaths[0];
});
ipcMain.handle('app:quit', () => {
  allowClose = true;
  win?.close();
});
ipcMain.handle('theme', () => (nativeTheme.shouldUseDarkColors ? 'dark' : 'light'));
ipcMain.on('log', (_e, m) => console.log(m));

let allowClose = false;

function createWindow() {
  const [w, h] = (SHOT && process.env.MUSE_GUI_SIZE ? process.env.MUSE_GUI_SIZE : '1440x900').split('x').map(Number);
  win = new BrowserWindow({
    width: w,
    height: h,
    minWidth: 960,
    minHeight: 600,
    title: 'muse · 调整与设计',
    backgroundColor: nativeTheme.shouldUseDarkColors ? '#1A1917' : '#F7F5F0',
    titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'default',
    autoHideMenuBar: true,
    show: !SHOT,
    paintWhenInitiallyHidden: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      backgroundThrottling: !SHOT,
    },
  });
  if (SHOT) win.webContents.once('did-finish-load', () => runShots().catch((e) => console.error(e)).finally(() => app.exit(0)));
  win.removeMenu();
  // 页面只加载自己的内容：不跳转、不开新窗口。
  win.webContents.on('will-navigate', (e) => e.preventDefault());
  win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  // 关窗交给页面：有未保存的修改时页面会先确认，再调 app:quit。
  win.on('close', (e) => {
    if (allowClose) return;
    e.preventDefault();
    sendToPage('app:close-request');
  });
  nativeTheme.on('updated', () =>
    sendToPage('theme', nativeTheme.shouldUseDarkColors ? 'dark' : 'light'),
  );
  if (process.env.MUSE_GUI_DEV_URL) win.loadURL(process.env.MUSE_GUI_DEV_URL);
  else win.loadURL('app://muse/index.html');
}

async function runShots() {
  const steps = JSON.parse(fs.readFileSync(process.env.MUSE_GUI_STEPS, 'utf8'));
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  fs.mkdirSync(SHOT, { recursive: true });
  for (const [i, step] of steps.entries()) {
    if (step.size) win.setContentSize(step.size[0], step.size[1]);
    if (step.js) await win.webContents.executeJavaScript(step.js).catch((e) => console.error('步骤出错', i, e));
    await sleep(step.wait ?? 1500);
    if (step.shot) {
      const img = await win.webContents.capturePage();
      fs.writeFileSync(path.join(SHOT, step.shot + '.png'), img.toPNG());
      console.log('截图', step.shot);
    }
  }
}

app.whenReady().then(() => {
  const root = path.join(__dirname, '..', 'dist');
  protocol.handle('app', (req) => {
    const p = decodeURIComponent(new URL(req.url).pathname);
    const file = path.normalize(path.join(root, p));
    if (!file.startsWith(root + path.sep)) return new Response('', { status: 403 });
    return net.fetch(pathToFileURL(file).toString());
  });
  createWindow();
});

app.on('window-all-closed', () => app.quit());
