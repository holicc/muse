# muse 开发手册

## 代码结构

Cargo workspace，edition 2024，四个 crate，加上设计窗口 `gui/`：

- `crates/muse-core`：和平台、GPU 无关，全部可以单测。
  - `lib.rs`：诗词包格式（`Pack`）、加载与校验。
  - `layout.rs`：排版。坐标以屏幕高度为 1、原点左上。`Layout` 由 `PlacedChar`、`Shape`、`Column`（时间线分组）组成；
    每个字和形状标了 `Part`；`Placement` 整首定位；`offset_parts` 部件位移；`part_at` 命中测试。
  - `scene.rs`：时间线 `Timeline`、粒子 `ParticleSystem`。
  - `config.rs`：`Config`（`config.toml`，`load_layered` 叠加 `settings.toml`）、`update_settings`、`Wallpaper`、`TextColor`、
    `Arrangement`（整首位置）、配置 / 状态目录。
  - `style.rs`：每首诗的 `PoemStyle`（颜色 `Paint` / `Color` 带透明度、背景、部件位移 / 字号 / 透明度），
    对外格式 `StyleDoc`，颜色文件（toml_edit 保留格式写回）和部件文件的读写。
- `crates/muse-render`：wgpu 渲染，不碰窗口系统，调用方给 `Device`、`Queue` 和目标纹理。
  - `lib.rs`：`Renderer`、配色 `Palette`、用户壁纸亮度 `Backdrop`、字色 `char_colors`，以及给设计窗口用的
    `part_bounds` / `chars` / `auto_color` / `reconfigure`。
  - `shader.wgsl`：背景、图层、文字、细线 / 印章、粒子的全部着色器。
  - `font.rs` 字体查找（记下字体文件 `sources`、`locate`、`cjk_families`）与字形图集；`images.rs` 多显示器共用的图片解码缓存
    （后台预解码下一首；`insert_encoded` 给读不了磁盘的 wasm 用）。
- `crates/muse-web`：设计窗口的画布，muse-render 编译成 wasm（只在 `wasm32` 下有内容），暴露 `Builder` / `Preview` 给网页。
- `crates/muse`：可执行程序。
  - `main.rs`：命令行解析、`load_config`（配置 + 命令行覆盖）、`prepare` 生成 `Session`、各子命令。
  - `app.rs`：winit 前端，`Mode::Preview` / `Desktop`（X11、macOS、Windows）。
  - `wayland.rs`：Wayland layer-shell 前端（Background 层，每个输出一个 surface）。
  - `desktop.rs`：各平台桌面层窗口（X11 窗口类型、macOS 窗口层级、Windows 挂到 WorkerW）。
  - `control.rs` 统一接口的传输（Unix socket / Windows 命名管道，按行 JSON、事件订阅）和 `Hub`；
    `api.rs` 接口方法、写文件的事务（备份、撤销）；`mcp.rs` `muse mcp`（rmcp，stdio）；`gui.rs` 找到并启动设计窗口。
  - `wallpaper.rs` 识别当前桌面壁纸；`tray.rs` 托盘；`power.rs` 电源与全屏暂停；`gpu.rs` wgpu 初始化；`autostart.rs` 开机自启。
- `gui/`：设计窗口，Electron + React + Motion + TypeScript + Vite。
  - `electron/main.cjs` 主进程：连接 muse、读文件给页面、启动壁纸、关窗交给页面决定；`preload.cjs` 暴露 `window.muse`。
  - `src/lib/`：`store.ts` 草稿与撤销、`api.ts` 读取 / 保存、`preview.ts` 包装 wasm 画布、`parts.ts` 部件名和颜色来源、`actions.ts` 共用操作。
  - `src/components/`：`Stage`（画布、叠加层、拖动吸附）、`TopBar`、`Inspector`、`GlobalPanel`、`color`（取色、渐变）、`controls`（基础控件）。
  - `src/styles/tokens.css` 设计令牌（浅色 / 深色），`app.css` 组件样式；设计稿是 Claude Design 的「muse · 调整与设计窗口」。

数据流：`main` 读配置 → `prepare` 加载诗词包、字体、壁纸、位置和样式 → 前端为每个窗口建 `Renderer` →
接口请求在 `control` 的连接线程里经 `api::handle` 处理：读操作直接回，写操作在事务里写文件后发 `Command::Reload` →
前端 `reload_session` 重读配置和样式 → `Renderer::reconfigure` / `set_font`。前端换诗、暂停时 `Session::report` 通知订阅者。

设计窗口：`state` 一次取全部数据 → 主进程读字体、图片、`pack.toml` 交给 wasm 画布 → 修改只进草稿 →
「保存」调 `save` 一次写盘（一步撤销）。

## 环境与命令

常用命令都在仓库根的 `Makefile` 里（`make help` 列出全部）：`make setup` 装依赖（wasm-bindgen 装进仓库的 `.tools/`，`make build` 时缺了也会自动装），`make build`，`make run` / `preview` /
`arrange`（打开设计窗口），`make check`（fmt、clippy、wasm clippy、交叉检查、测试、tsc，提交前跑），`make dist` 打包。
`make run` 会先编译设计窗口并生成启动脚本 `target/muse-studio-dev`（经 `MUSE_GUI` 交给 muse），托盘「调整与设计…」开发时也能用；
NixOS 上 electron 的路径在第一次 make 时解析成 `target/electron`（GC root），打开窗口时不再跑 nix-shell。
NixOS 上运行类目标自动包进 nix-shell；主机没装 make 时用 `nix-shell --run "make <目标>"`（shell.nix 带了 make 和 node）。

下面是 Makefile 背后的原始命令：

```sh
cargo fmt
cargo clippy --all-targets          # 要求无警告
cargo test
# 平台代码改动后交叉检查（target 已安装）
cargo clippy -p muse --target aarch64-apple-darwin
cargo clippy -p muse --target x86_64-pc-windows-msvc
# 运行和截图：wgpu、Wayland 的库运行时加载，NixOS 上要进 nix-shell
nix-shell --run "cargo run -- snapshot out.png --pack jing-ye-si --time 40"
cargo run -- check packs
# 设计窗口（gui/）
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked   # 和 crates/muse-web 里的版本一致；nixpkgs 版本偏旧
cd gui && npm install && npm run build                      # build:wasm + 类型检查 + vite
nix-shell -p electron --run "electron gui"                  # NixOS 上用 nixpkgs 的 electron；外面的 LD_LIBRARY_PATH 要清掉
npm run dist -- --linux dir                                 # 打包（当前平台），muse 和 packs 放进 resources/bin/
```

`muse arrange` 找设计窗口：环境变量 `MUSE_GUI` → 安装目录（`resources/bin/` 的上两级，macOS `Contents/MacOS`）→
开发时仓库里已经 build 过的 `gui/`（用 PATH 里的 electron 运行）。

沙箱里拿不到显卡，`run`、`preview`、`arrange`、`snapshot` 要在沙箱外的 `nix-shell` 里执行。

## 约定

- 注释、报错、提示用中文；README 用英文。
- 报错带上文件路径和具体的值，并说明怎么改（参考 `Config::validate`）。
- `config.toml` 写错报错退出；样式、位置等程序维护的文件写错时打印提示、跳过那一项，不影响运行。
- `config.toml` 可能由 home-manager 管理，程序不写它。
- `unsafe` 块前写 `// 安全性：` 说明成立的理由。
- 新增配置项要同时改：`Config` 及其默认值和校验、`config.example.toml`、`main.rs` 的 `USAGE` 和命令行解析（如需要）、`muse config` 的输出、README、`docs/features.md`。
- 新增功能同步更新 `docs/features.md`；结构变化同步更新本手册。
- 依赖尽量少；平台专用依赖放在对应的 `[target.'cfg(...)'.dependencies]` 下。
- 新增接口方法要同时改：`api.rs` 开头的表和 `handle`、需要的话 `mcp.rs` 的工具、`docs/features.md`。
- 设计窗口按设计稿还原，偏离设计稿要先问；界面文案用中文。

## 验证

- 每次改动：fmt、clippy 无警告、test 全过。
- 动到平台代码：加跑 macOS、Windows 交叉 clippy；没有真机验证的要如实说明。
- 动到画面：用 `snapshot` 截图看效果。需要样式 / 位置数据时，用 `XDG_CONFIG_HOME`、`XDG_STATE_HOME` 指到临时目录，
  不碰真实的用户目录；截图和临时文件用完删除。
- 新逻辑尽量放在 `muse-core` 里写单测（排版、样式解析、配置读写都有现成的测试可参照）。
- 改动较大时让子 Agent 独立 review，修完再 review 一轮。
- 接口：`api.rs` 里有用临时目录跑的测试；端到端可以在 Xvfb 上跑 `muse run`（临时 `XDG_*`，短路径的 `XDG_RUNTIME_DIR`，
  socket 路径太长会创建失败），再用 `muse status` / `muse mcp` 验证。
- 设计窗口：`MUSE_GUI_SHOT=<目录> MUSE_GUI_STEPS=<步骤.json>` 时窗口隐藏、2 倍屏、按步骤执行 JS 并截图
  （页面里 `window.__muse` / `__preview` / `__api` 可用）。隐藏窗口里第二次截图会让 GPU 进程崩溃，每张截图单独启动一次；
  截图模式下动画和过渡直接跳到终态。

## 平台要点

- Linux Wayland：layer-shell Background 层，`exclusive_zone = -1`，不接收键盘；GNOME 不支持 layer-shell。
- X11：窗口类型 `_NET_WM_WINDOW_TYPE_DESKTOP`。
- macOS：桌面窗口层级、忽略鼠标、出现在所有空间；Dock 不显示图标。
- Windows：窗口挂到桌面图标后面的 WorkerW（和 Wallpaper Engine 同样做法），退出时让系统重画原壁纸；
  接口走命名管道 `\\.\pipe\muse-<用户名>`（没有真机验证过）。
- 预览、设计窗口是普通窗口，用户把它全屏时会盖住系统栏，属正常。
- 设计窗口的 Chromium 缓存放在 `<appData>/muse-studio/`（`main.cjs` 里改的 userData），不和 muse 的配置目录混在一起。
- 设计窗口的画布要 WebGPU：Electron 带 `--enable-unsafe-webgpu`；Linux Wayland 下 Chromium 会提示 Vulkan 不兼容，实际能跑。
