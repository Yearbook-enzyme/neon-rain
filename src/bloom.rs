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

    // Half-resolution post-processing targets.
    bright_target: RenderTarget,
    horizontal_target: RenderTarget,
    vertical_target: RenderTarget,

    sampler: wgpu::Sampler,

    post_bind_group_layout: wgpu::BindGroupLayout,
    composite_bind_group_layout: wgpu::BindGroupLayout,

    bright_bind_group: wgpu::BindGroup,
    horizontal_bind_group: wgpu::BindGroup,
    vertical_bind_group: wgpu::BindGroup,
    composite_bind_group: wgpu::BindGroup,

    bright_pipeline: wgpu::RenderPipeline,
    horizontal_pipeline: wgpu::RenderPipeline,
    vertical_pipeline: wgpu::RenderPipeline,
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

        let half_width = (width / 2).max(1);
        let half_height = (height / 2).max(1);

        let bright_target =
            RenderTarget::new(device, "Bloom bright-pass target", half_width, half_height);

        let horizontal_target = RenderTarget::new(
            device,
            "Bloom horizontal blur target",
            half_width,
            half_height,
        );

        let vertical_target = RenderTarget::new(
            device,
            "Bloom vertical blur target",
            half_width,
            half_height,
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

        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Bloom composite bind group layout"),

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

        let bright_bind_group = Self::create_post_bind_group(
            device,
            &post_bind_group_layout,
            &hdr_target.view,
            &sampler,
            "Bloom bright-pass bind group",
        );

        let horizontal_bind_group = Self::create_post_bind_group(
            device,
            &post_bind_group_layout,
            &bright_target.view,
            &sampler,
            "Bloom horizontal blur bind group",
        );

        let vertical_bind_group = Self::create_post_bind_group(
            device,
            &post_bind_group_layout,
            &horizontal_target.view,
            &sampler,
            "Bloom vertical blur bind group",
        );

        let composite_bind_group = Self::create_composite_bind_group(
            device,
            &composite_bind_group_layout,
            &hdr_target.view,
            &vertical_target.view,
            &sampler,
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
            horizontal_target,
            vertical_target,

            sampler,

            post_bind_group_layout,
            composite_bind_group_layout,

            bright_bind_group,
            horizontal_bind_group,
            vertical_bind_group,
            composite_bind_group,

            bright_pipeline,
            horizontal_pipeline,
            vertical_pipeline,
            composite_pipeline,
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

    fn create_composite_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        original_view: &wgpu::TextureView,
        bloom_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
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
                    resource: wgpu::BindingResource::TextureView(bloom_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.hdr_target = RenderTarget::new(device, "Matrix HDR render target", width, height);

        self.view = self.hdr_target.view.clone();

        let half_width = (width / 2).max(1);
        let half_height = (height / 2).max(1);

        self.bright_target =
            RenderTarget::new(device, "Bloom bright-pass target", half_width, half_height);

        self.horizontal_target = RenderTarget::new(
            device,
            "Bloom horizontal blur target",
            half_width,
            half_height,
        );

        self.vertical_target = RenderTarget::new(
            device,
            "Bloom vertical blur target",
            half_width,
            half_height,
        );

        self.bright_bind_group = Self::create_post_bind_group(
            device,
            &self.post_bind_group_layout,
            &self.hdr_target.view,
            &self.sampler,
            "Bloom bright-pass bind group",
        );

        self.horizontal_bind_group = Self::create_post_bind_group(
            device,
            &self.post_bind_group_layout,
            &self.bright_target.view,
            &self.sampler,
            "Bloom horizontal blur bind group",
        );

        self.vertical_bind_group = Self::create_post_bind_group(
            device,
            &self.post_bind_group_layout,
            &self.horizontal_target.view,
            &self.sampler,
            "Bloom vertical blur bind group",
        );

        self.composite_bind_group = Self::create_composite_bind_group(
            device,
            &self.composite_bind_group_layout,
            &self.hdr_target.view,
            &self.vertical_target.view,
            &self.sampler,
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

    pub fn composite(&self, encoder: &mut wgpu::CommandEncoder, output_view: &wgpu::TextureView) {
        Self::run_pass(
            encoder,
            "Bloom bright-pass render pass",
            &self.bright_target.view,
            &self.bright_pipeline,
            &self.bright_bind_group,
        );

        Self::run_pass(
            encoder,
            "Bloom horizontal blur render pass",
            &self.horizontal_target.view,
            &self.horizontal_pipeline,
            &self.horizontal_bind_group,
        );

        Self::run_pass(
            encoder,
            "Bloom vertical blur render pass",
            &self.vertical_target.view,
            &self.vertical_pipeline,
            &self.vertical_bind_group,
        );

        Self::run_pass(
            encoder,
            "Bloom final composite render pass",
            output_view,
            &self.composite_pipeline,
            &self.composite_bind_group,
        );
    }
}
