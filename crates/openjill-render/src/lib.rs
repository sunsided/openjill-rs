#![forbid(unsafe_code)]

use std::sync::Arc;

use thiserror::Error;
use wgpu::{
    Backends, CompositeAlphaMode, Device, DeviceDescriptor, Features, Instance, InstanceDescriptor,
    Limits, PowerPreference, PresentMode, Queue, RequestAdapterOptions, Surface,
    SurfaceConfiguration, SurfaceError, TextureUsages,
};
use winit::window::Window;

/// Owns the wgpu instance, surface, and presentation state for the active window.
pub struct Presenter {
    /// WGPU instance that created this presenter surface.
    _instance: Instance,
    /// Surface bound to the active native window.
    surface: Surface<'static>,
    /// Logical GPU device used to encode and submit rendering commands.
    device: Device,
    /// GPU queue used to submit encoded command buffers.
    queue: Queue,
    /// Active swap-chain configuration used for presentation.
    surface_config: SurfaceConfiguration,
}

impl Presenter {
    /// Creates a presenter by resolving instance, surface, adapter, device, and queue.
    pub async fn new(window: Arc<Window>) -> Result<Self, PresenterError> {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            ..InstanceDescriptor::default()
        });
        let window_size = clamp_surface_size(window.inner_size().width, window.inner_size().height);
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::None,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or(PresenterError::NoCompatibleAdapter)?;
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("openjill-render-device"),
                    required_features: Features::empty(),
                    required_limits: Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = *capabilities
            .formats
            .first()
            .ok_or(PresenterError::NoSurfaceFormats)?;
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: window_size.0,
            height: window_size.1,
            present_mode: PresentMode::Fifo,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);
        Ok(Self {
            _instance: instance,
            surface,
            device,
            queue,
            surface_config,
        })
    }

    /// Reconfigures the presenter surface for a new window size.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = clamp_surface_size(width, height);
        if self.surface_config.width == width && self.surface_config.height == height {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Renders one frame with a black clear color and presents it to the window surface.
    pub fn present(&mut self) -> Result<(), PresenterError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("openjill-render-present-encoder"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("openjill-render-present-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }
}

/// Errors returned by the renderer presenter lifecycle.
#[derive(Debug, Error)]
pub enum PresenterError {
    /// Surface creation failed for the supplied native window.
    #[error("failed to create wgpu surface: {0}")]
    SurfaceCreation(#[from] wgpu::CreateSurfaceError),
    /// No compatible adapter could be selected for the current window surface.
    #[error("no compatible GPU adapter found for the active window surface")]
    NoCompatibleAdapter,
    /// Device or queue creation failed after adapter selection.
    #[error("failed to create wgpu device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    /// Surface capability enumeration returned no usable formats.
    #[error("wgpu surface reported no supported texture formats")]
    NoSurfaceFormats,
    /// Frame acquisition or presentation failed.
    #[error("surface presentation failed: {0}")]
    SurfaceError(#[from] SurfaceError),
}

/// Clamps zero-sized window dimensions to the minimum valid surface extent.
fn clamp_surface_size(width: u32, height: u32) -> (u32, u32) {
    (width.max(1), height.max(1))
}

#[cfg(test)]
/// Unit tests for presenter helper behavior.
mod tests {
    use super::clamp_surface_size;

    /// Verifies zero-sized dimensions are promoted to one pixel for surface safety.
    #[test]
    fn clamp_surface_size_promotes_zero_to_one() {
        assert_eq!(clamp_surface_size(0, 0), (1, 1));
        assert_eq!(clamp_surface_size(320, 200), (320, 200));
    }
}
