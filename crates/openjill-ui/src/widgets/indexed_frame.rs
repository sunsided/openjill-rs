use std::sync::{Arc, Mutex};

use eframe::CreationContext;
use egui::{PaintCallbackInfo, Rect, Response, Sense, Ui, Vec2};
use egui_wgpu::{Callback, CallbackResources, CallbackTrait, RenderState, ScreenDescriptor};
use openjill_render::{
    FRAMEBUFFER_HEIGHT, FRAMEBUFFER_PIXELS, FRAMEBUFFER_WIDTH, IndexedFramePainter, Palette,
};

/// Paints an OpenJill indexed framebuffer into an egui UI using `egui_wgpu`.
pub struct IndexedFrameCanvas {
    painter: Arc<Mutex<IndexedFramePainter>>,
}

impl IndexedFrameCanvas {
    /// Creates a canvas from eframe's WGPU creation context.
    pub fn from_creation_context(context: &CreationContext<'_>) -> Option<Self> {
        context
            .wgpu_render_state
            .as_ref()
            .map(Self::from_render_state)
    }

    /// Creates a canvas from an existing egui WGPU render state.
    pub fn from_render_state(render_state: &RenderState) -> Self {
        Self {
            painter: Arc::new(Mutex::new(IndexedFramePainter::new(
                &render_state.device,
                &render_state.queue,
                render_state.target_format,
                FRAMEBUFFER_WIDTH as u32,
                FRAMEBUFFER_HEIGHT as u32,
            ))),
        }
    }

    /// Shows the framebuffer at its native 320×200 logical size.
    pub fn show(&self, ui: &mut Ui, framebuffer: &[u8], palette: &Palette) -> Response {
        self.show_shared_sized(
            ui,
            Vec2::new(FRAMEBUFFER_WIDTH as f32, FRAMEBUFFER_HEIGHT as f32),
            Arc::<[u8]>::from(framebuffer),
            Arc::new(palette.clone()),
        )
    }

    /// Shows the framebuffer inside a sized egui paint callback.
    pub fn show_sized(
        &self,
        ui: &mut Ui,
        desired_size: Vec2,
        framebuffer: &[u8],
        palette: &Palette,
    ) -> Response {
        self.show_shared_sized(
            ui,
            desired_size,
            Arc::<[u8]>::from(framebuffer),
            Arc::new(palette.clone()),
        )
    }

    /// Shows shared framebuffer data without cloning it for the paint callback.
    pub fn show_shared(
        &self,
        ui: &mut Ui,
        framebuffer: Arc<[u8]>,
        palette: Arc<Palette>,
    ) -> Response {
        self.show_shared_sized(
            ui,
            Vec2::new(FRAMEBUFFER_WIDTH as f32, FRAMEBUFFER_HEIGHT as f32),
            framebuffer,
            palette,
        )
    }

    /// Shows shared framebuffer data inside a sized egui paint callback.
    pub fn show_shared_sized(
        &self,
        ui: &mut Ui,
        desired_size: Vec2,
        framebuffer: Arc<[u8]>,
        palette: Arc<Palette>,
    ) -> Response {
        let (rect, response) =
            ui.allocate_exact_size(desired_size.max(Vec2::splat(1.0)), Sense::hover());
        if !ui.is_rect_visible(rect) || framebuffer.len() != FRAMEBUFFER_PIXELS {
            return response;
        }

        ui.painter().add(Callback::new_paint_callback(
            rect,
            IndexedFrameCallback {
                painter: Arc::clone(&self.painter),
                viewport: rect,
                framebuffer,
                palette,
            },
        ));

        response
    }
}

struct IndexedFrameCallback {
    painter: Arc<Mutex<IndexedFramePainter>>,
    viewport: Rect,
    framebuffer: Arc<[u8]>,
    palette: Arc<Palette>,
}

impl CallbackTrait for IndexedFrameCallback {
    fn prepare(
        &self,
        _device: &egui_wgpu::wgpu::Device,
        queue: &egui_wgpu::wgpu::Queue,
        screen_descriptor: &ScreenDescriptor,
        _egui_encoder: &mut egui_wgpu::wgpu::CommandEncoder,
        _callback_resources: &mut CallbackResources,
    ) -> Vec<egui_wgpu::wgpu::CommandBuffer> {
        let [width, height] =
            viewport_size_in_pixels(self.viewport, screen_descriptor.pixels_per_point);
        let mut painter = self
            .painter
            .lock()
            .expect("openjill-ui canvas painter mutex poisoned");
        painter.resize(queue, width, height);
        let _ = painter.prepare(queue, &self.framebuffer, &self.palette);
        Vec::new()
    }

    fn paint(
        &self,
        _info: PaintCallbackInfo,
        render_pass: &mut egui_wgpu::wgpu::RenderPass<'static>,
        _callback_resources: &CallbackResources,
    ) {
        let painter = self
            .painter
            .lock()
            .expect("openjill-ui canvas painter mutex poisoned");
        painter.paint(render_pass);
    }
}

fn viewport_size_in_pixels(viewport: Rect, pixels_per_point: f32) -> [u32; 2] {
    [
        (viewport.width() * pixels_per_point).round().max(1.0) as u32,
        (viewport.height() * pixels_per_point).round().max(1.0) as u32,
    ]
}

#[cfg(test)]
mod tests {
    use super::viewport_size_in_pixels;

    #[test]
    fn viewport_size_in_pixels_rounds_logical_size() {
        assert_eq!(
            viewport_size_in_pixels(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 200.0)),
                1.5,
            ),
            [480, 300]
        );
    }

    #[test]
    fn viewport_size_in_pixels_clamps_zero_sized_rects() {
        assert_eq!(
            viewport_size_in_pixels(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO),
                2.0,
            ),
            [1, 1]
        );
    }
}
