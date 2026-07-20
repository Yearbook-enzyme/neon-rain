use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Wrap,
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OverlayVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl OverlayVertex {
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x2,
            1 => Float32x4,
        ],
    };
}

pub struct StatusOverlay {
    enabled: bool,
    remaining: f32,
    panel_pipeline: wgpu::RenderPipeline,
    panel_vertex_buffer: wgpu::Buffer,
    panel_vertex_count: u32,
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    message_buffer: Buffer,
    width: u32,
    height: u32,
    scale_factor: f64,
    panel_left: f32,
    panel_top: f32,
    panel_right: f32,
    panel_bottom: f32,
    text_left: f32,
    text_top: f32,
}

impl StatusOverlay {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Status overlay shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("status_overlay.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Status overlay pipeline layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });

        let panel_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Status overlay panel pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(OverlayVertex::LAYOUT)],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let panel_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Status overlay panel vertices"),
            size: (12 * size_of::<OverlayVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, target_format);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let message_buffer = Buffer::new(&mut font_system, Metrics::new(17.0, 24.0));

        let mut overlay = Self {
            enabled: true,
            remaining: 0.0,
            panel_pipeline,
            panel_vertex_buffer,
            panel_vertex_count: 0,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            message_buffer,
            width,
            height,
            scale_factor,
            panel_left: 0.0,
            panel_top: 0.0,
            panel_right: 0.0,
            panel_bottom: 0.0,
            text_left: 0.0,
            text_top: 0.0,
        };

        overlay.resize(queue, width, height, scale_factor);
        overlay
    }

    pub fn show(&mut self, queue: &wgpu::Queue, message: &str) {
        if !self.enabled {
            return;
        }

        self.remaining = 3.6;
        self.configure_message(message);
        self.resize(queue, self.width, self.height, self.scale_factor);
    }

    pub fn update(&mut self, dt: f32) {
        self.remaining = (self.remaining - dt.max(0.0)).max(0.0);
    }

    pub fn toggle_enabled(&mut self) -> bool {
        self.enabled = !self.enabled;
        if !self.enabled {
            self.remaining = 0.0;
        }
        self.enabled
    }

    pub fn resize(&mut self, queue: &wgpu::Queue, width: u32, height: u32, scale_factor: f64) {
        if width == 0 || height == 0 {
            return;
        }

        self.width = width;
        self.height = height;
        self.scale_factor = scale_factor;

        let scale = scale_factor.max(0.5) as f32;
        let logical_height = height as f32 / scale;
        let ui_scale = scale * (logical_height / 1080.0).clamp(0.78, 1.14);
        let margin = 24.0 * ui_scale;
        let panel_width = (width as f32 * 0.54)
            .clamp(360.0 * ui_scale, 820.0 * ui_scale)
            .min(width as f32 - margin * 2.0);
        let panel_height = 68.0 * ui_scale;

        self.panel_left = margin;
        self.panel_top = margin;
        self.panel_right = self.panel_left + panel_width;
        self.panel_bottom = self.panel_top + panel_height;
        self.text_left = self.panel_left + 20.0 * ui_scale;
        self.text_top = self.panel_top + 20.0 * ui_scale;

        self.message_buffer.set_metrics_and_size(
            Metrics::new(17.0 * ui_scale, 24.0 * ui_scale),
            Some((panel_width - 40.0 * ui_scale).max(1.0)),
            Some((panel_height - 20.0 * ui_scale).max(1.0)),
        );
        self.message_buffer.set_wrap(Wrap::None);
        self.message_buffer
            .shape_until_scroll(&mut self.font_system, false);

        let mut vertices = Vec::with_capacity(12);
        push_rect(
            &mut vertices,
            [
                self.panel_left,
                self.panel_top,
                self.panel_right,
                self.panel_bottom,
            ],
            width as f32,
            height as f32,
            [0.02, 0.72, 0.42, 0.72],
        );
        push_rect(
            &mut vertices,
            [
                self.panel_left + 2.0 * ui_scale,
                self.panel_top + 2.0 * ui_scale,
                self.panel_right - 2.0 * ui_scale,
                self.panel_bottom - 2.0 * ui_scale,
            ],
            width as f32,
            height as f32,
            [0.002, 0.018, 0.012, 0.93],
        );

        self.panel_vertex_count = vertices.len() as u32;
        queue.write_buffer(
            &self.panel_vertex_buffer,
            0,
            bytemuck::cast_slice(&vertices),
        );
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
    ) {
        if !self.enabled || self.remaining <= 0.0 || self.width == 0 || self.height == 0 {
            return;
        }

        self.viewport.update(
            queue,
            Resolution {
                width: self.width,
                height: self.height,
            },
        );

        let area = TextArea {
            buffer: &self.message_buffer,
            left: self.text_left,
            top: self.text_top,
            scale: 1.0,
            bounds: TextBounds {
                left: self.panel_left.round() as i32,
                top: self.panel_top.round() as i32,
                right: self.panel_right.round() as i32,
                bottom: self.panel_bottom.round() as i32,
            },
            default_color: Color::rgb(205, 255, 225),
            custom_glyphs: &[],
        };

        if let Err(error) = self.text_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            [area],
            &mut self.swash_cache,
        ) {
            eprintln!("Could not prepare status text: {error}");
            return;
        }

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Status overlay render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_pipeline(&self.panel_pipeline);
        render_pass.set_vertex_buffer(0, self.panel_vertex_buffer.slice(..));
        render_pass.draw(0..self.panel_vertex_count, 0..1);

        if let Err(error) = self
            .text_renderer
            .render(&self.atlas, &self.viewport, &mut render_pass)
        {
            eprintln!("Could not render status text: {error}");
        }

        drop(render_pass);
        self.atlas.trim();
    }

    fn configure_message(&mut self, message: &str) {
        self.message_buffer.set_text(
            message,
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
            None,
        );
        self.message_buffer
            .shape_until_scroll(&mut self.font_system, false);
    }
}

fn push_rect(
    vertices: &mut Vec<OverlayVertex>,
    rect: [f32; 4],
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let [left, top, right, bottom] = rect;
    let left = left / width * 2.0 - 1.0;
    let right = right / width * 2.0 - 1.0;
    let top = 1.0 - top / height * 2.0;
    let bottom = 1.0 - bottom / height * 2.0;

    vertices.extend_from_slice(&[
        OverlayVertex {
            position: [left, top],
            color,
        },
        OverlayVertex {
            position: [left, bottom],
            color,
        },
        OverlayVertex {
            position: [right, bottom],
            color,
        },
        OverlayVertex {
            position: [left, top],
            color,
        },
        OverlayVertex {
            position: [right, bottom],
            color,
        },
        OverlayVertex {
            position: [right, top],
            color,
        },
    ]);
}
