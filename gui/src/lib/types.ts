// 和 muse 统一接口（crates/muse/src/api.rs）对应的数据类型。

export type PaintDoc = string | string[];

/** 一首诗的样式，键名和文件里一致。 */
export interface StyleDoc {
  colors: Record<string, PaintDoc>;
  wallpaper?: string;
  offsets: Record<string, [number, number]>;
  sizes: Record<string, number>;
  opacity: Record<string, number>;
}

export interface Poem {
  name: string;
  dir: string;
  title: string;
  author: string;
  dynasty: string;
  lines: string[];
  source: string | null;
  /** 诗词包指定的版式；null 表示跟随全局。 */
  style: 'magazine' | 'classic' | null;
  orientation: 'vertical' | 'horizontal';
  position: 'left' | 'center' | 'right';
  particles: string | null;
  layers: string[];
}

export interface Config {
  packs: string | null;
  font: string | null;
  wallpaper: string; // auto / pack / 路径
  text_color: string; // auto / #rrggbb
  style: 'magazine' | 'classic';
  text_scale: number;
  start: string | null;
  single: boolean;
  fps: number;
  battery_fps: number;
  pause_on_fullscreen: boolean;
  hold: number;
  char_interval: number;
  tray: boolean;
}

/** 设计窗口能改的全局设置项。 */
export type SettingKey =
  | 'wallpaper'
  | 'font'
  | 'style'
  | 'text_scale'
  | 'text_color'
  | 'hold'
  | 'char_interval'
  | 'fps'
  | 'battery_fps'
  | 'pause_on_fullscreen'
  | 'tray';

export interface Arrangement {
  x: number | null;
  y: number | null;
  scale: number;
}

export interface FontInfo {
  description: string;
  files: string[];
  faces: [number, number][];
}

export interface Status {
  version: string;
  paused: boolean;
  index: number;
  total: number;
  pack: string;
  title: string;
  author: string;
}

export interface State {
  status: Status;
  config: Config;
  sources: Record<string, string>;
  warnings: string[];
  paths: { config: string | null; settings: string | null; colors_dir: string | null; layout_dir: string | null };
  poems: Poem[];
  styles: Record<string, StyleDoc>;
  arrangement: Arrangement;
  font: FontInfo;
  wallpaper: { global: string | null; desktop: string | null };
}

export interface Display {
  width: number;
  height: number;
  scale: number;
  label: string;
}

/** 画布上的部件框（画布像素）。 */
export interface PartRect {
  part: string;
  rect: [number, number, number, number];
}

export interface CharRect {
  part: string;
  index: number;
  ch: string;
  rect: [number, number, number, number];
}

export interface AutoColor {
  luma: number;
  light: boolean;
  ink: string;
  on_wallpaper: boolean;
}

declare global {
  interface Window {
    muse: {
      connect(): Promise<boolean>;
      start(): Promise<boolean>;
      call<T = unknown>(method: string, params?: unknown): Promise<T>;
      onEvent(fn: (msg: { event: string; data?: any }) => void): void;
      readFile(path: string): Promise<Uint8Array>;
      readText(path: string): Promise<string>;
      display(): Promise<Display>;
      pickImage(): Promise<string | null>;
      quit(): Promise<void>;
      onCloseRequest(fn: () => void): void;
      theme(): Promise<'light' | 'dark'>;
      onTheme(fn: (t: 'light' | 'dark') => void): void;
      platform: string;
      log(m: unknown): void;
    };
  }
}

export const emptyStyle = (): StyleDoc => ({ colors: {}, offsets: {}, sizes: {}, opacity: {} });
