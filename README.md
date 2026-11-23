# muse

用 Rust 原生渲染的古诗词动态桌面壁纸：每首诗配自己的场景、出处和故事，支持 macOS、Windows、Linux（Wayland layer-shell / X11，不支持 GNOME）。

## 现状

- [x] 诗词包格式（`pack.toml`）与校验：`cargo run -- check [目录]`
- [ ] 各平台桌面层窗口（macOS 优先）
- [ ] wgpu 渲染：背景、竖排文字、逐字晕开、粒子
- [ ] 暂停策略（全屏、锁屏、电池）
- [ ] 托盘、设置、开机自启

## 诗词包

`packs/` 下每个子目录是一首诗，内置诗词和自定义诗词用同一种格式，示例见 [`packs/ti-du-cheng-nan-zhuang`](packs/ti-du-cheng-nan-zhuang/pack.toml)。

故事按古籍原典用白话改写，并标注出处和可信度（`recorded` 史载 / `legend` 传说 / `speculative` 推测）。

## 许可证

[MIT](LICENSE)
