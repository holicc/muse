// 把 crates/muse-web 编译成 wasm 并生成 JS 绑定，放到 src/wasm/。
// 需要：rustup target add wasm32-unknown-unknown；wasm-bindgen-cli 0.2.130（和 Cargo.toml 里的版本一致）。
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const gui = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const root = path.dirname(gui);
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: 'inherit' });

run('cargo', ['build', '--release', '-p', 'muse-web', '--target', 'wasm32-unknown-unknown']);
run(process.env.WASM_BINDGEN || 'wasm-bindgen', [
  '--target', 'web',
  '--out-dir', path.join(gui, 'src', 'wasm'),
  path.join(root, 'target', 'wasm32-unknown-unknown', 'release', 'muse_web.wasm'),
]);
