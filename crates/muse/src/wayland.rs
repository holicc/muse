//! Linux Wayland 壁纸：每个显示器一个 background 层的 layer-shell 表面。
//!
//! 需要合成器支持 wlr-layer-shell（niri、Sway、Hyprland、KDE 等；GNOME 不支持）。
//! 帧调度：每次提交画面前向合成器要一个 frame callback。到了下一帧的时间（按设置的帧率，
//! 电池供电时降帧），上一帧的回调已经回来才画；表面被完全遮住或熄屏时合成器不回调，
//! 渲染就自然停下，进程只在定期检查电源状态时醒来。

use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::{Duration, Instant};

use muse_render::Renderer;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, FrameCallbackData};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_output, wl_surface};
use wayland_client::{Connection, Proxy, QueueHandle};

use crate::control::Command;
use crate::power::Pacer;
use crate::{Session, gpu, tray};

struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// 一个显示器上的壁纸。
///
/// 字段按声明顺序释放：wgpu 表面引用着 wl_surface，所以 `surface` 必须在 `layer` 之前。
struct OutputView {
    renderer: Option<Renderer>,
    surface: wgpu::Surface<'static>,
    layer: LayerSurface,
    output: wl_output::WlOutput,
    config: Option<wgpu::SurfaceConfiguration>,
    /// 逻辑尺寸和整数缩放。
    logical: (u32, u32),
    scale: i32,
    last_frame: Instant,
    /// 上一帧要的 frame callback 还没回来（表面可能不可见）。
    frame_pending: bool,
    /// 下一帧的时刻；None 表示暂停。
    next_due: Option<Instant>,
    /// 尺寸变化、切换诗词等，暂停时也要重画一帧。
    dirty: bool,
}

struct App {
    qh: QueueHandle<App>,
    registry: RegistryState,
    outputs: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    conn: Connection,
    instance: wgpu::Instance,
    gpu: Option<Gpu>,
    session: Session,
    pacer: Pacer,
    paused: bool,
    tray: Option<tray::Tray>,
    /// 设备丢失后重建失败（驱动重置、刚唤醒），到这个时刻再试。
    gpu_retry: Option<Instant>,
    /// 显卡初始化成功过；之后的失败只重试，不退出。
    had_gpu: bool,
    views: Vec<OutputView>,
    error: Option<String>,
}

pub fn run(session: Session) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("连接 Wayland 失败: {e}"))?;
    let (globals, mut queue) =
        registry_queue_init(&conn).map_err(|e| format!("读取 Wayland 全局对象失败: {e}"))?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|e| format!("合成器不支持 wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|_| {
        "合成器不支持 wlr-layer-shell（GNOME 不支持），无法作为壁纸运行；可以用 muse preview 预览".to_string()
    })?;
    // 其他线程（托盘、控制 socket）发来命令时，往这对 socket 里写一个字节唤醒 poll。
    let (wake_rx, wake_tx) = UnixStream::pair().map_err(|e| format!("创建唤醒通道失败: {e}"))?;
    wake_rx
        .set_nonblocking(true)
        .and_then(|_| wake_tx.set_nonblocking(true))
        .map_err(|e| format!("创建唤醒通道失败: {e}"))?;
    session
        .inbox
        .set_waker(Arc::new(move || drop((&wake_tx).write(&[1]))));
    let tray = session
        .config
        .tray
        .then(|| {
            tray::create(
                session.remote.clone(),
                session.icon(),
                session.overrides.clone(),
            )
        })
        .flatten();
    let mut app = App {
        qh: qh.clone(),
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        compositor,
        layer_shell,
        conn,
        instance: gpu::instance(),
        gpu: None,
        pacer: Pacer::new(&session.config),
        session,
        paused: false,
        tray,
        gpu_retry: None,
        had_gpu: false,
        views: Vec::new(),
        error: None,
    };
    let wl_err = |e: &dyn std::fmt::Display| format!("Wayland 连接出错: {e}");
    loop {
        while let Some(c) = app.session.inbox.try_recv() {
            if c == Command::Quit {
                return Ok(());
            }
            app.handle(c);
        }
        app.tick();
        queue.flush().map_err(|e| wl_err(&e))?;
        queue.dispatch_pending(&mut app).map_err(|e| wl_err(&e))?;
        if let Some(guard) = queue.prepare_read() {
            let timeout = app
                .next_deadline()
                .saturating_duration_since(Instant::now());
            let readable =
                wait_readable(guard.connection_fd(), &wake_rx, timeout).map_err(|e| wl_err(&e))?;
            // 清空唤醒字节。
            let _ = (&wake_rx).read(&mut [0u8; 64]);
            if readable {
                match guard.read() {
                    Ok(_) => {}
                    Err(wayland_client::backend::WaylandError::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(wl_err(&e)),
                }
            }
        }
        queue.dispatch_pending(&mut app).map_err(|e| wl_err(&e))?;
        if let Some(e) = app.error.take() {
            return Err(e);
        }
    }
}

/// 等 Wayland 连接可读或被唤醒，最多等 `timeout`。返回 Wayland 连接是否可读。
fn wait_readable(fd: impl AsFd, wake: impl AsFd, timeout: Duration) -> std::io::Result<bool> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let ts = Timespec {
        tv_sec: timeout.as_secs() as _,
        tv_nsec: timeout.subsec_nanos() as _,
    };
    let mut fds = [
        PollFd::new(&fd, PollFlags::IN),
        PollFd::new(&wake, PollFlags::IN),
    ];
    loop {
        match poll(&mut fds, Some(&ts)) {
            Ok(_) => return Ok(fds[0].revents().contains(PollFlags::IN)),
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

impl App {
    fn add_output(&mut self, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if self.views.iter().any(|v| v.output == output) {
            return;
        }
        let wl_surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            wl_surface,
            Layer::Background,
            Some("muse"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(0, 0);
        // 壁纸不接收任何输入。
        if let Ok(region) = smithay_client_toolkit::compositor::Region::new(&self.compositor) {
            layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        layer.commit();

        let display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(self.conn.backend().display_ptr().cast()).expect("display 指针为空"),
        ));
        let window = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(layer.wl_surface().id().as_ptr().cast()).expect("surface 指针为空"),
        ));
        // 安全性：surface 在 OutputView 里先于 layer 释放（见字段顺序）。
        let surface = match unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(display),
                    raw_window_handle: window,
                })
        } {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(format!("创建绘制表面失败: {e}"));
                return;
            }
        };
        let scale = self
            .outputs
            .info(&output)
            .map_or(1, |i| i.scale_factor.max(1));
        self.views.push(OutputView {
            output,
            layer,
            surface,
            config: None,
            renderer: None,
            logical: (0, 0),
            scale,
            last_frame: Instant::now(),
            frame_pending: false,
            next_due: None,
            dirty: true,
        });
    }

    fn view_index(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.views
            .iter()
            .position(|v| v.layer.wl_surface() == surface)
    }

    /// 按当前逻辑尺寸和缩放重新配置表面。
    fn reconfigure(&mut self, i: usize) {
        if self.gpu.is_none() {
            match gpu::init_with(&self.instance, Some(&self.views[i].surface)) {
                Ok((adapter, device, queue)) => {
                    eprintln!("显卡: {}", adapter.get_info().name);
                    self.had_gpu = true;
                    self.gpu = Some(Gpu {
                        adapter,
                        device,
                        queue,
                    });
                }
                Err(e) if !self.had_gpu => {
                    // 启动时就拿不到显卡：直接报错退出。
                    self.error = Some(e);
                    return;
                }
                Err(e) => {
                    eprintln!("{e}，稍后重试");
                    self.gpu_retry = Some(Instant::now() + crate::power::CHECK_INTERVAL);
                    return;
                }
            }
        }
        let gpu = self.gpu.as_ref().expect("已初始化");
        let v = &mut self.views[i];
        let (w, h) = (v.logical.0 * v.scale as u32, v.logical.1 * v.scale as u32);
        if w == 0 || h == 0 {
            return;
        }
        v.layer.wl_surface().set_buffer_scale(v.scale);
        let config = gpu::surface_config(&v.surface, &gpu.adapter, w, h);
        v.surface.configure(&gpu.device, &config);
        let s = &self.session;
        let renderer = v.renderer.get_or_insert_with(|| {
            s.renderer(gpu.device.clone(), gpu.queue.clone(), config.format)
        });
        renderer.resize(w, h);
        v.config = Some(config);
    }

    fn handle(&mut self, c: Command) {
        match c {
            Command::Next | Command::Prev | Command::Goto(_) => {
                for i in 0..self.views.len() {
                    if let Some(r) = &mut self.views[i].renderer {
                        match c {
                            Command::Next => r.next_pack(),
                            Command::Prev => r.prev_pack(),
                            Command::Goto(n) => r.set_pack(n),
                            _ => {}
                        }
                    }
                    self.redraw_soon(i);
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
                for i in 0..self.views.len() {
                    if let Some(r) = &mut self.views[i].renderer {
                        if let Some(f) = &font {
                            r.set_font(f.clone());
                        }
                        r.reconfigure(&self.session.options);
                    }
                    self.redraw_soon(i);
                }
                self.pacer = Pacer::new(&self.session.config);
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
            Command::Quit => {}
        }
        self.report();
    }

    /// 把当前第几首、是否暂停告诉对外接口。
    fn report(&self) {
        if let Some(r) = self.views.iter().find_map(|v| v.renderer.as_ref()) {
            self.session.report(r.current_index(), self.paused);
        }
    }

    /// 两帧间隔；None 表示暂停。
    fn interval(&self, renderer: &Renderer) -> Option<Duration> {
        if self.paused {
            None
        } else {
            self.pacer.frame_interval(renderer.is_animating())
        }
    }

    /// 画所有到点、且上一帧回调已经回来的表面。
    fn tick(&mut self) {
        self.pacer.refresh();
        let retry = self.gpu_retry.is_some_and(|t| t <= Instant::now());
        if gpu::take_device_lost() || retry {
            eprintln!("重建显卡设备");
            self.gpu_retry = None;
            for v in &mut self.views {
                v.renderer = None;
                v.config = None;
                // 丢失前请求的回调可能永远不会回来。
                v.frame_pending = false;
                v.dirty = true;
                v.next_due = None;
            }
            self.gpu = None;
            for i in 0..self.views.len() {
                if self.gpu_retry.is_some() {
                    break;
                }
                self.reconfigure(i);
            }
        }
        for i in 0..self.views.len() {
            self.maybe_render(i);
        }
    }

    fn maybe_render(&mut self, i: usize) {
        let v = &self.views[i];
        if v.frame_pending {
            return;
        }
        let Some(r) = &v.renderer else { return };
        let reached = v.next_due.is_none_or(|d| d <= Instant::now());
        let due = match self.interval(r) {
            None => v.dirty && reached,
            Some(_) => reached,
        };
        if due {
            self.render(i);
        }
    }

    /// 最近一个要做事的时刻（画下一帧或重新检查电源状态），poll 最多等到那时。
    /// 正等回调的表面不算，回调到了会唤醒 poll。
    fn next_deadline(&self) -> Instant {
        let now = Instant::now();
        self.views
            .iter()
            .filter(|v| !v.frame_pending)
            .filter_map(|v| {
                let r = v.renderer.as_ref()?;
                if v.dirty {
                    Some(v.next_due.unwrap_or(now))
                } else {
                    // 暂停（或用电池且 battery_fps = 0）时不按帧唤醒，只等电源检查或命令。
                    self.interval(r)?;
                    v.next_due
                }
            })
            .chain(self.gpu_retry)
            .fold(self.pacer.next_check(), Instant::min)
    }

    /// 尺寸、缩放变化或切换诗词后尽快重画，暂停时也画。
    fn redraw_soon(&mut self, i: usize) {
        self.views[i].dirty = true;
        self.views[i].next_due = None;
        self.maybe_render(i);
    }

    /// 推进动画、画一帧，并定好下一帧的时间。
    fn render(&mut self, i: usize) {
        let Some(gpu) = &self.gpu else { return };
        let paused = self.paused;
        let v = &mut self.views[i];
        let (Some(renderer), Some(config)) = (&mut v.renderer, &v.config) else {
            return;
        };
        let now = Instant::now();
        let dt = now.duration_since(v.last_frame).as_secs_f32();
        v.last_frame = now;
        let mut switched = false;
        if !paused {
            // 被遮挡很久后恢复时不追帧，从停下的地方接着播；上限要大于静止时的刷新间隔，
            // 否则停留阶段计时会变慢。
            let before = renderer.current_index();
            renderer.advance(dt.min(1.0));
            switched = renderer.current_index() != before;
        }
        let interval = if paused {
            None
        } else {
            self.pacer.frame_interval(renderer.is_animating())
        };
        v.next_due = interval.map(|d| now + d);
        v.dirty = false;

        let frame = match v.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                v.surface.configure(&gpu.device, config);
                // 这一帧没画成，下次再试（暂停时也要补画）。
                v.dirty = true;
                v.next_due = Some(now + Duration::from_millis(50));
                return;
            }
            _ => {
                v.dirty = true;
                v.next_due = Some(now + Duration::from_millis(50));
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        renderer.render(&view);
        // 回调要在 present 提交之前请求，才和这一帧绑定。
        let wl = v.layer.wl_surface();
        wl.frame(&self.qh, FrameCallbackData(wl.clone()));
        v.frame_pending = true;
        gpu.queue.present(frame);
        if switched {
            self.report();
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(i) = self.view_index(surface) {
            self.views[i].scale = factor.max(1);
            self.reconfigure(i);
            self.redraw_soon(i);
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(i) = self.view_index(surface) {
            self.views[i].frame_pending = false;
            self.maybe_render(i);
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.add_output(qh, output);
    }

    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let scale = self
            .outputs
            .info(&output)
            .map_or(1, |i| i.scale_factor.max(1));
        if let Some(i) = self.views.iter().position(|v| v.output == output)
            && self.views[i].scale != scale
        {
            self.views[i].scale = scale;
            self.reconfigure(i);
            self.redraw_soon(i);
        }
    }

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.views.retain(|v| v.output != output);
    }
}

impl LayerShellHandler for App {
    /// 合成器关掉了表面（比如重新配置显示器）。显示器还在就重建一个。
    fn closed(&mut self, _: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface) {
        let Some(i) = self.views.iter().position(|v| &v.layer == layer) else {
            return;
        };
        let output = self.views.remove(i).output;
        if self.outputs.info(&output).is_some() {
            self.add_output(qh, output);
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(i) = self.views.iter().position(|v| &v.layer == layer) else {
            return;
        };
        let (w, h) = configure.new_size;
        let first = self.views[i].config.is_none();
        if self.views[i].logical != (w, h) || first {
            self.views[i].logical = (w, h);
            self.reconfigure(i);
        }
        if first {
            // 第一帧直接画，表面要有内容才会被映射，之后由 tick 调度。
            self.render(i);
        } else {
            self.redraw_soon(i);
        }
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(App);
