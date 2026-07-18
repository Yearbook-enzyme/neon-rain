mod atlas;
mod bloom;
mod simulation;

use std::{mem::size_of, sync::Arc, time::Instant};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    keyboard::{KeyCode, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

use atlas::{ATLAS_HEIGHT, ATLAS_WIDTH, create_glyph_atlas};
use bloom::{Bloom, HDR_FORMAT};
use simulation::{GLYPHS_PER_STREAM, Simulation};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    time: f32,
    aspect: f32,
    resolution: [f32; 2],
    controls: [f32; 4],
    stream_count: u32,
    padding: [u32; 3],
}

const MAX_GLYPH_INSTANCES: usize = simulation::MAX_STREAMS * GLYPHS_PER_STREAM;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlyphInstance {
    // center x, center y, glyph width, glyph height
    position_size: [f32; 4],

    // core red, core green, core blue, local glow strength
    color_glow: [f32; 4],

    // glyph atlas index, reserved, reserved, reserved
    glyph_data: [u32; 4],
}

impl GlyphInstance {
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x4,
            1 => Float32x4,
            2 => Uint32x4,
        ],
    };
}

fn stable_unit(mut value: u32) -> f32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;

    value as f32 / u32::MAX as f32
}

fn mix(left: f32, right: f32, amount: f32) -> f32 {
    left + (right - left) * amount
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let amount = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);

    amount * amount * (3.0 - 2.0 * amount)
}

fn visual_scale(size: winit::dpi::PhysicalSize<u32>) -> f32 {
    let pixel_area = size.width.max(1) as f32 * size.height.max(1) as f32;

    (pixel_area / (1600.0 * 900.0)).sqrt().clamp(0.72, 2.40)
}

struct State {
    instance: wgpu::Instance,
    window: Arc<Window>,

    device: wgpu::Device,
    queue: wgpu::Queue,

    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    size: winit::dpi::PhysicalSize<u32>,

    render_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    glyph_instance_buffer: wgpu::Buffer,
    render_bind_group: wgpu::BindGroup,
    bloom: Bloom,

    simulation: Simulation,
    last_frame: Instant,

    paused: bool,
    speed_scale: f32,
    glow_strength: f32,
    exposure: f32,
    target_exposure: f32,

    glyph_instances: Vec<GlyphInstance>,
    glyph_instance_count: u32,

    stats_elapsed: f32,
    stats_frames: u32,
    stats_worst_ms: f32,

    // These fields keep the GPU resources alive.
    _glyph_texture: wgpu::Texture,
    _glyph_texture_view: wgpu::TextureView,
    _glyph_sampler: wgpu::Sampler,

    start_time: Instant,
}

impl State {
    async fn new(display: OwnedDisplayHandle, window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(display),
        ));

        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create GPU surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .expect("No compatible GPU adapter found");

        let adapter_info = adapter.get_info();

        println!(
            "Using GPU: {} ({:?}, {:?})",
            adapter_info.name, adapter_info.backend, adapter_info.device_type,
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("Failed to create GPU device");

        let size = window.inner_size();
        let capabilities = surface.get_capabilities(&adapter);

        let surface_format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(capabilities.formats[0]);

        let initial_uniforms = Uniforms {
            time: 0.0,
            aspect: calculate_aspect(size),
            resolution: [size.width as f32, size.height as f32],
            controls: [1.0, 1.0, 1.0, 0.0],
            stream_count: 0,
            padding: [0; 3],
        };

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Animation uniforms"),
            contents: bytemuck::bytes_of(&initial_uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let empty_instances = vec![GlyphInstance::zeroed(); MAX_GLYPH_INSTANCES];

        let glyph_instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Glyph instance buffer"),
            contents: bytemuck::cast_slice(&empty_instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        let atlas_data = create_glyph_atlas();

        let glyph_texture = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("Glyph atlas texture"),
                size: wgpu::Extent3d {
                    width: ATLAS_WIDTH,
                    height: ATLAS_HEIGHT,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &atlas_data,
        );

        let glyph_texture_view = glyph_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let glyph_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Glyph atlas sampler"),

            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,

            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,

            ..Default::default()
        });

        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Render bind group layout"),

                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,

                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },

                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,

                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },

                            view_dimension: wgpu::TextureViewDimension::D2,

                            multisampled: false,
                        },

                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,

                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),

                        count: None,
                    },
                ],
            });

        let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Render bind group"),
            layout: &render_bind_group_layout,

            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&glyph_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&glyph_sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Neon rain shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render pipeline layout"),

            bind_group_layouts: &[Some(&render_bind_group_layout)],

            immediate_size: 0,
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Neon rain pipeline"),
            layout: Some(&pipeline_layout),

            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),

                compilation_options: wgpu::PipelineCompilationOptions::default(),

                buffers: &[Some(GlyphInstance::LAYOUT)],
            },

            primitive: wgpu::PrimitiveState::default(),

            depth_stencil: None,

            multisample: wgpu::MultisampleState::default(),

            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),

                compilation_options: wgpu::PipelineCompilationOptions::default(),

                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,

                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),

                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),

            multiview_mask: None,
            cache: None,
        });

        let bloom = Bloom::new(&device, size.width, size.height, surface_format);

        let simulation = Simulation::new(size.width, size.height);
        let now = Instant::now();

        let state = Self {
            instance,
            window,

            device,
            queue,

            surface,
            surface_format,
            size,

            render_pipeline,
            uniform_buffer,
            glyph_instance_buffer,
            render_bind_group,
            bloom,

            simulation,
            last_frame: now,

            paused: false,
            speed_scale: 1.0,
            glow_strength: 1.0,
            exposure: 1.0,
            target_exposure: 1.0,

            glyph_instances: Vec::with_capacity(MAX_GLYPH_INSTANCES),
            glyph_instance_count: 0,

            stats_elapsed: 0.0,
            stats_frames: 0,
            stats_worst_ms: 0.0,

            _glyph_texture: glyph_texture,
            _glyph_texture_view: glyph_texture_view,
            _glyph_sampler: glyph_sampler,

            start_time: Instant::now(),
        };

        state.configure_surface();
        state
    }

    fn configure_surface(&self) {
        if self.size.width == 0 || self.size.height == 0 {
            return;
        }

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,

            format: self.surface_format,
            color_space: wgpu::SurfaceColorSpace::Auto,

            width: self.size.width,
            height: self.size.height,

            present_mode: wgpu::PresentMode::AutoVsync,

            desired_maximum_frame_latency: 2,

            alpha_mode: wgpu::CompositeAlphaMode::Auto,

            view_formats: vec![self.surface_format.add_srgb_suffix()],
        };

        self.surface.configure(&self.device, &config);
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }

        self.size = new_size;
        self.simulation.resize(new_size.width, new_size.height);
        self.bloom
            .resize(&self.device, new_size.width, new_size.height);
        self.configure_surface();
    }

    fn print_controls(&self) {
        println!(
            "paused={}  speed={:.2}  glow={:.2}  exposure={:.2}",
            self.paused, self.speed_scale, self.glow_strength, self.exposure,
        );
    }

    fn apply_preset(&mut self, preset: u8) {
        match preset {
            1 => {
                self.speed_scale = 0.20;
                self.glow_strength = 0.05;
                self.exposure = 0.35;
            }

            2 => {
                self.speed_scale = 1.0;
                self.glow_strength = 1.0;
                self.exposure = 1.0;
            }

            3 => {
                self.speed_scale = 2.75;
                self.glow_strength = 5.0;
                self.exposure = 2.25;
            }

            4 => {
                self.speed_scale = 0.03;
                self.glow_strength = 4.0;
                self.exposure = 1.35;
            }

            _ => return,
        }

        println!("Applied preset {preset}");
        self.print_controls();
    }

    fn toggle_fullscreen(&self) {
        let fullscreen = if self.window.fullscreen().is_some() {
            None
        } else {
            Some(Fullscreen::Borderless(None))
        };

        self.window.set_fullscreen(fullscreen);
    }

    fn rebuild_glyph_instances(&mut self) {
        self.glyph_instances.clear();

        let scale = visual_scale(self.size);
        let width = self.size.width.max(1) as f32;
        let height = self.size.height.max(1) as f32;
        let exposure = self.exposure.max(0.01);
        let glow_control = self.glow_strength.max(0.0);

        for (stream_index, stream) in self.simulation.streams.iter().enumerate() {
            let depth = stream.depth.clamp(0.0, 1.0);
            let depth_shape = depth.powf(1.45);

            // These decisions are deliberately seeded only from the
            // persistent stream slot. The old shader included phase in
            // the random seed, which made streams and white heads toggle
            // unpredictably every frame.
            let density_sample =
                stable_unit((stream_index as u32).wrapping_mul(0x9e37_79b9) ^ 0x4d41_5452);

            let density_keep = mix(0.96, 0.28, depth_shape);

            if density_sample > density_keep {
                continue;
            }

            let glyph_width = mix(9.5, 20.5, depth_shape) * scale;
            let glyph_height = mix(15.0, 30.0, depth_shape) * scale;

            let atmosphere = mix(0.42, 1.08, depth.powf(0.78));
            let glow_atmosphere = mix(0.06, 1.18, depth.powf(1.55));
            let head_probability = mix(0.015, 0.42, depth.powf(1.40));
            let head_depth = mix(0.06, 0.78, depth.powf(1.60));
            let cascade_depth = mix(0.18, 1.08, depth.powf(1.15));

            let head_sample =
                stable_unit((stream_index as u32).wrapping_mul(0x85eb_ca6b) ^ 0x4845_4144);
            let white_head_present = head_sample < head_probability;

            // Stable anatomy for the lifetime of this stream.
            // Protected leading glyphs participate in the seed so a
            // respawn can produce a different body without flickering.
            let anatomy_seed = (stream_index as u32).wrapping_mul(0x9e37_79b9)
                ^ stream.glyphs[0].wrapping_mul(0x85eb_ca6b)
                ^ stream.glyphs[1].wrapping_mul(0xc2b2_ae35)
                ^ stream.glyphs[2].wrapping_mul(0x27d4_eb2f);

            let primary_gap_center = mix(0.30, 0.68, stable_unit(anatomy_seed ^ 0x4741_5031));

            let primary_gap_half_width = mix(0.035, 0.075, stable_unit(anatomy_seed ^ 0x5749_4431));

            let secondary_gap_center = mix(0.62, 0.88, stable_unit(anatomy_seed ^ 0x4741_5032));

            let secondary_gap_half_width =
                mix(0.025, 0.055, stable_unit(anatomy_seed ^ 0x5749_4432));

            let stream_length = (stream.length as usize).min(GLYPHS_PER_STREAM);

            let has_primary_gap =
                stream_length >= 10 && stable_unit(anatomy_seed ^ 0x5052_494d) > 0.18;

            let gaps_are_separated = (secondary_gap_center - primary_gap_center).abs()
                > primary_gap_half_width + secondary_gap_half_width + 0.08;

            let has_secondary_gap = stream_length >= 24
                && gaps_are_separated
                && stable_unit(anatomy_seed ^ 0x5345_434f) > 0.55;

            for segment in 0..stream_length {
                if self.glyph_instances.len() >= MAX_GLYPH_INSTANCES {
                    break;
                }

                let center_x = stream.x;
                let center_y = stream.head - segment as f32 * glyph_height;

                let margin_x = glyph_width * 0.70;
                let margin_y = glyph_height * 0.70;

                if center_x + margin_x < 0.0
                    || center_x - margin_x > width
                    || center_y + margin_y < 0.0
                    || center_y - margin_y > height
                {
                    continue;
                }

                let trail_position =
                    segment as f32 / (stream_length.saturating_sub(1).max(1) as f32);

                // Preserve a solid leading cluster. Gaps appear only
                // in the body and remain attached to the stream.
                let protected_head = segment < 4;

                let in_primary_gap = has_primary_gap
                    && (trail_position - primary_gap_center).abs() < primary_gap_half_width;

                let in_secondary_gap = has_secondary_gap
                    && (trail_position - secondary_gap_center).abs() < secondary_gap_half_width;

                if !protected_head && (in_primary_gap || in_secondary_gap) {
                    continue;
                }

                // Four-glyph groups share a stable energy character,
                // making the body read as clusters rather than noise.
                let cluster_index = segment / 4;

                let cluster_energy = mix(
                    0.82,
                    1.10,
                    stable_unit(anatomy_seed ^ (cluster_index as u32).wrapping_mul(0x1656_67b1)),
                );

                let glyph_energy = mix(
                    0.94,
                    1.06,
                    stable_unit(anatomy_seed ^ (segment as u32).wrapping_mul(0xd3a2_646c)),
                );

                let head_profile = (-(segment as f32) * 0.72).exp();

                let tail_narrowing = smoothstep(0.58, 1.0, trail_position);

                // Maintain a readable middle body before the final
                // section becomes narrower and dimmer.
                let trail_fade = (1.0 - trail_position).powf(1.38);

                let anatomy_energy = cluster_energy * glyph_energy * mix(1.0, 0.72, tail_narrowing);

                let instance_glyph_width =
                    glyph_width * (1.0 + head_profile * 0.08) * mix(1.0, 0.82, tail_narrowing);

                let instance_glyph_height = glyph_height * (1.0 + head_profile * 0.035);

                let cascade_delta = trail_position - stream.cascade_position;
                let cascade_core = (-cascade_delta * cascade_delta * 360.0).exp();
                let cascade_wake = if cascade_delta < 0.0 {
                    (cascade_delta * 14.0).exp()
                } else {
                    0.0
                };
                let cascade_packet =
                    stream.cascade_intensity * (cascade_core * 1.20 + cascade_wake * 0.16);

                let head_injection = (-(segment as f32) * 0.38).exp();
                let propagation_profile = 0.80 + head_injection * 0.42;
                let base_energy = stream.brightness * trail_fade * propagation_profile;

                // Every stream receives a recognizable green
                // leading cluster. The rarer white head is added below.
                let head_lift = match segment {
                    0 => 0.52,
                    1 => 0.24,
                    2 => 0.10,
                    _ => 0.0,
                };

                let core_energy =
                    base_energy * atmosphere * anatomy_energy * (1.0 + cascade_packet * 0.35)
                        + stream.brightness * atmosphere * head_lift;

                let cascade_energy =
                    stream.brightness * cascade_depth * cascade_packet * (0.45 + trail_fade * 0.55);

                let head_energy = if segment == 0 && white_head_present {
                    stream.brightness * atmosphere * 1.30 * head_depth
                } else {
                    0.0
                };

                let white_energy = head_energy + cascade_energy * 0.82;

                let core_color = [
                    (0.03 * core_energy + 0.78 * white_energy) * exposure,
                    (1.00 * core_energy + 1.00 * white_energy) * exposure,
                    (0.27 * core_energy + 0.84 * white_energy) * exposure,
                ];

                let glow_energy = (base_energy * anatomy_energy * glow_atmosphere * 0.34
                    + stream.brightness * glow_atmosphere * cascade_packet * 0.42
                    + stream.brightness * glow_atmosphere * head_profile * 0.08)
                    * glow_control
                    * exposure;

                self.glyph_instances.push(GlyphInstance {
                    position_size: [
                        center_x,
                        center_y,
                        instance_glyph_width,
                        instance_glyph_height,
                    ],
                    color_glow: [core_color[0], core_color[1], core_color[2], glow_energy],
                    glyph_data: [stream.glyphs[segment], 0, 0, 0],
                });
            }
        }

        self.glyph_instance_count = self.glyph_instances.len() as u32;

        if !self.glyph_instances.is_empty() {
            self.queue.write_buffer(
                &self.glyph_instance_buffer,
                0,
                bytemuck::cast_slice(&self.glyph_instances),
            );
        }
    }

    fn update_frame_stats(&mut self, dt: f32) {
        self.stats_elapsed += dt;
        self.stats_frames += 1;
        self.stats_worst_ms = self.stats_worst_ms.max(dt * 1000.0);

        if self.stats_elapsed < 1.0 {
            return;
        }

        let fps = self.stats_frames as f32 / self.stats_elapsed;
        let average_ms = self.stats_elapsed * 1000.0 / self.stats_frames.max(1) as f32;

        self.window.set_title(&format!(
            "Neon Rain — {:.0} FPS — {:.1} ms — {} glyphs",
            fps, average_ms, self.glyph_instance_count,
        ));

        println!(
            "fps={fps:.1}  frame={average_ms:.2}ms  worst={:.2}ms  glyphs={}",
            self.stats_worst_ms, self.glyph_instance_count,
        );

        self.stats_elapsed = 0.0;
        self.stats_frames = 0;
        self.stats_worst_ms = 0.0;
    }

    fn update(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();

        self.last_frame = now;

        let simulation_dt = if self.paused {
            0.0
        } else {
            dt * self.speed_scale
        };

        self.simulation.update(simulation_dt);

        let stream_fraction = self.simulation.streams.len() as f32 / simulation::MAX_STREAMS as f32;
        self.target_exposure = 1.35 - stream_fraction * 0.45;

        let adapt_speed = 2.0;
        self.exposure += (self.target_exposure - self.exposure) * (1.0 - (-adapt_speed * dt).exp());

        self.rebuild_glyph_instances();

        let uniforms = Uniforms {
            time: self.start_time.elapsed().as_secs_f32(),
            aspect: calculate_aspect(self.size),
            resolution: [self.size.width as f32, self.size.height as f32],
            controls: [self.speed_scale, self.glow_strength, self.exposure, 0.0],
            stream_count: self.glyph_instance_count,
            padding: [0; 3],
        };

        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        self.update_frame_stats(dt);
    }

    fn render(&mut self) {
        if self.size.width == 0 || self.size.height == 0 {
            return;
        }

        self.update();

        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,

            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return;
            }

            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                drop(texture);
                self.configure_surface();
                return;
            }

            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure_surface();
                return;
            }

            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .instance
                    .create_surface(self.window.clone())
                    .expect("Failed to recreate surface");

                self.configure_surface();
                return;
            }

            wgpu::CurrentSurfaceTexture::Validation => {
                panic!("Surface validation error",);
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.surface_format.add_srgb_suffix()),

                ..Default::default()
            });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Neon Rain command encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Neon Rain render pass"),

                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.bloom.view,
                    depth_slice: None,
                    resolve_target: None,

                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.00025,
                            g: 0.0022,
                            b: 0.0007,
                            a: 1.0,
                        }),

                        store: wgpu::StoreOp::Store,
                    },
                })],

                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);

            render_pass.set_bind_group(0, &self.render_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.glyph_instance_buffer.slice(..));

            render_pass.draw(0..6, 0..self.glyph_instance_count);
        }

        self.bloom.composite(&mut encoder, &view);

        self.queue.submit([encoder.finish()]);

        self.window.pre_present_notify();
        self.queue.present(surface_texture);
    }
}

fn calculate_aspect(size: winit::dpi::PhysicalSize<u32>) -> f32 {
    if size.height == 0 {
        return 1.0;
    }

    size.width as f32 / size.height as f32
}

#[derive(Default)]
struct App {
    state: Option<State>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Neon Rain")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0)),
                )
                .expect("Failed to create window"),
        );

        let state = pollster::block_on(State::new(
            event_loop.owned_display_handle(),
            window.clone(),
        ));

        self.state = Some(state);
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        if window_id != state.window.id() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key),

                        state: ElementState::Pressed,

                        repeat: false,
                        ..
                    },
                ..
            } => match key {
                KeyCode::Escape => {
                    event_loop.exit();
                }

                KeyCode::F11 => {
                    state.toggle_fullscreen();
                }

                KeyCode::Space => {
                    state.paused = !state.paused;
                    state.print_controls();
                }

                KeyCode::KeyR => {
                    state.simulation = Simulation::new(state.size.width, state.size.height);

                    println!("Regenerated all persistent streams");
                }

                KeyCode::ArrowUp => {
                    state.speed_scale = (state.speed_scale + 0.25).min(5.0);

                    state.print_controls();
                }

                KeyCode::ArrowDown => {
                    state.speed_scale = (state.speed_scale - 0.25).max(0.0);

                    state.print_controls();
                }

                KeyCode::ArrowRight => {
                    state.glow_strength = (state.glow_strength + 0.50).min(8.0);

                    state.print_controls();
                }

                KeyCode::ArrowLeft => {
                    state.glow_strength = (state.glow_strength - 0.50).max(0.0);

                    state.print_controls();
                }

                KeyCode::KeyE => {
                    state.exposure = (state.exposure + 0.20).min(4.0);

                    state.print_controls();
                }

                KeyCode::KeyQ => {
                    state.exposure = (state.exposure - 0.20).max(0.10);

                    state.print_controls();
                }

                KeyCode::Digit1 => {
                    state.apply_preset(1);
                }

                KeyCode::Digit2 => {
                    state.apply_preset(2);
                }

                KeyCode::Digit3 => {
                    state.apply_preset(3);
                }

                KeyCode::Digit4 => {
                    state.apply_preset(4);
                }

                _ => {}
            },

            WindowEvent::Resized(size) => {
                state.resize(size);
            }

            WindowEvent::RedrawRequested => {
                state.render();

                state.window.request_redraw();
            }

            _ => {}
        }
    }
}

fn main() {
    env_logger::init();

    let event_loop = EventLoop::new().expect("Failed to create event loop");

    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();

    event_loop.run_app(&mut app).expect("Application error");
}
