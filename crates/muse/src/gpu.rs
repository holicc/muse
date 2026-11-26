//! wgpu 初始化。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 显卡设备丢失（驱动重置、休眠唤醒后失效等），前端看到后重建 GPU 和所有表面。
static DEVICE_LOST: AtomicBool = AtomicBool::new(false);

/// 取走「设备丢失」标记。
pub fn take_device_lost() -> bool {
    DEVICE_LOST.swap(false, Ordering::Relaxed)
}

pub fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle())
}

/// 选适配器并创建设备。壁纸常驻后台，优先省电的集成显卡。
pub fn init_with(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: surface,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("找不到可用的显卡: {e}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("muse"),
        required_limits:
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .map_err(|e| format!("创建显卡设备失败: {e}"))?;
    // wgpu 默认遇到未捕获的错误直接 panic；壁纸常驻后台，只记日志，设备丢失时由前端重建。
    device.on_uncaptured_error(Arc::new(|e| eprintln!("显卡错误: {e}")));
    device.set_device_lost_callback(|reason, msg| {
        if reason != wgpu::DeviceLostReason::Destroyed {
            eprintln!("显卡设备丢失: {msg}");
            DEVICE_LOST.store(true, Ordering::Relaxed);
        }
    });
    Ok((adapter, device, queue))
}

/// 不需要窗口时（截图）用。
pub fn init(
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
    init_with(&instance(), surface)
}

/// 为窗口表面挑配置：sRGB 格式、不透明、尽量不阻塞的呈现模式。
pub fn surface_config(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
) -> wgpu::SurfaceConfiguration {
    let caps = surface.get_capabilities(adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(caps.formats[0]);
    let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
        wgpu::CompositeAlphaMode::Opaque
    } else {
        caps.alpha_modes[0]
    };
    let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    };
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: width.max(1),
        height: height.max(1),
        present_mode,
        desired_maximum_frame_latency: 2,
        alpha_mode,
        view_formats: vec![],
        color_space: Default::default(),
    }
}
