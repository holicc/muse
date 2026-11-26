// 打安装包：先编译 muse（release），再用 electron-builder 把设计窗口、muse 和诗词包打在一起。
// 安装后的布局：设计窗口在安装目录根上（muse-studio），muse 和 packs 在 resources/bin/ 下。
// 在哪个平台上运行就打哪个平台的包；参数原样传给 electron-builder（如 --dir 只生成目录）。
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const gui = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const root = path.dirname(gui);
execFileSync('cargo', ['build', '--release', '-p', 'muse'], { cwd: root, stdio: 'inherit' });
execFileSync('npx', ['electron-builder', ...process.argv.slice(2)], { cwd: gui, stdio: 'inherit', shell: process.platform === 'win32' });
