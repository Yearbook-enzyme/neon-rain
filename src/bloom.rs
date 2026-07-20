use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct BloomSettings {
    pub near_strength: f32,
    pub wide_strength: f32,
    pub history_retention: f32,
    pub history_deposit: f32,
    // red, green, blue, vignette strength
    pub background_color: [f32; 4],
}

pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

struct RenderTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderTarget {
    fn new(device: &wgpu::Device, label: &str, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            _texture: texture,
            view,
        }
    }
}

pub struct Bloom {
    // Full-resolution HDR target used by the Matrix renderer.
    hdr_target: RenderTarget,
    pub view: wgpu::TextureView,

    // Half-resolution near bloom.
    bright_target: RenderTarget,
    near_horizontal_target: RenderTarget,
    near_vertical_target: RenderTarget,

    // Quarter-resolution wide atmospheric bloom.
    wide_source_target: RenderTarget,
    wide_horizontal_target: RenderTarget,
    wide_vertical_target: RenderTarget,

    // Quarter-resolution temporal light history ping-pong targets.
    history_a_target: RenderTarget,
    history_b_target: RenderTarget,
    history_a_is_previous: bool,
    history_valid: bool,

    sampler: wgpu::Sampler,
    settings_buffer: wgpu::Buffer,

    post_bind_group_layout: wgpu::BindGroupLayout,
    composite_bind_group_layout: wgpu::BindGroupLayout,
    history_bind_group_layout: wgpu::BindGroupLayout,

    bright_bind_group: wgpu::BindGroup,
    near_horizontal_bind_group: wgpu::BindGroup,
    near_vertical_bind_group: wgpu::BindGroup,
    wide_downsample_bind_group: wgpu::BindGroup,
    wide_horizontal_bind_group: wgpu::BindGroup,
    wide_vertical_bind_group: wgpu::BindGroup,
    history_a_to_b_bind_group: wgpu::BindGroup,
    history_b_to_a_bind_group: wgpu::BindGroup,
    composite_history_a_bind_group: wgpu::BindGroup,
    composite_history_b_bind_group: wgpu::BindGroup,

    bright_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,
    horizontal_pipeline: wgpu::RenderPipeline,
    vertical_pipeline: wgpu::RenderPipeline,
    wide_vertical_pipeline: wgpu::RenderPipeline,
    history_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
}

impl Bloom {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let hdr_target = RenderTarget::new(device, "Matrix HDR render target", width, height);
        let view = hdr_target.view.clone();

        let (half_width, half_height, quarter_width, quarter_height) =
            Self::scaled_sizes(width, height);

        let bright_target =
            RenderTarget::new(device, "Bloom bright-pass target", half_width, half_height);
        let near_horizontal_target = RenderTarget::new(
            device,
            "Bloom near horizontal target",
            half_width,
            half_height,
        );
        let near_vertical_target = RenderTarget::new(
            device,
            "Bloom near vertical target",
            half_width,
            half_height,
        );

        let wide_source_target = RenderTarget::new(
            device,
            "Bloom wide downsample target",
            quarter_width,
            quarter_height,
        );
        let wide_horizontal_target = RenderTarget::new(
            device,
            "Bloom wide horizontal target",
            quarter_width,
            quarter_height,
        );
        let wide_vertical_target = RenderTarget::new(
            device,
            "Bloom wide vertical target",
            quarter_width,
            quarter_height,
        );
        let history_a_target = RenderTarget::new(
            device,
            "Bloom history A target",
            quarter_width,
            quarter_height,
        );
        let history_b_target = RenderTarget::new(
            device,
            "Bloom history B target",
            quarter_width,
            quarter_height,
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Bloom linear sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let initial_settings = BloomSettings {
            near_strength: 0.90,
            wide_strength: 0.30,
            history_retention: 0.86,
            history_deposit: 0.15,
            background_color: [0.00018, 0.00145, 0.00048, 0.16],
        };

        let settings_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bloom settings buffer"),
            contents: bytemuck::bytes_of(&initial_settings),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let post_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Bloom post-process bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let history_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Bloom temporal history bind group layout"),
                entries: &[
                    Self::texture_layout_entry(0),
                    Self::texture_layout_entry(1),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Bloom composite bind group layout"),
                entries: &[
                    Self::texture_layout_entry(0),
                    Self::texture_layout_entry(1),
                    Self::texture_layout_entry(2),
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let bright_bind_group = Self::create_post_bind_group(
            device,
            &post_bind_group_layout,
            &hdr_target.view,
            &sampler,
            "Bloom bright-pass bind group",
        );
        let near_horizontal_bind_group = Self::create_post_bind_group(
            device,
            &post_bind_group_layout,
            &bright_target.view,
            &sampler,
            "Bloom near horizontal bind group",
        );
        let near_vertical_bind_group = Self::create_post_bind_group(
            device,
            &post_bind_group_layout,
            &near_horizontal_target.view,
            &sampler,
            "Bloom near vertical bind group",
        );
        let wide_downsample_bind_group = Self::create_post_bind_group(
            device,
            &post_bind_group_layout,
            &near_vertical_target.view,
            &sampler,
            "Bloom wide downsample bind group",
        );
        let wide_horizontal_bind_group = Self::create_post_bind_group(
            device,
            &post_bind_group_layout,
            &wide_source_target.view,
            &sampler,
            "Bloom wide horizontal bind group",
        );
        let wide_vertical_bind_group = Self::create_post_bind_group(
            device,
            &post_bind_group_layout,
            &wide_horizontal_target.view,
            &sampler,
            "Bloom wide vertical bind group",
        );
        let history_a_to_b_bind_group = Self::create_history_bind_group(
            device,
            &history_bind_group_layout,
            &wide_vertical_target.view,
            &history_a_target.view,
            &sampler,
            &settings_buffer,
            "Bloom history A to B bind group",
        );
        let history_b_to_a_bind_group = Self::create_history_bind_group(
            device,
            &history_bind_group_layout,
            &wide_vertical_target.view,
            &history_b_target.view,
            &sampler,
            &settings_buffer,
            "Bloom history B to A bind group",
        );
        let composite_history_a_bind_group = Self::create_composite_bind_group(
            device,
            &composite_bind_group_layout,
            &hdr_target.view,
            &near_vertical_target.view,
            &history_a_target.view,
            &sampler,
            &settings_buffer,
        );
        let composite_history_b_bind_group = Self::create_composite_bind_group(
            device,
            &composite_bind_group_layout,
            &hdr_target.view,
            &near_vertical_target.view,
            &history_b_target.view,
            &sampler,
            &settings_buffer,
        );

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Bloom shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()),
        });

        let post_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Bloom post-process pipeline layout"),
            bind_group_layouts: &[Some(&post_bind_group_layout)],
            immediate_size: 0,
        });
        let history_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Bloom temporal history pipeline layout"),
                bind_group_layouts: &[Some(&history_bind_group_layout)],
                immediate_size: 0,
            });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Bloom composite pipeline layout"),
                bind_group_layouts: &[Some(&composite_bind_group_layout)],
                immediate_size: 0,
            });

        let bright_pipeline = Self::create_pipeline(
            device,
            &shader,
            &post_pipeline_layout,
            "Bloom bright-pass pipeline",
            "fs_bright",
            HDR_FORMAT,
        );
        let downsample_pipeline = Self::create_pipeline(
            device,
            &shader,
            &post_pipeline_layout,
            "Bloom downsample pipeline",
            "fs_downsample",
            HDR_FORMAT,
        );
        let horizontal_pipeline = Self::create_pipeline(
            device,
            &shader,
            &post_pipeline_layout,
            "Bloom horizontal blur pipeline",
            "fs_blur_horizontal",
            HDR_FORMAT,
        );
        let vertical_pipeline = Self::create_pipeline(
            device,
            &shader,
            &post_pipeline_layout,
            "Bloom vertical blur pipeline",
            "fs_blur_vertical",
            HDR_FORMAT,
        );
        let wide_vertical_pipeline = Self::create_pipeline(
            device,
            &shader,
            &post_pipeline_layout,
            "Bloom directional wide vertical pipeline",
            "fs_blur_vertical_wide",
            HDR_FORMAT,
        );
        let history_pipeline = Self::create_pipeline(
            device,
            &shader,
            &history_pipeline_layout,
            "Bloom temporal history pipeline",
            "fs_history",
            HDR_FORMAT,
        );
        let composite_pipeline = Self::create_pipeline(
            device,
            &shader,
            &composite_pipeline_layout,
            "Bloom composite pipeline",
            "fs_composite",
            surface_format,
        );

        Self {
            hdr_target,
            view,
            bright_target,
            near_horizontal_target,
            near_vertical_target,
            wide_source_target,
            wide_horizontal_target,
            wide_vertical_target,
            history_a_target,
            history_b_target,
            history_a_is_previous: true,
            history_valid: false,
            sampler,
            settings_buffer,
            post_bind_group_layout,
            composite_bind_group_layout,
            history_bind_group_layout,
            bright_bind_group,
            near_horizontal_bind_group,
            near_vertical_bind_group,
            wide_downsample_bind_group,
            wide_horizontal_bind_group,
            wide_vertical_bind_group,
            history_a_to_b_bind_group,
            history_b_to_a_bind_group,
            composite_history_a_bind_group,
            composite_history_b_bind_group,
            bright_pipeline,
            downsample_pipeline,
            horizontal_pipeline,
            vertical_pipeline,
            wide_vertical_pipeline,
            history_pipeline,
            composite_pipeline,
        }
    }

    fn scaled_sizes(width: u32, height: u32) -> (u32, u32, u32, u32) {
        (
            (width / 2).max(1),
            (height / 2).max(1),
            (width / 4).max(1),
            (height / 4).max(1),
        )
    }

    fn texture_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }
    }

    fn create_pipeline(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        label: &str,
        fragment_entry: &str,
        target_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(fragment_entry),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    fn create_post_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        input_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        label: &str,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    fn create_history_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        current_view: &wgpu::TextureView,
        previous_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        settings_buffer: &wgpu::Buffer,
        label: &str,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(current_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(previous_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: settings_buffer.as_entire_binding(),
                },
            ],
        })
    }

    fn create_composite_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        original_view: &wgpu::TextureView,
        near_view: &wgpu::TextureView,
        wide_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        settings_buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bloom composite bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(original_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(near_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(wide_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: settings_buffer.as_entire_binding(),
                },
            ],
        })
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.hdr_target = RenderTarget::new(device, "Matrix HDR render target", width, height);
        self.view = self.hdr_target.view.clone();

        let (half_width, half_height, quarter_width, quarter_height) =
            Self::scaled_sizes(width, height);

        self.bright_target =
            RenderTarget::new(device, "Bloom bright-pass target", half_width, half_height);
        self.near_horizontal_target = RenderTarget::new(
            device,
            "Bloom near horizontal target",
            half_width,
            half_height,
        );
        self.near_vertical_target = RenderTarget::new(
            device,
            "Bloom near vertical target",
            half_width,
            half_height,
        );
        self.wide_source_target = RenderTarget::new(
            device,
            "Bloom wide downsample target",
            quarter_width,
            quarter_height,
        );
        self.wide_horizontal_target = RenderTarget::new(
            device,
            "Bloom wide horizontal target",
            quarter_width,
            quarter_height,
        );
        self.wide_vertical_target = RenderTarget::new(
            device,
            "Bloom wide vertical target",
            quarter_width,
            quarter_height,
        );
        self.history_a_target = RenderTarget::new(
            device,
            "Bloom history A target",
            quarter_width,
            quarter_height,
        );
        self.history_b_target = RenderTarget::new(
            device,
            "Bloom history B target",
            quarter_width,
            quarter_height,
        );
        self.history_a_is_previous = true;
        self.history_valid = false;

        self.rebuild_bind_groups(device);
    }

    fn rebuild_bind_groups(&mut self, device: &wgpu::Device) {
        self.bright_bind_group = Self::create_post_bind_group(
            device,
            &self.post_bind_group_layout,
            &self.hdr_target.view,
            &self.sampler,
            "Bloom bright-pass bind group",
        );
        self.near_horizontal_bind_group = Self::create_post_bind_group(
            device,
            &self.post_bind_group_layout,
            &self.bright_target.view,
            &self.sampler,
            "Bloom near horizontal bind group",
        );
        self.near_vertical_bind_group = Self::create_post_bind_group(
            device,
            &self.post_bind_group_layout,
            &self.near_horizontal_target.view,
            &self.sampler,
            "Bloom near vertical bind group",
        );
        self.wide_downsample_bind_group = Self::create_post_bind_group(
            device,
            &self.post_bind_group_layout,
            &self.near_vertical_target.view,
            &self.sampler,
            "Bloom wide downsample bind group",
        );
        self.wide_horizontal_bind_group = Self::create_post_bind_group(
            device,
            &self.post_bind_group_layout,
            &self.wide_source_target.view,
            &self.sampler,
            "Bloom wide horizontal bind group",
        );
        self.wide_vertical_bind_group = Self::create_post_bind_group(
            device,
            &self.post_bind_group_layout,
            &self.wide_horizontal_target.view,
            &self.sampler,
            "Bloom wide vertical bind group",
        );
        self.history_a_to_b_bind_group = Self::create_history_bind_group(
            device,
            &self.history_bind_group_layout,
            &self.wide_vertical_target.view,
            &self.history_a_target.view,
            &self.sampler,
            &self.settings_buffer,
            "Bloom history A to B bind group",
        );
        self.history_b_to_a_bind_group = Self::create_history_bind_group(
            device,
            &self.history_bind_group_layout,
            &self.wide_vertical_target.view,
            &self.history_b_target.view,
            &self.sampler,
            &self.settings_buffer,
            "Bloom history B to A bind group",
        );
        self.composite_history_a_bind_group = Self::create_composite_bind_group(
            device,
            &self.composite_bind_group_layout,
            &self.hdr_target.view,
            &self.near_vertical_target.view,
            &self.history_a_target.view,
            &self.sampler,
            &self.settings_buffer,
        );
        self.composite_history_b_bind_group = Self::create_composite_bind_group(
            device,
            &self.composite_bind_group_layout,
            &self.hdr_target.view,
            &self.near_vertical_target.view,
            &self.history_b_target.view,
            &self.sampler,
            &self.settings_buffer,
        );
    }

    fn run_pass(
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
    ) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }

    fn clear_target(encoder: &mut wgpu::CommandEncoder, label: &str, target: &wgpu::TextureView) {
        let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    pub fn invalidate_history(&mut self) {
        self.history_valid = false;
    }

    pub fn composite(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
        settings: BloomSettings,
        paused: bool,
    ) {
        queue.write_buffer(&self.settings_buffer, 0, bytemuck::bytes_of(&settings));

        Self::run_pass(
            encoder,
            "Bloom bright-pass render pass",
            &self.bright_target.view,
            &self.bright_pipeline,
            &self.bright_bind_group,
        );
        Self::run_pass(
            encoder,
            "Bloom near horizontal render pass",
            &self.near_horizontal_target.view,
            &self.horizontal_pipeline,
            &self.near_horizontal_bind_group,
        );
        Self::run_pass(
            encoder,
            "Bloom near vertical render pass",
            &self.near_vertical_target.view,
            &self.vertical_pipeline,
            &self.near_vertical_bind_group,
        );
        Self::run_pass(
            encoder,
            "Bloom wide downsample render pass",
            &self.wide_source_target.view,
            &self.downsample_pipeline,
            &self.wide_downsample_bind_group,
        );
        Self::run_pass(
            encoder,
            "Bloom wide horizontal render pass",
            &self.wide_horizontal_target.view,
            &self.horizontal_pipeline,
            &self.wide_horizontal_bind_group,
        );
        Self::run_pass(
            encoder,
            "Bloom wide vertical render pass",
            &self.wide_vertical_target.view,
            &self.wide_vertical_pipeline,
            &self.wide_vertical_bind_group,
        );

        if !self.history_valid {
            Self::clear_target(
                encoder,
                "Clear bloom history A",
                &self.history_a_target.view,
            );
            Self::clear_target(
                encoder,
                "Clear bloom history B",
                &self.history_b_target.view,
            );
            self.history_valid = true;
        }

        if paused {
            let composite_bind_group = if self.history_a_is_previous {
                &self.composite_history_a_bind_group
            } else {
                &self.composite_history_b_bind_group
            };

            Self::run_pass(
                encoder,
                "Bloom paused composite render pass",
                output_view,
                &self.composite_pipeline,
                composite_bind_group,
            );

            return;
        }

        let (history_target, history_bind_group, composite_bind_group) =
            if self.history_a_is_previous {
                (
                    &self.history_b_target.view,
                    &self.history_a_to_b_bind_group,
                    &self.composite_history_b_bind_group,
                )
            } else {
                (
                    &self.history_a_target.view,
                    &self.history_b_to_a_bind_group,
                    &self.composite_history_a_bind_group,
                )
            };

        Self::run_pass(
            encoder,
            "Bloom temporal light-history pass",
            history_target,
            &self.history_pipeline,
            history_bind_group,
        );

        Self::run_pass(
            encoder,
            "Bloom final composite render pass",
            output_view,
            &self.composite_pipeline,
            composite_bind_group,
        );

        self.history_a_is_previous = !self.history_a_is_previous;
    }
}
