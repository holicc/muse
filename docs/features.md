# muse 功能说明

古诗词动态桌面壁纸：把一首古诗逐字晕开地画在桌面上，配粒子效果，每首诗带出处和故事，定时轮换。
支持 macOS、Windows、Linux（Wayland layer-shell / X11，不支持 GNOME）。

## 命令

| 命令 | 作用 |
|---|---|
| `muse run` | 作为桌面壁纸运行（Linux 有 Wayland 用 layer-shell，否则 X11；macOS、Windows 用桌面层窗口） |
| `muse preview` | 普通窗口预览，调试诗词包用。N 下一首，空格暂停，Esc 退出 |
| `muse arrange` | 打开设计窗口（见下文） |
| `muse snapshot <out.png>` | 离屏渲染一帧，`--time` 截取时刻，`--size` 尺寸 |
| `muse check [目录]` | 校验诗词包 |
| `muse next` / `prev` / `pause` / `quit` / `reload` | 控制正在运行的壁纸 |
| `muse goto <诗词包>` / `muse status` | 切到某首诗 / 查看正在显示哪首、是否暂停 |
| `muse undo` | 撤销最近一次修改（设计窗口、MCP 做的修改） |
| `muse mcp` | 作为 MCP 服务运行（stdio），让 AI 控制壁纸（见下文） |
| `muse autostart on\|off\|status` | 开机自启（Linux XDG autostart、macOS LaunchAgent、Windows 注册表 Run） |
| `muse config` | 显示配置文件位置和当前生效的设置 |

命令行选项覆盖配置文件：`--packs` `--pack` `--single` `--font` `--wallpaper` `--text-color` `--style` `--scale` `--fps` `--no-tray` `--time` `--size`。

同一时间只运行一个壁纸实例（Unix 用控制 socket，Windows 用命名 mutex + 命名管道）。

## 配置 `config.toml`

位置：Linux `$XDG_CONFIG_HOME/muse/`（默认 `~/.config/muse/`），macOS `~/Library/Application Support/muse/`，Windows `%APPDATA%\muse\`。
全部可选，示例和默认值见仓库根的 `config.example.toml`。

设计窗口、MCP 改的全局设置写进状态目录的 `settings.toml`，覆盖 `config.toml` 的同名项（`config.toml` 程序永远不写）。
优先级：命令行 > `settings.toml` > `config.toml` > 默认值。`settings.toml` 里写错的项打印提示后跳过。

| 键 | 说明 |
|---|---|
| `packs` | 诗词包目录；不填依次找当前目录、程序旁边、配置目录下的 `packs` |
| `font` | 字体文件路径或系统字体名；不填自动找中文宋体。可变字体按角色用不同字重 |
| `wallpaper` | `"auto"` 当前桌面壁纸（默认）/ `"pack"` 诗词包自带背景 / 图片路径（png、jpg、webp、bmp） |
| `text_color` | `"auto"`（默认）或 `"#rrggbb"` |
| `style` | `magazine`（杂志风，默认）/ `classic`（传统）；诗词包写了 `text.style` 时以包为准 |
| `text_scale` | 字号倍数 0.3–3 |
| `start` / `single` | 从哪个包开始 / 只显示这一首 |
| `fps` / `battery_fps` | 插电帧率 1–120（默认 30）/ 电池帧率 0–120（默认 15），0 表示用电池时暂停 |
| `pause_on_fullscreen` | 全屏应用在前台时暂停（Windows） |
| `hold` / `char_interval` | 每首显示完后停留秒数 / 逐字出现间隔 |
| `tray` | 显示托盘图标（Linux 需要支持 StatusNotifierItem 的面板） |

## 背景

- 每首诗可以单独设背景（颜色文件里的 `wallpaper`），不设跟随全局；换到这首诗时背景随之切换。
- `auto`：读取当前桌面壁纸。Linux 依次识别 Noctalia、swww/awww、hyprpaper、swaybg、waypaper、KDE Plasma（含壁纸包目录）、feh；
  macOS 取主屏幕桌面图片；Windows 取系统壁纸设置。找不到或解不了码时用诗词包自带背景。
- 画在壁纸上时，按诗主体所在区域的亮度（WCAG 对比度）自动选深色或浅色字，不加垫底蒙版。
- 用户壁纸按 cover 铺满、不做视差漂移；诗词包的图层按 `depth` 缓慢视差漂移。

## 排版与动画

- 两种版式：杂志风（大号粗体标题、细线分栏、朱红印章、角落刊头 / 期号 / 出处小字、背景超大淡字）和传统居中。
- 竖排 / 横排；标点不显示；过长的句子均分成几列；长诗按可读字号分页，尽量在整联处断开。
- 逐字墨晕显现，整首停留后淡出，换下一首；粒子：花瓣、雪、雨、萤火、月光微尘。
- 节能：被遮挡、熄屏、锁屏时停止渲染；用电池时降帧；画面静止时降低刷新；Windows 全屏应用在前台时暂停。

## 设计窗口 `muse arrange`

Electron 应用（`gui/`），和 muse 打在同一个安装包里；也可以从托盘菜单「调整与设计…」打开。
画布用 wasm 编译的同一套渲染器，效果和桌面壁纸一致。读取、保存都经正在运行的 muse，muse 没运行时可以在窗口里启动它。

- 检查器：没选部件时编辑整首（整首颜色、本首背景、部件列表、版式与字号）；选了部件编辑它的颜色（纯色 / 渐变 / 透明度）、
  字号（相对整首的百分比）、透明度、位置；正文、印章等可以单字上色。颜色来源阶梯：单字 › 部件 › 整首 › 全局 › 自动（印章为默认朱红）。
- 全局设置：默认背景、字体（悬停即时预览）、版式、字号倍数、全局字色、节奏、帧率（含暂停和自定义）、全屏暂停、托盘。
  诗词包指定了版式的诗显示锁定。
- 画布：拖部件只动这个部件；空白处或按住 ⇧ 拖整首；吸附其他部件的边和中线（整首吸附画面中线、三分线、安全边距），Alt 暂停吸附；
  拖出安全区阻尼 0.35，松手弹回。滚轮调整首字号（以指针为锚点），⌘/Ctrl + 滚轮调选中部件字号。水印在列表里选或 Tab 切到后可拖。
- 快捷键：Tab / ⇧Tab 切部件，方向键微移（⇧ 10px），回车 / ⌘S 保存，N / ⇧N 切诗，R 复位本首，⇧R 全部复位（确认），
  ⌘Z / ⌘⇧Z 撤销重做，⌘\ 收起侧栏，Esc 取消拖动 / 退出单字 / 取消选中（不关窗），? 快捷键卡。
- 所有修改先在窗口里，点「保存」一次写盘，壁纸立即更新并切到正在编辑的这首；「取消」放弃修改（有修改时先确认）。
- 部件：`title`、`line.N`（第 N 列，按显示的列算，长诗分页不重新编号）、`byline`、`seal`、`rule`、`masthead`、`issue`、`source`、`watermark`。

## 统一接口与 MCP

正在运行的 muse 是服务端（Linux / macOS 的 Unix socket，Windows 的命名管道），命令行、托盘、设计窗口、`muse mcp` 都是客户端，
muse 没运行时一律报错提示先 `muse run`。协议是按行的 JSON，方法见 `crates/muse/src/api.rs` 开头的表。
每次写入前把原文件备份到状态目录的 `backups/`（保留最近 100 份），并记进撤销历史（`muse undo`、MCP 的 `undo`）。

`muse mcp` 的工具：`status`、`list_poems`、`next_poem`、`previous_poem`、`show_poem`、`set_paused`、`get_style`、`update_style`、
`reset_style`、`get_settings`、`update_settings`、`get_arrangement`、`set_arrangement`、`undo`、`history`、`reload`。
在 Claude Code 里添加：`claude mcp add muse -- muse mcp`。

## 每首诗的颜色

配置目录 `poems/<诗词包目录名>.toml`：

```toml
color = "#e8ebf5"                  # 整首（标题、正文、署名）
title = "#e0b04a"                  # 部件
"line.1" = ["#ffffff", "#7aa8ff"]  # 渐变：顺着阅读方向，两个以上颜色，色标均匀分布
"line.3.2" = "#ff5a4a80"           # 单个字：第 3 列第 2 个字；#rrggbbaa 带透明度
seal = "#2f6f4f"                   # 印章底色（也可以渐变）；印章里的字保持纸色
wallpaper = "pack"                 # 这首诗的背景：auto / pack / 图片路径；不写跟随全局
```

优先级：单个字 > 部件 > 每首的 `color` > 全局 `text_color` > 自动配色（印章为默认朱红）。
不透明度 = 角色自带的（小字较淡）× 颜色自带的 × 部件透明度。写错的项打印提示后跳过。
设计窗口和 MCP 写这个文件时只改变了的项，保留其他内容和注释；手改后用 `muse reload` 让运行中的壁纸生效。

## 用户数据

| 内容 | 位置 | 谁写 |
|---|---|---|
| `config.toml` | 配置目录 | 用户（可能由 home-manager 管理），程序不写 |
| 每首诗颜色、背景 `poems/<包名>.toml` | 配置目录 | 用户手写，或设计窗口 / MCP（保留格式） |
| 全局设置 `settings.toml` | 状态目录：Linux `$XDG_STATE_HOME/muse/`（默认 `~/.local/state/muse/`），其他平台同配置目录 | 设计窗口 / MCP |
| 整首位置 `arrangement.toml`（x、y、scale） | 状态目录 | 设计窗口 / MCP |
| 部件位置、字号、透明度 `poems/<包名>.toml`（`[offsets]` `[sizes]` `[opacity]`） | 状态目录 | 设计窗口 / MCP |
| 修改前的备份 `backups/` | 状态目录 | muse |

删掉状态目录里的文件即恢复默认。

## 诗词包

`packs/` 下每个子目录一首诗，内置和自定义同一格式（`pack.toml`，未知字段报错并带行号）：

```toml
format = 1
[poem]
title = "题都城南庄"
author = "崔护"
dynasty = "唐"
lines = ["去年今日此门中", "人面桃花相映红", ...]
source = "《全唐诗》"
[story]                 # 可选：白话故事
file = "story.md"
credibility = "legend"  # recorded 史载 / legend 传说 / speculative 推测
citation = "孟棨《本事诗·情感》"
[[layer]]               # 可选：背景图层，depth 0（远）到 1（近）
image = "sky.png"
depth = 0.2
[particles]             # 可选：petals / snow / rain / fireflies / moonlight_dust
preset = "petals"
[text]                  # 可选
position = "right"      # left / center / right
orientation = "vertical"  # vertical / horizontal
style = "magazine"      # 不写则用用户设置
```

内置 144 首，从《诗经》到清代的名篇。故事按古籍原典用白话改写，标注出处和可信度；只给出处可靠的篇目写（目前 25 首），
文件开头标了「草稿，待审」的还没有人工审过。
