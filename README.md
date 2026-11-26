# muse

A live desktop wallpaper of classical Chinese poetry, rendered natively in Rust. Each poem comes with its own scene, source and story. Runs on macOS, Windows and Linux (Wayland layer-shell / X11; GNOME is not supported).

## Status

- [x] Poem pack format (`pack.toml`) and validation: `cargo run -- check [dir]`
- [x] wgpu rendering: background, vertical / horizontal text, per-character ink-spread reveal, particles (petals, snow, rain, fireflies, moonlight dust), parallax layers
- [x] Two layout styles: magazine (default) and classic; font and text size are configurable
- [x] Windowed preview (all platforms) and offscreen snapshots
- [x] Linux Wayland wallpaper (layer-shell, one surface per output)
- [x] macOS and Windows desktop-level windows, Linux X11 (builds, not yet tested on real hardware)
- [x] Pause policy: stops rendering when occluded, screen off or locked; lower frame rate on battery; pauses while a fullscreen app is in front on Windows
- [x] Tray icon, config file, autostart, command-line control (`muse next` / `pause` / `quit`)
- [x] Draws on top of your current desktop wallpaper; per-poem colors, gradients, transparency, per-character colors and backgrounds
- [x] Design window (Electron, same renderer compiled to WebAssembly): drag parts, resize, recolor, global settings
- [x] One local API for the CLI, tray, design window and an MCP server (`muse mcp`), with backups and undo

## Usage

```sh
cargo run --release -- preview            # windowed preview: N next poem, Space pause, Esc quit
cargo run --release -- run                # Wayland wallpaper
cargo run --release -- arrange            # open the design window
cargo run --release -- snapshot out.png --size 3840x2160 --time 20
cargo run --release -- next               # next poem (also prev, pause, quit, reload, status, undo)
cargo run --release -- mcp                # MCP server on stdio for AI assistants
cargo run --release -- autostart on       # start at login
cargo run --release -- config             # config file location and current settings
cargo run --release -- --help             # all options
```

Settings live in a config file; see [`config.example.toml`](config.example.toml).

By default the font is a Chinese serif found on the system (Noto Serif CJK, Source Han Serif, SimSun, etc.); use `--font` to pick another.

By default the poem is drawn on your current desktop wallpaper, with dark or light text chosen from the image's brightness. Set `wallpaper = "pack"` to use the pack's own background, or an image path to use a specific picture (`--wallpaper` on the command line). On Linux, wallpapers set by Noctalia, swww, hyprpaper, swaybg, waypaper, KDE Plasma and feh are detected.

Text color is chosen automatically to match the background. Set `text_color = "#c8a96e"` (or `--text-color`) to use a fixed color for the title and verses; small captions use the same color, slightly fainter, and the seal stays vermilion.

### Design window

Run `muse arrange` or choose "调整与设计" in the tray menu. The canvas uses the same renderer as the wallpaper (compiled to WebAssembly), so what you see is what you get. The window talks to the running wallpaper; if it isn't running, the window offers to start it.

- Drag a part (title, a column, byline, seal, rule, masthead, issue number, source, background character) to move just that part; drag empty space or hold Shift to move the whole poem. Parts snap to each other, to the screen center and thirds; hold Alt to stop snapping.
- Scroll to resize the whole poem; Ctrl/⌘ + scroll resizes the selected part. Size and opacity per part are also in the sidebar.
- Colors: solid, gradient (along the reading direction) or per character, with transparency and a palette of traditional Chinese colors. Each poem can also have its own background.
- Global settings: default background, font, layout, text size, text color, timing, frame rate, tray.
- Nothing is written until you click 保存 (Save; Enter or ⌘S). Shortcuts are listed with `?`.

Per-poem positions, sizes and opacity go to `~/.local/state/muse/` (the config directory on macOS and Windows); global settings from the window go to `settings.toml` there and override `config.toml`, which is never rewritten. Every change is backed up and can be undone with `muse undo`.

### Per-poem colors

Colors live in `~/.config/muse/poems/<pack>.toml` (the pack's directory name, e.g. `jing-ye-si.toml`). The design window edits this file and keeps your comments; you can also write it by hand:

```toml
color = "#e8ebf5"                  # whole poem: title, verses, byline
title = "#e0b04a"
"line.1" = ["#ffffff", "#7aa8ff"]  # gradient along the reading direction (2 or more colors)
"line.3.2" = "#ff5a4a80"           # a single character: column 3, character 2; #rrggbbaa adds transparency
seal = "#2f6f4f"                   # seal background
wallpaper = "pack"                 # this poem's background: auto, pack or an image path
```

Also available: `byline`, `rule`, `masthead`, `issue`, `source`, `watermark`. A single character beats its part, a part beats `color`, and `color` beats `text_color`. Run `muse reload` after editing by hand.

### AI control (MCP)

`muse mcp` exposes the running wallpaper as MCP tools: switch poems, read and change per-poem styles and global settings, undo. For Claude Code: `claude mcp add muse -- muse mcp`.

On NixOS, wgpu and Wayland libraries are loaded at runtime; enter the dev environment with `nix-shell` before running.

The design window lives in `gui/` (Electron + React). The `Makefile` wraps everything: `make setup` once (wasm target, `wasm-bindgen-cli` 0.2.130, npm packages), then `make build`, `make run`, `make arrange`, `make check` and `make dist` (packages the window together with `muse` and the bundled poems). `make help` lists all targets. On NixOS without make installed, use `nix-shell --run "make <target>"`.

## Poem packs

muse ships with 144 well-known poems, from the *Book of Songs* to the Qing dynasty; 25 of them come with a plain-language story retold from classical sources. Each subdirectory of `packs/` is one poem. Bundled and custom poems use the same format; see [`packs/ti-du-cheng-nan-zhuang`](packs/ti-du-cheng-nan-zhuang/pack.toml) for an example.

Stories are retold in modern Chinese from the original classical sources, with citations and a credibility level (`recorded` in historical records / `legend` / `speculative`).

## License

[MIT](LICENSE)
