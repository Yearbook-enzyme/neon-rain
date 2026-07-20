use std::mem::size_of;

use bytemuck::{Pod, Zeroable};
use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Wrap,
};

use crate::music::{MusicInspectorFrame, MusicReactor};

const SIGNAL_COUNT: usize = 50;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InspectorVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl InspectorVertex {
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x2,
            1 => Float32x4,
        ],
    };
}

#[derive(Clone, Copy)]
enum VisualStyle {
    Meter,
    Centered,
    Pulse,
    Phase,
    Color,
}

#[derive(Clone, Copy)]
enum ValueFormat {
    Decimal,
    Percent,
    Multiplier,
    Bpm,
    Degrees,
    Signed,
    Rgb,
}

#[derive(Clone, Copy)]
struct SignalReading {
    group: &'static str,
    name: &'static str,
    description: &'static str,
    value: f32,
    minimum: f32,
    maximum: f32,
    baseline: f32,
    style: VisualStyle,
    format: ValueFormat,
    color: [f32; 3],
}

#[derive(Clone, Copy)]
enum InspectorSignal {
    Overall,
    Bass,
    Mid,
    Treble,
    Stereo,
    Onset,
    Tempo,
    BeatConfidence,
    Busyness,
    Performance,
    SectionEnergy,
    AdaptiveFloor,
    AdaptivePeak,
    BeatPulse,
    TrackPulse,
    BeatPhase,
    BarPhase,
    ColorPhase,
    ColorAccent,
    CameraSection,
    ClimaxPulse,
    MoodbarEnergy,
    MoodbarChange,
    MoodbarProgress,
    MoodbarColor,
    StructureDrive,
    DetailDrive,
    CallResponse,
    SignatureEvent,
    SpeedMultiplier,
    GlowMultiplier,
    ExposureMultiplier,
    FovOffset,
    CascadeMultiplier,
    CouplingMultiplier,
    RainDensityMultiplier,
    RainEnergyMultiplier,
    HeadActivityMultiplier,
    GlyphVariation,
    PaletteMix,
    WallpaperColorMultiplier,
    ApparitionFrequencyMultiplier,
    ApparitionOpacityMultiplier,
    MediaCycleMultiplier,
    CinematicTempoMultiplier,
    StereoCameraDrift,
    SpatialStrength,
    RainVisibilityFloor,
    PrimaryPalette,
    SecondaryPalette,
}

impl InspectorSignal {
    const ALL: [Self; SIGNAL_COUNT] = [
        Self::Overall,
        Self::Bass,
        Self::Mid,
        Self::Treble,
        Self::Stereo,
        Self::Onset,
        Self::Tempo,
        Self::BeatConfidence,
        Self::Busyness,
        Self::Performance,
        Self::SectionEnergy,
        Self::AdaptiveFloor,
        Self::AdaptivePeak,
        Self::BeatPulse,
        Self::TrackPulse,
        Self::BeatPhase,
        Self::BarPhase,
        Self::ColorPhase,
        Self::ColorAccent,
        Self::CameraSection,
        Self::ClimaxPulse,
        Self::MoodbarEnergy,
        Self::MoodbarChange,
        Self::MoodbarProgress,
        Self::MoodbarColor,
        Self::StructureDrive,
        Self::DetailDrive,
        Self::CallResponse,
        Self::SignatureEvent,
        Self::SpeedMultiplier,
        Self::GlowMultiplier,
        Self::ExposureMultiplier,
        Self::FovOffset,
        Self::CascadeMultiplier,
        Self::CouplingMultiplier,
        Self::RainDensityMultiplier,
        Self::RainEnergyMultiplier,
        Self::HeadActivityMultiplier,
        Self::GlyphVariation,
        Self::PaletteMix,
        Self::WallpaperColorMultiplier,
        Self::ApparitionFrequencyMultiplier,
        Self::ApparitionOpacityMultiplier,
        Self::MediaCycleMultiplier,
        Self::CinematicTempoMultiplier,
        Self::StereoCameraDrift,
        Self::SpatialStrength,
        Self::RainVisibilityFloor,
        Self::PrimaryPalette,
        Self::SecondaryPalette,
    ];

    fn reading(self, frame: &MusicInspectorFrame) -> SignalReading {
        let green = [0.18, 1.0, 0.46];
        let cyan = [0.10, 0.90, 1.0];
        let amber = [1.0, 0.68, 0.16];
        let magenta = [1.0, 0.24, 0.72];
        let color_average = |rgb: [f32; 3]| (rgb[0] + rgb[1] + rgb[2]) / 3.0;

        match self {
            Self::Overall => reading(
                "INPUT",
                "Overall level",
                "Smoothed full-band audio energy entering the music reactor.",
                frame.overall,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                green,
            ),
            Self::Bass => reading(
                "INPUT",
                "Bass",
                "Low-frequency energy. This is a major driver of weight, speed, and camera pressure.",
                frame.bass,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                amber,
            ),
            Self::Mid => reading(
                "INPUT",
                "Midrange",
                "Mid-frequency energy used heavily by density, coupling, and palette motion.",
                frame.mid,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                green,
            ),
            Self::Treble => reading(
                "INPUT",
                "Treble",
                "High-frequency detail used by glyph activity, glow, and fine variation.",
                frame.treble,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                cyan,
            ),
            Self::Stereo => reading(
                "INPUT",
                "Stereo balance",
                "Signed left-right energy balance from the captured audio stream.",
                frame.stereo,
                -1.0,
                1.0,
                0.0,
                VisualStyle::Centered,
                ValueFormat::Signed,
                magenta,
            ),
            Self::Onset => reading(
                "INPUT",
                "Onset strength",
                "Fast transient detector. Percussive attacks should appear as sharp expansions.",
                frame.onset,
                0.0,
                1.0,
                0.0,
                VisualStyle::Pulse,
                ValueFormat::Percent,
                amber,
            ),
            Self::Tempo => reading(
                "INPUT",
                "Estimated tempo",
                "Current smoothed tempo estimate used for rhythmic phase and cinematic timing.",
                frame.tempo_bpm,
                45.0,
                210.0,
                120.0,
                VisualStyle::Meter,
                ValueFormat::Bpm,
                cyan,
            ),
            Self::BeatConfidence => reading(
                "INPUT",
                "Beat confidence",
                "How strongly the analyzer trusts its beat timing estimate.",
                frame.beat_confidence,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                green,
            ),
            Self::Busyness => reading(
                "INPUT",
                "Busyness",
                "Density and irregularity estimate used to restrain camera movement when music is crowded.",
                frame.busyness,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                magenta,
            ),
            Self::Performance => reading(
                "ADAPTIVE",
                "Performance drive",
                "Loudness normalized against the song's moving floor and peak.",
                frame.performance,
                0.0,
                1.2,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Decimal,
                green,
            ),
            Self::SectionEnergy => reading(
                "ADAPTIVE",
                "Section energy",
                "Slow macro-energy estimate that follows verses, builds, and climaxes.",
                frame.section_energy,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                cyan,
            ),
            Self::AdaptiveFloor => reading(
                "ADAPTIVE",
                "Adaptive floor",
                "The moving quiet reference used to normalize songs with different mastering levels.",
                frame.adaptive_floor,
                0.0,
                0.72,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Decimal,
                cyan,
            ),
            Self::AdaptivePeak => reading(
                "ADAPTIVE",
                "Adaptive peak",
                "The moving loud reference used to normalize the current performance.",
                frame.adaptive_peak,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Decimal,
                amber,
            ),
            Self::BeatPulse => reading(
                "RHYTHM",
                "Beat pulse",
                "A short decaying impulse emitted on each accepted beat.",
                frame.beat_pulse,
                0.0,
                1.35,
                0.0,
                VisualStyle::Pulse,
                ValueFormat::Decimal,
                amber,
            ),
            Self::TrackPulse => reading(
                "RHYTHM",
                "Track-change pulse",
                "A slower pulse emitted when Strawberry advances to a new track.",
                frame.track_pulse,
                0.0,
                1.0,
                0.0,
                VisualStyle::Pulse,
                ValueFormat::Percent,
                cyan,
            ),
            Self::BeatPhase => reading(
                "RHYTHM",
                "Beat phase",
                "Position through the current estimated beat, from zero to one.",
                frame.beat_phase,
                0.0,
                1.0,
                0.0,
                VisualStyle::Phase,
                ValueFormat::Percent,
                amber,
            ),
            Self::BarPhase => reading(
                "RHYTHM",
                "Bar phase",
                "Position through a four-beat phrase used for larger motion and call-response behavior.",
                frame.bar_phase,
                0.0,
                1.0,
                0.0,
                VisualStyle::Phase,
                ValueFormat::Percent,
                magenta,
            ),
            Self::ColorPhase => reading(
                "COLOR",
                "Color phase",
                "Continuous palette position advanced by musical detail and beat accents.",
                frame.color_phase,
                0.0,
                1.0,
                0.0,
                VisualStyle::Phase,
                ValueFormat::Percent,
                cyan,
            ),
            Self::ColorAccent => reading(
                "COLOR",
                "Color accent",
                "Decaying accent impulse that brightens and shifts palette reactions on beats.",
                frame.color_accent,
                0.0,
                1.0,
                0.0,
                VisualStyle::Pulse,
                ValueFormat::Percent,
                magenta,
            ),
            Self::CameraSection => reading(
                "STRUCTURE",
                "Camera section",
                "Slow structural energy used to decide when camera language can become larger.",
                frame.camera_section,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                cyan,
            ),
            Self::ClimaxPulse => reading(
                "STRUCTURE",
                "Climax pulse",
                "Rare macro-event pulse emitted at structurally strong eight-beat boundaries.",
                frame.climax_pulse,
                0.0,
                1.0,
                0.0,
                VisualStyle::Pulse,
                ValueFormat::Percent,
                magenta,
            ),
            Self::MoodbarEnergy => reading(
                "MOODBAR",
                "Moodbar energy",
                "Brightness of the precomputed moodbar color at the current track position.",
                frame.moodbar_energy,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                green,
            ),
            Self::MoodbarChange => reading(
                "MOODBAR",
                "Moodbar change",
                "Local color contrast and look-ahead change in the track's moodbar timeline.",
                frame.moodbar_change,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                amber,
            ),
            Self::MoodbarProgress => reading(
                "MOODBAR",
                "Track progress",
                "Current normalized position through the active track.",
                frame.moodbar_progress,
                0.0,
                1.0,
                0.0,
                VisualStyle::Phase,
                ValueFormat::Percent,
                cyan,
            ),
            Self::MoodbarColor => reading(
                "MOODBAR",
                "Moodbar color",
                "The RGB moodbar sample aligned with the current playback position.",
                color_average(frame.moodbar_rgb),
                0.0,
                1.0,
                0.0,
                VisualStyle::Color,
                ValueFormat::Rgb,
                frame.moodbar_rgb,
            ),
            Self::StructureDrive => reading(
                "DERIVED",
                "Structure drive",
                "Combined macro-structure value built from section energy, moodbar energy, and track pulses.",
                frame.structure_drive,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                cyan,
            ),
            Self::DetailDrive => reading(
                "DERIVED",
                "Detail drive",
                "Combined micro-detail value built from transients, treble, mids, and performance.",
                frame.detail_drive,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                green,
            ),
            Self::CallResponse => reading(
                "DERIVED",
                "Call-response balance",
                "Signed phrase-level alternation that moves emphasis between opposite sides of the field.",
                frame.call_response_balance,
                -1.0,
                1.0,
                0.0,
                VisualStyle::Centered,
                ValueFormat::Signed,
                magenta,
            ),
            Self::SignatureEvent => reading(
                "DERIVED",
                "Signature event",
                "A rare climax event after structural, rhythmic, and semantic checks agree.",
                frame.signature_event,
                0.0,
                1.0,
                0.0,
                VisualStyle::Pulse,
                ValueFormat::Percent,
                amber,
            ),
            Self::SpeedMultiplier => reading(
                "MAPPED OUTPUT",
                "Rain speed multiplier",
                "Final music multiplier applied to simulation speed.",
                frame.speed_multiplier,
                0.70,
                2.20,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                green,
            ),
            Self::GlowMultiplier => reading(
                "MAPPED OUTPUT",
                "Glow multiplier",
                "Final music multiplier applied to glow strength.",
                frame.glow_multiplier,
                0.58,
                3.25,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                cyan,
            ),
            Self::ExposureMultiplier => reading(
                "MAPPED OUTPUT",
                "Exposure multiplier",
                "Final music multiplier applied to automatic exposure.",
                frame.exposure_multiplier,
                0.76,
                1.58,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                green,
            ),
            Self::FovOffset => reading(
                "MAPPED OUTPUT",
                "FOV offset",
                "Music-driven narrowing of field of view; negative values move the camera optically inward.",
                frame.fov_offset,
                -6.2,
                0.0,
                0.0,
                VisualStyle::Centered,
                ValueFormat::Degrees,
                magenta,
            ),
            Self::CascadeMultiplier => reading(
                "MAPPED OUTPUT",
                "Cascade multiplier",
                "Final strength of cascading propagation through streams.",
                frame.cascade_multiplier,
                0.62,
                3.35,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                amber,
            ),
            Self::CouplingMultiplier => reading(
                "MAPPED OUTPUT",
                "Media coupling multiplier",
                "Final music pressure applied to rain-and-image coupling.",
                frame.coupling_multiplier,
                0.60,
                2.70,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                magenta,
            ),
            Self::RainDensityMultiplier => reading(
                "MAPPED OUTPUT",
                "Rain density multiplier",
                "Final music multiplier controlling how much rain is allowed to remain visible.",
                frame.rain_density_multiplier,
                0.64,
                2.75,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                green,
            ),
            Self::RainEnergyMultiplier => reading(
                "MAPPED OUTPUT",
                "Rain energy multiplier",
                "Final music multiplier controlling stream brightness and energetic presence.",
                frame.rain_energy_multiplier,
                0.55,
                2.85,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                amber,
            ),
            Self::HeadActivityMultiplier => reading(
                "MAPPED OUTPUT",
                "Head activity multiplier",
                "Final music multiplier controlling bright leading glyph activity.",
                frame.head_activity_multiplier,
                0.65,
                3.25,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                cyan,
            ),
            Self::GlyphVariation => reading(
                "MAPPED OUTPUT",
                "Glyph variation",
                "Final zero-to-one amount of music-driven glyph-level variation.",
                frame.glyph_variation,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                cyan,
            ),
            Self::PaletteMix => reading(
                "MAPPED OUTPUT",
                "Palette mix",
                "Amount of generated music palette blended into the active theme.",
                frame.palette_mix,
                0.0,
                0.98,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                magenta,
            ),
            Self::WallpaperColorMultiplier => reading(
                "MAPPED OUTPUT",
                "Wallpaper color multiplier",
                "Music-driven gain applied to wallpaper-derived color influence.",
                frame.wallpaper_color_multiplier,
                0.75,
                2.55,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                magenta,
            ),
            Self::ApparitionFrequencyMultiplier => reading(
                "MAPPED OUTPUT",
                "Apparition frequency",
                "Music multiplier controlling how often image apparitions can emerge.",
                frame.apparition_frequency_multiplier,
                0.55,
                2.35,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                cyan,
            ),
            Self::ApparitionOpacityMultiplier => reading(
                "MAPPED OUTPUT",
                "Apparition opacity",
                "Music multiplier controlling apparition visibility.",
                frame.apparition_opacity_multiplier,
                0.62,
                2.25,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                green,
            ),
            Self::MediaCycleMultiplier => reading(
                "MAPPED OUTPUT",
                "Media-cycle speed",
                "Music multiplier controlling automatic image cycling time.",
                frame.media_cycle_multiplier,
                0.55,
                1.85,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                cyan,
            ),
            Self::CinematicTempoMultiplier => reading(
                "MAPPED OUTPUT",
                "Cinematic tempo",
                "Tempo-derived multiplier controlling the director's movement cadence.",
                frame.cinematic_tempo_multiplier,
                0.72,
                1.42,
                1.0,
                VisualStyle::Centered,
                ValueFormat::Multiplier,
                magenta,
            ),
            Self::StereoCameraDrift => reading(
                "MAPPED OUTPUT",
                "Stereo camera drift",
                "Signed camera drift obtained from left-right audio balance.",
                frame.stereo_camera_drift,
                -0.16,
                0.16,
                0.0,
                VisualStyle::Centered,
                ValueFormat::Signed,
                magenta,
            ),
            Self::SpatialStrength => reading(
                "MAPPED OUTPUT",
                "Spatial-field strength",
                "Amount of music-driven variation distributed across world-space lanes and depth.",
                frame.spatial_strength,
                0.0,
                1.0,
                0.0,
                VisualStyle::Meter,
                ValueFormat::Percent,
                cyan,
            ),
            Self::RainVisibilityFloor => reading(
                "MAPPED OUTPUT",
                "Rain visibility floor",
                "Minimum rain presence protected while media coupling is active.",
                frame.rain_visibility_floor,
                0.54,
                0.84,
                0.62,
                VisualStyle::Centered,
                ValueFormat::Percent,
                green,
            ),
            Self::PrimaryPalette => reading(
                "COLOR OUTPUT",
                "Primary palette color",
                "Generated primary RGB color after spectral, rhythmic, moodbar, and semantic influence.",
                color_average(frame.primary_palette),
                0.0,
                1.48,
                0.0,
                VisualStyle::Color,
                ValueFormat::Rgb,
                frame.primary_palette,
            ),
            Self::SecondaryPalette => reading(
                "COLOR OUTPUT",
                "Secondary palette color",
                "Generated complementary RGB color used for heads and accents.",
                color_average(frame.secondary_palette),
                0.0,
                1.46,
                0.0,
                VisualStyle::Color,
                ValueFormat::Rgb,
                frame.secondary_palette,
            ),
        }
    }
}

fn reading(
    group: &'static str,
    name: &'static str,
    description: &'static str,
    value: f32,
    minimum: f32,
    maximum: f32,
    baseline: f32,
    style: VisualStyle,
    format: ValueFormat,
    color: [f32; 3],
) -> SignalReading {
    SignalReading {
        group,
        name,
        description,
        value,
        minimum,
        maximum,
        baseline,
        style,
        format,
        color,
    }
}

#[derive(Clone, Copy, Default)]
struct InspectorLayout {
    panel_left: f32,
    panel_top: f32,
    panel_right: f32,
    panel_bottom: f32,
    title_x: f32,
    title_y: f32,
    info_x: f32,
    info_y: f32,
    value_x: f32,
    value_y: f32,
    footer_x: f32,
    footer_y: f32,
    visual_left: f32,
    visual_top: f32,
    visual_right: f32,
    visual_bottom: f32,
    ui_scale: f32,
}

pub struct SignalInspector {
    visible: bool,
    selected: usize,
    gain: f32,
    frozen: bool,
    auto_cycle: bool,
    auto_elapsed: f32,
    frame: MusicInspectorFrame,

    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,

    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    title_buffer: Buffer,
    info_buffer: Buffer,
    value_buffer: Buffer,
    footer_buffer: Buffer,

    layout: InspectorLayout,
    width: u32,
    height: u32,
    scale_factor: f64,
}

impl SignalInspector {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Signal inspector shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("signal_inspector.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Signal inspector pipeline layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Signal inspector pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(InspectorVertex::LAYOUT)],
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
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Signal inspector vertices"),
            size: (192 * size_of::<InspectorVertex>()) as u64,
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
        let title_buffer = Buffer::new(&mut font_system, Metrics::new(24.0, 32.0));
        let info_buffer = Buffer::new(&mut font_system, Metrics::new(17.0, 23.0));
        let value_buffer = Buffer::new(&mut font_system, Metrics::new(20.0, 28.0));
        let footer_buffer = Buffer::new(&mut font_system, Metrics::new(13.0, 18.0));

        let mut inspector = Self {
            visible: false,
            selected: 0,
            gain: 1.0,
            frozen: false,
            auto_cycle: false,
            auto_elapsed: 0.0,
            frame: MusicInspectorFrame::default(),
            pipeline,
            vertex_buffer,
            vertex_count: 0,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            title_buffer,
            info_buffer,
            value_buffer,
            footer_buffer,
            layout: InspectorLayout::default(),
            width,
            height,
            scale_factor,
        };
        inspector.resize(queue, width, height, scale_factor);
        inspector
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        self.auto_elapsed = 0.0;
    }

    pub fn next(&mut self) {
        self.selected = (self.selected + 1) % SIGNAL_COUNT;
        self.auto_elapsed = 0.0;
    }

    pub fn previous(&mut self) {
        self.selected = (self.selected + SIGNAL_COUNT - 1) % SIGNAL_COUNT;
        self.auto_elapsed = 0.0;
    }

    pub fn adjust_gain(&mut self, direction: f32) {
        self.gain = (self.gain * if direction > 0.0 { 1.25 } else { 0.80 }).clamp(0.25, 8.0);
    }

    pub fn toggle_freeze(&mut self) {
        self.frozen = !self.frozen;
    }

    pub fn toggle_auto_cycle(&mut self) {
        self.auto_cycle = !self.auto_cycle;
        self.auto_elapsed = 0.0;
    }

    pub fn reset(&mut self) {
        self.gain = 1.0;
        self.frozen = false;
        self.auto_cycle = false;
        self.auto_elapsed = 0.0;
    }

    pub fn update(&mut self, dt: f32, music: &MusicReactor) {
        if !self.frozen {
            self.frame = music.inspector_frame();
        }
        if self.visible && self.auto_cycle && !self.frozen {
            self.auto_elapsed += dt.max(0.0);
            if self.auto_elapsed >= 4.0 {
                self.auto_elapsed = 0.0;
                self.next();
            }
        }
    }

    pub fn resize(&mut self, _queue: &wgpu::Queue, width: u32, height: u32, scale_factor: f64) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.scale_factor = scale_factor;
        self.layout = calculate_layout(width, height, scale_factor);
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

        let signal = InspectorSignal::ALL[self.selected];
        let reading = signal.reading(&self.frame);
        let vertices = build_vertices(self.width, self.height, self.layout, reading, self.gain);
        self.vertex_count = vertices.len() as u32;
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));

        let ui = self.layout.ui_scale;
        let title = format!(
            "SIGNAL INSPECTOR  //  {:02}/{:02}  //  {}",
            self.selected + 1,
            SIGNAL_COUNT,
            reading.group,
        );
        let info = format!(
            "{}\n{}\n\nTrack: {}\nMusic capture: {}",
            reading.name,
            reading.description,
            if self.frame.track.is_empty() {
                "(no track metadata)"
            } else {
                self.frame.track.as_str()
            },
            if self.frame.active {
                "LIVE"
            } else {
                "INACTIVE / QUIET"
            },
        );
        let value = format!(
            "{}    gain {:.2}x{}{}",
            format_value(reading, &self.frame),
            self.gain,
            if self.frozen { "    FROZEN" } else { "" },
            if self.auto_cycle { "    AUTO" } else { "" },
        );
        let footer = "F2/Esc close   ←/→ signal   ↑/↓ gain   Space freeze   A auto-tour   R reset   Normal controls are suspended";

        configure_buffer(
            &mut self.title_buffer,
            &mut self.font_system,
            &title,
            Metrics::new(23.0 * ui, 31.0 * ui),
            (self.layout.panel_right - self.layout.title_x - 24.0 * ui).max(1.0),
            44.0 * ui,
        );
        configure_buffer(
            &mut self.info_buffer,
            &mut self.font_system,
            &info,
            Metrics::new(16.0 * ui, 22.0 * ui),
            (self.layout.panel_right - self.layout.info_x - 30.0 * ui).max(1.0),
            145.0 * ui,
        );
        configure_buffer(
            &mut self.value_buffer,
            &mut self.font_system,
            &value,
            Metrics::new(20.0 * ui, 28.0 * ui),
            (self.layout.panel_right - self.layout.value_x - 30.0 * ui).max(1.0),
            40.0 * ui,
        );
        configure_buffer(
            &mut self.footer_buffer,
            &mut self.font_system,
            footer,
            Metrics::new(12.5 * ui, 17.0 * ui),
            (self.layout.panel_right - self.layout.footer_x - 24.0 * ui).max(1.0),
            26.0 * ui,
        );

        self.viewport.update(
            queue,
            Resolution {
                width: self.width,
                height: self.height,
            },
        );
        let bounds = TextBounds {
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
                bounds,
                default_color: Color::rgb(220, 255, 235),
                custom_glyphs: &[],
            },
            TextArea {
                buffer: &self.info_buffer,
                left: self.layout.info_x,
                top: self.layout.info_y,
                scale: 1.0,
                bounds,
                default_color: Color::rgb(180, 235, 204),
                custom_glyphs: &[],
            },
            TextArea {
                buffer: &self.value_buffer,
                left: self.layout.value_x,
                top: self.layout.value_y,
                scale: 1.0,
                bounds,
                default_color: Color::rgb(225, 255, 235),
                custom_glyphs: &[],
            },
            TextArea {
                buffer: &self.footer_buffer,
                left: self.layout.footer_x,
                top: self.layout.footer_y,
                scale: 1.0,
                bounds,
                default_color: Color::rgb(110, 205, 150),
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
            eprintln!("Could not prepare signal inspector text: {error}");
            return;
        }

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Signal inspector render pass"),
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
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.draw(0..self.vertex_count, 0..1);
        if let Err(error) = self
            .text_renderer
            .render(&self.atlas, &self.viewport, &mut render_pass)
        {
            eprintln!("Could not render signal inspector text: {error}");
        }
        drop(render_pass);
        self.atlas.trim();
    }
}

fn format_value(reading: SignalReading, frame: &MusicInspectorFrame) -> String {
    match reading.format {
        ValueFormat::Decimal => format!("value {:.3}", reading.value),
        ValueFormat::Percent => format!("value {:>6.1}%", reading.value * 100.0),
        ValueFormat::Multiplier => format!("value {:.3}x", reading.value),
        ValueFormat::Bpm => format!("value {:.1} BPM", reading.value),
        ValueFormat::Degrees => format!("value {:+.2}°", reading.value),
        ValueFormat::Signed => format!("value {:+.3}", reading.value),
        ValueFormat::Rgb => {
            let rgb = if reading.name == "Moodbar color" {
                frame.moodbar_rgb
            } else if reading.name == "Primary palette color" {
                frame.primary_palette
            } else {
                frame.secondary_palette
            };
            format!("RGB {:.3}  {:.3}  {:.3}", rgb[0], rgb[1], rgb[2])
        }
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
    buffer.set_wrap(Wrap::Word);
    buffer.set_text(
        text,
        &Attrs::new().family(Family::Monospace),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(font_system, false);
}

fn calculate_layout(width: u32, height: u32, scale_factor: f64) -> InspectorLayout {
    let width = width as f32;
    let height = height as f32;
    let scale = scale_factor.max(0.5) as f32;
    let logical_height = height / scale;
    let ui_scale = scale * (logical_height / 1080.0).clamp(0.76, 1.12);
    let margin_x = (width * 0.055).max(34.0 * ui_scale);
    let margin_y = (height * 0.065).max(28.0 * ui_scale);
    let panel_left = margin_x;
    let panel_top = margin_y;
    let panel_right = width - margin_x;
    let panel_bottom = height - margin_y;
    let inner = 30.0 * ui_scale;
    let visual_top = panel_top + 235.0 * ui_scale;
    let visual_bottom = panel_bottom - 92.0 * ui_scale;

    InspectorLayout {
        panel_left,
        panel_top,
        panel_right,
        panel_bottom,
        title_x: panel_left + inner,
        title_y: panel_top + 18.0 * ui_scale,
        info_x: panel_left + inner,
        info_y: panel_top + 68.0 * ui_scale,
        value_x: panel_left + inner,
        value_y: panel_bottom - 76.0 * ui_scale,
        footer_x: panel_left + inner,
        footer_y: panel_bottom - 34.0 * ui_scale,
        visual_left: panel_left + inner,
        visual_top,
        visual_right: panel_right - inner,
        visual_bottom,
        ui_scale,
    }
}

fn build_vertices(
    width: u32,
    height: u32,
    layout: InspectorLayout,
    reading: SignalReading,
    gain: f32,
) -> Vec<InspectorVertex> {
    let mut vertices = Vec::with_capacity(120);
    let width = width as f32;
    let height = height as f32;
    let ui = layout.ui_scale;
    let border = (2.0 * ui).max(2.0);
    let line = (2.0 * ui).max(2.0);

    push_rect(
        &mut vertices,
        [0.0, 0.0, width, height],
        width,
        height,
        [0.0, 0.0, 0.0, 0.86],
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
        [0.03, 0.70, 0.38, 0.70],
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
        [0.002, 0.015, 0.010, 0.985],
    );
    push_rect(
        &mut vertices,
        [
            layout.visual_left,
            layout.visual_top,
            layout.visual_right,
            layout.visual_bottom,
        ],
        width,
        height,
        [0.006, 0.045, 0.027, 0.98],
    );

    let color = [
        reading.color[0].clamp(0.0, 1.4),
        reading.color[1].clamp(0.0, 1.4),
        reading.color[2].clamp(0.0, 1.4),
        0.92,
    ];
    let dim = [
        reading.color[0] * 0.20,
        reading.color[1] * 0.20,
        reading.color[2] * 0.20,
        0.65,
    ];
    let normal = normalize(reading.value, reading.minimum, reading.maximum);
    let baseline = normalize(reading.baseline, reading.minimum, reading.maximum);
    let amplified = (baseline + (normal - baseline) * gain).clamp(0.0, 1.0);
    let left = layout.visual_left + 26.0 * ui;
    let right = layout.visual_right - 26.0 * ui;
    let top = layout.visual_top + 26.0 * ui;
    let bottom = layout.visual_bottom - 26.0 * ui;
    let center_x = (left + right) * 0.5;
    let center_y = (top + bottom) * 0.5;

    match reading.style {
        VisualStyle::Meter => {
            let meter_top = center_y - 28.0 * ui;
            let meter_bottom = center_y + 28.0 * ui;
            push_rect(
                &mut vertices,
                [left, meter_top, right, meter_bottom],
                width,
                height,
                dim,
            );
            let fill_right = left + (right - left) * amplified;
            push_rect(
                &mut vertices,
                [left, meter_top, fill_right, meter_bottom],
                width,
                height,
                color,
            );
            push_rect(
                &mut vertices,
                [
                    left + (right - left) * baseline - line * 0.5,
                    meter_top - 12.0 * ui,
                    left + (right - left) * baseline + line * 0.5,
                    meter_bottom + 12.0 * ui,
                ],
                width,
                height,
                [0.72, 1.0, 0.82, 0.78],
            );
        }
        VisualStyle::Centered => {
            let meter_top = center_y - 28.0 * ui;
            let meter_bottom = center_y + 28.0 * ui;
            push_rect(
                &mut vertices,
                [left, meter_top, right, meter_bottom],
                width,
                height,
                dim,
            );
            let base_x = left + (right - left) * baseline;
            let value_x = left + (right - left) * amplified;
            push_rect(
                &mut vertices,
                [
                    base_x.min(value_x),
                    meter_top,
                    base_x.max(value_x),
                    meter_bottom,
                ],
                width,
                height,
                color,
            );
            push_rect(
                &mut vertices,
                [
                    base_x - line,
                    meter_top - 15.0 * ui,
                    base_x + line,
                    meter_bottom + 15.0 * ui,
                ],
                width,
                height,
                [0.72, 1.0, 0.82, 0.90],
            );
            push_rect(
                &mut vertices,
                [
                    value_x - 5.0 * ui,
                    meter_top - 8.0 * ui,
                    value_x + 5.0 * ui,
                    meter_bottom + 8.0 * ui,
                ],
                width,
                height,
                color,
            );
        }
        VisualStyle::Pulse => {
            let maximum = ((right - left).min(bottom - top) * 0.40).max(20.0 * ui);
            let radius = (18.0 * ui + maximum * amplified.sqrt()).min(maximum);
            push_rect(
                &mut vertices,
                [
                    center_x - radius,
                    center_y - radius,
                    center_x + radius,
                    center_y + radius,
                ],
                width,
                height,
                [color[0], color[1], color[2], 0.26],
            );
            let core = (8.0 * ui + radius * 0.34).max(8.0 * ui);
            push_rect(
                &mut vertices,
                [
                    center_x - core,
                    center_y - core,
                    center_x + core,
                    center_y + core,
                ],
                width,
                height,
                color,
            );
        }
        VisualStyle::Phase => {
            let y = bottom - (bottom - top) * 0.22;
            push_rect(
                &mut vertices,
                [left, y - line, right, y + line],
                width,
                height,
                dim,
            );
            let x = left + (right - left) * amplified;
            push_rect(
                &mut vertices,
                [x - 8.0 * ui, top, x + 8.0 * ui, bottom],
                width,
                height,
                [color[0], color[1], color[2], 0.28],
            );
            push_rect(
                &mut vertices,
                [x - 3.0 * ui, top, x + 3.0 * ui, bottom],
                width,
                height,
                color,
            );
        }
        VisualStyle::Color => {
            let inset = 32.0 * ui;
            push_rect(
                &mut vertices,
                [left + inset, top + inset, right - inset, bottom - inset],
                width,
                height,
                [color[0], color[1], color[2], 0.88],
            );
            let swatch_height = 18.0 * ui;
            let channel_width = (right - left - inset * 2.0) / 3.0;
            let channels = [reading.color[0], reading.color[1], reading.color[2]];
            for (index, channel) in channels.iter().enumerate() {
                let channel_left = left + inset + channel_width * index as f32;
                let channel_right = channel_left + channel_width - 8.0 * ui;
                let channel_top = bottom - inset - swatch_height * channel.clamp(0.0, 1.0) * 5.0;
                push_rect(
                    &mut vertices,
                    [channel_left, channel_top, channel_right, bottom - inset],
                    width,
                    height,
                    [
                        if index == 0 { 1.0 } else { 0.08 },
                        if index == 1 { 1.0 } else { 0.08 },
                        if index == 2 { 1.0 } else { 0.08 },
                        0.92,
                    ],
                );
            }
        }
    }

    vertices
}

fn normalize(value: f32, minimum: f32, maximum: f32) -> f32 {
    if maximum <= minimum {
        return 0.0;
    }
    ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0)
}

fn push_rect(
    vertices: &mut Vec<InspectorVertex>,
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
        InspectorVertex {
            position: [left, top],
            color,
        },
        InspectorVertex {
            position: [right, top],
            color,
        },
        InspectorVertex {
            position: [right, bottom],
            color,
        },
        InspectorVertex {
            position: [left, top],
            color,
        },
        InspectorVertex {
            position: [right, bottom],
            color,
        },
        InspectorVertex {
            position: [left, bottom],
            color,
        },
    ]);
}
