mod atlas;
mod bloom;
mod help_overlay;
mod music;
mod resilience;
mod settings;
mod signal_inspector;
mod simulation;

use std::{
    collections::HashMap,
    env, fs,
    io::{Read, Write},
    mem::size_of,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Instant, UNIX_EPOCH},
};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, OwnedDisplayHandle},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Fullscreen, Window, WindowId},
};

use atlas::{ATLAS_HEIGHT, ATLAS_WIDTH, create_glyph_atlas};
use bloom::{Bloom, BloomSettings, HDR_FORMAT};
use help_overlay::HelpOverlay;
use music::{MusicColorMode, MusicReactor};
use settings::{CliAction, LaunchOptions, Preferences};
use signal_inspector::SignalInspector;
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

const MAX_RAIN_GLYPH_INSTANCES: usize = simulation::MAX_STREAMS * GLYPHS_PER_STREAM;
const MAX_GLYPH_INSTANCES: usize = MAX_RAIN_GLYPH_INSTANCES + 12288;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlyphInstance {
    // center x, center y, glyph width, glyph height
    position_size: [f32; 4],

    // core red, core green, core blue, local glow strength
    color_glow: [f32; 4],

    // local glow red, green, blue, reserved
    glow_color: [f32; 4],

    // glyph atlas index, depth band, reserved, reserved
    glyph_data: [u32; 4],
}

impl GlyphInstance {
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x4,
            1 => Float32x4,
            2 => Float32x4,
            3 => Uint32x4,
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

fn load_image_pixels(path: &Path, max_dimension: u32) -> Option<(u32, u32, Vec<[f32; 3]>)> {
    let image = image::ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .ok()?
        .decode()
        .ok()?;

    let image = image.thumbnail(max_dimension, max_dimension).to_rgb8();

    let width = image.width();
    let height = image.height();
    let pixels = image
        .pixels()
        .map(|pixel| {
            [
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                pixel[2] as f32 / 255.0,
            ]
        })
        .collect();

    Some((width, height, pixels))
}

fn build_filtered_media_pixels(
    width: u32,
    height: u32,
    pixels: &[[f32; 3]],
    target_resolution: u32,
) -> (u32, u32, Vec<[f32; 3]>) {
    if width == 0 || height == 0 || pixels.is_empty() {
        return (0, 0, Vec::new());
    }

    let longest = width.max(height).max(1);
    let scale = (target_resolution.max(8) as f32 / longest as f32).min(1.0);
    let filtered_width = ((width as f32 * scale).round() as u32).max(1);
    let filtered_height = ((height as f32 * scale).round() as u32).max(1);
    let mut filtered = vec![[0.0; 3]; (filtered_width * filtered_height) as usize];

    for fy in 0..filtered_height {
        for fx in 0..filtered_width {
            let source_x = fx as f32 / filtered_width as f32 * width as f32;
            let source_y = fy as f32 / filtered_height as f32 * height as f32;

            let base_x = source_x.floor() as i32;
            let base_y = source_y.floor() as i32;

            let mut accum = [0.0; 3];
            let mut total_weight = 0.0;

            for offset_y in -1..=1 {
                for offset_x in -1..=1 {
                    let sample_x = (base_x + offset_x).clamp(0, width as i32 - 1) as u32;
                    let sample_y = (base_y + offset_y).clamp(0, height as i32 - 1) as u32;
                    let weight = match (offset_x.abs(), offset_y.abs()) {
                        (0, 0) => 0.36,
                        (1, 0) | (0, 1) => 0.12,
                        _ => 0.04,
                    };
                    let sample = pixels[(sample_y * width + sample_x) as usize];
                    accum[0] += sample[0] * weight;
                    accum[1] += sample[1] * weight;
                    accum[2] += sample[2] * weight;
                    total_weight += weight;
                }
            }

            filtered[(fy * filtered_width + fx) as usize] = [
                accum[0] / total_weight,
                accum[1] / total_weight,
                accum[2] / total_weight,
            ];
        }
    }

    (filtered_width, filtered_height, filtered)
}

fn visual_scale(size: winit::dpi::PhysicalSize<u32>) -> f32 {
    let pixel_area = size.width.max(1) as f32 * size.height.max(1) as f32;

    (pixel_area / (1600.0 * 900.0)).sqrt().clamp(0.72, 2.40)
}

const WORLD_NEAR_Z: f32 = 3.0;
const WORLD_DEPTH_SPAN: f32 = 42.0;
const WORLD_FAR_Z: f32 = WORLD_NEAR_Z + WORLD_DEPTH_SPAN;
const CAMERA_MIN_FOV_Y: f32 = 32.0;
const CAMERA_MAX_FOV_Y: f32 = 88.0;

#[derive(Clone, Copy, Debug, Default)]
struct CameraInput {
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    boost: bool,
    precision: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AutoFlightMode {
    #[default]
    Off,
    Forward,
    Weave,
    Orbit,
    Tunnel,
}

impl AutoFlightMode {
    fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Some(Self::Off),
            "forward" | "straight" => Some(Self::Forward),
            "weave" | "drift" => Some(Self::Weave),
            "orbit" => Some(Self::Orbit),
            "tunnel" => Some(Self::Tunnel),
            _ => None,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Off => Self::Forward,
            Self::Forward => Self::Weave,
            Self::Weave => Self::Orbit,
            Self::Orbit => Self::Tunnel,
            Self::Tunnel => Self::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Forward => "forward",
            Self::Weave => "weave",
            Self::Orbit => "orbit",
            Self::Tunnel => "tunnel",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CinematicDirector {
    enabled: bool,
    timer: f32,
    next_change: f32,
    serial: u32,
    lateral_transition_duration: f32,
}

impl Default for CinematicDirector {
    fn default() -> Self {
        Self {
            enabled: true,
            timer: 0.0,
            next_change: 8.0,
            serial: 0,
            lateral_transition_duration: 7.2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CameraState {
    position: [f32; 3],
    velocity: [f32; 3],
    yaw: f32,
    pitch: f32,
    target_yaw: f32,
    target_pitch: f32,
    fov_y: f32,
    target_fov_y: f32,
    movement_speed: f32,
    auto_flight: AutoFlightMode,
    mouse_look: bool,
    show_reticle: bool,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            velocity: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            target_yaw: 0.0,
            target_pitch: 0.0,
            fov_y: 60.0,
            target_fov_y: 60.0,
            movement_speed: 9.0,
            auto_flight: AutoFlightMode::Forward,
            mouse_look: false,
            show_reticle: true,
        }
    }
}

fn camera_basis(yaw: f32, pitch: f32) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let sin_yaw = yaw.sin();
    let cos_yaw = yaw.cos();
    let sin_pitch = pitch.sin();
    let cos_pitch = pitch.cos();

    let right = [cos_yaw, 0.0, -sin_yaw];
    let up = [-sin_yaw * sin_pitch, cos_pitch, -cos_yaw * sin_pitch];
    let forward = [sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch];

    (right, up, forward)
}

fn world_to_camera(relative: [f32; 3], yaw: f32, pitch: f32) -> [f32; 3] {
    let (right, up, forward) = camera_basis(yaw, pitch);

    [
        relative[0] * right[0] + relative[1] * right[1] + relative[2] * right[2],
        relative[0] * up[0] + relative[1] * up[1] + relative[2] * up[2],
        relative[0] * forward[0] + relative[1] * forward[1] + relative[2] * forward[2],
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaMode {
    Off,
    Silhouette,
    Color,
    Ghost,
}

impl MediaMode {
    fn next(self) -> Self {
        match self {
            Self::Off => Self::Silhouette,
            Self::Silhouette => Self::Color,
            Self::Color => Self::Ghost,
            Self::Ghost => Self::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Silhouette => "silhouette",
            Self::Color => "color",
            Self::Ghost => "ghost",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct MediaSample {
    color: [f32; 3],
    mask: f32,
    weight: f32,
    carve: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaSpaceMode {
    Flat,
    Portal,
    Extruded,
    Volume,
}

impl MediaSpaceMode {
    fn next(self) -> Self {
        match self {
            Self::Flat => Self::Portal,
            Self::Portal => Self::Extruded,
            Self::Extruded => Self::Volume,
            Self::Volume => Self::Flat,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::Portal => "portal",
            Self::Extruded => "extruded",
            Self::Volume => "volume",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaPreviewMode {
    Off,
    Image,
    Matrix,
    Rain,
}

impl MediaPreviewMode {
    fn next(self) -> Self {
        match self {
            Self::Off => Self::Image,
            Self::Image => Self::Matrix,
            Self::Matrix => Self::Rain,
            Self::Rain => Self::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Image => "image",
            Self::Matrix => "matrix",
            Self::Rain => "rain",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaCouplingMode {
    Influence,
    Formed,
    Diagnostic,
}

impl MediaCouplingMode {
    fn next(self) -> Self {
        match self {
            Self::Influence => Self::Formed,
            Self::Formed => Self::Diagnostic,
            Self::Diagnostic => Self::Influence,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Influence => "influence",
            Self::Formed => "rain-formed",
            Self::Diagnostic => "diagnostic",
        }
    }

    fn depth_scale(self) -> f32 {
        match self {
            Self::Influence => 1.0,
            Self::Formed => 3.60,
            Self::Diagnostic => 4.20,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ImageSignature {
    luminance: f32,
    saturation: f32,
    warmth: f32,
    contrast: f32,
}

fn analyze_image_signature(pixels: &[[f32; 3]]) -> ImageSignature {
    if pixels.is_empty() {
        return ImageSignature::default();
    }

    let mut luminance_sum = 0.0f32;
    let mut luminance_squared_sum = 0.0f32;
    let mut saturation_sum = 0.0f32;
    let mut warmth_sum = 0.0f32;

    for color in pixels {
        let luminance = color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722;
        let maximum = color[0].max(color[1]).max(color[2]);
        let minimum = color[0].min(color[1]).min(color[2]);
        let saturation = if maximum > 0.001 {
            (maximum - minimum) / maximum
        } else {
            0.0
        };
        let warmth = (0.5 + (color[0] - color[2]) * 0.5).clamp(0.0, 1.0);

        luminance_sum += luminance;
        luminance_squared_sum += luminance * luminance;
        saturation_sum += saturation;
        warmth_sum += warmth;
    }

    let count = pixels.len() as f32;
    let luminance = luminance_sum / count;
    let variance = (luminance_squared_sum / count - luminance * luminance).max(0.0);

    ImageSignature {
        luminance: luminance.clamp(0.0, 1.0),
        saturation: (saturation_sum / count).clamp(0.0, 1.0),
        warmth: (warmth_sum / count).clamp(0.0, 1.0),
        contrast: variance.sqrt().clamp(0.0, 1.0),
    }
}

#[derive(Clone, Debug)]
struct CouplingImage {
    name: String,
    width: u32,
    height: u32,
    pixels: Vec<[f32; 3]>,
    signature: ImageSignature,
}

const COUPLING_CACHE_MAGIC: &[u8; 8] = b"NRCPLG03";

fn coupling_cache_root() -> Option<PathBuf> {
    env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|root| root.join("neon-rain").join("coupling-v3"))
}

fn stable_path_hash(path: &Path) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn source_signature(path: &Path) -> Option<(u64, u64, u32)> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((metadata.len(), modified.as_secs(), modified.subsec_nanos()))
}

fn coupling_cache_path(path: &Path) -> Option<PathBuf> {
    let root = coupling_cache_root()?;
    Some(root.join(format!("{:016x}.nrc", stable_path_hash(path))))
}

fn read_u32(reader: &mut impl Read) -> Option<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> Option<u64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes).ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn load_cached_coupling_image(path: &Path) -> Option<CouplingImage> {
    let cache_path = coupling_cache_path(path)?;
    let (source_size, modified_secs, modified_nanos) = source_signature(path)?;
    let mut reader = fs::File::open(cache_path).ok()?;

    let mut magic = [0u8; 8];
    reader.read_exact(&mut magic).ok()?;
    if &magic != COUPLING_CACHE_MAGIC {
        return None;
    }

    if read_u64(&mut reader)? != source_size
        || read_u64(&mut reader)? != modified_secs
        || read_u32(&mut reader)? != modified_nanos
    {
        return None;
    }

    let width = read_u32(&mut reader)?;
    let height = read_u32(&mut reader)?;
    let pixel_count = read_u32(&mut reader)? as usize;
    let expected_count = width.checked_mul(height)? as usize;

    if width == 0 || height == 0 || pixel_count != expected_count || pixel_count > 1_000_000 {
        return None;
    }

    let mut pixels = Vec::with_capacity(pixel_count);
    for _ in 0..pixel_count {
        let mut channels = [0.0f32; 3];
        for channel in &mut channels {
            let mut bytes = [0u8; 4];
            reader.read_exact(&mut bytes).ok()?;
            *channel = f32::from_le_bytes(bytes);
        }
        pixels.push(channels);
    }

    let signature = analyze_image_signature(&pixels);

    Some(CouplingImage {
        name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("image")
            .to_owned(),
        width,
        height,
        pixels,
        signature,
    })
}

fn write_cached_coupling_image(path: &Path, image: &CouplingImage) -> std::io::Result<()> {
    let Some(cache_path) = coupling_cache_path(path) else {
        return Ok(());
    };
    let Some((source_size, modified_secs, modified_nanos)) = source_signature(path) else {
        return Ok(());
    };
    let Some(parent) = cache_path.parent() else {
        return Ok(());
    };

    fs::create_dir_all(parent)?;
    let temporary = cache_path.with_extension("tmp");
    let mut writer = fs::File::create(&temporary)?;

    writer.write_all(COUPLING_CACHE_MAGIC)?;
    writer.write_all(&source_size.to_le_bytes())?;
    writer.write_all(&modified_secs.to_le_bytes())?;
    writer.write_all(&modified_nanos.to_le_bytes())?;
    writer.write_all(&image.width.to_le_bytes())?;
    writer.write_all(&image.height.to_le_bytes())?;
    writer.write_all(&(image.pixels.len() as u32).to_le_bytes())?;

    for pixel in &image.pixels {
        for channel in pixel {
            writer.write_all(&channel.to_le_bytes())?;
        }
    }

    writer.flush()?;
    fs::rename(temporary, cache_path)?;
    Ok(())
}

fn load_or_build_coupling_image(path: &Path) -> (Option<CouplingImage>, bool) {
    if let Some(image) = load_cached_coupling_image(path) {
        return (Some(image), true);
    }

    let image = load_image_pixels(path, 256).and_then(|(width, height, pixels)| {
        let (filtered_width, filtered_height, filtered_pixels) =
            build_filtered_media_pixels(width, height, &pixels, 64);

        if filtered_pixels.is_empty() {
            return None;
        }

        let signature = analyze_image_signature(&filtered_pixels);

        Some(CouplingImage {
            name: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("image")
                .to_owned(),
            width: filtered_width,
            height: filtered_height,
            pixels: filtered_pixels,
            signature,
        })
    });

    if let Some(image) = image.as_ref() {
        if let Err(error) = write_cached_coupling_image(path, image) {
            eprintln!(
                "Could not write coupling cache for {}: {error}",
                path.display()
            );
        }
    }

    (image, false)
}

#[derive(Clone, Copy, Debug, Default)]
struct TrackVisualMemoryEntry {
    coupling_index: usize,
    visits: u32,
}

fn track_visual_memory_cache_path() -> PathBuf {
    let mut root = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    root.push("neon-rain");
    root.push("track-visual-memory-v1.tsv");
    root
}

fn load_track_visual_memories(path: &Path) -> HashMap<String, TrackVisualMemoryEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };

    let mut entries = HashMap::new();
    for line in text.lines() {
        let mut fields = line.splitn(3, '\t');
        let Some(track_key) = fields.next() else {
            continue;
        };
        let Some(index_text) = fields.next() else {
            continue;
        };
        let visits_text = fields.next().unwrap_or("1");
        let Ok(coupling_index) = index_text.parse::<usize>() else {
            continue;
        };
        let visits = visits_text.parse::<u32>().unwrap_or(1);
        entries.insert(
            track_key.to_string(),
            TrackVisualMemoryEntry {
                coupling_index,
                visits,
            },
        );
    }

    entries
}

fn save_track_visual_memories(
    path: &Path,
    memories: &HashMap<String, TrackVisualMemoryEntry>,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut lines = Vec::with_capacity(memories.len());
    let mut keys = memories.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    for key in keys {
        if let Some(entry) = memories.get(&key) {
            lines.push(format!(
                "{}\t{}\t{}",
                key, entry.coupling_index, entry.visits
            ));
        }
    }

    fs::write(path, lines.join("\n"))
}

#[derive(Debug)]
struct MediaField {
    source_root: Option<PathBuf>,
    files: Vec<PathBuf>,
    current_index: usize,
    width: u32,
    height: u32,
    pixels: Vec<[f32; 3]>,
    filtered_width: u32,
    filtered_height: u32,
    filtered_pixels: Vec<[f32; 3]>,
    coupling_images: Vec<Option<CouplingImage>>,
    coupling_current_index: usize,
    coupling_previous_index: usize,
    coupling_transition: f32,
    coupling_transition_duration: f32,
    coupling_serial: u32,
    coupling_depth: f32,
    coupling_extent: f32,
    mode: MediaMode,
    opacity: f32,
    contrast: f32,
    scale: f32,
    depth: f32,
    offset_x: f32,
    offset_y: f32,
    lock_to_camera: bool,
    space_mode: MediaSpaceMode,
    show_gizmo: bool,
    preview_mode: MediaPreviewMode,
    coupling_mode: MediaCouplingMode,
    auto_cycle: bool,
    auto_cycle_interval: f32,
    auto_cycle_timer: f32,
    track_visual_memories: HashMap<String, TrackVisualMemoryEntry>,
    track_visual_memory_path: PathBuf,
    last_track_key: String,
}

impl Default for MediaField {
    fn default() -> Self {
        Self {
            source_root: None,
            files: Vec::new(),
            current_index: 0,
            width: 0,
            height: 0,
            pixels: Vec::new(),
            filtered_width: 0,
            filtered_height: 0,
            filtered_pixels: Vec::new(),
            coupling_images: Vec::new(),
            coupling_current_index: 0,
            coupling_previous_index: 0,
            coupling_transition: 1.0,
            coupling_transition_duration: 4.2,
            coupling_serial: 0,
            coupling_depth: 24.0,
            coupling_extent: 1.12,
            mode: MediaMode::Off,
            opacity: 1.00,
            contrast: 2.00,
            scale: 1.25,
            depth: 18.0,
            offset_x: 0.0,
            offset_y: 0.0,
            lock_to_camera: false,
            space_mode: MediaSpaceMode::Volume,
            show_gizmo: false,
            preview_mode: MediaPreviewMode::Off,
            coupling_mode: MediaCouplingMode::Formed,
            auto_cycle: true,
            auto_cycle_interval: 16.0,
            auto_cycle_timer: 0.0,
            track_visual_memories: HashMap::new(),
            track_visual_memory_path: track_visual_memory_cache_path(),
            last_track_key: String::new(),
        }
    }
}

impl MediaField {
    fn from_path(path: Option<PathBuf>) -> Self {
        let mut media = Self::default();
        media.track_visual_memories = load_track_visual_memories(&media.track_visual_memory_path);

        if let Some(path) = path {
            media.set_source(path);
        }

        media
    }

    fn is_supported_image(path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "webp"
                )
            })
            .unwrap_or(false)
    }

    fn collect_images(root: &Path, output: &mut Vec<PathBuf>) {
        if root.is_file() {
            if Self::is_supported_image(root) {
                output.push(root.to_path_buf());
            }

            return;
        }

        let Ok(entries) = fs::read_dir(root) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                Self::collect_images(&path, output);
            } else if Self::is_supported_image(&path) {
                output.push(path);
            }
        }
    }

    fn set_source(&mut self, path: PathBuf) {
        self.source_root = Some(path.clone());
        self.files.clear();
        Self::collect_images(&path, &mut self.files);
        self.files.sort();
        self.current_index = 0;

        if self.files.is_empty() {
            self.width = 0;
            self.height = 0;
            self.pixels.clear();
            self.filtered_width = 0;
            self.filtered_height = 0;
            self.filtered_pixels.clear();
            self.coupling_images.clear();
            self.coupling_current_index = 0;
            self.coupling_previous_index = 0;
            self.coupling_transition = 1.0;
            self.mode = MediaMode::Off;

            eprintln!("No PNG, JPEG, or WebP images found at {}", path.display());
            return;
        }

        self.preload_coupling_images();

        if self.load_current() {
            self.mode = MediaMode::Color;
            self.select_coupling_index_immediate(self.current_index);

            println!(
                "Loaded {} media images from {}",
                self.files.len(),
                path.display(),
            );
        }
    }

    fn reload(&mut self) {
        let Some(path) = self.source_root.clone() else {
            return;
        };

        let previous_name = self
            .current_path()
            .and_then(|path| path.file_name())
            .map(|name| name.to_owned());

        self.files.clear();
        Self::collect_images(&path, &mut self.files);
        self.files.sort();

        if self.files.is_empty() {
            self.current_index = 0;
            self.width = 0;
            self.height = 0;
            self.pixels.clear();
            self.filtered_width = 0;
            self.filtered_height = 0;
            self.filtered_pixels.clear();
            self.coupling_images.clear();
            self.coupling_current_index = 0;
            self.coupling_previous_index = 0;
            self.coupling_transition = 1.0;
            self.mode = MediaMode::Off;
            return;
        }

        self.current_index = previous_name
            .and_then(|name| {
                self.files.iter().position(|candidate| {
                    candidate
                        .file_name()
                        .is_some_and(|candidate_name| candidate_name == name)
                })
            })
            .unwrap_or(0);

        self.preload_coupling_images();
        self.load_current();
        self.select_coupling_index_immediate(self.current_index);
    }

    fn preload_coupling_images(&mut self) {
        self.coupling_images.clear();
        self.coupling_images.reserve(self.files.len());

        let mut loaded = 0usize;
        let mut cache_hits = 0usize;
        let mut cache_builds = 0usize;

        for path in &self.files {
            let (cached, was_cache_hit) = load_or_build_coupling_image(path);

            if cached.is_some() {
                loaded += 1;
                if was_cache_hit {
                    cache_hits += 1;
                } else {
                    cache_builds += 1;
                }
            }

            self.coupling_images.push(cached);
        }

        self.coupling_current_index = self.first_valid_coupling_index().unwrap_or(0);
        self.coupling_previous_index = self.coupling_current_index;
        self.coupling_transition = 1.0;

        println!(
            "Prepared {loaded} coupling images ({cache_hits} persistent-cache hits, {cache_builds} rebuilt)"
        );
    }

    fn first_valid_coupling_index(&self) -> Option<usize> {
        self.coupling_images.iter().position(Option::is_some)
    }

    fn sync_track_memory(&mut self, track_key: &str) {
        let track_key = track_key.trim();
        if track_key.is_empty() {
            return;
        }

        if self.last_track_key == track_key {
            return;
        }

        self.last_track_key.clear();
        self.last_track_key.push_str(track_key);

        if let Some(entry) = self.track_visual_memories.get(track_key).copied() {
            if self
                .coupling_images
                .get(entry.coupling_index)
                .is_some_and(Option::is_some)
                && entry.coupling_index != self.coupling_current_index
            {
                self.begin_coupling_transition(entry.coupling_index);
            }
        } else {
            self.remember_current_track_visual();
        }
    }

    fn remember_current_track_visual(&mut self) {
        if self.last_track_key.is_empty() {
            return;
        }

        let entry = self
            .track_visual_memories
            .entry(self.last_track_key.clone())
            .or_default();
        entry.coupling_index = self.coupling_current_index;
        entry.visits = entry.visits.saturating_add(1).max(1);

        if let Err(error) =
            save_track_visual_memories(&self.track_visual_memory_path, &self.track_visual_memories)
        {
            eprintln!("Could not save track visual memory: {error}");
        }
    }

    fn select_music_matched_index(
        &self,
        target: [f32; 4],
        seed: u32,
        excluded: Option<usize>,
    ) -> Option<usize> {
        let valid_count = self
            .coupling_images
            .iter()
            .filter(|image| image.is_some())
            .count();
        let mut best: Option<(usize, f32)> = None;

        for (index, image) in self.coupling_images.iter().enumerate() {
            let Some(image) = image.as_ref() else {
                continue;
            };
            if valid_count > 1 && excluded == Some(index) {
                continue;
            }

            let signature = image.signature;
            let distance = (signature.luminance - target[0]).abs() * 1.85
                + (signature.saturation - target[1]).abs() * 0.70
                + (signature.warmth - target[2]).abs() * 0.62
                + (signature.contrast - target[3]).abs() * 1.05;
            let variety = stable_unit(seed ^ (index as u32).wrapping_mul(0x9e37_79b9));
            let score = distance + variety * (0.10 + target[3] * 0.16);

            let should_replace = match best {
                Some((_, best_score)) => score < best_score,
                None => true,
            };
            if should_replace {
                best = Some((index, score));
            }
        }

        best.map(|(index, _)| index)
    }

    fn select_coupling_index_immediate(&mut self, requested_index: usize) {
        if self.coupling_images.is_empty() {
            return;
        }

        let selected = if self
            .coupling_images
            .get(requested_index)
            .is_some_and(Option::is_some)
        {
            requested_index
        } else {
            self.first_valid_coupling_index().unwrap_or(0)
        };

        self.coupling_current_index = selected;
        self.coupling_previous_index = selected;
        self.coupling_transition = 1.0;
    }

    fn begin_coupling_transition(&mut self, requested_index: usize) {
        if self.coupling_images.is_empty()
            || !self
                .coupling_images
                .get(requested_index)
                .is_some_and(Option::is_some)
            || requested_index == self.coupling_current_index
        {
            return;
        }

        self.coupling_previous_index = self.coupling_current_index;
        self.coupling_current_index = requested_index;
        self.coupling_transition = 0.0;
    }

    fn coupling_name(&self) -> &str {
        self.coupling_images
            .get(self.coupling_current_index)
            .and_then(Option::as_ref)
            .map(|image| image.name.as_str())
            .unwrap_or("none")
    }

    fn load_current(&mut self) -> bool {
        if self.files.is_empty() {
            return false;
        }

        for _ in 0..self.files.len() {
            let path = self.files[self.current_index].clone();

            match load_image_pixels(&path, 1280) {
                Some((width, height, pixels)) => {
                    self.width = width;
                    self.height = height;
                    self.pixels = pixels;
                    let (filtered_width, filtered_height, filtered_pixels) =
                        build_filtered_media_pixels(self.width, self.height, &self.pixels, 96);
                    self.filtered_width = filtered_width;
                    self.filtered_height = filtered_height;
                    self.filtered_pixels = filtered_pixels;
                    self.auto_cycle_timer = 0.0;

                    println!(
                        "Media image {}/{}: {} ({}x{})",
                        self.current_index + 1,
                        self.files.len(),
                        path.display(),
                        self.width,
                        self.height,
                    );

                    return true;
                }

                None => {
                    eprintln!("Could not load {}", path.display());
                    self.current_index = (self.current_index + 1) % self.files.len();
                }
            }
        }

        self.width = 0;
        self.height = 0;
        self.pixels.clear();
        self.filtered_width = 0;
        self.filtered_height = 0;
        self.filtered_pixels.clear();
        self.coupling_images.clear();
        self.coupling_current_index = 0;
        self.coupling_previous_index = 0;
        self.coupling_transition = 1.0;
        self.mode = MediaMode::Off;
        false
    }

    fn current_path(&self) -> Option<&Path> {
        self.files.get(self.current_index).map(PathBuf::as_path)
    }

    fn current_name(&self) -> &str {
        self.current_path()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("none")
    }

    fn title_label(&self) -> String {
        let lock_label = if self.lock_to_camera {
            "camera"
        } else {
            "world"
        };

        let cycle_progress = self.auto_cycle_timer.min(self.auto_cycle_interval.max(0.1));
        let transition_percent = (self.coupling_transition * 100.0).round();

        if self.files.is_empty() {
            return format!(
                "media none — {} coupling:{} cycle:{} {:.1}/{:.1}s field:camera",
                self.space_mode.label(),
                self.coupling_mode.label(),
                if self.auto_cycle { "on" } else { "off" },
                cycle_progress,
                self.auto_cycle_interval,
            );
        }

        format!(
            "{} {} preview:{} coupling:{} cycle:{} {:.1}/{:.1}s xfade:{:.0}% plane:{} d{:.1} ({:+.1}, {:+.1}) — plane:{} field:camera:{}",
            self.mode.label(),
            self.space_mode.label(),
            self.preview_mode.label(),
            self.coupling_mode.label(),
            if self.auto_cycle { "on" } else { "off" },
            cycle_progress,
            self.auto_cycle_interval,
            transition_percent,
            lock_label,
            self.depth,
            self.offset_x,
            self.offset_y,
            self.current_name(),
            self.coupling_name(),
        )
    }

    fn cycle_mode(&mut self) {
        if self.files.is_empty() {
            self.mode = MediaMode::Off;
            return;
        }

        self.mode = self.mode.next();
        println!("Media mode: {}", self.mode.label());
    }

    fn next_image(&mut self) {
        if self.files.is_empty() {
            return;
        }

        self.current_index = (self.current_index + 1) % self.files.len();
        self.load_current();
        self.begin_coupling_transition(self.current_index);
    }

    fn previous_image(&mut self) {
        if self.files.is_empty() {
            return;
        }

        self.current_index = if self.current_index == 0 {
            self.files.len() - 1
        } else {
            self.current_index - 1
        };

        self.load_current();
        self.begin_coupling_transition(self.current_index);
    }

    fn cycle_space_mode(&mut self) {
        self.space_mode = self.space_mode.next();
        println!("Media space mode: {}", self.space_mode.label());
    }

    fn toggle_space_lock(&mut self) {
        self.lock_to_camera = !self.lock_to_camera;
        println!(
            "Media plane lock: {}",
            if self.lock_to_camera {
                "camera"
            } else {
                "world"
            },
        );
    }

    fn toggle_gizmo(&mut self) {
        self.show_gizmo = !self.show_gizmo;
        println!("Media spatial guide: {}", self.show_gizmo);
    }

    fn cycle_preview_mode(&mut self) {
        self.preview_mode = self.preview_mode.next();
        println!("Media preview mode: {}", self.preview_mode.label());
    }

    fn cycle_coupling_mode(&mut self) {
        self.coupling_mode = self.coupling_mode.next();
        println!("Rain/media coupling: {}", self.coupling_mode.label());
    }

    fn toggle_auto_cycle(&mut self) {
        self.auto_cycle = !self.auto_cycle;
        self.auto_cycle_timer = 0.0;
        println!(
            "Media auto-cycle: {}",
            if self.auto_cycle { "on" } else { "off" },
        );
    }

    fn adjust_auto_cycle_interval(&mut self, amount: f32) {
        self.auto_cycle_interval = (self.auto_cycle_interval + amount).clamp(4.0, 60.0);
        println!(
            "Media auto-cycle interval: {:.1}s",
            self.auto_cycle_interval
        );
    }

    fn update_auto_cycle(&mut self, dt: f32, music_target: Option<[f32; 4]>) -> bool {
        if dt > 0.0 && self.coupling_transition < 1.0 {
            self.coupling_transition = (self.coupling_transition
                + dt / self.coupling_transition_duration.max(0.1))
            .clamp(0.0, 1.0);
        }

        if !self.auto_cycle || self.coupling_images.len() <= 1 || dt <= 0.0 {
            return false;
        }

        if !self.auto_cycle_timer.is_finite() {
            self.auto_cycle_timer = 0.0;
        }

        self.auto_cycle_timer += dt;
        if self.auto_cycle_timer < self.auto_cycle_interval {
            return false;
        }

        self.auto_cycle_timer = self
            .auto_cycle_timer
            .rem_euclid(self.auto_cycle_interval.max(0.1));
        self.coupling_serial = self.coupling_serial.wrapping_add(1);

        let valid_indices: Vec<usize> = self
            .coupling_images
            .iter()
            .enumerate()
            .filter_map(|(index, image)| image.as_ref().map(|_| index))
            .collect();

        if valid_indices.len() <= 1 {
            return false;
        }

        let current_position = valid_indices
            .iter()
            .position(|candidate| *candidate == self.coupling_current_index)
            .unwrap_or(0);
        let maximum_step = valid_indices.len() - 1;
        let random_step = 1
            + ((stable_unit(self.coupling_serial ^ 0x61c8_8647) * maximum_step as f32).floor()
                as usize)
                .min(maximum_step - 1);
        let random_selected = valid_indices[(current_position + random_step) % valid_indices.len()];
        let selected = music_target
            .and_then(|target| {
                self.select_music_matched_index(
                    target,
                    self.coupling_serial ^ 0x4d4f_4f44,
                    Some(self.coupling_current_index),
                )
            })
            .unwrap_or(random_selected);

        self.begin_coupling_transition(selected);
        println!(
            "Auto-cycled rain coupling to {} ({}/{})",
            self.coupling_name(),
            self.coupling_current_index + 1,
            self.coupling_images.len(),
        );
        true
    }

    fn sample_coupling_image(image: &CouplingImage, image_u: f32, image_v: f32) -> [f32; 3] {
        if image.width == 0 || image.height == 0 || image.pixels.is_empty() {
            return [0.0; 3];
        }

        let x = image_u.clamp(0.0, 1.0) * image.width.saturating_sub(1) as f32;
        let y = image_v.clamp(0.0, 1.0) * image.height.saturating_sub(1) as f32;

        let x0 = x.floor() as u32;
        let y0 = y.floor() as u32;
        let x1 = (x0 + 1).min(image.width - 1);
        let y1 = (y0 + 1).min(image.height - 1);
        let tx = x - x0 as f32;
        let ty = y - y0 as f32;

        let sample = |sx: u32, sy: u32| image.pixels[(sy * image.width + sx) as usize];
        let top_left = sample(x0, y0);
        let top_right = sample(x1, y0);
        let bottom_left = sample(x0, y1);
        let bottom_right = sample(x1, y1);

        let top = [
            mix(top_left[0], top_right[0], tx),
            mix(top_left[1], top_right[1], tx),
            mix(top_left[2], top_right[2], tx),
        ];
        let bottom = [
            mix(bottom_left[0], bottom_right[0], tx),
            mix(bottom_left[1], bottom_right[1], tx),
            mix(bottom_left[2], bottom_right[2], tx),
        ];

        [
            mix(top[0], bottom[0], ty),
            mix(top[1], bottom[1], ty),
            mix(top[2], bottom[2], ty),
        ]
    }

    fn sample_filtered_color(&self, image_u: f32, image_v: f32) -> [f32; 3] {
        let current = self
            .coupling_images
            .get(self.coupling_current_index)
            .and_then(Option::as_ref);
        let previous = self
            .coupling_images
            .get(self.coupling_previous_index)
            .and_then(Option::as_ref);

        match (previous, current) {
            (Some(previous), Some(current)) => {
                let previous_color = Self::sample_coupling_image(previous, image_u, image_v);
                let current_color = Self::sample_coupling_image(current, image_u, image_v);
                let transition = smoothstep(0.0, 1.0, self.coupling_transition);

                [
                    mix(previous_color[0], current_color[0], transition),
                    mix(previous_color[1], current_color[1], transition),
                    mix(previous_color[2], current_color[2], transition),
                ]
            }
            (None, Some(current)) => Self::sample_coupling_image(current, image_u, image_v),
            (Some(previous), None) => Self::sample_coupling_image(previous, image_u, image_v),
            (None, None) => {
                if self.filtered_width == 0
                    || self.filtered_height == 0
                    || self.filtered_pixels.is_empty()
                {
                    [0.0; 3]
                } else {
                    let fallback_pixels = self.filtered_pixels.clone();
                    let fallback = CouplingImage {
                        name: String::new(),
                        width: self.filtered_width,
                        height: self.filtered_height,
                        signature: analyze_image_signature(&fallback_pixels),
                        pixels: fallback_pixels,
                    };
                    Self::sample_coupling_image(&fallback, image_u, image_v)
                }
            }
        }
    }

    fn coupling_plane_size(&self, viewport_aspect: f32) -> (f32, f32) {
        let plane_height = 24.0 * self.scale.max(0.1);
        let plane_width = plane_height * viewport_aspect.clamp(0.50, 3.20) * self.coupling_extent;
        (plane_width, plane_height)
    }

    fn coupling_basis_and_center(
        &self,
        camera: &CameraState,
    ) -> ([f32; 3], [f32; 3], [f32; 3], [f32; 3]) {
        let (right, up, forward) = camera_basis(camera.yaw, camera.pitch);
        let center = [
            camera.position[0] + forward[0] * self.coupling_depth,
            camera.position[1] + forward[1] * self.coupling_depth,
            camera.position[2] + forward[2] * self.coupling_depth,
        ];
        (right, up, forward, center)
    }

    fn plane_size(&self) -> (f32, f32) {
        let image_aspect = self.width as f32 / self.height.max(1) as f32;
        let plane_height = 18.0 * self.scale.max(0.1);
        let plane_width = plane_height * image_aspect.max(0.1);
        (plane_width, plane_height)
    }

    fn plane_basis_and_center(
        &self,
        camera: &CameraState,
    ) -> ([f32; 3], [f32; 3], [f32; 3], [f32; 3]) {
        if self.lock_to_camera {
            let (right, up, forward) = camera_basis(camera.yaw, camera.pitch);
            (right, up, forward, self.world_center(camera))
        } else {
            (
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                self.world_center(camera),
            )
        }
    }

    fn reset_transform(&mut self) {
        self.depth = 18.0;
        self.offset_x = 0.0;
        self.offset_y = 0.0;
        self.scale = 1.25;
        println!("Media transform reset");
    }

    fn apply_strong_defaults(&mut self) {
        if !self.files.is_empty() {
            self.mode = MediaMode::Color;
        }
        self.opacity = 1.0;
        self.contrast = 2.0;
        self.scale = 1.25;
        self.depth = 18.0;
        self.offset_x = 0.0;
        self.offset_y = 0.0;
        self.space_mode = MediaSpaceMode::Volume;
        self.preview_mode = MediaPreviewMode::Off;
        self.coupling_mode = MediaCouplingMode::Formed;
        self.show_gizmo = false;
        self.auto_cycle = true;
        self.auto_cycle_interval = 16.0;
        self.auto_cycle_timer = 0.0;
        self.coupling_depth = 24.0;
        self.coupling_extent = 1.12;
        println!("Applied strong rain-formed media defaults");
    }

    fn world_center(&self, camera: &CameraState) -> [f32; 3] {
        if self.lock_to_camera {
            let (right, up, forward) = camera_basis(camera.yaw, camera.pitch);

            [
                camera.position[0]
                    + forward[0] * self.depth
                    + right[0] * self.offset_x
                    + up[0] * self.offset_y,
                camera.position[1]
                    + forward[1] * self.depth
                    + right[1] * self.offset_x
                    + up[1] * self.offset_y,
                camera.position[2]
                    + forward[2] * self.depth
                    + right[2] * self.offset_x
                    + up[2] * self.offset_y,
            ]
        } else {
            [self.offset_x, self.offset_y, self.depth]
        }
    }

    fn adjust_depth(&mut self, amount: f32) {
        self.depth = (self.depth + amount).clamp(4.0, 80.0);
    }

    fn move_plane(&mut self, delta_x: f32, delta_y: f32) {
        self.offset_x = (self.offset_x + delta_x).clamp(-40.0, 40.0);
        self.offset_y = (self.offset_y + delta_y).clamp(-24.0, 24.0);
    }

    fn sample_world(
        &self,
        world_position: [f32; 3],
        camera: &CameraState,
        viewport_aspect: f32,
    ) -> MediaSample {
        let has_coupling_image = self
            .coupling_images
            .get(self.coupling_current_index)
            .is_some_and(Option::is_some)
            || self
                .coupling_images
                .get(self.coupling_previous_index)
                .is_some_and(Option::is_some);

        if self.mode == MediaMode::Off || !has_coupling_image {
            return MediaSample::default();
        }

        let (plane_width, plane_height) = self.coupling_plane_size(viewport_aspect);
        let (axis_x, axis_y, axis_z, plane_center) = self.coupling_basis_and_center(camera);

        let relative = [
            world_position[0] - plane_center[0],
            world_position[1] - plane_center[1],
            world_position[2] - plane_center[2],
        ];

        let local_x = relative[0] * axis_x[0] + relative[1] * axis_x[1] + relative[2] * axis_x[2];
        let local_y = relative[0] * axis_y[0] + relative[1] * axis_y[1] + relative[2] * axis_y[2];
        let local_z = relative[0] * axis_z[0] + relative[1] * axis_z[1] + relative[2] * axis_z[2];

        let image_u = local_x / plane_width.max(0.01) + 0.5;
        let image_v = 0.5 - local_y / plane_height.max(0.01);

        if !(0.0..=1.0).contains(&image_u) || !(0.0..=1.0).contains(&image_v) {
            return MediaSample::default();
        }

        let color = self.sample_filtered_color(image_u, image_v);
        let luminance = color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722;
        let contrasted = ((luminance - 0.5) * (self.contrast * 0.72) + 0.5).clamp(0.0, 1.0);
        let softened = mix(luminance, contrasted, 0.55);
        let mask = smoothstep(0.10, 0.72, softened);

        let depth_range = 5.6 * self.coupling_mode.depth_scale();
        let weight = (1.0 - smoothstep(0.0, depth_range, local_z.abs())).clamp(0.0, 1.0);

        MediaSample {
            color,
            mask,
            weight,
            carve: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RainMode {
    Quiet,
    Classic,
    Surge,
    Dream,
    Amber,
    RedAlert,
    Ultraviolet,
    Ghost,
    Monochrome,
}

#[derive(Clone, Copy, Debug)]
struct ThemeProfile {
    label: &'static str,
    speed_scale: f32,
    glow_strength: f32,
    exposure_bias: f32,
    density_scale: f32,
    core_scale: f32,
    glow_scale: f32,
    head_scale: f32,
    cascade_scale: f32,
    parallax_scale: f32,
    body_color: [f32; 3],
    head_color: [f32; 3],
    cascade_color: [f32; 3],
    glow_color: [f32; 3],
    background_color: [f32; 3],
    vignette_strength: f32,
    near_bloom: f32,
    wide_bloom: f32,
    history_retention: f32,
    history_deposit: f32,
}

const PALETTE_NAMES: [&str; 6] = ["theme", "cyberpunk", "vaporwave", "ice", "ember", "rainbow"];

fn normalize_palette_name(name: &str) -> Option<&'static str> {
    match name.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "theme" | "native" | "default" => Some("theme"),
        "cyberpunk" | "cyber" => Some("cyberpunk"),
        "vaporwave" | "vapor" => Some("vaporwave"),
        "ice" | "frost" => Some("ice"),
        "ember" | "fire" => Some("ember"),
        "rainbow" | "spectrum" | "prismatic" => Some("rainbow"),
        _ => None,
    }
}

fn apply_named_palette(profile: &mut ThemeProfile, palette: &str) -> bool {
    match normalize_palette_name(palette) {
        Some("theme") => true,
        Some("cyberpunk") => {
            profile.body_color = [0.02, 1.05, 0.92];
            profile.head_color = [1.16, 1.10, 1.22];
            profile.cascade_color = [1.04, 0.06, 0.92];
            profile.glow_color = [0.08, 0.20, 0.52];
            profile.background_color = [0.00003, 0.00008, 0.00052];
            profile.wide_bloom *= 1.18;
            true
        }
        Some("vaporwave") => {
            profile.body_color = [1.02, 0.12, 0.82];
            profile.head_color = [0.76, 1.12, 1.18];
            profile.cascade_color = [0.18, 0.92, 1.12];
            profile.glow_color = [0.36, 0.04, 0.48];
            profile.background_color = [0.00046, 0.00002, 0.00062];
            profile.wide_bloom *= 1.24;
            true
        }
        Some("ice") => {
            profile.body_color = [0.24, 0.76, 1.06];
            profile.head_color = [0.94, 1.12, 1.18];
            profile.cascade_color = [0.48, 0.94, 1.12];
            profile.glow_color = [0.04, 0.22, 0.40];
            profile.background_color = [0.00002, 0.00016, 0.00042];
            true
        }
        Some("ember") => {
            profile.body_color = [1.12, 0.26, 0.025];
            profile.head_color = [1.22, 0.98, 0.54];
            profile.cascade_color = [1.18, 0.52, 0.06];
            profile.glow_color = [0.46, 0.055, 0.004];
            profile.background_color = [0.00100, 0.00010, 0.00001];
            profile.near_bloom *= 1.08;
            true
        }
        Some("rainbow") => {
            profile.body_color = [0.05, 1.04, 0.34];
            profile.head_color = [0.78, 1.08, 1.18];
            profile.cascade_color = [1.08, 0.10, 0.90];
            profile.glow_color = [0.04, 0.28, 0.52];
            profile.background_color = [0.00014, 0.00004, 0.00042];
            profile.wide_bloom *= 1.28;
            true
        }
        _ => false,
    }
}

fn next_palette_name(current: &str) -> &'static str {
    let normalized = normalize_palette_name(current).unwrap_or("theme");
    let index = PALETTE_NAMES
        .iter()
        .position(|candidate| *candidate == normalized)
        .unwrap_or(0);
    PALETTE_NAMES[(index + 1) % PALETTE_NAMES.len()]
}

impl RainMode {
    fn all() -> [Self; 9] {
        [
            Self::Quiet,
            Self::Classic,
            Self::Surge,
            Self::Dream,
            Self::Amber,
            Self::RedAlert,
            Self::Ultraviolet,
            Self::Ghost,
            Self::Monochrome,
        ]
    }

    fn from_name(name: &str) -> Option<Self> {
        match name
            .trim()
            .to_ascii_lowercase()
            .replace(&['_', ' '][..], "-")
            .as_str()
        {
            "1" | "quiet" => Some(Self::Quiet),
            "2" | "classic" | "matrix" | "classic-matrix" => Some(Self::Classic),
            "3" | "surge" | "green-surge" => Some(Self::Surge),
            "4" | "dream" | "dream-cyan" => Some(Self::Dream),
            "5" | "amber" | "amber-terminal" => Some(Self::Amber),
            "6" | "red" | "red-alert" => Some(Self::RedAlert),
            "7" | "ultraviolet" | "uv" => Some(Self::Ultraviolet),
            "8" | "ghost" => Some(Self::Ghost),
            "9" | "monochrome" | "mono" => Some(Self::Monochrome),
            _ => None,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Classic => "classic",
            Self::Surge => "surge",
            Self::Dream => "dream",
            Self::Amber => "amber",
            Self::RedAlert => "red-alert",
            Self::Ultraviolet => "ultraviolet",
            Self::Ghost => "ghost",
            Self::Monochrome => "monochrome",
        }
    }

    fn from_preset(preset: u8) -> Option<Self> {
        match preset {
            1 => Some(Self::Quiet),
            2 => Some(Self::Classic),
            3 => Some(Self::Surge),
            4 => Some(Self::Dream),
            5 => Some(Self::Amber),
            6 => Some(Self::RedAlert),
            7 => Some(Self::Ultraviolet),
            8 => Some(Self::Ghost),
            9 => Some(Self::Monochrome),
            _ => None,
        }
    }

    fn preset(self) -> u8 {
        match self {
            Self::Quiet => 1,
            Self::Classic => 2,
            Self::Surge => 3,
            Self::Dream => 4,
            Self::Amber => 5,
            Self::RedAlert => 6,
            Self::Ultraviolet => 7,
            Self::Ghost => 8,
            Self::Monochrome => 9,
        }
    }

    fn next(self) -> Self {
        Self::from_preset(self.preset() % 9 + 1).expect("theme preset range is valid")
    }

    fn previous(self) -> Self {
        let preset = if self.preset() == 1 {
            9
        } else {
            self.preset() - 1
        };

        Self::from_preset(preset).expect("theme preset range is valid")
    }

    fn profile(self) -> ThemeProfile {
        match self {
            Self::Quiet => ThemeProfile {
                label: "Quiet",
                speed_scale: 0.20,
                glow_strength: 0.42,
                exposure_bias: 0.72,
                density_scale: 0.68,
                core_scale: 0.82,
                glow_scale: 0.68,
                head_scale: 0.76,
                cascade_scale: 0.58,
                parallax_scale: 0.52,
                body_color: [0.025, 0.82, 0.20],
                head_color: [0.62, 0.92, 0.70],
                cascade_color: [0.24, 0.86, 0.34],
                glow_color: [0.00, 0.30, 0.070],
                background_color: [0.00012, 0.00090, 0.00028],
                vignette_strength: 0.18,
                near_bloom: 0.62,
                wide_bloom: 0.16,
                history_retention: 0.80,
                history_deposit: 0.10,
            },

            Self::Classic => ThemeProfile {
                label: "Classic Matrix",
                speed_scale: 1.00,
                glow_strength: 1.00,
                exposure_bias: 1.00,
                density_scale: 1.00,
                core_scale: 1.00,
                glow_scale: 1.00,
                head_scale: 1.00,
                cascade_scale: 1.00,
                parallax_scale: 1.00,
                body_color: [0.03, 1.00, 0.27],
                head_color: [0.78, 1.00, 0.84],
                cascade_color: [0.40, 1.00, 0.52],
                glow_color: [0.00, 0.38, 0.075],
                background_color: [0.00018, 0.00145, 0.00048],
                vignette_strength: 0.16,
                near_bloom: 0.90,
                wide_bloom: 0.30,
                history_retention: 0.86,
                history_deposit: 0.15,
            },

            Self::Surge => ThemeProfile {
                label: "Green Surge",
                speed_scale: 2.35,
                glow_strength: 2.10,
                exposure_bias: 1.22,
                density_scale: 1.10,
                core_scale: 1.18,
                glow_scale: 1.32,
                head_scale: 1.25,
                cascade_scale: 1.42,
                parallax_scale: 1.18,
                body_color: [0.05, 1.12, 0.23],
                head_color: [1.00, 1.10, 0.88],
                cascade_color: [0.68, 1.12, 0.42],
                glow_color: [0.015, 0.46, 0.085],
                background_color: [0.00020, 0.00165, 0.00040],
                vignette_strength: 0.13,
                near_bloom: 1.06,
                wide_bloom: 0.38,
                history_retention: 0.84,
                history_deposit: 0.20,
            },

            Self::Dream => ThemeProfile {
                label: "Dream Cyan",
                speed_scale: 0.07,
                glow_strength: 1.70,
                exposure_bias: 1.08,
                density_scale: 0.82,
                core_scale: 0.92,
                glow_scale: 1.24,
                head_scale: 0.92,
                cascade_scale: 0.78,
                parallax_scale: 0.72,
                body_color: [0.02, 0.70, 0.92],
                head_color: [0.70, 1.00, 1.08],
                cascade_color: [0.24, 0.88, 1.12],
                glow_color: [0.00, 0.30, 0.24],
                background_color: [0.00008, 0.00060, 0.00120],
                vignette_strength: 0.12,
                near_bloom: 0.82,
                wide_bloom: 0.42,
                history_retention: 0.89,
                history_deposit: 0.12,
            },

            Self::Amber => ThemeProfile {
                label: "Amber Terminal",
                speed_scale: 0.72,
                glow_strength: 0.88,
                exposure_bias: 0.94,
                density_scale: 0.88,
                core_scale: 1.02,
                glow_scale: 0.88,
                head_scale: 1.02,
                cascade_scale: 0.82,
                parallax_scale: 0.62,
                body_color: [1.00, 0.48, 0.045],
                head_color: [1.20, 0.92, 0.46],
                cascade_color: [1.10, 0.64, 0.10],
                glow_color: [0.38, 0.12, 0.004],
                background_color: [0.00115, 0.00042, 0.000035],
                vignette_strength: 0.20,
                near_bloom: 0.76,
                wide_bloom: 0.20,
                history_retention: 0.82,
                history_deposit: 0.11,
            },

            Self::RedAlert => ThemeProfile {
                label: "Red Alert",
                speed_scale: 1.65,
                glow_strength: 1.38,
                exposure_bias: 0.92,
                density_scale: 0.72,
                core_scale: 1.10,
                glow_scale: 1.15,
                head_scale: 1.36,
                cascade_scale: 1.58,
                parallax_scale: 1.10,
                body_color: [1.00, 0.035, 0.018],
                head_color: [1.30, 0.42, 0.16],
                cascade_color: [1.18, 0.11, 0.025],
                glow_color: [0.42, 0.012, 0.004],
                background_color: [0.00105, 0.000025, 0.000012],
                vignette_strength: 0.24,
                near_bloom: 0.92,
                wide_bloom: 0.24,
                history_retention: 0.78,
                history_deposit: 0.18,
            },

            Self::Ultraviolet => ThemeProfile {
                label: "Ultraviolet",
                speed_scale: 1.12,
                glow_strength: 1.58,
                exposure_bias: 1.02,
                density_scale: 0.90,
                core_scale: 1.00,
                glow_scale: 1.30,
                head_scale: 1.08,
                cascade_scale: 1.18,
                parallax_scale: 0.94,
                body_color: [0.58, 0.055, 1.02],
                head_color: [0.96, 0.76, 1.22],
                cascade_color: [0.04, 0.82, 1.12],
                glow_color: [0.24, 0.018, 0.42],
                background_color: [0.00032, 0.000018, 0.00100],
                vignette_strength: 0.16,
                near_bloom: 0.90,
                wide_bloom: 0.36,
                history_retention: 0.87,
                history_deposit: 0.15,
            },

            Self::Ghost => ThemeProfile {
                label: "Ghost",
                speed_scale: 0.38,
                glow_strength: 1.22,
                exposure_bias: 0.88,
                density_scale: 0.62,
                core_scale: 0.88,
                glow_scale: 1.28,
                head_scale: 0.72,
                cascade_scale: 0.56,
                parallax_scale: 0.48,
                body_color: [0.55, 0.82, 0.72],
                head_color: [0.96, 1.08, 1.02],
                cascade_color: [0.48, 0.78, 0.70],
                glow_color: [0.10, 0.26, 0.21],
                background_color: [0.00020, 0.00054, 0.00044],
                vignette_strength: 0.12,
                near_bloom: 0.70,
                wide_bloom: 0.48,
                history_retention: 0.92,
                history_deposit: 0.09,
            },

            Self::Monochrome => ThemeProfile {
                label: "Monochrome",
                speed_scale: 0.92,
                glow_strength: 0.92,
                exposure_bias: 0.86,
                density_scale: 0.92,
                core_scale: 0.96,
                glow_scale: 0.90,
                head_scale: 1.00,
                cascade_scale: 0.90,
                parallax_scale: 0.76,
                body_color: [0.72, 0.72, 0.72],
                head_color: [1.12, 1.12, 1.12],
                cascade_color: [0.90, 0.90, 0.90],
                glow_color: [0.22, 0.22, 0.22],
                background_color: [0.00034, 0.00034, 0.00034],
                vignette_strength: 0.20,
                near_bloom: 0.72,
                wide_bloom: 0.18,
                history_retention: 0.82,
                history_deposit: 0.10,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ThemeRuntime {
    speed_scale: f32,
    glow_strength: f32,
    exposure_bias: f32,
    density_scale: f32,
    core_scale: f32,
    glow_scale: f32,
    head_scale: f32,
    cascade_scale: f32,
    parallax_scale: f32,
    body_color: [f32; 3],
    head_color: [f32; 3],
    cascade_color: [f32; 3],
    glow_color: [f32; 3],
    background_color: [f32; 3],
    vignette_strength: f32,
    near_bloom: f32,
    wide_bloom: f32,
    history_retention: f32,
    history_deposit: f32,
}

impl ThemeRuntime {
    fn from_profile(profile: ThemeProfile) -> Self {
        Self {
            speed_scale: profile.speed_scale,
            glow_strength: profile.glow_strength,
            exposure_bias: profile.exposure_bias,
            density_scale: profile.density_scale,
            core_scale: profile.core_scale,
            glow_scale: profile.glow_scale,
            head_scale: profile.head_scale,
            cascade_scale: profile.cascade_scale,
            parallax_scale: profile.parallax_scale,
            body_color: profile.body_color,
            head_color: profile.head_color,
            cascade_color: profile.cascade_color,
            glow_color: profile.glow_color,
            background_color: profile.background_color,
            vignette_strength: profile.vignette_strength,
            near_bloom: profile.near_bloom,
            wide_bloom: profile.wide_bloom,
            history_retention: profile.history_retention,
            history_deposit: profile.history_deposit,
        }
    }

    fn approach(&mut self, target: ThemeProfile, amount: f32) {
        self.speed_scale = mix(self.speed_scale, target.speed_scale, amount);
        self.glow_strength = mix(self.glow_strength, target.glow_strength, amount);
        self.exposure_bias = mix(self.exposure_bias, target.exposure_bias, amount);
        self.density_scale = mix(self.density_scale, target.density_scale, amount);
        self.core_scale = mix(self.core_scale, target.core_scale, amount);
        self.glow_scale = mix(self.glow_scale, target.glow_scale, amount);
        self.head_scale = mix(self.head_scale, target.head_scale, amount);
        self.cascade_scale = mix(self.cascade_scale, target.cascade_scale, amount);
        self.parallax_scale = mix(self.parallax_scale, target.parallax_scale, amount);
        self.vignette_strength = mix(self.vignette_strength, target.vignette_strength, amount);

        for channel in 0..3 {
            self.body_color[channel] =
                mix(self.body_color[channel], target.body_color[channel], amount);
            self.head_color[channel] =
                mix(self.head_color[channel], target.head_color[channel], amount);
            self.cascade_color[channel] = mix(
                self.cascade_color[channel],
                target.cascade_color[channel],
                amount,
            );
            self.glow_color[channel] =
                mix(self.glow_color[channel], target.glow_color[channel], amount);
            self.background_color[channel] = mix(
                self.background_color[channel],
                target.background_color[channel],
                amount,
            );
        }

        self.near_bloom = mix(self.near_bloom, target.near_bloom, amount);
        self.wide_bloom = mix(self.wide_bloom, target.wide_bloom, amount);
        self.history_retention = mix(self.history_retention, target.history_retention, amount);
        self.history_deposit = mix(self.history_deposit, target.history_deposit, amount);
    }

    fn bloom_settings(self) -> BloomSettings {
        BloomSettings {
            near_strength: self.near_bloom,
            wide_strength: self.wide_bloom,
            history_retention: self.history_retention,
            history_deposit: self.history_deposit,
            background_color: [
                self.background_color[0],
                self.background_color[1],
                self.background_color[2],
                self.vignette_strength,
            ],
        }
    }
}

#[derive(Clone, Debug)]
struct ApparitionImage {
    name: String,
    width: u32,
    height: u32,
    pixels: Vec<[f32; 3]>,
}

#[derive(Clone, Debug)]
struct MediaApparition {
    image: ApparitionImage,
    position: [f32; 3],
    velocity: [f32; 3],
    scale: f32,
    opacity: f32,
    lifetime: f32,
    age: f32,
    sway_phase: f32,
    sway_amount: f32,
    seed: u32,
    color_mix: f32,
    billboard: bool,
}

impl MediaApparition {
    fn fade_amount(&self) -> f32 {
        let fade_in = smoothstep(0.0, 1.15, self.age);
        let fade_out = 1.0 - smoothstep(self.lifetime - 2.1, self.lifetime, self.age);
        (fade_in * fade_out).clamp(0.0, 1.0)
    }
}

#[derive(Debug)]
struct ApparitionSystem {
    enabled: bool,
    frequency: f32,
    opacity: f32,
    scale: f32,
    max_count: usize,
    spawn_accumulator: f32,
    serial: u32,
}

impl Default for ApparitionSystem {
    fn default() -> Self {
        Self {
            enabled: true,
            frequency: 0.065,
            opacity: 0.34,
            scale: 1.0,
            max_count: 2,
            spawn_accumulator: 0.0,
            serial: 0,
        }
    }
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
    signal_inspector: SignalInspector,
    help_overlay: HelpOverlay,

    simulation: Simulation,
    last_frame: Instant,

    // Pause-aware clock for camera language and depth parallax.
    motion_time: f32,
    cinematic_director: CinematicDirector,
    music: MusicReactor,

    camera: CameraState,
    camera_input: CameraInput,
    media: MediaField,
    apparition_system: ApparitionSystem,
    apparitions: Vec<MediaApparition>,

    paused: bool,
    mode: RainMode,
    palette_name: String,
    theme: ThemeRuntime,
    target_theme: ThemeProfile,
    state_path: PathBuf,
    remember_preferences: bool,

    exposure: f32,
    target_exposure: f32,

    glyph_instances: Vec<GlyphInstance>,
    glyph_instance_count: u32,
    media_affected_glyphs: u32,
    media_rain_glyphs: u32,

    stats_elapsed: f32,
    stats_frames: u32,
    stats_worst_ms: f32,

    // These fields keep the GPU resources alive.
    _glyph_texture: wgpu::Texture,
    _glyph_texture_view: wgpu::TextureView,
    _glyph_sampler: wgpu::Sampler,
}

impl State {
    async fn new(
        display: OwnedDisplayHandle,
        window: Arc<Window>,
        media_path: Option<PathBuf>,
        launch: &LaunchOptions,
    ) -> Self {
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
        let signal_inspector = SignalInspector::new(
            &device,
            &queue,
            surface_format.add_srgb_suffix(),
            size.width,
            size.height,
            window.scale_factor(),
        );
        let help_overlay = HelpOverlay::new(
            &device,
            &queue,
            surface_format.add_srgb_suffix(),
            size.width,
            size.height,
            window.scale_factor(),
        );

        let simulation = Simulation::new(size.width, size.height);
        let media = MediaField::from_path(media_path);
        let now = Instant::now();

        let initial_mode =
            RainMode::from_name(&launch.preferences.theme).unwrap_or(RainMode::Classic);
        let palette_name = normalize_palette_name(&launch.preferences.palette)
            .unwrap_or("theme")
            .to_owned();
        let mut initial_theme = initial_mode.profile();
        apply_named_palette(&mut initial_theme, &palette_name);

        let mut camera = CameraState::default();
        camera.auto_flight = AutoFlightMode::from_name(&launch.preferences.auto_flight)
            .unwrap_or(AutoFlightMode::Forward);

        let mut cinematic_director = CinematicDirector::default();
        cinematic_director.enabled = launch.preferences.cinematic;

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
            signal_inspector,
            help_overlay,

            simulation,
            last_frame: now,

            motion_time: 0.0,
            cinematic_director,
            music: MusicReactor::new(),

            camera,
            camera_input: CameraInput::default(),
            media,
            apparition_system: ApparitionSystem::default(),
            apparitions: Vec::new(),

            paused: false,
            mode: initial_mode,
            palette_name,
            theme: ThemeRuntime::from_profile(initial_theme),
            target_theme: initial_theme,
            state_path: launch.state_path.clone(),
            remember_preferences: launch.preferences.remember,

            exposure: 1.0,
            target_exposure: 1.0,

            glyph_instances: Vec::with_capacity(MAX_GLYPH_INSTANCES),
            glyph_instance_count: 0,
            media_affected_glyphs: 0,
            media_rain_glyphs: 0,

            stats_elapsed: 0.0,
            stats_frames: 0,
            stats_worst_ms: 0.0,

            _glyph_texture: glyph_texture,
            _glyph_texture_view: glyph_texture_view,
            _glyph_sampler: glyph_sampler,
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
        self.signal_inspector.resize(
            &self.queue,
            new_size.width,
            new_size.height,
            self.window.scale_factor(),
        );
        self.help_overlay.resize(
            &self.queue,
            new_size.width,
            new_size.height,
            self.window.scale_factor(),
        );
        self.configure_surface();
    }

    fn print_controls(&self) {
        println!(
            "theme={}  palette={}  paused={}  rain_speed={:.2}  glow={:.2}  exposure={:.2}  camera=({:.1}, {:.1}, {:.1})  yaw={:.1} pitch={:.1} fov={:.1} move_speed={:.1} flight={} director={} look={} reticle={} media={} opacity={:.2} contrast={:.2} scale={:.2} depth={:.1} offset=({:+.1}, {:+.1}) lock={} guide={} preview={} coupling={} cycle={} {:.1}s affected={}/{} apparitions={} count={} freq={:.2} rainmusic:d{:.2} e{:.2} g{:.2}",
            self.mode.profile().label,
            self.palette_name,
            self.paused,
            self.theme.speed_scale,
            self.theme.glow_strength,
            self.exposure,
            self.camera.position[0],
            self.camera.position[1],
            self.camera.position[2],
            self.camera.yaw.to_degrees(),
            self.camera.pitch.to_degrees(),
            self.camera.fov_y,
            self.camera.movement_speed,
            self.camera.auto_flight.label(),
            if self.cinematic_director.enabled {
                "on"
            } else {
                "off"
            },
            self.camera.mouse_look,
            self.camera.show_reticle,
            self.media.title_label(),
            self.media.opacity,
            self.media.contrast,
            self.media.scale,
            self.media.depth,
            self.media.offset_x,
            self.media.offset_y,
            if self.media.lock_to_camera {
                "camera"
            } else {
                "world"
            },
            self.media.show_gizmo,
            self.media.preview_mode.label(),
            self.media.coupling_mode.label(),
            if self.media.auto_cycle { "on" } else { "off" },
            self.media.auto_cycle_interval,
            self.media_affected_glyphs,
            self.media_rain_glyphs,
            if self.apparition_system.enabled {
                "on"
            } else {
                "off"
            },
            self.apparitions.len(),
            self.apparition_system.frequency,
            self.music.rain_density_multiplier(),
            self.music.rain_energy_multiplier(),
            self.music.glyph_variation_amount(),
        );
        println!("{}", self.music.status_label());
    }

    fn apply_preset(&mut self, preset: u8) {
        let Some(mode) = RainMode::from_preset(preset) else {
            return;
        };

        self.apply_theme(mode);
    }

    fn apply_theme(&mut self, mode: RainMode) {
        self.mode = mode;
        let mut profile = mode.profile();
        apply_named_palette(&mut profile, &self.palette_name);
        self.target_theme = profile;

        println!(
            "Transitioning to theme {}: {} with palette {}",
            mode.preset(),
            self.target_theme.label,
            self.palette_name,
        );

        self.print_controls();
    }

    fn set_palette(&mut self, palette: &str) {
        let Some(normalized) = normalize_palette_name(palette) else {
            eprintln!("Unknown palette: {palette}");
            return;
        };

        self.palette_name = normalized.to_owned();
        let mut profile = self.mode.profile();
        apply_named_palette(&mut profile, &self.palette_name);
        self.target_theme = profile;
        self.bloom.invalidate_history();

        println!("Palette: {}", self.palette_name);
        self.print_controls();
    }

    fn cycle_palette(&mut self) {
        let palette = next_palette_name(&self.palette_name);
        self.set_palette(palette);
    }

    fn save_preferences(&self) {
        if !self.remember_preferences {
            return;
        }

        let preferences = Preferences {
            theme: self.mode.slug().to_owned(),
            palette: self.palette_name.clone(),
            fullscreen: self.window.fullscreen().is_some(),
            window_width: self.size.width.max(1),
            window_height: self.size.height.max(1),
            auto_flight: self.camera.auto_flight.label().to_owned(),
            cinematic: self.cinematic_director.enabled,
            media_enabled: self.media.source_root.is_some(),
            media_path: self.media.source_root.clone(),
            remember: self.remember_preferences,
        };

        if let Err(error) = settings::save_session(&self.state_path, &preferences) {
            eprintln!(
                "Could not save remembered session settings to {}: {error}",
                self.state_path.display(),
            );
        } else {
            println!("Remembered session settings: {}", self.state_path.display(),);
        }
    }

    fn cycle_media_mode(&mut self) {
        self.media.cycle_mode();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn next_media_image(&mut self) {
        self.media.next_image();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn previous_media_image(&mut self) {
        self.media.previous_image();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn reload_media(&mut self) {
        self.media.reload();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn adjust_media_opacity(&mut self, amount: f32) {
        self.media.opacity = (self.media.opacity + amount).clamp(0.0, 1.0);
        self.print_controls();
    }

    fn adjust_media_contrast(&mut self, amount: f32) {
        self.media.contrast = (self.media.contrast + amount).clamp(0.25, 4.0);
        self.print_controls();
    }

    fn adjust_media_scale(&mut self, amount: f32) {
        self.media.scale = (self.media.scale + amount).clamp(0.25, 3.0);
        self.print_controls();
    }

    fn adjust_media_depth(&mut self, amount: f32) {
        self.media.adjust_depth(amount);
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn move_media_plane(&mut self, delta_x: f32, delta_y: f32) {
        self.media.move_plane(delta_x, delta_y);
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn cycle_media_space_mode(&mut self) {
        self.media.cycle_space_mode();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn toggle_media_space_lock(&mut self) {
        self.media.toggle_space_lock();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn toggle_media_gizmo(&mut self) {
        self.media.toggle_gizmo();
        self.print_controls();
    }

    fn cycle_media_preview_mode(&mut self) {
        self.media.cycle_preview_mode();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn cycle_media_coupling_mode(&mut self) {
        self.media.cycle_coupling_mode();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn toggle_media_auto_cycle(&mut self) {
        self.media.toggle_auto_cycle();
        self.print_controls();
    }

    fn adjust_media_auto_cycle_interval(&mut self, amount: f32) {
        self.media.adjust_auto_cycle_interval(amount * 2.0);
        self.print_controls();
    }

    fn toggle_apparitions(&mut self) {
        self.apparition_system.enabled = !self.apparition_system.enabled;
        println!(
            "Ambient media apparitions: {}",
            if self.apparition_system.enabled {
                "on"
            } else {
                "off"
            },
        );
        self.print_controls();
    }

    fn adjust_apparition_frequency(&mut self, amount: f32) {
        self.apparition_system.frequency =
            (self.apparition_system.frequency + amount).clamp(0.02, 0.45);
        self.print_controls();
    }

    fn adjust_apparition_opacity(&mut self, amount: f32) {
        self.apparition_system.opacity =
            (self.apparition_system.opacity + amount).clamp(0.08, 0.85);
        self.print_controls();
    }

    fn spawn_media_apparition(&mut self) {
        if !self.apparition_system.enabled
            || self.media.files.is_empty()
            || self.apparitions.len() >= self.apparition_system.max_count
        {
            return;
        }

        self.apparition_system.serial = self.apparition_system.serial.wrapping_add(1);
        let seed = self.apparition_system.serial;
        let random_image_index = ((stable_unit(seed ^ 0x31af_2017) * self.media.files.len() as f32)
            as usize)
            .min(self.media.files.len().saturating_sub(1));
        let image_index = self
            .music
            .image_match_target()
            .and_then(|target| {
                self.media
                    .select_music_matched_index(target, seed ^ 0x4252_4954, None)
            })
            .unwrap_or(random_image_index);
        let Some(cached_image) = self
            .media
            .coupling_images
            .get(image_index)
            .and_then(Option::as_ref)
            .cloned()
        else {
            return;
        };

        let CouplingImage {
            name,
            width,
            height,
            pixels,
            signature: _,
        } = cached_image;
        let call_balance = self.music.call_response_balance();
        let x = mix(-16.0, 16.0, stable_unit(seed ^ 0x51c3_a729)) + call_balance * 4.2;
        let y = mix(8.5, 18.0, stable_unit(seed ^ 0x07ac_e52d));
        let z = self.camera.position[2] + mix(26.0, 70.0, stable_unit(seed ^ 0xd4eb_2f15));
        let drift_x = mix(-0.22, 0.22, stable_unit(seed ^ 0x82f6_3b79));
        let drift_z = mix(-0.08, 0.12, stable_unit(seed ^ 0x1656_67b1));
        let fall_speed = mix(-1.6, -0.45, stable_unit(seed ^ 0xc2b2_ae35));
        let lifetime = mix(6.0, 10.5, stable_unit(seed ^ 0x27d4_eb2f));
        let scale = self.apparition_system.scale * mix(0.52, 0.98, stable_unit(seed ^ 0x9e37_79b9));
        let opacity =
            self.apparition_system.opacity * mix(0.72, 1.18, stable_unit(seed ^ 0x94d0_49bb));
        let sway_phase = stable_unit(seed ^ 0x68bc_21eb) * std::f32::consts::TAU;
        let sway_amount = mix(0.18, 0.95, stable_unit(seed ^ 0x4f1b_bcdc));
        let color_mix = mix(0.90, 1.0, stable_unit(seed ^ 0x6a09_e667));

        self.apparitions.push(MediaApparition {
            image: ApparitionImage {
                name,
                width,
                height,
                pixels,
            },
            position: [x, y, z],
            velocity: [drift_x, fall_speed, drift_z],
            scale,
            opacity,
            lifetime,
            age: 0.0,
            sway_phase,
            sway_amount,
            seed,
            color_mix,
            billboard: true,
        });
    }

    fn update_apparitions(&mut self, dt: f32) {
        if dt > 0.0 && self.apparition_system.enabled {
            if self.apparitions.len() >= self.apparition_system.max_count {
                self.apparition_system.spawn_accumulator =
                    self.apparition_system.spawn_accumulator.min(0.95);
            } else {
                self.apparition_system.spawn_accumulator += dt
                    * self.apparition_system.frequency
                    * self.music.apparition_frequency_multiplier();

                while self.apparition_system.spawn_accumulator >= 1.0
                    && self.apparitions.len() < self.apparition_system.max_count
                {
                    self.apparition_system.spawn_accumulator -= 1.0;
                    self.spawn_media_apparition();
                }
            }
        }

        let motion_time = self.motion_time;
        let camera_z = self.camera.position[2];
        self.apparitions.retain_mut(|apparition| {
            apparition.age += dt;
            apparition.position[0] += apparition.velocity[0] * dt;
            apparition.position[1] += apparition.velocity[1] * dt;
            apparition.position[2] += apparition.velocity[2] * dt;
            apparition.position[0] += (motion_time * 0.38 + apparition.sway_phase).sin()
                * apparition.sway_amount
                * 0.045
                * dt;

            apparition.age < apparition.lifetime
                && apparition.position[1] > -16.0
                && apparition.position[2] > camera_z + 5.5
                && apparition.position[2] < camera_z + 105.0
        });
    }

    fn reset_media_transform(&mut self) {
        self.media.reset_transform();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn apply_strong_media_defaults(&mut self) {
        self.media.apply_strong_defaults();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn focus_media_plane(&mut self) {
        let center = self.media.world_center(&self.camera);
        let delta = [
            center[0] - self.camera.position[0],
            center[1] - self.camera.position[1],
            center[2] - self.camera.position[2],
        ];

        let horizontal = (delta[0] * delta[0] + delta[2] * delta[2]).sqrt();

        if horizontal < 0.0001 && delta[1].abs() < 0.0001 {
            return;
        }

        self.camera.auto_flight = AutoFlightMode::Off;
        self.camera.velocity = [0.0; 3];
        self.camera.target_yaw = delta[0].atan2(delta[2]);
        self.camera.target_pitch = delta[1].atan2(horizontal.max(0.0001)).clamp(-1.48, 1.48);
        self.bloom.invalidate_history();

        println!(
            "Focused media plane at ({:.1}, {:.1}, {:.1})",
            center[0], center[1], center[2],
        );
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

    fn set_mouse_look(&mut self, enabled: bool) {
        if enabled == self.camera.mouse_look {
            return;
        }

        if enabled {
            let grab_result = self
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));

            if let Err(error) = grab_result {
                eprintln!("Could not capture the mouse: {error}");
                return;
            }

            self.window.set_cursor_visible(false);
            self.camera.mouse_look = true;
        } else {
            if let Err(error) = self.window.set_cursor_grab(CursorGrabMode::None) {
                eprintln!("Could not release the mouse: {error}");
            }

            self.window.set_cursor_visible(true);
            self.camera.mouse_look = false;
        }

        self.print_controls();
    }

    fn toggle_mouse_look(&mut self) {
        self.set_mouse_look(!self.camera.mouse_look);
    }

    fn handle_mouse_motion(&mut self, delta_x: f64, delta_y: f64) {
        if !self.camera.mouse_look || self.paused {
            return;
        }

        let sensitivity = 0.0022;

        self.camera.target_yaw =
            (self.camera.target_yaw + delta_x as f32 * sensitivity).clamp(-0.62, 0.62);

        self.camera.target_pitch =
            (self.camera.target_pitch - delta_y as f32 * sensitivity).clamp(-0.38, 0.38);
    }

    fn reset_camera(&mut self) {
        let mouse_look = self.camera.mouse_look;
        let show_reticle = self.camera.show_reticle;
        let movement_speed = self.camera.movement_speed;

        self.camera = CameraState {
            mouse_look,
            show_reticle,
            movement_speed,
            ..CameraState::default()
        };

        self.camera_input = CameraInput::default();
        self.bloom.invalidate_history();

        println!("Reset world-space camera");
        self.print_controls();
    }

    fn adjust_zoom(&mut self, scroll_lines: f32) {
        self.camera.target_fov_y = (self.camera.target_fov_y - scroll_lines * 2.5)
            .clamp(CAMERA_MIN_FOV_Y, CAMERA_MAX_FOV_Y);
        self.cinematic_director.timer = 0.0;
        self.cinematic_director.next_change = self.cinematic_director.next_change.max(10.0);

        self.print_controls();
    }

    fn adjust_camera_speed(&mut self, amount: f32) {
        self.camera.movement_speed = (self.camera.movement_speed + amount).clamp(1.0, 32.0);
        self.print_controls();
    }

    fn toggle_cinematic_director(&mut self) {
        self.cinematic_director.enabled = !self.cinematic_director.enabled;
        self.cinematic_director.timer = 0.0;
        self.cinematic_director.next_change = 8.0;

        println!(
            "Cinematic movement director: {}",
            if self.cinematic_director.enabled {
                "on"
            } else {
                "off"
            },
        );
        self.print_controls();
    }

    fn toggle_music_reactivity(&mut self) {
        self.music.toggle();
        self.print_controls();
    }

    fn cycle_music_intensity(&mut self) {
        self.music.cycle_intensity();
        self.print_controls();
    }

    fn cycle_music_source(&mut self) {
        self.music.cycle_source();
        self.print_controls();
    }

    fn cycle_music_color_mode(&mut self) {
        self.music.cycle_color_mode();
        self.bloom.invalidate_history();
        self.print_controls();
    }

    fn update_cinematic_director(&mut self, dt: f32) {
        if !self.cinematic_director.enabled || dt <= 0.0 {
            return;
        }

        self.cinematic_director.timer += dt * self.music.cinematic_tempo_multiplier();
        if self.cinematic_director.timer < self.cinematic_director.next_change {
            return;
        }

        let phrase_ready = self.music.cinematic_phrase_ready();
        let holdoff_window = if self.music.active() { 3.6 } else { 0.0 };
        if !phrase_ready
            && self.music.active()
            && self.cinematic_director.timer < self.cinematic_director.next_change + holdoff_window
        {
            return;
        }

        self.cinematic_director.timer = 0.0;
        self.cinematic_director.serial = self.cinematic_director.serial.wrapping_add(1);
        let seed = self.cinematic_director.serial;
        let movement_choice = stable_unit(seed ^ 0x2c1b_3c6d);

        self.camera.auto_flight = if movement_choice < 0.28 {
            AutoFlightMode::Forward
        } else if movement_choice < 0.58 {
            AutoFlightMode::Weave
        } else if movement_choice < 0.76 {
            AutoFlightMode::Orbit
        } else {
            AutoFlightMode::Tunnel
        };

        self.cinematic_director.lateral_transition_duration = match self.camera.auto_flight {
            AutoFlightMode::Forward => 5.8,
            AutoFlightMode::Weave => 7.2,
            AutoFlightMode::Orbit => 8.6,
            AutoFlightMode::Tunnel => 7.8,
            AutoFlightMode::Off => 5.8,
        };

        self.camera.target_fov_y = match self.camera.auto_flight {
            AutoFlightMode::Forward => mix(50.0, 66.0, stable_unit(seed ^ 0x91e1_0da5)),
            AutoFlightMode::Weave => mix(48.0, 70.0, stable_unit(seed ^ 0x91e1_0da5)),
            AutoFlightMode::Orbit => mix(54.0, 74.0, stable_unit(seed ^ 0x91e1_0da5)),
            AutoFlightMode::Tunnel => mix(42.0, 60.0, stable_unit(seed ^ 0x91e1_0da5)),
            AutoFlightMode::Off => 60.0,
        }
        .clamp(CAMERA_MIN_FOV_Y, CAMERA_MAX_FOV_Y);

        self.cinematic_director.next_change = {
            let interval_choice = stable_unit(seed ^ 0x7f4a_7c15);
            if interval_choice < 0.24 {
                8.0
            } else if interval_choice < 0.54 {
                12.0
            } else if interval_choice < 0.82 {
                16.0
            } else {
                20.0
            }
        };

        self.bloom.invalidate_history();
        println!(
            "Cinematic change: flight={} target_fov={:.1} next={:.1}s timing={}",
            self.camera.auto_flight.label(),
            self.camera.target_fov_y,
            self.cinematic_director.next_change,
            if phrase_ready { "phrase" } else { "fallback" },
        );
    }

    fn cycle_auto_flight(&mut self) {
        self.camera.auto_flight = self.camera.auto_flight.next();
        self.bloom.invalidate_history();

        println!("Automatic flight: {}", self.camera.auto_flight.label());
        self.print_controls();
    }

    fn toggle_reticle(&mut self) {
        self.camera.show_reticle = !self.camera.show_reticle;
        self.print_controls();
    }

    fn toggle_help_overlay(&mut self) {
        self.camera_input = CameraInput::default();
        self.set_mouse_look(false);
        self.help_overlay.toggle();

        println!(
            "Help overlay: {}",
            if self.help_overlay.is_visible() {
                "open"
            } else {
                "closed"
            },
        );
    }

    fn update_camera(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }

        let input_x: f32 = if self.camera_input.right { 1.0 } else { 0.0 }
            - if self.camera_input.left { 1.0 } else { 0.0 };

        let input_y: f32 = if self.camera_input.up { 1.0 } else { 0.0 }
            - if self.camera_input.down { 1.0 } else { 0.0 };

        let input_z: f32 = if self.camera_input.forward { 1.0 } else { 0.0 }
            - if self.camera_input.backward { 1.0 } else { 0.0 };

        let input_length = (input_x * input_x + input_y * input_y + input_z * input_z)
            .sqrt()
            .max(1.0);

        let speed_modifier = if self.camera_input.boost {
            3.0
        } else if self.camera_input.precision {
            0.24
        } else {
            1.0
        };

        let (right, _, forward) = camera_basis(self.camera.yaw, self.camera.pitch);
        let manual_speed = self.camera.movement_speed * speed_modifier;

        let manual_velocity = [
            (right[0] * input_x + forward[0] * input_z) / input_length * manual_speed,
            (input_y + forward[1] * input_z) / input_length * manual_speed,
            (right[2] * input_x + forward[2] * input_z) / input_length * manual_speed,
        ];

        let base_auto_speed =
            1.65 * self.theme.parallax_scale.max(0.25) * self.music.speed_multiplier();
        let phase = self.motion_time;
        let music_stereo_drift = self.music.stereo_camera_drift();
        let music_camera_velocity = self.music.camera_velocity_coordinates(phase);
        let music_camera_look = self.music.camera_look_coordinates(phase);

        let auto_velocity = match self.camera.auto_flight {
            AutoFlightMode::Off => [0.0, 0.0, 0.0],
            AutoFlightMode::Forward => [0.0, 0.0, base_auto_speed],
            AutoFlightMode::Weave => [
                (phase * 0.34).sin() * 0.58,
                (phase * 0.26 + 0.8).cos() * 0.24,
                base_auto_speed * 1.06,
            ],
            AutoFlightMode::Orbit => {
                let target_x = (phase * 0.145).sin() * 5.0;
                let target_y = (phase * 0.125 + 0.7).cos() * 2.4;

                [
                    (target_x - self.camera.position[0]) * 0.32,
                    (target_y - self.camera.position[1]) * 0.32,
                    base_auto_speed * 0.78,
                ]
            }
            AutoFlightMode::Tunnel => [
                -self.camera.position[0] * 0.62 + (phase * 0.58).sin() * 0.20,
                -self.camera.position[1] * 0.62 + (phase * 0.44).cos() * 0.12,
                base_auto_speed * 2.05,
            ],
        };

        let lateral_transition = smoothstep(
            0.0,
            self.cinematic_director.lateral_transition_duration,
            self.cinematic_director.timer,
        );
        let mut auto_velocity = [
            auto_velocity[0] * lateral_transition + music_stereo_drift + music_camera_velocity[0],
            auto_velocity[1] * lateral_transition + music_camera_velocity[1],
            (auto_velocity[2] + music_camera_velocity[2]).max(0.0),
        ];

        // Lateral stability: keep autonomous/music motion centered without
        // removing the visible sway. A weak spring acts near the middle and
        // progressively strengthens before the camera reaches the edge of the
        // populated rain volume. Manual left/right movement remains unrestricted.
        if input_x.abs() < 0.01 {
            let lateral_distance = self.camera.position[0].abs();
            let edge_pressure = smoothstep(5.5, 13.5, lateral_distance);
            let recenter_strength = mix(0.026, 0.145, edge_pressure);
            auto_velocity[0] -= self.camera.position[0] * recenter_strength;
        }

        if !self.camera.mouse_look {
            let (target_yaw, target_pitch) = match self.camera.auto_flight {
                AutoFlightMode::Off | AutoFlightMode::Forward => (0.0, 0.0),
                AutoFlightMode::Weave => (
                    (phase * 0.19).sin() * 0.10 * lateral_transition,
                    (phase * 0.16 + 0.4).sin() * 0.04 * lateral_transition,
                ),
                AutoFlightMode::Orbit => (
                    (phase * 0.145).cos() * 0.16 * lateral_transition,
                    (phase * 0.125).sin() * 0.06 * lateral_transition,
                ),
                AutoFlightMode::Tunnel => (
                    (phase * 0.31).sin() * 0.09,
                    (phase * 0.27 + 1.1).cos() * 0.05,
                ),
            };

            self.camera.target_yaw = target_yaw + music_camera_look[0];
            self.camera.target_pitch = target_pitch + music_camera_look[1];
        }

        let target_velocity = [
            manual_velocity[0] + auto_velocity[0],
            manual_velocity[1] + auto_velocity[1],
            manual_velocity[2] + auto_velocity[2],
        ];

        for axis in 0..3 {
            let response_rate = if axis < 2 {
                if self.cinematic_director.enabled {
                    0.54
                } else {
                    4.2
                }
            } else if target_velocity[axis].abs() > self.camera.velocity[axis].abs() {
                3.8
            } else {
                2.6
            };
            let velocity_response = 1.0 - (-response_rate * dt).exp();

            self.camera.velocity[axis] +=
                (target_velocity[axis] - self.camera.velocity[axis]) * velocity_response;

            self.camera.position[axis] += self.camera.velocity[axis] * dt;
        }

        let unclamped_camera_x = self.camera.position[0];
        self.camera.position[0] = self.camera.position[0].clamp(-15.0, 15.0);
        self.camera.position[1] = self.camera.position[1].clamp(-11.0, 11.0);

        // Do not allow outward velocity to remain stored against the hard
        // boundary. Without this, the camera can linger at an edge after the
        // driving signal has already changed direction.
        if self.camera.position[0] != unclamped_camera_x
            && self.camera.velocity[0].signum() == unclamped_camera_x.signum()
        {
            self.camera.velocity[0] = 0.0;
        }

        let look_response_rate: f32 = if self.cinematic_director.enabled {
            2.2
        } else {
            13.0
        };
        let look_response = 1.0 - (-look_response_rate * dt).exp();
        self.camera.yaw += (self.camera.target_yaw - self.camera.yaw) * look_response;
        self.camera.pitch += (self.camera.target_pitch - self.camera.pitch) * look_response;

        let fov_response_rate: f32 = if self.cinematic_director.enabled {
            1.65
        } else {
            8.0
        };
        let fov_response = 1.0 - (-fov_response_rate * dt).exp();
        self.camera.fov_y += (self.camera.target_fov_y - self.camera.fov_y) * fov_response;
    }

    fn rebuild_glyph_instances(&mut self) {
        self.glyph_instances.clear();
        self.media_affected_glyphs = 0;
        self.media_rain_glyphs = 0;

        let scale = visual_scale(self.size);

        let width = self.size.width.max(1) as f32;

        let height = self.size.height.max(1) as f32;

        let aspect = (width / height).max(0.25);

        let exposure = self.exposure.max(0.01);

        let glow_control = (self.theme.glow_strength * self.music.glow_multiplier()).max(0.0);

        let mode_core_scale = self.theme.core_scale;

        let mode_glow_scale = self.theme.glow_scale;

        let mode_head_scale = self.theme.head_scale;

        let mode_cascade_scale = self.theme.cascade_scale * self.music.cascade_multiplier();

        let music_color_mode = self.music.color_mode();
        let music_palette_mix = self.music.palette_mix();
        let music_primary_color = self.music.primary_palette_color();
        let music_secondary_color = self.music.secondary_palette_color();
        let music_structure_drive = self.music.structure_drive();
        let music_detail_drive = self.music.detail_drive();
        let music_signature_event = self.music.signature_event_strength();
        let base_palette_mix = match music_color_mode {
            MusicColorMode::Wallpaper => music_palette_mix,
            MusicColorMode::Palette => (music_palette_mix * 0.96).clamp(0.0, 1.0),
            MusicColorMode::Hybrid => (music_palette_mix * 0.74).clamp(0.0, 1.0),
        };

        let mode_body_color = [
            mix(
                self.theme.body_color[0],
                music_primary_color[0],
                base_palette_mix * 0.82,
            ),
            mix(
                self.theme.body_color[1],
                music_primary_color[1],
                base_palette_mix * 0.82,
            ),
            mix(
                self.theme.body_color[2],
                music_primary_color[2],
                base_palette_mix * 0.82,
            ),
        ];

        let mode_head_color = [
            mix(
                self.theme.head_color[0],
                music_secondary_color[0].max(0.68),
                base_palette_mix * 0.72,
            ),
            mix(
                self.theme.head_color[1],
                music_secondary_color[1].max(0.68),
                base_palette_mix * 0.72,
            ),
            mix(
                self.theme.head_color[2],
                music_secondary_color[2].max(0.68),
                base_palette_mix * 0.72,
            ),
        ];

        let mode_cascade_color = [
            mix(
                self.theme.cascade_color[0],
                music_secondary_color[0],
                base_palette_mix * 0.90,
            ),
            mix(
                self.theme.cascade_color[1],
                music_secondary_color[1],
                base_palette_mix * 0.90,
            ),
            mix(
                self.theme.cascade_color[2],
                music_secondary_color[2],
                base_palette_mix * 0.90,
            ),
        ];

        let mode_glow_color = [
            mix(
                self.theme.glow_color[0],
                music_primary_color[0] * 0.56,
                base_palette_mix,
            ),
            mix(
                self.theme.glow_color[1],
                music_primary_color[1] * 0.56,
                base_palette_mix,
            ),
            mix(
                self.theme.glow_color[2],
                music_primary_color[2] * 0.56,
                base_palette_mix,
            ),
        ];

        let music_density_multiplier = self.music.rain_density_multiplier();
        let music_energy_multiplier = self.music.rain_energy_multiplier();
        let music_head_activity = self.music.head_activity_multiplier();
        let music_glyph_variation = self.music.glyph_variation_amount();
        let music_rain_phase = self.music.rain_phase();
        let music_spatial_strength = self.music.spatial_strength();
        let music_field_coordinates = self.music.field_coordinates(self.motion_time);
        let palette_channel_gains = self.music.wallpaper_channel_gains();
        let moodbar_channel_gains = self.music.moodbar_channel_gains();
        let wallpaper_channel_gains = [
            (palette_channel_gains[0] * moodbar_channel_gains[0]).clamp(0.68, 1.48),
            (palette_channel_gains[1] * moodbar_channel_gains[1]).clamp(0.68, 1.48),
            (palette_channel_gains[2] * moodbar_channel_gains[2]).clamp(0.68, 1.48),
        ];
        let image_palette_accent_mix = self.music.image_palette_accent_mix();

        let effective_fov_y = (self.camera.fov_y
            + self.music.fov_offset()
            + self.music.camera_fov_wave(self.motion_time))
        .clamp(CAMERA_MIN_FOV_Y, CAMERA_MAX_FOV_Y);
        let fov_y = effective_fov_y
            .to_radians()
            .clamp(0.10, std::f32::consts::PI - 0.10);

        let tan_half_fov = (fov_y * 0.5).tan().max(0.05);

        let far_half_height = WORLD_FAR_Z * tan_half_fov;

        let far_half_width = far_half_height * aspect;

        // A small camera-local drift retains the organic motion from
        // the layered renderer, but now it moves the viewpoint through
        // the same world rather than shifting each depth plane.
        let autonomous_bob_scale = if self.music.active() { 0.28 } else { 1.0 };
        let camera_bob_x = ((self.motion_time * 0.105).sin()
            + (self.motion_time * 0.041 + 1.7).sin() * 0.42)
            * 0.16
            * self.theme.parallax_scale
            * autonomous_bob_scale;

        let camera_bob_y = ((self.motion_time * 0.083 + 0.9).cos()
            + (self.motion_time * 0.029).sin() * 0.35)
            * 0.10
            * self.theme.parallax_scale
            * autonomous_bob_scale;

        let camera_x = self.camera.position[0] + camera_bob_x;

        let camera_y = self.camera.position[1] + camera_bob_y;

        let camera_z = self.camera.position[2];
        let camera_yaw = self.camera.yaw;
        let camera_pitch = self.camera.pitch;

        for (stream_index, stream) in self.simulation.streams.iter().enumerate() {
            let stream_slot = stream_index as u32;

            // Every persistent stream owns one position in a repeating
            // world-space depth volume. Passing the near plane advances
            // it to the next volume ahead of the camera.
            let base_z = WORLD_NEAR_Z
                + stable_unit(stream_slot.wrapping_mul(0x9e37_79b9) ^ 0x5a5f_574f)
                    * WORLD_DEPTH_SPAN;

            let cycle_index = ((camera_z + WORLD_NEAR_Z - base_z) / WORLD_DEPTH_SPAN).ceil() as i32;

            let world_z = base_z + cycle_index as f32 * WORLD_DEPTH_SPAN;

            let relative_world_z = world_z - camera_z;

            let cycle_seed = stream_slot.wrapping_mul(0x85eb_ca6b)
                ^ (cycle_index as u32).wrapping_mul(0xc2b2_ae35);

            let lane_ndc = (stream.x / width * 2.0 - 1.0).clamp(-1.35, 1.35);

            let cycle_jitter = (stable_unit(cycle_seed ^ 0x584c_414e) * 2.0 - 1.0) * 0.28;

            let base_world_x = (lane_ndc * 0.84 + cycle_jitter) * far_half_width;

            // The depth volume already recycles in Z. Recycle it laterally as
            // well, selecting the nearest periodic copy of every stream around
            // the camera. This makes the Matrix space effectively endless and
            // prevents a long-running camera drift from exposing a finite edge.
            let lateral_tile_span = (far_half_width * 1.68).max(1.0);
            let lateral_tile_index = ((camera_x - base_world_x) / lateral_tile_span).round();
            let world_x = base_world_x + lateral_tile_index * lateral_tile_span;

            let relative_x = world_x - camera_x;

            let reference_camera = world_to_camera(
                [relative_x, -camera_y, relative_world_z],
                camera_yaw,
                camera_pitch,
            );

            let reference_z = reference_camera[2];

            if reference_z < WORLD_NEAR_Z || reference_z > WORLD_FAR_Z + 0.01 {
                continue;
            }

            let depth = (1.0 - (reference_z - WORLD_NEAR_Z) / WORLD_DEPTH_SPAN).clamp(0.0, 1.0);

            let depth_shape = depth.powf(1.35);
            let depth_band = (depth * 4.0).round() as u32;

            let music_field_phase = lane_ndc * 5.6
                + depth * 7.4
                + music_rain_phase * std::f32::consts::TAU * 1.55
                + music_field_coordinates[0] * 1.35
                + music_field_coordinates[2] * 0.85;
            let music_field_primary = 0.5 + 0.5 * music_field_phase.sin();
            let music_field_cross = 0.5
                + 0.5
                    * (lane_ndc * -3.8
                        + depth * 10.2
                        + music_rain_phase * std::f32::consts::TAU * 0.86
                        + music_field_coordinates[1] * 1.65)
                        .cos();
            let music_field = mix(music_field_primary, music_field_cross, 0.38);
            let music_density_field =
                mix(1.0, mix(0.62, 1.34, music_field), music_spatial_strength);
            let music_energy_field = mix(
                1.0,
                mix(0.68, 1.42, music_field_cross),
                music_spatial_strength,
            );

            let near_recycle_fade = smoothstep(WORLD_NEAR_Z, WORLD_NEAR_Z + 1.35, reference_z);

            let far_recycle_fade = 1.0 - smoothstep(WORLD_FAR_Z - 4.5, WORLD_FAR_Z, reference_z);

            let volume_visibility = near_recycle_fade * far_recycle_fade;

            if volume_visibility < 0.005 {
                continue;
            }

            let ndc_x = reference_camera[0] / reference_z / tan_half_fov / aspect;

            let size_variation = mix(0.86, 1.16, stable_unit(cycle_seed ^ 0x5349_5a45));

            let base_glyph_width = 11.5 * scale * size_variation;
            let base_glyph_height = 18.5 * scale * size_variation;

            let perspective_scale = (WORLD_FAR_Z / reference_z).powf(0.60).clamp(0.72, 3.25);

            let glyph_width = base_glyph_width * perspective_scale;

            let glyph_height = base_glyph_height * perspective_scale;

            let margin_ndc_x = glyph_width / width * 2.0;

            if ndc_x + margin_ndc_x < -1.0 || ndc_x - margin_ndc_x > 1.0 {
                continue;
            }

            let density_sample = stable_unit(cycle_seed ^ 0x4445_4e53);

            let base_density_keep = (mix(0.94, 0.30, depth_shape)
                * self.theme.density_scale
                * music_density_multiplier
                * music_density_field)
                .clamp(0.04, 1.0);

            let density_keep = if self.media.mode != MediaMode::Off
                && self.media.coupling_mode != MediaCouplingMode::Influence
            {
                base_density_keep.max(0.92)
            } else {
                base_density_keep
            };

            let density_presence = 1.0
                - smoothstep(
                    (density_keep - 0.08).max(0.0),
                    (density_keep + 0.08).min(1.0),
                    density_sample,
                );

            if density_presence < 0.005 {
                continue;
            }

            let fog_visibility = mix(0.38, 1.0, depth.powf(0.58)) * volume_visibility;

            let atmosphere = mix(0.40, 1.10, depth.powf(0.78)) * fog_visibility;

            let glow_atmosphere =
                mix(0.10, 1.18, depth.powf(1.42)) * mix(0.72, 1.0, fog_visibility);

            let head_probability = (mix(0.02, 0.44, depth.powf(1.32))
                * music_head_activity
                * mix(0.76, 1.28, music_field_primary * music_spatial_strength))
            .clamp(0.0, 0.92);

            let head_depth = mix(0.08, 0.82, depth.powf(1.48));

            let cascade_depth = mix(0.20, 1.10, depth.powf(1.10));

            let head_sample = stable_unit(cycle_seed ^ 0x4845_4144);

            let white_head_present = head_sample < head_probability;

            let anatomy_seed = cycle_seed
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

            let head_ndc_y = 1.0 - stream.head / height * 2.0;

            let stream_music_wave = 0.5
                + 0.5
                    * ((music_rain_phase * std::f32::consts::TAU)
                        + stable_unit(cycle_seed ^ 0x4d55_5349) * std::f32::consts::TAU
                        + depth * 4.8
                        + lane_ndc * 2.4
                        + music_field_coordinates[2] * 1.1)
                        .sin();

            let stream_music_presence =
                mix(0.72, 1.30, stream_music_wave) * music_energy_multiplier * music_energy_field;

            for segment in 0..stream_length {
                if self.glyph_instances.len() >= MAX_RAIN_GLYPH_INSTANCES {
                    break;
                }

                let trail_position =
                    segment as f32 / (stream_length.saturating_sub(1).max(1) as f32);

                let protected_head = segment < 4;

                let in_primary_gap = has_primary_gap
                    && (trail_position - primary_gap_center).abs() < primary_gap_half_width;

                let in_secondary_gap = has_secondary_gap
                    && (trail_position - secondary_gap_center).abs() < secondary_gap_half_width;

                if !protected_head && (in_primary_gap || in_secondary_gap) {
                    continue;
                }

                let segment_ndc_offset = segment as f32 * base_glyph_height / height * 2.0;

                let world_y = (head_ndc_y + segment_ndc_offset) * far_half_height;

                let relative_y = world_y - camera_y;

                let glyph_camera = world_to_camera(
                    [relative_x, relative_y, relative_world_z],
                    camera_yaw,
                    camera_pitch,
                );

                if glyph_camera[2] < WORLD_NEAR_Z || glyph_camera[2] > WORLD_FAR_Z {
                    continue;
                }

                let glyph_ndc_x = glyph_camera[0] / glyph_camera[2] / tan_half_fov / aspect;
                let ndc_y = glyph_camera[1] / glyph_camera[2] / tan_half_fov;

                let center_x = (glyph_ndc_x * 0.5 + 0.5) * width;
                let center_y = (0.5 - ndc_y * 0.5) * height;

                let call_response = self.music.call_response_at(glyph_ndc_x, depth);
                let response_energy = mix(0.84, 1.20, call_response);
                let signature_visibility_boost =
                    1.0 + music_signature_event * (0.18 + depth * 0.18);

                let margin_x = glyph_width * 0.70;

                let margin_y = glyph_height * 0.70;

                if center_x + margin_x < 0.0
                    || center_x - margin_x > width
                    || center_y + margin_y < 0.0
                    || center_y - margin_y > height
                {
                    continue;
                }

                let media_sample = self.media.sample_world(
                    [
                        world_x + music_field_coordinates[0] * music_spatial_strength * 0.42,
                        world_y + music_field_coordinates[1] * music_spatial_strength * 0.28,
                        world_z + music_field_coordinates[2] * music_spatial_strength * 0.34,
                    ],
                    &self.camera,
                    aspect,
                );

                let media_amount = if self.media.mode == MediaMode::Off {
                    0.0
                } else {
                    (self.media.opacity
                        * self.music.coupling_multiplier()
                        * (1.0 + music_signature_event * 0.22))
                        .clamp(0.0, 1.35)
                };

                let media_weighted_mask = media_sample.mask * media_sample.weight;
                let stable_media_mask = smoothstep(0.08, 0.82, media_weighted_mask);
                let media_affected_strength = stable_media_mask.max(media_sample.carve);
                let is_media_affected = media_affected_strength > 0.035;

                let media_emphasis = match self.media.coupling_mode {
                    MediaCouplingMode::Influence => match self.media.mode {
                        MediaMode::Off => 1.0,
                        MediaMode::Silhouette => mix(0.18, 2.00, stable_media_mask.powf(0.82)),
                        MediaMode::Color => mix(0.30, 1.72, stable_media_mask.powf(0.90)),
                        MediaMode::Ghost => mix(0.13, 1.42, stable_media_mask.powf(0.72)),
                    },
                    MediaCouplingMode::Formed => match self.media.mode {
                        MediaMode::Off => 1.0,
                        MediaMode::Silhouette => mix(0.06, 5.00, stable_media_mask.powf(0.48)),
                        MediaMode::Color => mix(0.08, 5.40, stable_media_mask.powf(0.44)),
                        MediaMode::Ghost => mix(0.06, 4.50, stable_media_mask.powf(0.42)),
                    },
                    MediaCouplingMode::Diagnostic => {
                        if is_media_affected {
                            4.20
                        } else {
                            0.10
                        }
                    }
                };

                let carve_amount = (media_sample.carve * media_amount).clamp(0.0, 1.0);
                let portal_gate = match self.media.coupling_mode {
                    MediaCouplingMode::Formed => mix(1.0, 0.58, carve_amount),
                    MediaCouplingMode::Influence => mix(1.0, 0.34, carve_amount),
                    MediaCouplingMode::Diagnostic => mix(1.0, 0.08, carve_amount),
                };
                let rain_visibility_floor = self.music.rain_visibility_floor();

                let media_visibility = if self.media.mode == MediaMode::Off {
                    1.0
                } else {
                    match self.media.coupling_mode {
                        MediaCouplingMode::Influence => {
                            mix(1.0, media_emphasis, media_amount) * portal_gate
                        }
                        MediaCouplingMode::Formed => {
                            mix(rain_visibility_floor, media_emphasis, media_amount.min(1.0))
                                .max(rain_visibility_floor)
                                * portal_gate
                        }
                        MediaCouplingMode::Diagnostic => media_emphasis * portal_gate,
                    }
                };

                let base_media_tint_amount = match self.media.coupling_mode {
                    MediaCouplingMode::Influence => stable_media_mask.powf(0.72) * media_amount,
                    MediaCouplingMode::Formed => {
                        (stable_media_mask.powf(0.28) * media_amount * 1.18).clamp(0.0, 1.0)
                    }
                    MediaCouplingMode::Diagnostic => {
                        if is_media_affected {
                            1.0
                        } else {
                            0.0
                        }
                    }
                };

                let wallpaper_color_gain = self.music.wallpaper_color_multiplier();
                let media_tint_amount = match music_color_mode {
                    MusicColorMode::Wallpaper => base_media_tint_amount * 0.10,
                    MusicColorMode::Palette => (base_media_tint_amount * 0.22).clamp(0.0, 1.0),
                    MusicColorMode::Hybrid => {
                        (base_media_tint_amount * wallpaper_color_gain * 0.28).clamp(0.0, 1.0)
                    }
                };

                let maximum_channel = media_sample.color[0]
                    .max(media_sample.color[1])
                    .max(media_sample.color[2])
                    .max(0.08);

                let media_hue = [
                    media_sample.color[0] / maximum_channel,
                    media_sample.color[1] / maximum_channel,
                    media_sample.color[2] / maximum_channel,
                ];

                let media_target_color = if self.media.coupling_mode
                    == MediaCouplingMode::Diagnostic
                    && is_media_affected
                {
                    [1.25, 0.22, 0.025]
                } else {
                    match self.media.mode {
                        MediaMode::Color => [
                            media_hue[0] * mix(0.58, 0.96, stable_media_mask),
                            media_hue[1] * mix(0.58, 0.96, stable_media_mask),
                            media_hue[2] * mix(0.58, 0.96, stable_media_mask),
                        ],
                        MediaMode::Ghost => {
                            let ghost = mix(0.68, 1.0, media_sample.mask);
                            [ghost * 0.82, ghost * 0.94, ghost]
                        }
                        MediaMode::Off | MediaMode::Silhouette => mode_body_color,
                    }
                };

                let shifted_media_target_color = [
                    (media_target_color[0] * wallpaper_channel_gains[0]).clamp(0.0, 1.45),
                    (media_target_color[1] * wallpaper_channel_gains[1]).clamp(0.0, 1.45),
                    (media_target_color[2] * wallpaper_channel_gains[2]).clamp(0.0, 1.45),
                ];

                let effective_media_target_color = match music_color_mode {
                    MusicColorMode::Wallpaper => music_primary_color,
                    MusicColorMode::Palette => music_primary_color,
                    MusicColorMode::Hybrid => [
                        mix(
                            shifted_media_target_color[0],
                            music_primary_color[0],
                            image_palette_accent_mix * 0.78,
                        ),
                        mix(
                            shifted_media_target_color[1],
                            music_primary_color[1],
                            image_palette_accent_mix * 0.78,
                        ),
                        mix(
                            shifted_media_target_color[2],
                            music_primary_color[2],
                            image_palette_accent_mix * 0.78,
                        ),
                    ],
                };

                let local_body_color = [
                    mix(
                        mode_body_color[0],
                        effective_media_target_color[0],
                        media_tint_amount,
                    ),
                    mix(
                        mode_body_color[1],
                        effective_media_target_color[1],
                        media_tint_amount,
                    ),
                    mix(
                        mode_body_color[2],
                        effective_media_target_color[2],
                        media_tint_amount,
                    ),
                ];

                let local_head_color = [
                    mix(
                        mode_head_color[0],
                        effective_media_target_color[0].max(0.72),
                        media_tint_amount * 0.78,
                    ),
                    mix(
                        mode_head_color[1],
                        effective_media_target_color[1].max(0.72),
                        media_tint_amount * 0.78,
                    ),
                    mix(
                        mode_head_color[2],
                        effective_media_target_color[2].max(0.72),
                        media_tint_amount * 0.78,
                    ),
                ];

                let local_cascade_color = [
                    mix(
                        mode_cascade_color[0],
                        effective_media_target_color[0],
                        media_tint_amount * 0.88,
                    ),
                    mix(
                        mode_cascade_color[1],
                        effective_media_target_color[1],
                        media_tint_amount * 0.88,
                    ),
                    mix(
                        mode_cascade_color[2],
                        effective_media_target_color[2],
                        media_tint_amount * 0.88,
                    ),
                ];

                let local_glow_color = [
                    mix(
                        mode_glow_color[0],
                        effective_media_target_color[0] * 0.48,
                        media_tint_amount,
                    ),
                    mix(
                        mode_glow_color[1],
                        effective_media_target_color[1] * 0.48,
                        media_tint_amount,
                    ),
                    mix(
                        mode_glow_color[2],
                        effective_media_target_color[2] * 0.48,
                        media_tint_amount,
                    ),
                ];

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

                let trail_fade = (1.0 - trail_position).powf(1.38);

                let trail_music_wave = 0.5
                    + 0.5
                        * ((trail_position * 11.0)
                            + music_rain_phase * std::f32::consts::TAU * 1.4
                            + stable_unit(
                                anatomy_seed ^ (segment as u32).wrapping_mul(0x7f4a_7c15),
                            ) * std::f32::consts::TAU)
                            .sin();

                let music_trail_presence =
                    mix(0.82, 1.28, trail_music_wave * music_glyph_variation)
                        * response_energy
                        * (1.0 + music_detail_drive * 0.08);

                let anatomy_energy = cluster_energy
                    * glyph_energy
                    * mix(1.0, 0.72, tail_narrowing)
                    * music_trail_presence;

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

                let cascade_packet = stream.cascade_intensity
                    * (cascade_core * 1.20 + cascade_wake * 0.16)
                    * mode_cascade_scale
                    * mix(0.92, 1.24, call_response)
                    * (1.0 + music_signature_event * 0.10);

                let head_injection = (-(segment as f32) * 0.38).exp();

                let propagation_profile = 0.80 + head_injection * 0.42;

                let base_energy =
                    stream.brightness * trail_fade * propagation_profile * stream_music_presence;

                let head_lift = match segment {
                    0 => 0.52,
                    1 => 0.24,
                    2 => 0.10,
                    _ => 0.0,
                } * mode_head_scale;

                let core_energy =
                    base_energy * atmosphere * anatomy_energy * (1.0 + cascade_packet * 0.35)
                        + stream.brightness * atmosphere * head_lift;

                let cascade_energy =
                    stream.brightness * cascade_depth * cascade_packet * (0.45 + trail_fade * 0.55);

                let head_energy = if segment == 0 && white_head_present {
                    stream.brightness * atmosphere * 1.30 * head_depth * mode_head_scale
                } else {
                    0.0
                };

                let cascade_light = cascade_energy * 0.82;

                let visibility_limit = match self.media.coupling_mode {
                    MediaCouplingMode::Influence => 2.4,
                    MediaCouplingMode::Formed | MediaCouplingMode::Diagnostic => 6.0,
                };

                let effective_media_visibility =
                    if self.media.coupling_mode == MediaCouplingMode::Formed {
                        media_visibility.max(self.music.rain_visibility_floor())
                    } else {
                        media_visibility
                    };
                let visibility = (density_presence
                    * fog_visibility
                    * effective_media_visibility
                    * response_energy
                    * signature_visibility_boost)
                    .clamp(0.0, visibility_limit);

                let core_color = [
                    (local_body_color[0] * core_energy
                        + local_head_color[0] * head_energy
                        + local_cascade_color[0] * cascade_light)
                        * exposure
                        * mode_core_scale
                        * visibility,
                    (local_body_color[1] * core_energy
                        + local_head_color[1] * head_energy
                        + local_cascade_color[1] * cascade_light)
                        * exposure
                        * mode_core_scale
                        * visibility,
                    (local_body_color[2] * core_energy
                        + local_head_color[2] * head_energy
                        + local_cascade_color[2] * cascade_light)
                        * exposure
                        * mode_core_scale
                        * visibility,
                ];

                let glow_energy = (base_energy * anatomy_energy * glow_atmosphere * 0.34
                    + stream.brightness * glow_atmosphere * cascade_packet * 0.42
                    + stream.brightness * glow_atmosphere * head_profile * 0.08)
                    * glow_control
                    * exposure
                    * mode_glow_scale
                    * visibility
                    * mix(0.94, 1.18, call_response)
                    * (1.0 + music_signature_event * 0.18 + music_structure_drive * 0.04);

                self.media_rain_glyphs = self.media_rain_glyphs.saturating_add(1);

                if is_media_affected {
                    self.media_affected_glyphs = self.media_affected_glyphs.saturating_add(1);
                }

                self.glyph_instances.push(GlyphInstance {
                    position_size: [
                        center_x,
                        center_y,
                        instance_glyph_width,
                        instance_glyph_height,
                    ],

                    color_glow: [core_color[0], core_color[1], core_color[2], glow_energy],

                    // The alpha channel carries continuous
                    // world-space depth to the glyph shader.
                    glow_color: [
                        local_glow_color[0],
                        local_glow_color[1],
                        local_glow_color[2],
                        depth,
                    ],

                    glyph_data: [stream.glyphs[segment], depth_band, 0, 0],
                });
            }
        }

        if self.apparition_system.enabled && !self.apparitions.is_empty() {
            let available_preview = MAX_GLYPH_INSTANCES.saturating_sub(self.glyph_instances.len());
            let apparition_image_light = self.music.apparition_image_light_multiplier();
            let apparition_channel_gains = self.music.moodbar_channel_gains();
            let apparition_budget = available_preview.min(6000usize);
            let mut apparition_instances = 0usize;

            for apparition in &self.apparitions {
                if apparition_instances >= apparition_budget {
                    break;
                }

                let image_aspect =
                    apparition.image.width as f32 / apparition.image.height.max(1) as f32;
                let columns = 52usize;
                let rows =
                    ((columns as f32 / image_aspect.max(0.25)).round() as usize).clamp(16, 60);
                let plane_height = 12.5 * apparition.scale;
                let plane_width = plane_height * image_aspect.max(0.1);
                let plane_center_camera = world_to_camera(
                    [
                        apparition.position[0] - self.camera.position[0],
                        apparition.position[1] - self.camera.position[1],
                        apparition.position[2] - self.camera.position[2],
                    ],
                    self.camera.yaw,
                    self.camera.pitch,
                );
                let projected_height_fraction = if plane_center_camera[2] > 0.1 {
                    plane_height / (2.0 * plane_center_camera[2] * tan_half_fov)
                } else {
                    1.0
                };
                let projected_size_fade = 1.0 - smoothstep(0.30, 0.48, projected_height_fraction);
                let fade = apparition.fade_amount() * projected_size_fade;
                if fade < 0.004 {
                    continue;
                }
                let (axis_x, axis_y, _axis_z) = if apparition.billboard {
                    camera_basis(self.camera.yaw, self.camera.pitch)
                } else {
                    ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0])
                };
                let plane_center = apparition.position;

                for row in 0..rows {
                    for column in 0..columns {
                        if self.glyph_instances.len() >= MAX_GLYPH_INSTANCES
                            || apparition_instances >= apparition_budget
                        {
                            break;
                        }

                        let u = if columns <= 1 {
                            0.5
                        } else {
                            column as f32 / (columns - 1) as f32
                        };
                        let v = if rows <= 1 {
                            0.5
                        } else {
                            row as f32 / (rows - 1) as f32
                        };

                        let cell_seed = (column as u32).wrapping_mul(0x9e37_79b9)
                            ^ (row as u32).wrapping_mul(0x85eb_ca6b)
                            ^ apparition.seed;
                        let jitter_u = (stable_unit(cell_seed ^ 0x243f_6a88) - 0.5)
                            / columns.max(1) as f32
                            * 0.58;
                        let jitter_v = (stable_unit(cell_seed ^ 0xb7e1_5162) - 0.5)
                            / rows.max(1) as f32
                            * 0.42;
                        let display_u = (u + jitter_u).clamp(0.0, 1.0);
                        let display_v = (v + jitter_v).clamp(0.0, 1.0);

                        let x =
                            (u * (apparition.image.width.saturating_sub(1)) as f32).round() as u32;
                        let y =
                            (v * (apparition.image.height.saturating_sub(1)) as f32).round() as u32;
                        let pixel_index = (y * apparition.image.width + x) as usize;
                        let sample_color = apparition
                            .image
                            .pixels
                            .get(pixel_index)
                            .copied()
                            .unwrap_or([0.0; 3]);
                        let luminance = sample_color[0] * 0.2126
                            + sample_color[1] * 0.7152
                            + sample_color[2] * 0.0722;
                        let contrasted = ((luminance - 0.5) * 1.85 + 0.5).clamp(0.0, 1.0);
                        let presence = smoothstep(0.12, 0.84, luminance.max(contrasted * 0.78));
                        let dropout_seed = stable_unit(cell_seed ^ 0x41a7_1105);
                        let streak_gate = smoothstep(
                            0.18,
                            0.92,
                            0.5 + 0.5
                                * ((v * 18.0)
                                    + dropout_seed * std::f32::consts::TAU
                                    + self.motion_time * 0.10)
                                    .sin(),
                        );

                        let edge_distance = u.min(1.0 - u).min(v.min(1.0 - v));
                        let edge_vignette = smoothstep(0.0, 0.17, edge_distance);
                        let radial_x = (u - 0.5) * 2.0;
                        let radial_y = (v - 0.5) * 2.0;
                        let radial_distance = (radial_x * radial_x + radial_y * radial_y).sqrt();
                        let radial_vignette = 1.0 - smoothstep(0.68, 1.18, radial_distance);
                        let vignette = (edge_vignette * radial_vignette).clamp(0.0, 1.0);

                        let rain_presence = presence * mix(0.62, 1.0, streak_gate) * vignette;
                        if rain_presence < 0.10 || dropout_seed > rain_presence + 0.22 {
                            continue;
                        }

                        let plane_x = (display_u - 0.5) * plane_width;
                        let plane_y = (0.5 - display_v) * plane_height;
                        let world_point = [
                            plane_center[0] + axis_x[0] * plane_x + axis_y[0] * plane_y,
                            plane_center[1] + axis_x[1] * plane_x + axis_y[1] * plane_y,
                            plane_center[2] + axis_x[2] * plane_x + axis_y[2] * plane_y,
                        ];

                        let camera_point = world_to_camera(
                            [
                                world_point[0] - self.camera.position[0],
                                world_point[1] - self.camera.position[1],
                                world_point[2] - self.camera.position[2],
                            ],
                            self.camera.yaw,
                            self.camera.pitch,
                        );

                        if camera_point[2] <= WORLD_NEAR_Z || camera_point[2] > WORLD_FAR_Z {
                            continue;
                        }

                        let ndc_x = camera_point[0] / camera_point[2] / tan_half_fov / aspect;
                        let ndc_y = camera_point[1] / camera_point[2] / tan_half_fov;
                        let screen_x = (ndc_x * 0.5 + 0.5) * width;
                        let screen_y = (0.5 - ndc_y * 0.5) * height;

                        if screen_x < -40.0
                            || screen_x > width + 40.0
                            || screen_y < -40.0
                            || screen_y > height + 40.0
                        {
                            continue;
                        }

                        let cell_world_width = plane_width / columns.max(1) as f32;
                        let cell_world_height = plane_height / rows.max(1) as f32;
                        let projected_cell_width = (cell_world_width * width
                            / (camera_point[2] * 2.0 * tan_half_fov * aspect.max(0.001)))
                        .abs();
                        let projected_cell_height = (cell_world_height * height
                            / (camera_point[2] * 2.0 * tan_half_fov.max(0.001)))
                        .abs();
                        let preview_depth = (1.0
                            - (camera_point[2] - WORLD_NEAR_Z) / WORLD_DEPTH_SPAN)
                            .clamp(0.0, 1.0);
                        let preview_depth_band = (preview_depth * 4.0).round() as u32;

                        let image_color = [
                            (sample_color[0] * mix(1.0, apparition_channel_gains[0], 0.24))
                                .clamp(0.0, 1.45),
                            (sample_color[1] * mix(1.0, apparition_channel_gains[1], 0.24))
                                .clamp(0.0, 1.45),
                            (sample_color[2] * mix(1.0, apparition_channel_gains[2], 0.24))
                                .clamp(0.0, 1.45),
                        ];

                        self.glyph_instances.push(GlyphInstance {
                            position_size: [
                                screen_x,
                                screen_y,
                                projected_cell_width.max(1.0) * mix(0.92, 1.24, rain_presence),
                                projected_cell_height.max(1.0) * mix(1.10, 1.85, rain_presence),
                            ],
                            color_glow: [
                                image_color[0]
                                    * (0.22 + rain_presence * 0.92)
                                    * fade
                                    * apparition_image_light,
                                image_color[1]
                                    * (0.22 + rain_presence * 0.92)
                                    * fade
                                    * apparition_image_light,
                                image_color[2]
                                    * (0.22 + rain_presence * 0.92)
                                    * fade
                                    * apparition_image_light,
                                apparition.opacity
                                    * self.music.apparition_opacity_multiplier()
                                    * (0.02 + rain_presence * 0.10)
                                    * fade,
                            ],
                            glow_color: [
                                mix(
                                    self.theme.glow_color[0],
                                    image_color[0],
                                    apparition.color_mix,
                                ),
                                mix(
                                    self.theme.glow_color[1],
                                    image_color[1],
                                    apparition.color_mix,
                                ),
                                mix(
                                    self.theme.glow_color[2],
                                    image_color[2],
                                    apparition.color_mix,
                                ),
                                preview_depth,
                            ],
                            glyph_data: [
                                ((contrasted * 63.0).round() as u32).min(63),
                                preview_depth_band,
                                0,
                                0,
                            ],
                        });
                        apparition_instances += 1;
                    }
                }
            }
        }

        if self.media.preview_mode != MediaPreviewMode::Off
            && self.media.mode != MediaMode::Off
            && !self.media.files.is_empty()
        {
            let (plane_width, plane_height) = self.media.plane_size();
            let (axis_x, axis_y, axis_z, plane_center) =
                self.media.plane_basis_and_center(&self.camera);

            let image_aspect = self.media.width as f32 / self.media.height.max(1) as f32;
            let columns = match self.media.preview_mode {
                MediaPreviewMode::Image => 110usize,
                MediaPreviewMode::Rain => 84usize,
                _ => 72usize,
            };
            let max_rows = match self.media.preview_mode {
                MediaPreviewMode::Image => 110usize,
                MediaPreviewMode::Rain => 96usize,
                _ => 72usize,
            };
            let rows =
                ((columns as f32 / image_aspect.max(0.25)).round() as usize).clamp(18, max_rows);

            let max_preview_instances = match self.media.preview_mode {
                MediaPreviewMode::Image => 9000usize,
                MediaPreviewMode::Rain => 4200usize,
                _ => 1800usize,
            };
            let available_preview = MAX_GLYPH_INSTANCES.saturating_sub(self.glyph_instances.len());
            let preview_budget = available_preview.min(max_preview_instances);

            if preview_budget > 0 {
                let layer_count = match self.media.preview_mode {
                    MediaPreviewMode::Image | MediaPreviewMode::Rain => 1usize,
                    _ => match self.media.space_mode {
                        MediaSpaceMode::Volume => 3usize,
                        _ => 1usize,
                    },
                };

                for layer_index in 0..layer_count {
                    let layer_t = if layer_count <= 1 {
                        0.5
                    } else {
                        layer_index as f32 / (layer_count - 1) as f32
                    };

                    let volume_offset = match self.media.space_mode {
                        MediaSpaceMode::Volume => mix(-2.2, 2.2, layer_t),
                        _ => 0.0,
                    };

                    for row in 0..rows {
                        for column in 0..columns {
                            if self.glyph_instances.len() >= MAX_GLYPH_INSTANCES
                                || (self.glyph_instances.len() + 1)
                                    .saturating_sub(MAX_RAIN_GLYPH_INSTANCES)
                                    >= preview_budget
                            {
                                break;
                            }

                            let u = if columns <= 1 {
                                0.5
                            } else {
                                column as f32 / (columns - 1) as f32
                            };
                            let v = if rows <= 1 {
                                0.5
                            } else {
                                row as f32 / (rows - 1) as f32
                            };

                            let preview_seed = stable_unit(
                                ((column as u32).wrapping_mul(0x9e37_79b9))
                                    ^ ((row as u32).wrapping_mul(0x85eb_ca6b))
                                    ^ 0x52a1_4e55,
                            );

                            let (sample_u, sample_v, display_u, display_v) =
                                if self.media.preview_mode == MediaPreviewMode::Rain {
                                    let fall_speed =
                                        mix(0.08, 0.28, stable_unit((column as u32) ^ 0x41c6_ce57));
                                    let fall_phase =
                                        (self.motion_time * fall_speed + preview_seed).fract();
                                    let rain_v = (v + fall_phase).fract();
                                    let sway = (self.motion_time * 0.45
                                        + column as f32 * 0.37
                                        + preview_seed * std::f32::consts::TAU)
                                        .sin()
                                        * 0.006;
                                    (u, v, (u + sway).clamp(0.0, 1.0), rain_v)
                                } else {
                                    (u, v, u, v)
                                };

                            let x = (sample_u * (self.media.width.saturating_sub(1)) as f32).round()
                                as u32;
                            let y = (sample_v * (self.media.height.saturating_sub(1)) as f32)
                                .round() as u32;
                            let pixel_index = (y * self.media.width + x) as usize;
                            let sample_color = self
                                .media
                                .pixels
                                .get(pixel_index)
                                .copied()
                                .unwrap_or([0.0; 3]);
                            let luminance = sample_color[0] * 0.2126
                                + sample_color[1] * 0.7152
                                + sample_color[2] * 0.0722;
                            let contrasted =
                                ((luminance - 0.5) * self.media.contrast + 0.5).clamp(0.0, 1.0);
                            let mask = smoothstep(0.08, 0.90, contrasted);

                            let presence = match self.media.preview_mode {
                                MediaPreviewMode::Off => 0.0,
                                MediaPreviewMode::Image => {
                                    let image_luma = sample_color[0] * 0.2126
                                        + sample_color[1] * 0.7152
                                        + sample_color[2] * 0.0722;
                                    smoothstep(0.02, 0.82, image_luma.max(contrasted * 0.78))
                                }
                                MediaPreviewMode::Matrix => mask.powf(0.75),
                                MediaPreviewMode::Rain => {
                                    let rain_mask = mask.powf(0.82);
                                    let streak_gate = smoothstep(
                                        0.28,
                                        0.86,
                                        0.5 + 0.5
                                            * ((display_v * 20.0) + preview_seed * 12.0).sin(),
                                    );
                                    rain_mask * streak_gate
                                }
                            };

                            let minimum_presence = match self.media.preview_mode {
                                MediaPreviewMode::Image => 0.02,
                                MediaPreviewMode::Rain => 0.12,
                                _ => 0.05,
                            };

                            if presence < minimum_presence {
                                continue;
                            }

                            let plane_x = (display_u - 0.5) * plane_width;
                            let plane_y = (0.5 - display_v) * plane_height;
                            let plane_z = match self.media.space_mode {
                                MediaSpaceMode::Extruded => mix(-2.8, 2.8, contrasted),
                                MediaSpaceMode::Volume => volume_offset,
                                _ => 0.0,
                            };

                            let world_point = [
                                plane_center[0]
                                    + axis_x[0] * plane_x
                                    + axis_y[0] * plane_y
                                    + axis_z[0] * plane_z,
                                plane_center[1]
                                    + axis_x[1] * plane_x
                                    + axis_y[1] * plane_y
                                    + axis_z[1] * plane_z,
                                plane_center[2]
                                    + axis_x[2] * plane_x
                                    + axis_y[2] * plane_y
                                    + axis_z[2] * plane_z,
                            ];

                            let camera_point = world_to_camera(
                                [
                                    world_point[0] - self.camera.position[0],
                                    world_point[1] - self.camera.position[1],
                                    world_point[2] - self.camera.position[2],
                                ],
                                self.camera.yaw,
                                self.camera.pitch,
                            );

                            if camera_point[2] <= WORLD_NEAR_Z || camera_point[2] > WORLD_FAR_Z {
                                continue;
                            }

                            let ndc_x = camera_point[0] / camera_point[2] / tan_half_fov / aspect;
                            let ndc_y = camera_point[1] / camera_point[2] / tan_half_fov;
                            let screen_x = (ndc_x * 0.5 + 0.5) * width;
                            let screen_y = (0.5 - ndc_y * 0.5) * height;

                            if screen_x < -40.0
                                || screen_x > width + 40.0
                                || screen_y < -40.0
                                || screen_y > height + 40.0
                            {
                                continue;
                            }

                            let perspective_scale =
                                (WORLD_FAR_Z / camera_point[2]).powf(0.60).clamp(0.58, 3.25);
                            let preview_depth = (1.0
                                - (camera_point[2] - WORLD_NEAR_Z) / WORLD_DEPTH_SPAN)
                                .clamp(0.0, 1.0);
                            let preview_depth_band = (preview_depth * 4.0).round() as u32;

                            let cell_world_width = plane_width / columns.max(1) as f32;
                            let cell_world_height = plane_height / rows.max(1) as f32;
                            let projected_cell_width = (cell_world_width * width
                                / (camera_point[2] * 2.0 * tan_half_fov * aspect.max(0.001)))
                            .abs();
                            let projected_cell_height = (cell_world_height * height
                                / (camera_point[2] * 2.0 * tan_half_fov.max(0.001)))
                            .abs();

                            let (
                                preview_width,
                                preview_height,
                                glyph_index,
                                mode_color,
                                glow_color,
                                alpha,
                            ) = match self.media.preview_mode {
                                MediaPreviewMode::Image => (
                                    projected_cell_width.max(1.0) * 1.10,
                                    projected_cell_height.max(1.0) * 1.10,
                                    63u32,
                                    [
                                        sample_color[0] * (0.35 + presence * 1.10),
                                        sample_color[1] * (0.35 + presence * 1.10),
                                        sample_color[2] * (0.35 + presence * 1.10),
                                    ],
                                    sample_color,
                                    0.06 + presence * 0.18,
                                ),
                                MediaPreviewMode::Matrix => (
                                    7.5 * scale * perspective_scale,
                                    11.5 * scale * perspective_scale,
                                    ((contrasted * 63.0).round() as u32).min(63),
                                    [
                                        mix(self.theme.body_color[0], sample_color[0], 0.18),
                                        mix(self.theme.body_color[1], sample_color[1], 0.18),
                                        mix(self.theme.body_color[2], sample_color[2], 0.18),
                                    ],
                                    self.theme.glow_color,
                                    0.10 + presence * 0.18,
                                ),
                                MediaPreviewMode::Rain => (
                                    projected_cell_width.max(1.0) * mix(0.95, 1.35, presence),
                                    projected_cell_height.max(1.0) * mix(1.20, 1.85, presence),
                                    63u32,
                                    [
                                        sample_color[0] * (0.22 + presence * 0.95),
                                        sample_color[1] * (0.22 + presence * 0.95),
                                        sample_color[2] * (0.22 + presence * 0.95),
                                    ],
                                    [
                                        mix(self.theme.glow_color[0], sample_color[0], 0.72),
                                        mix(self.theme.glow_color[1], sample_color[1], 0.72),
                                        mix(self.theme.glow_color[2], sample_color[2], 0.72),
                                    ],
                                    0.05 + presence * 0.22,
                                ),
                                MediaPreviewMode::Off => {
                                    (0.0, 0.0, 0u32, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 0.0)
                                }
                            };

                            self.glyph_instances.push(GlyphInstance {
                                position_size: [screen_x, screen_y, preview_width, preview_height],
                                color_glow: [mode_color[0], mode_color[1], mode_color[2], alpha],
                                glow_color: [
                                    glow_color[0],
                                    glow_color[1],
                                    glow_color[2],
                                    preview_depth,
                                ],
                                glyph_data: [glyph_index, preview_depth_band, 0, 0],
                            });
                        }
                    }
                }
            }
        }

        if self.media.show_gizmo
            && self.media.mode != MediaMode::Off
            && !self.media.files.is_empty()
        {
            let (plane_width, plane_height) = self.media.plane_size();
            let (axis_x, axis_y, axis_z, plane_center) =
                self.media.plane_basis_and_center(&self.camera);
            let guide_color = match self.media.space_mode {
                MediaSpaceMode::Flat => [0.18, 0.92, 1.10],
                MediaSpaceMode::Portal => [1.12, 0.58, 0.08],
                MediaSpaceMode::Extruded => [1.02, 0.18, 0.92],
                MediaSpaceMode::Volume => [0.28, 0.62, 1.14],
            };

            let guide_points = [
                (0.0, 0.0, 0.0, 42.0),
                (-0.5, -0.5, 0.0, 24.0),
                (0.5, -0.5, 0.0, 24.0),
                (-0.5, 0.5, 0.0, 24.0),
                (0.5, 0.5, 0.0, 24.0),
                (0.0, -0.5, 0.0, 18.0),
                (0.0, 0.5, 0.0, 18.0),
                (-0.5, 0.0, 0.0, 18.0),
                (0.5, 0.0, 0.0, 18.0),
                (0.0, 0.0, 2.0, 16.0),
            ];

            for (point_index, (plane_x, plane_y, plane_z, marker_size)) in
                guide_points.into_iter().enumerate()
            {
                if self.glyph_instances.len() >= MAX_GLYPH_INSTANCES {
                    break;
                }

                let world_point = [
                    plane_center[0]
                        + axis_x[0] * plane_x * plane_width
                        + axis_y[0] * plane_y * plane_height
                        + axis_z[0] * plane_z,
                    plane_center[1]
                        + axis_x[1] * plane_x * plane_width
                        + axis_y[1] * plane_y * plane_height
                        + axis_z[1] * plane_z,
                    plane_center[2]
                        + axis_x[2] * plane_x * plane_width
                        + axis_y[2] * plane_y * plane_height
                        + axis_z[2] * plane_z,
                ];

                let camera_point = world_to_camera(
                    [
                        world_point[0] - self.camera.position[0],
                        world_point[1] - self.camera.position[1],
                        world_point[2] - self.camera.position[2],
                    ],
                    self.camera.yaw,
                    self.camera.pitch,
                );

                if camera_point[2] <= 0.10 {
                    continue;
                }

                let marker_ndc_x = camera_point[0] / camera_point[2] / tan_half_fov / aspect;
                let marker_ndc_y = camera_point[1] / camera_point[2] / tan_half_fov;
                let marker_x = (marker_ndc_x * 0.5 + 0.5) * width;
                let marker_y = (0.5 - marker_ndc_y * 0.5) * height;

                if marker_x < -80.0
                    || marker_x > width + 80.0
                    || marker_y < -80.0
                    || marker_y > height + 80.0
                {
                    continue;
                }

                let center_boost = if point_index == 0 { 1.0 } else { 0.72 };
                let normal_boost = if point_index == 9 { 0.52 } else { 1.0 };
                let marker_alpha = center_boost * normal_boost;

                self.glyph_instances.push(GlyphInstance {
                    position_size: [marker_x, marker_y, marker_size * scale, marker_size * scale],
                    color_glow: [
                        guide_color[0] * marker_alpha,
                        guide_color[1] * marker_alpha,
                        guide_color[2] * marker_alpha,
                        0.16 * marker_alpha,
                    ],
                    glow_color: [
                        guide_color[0] * 0.28,
                        guide_color[1] * 0.28,
                        guide_color[2] * 0.28,
                        1.0,
                    ],
                    glyph_data: [0, 4, 1, 0],
                });
            }
        }

        if self.camera.mouse_look && self.camera.show_reticle {
            let reticle_color = self.theme.head_color;

            self.glyph_instances.push(GlyphInstance {
                position_size: [width * 0.5, height * 0.5, 30.0 * scale, 30.0 * scale],
                color_glow: [
                    reticle_color[0] * 0.72,
                    reticle_color[1] * 0.72,
                    reticle_color[2] * 0.72,
                    0.18,
                ],
                glow_color: [
                    self.theme.glow_color[0],
                    self.theme.glow_color[1],
                    self.theme.glow_color[2],
                    1.0,
                ],
                glyph_data: [0, 4, 1, 0],
            });
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
            "Neon Rain — {} — {:.0} FPS — {:.1} ms — {} glyphs — affected {}/{} — ghosts {}{} — z {:.1} — yaw {:.0}° pitch {:.0}° — FOV {:.0} — move {:.1} — flight {}{} — {} — {}",
            self.mode.profile().label,
            fps,
            average_ms,
            self.glyph_instance_count,
            self.media_affected_glyphs,
            self.media_rain_glyphs,
            self.apparitions.len(),
            if self.apparition_system.enabled { " on" } else { " off" },
            self.camera.position[2],
            self.camera.yaw.to_degrees(),
            self.camera.pitch.to_degrees(),
            self.camera.fov_y,
            self.camera.movement_speed,
            self.camera.auto_flight.label(),
            if self.camera.mouse_look { " — LOOK" } else { "" },
            self.media.title_label(),
            self.music.status_label(),
        ));

        println!(
            "fps={fps:.1}  frame={average_ms:.2}ms  worst={:.2}ms  glyphs={}  affected={}/{}  apparitions={}",
            self.stats_worst_ms,
            self.glyph_instance_count,
            self.media_affected_glyphs,
            self.media_rain_glyphs,
            self.apparitions.len(),
        );

        self.stats_elapsed = 0.0;
        self.stats_frames = 0;
        self.stats_worst_ms = 0.0;
    }

    fn update(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();

        self.last_frame = now;

        let active_dt = if self.paused { 0.0 } else { dt.min(0.05) };

        if active_dt > 0.0 {
            let theme_response = 1.0 - (-3.2 * active_dt).exp();

            self.theme.approach(self.target_theme, theme_response);

            self.motion_time += active_dt;
        }

        self.music.update(active_dt);
        self.signal_inspector.update(active_dt, &self.music);
        self.media.sync_track_memory(self.music.track_identifier());
        self.update_cinematic_director(active_dt);
        self.update_camera(active_dt);

        let simulation_dt = active_dt * self.theme.speed_scale * self.music.speed_multiplier();

        self.simulation.update(simulation_dt);

        let stream_fraction = self.simulation.streams.len() as f32 / simulation::MAX_STREAMS as f32;

        let automatic_exposure = 1.35 - stream_fraction * 0.45;

        self.target_exposure =
            automatic_exposure * self.theme.exposure_bias * self.music.exposure_multiplier();

        if active_dt > 0.0 {
            let exposure_response = 1.0 - (-2.0 * active_dt).exp();

            self.exposure += (self.target_exposure - self.exposure) * exposure_response;

            let music_image_target = self.music.image_match_target();
            self.media.update_auto_cycle(
                active_dt * self.music.media_cycle_multiplier(),
                music_image_target,
            );
        }

        self.update_apparitions(active_dt);
        self.rebuild_glyph_instances();

        let uniforms = Uniforms {
            time: self.motion_time,
            aspect: calculate_aspect(self.size),
            resolution: [self.size.width as f32, self.size.height as f32],
            controls: [
                self.theme.speed_scale * self.music.speed_multiplier(),
                self.theme.glow_strength * self.music.glow_multiplier(),
                self.exposure,
                0.0,
            ],
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
                            r: self.theme.background_color[0] as f64,
                            g: self.theme.background_color[1] as f64,
                            b: self.theme.background_color[2] as f64,
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

        self.bloom.composite(
            &self.queue,
            &mut encoder,
            &view,
            self.theme.bloom_settings(),
            self.paused,
        );

        self.help_overlay
            .render(&self.device, &self.queue, &mut encoder, &view);

        self.signal_inspector
            .render(&self.device, &self.queue, &mut encoder, &view);

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

struct App {
    state: Option<State>,
    launch: LaunchOptions,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let mut window_attributes = Window::default_attributes()
            .with_title("Neon Rain")
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.launch.preferences.window_width as f64,
                self.launch.preferences.window_height as f64,
            ));

        if self.launch.preferences.fullscreen {
            window_attributes =
                window_attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }

        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .expect("Failed to create window"),
        );

        let state = pollster::block_on(State::new(
            event_loop.owned_display_handle(),
            window.clone(),
            self.launch.media_path(),
            &self.launch,
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
                state.save_preferences();
                event_loop.exit();
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key),

                        state: key_state,
                        repeat,
                        ..
                    },
                ..
            } => {
                let pressed = key_state == ElementState::Pressed;

                if matches!(key, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                    state.camera_input.boost = pressed;
                }

                if state.signal_inspector.is_visible() {
                    if pressed && !repeat {
                        match key {
                            KeyCode::F2 | KeyCode::Escape => {
                                state.signal_inspector.toggle();
                                state.camera_input = CameraInput::default();
                            }
                            KeyCode::ArrowRight => state.signal_inspector.next(),
                            KeyCode::ArrowLeft => state.signal_inspector.previous(),
                            KeyCode::ArrowUp => state.signal_inspector.adjust_gain(1.0),
                            KeyCode::ArrowDown => state.signal_inspector.adjust_gain(-1.0),
                            KeyCode::Space => state.signal_inspector.toggle_freeze(),
                            KeyCode::KeyA => state.signal_inspector.toggle_auto_cycle(),
                            KeyCode::KeyR => state.signal_inspector.reset(),
                            _ => {}
                        }
                    }

                    return;
                }

                if pressed && !repeat && key == KeyCode::F2 && !state.help_overlay.is_visible() {
                    state.camera_input = CameraInput::default();
                    state.set_mouse_look(false);
                    state.signal_inspector.toggle();
                    println!("Signal inspector: open");
                    return;
                }

                let help_shortcut =
                    key == KeyCode::F1 || (key == KeyCode::Slash && state.camera_input.boost);

                if pressed && !repeat && help_shortcut {
                    state.toggle_help_overlay();
                    return;
                }

                if state.help_overlay.is_visible() {
                    if pressed && !repeat && key == KeyCode::Escape {
                        state.toggle_help_overlay();
                    }

                    return;
                }

                match key {
                    KeyCode::KeyW => {
                        state.camera_input.forward = pressed;
                    }

                    KeyCode::KeyS => {
                        state.camera_input.backward = pressed;
                    }

                    KeyCode::KeyA => {
                        state.camera_input.left = pressed;
                    }

                    KeyCode::KeyD => {
                        state.camera_input.right = pressed;
                    }

                    KeyCode::KeyQ => {
                        state.camera_input.down = pressed;
                    }

                    KeyCode::KeyE => {
                        state.camera_input.up = pressed;
                    }

                    KeyCode::ShiftLeft | KeyCode::ShiftRight => {
                        state.camera_input.boost = pressed;
                    }

                    KeyCode::ControlLeft | KeyCode::ControlRight => {
                        state.camera_input.precision = pressed;
                    }

                    KeyCode::Escape if pressed && !repeat => {
                        if state.camera.mouse_look {
                            state.set_mouse_look(false);
                        } else {
                            state.save_preferences();
                            event_loop.exit();
                        }
                    }

                    KeyCode::F11 if pressed && !repeat => {
                        state.toggle_fullscreen();
                    }

                    KeyCode::Space if pressed && !repeat => {
                        state.paused = !state.paused;

                        state.camera_input = CameraInput::default();

                        state.print_controls();
                    }

                    KeyCode::KeyC if pressed && !repeat => {
                        state.cycle_auto_flight();
                    }

                    KeyCode::Tab if pressed && !repeat => {
                        state.toggle_mouse_look();
                    }

                    KeyCode::KeyH if pressed && !repeat => {
                        state.toggle_reticle();
                    }

                    KeyCode::PageUp if pressed && !repeat => {
                        state.adjust_camera_speed(1.0);
                    }

                    KeyCode::PageDown if pressed && !repeat => {
                        state.adjust_camera_speed(-1.0);
                    }

                    KeyCode::KeyR if pressed && !repeat => {
                        state.reset_camera();
                    }

                    KeyCode::KeyG if pressed && !repeat => {
                        state.simulation = Simulation::new(state.size.width, state.size.height);

                        state.bloom.invalidate_history();

                        println!("Regenerated all persistent streams",);
                    }

                    KeyCode::Backslash if pressed && !repeat => {
                        state.toggle_apparitions();
                    }

                    KeyCode::Quote if pressed && !repeat => {
                        state.adjust_apparition_opacity(0.05);
                    }

                    KeyCode::Slash if pressed && !repeat => {
                        state.adjust_apparition_opacity(-0.05);
                    }

                    KeyCode::Backquote if pressed && !repeat => {
                        state.adjust_apparition_frequency(0.03);
                    }

                    KeyCode::Backspace if pressed && !repeat => {
                        state.adjust_apparition_frequency(-0.03);
                    }

                    KeyCode::ArrowUp if pressed && !repeat => {
                        state.move_media_plane(0.0, 1.0);
                    }

                    KeyCode::ArrowDown if pressed && !repeat => {
                        state.move_media_plane(0.0, -1.0);
                    }

                    KeyCode::ArrowRight if pressed && !repeat => {
                        state.move_media_plane(1.0, 0.0);
                    }

                    KeyCode::ArrowLeft if pressed && !repeat => {
                        state.move_media_plane(-1.0, 0.0);
                    }

                    KeyCode::Equal if pressed && !repeat => {
                        state.target_theme.exposure_bias =
                            (state.target_theme.exposure_bias + 0.20).min(4.0);

                        state.print_controls();
                    }

                    KeyCode::Minus if pressed && !repeat => {
                        state.target_theme.exposure_bias =
                            (state.target_theme.exposure_bias - 0.20).max(0.10);

                        state.print_controls();
                    }

                    KeyCode::Digit0 if pressed && !repeat => {
                        state.apply_strong_media_defaults();
                    }

                    KeyCode::Digit1 if pressed && !repeat => {
                        state.apply_preset(1);
                    }

                    KeyCode::Digit2 if pressed && !repeat => {
                        state.apply_preset(2);
                    }

                    KeyCode::Digit3 if pressed && !repeat => {
                        state.apply_preset(3);
                    }

                    KeyCode::Digit4 if pressed && !repeat => {
                        state.apply_preset(4);
                    }

                    KeyCode::Digit5 if pressed && !repeat => {
                        state.apply_preset(5);
                    }

                    KeyCode::Digit6 if pressed && !repeat => {
                        state.apply_preset(6);
                    }

                    KeyCode::Digit7 if pressed && !repeat => {
                        state.apply_preset(7);
                    }

                    KeyCode::Digit8 if pressed && !repeat => {
                        state.apply_preset(8);
                    }

                    KeyCode::Digit9 if pressed && !repeat => {
                        state.apply_preset(9);
                    }

                    KeyCode::BracketRight if pressed && !repeat => {
                        state.apply_theme(state.mode.next());
                    }

                    KeyCode::BracketLeft if pressed && !repeat => {
                        state.apply_theme(state.mode.previous());
                    }

                    KeyCode::KeyM if pressed && !repeat => {
                        state.cycle_media_mode();
                    }

                    KeyCode::F3 if pressed && !repeat => {
                        state.cycle_palette();
                    }

                    KeyCode::F4 if pressed && !repeat => {
                        state.cycle_music_color_mode();
                    }

                    KeyCode::F5 if pressed && !repeat => {
                        if state.camera_input.boost {
                            state.cycle_music_intensity();
                        } else {
                            state.toggle_music_reactivity();
                        }
                    }

                    KeyCode::F6 if pressed && !repeat => {
                        state.toggle_cinematic_director();
                    }

                    KeyCode::F10 if pressed && !repeat => {
                        state.cycle_music_source();
                    }

                    KeyCode::F7 if pressed && !repeat => {
                        state.toggle_media_auto_cycle();
                    }

                    KeyCode::F8 if pressed && !repeat => {
                        state.adjust_media_auto_cycle_interval(1.0);
                    }

                    KeyCode::F9 if pressed && !repeat => {
                        state.adjust_media_auto_cycle_interval(-1.0);
                    }

                    KeyCode::Comma if pressed && !repeat => {
                        state.previous_media_image();
                    }

                    KeyCode::Period if pressed && !repeat => {
                        state.next_media_image();
                    }

                    KeyCode::KeyI if pressed && !repeat => {
                        state.reload_media();
                    }

                    KeyCode::KeyO if pressed && !repeat => {
                        state.adjust_media_opacity(-0.08);
                    }

                    KeyCode::KeyP if pressed && !repeat => {
                        state.adjust_media_opacity(0.08);
                    }

                    KeyCode::KeyK if pressed && !repeat => {
                        state.adjust_media_contrast(-0.15);
                    }

                    KeyCode::KeyL if pressed && !repeat => {
                        state.adjust_media_contrast(0.15);
                    }

                    KeyCode::KeyZ if pressed && !repeat => {
                        state.adjust_media_scale(-0.10);
                    }

                    KeyCode::KeyX if pressed && !repeat => {
                        state.adjust_media_scale(0.10);
                    }

                    KeyCode::KeyJ if pressed && !repeat => {
                        state.adjust_media_depth(-1.5);
                    }

                    KeyCode::KeyN if pressed && !repeat => {
                        state.adjust_media_depth(1.5);
                    }

                    KeyCode::KeyV if pressed && !repeat => {
                        state.cycle_media_space_mode();
                    }

                    KeyCode::KeyB if pressed && !repeat => {
                        state.toggle_media_space_lock();
                    }

                    KeyCode::KeyF if pressed && !repeat => {
                        state.focus_media_plane();
                    }

                    KeyCode::KeyY if pressed && !repeat => {
                        state.toggle_media_gizmo();
                    }

                    KeyCode::KeyT if pressed && !repeat => {
                        state.cycle_media_preview_mode();
                    }

                    KeyCode::Semicolon if pressed && !repeat => {
                        state.cycle_media_coupling_mode();
                    }

                    KeyCode::KeyU if pressed && !repeat => {
                        state.reset_media_transform();
                    }

                    _ => {}
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if state.signal_inspector.is_visible() {
                    return;
                }

                if state.help_overlay.is_visible() {
                    return;
                }

                let scroll_lines = match delta {
                    MouseScrollDelta::LineDelta(_, vertical) => vertical,

                    MouseScrollDelta::PixelDelta(position) => position.y as f32 / 120.0,
                };

                state.adjust_zoom(scroll_lines);
            }

            WindowEvent::Focused(false) => {
                state.camera_input = CameraInput::default();
                state.set_mouse_look(false);
            }

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

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        if let DeviceEvent::MouseMotion { delta } = event {
            state.handle_mouse_motion(delta.0, delta.1);
        }
    }
}

fn print_cli_help() {
    println!(
        "Neon Rain {}\n\
         \n\
         A living, music-reactive Matrix rain visualizer.\n\
         \n\
         Usage:\n\
           neon-rain [OPTIONS] [MEDIA_PATH]\n\
         \n\
         Appearance:\n\
           --theme NAME          Select a named motion/theme profile\n\
           --palette NAME        Apply an independent color palette\n\
           --list-themes         List available theme names\n\
           --list-palettes       List available palette names\n\
         \n\
         Window and motion:\n\
           --fullscreen          Start borderless fullscreen\n\
           --windowed            Start in a normal window\n\
           --size WIDTHxHEIGHT   Set the initial window size\n\
           --auto-flight MODE    off, forward, weave, orbit, or tunnel\n\
           --cinematic           Enable the cinematic director\n\
           --no-cinematic        Disable the cinematic director\n\
         \n\
         Media:\n\
           --image PATH          Use one image as the media source\n\
           --media-dir PATH      Load images from a directory\n\
           --media               Enable configured media\n\
           --no-media            Disable local media coupling\n\
           --warm-cache          Prepare the media coupling cache and exit\n\
         \n\
         Configuration:\n\
           --config PATH         Use a specific configuration file\n\
           --print-config        Print effective settings and exit\n\
           --write-default-config  Create the default XDG config and exit\n\
           --reset-session       Clear remembered session choices\n\
           --remember            Load and save remembered choices\n\
           --no-remember         Ignore and do not save session choices\n\
         \n\
           -h, --help            Show this help and exit\n\
           -V, --version         Show the version and exit",
        env!("CARGO_PKG_VERSION"),
    );
}

fn print_themes() {
    println!("Available Neon Rain themes:");
    for mode in RainMode::all() {
        println!(
            "  {:<13} preset={}  {}",
            mode.slug(),
            mode.preset(),
            mode.profile().label,
        );
    }
}

fn print_palettes() {
    println!("Available Neon Rain palettes:");
    for palette in PALETTE_NAMES {
        println!("  {palette}");
    }
}

fn main() {
    env_logger::init();

    let launch = match settings::parse_launch_options() {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("neon-rain: {error}\n");
            print_cli_help();
            std::process::exit(2);
        }
    };

    match launch.action {
        CliAction::Help => {
            print_cli_help();
            return;
        }
        CliAction::Version => {
            println!("neon-rain {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        CliAction::ListThemes => {
            print_themes();
            return;
        }
        CliAction::ListPalettes => {
            print_palettes();
            return;
        }
        CliAction::PrintConfig => {
            print!("{}", settings::render_preferences(&launch.preferences));
            println!("# config_path = {}", launch.config_path.display());
            println!("# state_path = {}", launch.state_path.display());
            return;
        }
        CliAction::WriteDefaultConfig => {
            match settings::write_default_config(&launch.config_path) {
                Ok(()) => println!(
                    "Created Neon Rain configuration: {}",
                    launch.config_path.display(),
                ),
                Err(error) => {
                    eprintln!("Could not create {}: {error}", launch.config_path.display(),);
                    std::process::exit(1);
                }
            }
            return;
        }
        CliAction::ResetSession => {
            match settings::reset_session(&launch.state_path) {
                Ok(true) => println!(
                    "Removed remembered session: {}",
                    launch.state_path.display(),
                ),
                Ok(false) => println!(
                    "No remembered session existed at {}",
                    launch.state_path.display(),
                ),
                Err(error) => {
                    eprintln!("Could not reset {}: {error}", launch.state_path.display(),);
                    std::process::exit(1);
                }
            }
            return;
        }
        CliAction::Run => {}
    }

    if RainMode::from_name(&launch.preferences.theme).is_none() {
        eprintln!(
            "Unknown theme {:?}. Run --list-themes.",
            launch.preferences.theme,
        );
        std::process::exit(2);
    }

    if normalize_palette_name(&launch.preferences.palette).is_none() {
        eprintln!(
            "Unknown palette {:?}. Run --list-palettes.",
            launch.preferences.palette,
        );
        std::process::exit(2);
    }

    if AutoFlightMode::from_name(&launch.preferences.auto_flight).is_none() {
        eprintln!(
            "Unknown auto-flight mode {:?}. Use off, forward, weave, orbit, or tunnel.",
            launch.preferences.auto_flight,
        );
        std::process::exit(2);
    }

    if launch.warm_cache {
        if let Some(path) = launch.media_path() {
            let media = MediaField::from_path(Some(path));
            println!(
                "Persistent coupling cache ready: {}/{} images",
                media
                    .coupling_images
                    .iter()
                    .filter(|image| image.is_some())
                    .count(),
                media.files.len(),
            );
        } else {
            println!("No media path was available to warm");
        }
        return;
    }

    println!(
        "Startup settings: theme={} palette={} fullscreen={} size={}x{} flight={} cinematic={} remember={}",
        launch.preferences.theme,
        launch.preferences.palette,
        launch.preferences.fullscreen,
        launch.preferences.window_width,
        launch.preferences.window_height,
        launch.preferences.auto_flight,
        launch.preferences.cinematic,
        launch.preferences.remember,
    );
    println!("Configuration: {}", launch.config_path.display());
    println!("Remembered session: {}", launch.state_path.display());

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        state: None,
        launch,
    };

    event_loop.run_app(&mut app).expect("Application error");
}
