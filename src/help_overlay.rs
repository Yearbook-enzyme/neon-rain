use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Wrap,
};

const LEFT_HELP: &str = r#"CAMERA
W / S       Forward / backward
A / D       Strafe left / right
Q / E       Move down / up
Shift       Movement boost
Ctrl        Precision movement
PgUp/PgDn   Camera speed
Tab         Capture mouse-look
Mouse       Look while captured
Wheel       Zoom / field of view
C           Cycle auto-flight
H           Toggle reticle
R           Reset camera
G           Regenerate streams

SCENE
Space       Pause / resume
F11         Toggle fullscreen
F12         Cycle complete scene
Home        Reload config file
End         Save session now
Insert      Toggle status toasts
[ / ]       Previous / next theme
F3          Cycle color palette
0           Strong media defaults
1 - 9       Theme presets
- / =       Exposure down / up"#;

const RIGHT_HELP: &str = r#"MEDIA
M           Cycle media mode
, / .       Previous / next image
I           Reload media folder
O / P       Opacity down / up
K / L       Contrast down / up
Z / X       Scale down / up
J / N       Nearer / farther
Arrows      Move media plane
V           Cycle spatial mode
B           Camera / world lock
F           Face media plane
Y           Toggle spatial guide
T           Cycle preview mode
;           Cycle rain coupling
U           Reset media transform

MUSIC + AMBIENCE
F2          Signal inspector
F4          Music color mode
F5          Music reaction on / off
Shift+F5    Reaction intensity
F6          Cinematic director
F7          Auto-cycle media
F8 / F9     Cycle slower / faster
F10         Music source
\            Apparitions on / off
' / Slash   Apparition opacity + / -
` / Bksp    Apparition frequency + / -"#;

const TITLE: &str = "NEON RAIN  //  KEYBINDINGS";
const FOOTER: &str = "F1 or ? closes help    •    Esc closes help    •    Other controls are suspended while this panel is open";

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

#[derive(Clone, Copy, Default)]
struct HelpLayout {
    panel_left: f32,
    panel_top: f32,
    panel_right: f32,
    panel_bottom: f32,
    left_x: f32,
    right_x: f32,
    body_top: f32,
    body_bottom: f32,
    column_width: f32,
    title_x: f32,
    title_y: f32,
    footer_x: f32,
    footer_y: f32,
    ui_scale: f32,
}

pub struct HelpOverlay {
    visible: bool,
    panel_pipeline: wgpu::RenderPipeline,
    panel_vertex_buffer: wgpu::Buffer,
    panel_vertex_count: u32,

    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    title_buffer: Buffer,
    left_buffer: Buffer,
    right_buffer: Buffer,
    footer_buffer: Buffer,

    layout: HelpLayout,
    width: u32,
    height: u32,
}

impl HelpOverlay {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Help overlay shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("help_overlay.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Help overlay pipeline layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });

        let panel_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Help overlay panel pipeline"),
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
            label: Some("Help overlay panel vertices"),
            size: (36 * size_of::<OverlayVertex>()) as u64,
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

        let title_buffer = Buffer::new(&mut font_system, Metrics::new(26.0, 34.0));
        let left_buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 21.0));
        let right_buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 21.0));
        let footer_buffer = Buffer::new(&mut font_system, Metrics::new(13.0, 18.0));

        let mut overlay = Self {
            visible: false,
            panel_pipeline,
            panel_vertex_buffer,
            panel_vertex_count: 0,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            title_buffer,
            left_buffer,
            right_buffer,
            footer_buffer,
            layout: HelpLayout::default(),
            width,
            height,
        };

        overlay.resize(queue, width, height, scale_factor);
        overlay
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn resize(&mut self, queue: &wgpu::Queue, width: u32, height: u32, scale_factor: f64) {
        if width == 0 || height == 0 {
            return;
        }

        self.width = width;
        self.height = height;
        self.layout = calculate_layout(width, height, scale_factor);

        let body_height = (self.layout.body_bottom - self.layout.body_top).max(1.0);
        let logical_height = height as f32 / scale_factor.max(0.5) as f32;
        let compactness = (logical_height / 1080.0).clamp(0.72, 1.12);
        let font_scale = scale_factor as f32 * compactness;

        configure_buffer(
            &mut self.title_buffer,
            &mut self.font_system,
            TITLE,
            Metrics::new(26.0 * font_scale, 34.0 * font_scale),
            (self.layout.panel_right - self.layout.title_x - 24.0 * font_scale).max(1.0),
            44.0 * font_scale,
        );

        configure_buffer(
            &mut self.left_buffer,
            &mut self.font_system,
            LEFT_HELP,
            Metrics::new(15.5 * font_scale, 20.5 * font_scale),
            self.layout.column_width,
            body_height,
        );

        configure_buffer(
            &mut self.right_buffer,
            &mut self.font_system,
            RIGHT_HELP,
            Metrics::new(15.5 * font_scale, 20.5 * font_scale),
            self.layout.column_width,
            body_height,
        );

        configure_buffer(
            &mut self.footer_buffer,
            &mut self.font_system,
            FOOTER,
            Metrics::new(12.5 * font_scale, 17.0 * font_scale),
            (self.layout.panel_right - self.layout.footer_x - 24.0 * font_scale).max(1.0),
            26.0 * font_scale,
        );

        let vertices = build_panel_vertices(width, height, self.layout);
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
        if !self.visible || self.width == 0 || self.height == 0 {
            return;
        }

        self.viewport.update(
            queue,
            Resolution {
                width: self.width,
                height: self.height,
            },
        );

        let clip_bounds = TextBounds {
            left: self.layout.panel_left.round() as i32,
            top: self.layout.panel_top.round() as i32,
            right: self.layout.panel_right.round() as i32,
            bottom: self.layout.panel_bottom.round() as i32,
        };

        let areas = [
            TextArea {
                buffer: &self.title_buffer,
                left: self.layout.title_x,
                top: self.layout.title_y,
                scale: 1.0,
                bounds: clip_bounds,
                default_color: Color::rgb(220, 255, 235),
                custom_glyphs: &[],
            },
            TextArea {
                buffer: &self.left_buffer,
                left: self.layout.left_x,
                top: self.layout.body_top,
                scale: 1.0,
                bounds: clip_bounds,
                default_color: Color::rgb(185, 242, 207),
                custom_glyphs: &[],
            },
            TextArea {
                buffer: &self.right_buffer,
                left: self.layout.right_x,
                top: self.layout.body_top,
                scale: 1.0,
                bounds: clip_bounds,
                default_color: Color::rgb(185, 242, 207),
                custom_glyphs: &[],
            },
            TextArea {
                buffer: &self.footer_buffer,
                left: self.layout.footer_x,
                top: self.layout.footer_y,
                scale: 1.0,
                bounds: clip_bounds,
                default_color: Color::rgb(115, 202, 153),
                custom_glyphs: &[],
            },
        ];

        if let Err(error) = self.text_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            areas,
            &mut self.swash_cache,
        ) {
            eprintln!("Could not prepare help text: {error}");
            return;
        }

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Help overlay render pass"),
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

            if let Err(error) =
                self.text_renderer
                    .render(&self.atlas, &self.viewport, &mut render_pass)
            {
                eprintln!("Could not render help text: {error}");
            }
        }

        self.atlas.trim();
    }
}

fn configure_buffer(
    buffer: &mut Buffer,
    font_system: &mut FontSystem,
    text: &str,
    metrics: Metrics,
    width: f32,
    height: f32,
) {
    buffer.set_metrics_and_size(metrics, Some(width), Some(height));
    buffer.set_wrap(Wrap::None);
    buffer.set_text(
        text,
        &Attrs::new().family(Family::Monospace),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(font_system, false);
}

fn calculate_layout(width: u32, height: u32, scale_factor: f64) -> HelpLayout {
    let width = width as f32;
    let height = height as f32;
    let scale = scale_factor.max(0.5) as f32;
    let logical_height = height / scale;
    let ui_scale = scale * (logical_height / 1080.0).clamp(0.76, 1.16);

    let margin_x = (width * 0.045).max(28.0 * ui_scale);
    let margin_y = (height * 0.050).max(22.0 * ui_scale);
    let panel_left = margin_x;
    let panel_top = margin_y;
    let panel_right = width - margin_x;
    let panel_bottom = height - margin_y;

    let inner = 28.0 * ui_scale;
    let header_height = 66.0 * ui_scale;
    let footer_height = 42.0 * ui_scale;
    let column_gap = 46.0 * ui_scale;
    let available_width = panel_right - panel_left - inner * 2.0;
    let column_width = ((available_width - column_gap) * 0.5).max(1.0);

    HelpLayout {
        panel_left,
        panel_top,
        panel_right,
        panel_bottom,
        left_x: panel_left + inner,
        right_x: panel_left + inner + column_width + column_gap,
        body_top: panel_top + header_height,
        body_bottom: panel_bottom - footer_height,
        column_width,
        title_x: panel_left + inner,
        title_y: panel_top + 17.0 * ui_scale,
        footer_x: panel_left + inner,
        footer_y: panel_bottom - 30.0 * ui_scale,
        ui_scale,
    }
}

fn build_panel_vertices(width: u32, height: u32, layout: HelpLayout) -> Vec<OverlayVertex> {
    let mut vertices = Vec::with_capacity(36);
    let width = width as f32;
    let height = height as f32;
    let border = (2.0 * layout.ui_scale).max(2.0);
    let line = (1.0 * layout.ui_scale).max(1.0);

    push_rect(
        &mut vertices,
        [0.0, 0.0, width, height],
        width,
        height,
        [0.0, 0.0, 0.0, 0.58],
    );

    push_rect(
        &mut vertices,
        [
            layout.panel_left,
            layout.panel_top,
            layout.panel_right,
            layout.panel_bottom,
        ],
        width,
        height,
        [0.03, 0.78, 0.43, 0.72],
    );

    push_rect(
        &mut vertices,
        [
            layout.panel_left + border,
            layout.panel_top + border,
            layout.panel_right - border,
            layout.panel_bottom - border,
        ],
        width,
        height,
        [0.002, 0.020, 0.013, 0.965],
    );

    push_rect(
        &mut vertices,
        [
            layout.panel_left + border,
            layout.panel_top + border,
            layout.panel_right - border,
            layout.body_top - 10.0 * layout.ui_scale,
        ],
        width,
        height,
        [0.0, 0.090, 0.052, 0.82],
    );

    let center_x = (layout.left_x + layout.column_width + layout.right_x) * 0.5;
    push_rect(
        &mut vertices,
        [
            center_x - line,
            layout.body_top,
            center_x + line,
            layout.body_bottom,
        ],
        width,
        height,
        [0.06, 0.50, 0.30, 0.42],
    );

    push_rect(
        &mut vertices,
        [
            layout.panel_left + 18.0 * layout.ui_scale,
            layout.body_bottom + 5.0 * layout.ui_scale,
            layout.panel_right - 18.0 * layout.ui_scale,
            layout.body_bottom + 5.0 * layout.ui_scale + line,
        ],
        width,
        height,
        [0.06, 0.50, 0.30, 0.42],
    );

    vertices
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
            position: [right, top],
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
            position: [left, bottom],
            color,
        },
    ]);
}
