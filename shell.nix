# NixOS 开发环境：wgpu / winit / Wayland 在运行时 dlopen 这些库，需要放进 LD_LIBRARY_PATH。
{ pkgs ? import <nixpkgs> { } }:
let
  runtimeLibs = with pkgs; [
    vulkan-loader
    libGL
    wayland
    libxkbcommon
    # 新版 nixpkgs 改名，旧版仍在 xorg 下。
    (pkgs.libx11 or pkgs.xorg.libX11)
    (pkgs.libxcursor or pkgs.xorg.libXcursor)
    (pkgs.libxi or pkgs.xorg.libXi)
    (pkgs.libxrandr or pkgs.xorg.libXrandr)
  ];
in
pkgs.mkShell {
  # make 跑仓库里的 Makefile；nodejs 编译设计窗口。
  packages = with pkgs; [ gnumake nodejs ];
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
}
