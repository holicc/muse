# muse 常用命令。`make help` 列出全部目标。
#
# NixOS 上 wgpu、Wayland 的库运行时加载，运行类的命令自动包进 nix-shell（用仓库里的 shell.nix）；
# 设计窗口用 nixpkgs 的 electron，并清掉外面的 LD_LIBRARY_PATH（混进别的 glibc 版本的库会起不来）。
# 其他系统直接运行，electron 用 gui/node_modules 里的。
# NixOS 上没装 make 时：nix-shell --run "make <目标>"（shell.nix 里带了 make）。

WASM_BINDGEN_VERSION := 0.2.130
# wasm-bindgen 装在仓库的 .tools/ 里（和 crates/muse-web 的版本对齐，不依赖 PATH），缺了或版本不对自动安装。
TOOLS := $(CURDIR)/.tools
WASM_BINDGEN ?= $(TOOLS)/bin/wasm-bindgen
ARGS ?=
# 开发时启动设计窗口的脚本，交给 muse 的 MUSE_GUI（托盘「调整与设计…」、muse arrange 都用它）。
STUDIO := $(CURDIR)/target/muse-studio-dev

NIXOS := $(wildcard /etc/NIXOS)
ifneq ($(NIXOS),)
  # 已经在 nix-shell 里（比如 nix-shell --run make）就不再套一层。
  ifdef IN_NIX_SHELL
    RUN = sh -c
  else
    RUN = nix-shell --run
  endif
  # electron 的路径只在第一次解析（target/electron 是 GC root），打开设计窗口时不再跑 nix-shell（每次要好几秒）。
  ELECTRON_DEP = $(CURDIR)/target/electron
  ELECTRON = env -u LD_LIBRARY_PATH $(ELECTRON_DEP)/bin/electron $(CURDIR)/gui
  NPM_ENV = ELECTRON_SKIP_BINARY_DOWNLOAD=1
else
  RUN = sh -c
  ELECTRON_DEP =
  ELECTRON = $(CURDIR)/gui/node_modules/.bin/electron $(CURDIR)/gui
  NPM_ENV =
endif

.PHONY: help setup build release wasm gui run preview arrange studio wasm-bindgen snapshot check test fmt lint cross dist clean

help: ## 列出全部目标
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  make %-10s %s\n", $$1, $$2}'

setup: wasm-bindgen ## 装 wasm 编译目标、wasm-bindgen、设计窗口的 npm 依赖（build 时缺了会自动做）
	rustup target add wasm32-unknown-unknown
	cd gui && $(NPM_ENV) npm install --no-audit --no-fund

wasm-bindgen:
	@if [ "$$($(WASM_BINDGEN) --version 2>/dev/null)" != "wasm-bindgen $(WASM_BINDGEN_VERSION)" ]; then \
		echo "安装 wasm-bindgen $(WASM_BINDGEN_VERSION) 到 $(TOOLS)"; \
		cargo install wasm-bindgen-cli --version $(WASM_BINDGEN_VERSION) --locked --root $(TOOLS) || exit 1; \
	fi

gui/node_modules:
	cd gui && $(NPM_ENV) npm install --no-audit --no-fund

build: gui ## 编译 muse 和设计窗口（开发版）
	cargo build -p muse

release: ## 编译 muse（发布版）
	cargo build --release -p muse

wasm: wasm-bindgen ## 只编译设计窗口的 wasm 画布
	@rustup target list --installed | grep -qx wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown
	cd gui && WASM_BINDGEN=$(WASM_BINDGEN) node scripts/build-wasm.mjs

gui: wasm gui/node_modules ## 编译设计窗口（wasm + 类型检查 + vite）
	cd gui && npm run build:ui

run: build studio ## 作为壁纸运行，托盘里能打开设计窗口（ARGS="--pack jing-ye-si" 传选项）
	MUSE_GUI=$(STUDIO) $(RUN) "cargo run -p muse -- run $(ARGS)"

preview: ## 窗口预览
	$(RUN) "cargo run -p muse -- preview $(ARGS)"

arrange: build studio ## 打开设计窗口（壁纸没运行时窗口里也能启动）
	MUSE_GUI=$(STUDIO) MUSE_BIN=$(CURDIR)/target/debug/muse $(STUDIO)

studio: $(ELECTRON_DEP)
	@mkdir -p target
	@printf '#!/bin/sh\nexec %s\n' '$(ELECTRON)' > $(STUDIO)
	@chmod +x $(STUDIO)

snapshot: ## 截一帧到 out.png（ARGS="--pack jing-ye-si --time 40"）
	$(RUN) "cargo run -p muse -- snapshot out.png $(ARGS)"

check: fmt lint test cross ## 提交前的全部检查
	cd gui && npx tsc --noEmit

test: ## 跑测试
	cargo test

fmt: ## 格式化
	cargo fmt

lint: ## clippy（要求无警告）
	cargo clippy --all-targets -- -D warnings
	cargo clippy -p muse-web --target wasm32-unknown-unknown -- -D warnings

cross: ## macOS、Windows 交叉检查（target 没装就跳过）
	@for t in aarch64-apple-darwin x86_64-pc-windows-msvc; do \
		if rustup target list --installed | grep -qx $$t; then \
			echo "clippy $$t"; cargo clippy -p muse --target $$t -- -D warnings || exit 1; \
		else echo "跳过 $$t（rustup target add $$t）"; fi; \
	done

$(CURDIR)/target/electron:
	@mkdir -p target
	nix-build '<nixpkgs>' -A electron -o $@

dist: gui ## 打当前平台的安装包到 gui/release/（ARGS="--dir" 只生成目录）
	cd gui && node scripts/dist.mjs $(ARGS)

clean: ## 删掉编译产物
	cargo clean
	rm -rf gui/dist gui/release gui/src/wasm
