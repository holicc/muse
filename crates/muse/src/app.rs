//! winit 前端：窗口预览（所有平台），以及 X11、macOS、Windows 上的桌面层壁纸。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use muse_render::Renderer;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowId};

use crate::control::Command;
use crate::power::{CHECK_INTERVAL, Pacer};
use crate::{Session, desktop, gpu, tray};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 普通窗口，用来编写和调试诗词包。
    Preview,
    /// 每个显示器一个桌面层窗口。
    Desktop,
}

/// 用来从其他线程唤醒事件循环。
#[derive(Debug, Clone, Copy)]
struct Wake;

struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// 一个窗口。字段按声明顺序释放：渲染器、表面先于窗口。
struct Win {
    renderer: Renderer,
    surface: wgpu::Surface<'static>,
    window: Arc<Window>,
    config: wgpu::SurfaceConfiguration,
    occluded: bool,
    last_frame: Instant,
    next_due: Option<Instant>,
}

struct App {
    mode: Mode,
    session: Session,
    gpu: Option<Gpu>,
    wins: Vec<Win>,
    pacer: Pacer,
    paused: bool,
    tray: Option<tray::Tray>,
    /// 显示器布局，变化时重建窗口。
    monitors: Vec<(i32, i32, u32, u32)>,
    next_monitor_check: Instant,
    /// 窗口被系统销毁（比如 Windows 上 Explorer 重启），需要重建。
    rebuild: bool,
    error: Option<String>,
}

pub fn run(mode: Mode, session: Session) -> Result<(), String> {
    let mut builder = EventLoop::<Wake>::with_user_event();
    if mode == Mode::Desktop {
        #[cfg(target_os = "macos")]
        {
            // 不在 Dock 里显示图标。
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder.with_activation_policy(ActivationPolicy::Accessory);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // Wayland 走 layer-shell 前端，这里只处理 X11。
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_x11();
        }
    }
    let event_loop = builder
        .build()
        .map_err(|e| format!("无法创建窗口事件循环: {e}"))?;
    let proxy = Mutex::new(event_loop.create_proxy());
    session.inbox.set_waker(Arc::new(move || {
        let _ = proxy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .send_event(Wake);
    }));
    let pacer = Pacer::new(&session.config);
    let mut app = App {
        mode,
        session,
        gpu: None,
        wins: Vec::new(),
        pacer,
        paused: false,
        tray: None,
        monitors: Vec::new(),
        next_monitor_check: Instant::now() + CHECK_INTERVAL,
        rebuild: false,
        error: None,
    };
    let result = event_loop.run_app(&mut app);
    app.wins.clear();
    if mode == Mode::Desktop {
        desktop::restore();
    }
    result.map_err(|e| format!("窗口事件循环出错: {e}"))?;
    app.error.map_or(Ok(()), Err)
}

fn monitor_layout(el: &ActiveEventLoop) -> Vec<(i32, i32, u32, u32)> {
    let mut v: Vec<_> = el
        .available_monitors()
        .map(|m| {
            let (p, s) = (m.position(), m.size());
            (p.x, p.y, s.width, s.height)
        })
        .collect();
    v.sort();
    v
}

impl App {
    fn create_windows(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        self.wins.clear();
        match self.mode {
            Mode::Preview => {
                let attrs = Window::default_attributes()
                    .with_title("muse 预览")
                    .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
                let w = el
                    .create_window(attrs)
                    .map_err(|e| format!("创建窗口失败: {e}"))?;
                self.add_window(w)?;
            }
            Mode::Desktop => {
                let layout = monitor_layout(el);
                let monitors: Vec<MonitorHandle> = el.available_monitors().collect();
                if monitors.is_empty() {
                    return Err("没有找到显示器".into());
                }
                for m in &monitors {
                    let w = el
                        .create_window(desktop::attributes(m))
                        .map_err(|e| format!("创建窗口失败: {e}"))?;
                    // 挂不到桌面层的窗口会变成盖住整个屏幕的普通窗口，不能留着。
                    desktop::attach(&w, m)?;
                    self.add_window(w)?;
                }
                // 全部成功才记下布局；失败时下次检查会重试。
                self.monitors = layout;
            }
        }
        if let Some(w) = self.wins.first() {
            set_title(w, self.mode);
        }
        Ok(())
    }

    fn add_window(&mut self, window: Window) -> Result<(), String> {
        let window = Arc::new(window);
        let instance = match &self.gpu {
            Some(g) => g.instance.clone(),
            None => gpu::instance(),
        };
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("创建绘制表面失败: {e}"))?;
        if self.gpu.is_none() {
            let (adapter, device, queue) = gpu::init_with(&instance, Some(&surface))?;
            eprintln!("显卡: {}", adapter.get_info().name);
            self.gpu = Some(Gpu {
                instance,
                adapter,
                device,
                queue,
            });
        }
        let g = self.gpu.as_ref().expect("已初始化");
        let size = window.inner_size();
        let config = gpu::surface_config(&surface, &g.adapter, size.width, size.height);
        surface.configure(&g.device, &config);
        let s = &self.session;
        let mut renderer = s.renderer(g.device.clone(), g.queue.clone(), config.format);
        renderer.resize(config.width, config.height);
        self.wins.push(Win {
            renderer,
            surface,
            window,
            config,
            occluded: false,
            last_frame: Instant::now(),
            next_due: Some(Instant::now()),
        });
        Ok(())
    }

    fn handle(&mut self, el: &ActiveEventLoop, c: Command) {
        let switch = |r: &mut Renderer| match c {
            Command::Next => r.next_pack(),
            Command::Prev => r.prev_pack(),
            Command::Goto(i) => r.set_pack(i),
            _ => {}
        };
        match c {
            Command::Next | Command::Prev | Command::Goto(_) => {
                for w in &mut self.wins {
                    switch(&mut w.renderer);
                    w.window.request_redraw();
                }
                if let Some(w) = self.wins.first() {
                    set_title(w, self.mode);
                }
            }
            Command::TogglePause | Command::SetPaused(_) => {
                self.paused = match c {
                    Command::SetPaused(p) => p,
                    _ => !self.paused,
                };
                if let Some(t) = &self.tray {
                    t.set_paused(self.paused);
                }
            }
            Command::Reload => {
                let font = crate::reload_session(&mut self.session);
                for w in &mut self.wins {
                    if let Some(f) = &font {
                        w.renderer.set_font(f.clone());
                    }
                    w.renderer.reconfigure(&self.session.options);
                    w.window.request_redraw();
                }
                self.pacer = Pacer::new(&self.session.config);
                self.sync_tray();
            }
            Command::Quit => el.exit(),
        }
        self.report();
    }

    /// 托盘按设置显示或隐藏（只有桌面壁纸模式有托盘）。
    fn sync_tray(&mut self) {
        if self.mode != Mode::Desktop {
            return;
        }
        match (self.session.config.tray, self.tray.is_some()) {
            (true, false) => {
                self.tray = tray::create(
                    self.session.remote.clone(),
                    self.session.icon(),
                    self.session.overrides.clone(),
                );
                if let Some(t) = &self.tray {
                    t.set_paused(self.paused);
                }
            }
            (false, true) => self.tray = None,
            _ => {}
        }
    }

    /// 把当前第几首、是否暂停告诉对外接口。
    fn report(&self) {
        if let Some(w) = self.wins.first() {
            self.session.report(w.renderer.current_index(), self.paused);
        }
    }
}

impl ApplicationHandler<Wake> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if !self.wins.is_empty() {
            return;
        }
        if let Err(e) = self.create_windows(el) {
            self.error = Some(e);
            el.exit();
            return;
        }
        self.sync_tray();
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: Wake) {
        // 命令在 about_to_wait 里统一处理。
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(i) = self.wins.iter().position(|w| w.window.id() == id) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Destroyed if self.mode == Mode::Desktop => {
                eprintln!("壁纸窗口被系统销毁，重建");
                self.rebuild = true;
            }
            WindowEvent::Resized(size) => {
                let Some(g) = &self.gpu else { return };
                let w = &mut self.wins[i];
                w.config.width = size.width.max(1);
                w.config.height = size.height.max(1);
                w.surface.configure(&g.device, &w.config);
                w.renderer.resize(w.config.width, w.config.height);
                w.window.request_redraw();
            }
            WindowEvent::Occluded(occluded) => {
                let w = &mut self.wins[i];
                w.occluded = occluded;
                if !occluded {
                    w.next_due = Some(Instant::now());
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } if self.mode == Mode::Preview => match logical_key {
                Key::Named(NamedKey::Escape) => self.handle(el, Command::Quit),
                Key::Named(NamedKey::Space) => self.handle(el, Command::TogglePause),
                Key::Character(c) if c.eq_ignore_ascii_case("n") => self.handle(el, Command::Next),
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                let paused = self.paused;
                let Some(g) = &self.gpu else { return };
                let w = &mut self.wins[i];
                let now = Instant::now();
                let dt = now.duration_since(w.last_frame).as_secs_f32();
                w.last_frame = now;
                if !paused && !w.occluded {
                    let before = w.renderer.current_index();
                    w.renderer.advance(dt.min(0.25));
                    if i == 0 && w.renderer.current_index() != before {
                        set_title(w, self.mode);
                        self.session
                            .report(self.wins[0].renderer.current_index(), paused);
                    }
                }
                let w = &mut self.wins[i];
                draw(w, g);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        while let Some(c) = self.session.inbox.try_recv() {
            self.handle(el, c);
        }
        self.pacer.refresh();
        if gpu::take_device_lost() || self.rebuild {
            self.rebuild = false;
            self.wins.clear();
            self.gpu = None;
            if let Err(e) = self.create_windows(el) {
                eprintln!("{e}");
                // 过一会儿再试。
                self.next_monitor_check = Instant::now() + CHECK_INTERVAL;
                self.monitors.clear();
            }
        }
        let now = Instant::now();
        if self.mode == Mode::Desktop && now >= self.next_monitor_check {
            self.next_monitor_check = now + CHECK_INTERVAL;
            if monitor_layout(el) != self.monitors {
                eprintln!("显示器布局变化，重建窗口");
                if let Err(e) = self.create_windows(el) {
                    eprintln!("{e}");
                }
            }
        }

        let mut wake = self.pacer.next_check();
        if self.mode == Mode::Desktop {
            wake = wake.min(self.next_monitor_check);
        }
        for w in &mut self.wins {
            if self.paused || w.occluded {
                continue;
            }
            let Some(interval) = self.pacer.frame_interval(w.renderer.is_animating()) else {
                continue;
            };
            let due = w.next_due.unwrap_or(now);
            if due <= now {
                // 下一帧的时间在请求时就定好；被遮挡导致重绘迟迟不来时也不会空转。
                w.window.request_redraw();
                let next = now + interval;
                w.next_due = Some(next);
                wake = wake.min(next);
            } else {
                wake = wake.min(due);
            }
        }
        el.set_control_flow(ControlFlow::WaitUntil(
            wake.max(now + Duration::from_millis(1)),
        ));
    }
}

fn set_title(w: &Win, _mode: Mode) {
    let poem = &w.renderer.current_pack().pack.poem;
    w.window.set_title(&format!(
        "muse 预览 · {}（{}·{}）",
        poem.title, poem.dynasty, poem.author
    ));
}

fn draw(w: &mut Win, g: &Gpu) {
    let frame = match w.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            w.config = gpu::surface_config(&w.surface, &g.adapter, w.config.width, w.config.height);
            w.surface.configure(&g.device, &w.config);
            return;
        }
        _ => return,
    };
    let view = frame.texture.create_view(&Default::default());
    w.renderer.render(&view);
    w.window.pre_present_notify();
    g.queue.present(frame);
}
