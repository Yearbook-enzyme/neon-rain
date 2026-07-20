use std::{
    env, fs,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::resilience::{OperatingState, RollingAnalyzer};

const SAMPLE_RATE: f32 = 48_000.0;
const FRAME_BYTES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicIntensity {
    Subtle,
    Balanced,
    Intense,
}

impl MusicIntensity {
    pub fn next(self) -> Self {
        match self {
            Self::Subtle => Self::Balanced,
            Self::Balanced => Self::Intense,
            Self::Intense => Self::Subtle,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Subtle => "subtle",
            Self::Balanced => "balanced",
            Self::Intense => "intense",
        }
    }

    fn gain(self) -> f32 {
        match self {
            Self::Subtle => 0.58,
            Self::Balanced => 1.00,
            Self::Intense => 1.58,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicSourceMode {
    System,
    Strawberry,
}

impl MusicSourceMode {
    pub fn next(self) -> Self {
        match self {
            Self::System => Self::Strawberry,
            Self::Strawberry => Self::System,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Strawberry => "player-aware",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicColorMode {
    Wallpaper,
    Palette,
    Hybrid,
}

impl MusicColorMode {
    pub fn next(self) -> Self {
        match self {
            Self::Wallpaper => Self::Palette,
            Self::Palette => Self::Hybrid,
            Self::Hybrid => Self::Wallpaper,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Wallpaper => "cycle",
            Self::Palette => "energy",
            Self::Hybrid => "hybrid",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct MoodbarTimeline {
    samples: Arc<Vec<[f32; 3]>>,
    source: String,
}

fn load_moodbar_timeline(helper_output: &str) -> MoodbarTimeline {
    let mut fields = helper_output.trim().splitn(2, '|');
    let path = fields.next().unwrap_or_default().trim();
    let source = fields.next().unwrap_or("moodbar").trim().to_string();
    if path.is_empty() {
        return MoodbarTimeline::default();
    }

    let Ok(data) = fs::read(path) else {
        return MoodbarTimeline::default();
    };
    let samples = data
        .chunks_exact(3)
        .map(|rgb| {
            [
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
            ]
        })
        .collect::<Vec<_>>();

    if samples.len() < 32 {
        MoodbarTimeline::default()
    } else {
        MoodbarTimeline {
            samples: Arc::new(samples),
            source,
        }
    }
}

fn sample_moodbar(
    timeline: &MoodbarTimeline,
    position_seconds: f32,
    duration_seconds: f32,
) -> ([f32; 3], f32, f32, f32) {
    if timeline.samples.is_empty() || duration_seconds <= 0.1 {
        return ([0.0; 3], 0.0, 0.0, 0.0);
    }

    let progress = (position_seconds / duration_seconds).clamp(0.0, 0.999_999);
    let last = timeline.samples.len().saturating_sub(1);
    let position = progress * last as f32;
    let index = position.floor() as usize;
    let next = (index + 1).min(last);
    let fraction = position - index as f32;
    let a = timeline.samples[index];
    let b = timeline.samples[next];
    let current = [
        a[0] + (b[0] - a[0]) * fraction,
        a[1] + (b[1] - a[1]) * fraction,
        a[2] + (b[2] - a[2]) * fraction,
    ];

    let radius = 7usize;
    let start = index.saturating_sub(radius);
    let end = (index + radius).min(last);
    let mut mean = [0.0f32; 3];
    let mut count = 0.0f32;
    for sample in &timeline.samples[start..=end] {
        mean[0] += sample[0];
        mean[1] += sample[1];
        mean[2] += sample[2];
        count += 1.0;
    }
    if count > 0.0 {
        mean[0] /= count;
        mean[1] /= count;
        mean[2] /= count;
    }

    let energy = (current[0] * 0.30 + current[1] * 0.42 + current[2] * 0.28).clamp(0.0, 1.0);
    let local_contrast = ((current[0] - mean[0]).abs()
        + (current[1] - mean[1]).abs()
        + (current[2] - mean[2]).abs())
        / 3.0;
    let lookahead = timeline.samples[(index + 10).min(last)];
    let change = (((lookahead[0] - current[0]).abs()
        + (lookahead[1] - current[1]).abs()
        + (lookahead[2] - current[2]).abs())
        / 3.0
        + local_contrast * 0.70)
        .clamp(0.0, 1.0);

    (current, energy, change, progress)
}

#[derive(Clone, Debug)]
struct TrackProfile {
    label: String,
    camera: f32,
    pulse: f32,
    color: f32,
    density: f32,
    apparition: f32,
    fluidity: f32,
    tags: String,
}

impl Default for TrackProfile {
    fn default() -> Self {
        Self {
            label: "balanced".to_string(),
            camera: 0.78,
            pulse: 1.0,
            color: 1.0,
            density: 1.0,
            apparition: 1.0,
            fluidity: 1.0,
            tags: String::new(),
        }
    }
}

impl TrackProfile {
    fn from_helper_output(output: &str) -> Option<Self> {
        let fields = output.trim().split('|').collect::<Vec<_>>();
        if fields.len() < 7 {
            return None;
        }

        let parse = |index: usize| fields.get(index)?.parse::<f32>().ok();
        Some(Self {
            label: fields[0].trim().to_string(),
            camera: parse(1)?.clamp(0.35, 1.35),
            pulse: parse(2)?.clamp(0.55, 1.55),
            color: parse(3)?.clamp(0.65, 1.45),
            density: parse(4)?.clamp(0.60, 1.50),
            apparition: parse(5)?.clamp(0.55, 1.45),
            fluidity: parse(6)?.clamp(0.60, 1.45),
            tags: fields
                .get(7)
                .copied()
                .unwrap_or_default()
                .trim()
                .to_string(),
        })
    }
}

#[derive(Clone, Debug)]
struct LyricSemanticFrame {
    found: bool,
    timed: bool,
    label: String,
    warmth: f32,
    coolness: f32,
    darkness: f32,
    intimacy: f32,
    motion: f32,
    tension: f32,
    release: f32,
    transcendence: f32,
    synthetic: f32,
    organic: f32,
    source: String,
}

impl Default for LyricSemanticFrame {
    fn default() -> Self {
        Self {
            found: false,
            timed: false,
            label: "none".to_string(),
            warmth: 0.0,
            coolness: 0.0,
            darkness: 0.0,
            intimacy: 0.0,
            motion: 0.0,
            tension: 0.0,
            release: 0.0,
            transcendence: 0.0,
            synthetic: 0.0,
            organic: 0.0,
            source: "none".to_string(),
        }
    }
}

impl LyricSemanticFrame {
    fn from_helper_output(output: &str) -> Option<Self> {
        let fields = output.trim().split('|').collect::<Vec<_>>();
        if fields.len() < 14 {
            return None;
        }

        let parse = |index: usize| fields.get(index)?.parse::<f32>().ok();
        Some(Self {
            found: fields[0] == "1",
            timed: fields[1] == "1",
            label: fields[2].trim().to_string(),
            warmth: parse(3)?.clamp(0.0, 1.0),
            coolness: parse(4)?.clamp(0.0, 1.0),
            darkness: parse(5)?.clamp(0.0, 1.0),
            intimacy: parse(6)?.clamp(0.0, 1.0),
            motion: parse(7)?.clamp(0.0, 1.0),
            tension: parse(8)?.clamp(0.0, 1.0),
            release: parse(9)?.clamp(0.0, 1.0),
            transcendence: parse(10)?.clamp(0.0, 1.0),
            synthetic: parse(11)?.clamp(0.0, 1.0),
            organic: parse(12)?.clamp(0.0, 1.0),
            source: fields[13].trim().to_string(),
        })
    }
}

fn infer_track_profile(text: &str) -> TrackProfile {
    let lowered = text.to_lowercase();
    let contains_any = |words: &[&str]| words.iter().any(|word| lowered.contains(word));

    if contains_any(&[
        "ambient",
        "downtempo",
        "drone",
        "dream pop",
        "ethereal",
        "new age",
        "classical",
        "soundtrack",
        "meditation",
    ]) {
        TrackProfile {
            label: "atmospheric".to_string(),
            camera: 0.62,
            pulse: 0.72,
            color: 1.18,
            density: 0.82,
            apparition: 1.18,
            fluidity: 1.28,
            tags: text.to_string(),
        }
    } else if contains_any(&[
        "techno",
        "house",
        "trance",
        "dance",
        "disco",
        "electronic",
        "drum and bass",
        "dnb",
        "breakbeat",
        "edm",
    ]) {
        TrackProfile {
            label: "kinetic".to_string(),
            camera: 0.72,
            pulse: 1.34,
            color: 1.12,
            density: 1.16,
            apparition: 0.92,
            fluidity: 0.88,
            tags: text.to_string(),
        }
    } else if contains_any(&[
        "metal",
        "punk",
        "hardcore",
        "noise",
        "industrial",
        "grindcore",
        "post-hardcore",
    ]) {
        TrackProfile {
            label: "dense".to_string(),
            camera: 0.48,
            pulse: 1.42,
            color: 0.94,
            density: 1.38,
            apparition: 0.72,
            fluidity: 0.72,
            tags: text.to_string(),
        }
    } else if contains_any(&[
        "hip hop", "hip-hop", "rap", "trip hop", "trip-hop", "funk", "dub", "reggae", "soul", "r&b",
    ]) {
        TrackProfile {
            label: "bass-led".to_string(),
            camera: 0.68,
            pulse: 1.18,
            color: 1.06,
            density: 1.02,
            apparition: 1.00,
            fluidity: 0.94,
            tags: text.to_string(),
        }
    } else if contains_any(&[
        "jazz",
        "progressive",
        "experimental",
        "psychedelic",
        "avant-garde",
        "math rock",
    ]) {
        TrackProfile {
            label: "dynamic".to_string(),
            camera: 0.76,
            pulse: 1.02,
            color: 1.22,
            density: 0.96,
            apparition: 1.08,
            fluidity: 1.16,
            tags: text.to_string(),
        }
    } else {
        TrackProfile::default()
    }
}

fn mix_local(a: f32, b: f32, amount: f32) -> f32 {
    a + (b - a) * amount.clamp(0.0, 1.0)
}

fn smoothstep_local(edge0: f32, edge1: f32, value: f32) -> f32 {
    if (edge1 - edge0).abs() < f32::EPSILON {
        return if value < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn follow_local(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    current + (target - current) * (1.0 - (-rate.max(0.0) * dt.max(0.0)).exp())
}

fn stable_unit_local(mut value: u32) -> f32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    value as f32 / u32::MAX as f32
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let hue = hue.rem_euclid(1.0) * 6.0;
    let sector = hue.floor() as i32;
    let fraction = hue - sector as f32;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * fraction);
    let t = value * (1.0 - saturation * (1.0 - fraction));

    match sector.rem_euclid(6) {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}

#[derive(Clone, Debug)]
struct MusicTelemetry {
    capture_available: bool,
    live: bool,
    overall: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    stereo: f32,
    onset: f32,
    tempo_bpm: f32,
    beat_confidence: f32,
    busyness: f32,
    beat_serial: u64,
    strawberry_available: bool,
    strawberry_playing: bool,
    track: String,
    track_serial: u64,
    profile: TrackProfile,
    lyric_semantics: LyricSemanticFrame,
    moodbar: MoodbarTimeline,
    playback_position: f32,
    track_duration: f32,
}

impl Default for MusicTelemetry {
    fn default() -> Self {
        Self {
            capture_available: false,
            live: false,
            overall: 0.0,
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            stereo: 0.0,
            onset: 0.0,
            tempo_bpm: 120.0,
            beat_confidence: 0.0,
            busyness: 0.0,
            beat_serial: 0,
            strawberry_available: false,
            strawberry_playing: false,
            track: String::new(),
            track_serial: 0,
            profile: TrackProfile::default(),
            lyric_semantics: LyricSemanticFrame::default(),
            moodbar: MoodbarTimeline::default(),
            playback_position: 0.0,
            track_duration: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MusicInspectorFrame {
    pub active: bool,
    pub track: String,
    pub overall: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub stereo: f32,
    pub onset: f32,
    pub tempo_bpm: f32,
    pub beat_confidence: f32,
    pub busyness: f32,
    pub performance: f32,
    pub section_energy: f32,
    pub adaptive_floor: f32,
    pub adaptive_peak: f32,
    pub moodbar_rgb: [f32; 3],
    pub moodbar_energy: f32,
    pub moodbar_change: f32,
    pub moodbar_progress: f32,
    pub beat_pulse: f32,
    pub track_pulse: f32,
    pub beat_phase: f32,
    pub bar_phase: f32,
    pub color_phase: f32,
    pub color_accent: f32,
    pub camera_section: f32,
    pub climax_pulse: f32,
    pub structure_drive: f32,
    pub detail_drive: f32,
    pub call_response_balance: f32,
    pub signature_event: f32,
    pub speed_multiplier: f32,
    pub glow_multiplier: f32,
    pub exposure_multiplier: f32,
    pub fov_offset: f32,
    pub cascade_multiplier: f32,
    pub coupling_multiplier: f32,
    pub rain_density_multiplier: f32,
    pub rain_energy_multiplier: f32,
    pub head_activity_multiplier: f32,
    pub glyph_variation: f32,
    pub palette_mix: f32,
    pub wallpaper_color_multiplier: f32,
    pub apparition_frequency_multiplier: f32,
    pub apparition_opacity_multiplier: f32,
    pub media_cycle_multiplier: f32,
    pub cinematic_tempo_multiplier: f32,
    pub stereo_camera_drift: f32,
    pub spatial_strength: f32,
    pub rain_visibility_floor: f32,
    pub primary_palette: [f32; 3],
    pub secondary_palette: [f32; 3],
    pub timeline_confidence: f32,
    pub timeline_change: f32,
    pub timeline_novelty: f32,
    pub timeline_trend: f32,
    pub operating_state: String,
    pub timeline_lookahead: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConductorGesture {
    Breathe,
    SwayLeft,
    SwayRight,
    StepLeft,
    StepRight,
    Lunge,
    Rise,
    Drop,
    TurnLeft,
    TurnRight,
    Orbit,
    Stillness,
}

impl ConductorGesture {
    fn label(self) -> &'static str {
        match self {
            Self::Breathe => "breathe",
            Self::SwayLeft => "sway-left",
            Self::SwayRight => "sway-right",
            Self::StepLeft => "step-left",
            Self::StepRight => "step-right",
            Self::Lunge => "lunge",
            Self::Rise => "rise",
            Self::Drop => "drop",
            Self::TurnLeft => "turn-left",
            Self::TurnRight => "turn-right",
            Self::Orbit => "orbit",
            Self::Stillness => "stillness",
        }
    }
}

pub struct MusicReactor {
    enabled: bool,
    intensity: MusicIntensity,
    source_mode: MusicSourceMode,
    color_mode: MusicColorMode,
    shared: Arc<Mutex<MusicTelemetry>>,
    capture_available: bool,
    active: bool,
    strawberry_available: bool,
    strawberry_playing: bool,
    track: String,
    overall: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    stereo: f32,
    onset: f32,
    tempo_bpm: f32,
    beat_confidence: f32,
    busyness: f32,
    beat_phase: f32,
    bar_phase: f32,
    beat_counter: u32,
    profile: TrackProfile,
    lyric_found: bool,
    lyric_timed: bool,
    lyric_label: String,
    lyric_source: String,
    lyric_warmth: f32,
    lyric_coolness: f32,
    lyric_darkness: f32,
    lyric_intimacy: f32,
    lyric_motion: f32,
    lyric_tension: f32,
    lyric_release: f32,
    lyric_transcendence: f32,
    lyric_synthetic: f32,
    lyric_organic: f32,
    performance: f32,
    section_energy: f32,
    adaptive_floor: f32,
    adaptive_peak: f32,
    moodbar_rgb: [f32; 3],
    moodbar_energy: f32,
    moodbar_change: f32,
    moodbar_progress: f32,
    moodbar_available: bool,
    moodbar_source: String,
    beat_pulse: f32,
    track_pulse: f32,
    color_phase: f32,
    color_accent: f32,
    camera_section: f32,
    climax_pulse: f32,
    last_beat_serial: u64,
    last_track_serial: u64,
    conductor_energy: f32,
    conductor_tension: f32,
    conductor_release: f32,
    conductor_openness: f32,
    conductor_novelty: f32,
    conductor_confidence: f32,
    conductor_intimacy: f32,
    conductor_momentum: f32,
    conductor_restlessness: f32,
    conductor_transcendence: f32,
    conductor_anticipation: f32,
    conductor_residual: f32,
    conductor_stillness: f32,
    conductor_habituation: f32,
    conductor_surprise: f32,
    conductor_gesture: ConductorGesture,
    conductor_gesture_phase: f32,
    conductor_gesture_duration: f32,
    conductor_gesture_strength: f32,
    conductor_gesture_serial: u32,
    conductor_last_gesture_beat: u32,
    conductor_previous_signature: [f32; 4],
    rolling_analysis: RollingAnalyzer,
    timeline_confidence: f32,
    timeline_change: f32,
    timeline_novelty: f32,
    timeline_trend: f32,
    operating_state: OperatingState,
    timeline_lookahead: f32,
}

impl MusicReactor {
    pub fn new() -> Self {
        let shared = Arc::new(Mutex::new(MusicTelemetry::default()));
        spawn_pipewire_capture(Arc::clone(&shared));
        spawn_strawberry_monitor(Arc::clone(&shared));
        println!(
            "Deployment resilience: confidence-aware live timeline enabled; metadata, lyrics, and moodbars are optional enrichment."
        );
        println!(
            "Runtime capabilities: PipeWire={} MPRIS={} moodbar-helper={} lyric-helper={} profile-helper={}",
            capability_flag("pw-record"),
            capability_flag("playerctl"),
            capability_flag("neon-rain-moodbar-profile"),
            capability_flag("neon-rain-lyric-runtime"),
            capability_flag("neon-rain-track-profile"),
        );

        Self {
            enabled: true,
            intensity: MusicIntensity::Intense,
            source_mode: MusicSourceMode::Strawberry,
            color_mode: MusicColorMode::Hybrid,
            shared,
            capture_available: false,
            active: false,
            strawberry_available: false,
            strawberry_playing: false,
            track: String::new(),
            overall: 0.0,
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            stereo: 0.0,
            onset: 0.0,
            tempo_bpm: 120.0,
            beat_confidence: 0.0,
            busyness: 0.0,
            beat_phase: 0.0,
            bar_phase: 0.0,
            beat_counter: 0,
            profile: TrackProfile::default(),
            lyric_found: false,
            lyric_timed: false,
            lyric_label: "none".to_string(),
            lyric_source: "none".to_string(),
            lyric_warmth: 0.0,
            lyric_coolness: 0.0,
            lyric_darkness: 0.0,
            lyric_intimacy: 0.0,
            lyric_motion: 0.0,
            lyric_tension: 0.0,
            lyric_release: 0.0,
            lyric_transcendence: 0.0,
            lyric_synthetic: 0.0,
            lyric_organic: 0.0,
            performance: 0.0,
            section_energy: 0.0,
            adaptive_floor: 0.08,
            adaptive_peak: 0.55,
            moodbar_rgb: [0.0; 3],
            moodbar_energy: 0.0,
            moodbar_change: 0.0,
            moodbar_progress: 0.0,
            moodbar_available: false,
            moodbar_source: String::new(),
            beat_pulse: 0.0,
            track_pulse: 0.0,
            color_phase: 0.42,
            color_accent: 0.0,
            camera_section: 0.0,
            climax_pulse: 0.0,
            last_beat_serial: 0,
            last_track_serial: 0,
            conductor_energy: 0.0,
            conductor_tension: 0.0,
            conductor_release: 0.0,
            conductor_openness: 0.0,
            conductor_novelty: 0.0,
            conductor_confidence: 0.0,
            conductor_intimacy: 0.0,
            conductor_momentum: 0.0,
            conductor_restlessness: 0.0,
            conductor_transcendence: 0.0,
            conductor_anticipation: 0.0,
            conductor_residual: 0.0,
            conductor_stillness: 0.0,
            conductor_habituation: 0.0,
            conductor_surprise: 0.0,
            conductor_gesture: ConductorGesture::Breathe,
            conductor_gesture_phase: 1.0,
            conductor_gesture_duration: 3.0,
            conductor_gesture_strength: 0.0,
            conductor_gesture_serial: 0,
            conductor_last_gesture_beat: 0,
            conductor_previous_signature: [0.0; 4],
            rolling_analysis: RollingAnalyzer::new(),
            timeline_confidence: 0.0,
            timeline_change: 0.0,
            timeline_novelty: 0.0,
            timeline_trend: 0.0,
            operating_state: OperatingState::Autonomous,
            timeline_lookahead: 0.0,
        }
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        println!(
            "Music reactivity: {}",
            if self.enabled { "on" } else { "off" }
        );
    }

    pub fn cycle_intensity(&mut self) {
        self.intensity = self.intensity.next();
        println!("Music intensity: {}", self.intensity.label());
    }

    pub fn cycle_source(&mut self) {
        self.source_mode = self.source_mode.next();
        println!("Music source mode: {}", self.source_mode.label());
    }

    pub fn cycle_color_mode(&mut self) {
        self.color_mode = self.color_mode.next();
        println!("Music color mode: {}", self.color_mode.label());
    }

    pub fn update(&mut self, dt: f32) {
        let snapshot = self
            .shared
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();

        self.capture_available = snapshot.capture_available;
        self.strawberry_available = snapshot.strawberry_available;
        self.strawberry_playing = snapshot.strawberry_playing;
        self.track = snapshot.track;
        self.profile = snapshot.profile;
        self.lyric_found = snapshot.lyric_semantics.found;
        self.lyric_timed = snapshot.lyric_semantics.timed;
        self.lyric_label = snapshot.lyric_semantics.label.clone();
        self.lyric_source = snapshot.lyric_semantics.source.clone();
        self.moodbar_available = !snapshot.moodbar.samples.is_empty();
        self.moodbar_source = snapshot.moodbar.source.clone();

        let source_gate = match self.source_mode {
            MusicSourceMode::System => snapshot.live,
            MusicSourceMode::Strawberry => {
                snapshot.live && (!snapshot.strawberry_available || snapshot.strawberry_playing)
            }
        };
        self.active = self.enabled && self.capture_available && source_gate;

        let gate = if self.active { 1.0 } else { 0.0 };
        let target_overall = snapshot.overall * gate;
        let target_bass = snapshot.bass * gate;
        let target_mid = snapshot.mid * gate;
        let target_treble = snapshot.treble * gate;
        let target_stereo = snapshot.stereo * gate;
        let target_onset = snapshot.onset * gate;
        let target_tempo = snapshot.tempo_bpm;
        let target_confidence = snapshot.beat_confidence * gate;
        let target_busyness = snapshot.busyness * gate;
        let lyric_authority = if snapshot.lyric_semantics.found {
            if snapshot.lyric_semantics.timed {
                1.0
            } else {
                0.34
            }
        } else {
            0.0
        } * gate;

        let dt = dt.max(0.0).min(0.1);
        let follow = |current: f32, target: f32, attack: f32, release: f32| {
            let rate = if target > current { attack } else { release };
            current + (target - current) * (1.0 - (-rate * dt).exp())
        };

        self.overall = follow(self.overall, target_overall, 13.0, 3.0);
        self.bass = follow(self.bass, target_bass, 11.0, 2.6);
        self.mid = follow(self.mid, target_mid, 10.0, 3.2);
        self.treble = follow(self.treble, target_treble, 18.0, 5.5);
        self.stereo = follow(self.stereo, target_stereo, 3.0, 1.6);
        self.onset = follow(self.onset, target_onset, 28.0, 8.0);
        self.tempo_bpm = follow(self.tempo_bpm, target_tempo, 1.6, 0.45).clamp(45.0, 210.0);
        self.beat_confidence = follow(self.beat_confidence, target_confidence, 2.8, 0.55);
        self.busyness = follow(self.busyness, target_busyness, 3.2, 0.72);
        self.lyric_warmth = follow(
            self.lyric_warmth,
            snapshot.lyric_semantics.warmth * lyric_authority,
            2.4,
            0.82,
        );
        self.lyric_coolness = follow(
            self.lyric_coolness,
            snapshot.lyric_semantics.coolness * lyric_authority,
            2.4,
            0.82,
        );
        self.lyric_darkness = follow(
            self.lyric_darkness,
            snapshot.lyric_semantics.darkness * lyric_authority,
            1.9,
            0.68,
        );
        self.lyric_intimacy = follow(
            self.lyric_intimacy,
            snapshot.lyric_semantics.intimacy * lyric_authority,
            1.7,
            0.58,
        );
        self.lyric_motion = follow(
            self.lyric_motion,
            snapshot.lyric_semantics.motion * lyric_authority,
            2.6,
            0.90,
        );
        self.lyric_tension = follow(
            self.lyric_tension,
            snapshot.lyric_semantics.tension * lyric_authority,
            2.8,
            0.74,
        );
        self.lyric_release = follow(
            self.lyric_release,
            snapshot.lyric_semantics.release * lyric_authority,
            2.0,
            0.64,
        );
        self.lyric_transcendence = follow(
            self.lyric_transcendence,
            snapshot.lyric_semantics.transcendence * lyric_authority,
            1.8,
            0.60,
        );
        self.lyric_synthetic = follow(
            self.lyric_synthetic,
            snapshot.lyric_semantics.synthetic * lyric_authority,
            1.7,
            0.56,
        );
        self.lyric_organic = follow(
            self.lyric_organic,
            snapshot.lyric_semantics.organic * lyric_authority,
            1.7,
            0.56,
        );

        let (external_rgb, external_energy, external_change, external_progress) = sample_moodbar(
            &snapshot.moodbar,
            snapshot.playback_position,
            snapshot.track_duration,
        );
        let external_available = self.active && !snapshot.moodbar.samples.is_empty();
        let rolling = self.rolling_analysis.update(
            dt,
            self.capture_available,
            self.active,
            snapshot.strawberry_available,
            external_available || snapshot.lyric_semantics.found,
            self.track.as_str(),
            snapshot.playback_position,
            snapshot.track_duration,
            self.overall,
            self.bass,
            self.mid,
            self.treble,
            self.onset,
            self.busyness,
            self.beat_confidence,
            self.section_energy,
            self.performance,
        );

        let external_confidence = if external_available { 0.96 } else { 0.0 };
        let learned_confidence = rolling.learned_confidence * (1.0 - external_confidence * 0.88);
        let rolling_confidence = rolling.energy.confidence;
        let rolling_weight = rolling_confidence
            * (1.0 - external_confidence * 0.72)
            * (1.0 - learned_confidence * 0.58);
        let total_weight = (external_confidence + learned_confidence + rolling_weight).max(0.0001);
        let fused_energy = (external_energy * external_confidence
            + rolling.learned_energy * learned_confidence
            + rolling.energy.value * rolling_weight)
            / total_weight;
        let fused_change = (external_change * external_confidence
            + rolling.learned_change * learned_confidence
            + rolling.change.value * rolling_weight)
            / total_weight;
        let fused_rgb = [
            (external_rgb[0] * external_confidence
                + rolling.learned_rgb[0] * learned_confidence
                + rolling.rgb[0] * rolling_weight)
                / total_weight,
            (external_rgb[1] * external_confidence
                + rolling.learned_rgb[1] * learned_confidence
                + rolling.rgb[1] * rolling_weight)
                / total_weight,
            (external_rgb[2] * external_confidence
                + rolling.learned_rgb[2] * learned_confidence
                + rolling.rgb[2] * rolling_weight)
                / total_weight,
        ];

        self.timeline_confidence =
            (external_confidence + learned_confidence + rolling_weight).clamp(0.0, 1.0);
        self.timeline_change = fused_change.clamp(0.0, 1.0);
        self.timeline_novelty = rolling.novelty.value;
        self.timeline_trend = rolling.trend.value;
        self.timeline_lookahead = rolling.lookahead_release;
        self.operating_state = rolling.state;
        self.moodbar_available = external_available || rolling_confidence > 0.10;
        self.moodbar_source = if external_available {
            snapshot.moodbar.source.clone()
        } else if learned_confidence > 0.18 {
            "learned-cache".to_string()
        } else if rolling_confidence > 0.10 {
            "live-history".to_string()
        } else {
            "unavailable".to_string()
        };

        for channel in 0..3 {
            self.moodbar_rgb[channel] =
                follow(self.moodbar_rgb[channel], fused_rgb[channel], 4.2, 1.25);
        }
        self.moodbar_energy = follow(self.moodbar_energy, fused_energy, 3.8, 0.95);
        self.moodbar_change = follow(self.moodbar_change, fused_change, 6.2, 1.45);
        self.moodbar_progress = if external_available {
            external_progress
        } else {
            rolling.progress
        };

        let beat_period = (60.0 / self.tempo_bpm.max(1.0)).clamp(0.285, 1.333);
        self.beat_phase = (self.beat_phase + dt / beat_period).rem_euclid(1.0);
        self.bar_phase = (self.bar_phase + dt / (beat_period * 4.0)).rem_euclid(1.0);

        if self.active {
            let floor_rate = if self.overall < self.adaptive_floor {
                2.4
            } else {
                0.08
            };
            let peak_rate = if self.overall > self.adaptive_peak {
                10.0
            } else {
                0.18
            };

            self.adaptive_floor +=
                (self.overall - self.adaptive_floor) * (1.0 - (-floor_rate * dt).exp());
            self.adaptive_peak +=
                (self.overall - self.adaptive_peak) * (1.0 - (-peak_rate * dt).exp());

            self.adaptive_floor = self.adaptive_floor.clamp(0.0, 0.72);
            self.adaptive_peak = self.adaptive_peak.clamp(self.adaptive_floor + 0.12, 1.0);

            let adaptive_span = (self.adaptive_peak - self.adaptive_floor).max(0.14);
            let normalized = ((self.overall - self.adaptive_floor + 0.018) / adaptive_span)
                .clamp(0.0, 1.0)
                .powf(0.72);

            self.performance = follow(self.performance, normalized, 9.0, 1.55);
            self.section_energy = follow(self.section_energy, normalized, 1.35, 0.34);
            self.camera_section = follow(
                self.camera_section,
                (normalized * 0.72 + self.moodbar_energy * 0.28).clamp(0.0, 1.0),
                0.55,
                0.18,
            );
        } else {
            self.performance = follow(self.performance, 0.0, 3.0, 3.0);
            self.section_energy = follow(self.section_energy, 0.0, 1.2, 1.2);
            self.camera_section = follow(self.camera_section, 0.0, 1.0, 1.0);
        }

        self.climax_pulse *= (-1.28 * dt).exp();

        let palette_motion =
            0.018 + self.performance * 0.060 + self.mid * 0.055 + self.treble * 0.085;
        self.color_phase = (self.color_phase + dt * palette_motion).rem_euclid(1.0);
        self.color_accent *= (-2.4 * dt).exp();

        self.beat_pulse *= (-5.4 * dt).exp();
        if self.active && snapshot.beat_serial != self.last_beat_serial {
            self.last_beat_serial = snapshot.beat_serial;
            self.beat_pulse = (0.82 + self.onset * 0.52).clamp(0.0, 1.35);
            self.beat_phase = 0.0;
            self.beat_counter = self.beat_counter.wrapping_add(1);
            if self.beat_counter % 4 == 0 {
                self.bar_phase = 0.0;
            }
            if self.beat_counter % 8 == 0
                && self.camera_section > 0.56
                && self.beat_confidence > 0.54
                && self.moodbar_change > 0.08
            {
                self.climax_pulse = 1.0;
            }
            self.color_accent = 1.0;
            self.color_phase = (self.color_phase + 0.032 + self.mid * 0.040).rem_euclid(1.0);
        }

        self.track_pulse *= (-1.8 * dt).exp();
        if snapshot.track_serial != self.last_track_serial {
            self.last_track_serial = snapshot.track_serial;
            if self.enabled && snapshot.strawberry_playing {
                self.track_pulse = 1.0;
                self.color_accent = 1.0;
                self.color_phase = (self.color_phase + 0.16).rem_euclid(1.0);
            }
        }

        self.update_living_conductor(dt);
    }

    fn conductor_phrase_ready(&self) -> bool {
        if !self.active {
            return false;
        }

        let start_or_end = self.bar_phase.min(1.0 - self.bar_phase) <= 0.085;
        let midpoint = (self.bar_phase - 0.5).abs() <= 0.060;
        let rhythmic_trust = self.beat_confidence > 0.32;
        let movement_signal = self.structure_drive() > 0.14
            || self.transient_drive() > 0.16
            || self.conductor_release > 0.34;

        rhythmic_trust && movement_signal && (start_or_end || midpoint)
    }

    fn update_living_conductor(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return;
        }

        let signature = [self.overall, self.bass, self.mid, self.treble];
        let signature_change = signature
            .iter()
            .zip(self.conductor_previous_signature.iter())
            .map(|(current, previous)| (current - previous).abs())
            .sum::<f32>()
            / 4.0;
        self.conductor_previous_signature = signature;

        let performance = self.performance_drive();
        let transient = self.transient_drive();
        let structure = self.structure_drive();
        let lyric_strength = self.lyric_guidance_strength();
        let rising_energy = (performance - self.conductor_energy).max(0.0);
        let falling_energy = (self.conductor_energy - performance).max(0.0);

        let energy_target = if self.active {
            (self.section_energy * 0.42
                + performance * 0.26
                + self.bass * 0.12
                + self.overall * 0.10
                + self.moodbar_energy * 0.10)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let novelty_target = if self.active {
            (signature_change * 2.4
                + self.moodbar_change * 0.42
                + self.onset * 0.24
                + self.track_pulse * 0.26)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let repetition =
            (1.0 - novelty_target).clamp(0.0, 1.0) * (0.42 + self.beat_confidence * 0.58);
        self.conductor_habituation = follow_local(
            self.conductor_habituation,
            repetition,
            if repetition > self.conductor_habituation {
                0.85
            } else {
                2.1
            },
            dt,
        );

        let surprise_target = (novelty_target * (1.0 - self.conductor_habituation * 0.72)
            + self.moodbar_change * 0.28
            + self.track_pulse * 0.36)
            .clamp(0.0, 1.0);

        let tension_target = if self.active {
            (rising_energy * 1.10
                + self.busyness * 0.22
                + structure * 0.20
                + self.lyric_tension * lyric_strength * 0.26
                + (1.0 - self.lyric_release * lyric_strength) * self.onset * 0.10
                - falling_energy * 0.18)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let phrase_edge = self.bar_pulse();
        let release_target = if self.active {
            (phrase_edge * (self.conductor_tension * 0.58 + transient * 0.38)
                + self.lyric_release * lyric_strength * 0.34
                + self.climax_pulse * 0.44
                + falling_energy * 0.12)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let confidence_target = if self.active {
            (self.beat_confidence * 0.46
                + structure * 0.24
                + (1.0 - self.busyness) * 0.12
                + performance * 0.18)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let intimacy_target = if self.active {
            (self.lyric_intimacy * lyric_strength * 0.56
                + (1.0 - energy_target) * 0.28
                + self.lyric_organic * lyric_strength * 0.10
                + (1.0 - self.busyness) * 0.06)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let transcendence_target = if self.active {
            (self.lyric_transcendence * lyric_strength * 0.54
                + self.moodbar_energy * 0.14
                + release_target * 0.18
                + self.treble * 0.14)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let openness_target = if self.active {
            (0.24 + release_target * 0.34 + transcendence_target * 0.28 + self.treble * 0.12
                - tension_target * 0.24
                - intimacy_target * 0.08)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let momentum_target = if self.active {
            (energy_target * 0.42 + self.bass * 0.18 + structure * 0.22 + release_target * 0.18)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let restlessness_target = if self.active {
            (self.busyness * 0.34
                + novelty_target * 0.28
                + self.onset * 0.16
                + self.lyric_motion_axis() * 0.14
                - confidence_target * 0.16)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let anticipation_target = if self.active {
            (tension_target * (1.0 - phrase_edge) * 0.56
                + rising_energy * 0.32
                + self.moodbar_change * 0.18
                + self.timeline_lookahead * 0.22
                - release_target * 0.46)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };

        let stillness_target = if self.active {
            ((1.0 - energy_target) * 0.34
                + intimacy_target * 0.32
                + anticipation_target * 0.20
                + confidence_target * 0.10
                - restlessness_target * 0.30
                - release_target * 0.42)
                .clamp(0.0, 1.0)
        } else {
            0.62
        };

        self.conductor_energy = follow_local(self.conductor_energy, energy_target, 1.35, dt);
        self.conductor_tension = follow_local(
            self.conductor_tension,
            tension_target,
            if tension_target > self.conductor_tension {
                1.15
            } else {
                0.62
            },
            dt,
        );
        self.conductor_release = follow_local(
            self.conductor_release,
            release_target,
            if release_target > self.conductor_release {
                5.8
            } else {
                1.25
            },
            dt,
        );
        self.conductor_openness = follow_local(self.conductor_openness, openness_target, 0.92, dt);
        self.conductor_novelty = follow_local(
            self.conductor_novelty,
            novelty_target,
            if novelty_target > self.conductor_novelty {
                4.8
            } else {
                1.35
            },
            dt,
        );
        self.conductor_confidence =
            follow_local(self.conductor_confidence, confidence_target, 0.78, dt);
        self.conductor_intimacy = follow_local(self.conductor_intimacy, intimacy_target, 0.72, dt);
        self.conductor_momentum = follow_local(self.conductor_momentum, momentum_target, 1.08, dt);
        self.conductor_restlessness =
            follow_local(self.conductor_restlessness, restlessness_target, 1.22, dt);
        self.conductor_transcendence =
            follow_local(self.conductor_transcendence, transcendence_target, 0.74, dt);
        self.conductor_anticipation = follow_local(
            self.conductor_anticipation,
            anticipation_target,
            if anticipation_target > self.conductor_anticipation {
                1.5
            } else {
                2.2
            },
            dt,
        );
        self.conductor_stillness = follow_local(
            self.conductor_stillness,
            stillness_target,
            if stillness_target > self.conductor_stillness {
                0.82
            } else {
                2.5
            },
            dt,
        );
        self.conductor_surprise = follow_local(
            self.conductor_surprise,
            surprise_target,
            if surprise_target > self.conductor_surprise {
                6.2
            } else {
                1.5
            },
            dt,
        );

        self.conductor_residual *= (-0.58 * dt).exp();
        let consequence = (self.conductor_release * 0.66
            + self.conductor_surprise * 0.24
            + self.climax_pulse * 0.48)
            .clamp(0.0, 1.0);
        if consequence > 0.48 {
            self.conductor_residual = self.conductor_residual.max(consequence);
        }

        self.conductor_gesture_phase =
            (self.conductor_gesture_phase + dt / self.conductor_gesture_duration.max(0.4)).min(1.0);

        let phrase_ready = self.conductor_phrase_ready();
        let gesture_finished = self.conductor_gesture_phase >= 1.0;
        let new_beat = self.beat_counter != self.conductor_last_gesture_beat;
        if self.active && gesture_finished && new_beat && phrase_ready {
            self.conductor_last_gesture_beat = self.beat_counter;
            self.select_conductor_gesture();
        }
    }

    fn select_conductor_gesture(&mut self) {
        self.conductor_gesture_serial = self.conductor_gesture_serial.wrapping_add(1);
        let seed = self.conductor_gesture_serial
            ^ self.beat_counter.rotate_left(9)
            ^ (self.track.len() as u32).wrapping_mul(0x9e37_79b9);
        let choice = stable_unit_local(seed);
        let direction_left = stable_unit_local(seed ^ 0xa531_7c6d) < 0.5;

        self.conductor_gesture = if self.conductor_stillness > 0.64 && self.conductor_energy < 0.48
        {
            if self.conductor_intimacy > 0.55 {
                ConductorGesture::Breathe
            } else {
                ConductorGesture::Stillness
            }
        } else if self.conductor_release > 0.58 && self.conductor_tension > 0.38 {
            if self.conductor_transcendence > 0.54 {
                ConductorGesture::Rise
            } else if choice < 0.68 {
                ConductorGesture::Lunge
            } else {
                ConductorGesture::Drop
            }
        } else if self.conductor_surprise > 0.52 || self.conductor_novelty > 0.62 {
            if choice < 0.38 {
                if direction_left {
                    ConductorGesture::TurnLeft
                } else {
                    ConductorGesture::TurnRight
                }
            } else {
                if direction_left {
                    ConductorGesture::StepLeft
                } else {
                    ConductorGesture::StepRight
                }
            }
        } else if self.conductor_openness > 0.66 {
            if choice < 0.52 {
                ConductorGesture::Orbit
            } else {
                ConductorGesture::Rise
            }
        } else if self.conductor_restlessness > 0.52 {
            if direction_left {
                ConductorGesture::StepLeft
            } else {
                ConductorGesture::StepRight
            }
        } else if direction_left {
            ConductorGesture::SwayLeft
        } else {
            ConductorGesture::SwayRight
        };

        let beat_period = (60.0 / self.tempo_bpm.max(1.0)).clamp(0.285, 1.333);
        let beats = match self.conductor_gesture {
            ConductorGesture::Stillness => 8.0,
            ConductorGesture::Breathe => 6.0,
            ConductorGesture::Orbit => 8.0,
            ConductorGesture::Rise | ConductorGesture::Lunge | ConductorGesture::Drop => 4.0,
            ConductorGesture::TurnLeft | ConductorGesture::TurnRight => 4.0,
            ConductorGesture::StepLeft | ConductorGesture::StepRight => 3.0,
            ConductorGesture::SwayLeft | ConductorGesture::SwayRight => 6.0,
        };
        self.conductor_gesture_duration = (beat_period * beats).clamp(1.4, 7.0);
        self.conductor_gesture_strength = (0.24
            + self.conductor_confidence * 0.26
            + self.conductor_momentum * 0.22
            + self.conductor_release * 0.20
            + self.conductor_surprise * 0.14
            - self.conductor_stillness * 0.16)
            .clamp(0.12, 1.0);
        self.conductor_gesture_phase = 0.0;
    }

    fn conductor_gesture_envelope(&self) -> f32 {
        let phase = self.conductor_gesture_phase.clamp(0.0, 1.0);
        if phase < 0.18 {
            smoothstep_local(0.0, 0.18, phase) * 0.42
        } else if phase < 0.52 {
            mix_local(0.42, 1.0, smoothstep_local(0.18, 0.52, phase))
        } else {
            1.0 - smoothstep_local(0.52, 1.0, phase) * 0.88
        }
    }

    fn conductor_gesture_velocity(&self) -> [f32; 3] {
        let phase = self.conductor_gesture_phase.clamp(0.0, 1.0);
        let envelope = self.conductor_gesture_envelope() * self.conductor_gesture_strength;
        let recoil = ((phase - 0.68) * std::f32::consts::PI).sin().max(0.0) * 0.28;
        let wave = (phase * std::f32::consts::TAU).sin();

        match self.conductor_gesture {
            ConductorGesture::Breathe => [wave * 0.035, wave * 0.028, envelope * 0.075],
            ConductorGesture::SwayLeft => [-envelope * 0.17 + recoil * 0.05, wave * 0.018, 0.035],
            ConductorGesture::SwayRight => [envelope * 0.17 - recoil * 0.05, wave * 0.018, 0.035],
            ConductorGesture::StepLeft => [-envelope * 0.34 + recoil * 0.15, 0.0, envelope * 0.08],
            ConductorGesture::StepRight => [envelope * 0.34 - recoil * 0.15, 0.0, envelope * 0.08],
            ConductorGesture::Lunge => [
                wave * 0.035,
                -recoil * 0.035,
                envelope * 0.46 - recoil * 0.12,
            ],
            ConductorGesture::Rise => [
                wave * 0.025,
                envelope * 0.22 - recoil * 0.08,
                envelope * 0.18,
            ],
            ConductorGesture::Drop => [
                wave * 0.025,
                -envelope * 0.18 + recoil * 0.07,
                envelope * 0.20,
            ],
            ConductorGesture::TurnLeft => [
                -envelope * 0.22 + recoil * 0.08,
                wave * 0.012,
                envelope * 0.10,
            ],
            ConductorGesture::TurnRight => [
                envelope * 0.22 - recoil * 0.08,
                wave * 0.012,
                envelope * 0.10,
            ],
            ConductorGesture::Orbit => [
                wave * envelope * 0.26,
                wave.cos() * envelope * 0.11,
                envelope * 0.10,
            ],
            ConductorGesture::Stillness => [0.0, 0.0, 0.0],
        }
    }

    fn conductor_gesture_look(&self) -> [f32; 2] {
        let envelope = self.conductor_gesture_envelope() * self.conductor_gesture_strength;
        let phase = self.conductor_gesture_phase.clamp(0.0, 1.0);
        let wave = (phase * std::f32::consts::TAU).sin();

        match self.conductor_gesture {
            ConductorGesture::TurnLeft => [-envelope * 0.038, wave * 0.004],
            ConductorGesture::TurnRight => [envelope * 0.038, wave * 0.004],
            ConductorGesture::StepLeft => [-envelope * 0.014, 0.0],
            ConductorGesture::StepRight => [envelope * 0.014, 0.0],
            ConductorGesture::Rise => [wave * 0.004, -envelope * 0.014],
            ConductorGesture::Drop => [wave * 0.004, envelope * 0.012],
            ConductorGesture::Orbit => [wave * envelope * 0.018, wave.cos() * envelope * 0.008],
            ConductorGesture::Lunge => [wave * 0.004, -envelope * 0.006],
            _ => [wave * envelope * 0.004, 0.0],
        }
    }

    fn conductor_motion_suppression(&self) -> f32 {
        (1.0 - self.conductor_stillness * 0.72 - self.conductor_anticipation * 0.24)
            .clamp(0.18, 1.0)
    }

    pub fn conductor_status(&self) -> String {
        format!(
            "gesture:{} mood:e{:.2} t{:.2} r{:.2} o{:.2} n{:.2} c{:.2} still{:.2} anticipate{:.2} residual{:.2}",
            self.conductor_gesture.label(),
            self.conductor_energy,
            self.conductor_tension,
            self.conductor_release,
            self.conductor_openness,
            self.conductor_novelty,
            self.conductor_confidence,
            self.conductor_stillness,
            self.conductor_anticipation,
            self.conductor_residual,
        )
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn beat_pulse(&self) -> f32 {
        self.beat_pulse
    }

    fn performance_drive(&self) -> f32 {
        if self.active {
            let timeline = if self.moodbar_available {
                self.moodbar_energy
            } else {
                self.section_energy
            };
            (self.performance * 0.56 + self.overall * 0.18 + timeline * 0.26).clamp(0.0, 1.2)
        } else {
            0.0
        }
    }

    fn transient_drive(&self) -> f32 {
        self.beat_pulse.max(self.onset * 1.18).clamp(0.0, 1.35)
    }

    fn camera_clarity(&self) -> f32 {
        let busy_reduction = (1.0 - self.busyness * 0.72).clamp(0.24, 1.0);
        let rhythm_trust = (0.48 + self.beat_confidence * 0.52).clamp(0.48, 1.0);
        busy_reduction * rhythm_trust * self.profile.camera
    }

    fn bar_pulse(&self) -> f32 {
        let distance = self.bar_phase.min(1.0 - self.bar_phase);
        (1.0 - smoothstep_local(0.0, 0.20, distance)).clamp(0.0, 1.0)
    }

    pub fn track_identifier(&self) -> &str {
        &self.track
    }

    pub fn structure_drive(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        (self.camera_section * 0.58
            + self.section_energy * 0.22
            + self.moodbar_energy * 0.12
            + self.track_pulse * 0.08)
            .clamp(0.0, 1.0)
    }

    pub fn detail_drive(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        (self.transient_drive() * 0.48
            + self.treble * 0.20
            + self.mid * 0.16
            + self.performance_drive() * 0.16)
            .clamp(0.0, 1.0)
    }

    fn lyric_guidance_strength(&self) -> f32 {
        if !self.active || !self.lyric_found {
            return 0.0;
        }

        if self.lyric_timed { 1.0 } else { 0.34 }
    }

    fn lyric_warmth_axis(&self) -> f32 {
        (self.lyric_warmth - self.lyric_coolness).clamp(-1.0, 1.0) * self.lyric_guidance_strength()
    }

    fn lyric_light_axis(&self) -> f32 {
        ((self.lyric_release * 0.42 + self.lyric_transcendence * 0.58)
            - (self.lyric_darkness * 0.72 + self.lyric_tension * 0.18))
            .clamp(-1.0, 1.0)
            * self.lyric_guidance_strength()
    }

    fn lyric_motion_axis(&self) -> f32 {
        (self.lyric_motion * 0.72 + self.lyric_tension * 0.16 + self.lyric_transcendence * 0.18
            - self.lyric_intimacy * 0.10)
            .clamp(0.0, 1.0)
            * self.lyric_guidance_strength()
    }

    fn lyric_camera_scale(&self) -> f32 {
        (1.0 + self.lyric_motion_axis() * 0.18
            + self.lyric_transcendence * self.lyric_guidance_strength() * 0.12
            - self.lyric_intimacy * self.lyric_guidance_strength() * 0.16)
            .clamp(0.78, 1.26)
    }

    pub fn lyric_semantic_label(&self) -> &str {
        if self.lyric_found {
            self.lyric_label.as_str()
        } else {
            "none"
        }
    }

    pub fn call_response_balance(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let phrase = (self.bar_phase * std::f32::consts::TAU * 0.5).sin();
        let support = 0.42
            + self.detail_drive() * 0.30
            + self.structure_drive() * 0.22
            + self.lyric_motion_axis() * 0.10
            + self.lyric_synthetic * self.lyric_guidance_strength() * 0.06;
        (phrase * support).clamp(-1.0, 1.0)
    }

    pub fn call_response_at(&self, x_normalized: f32, depth_normalized: f32) -> f32 {
        if !self.active {
            return 0.5;
        }

        let side = x_normalized.clamp(-1.0, 1.0).signum();
        let conversation =
            (self.bar_phase * std::f32::consts::TAU * 0.5 + depth_normalized * 1.2).sin();
        let emphasis = 0.5 + 0.5 * (conversation * side * self.call_response_balance());
        emphasis.clamp(0.0, 1.0)
    }

    pub fn signature_event_strength(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let semantic_release = (self.lyric_release * 0.42
            + self.lyric_transcendence * 0.38
            + self.lyric_tension * 0.20)
            * self.lyric_guidance_strength();
        (self.climax_pulse
            * (0.52
                + self.structure_drive() * 0.28
                + self.beat_confidence * 0.14
                + semantic_release * 0.18))
            .clamp(0.0, 1.0)
    }

    pub fn speed_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        let drive = self.performance_drive();
        let transient = self.transient_drive();
        (1.0 + self.intensity.gain()
            * ((drive - 0.34) * 0.70
                + self.bass * 0.28
                + transient * 0.10
                + self.lyric_motion_axis() * 0.12))
            .clamp(0.70, 2.20)
    }

    pub fn glow_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        let drive = self.performance_drive();
        let transient = self.transient_drive();
        let semantic_light = self.lyric_light_axis();
        let living_afterglow = self.conductor_residual * 0.34
            + self.conductor_release * 0.20
            + self.conductor_transcendence * 0.12;
        (0.72
            + self.intensity.gain()
                * (drive * 0.70
                    + self.treble * 0.52
                    + transient * 0.38
                    + semantic_light.max(0.0) * 0.18
                    + self.lyric_tension * self.lyric_guidance_strength() * 0.08
                    + living_afterglow))
            .clamp(0.58, 3.45)
    }

    pub fn exposure_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        let drive = self.performance_drive();
        (0.86
            + self.intensity.gain()
                * (drive * 0.22
                    + self.track_pulse * 0.10
                    + self.transient_drive() * 0.055
                    + self.lyric_light_axis() * 0.08))
            .clamp(0.76, 1.58)
    }

    pub fn fov_offset(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let structure = self.structure_drive();
        (-self.intensity.gain() * (structure * 2.55 + self.bass * 0.42)).clamp(-6.2, 0.0)
    }

    pub fn cascade_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        (0.68
            + self.intensity.gain()
                * (self.mid * 0.70
                    + self.transient_drive() * 1.02
                    + self.performance_drive() * 0.36
                    + self.lyric_motion_axis() * 0.14
                    + self.lyric_tension * self.lyric_guidance_strength() * 0.08)
                * self.profile.pulse)
            .clamp(0.62, 3.35)
    }

    pub fn coupling_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        (0.68
            + self.intensity.gain()
                * (self.mid * 0.74
                    + self.performance_drive() * 0.52
                    + self.color_accent * 0.30
                    + self.transient_drive() * 0.18))
            .clamp(0.60, 2.70)
    }

    pub fn rain_density_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        let gather = self.conductor_anticipation * 0.16 + self.conductor_stillness * 0.12;
        let expansion = self.conductor_release * 0.22
            + self.conductor_openness * 0.12
            + self.conductor_residual * 0.08;
        (0.72
            + self.intensity.gain()
                * (self.mid * 0.70
                    + self.performance_drive() * 0.52
                    + self.transient_drive() * 0.18
                    + self.lyric_motion_axis() * 0.12
                    + self.lyric_tension * self.lyric_guidance_strength() * 0.07
                    + expansion
                    - gather)
                * self.profile.density)
            .clamp(0.58, 2.90)
    }

    pub fn rain_energy_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        let consequence = self.conductor_residual * 0.24
            + self.conductor_release * 0.20
            + self.conductor_surprise * 0.12;
        (0.64
            + self.intensity.gain()
                * (self.bass * 0.68
                    + self.mid * 0.34
                    + self.performance_drive() * 0.38
                    + self.transient_drive() * 0.18
                    + self.lyric_light_axis().max(0.0) * 0.10
                    + self.lyric_tension * self.lyric_guidance_strength() * 0.06
                    + consequence))
            .clamp(0.52, 3.05)
    }

    pub fn head_activity_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        (0.72
            + self.intensity.gain()
                * (self.treble * 0.80
                    + self.transient_drive() * 0.80
                    + self.performance_drive() * 0.27
                    + self.lyric_transcendence * self.lyric_guidance_strength() * 0.11)
                * self.profile.pulse)
            .clamp(0.65, 3.25)
    }

    pub fn glyph_variation_amount(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let attention = self.conductor_surprise * 0.24
            + self.conductor_novelty * 0.18
            + self.conductor_restlessness * 0.08;
        (self.intensity.gain()
            * (self.treble * 0.78
                + self.transient_drive() * 0.48
                + self.onset * 0.48
                + self.mid * 0.18
                + self.lyric_synthetic * self.lyric_guidance_strength() * 0.12
                + self.lyric_motion_axis() * 0.08
                + attention)
            * self.profile.pulse)
            .clamp(0.0, 1.0)
    }

    pub fn rain_phase(&self) -> f32 {
        self.color_phase
    }

    fn color_temperature(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        (self.section_energy * 0.45
            + self.performance_drive() * 0.25
            + self.bass * 0.15
            + self.busyness * 0.10
            + self.moodbar_energy * 0.05
            + self.conductor_tension * 0.06
            + self.conductor_release * 0.10)
            .clamp(0.0, 1.0)
    }

    fn transient_heat(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        (self.transient_drive() * 0.62
            + self.onset * 0.18
            + self.beat_pulse * 0.14
            + self.color_accent * 0.06
            + self.conductor_surprise * 0.12
            + self.conductor_release * 0.14)
            .clamp(0.0, 1.25)
    }

    fn cycle_drift_amount(&self) -> f32 {
        match self.color_mode {
            MusicColorMode::Wallpaper => 1.0,
            MusicColorMode::Palette => 0.08,
            MusicColorMode::Hybrid => 0.35,
        }
    }

    fn energy_anchor_hue(&self) -> f32 {
        let energy = self.color_temperature();
        if energy < 0.18 {
            mix_local(0.72, 0.60, energy / 0.18)
        } else if energy < 0.40 {
            mix_local(0.60, 0.50, (energy - 0.18) / 0.22)
        } else if energy < 0.68 {
            mix_local(0.50, 0.11, (energy - 0.40) / 0.28)
        } else {
            mix_local(0.11, 0.02, ((energy - 0.68) / 0.32).clamp(0.0, 1.0))
        }
    }

    pub fn color_mode(&self) -> MusicColorMode {
        self.color_mode
    }

    pub fn palette_mix(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        (self.intensity.gain()
            * (0.08
                + self.performance_drive() * 0.42
                + self.mid * 0.38
                + self.treble * 0.34
                + self.color_accent * 0.34
                + self.transient_drive() * 0.18)
            * self.profile.color)
            .clamp(0.0, 0.98)
    }

    pub fn wallpaper_color_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        (0.82
            + self.intensity.gain()
                * (self.mid * 0.70
                    + self.performance_drive() * 0.42
                    + self.transient_drive() * 0.12))
            .clamp(0.75, 2.55)
    }

    pub fn primary_palette_color(&self) -> [f32; 3] {
        let semantic_hue = -self.lyric_warmth_axis() * 0.055;
        let cycle_hue = self.color_phase + self.bass * 0.075 - self.treble * 0.052 + semantic_hue;
        let temperature = self.color_temperature();
        let transient_heat = self.transient_heat().clamp(0.0, 1.0);
        let anchor_hue = self.energy_anchor_hue() + semantic_hue * 0.45;
        let cycle_amount = self.cycle_drift_amount();
        let restrained_drift = (self.color_phase - 0.5) * mix_local(0.12, 0.035, temperature);

        let hue = match self.color_mode {
            MusicColorMode::Wallpaper => cycle_hue,
            MusicColorMode::Palette => {
                anchor_hue + restrained_drift * cycle_amount + self.treble * 0.010
                    - self.bass * 0.006
            }
            MusicColorMode::Hybrid => {
                mix_local(anchor_hue, cycle_hue, cycle_amount) + restrained_drift * 0.45
            }
        };

        let semantic_saturation = (self.lyric_tension * 0.10 + self.lyric_synthetic * 0.06
            - self.lyric_intimacy * 0.06
            - self.lyric_release * 0.04)
            * self.lyric_guidance_strength();

        let saturation = match self.color_mode {
            MusicColorMode::Wallpaper => {
                (0.68 + self.mid * 0.25 + transient_heat * 0.08 + semantic_saturation)
                    .clamp(0.38, 1.0)
            }
            MusicColorMode::Palette => (0.48
                + temperature * 0.28
                + transient_heat * 0.18
                + self.treble * 0.10
                + semantic_saturation)
                .clamp(0.28, 0.98),
            MusicColorMode::Hybrid => (0.56
                + temperature * 0.24
                + self.mid * 0.10
                + transient_heat * 0.12
                + semantic_saturation)
                .clamp(0.32, 1.0),
        };

        let value = match self.color_mode {
            MusicColorMode::Wallpaper => (0.72
                + self.performance_drive() * 0.42
                + self.color_accent * 0.22
                + transient_heat * 0.10
                + self.lyric_light_axis() * 0.18)
                .clamp(0.0, 1.48),
            MusicColorMode::Palette => (0.56
                + temperature * 0.34
                + self.performance_drive() * 0.24
                + transient_heat * 0.18
                + self.color_accent * 0.10
                + self.lyric_light_axis() * 0.16)
                .clamp(0.0, 1.52),
            MusicColorMode::Hybrid => (0.62
                + temperature * 0.26
                + self.performance_drive() * 0.28
                + transient_heat * 0.14
                + self.color_accent * 0.14
                + self.lyric_light_axis() * 0.17)
                .clamp(0.0, 1.50),
        };

        hsv_to_rgb(hue, saturation, value)
    }

    pub fn secondary_palette_color(&self) -> [f32; 3] {
        let semantic_hue = -self.lyric_warmth_axis() * 0.040;
        let cycle_hue =
            self.color_phase + 0.16 + self.treble * 0.10 - self.bass * 0.025 + semantic_hue;
        let temperature = self.color_temperature();
        let transient_heat = self.transient_heat().clamp(0.0, 1.0);
        let anchor_hue =
            self.energy_anchor_hue() + mix_local(0.09, 0.03, temperature) + semantic_hue * 0.40;
        let cycle_amount = self.cycle_drift_amount();
        let restrained_drift = (self.color_phase - 0.5) * mix_local(0.10, 0.028, temperature);

        let hue = match self.color_mode {
            MusicColorMode::Wallpaper => cycle_hue,
            MusicColorMode::Palette => {
                anchor_hue + restrained_drift * cycle_amount + self.treble * 0.012
            }
            MusicColorMode::Hybrid => {
                mix_local(anchor_hue, cycle_hue, cycle_amount * 0.90) + restrained_drift * 0.38
            }
        };

        let saturation = match self.color_mode {
            MusicColorMode::Wallpaper => (0.54
                + self.bass * 0.28
                + self.mid * 0.14
                + self.lyric_tension * self.lyric_guidance_strength() * 0.08
                - self.lyric_intimacy * self.lyric_guidance_strength() * 0.05)
                .clamp(0.34, 0.98),
            MusicColorMode::Palette => (0.40
                + temperature * 0.22
                + transient_heat * 0.14
                + self.bass * 0.08
                + self.lyric_tension * self.lyric_guidance_strength() * 0.06
                - self.lyric_intimacy * self.lyric_guidance_strength() * 0.04)
                .clamp(0.24, 0.92),
            MusicColorMode::Hybrid => (0.48
                + temperature * 0.20
                + transient_heat * 0.12
                + self.bass * 0.10
                + self.lyric_tension * self.lyric_guidance_strength() * 0.06
                - self.lyric_intimacy * self.lyric_guidance_strength() * 0.04)
                .clamp(0.28, 0.96),
        };

        let value = match self.color_mode {
            MusicColorMode::Wallpaper => (0.68
                + self.mid * 0.32
                + self.performance_drive() * 0.28
                + self.color_accent * 0.26
                + self.lyric_light_axis() * 0.14)
                .clamp(0.0, 1.46),
            MusicColorMode::Palette => (0.60
                + temperature * 0.28
                + self.mid * 0.18
                + self.performance_drive() * 0.18
                + transient_heat * 0.14
                + self.lyric_light_axis() * 0.14)
                .clamp(0.0, 1.48),
            MusicColorMode::Hybrid => (0.64
                + temperature * 0.22
                + self.mid * 0.20
                + self.performance_drive() * 0.20
                + transient_heat * 0.12
                + self.lyric_light_axis() * 0.14)
                .clamp(0.0, 1.48),
        };

        hsv_to_rgb(hue, saturation, value)
    }

    pub fn apparition_frequency_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        (0.72
            + self.intensity.gain()
                * (self.performance_drive() * 0.60
                    + self.transient_drive() * 0.26
                    + self.track_pulse * 0.40
                    + self.lyric_transcendence * self.lyric_guidance_strength() * 0.16
                    + self.lyric_intimacy * self.lyric_guidance_strength() * 0.10
                    - self.lyric_tension * self.lyric_guidance_strength() * 0.08)
                * self.profile.apparition)
            .clamp(0.58, 2.75)
    }

    pub fn apparition_opacity_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        (0.72
            + self.intensity.gain()
                * (self.performance_drive() * 0.40
                    + self.mid * 0.18
                    + self.track_pulse * 0.22
                    + self.lyric_intimacy * self.lyric_guidance_strength() * 0.12
                    + self.lyric_transcendence * self.lyric_guidance_strength() * 0.10
                    + self.lyric_light_axis().max(0.0) * 0.08)
                * self.profile.apparition)
            .clamp(0.62, 1.95)
    }

    pub fn media_cycle_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        (0.78
            + self.intensity.gain()
                * (self.section_energy * 0.30
                    + self.moodbar_change * 0.72
                    + self.track_pulse * 0.28))
            .clamp(0.72, 2.15)
    }

    pub fn cinematic_tempo_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        let busy_slowdown = (1.0 - self.busyness * 0.34).clamp(0.58, 1.0);
        ((0.86 + self.intensity.gain() * self.section_energy * 0.28)
            * busy_slowdown
            * self.profile.fluidity)
            .clamp(0.62, 1.38)
    }

    pub fn stereo_camera_drift(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let phrase_angle = self.bar_phase * std::f32::consts::TAU;
        let phrase_emphasis = smoothstep_local(0.18, 0.88, phrase_angle.sin().abs());
        self.stereo
            * self.intensity.gain()
            * (0.005 + self.section_energy * 0.006)
            * (0.42 + phrase_emphasis * 0.58)
    }

    pub fn cinematic_phrase_ready(&self) -> bool {
        if !self.active {
            return false;
        }

        let start_or_end = self.bar_phase.min(1.0 - self.bar_phase) <= 0.085;
        let midpoint = (self.bar_phase - 0.5).abs() <= 0.060;
        let rhythmic_trust = self.beat_confidence > 0.34;
        let movement_signal = self.structure_drive() > 0.16 || self.transient_drive() > 0.18;

        rhythmic_trust && movement_signal && (start_or_end || midpoint)
    }

    pub fn spatial_strength(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        (self.intensity.gain()
            * (0.20
                + self.performance_drive() * 0.52
                + self.mid * 0.20
                + self.treble * 0.16
                + self.transient_drive() * 0.12))
            .clamp(0.0, 1.0)
    }

    pub fn field_coordinates(&self, time: f32) -> [f32; 3] {
        if !self.active {
            return [0.0, 0.0, 0.0];
        }

        let phase = self.color_phase * std::f32::consts::TAU;
        let fluidity = self.profile.fluidity;
        let x = (time * (0.18 + self.mid * 0.15) * fluidity + phase * 0.72).sin();
        let y = (time * (0.14 + self.treble * 0.17) * fluidity + phase * 1.13 + 1.1).cos();
        let z = (time * (0.11 + self.bass * 0.13) * fluidity + phase * 0.49 + 2.2).sin();

        [
            (x * (0.48 + self.mid * 0.52) + self.stereo * 0.46).clamp(-1.0, 1.0),
            (y * (0.50 + self.treble * 0.50)).clamp(-1.0, 1.0),
            (z * 0.42 + (self.performance_drive() - 0.42) * 0.72 + self.bass * 0.34)
                .clamp(-1.0, 1.0),
        ]
    }

    pub fn camera_velocity_coordinates(&self, _time: f32) -> [f32; 3] {
        if !self.active {
            return [0.0, 0.0, 0.0];
        }

        let gain = self.intensity.gain();
        let clarity = self.camera_clarity();
        let structure = self.structure_drive();
        let performance = self.performance_drive();
        let bar_angle = self.bar_phase * std::f32::consts::TAU;
        let phrase_angle = bar_angle * 0.5;
        let macro_drive = structure * 0.74 + performance * 0.26;
        let sway = self.call_response_balance();
        let semantic_scale = self.lyric_camera_scale();
        let semantic_lift = self.lyric_transcendence * self.lyric_guidance_strength();
        let dance_step = smoothstep_local(-0.10, 0.85, bar_angle.sin());
        let dance_rebound = (bar_angle + std::f32::consts::FRAC_PI_2).sin();
        let suppression = self.conductor_motion_suppression();
        let gesture = self.conductor_gesture_velocity();
        let residual_push = self.conductor_residual * (0.04 + self.conductor_momentum * 0.08);

        let base = [
            ((phrase_angle.sin() * (0.030 + structure * 0.070)
                + dance_rebound * (0.014 + structure * 0.024)
                + sway * 0.072
                + self.stereo * 0.036)
                * gain
                * clarity
                * semantic_scale)
                .clamp(-0.22, 0.22),
            (((phrase_angle + 0.8).cos() * (0.010 + structure * 0.022) + semantic_lift * 0.009
                - self.lyric_darkness * self.lyric_guidance_strength() * 0.006)
                * gain
                * clarity
                * semantic_scale)
                .clamp(-0.065, 0.085),
            (((macro_drive - 0.32) * 0.58
                + self.bass * 0.07
                + dance_step * (0.025 + structure * 0.045)
                + self.bar_pulse() * 0.022
                + self.lyric_motion_axis() * 0.046)
                * gain
                * (0.78 + clarity * 0.22)
                * semantic_scale)
                .clamp(-0.05, 0.68),
        ];

        [
            (base[0] * suppression + gesture[0]).clamp(-0.52, 0.52),
            (base[1] * suppression + gesture[1]).clamp(-0.25, 0.27),
            (base[2] * suppression + gesture[2] + residual_push).clamp(-0.08, 1.10),
        ]
    }

    pub fn camera_look_coordinates(&self, _time: f32) -> [f32; 2] {
        if !self.active {
            return [0.0, 0.0];
        }

        let gain = self.intensity.gain();
        let clarity = self.camera_clarity();
        let structure = self.structure_drive();
        let phrase_angle = self.bar_phase * std::f32::consts::TAU * 0.5;
        let semantic_scale = self.lyric_camera_scale();
        let suppression = self.conductor_motion_suppression();
        let gesture = self.conductor_gesture_look();

        let base_yaw = ((phrase_angle.sin() * (0.0026 + structure * 0.0048)
            + self.stereo * 0.0030
            + self.call_response_balance() * 0.0035)
            * gain
            * clarity
            * semantic_scale
            * suppression)
            .clamp(-0.012, 0.012);
        let base_pitch = (((phrase_angle + 1.1).cos() * (0.0018 + structure * 0.0026)
            + self.lyric_transcendence * self.lyric_guidance_strength() * 0.0014)
            * gain
            * clarity
            * semantic_scale
            * suppression)
            .clamp(-0.006, 0.007);

        [
            (base_yaw + gesture[0]).clamp(-0.050, 0.050),
            (base_pitch + gesture[1]).clamp(-0.022, 0.022),
        ]
    }

    pub fn camera_fov_wave(&self, _time: f32) -> f32 {
        if !self.active {
            return 0.0;
        }

        let phrase_breath = (self.bar_phase * std::f32::consts::TAU * 0.5)
            .sin()
            .max(0.0);
        let structure = self.structure_drive();
        let clarity = self.camera_clarity();
        let semantic_open = (self.lyric_transcendence * 0.55
            + self.lyric_motion * 0.25
            + self.lyric_release * 0.20)
            * self.lyric_guidance_strength();
        let semantic_close = self.lyric_intimacy * self.lyric_guidance_strength();
        let anticipation_narrowing = self.conductor_anticipation * 1.35;
        let release_opening = self.conductor_release * 1.10 + self.conductor_residual * 0.42;
        let still_breath = self.conductor_stillness
            * (self.conductor_gesture_phase * std::f32::consts::TAU).sin()
            * 0.32;

        (-self.intensity.gain()
            * (self.bass * 0.24
                + structure * 0.40
                + phrase_breath * structure * 0.20 * clarity
                + semantic_open * 0.18
                - semantic_close * 0.10)
            + anticipation_narrowing
            - release_opening
            + still_breath)
            .clamp(-4.2, 2.0)
    }

    pub fn wallpaper_channel_gains(&self) -> [f32; 3] {
        if !self.active {
            return [1.0, 1.0, 1.0];
        }

        let primary = self.primary_palette_color();
        let secondary = self.secondary_palette_color();
        let strength = (0.10
            + self.performance_drive() * 0.15
            + self.mid * 0.08
            + self.transient_drive() * 0.035)
            * self.intensity.gain();
        let warmth = self.lyric_warmth_axis();
        let light = self.lyric_light_axis();

        [
            (1.0 + (primary[0] * 0.72 + secondary[0] * 0.28 - 0.58) * strength
                + warmth * 0.10
                + light * 0.035)
                .clamp(0.68, 1.46),
            (1.0 + (primary[1] * 0.72 + secondary[1] * 0.28 - 0.58) * strength
                + light * 0.045
                + self.lyric_organic * self.lyric_guidance_strength() * 0.035)
                .clamp(0.68, 1.46),
            (1.0 + (primary[2] * 0.72 + secondary[2] * 0.28 - 0.58) * strength - warmth * 0.10
                + light * 0.035)
                .clamp(0.68, 1.46),
        ]
    }

    pub fn image_palette_accent_mix(&self) -> f32 {
        if !self.active {
            return 0.0;
        }

        (0.04
            + self.intensity.gain()
                * (self.performance_drive() * 0.075
                    + self.treble * 0.045
                    + self.color_accent * 0.055))
            .clamp(0.04, 0.24)
    }

    pub fn apparition_image_light_multiplier(&self) -> f32 {
        if !self.active {
            return 1.0;
        }

        (0.84
            + self.intensity.gain()
                * (self.performance_drive() * 0.22
                    + self.moodbar_energy * 0.22
                    + self.bass * 0.10
                    + self.treble * 0.08
                    + self.transient_drive() * 0.06))
            .clamp(0.78, 1.62)
    }

    pub fn image_match_target(&self) -> Option<[f32; 4]> {
        if !self.active || (!self.moodbar_available && !self.lyric_found) {
            return None;
        }

        let maximum = self.moodbar_rgb[0]
            .max(self.moodbar_rgb[1])
            .max(self.moodbar_rgb[2]);
        let minimum = self.moodbar_rgb[0]
            .min(self.moodbar_rgb[1])
            .min(self.moodbar_rgb[2]);
        let spectral_chroma = if self.moodbar_available {
            (maximum - minimum).clamp(0.0, 1.0)
        } else {
            0.34
        };
        let mood_warmth = if self.moodbar_available {
            (0.5 + (self.moodbar_rgb[0] - self.moodbar_rgb[2]) * 0.58).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let lyric_strength = self.lyric_guidance_strength();
        let lyric_brightness =
            (0.52 + self.lyric_light_axis() * 0.34 + self.lyric_motion_axis() * 0.08)
                .clamp(0.08, 0.92);
        let mood_brightness = (0.14 + self.moodbar_energy * 0.68).clamp(0.10, 0.86);
        let brightness = mix_local(mood_brightness, lyric_brightness, lyric_strength * 0.42);
        let lyric_saturation = (0.28
            + self.lyric_tension * 0.32
            + self.lyric_transcendence * 0.24
            + self.lyric_synthetic * 0.18
            - self.lyric_intimacy * 0.10)
            .clamp(0.08, 0.94);
        let mood_saturation =
            (0.16 + spectral_chroma * 0.66 + self.moodbar_change * 0.12).clamp(0.10, 0.92);
        let saturation = mix_local(mood_saturation, lyric_saturation, lyric_strength * 0.40);
        let lyric_warmth = (0.5 + self.lyric_warmth_axis() * 0.46).clamp(0.0, 1.0);
        let warmth = mix_local(mood_warmth, lyric_warmth, lyric_strength * 0.52);
        let lyric_contrast = (0.12
            + self.lyric_tension * 0.42
            + self.lyric_motion * 0.18
            + self.lyric_synthetic * 0.16
            - self.lyric_release * 0.10)
            .clamp(0.04, 0.84);
        let mood_contrast = (0.08 + self.moodbar_change * 0.62).clamp(0.06, 0.72);
        let contrast = mix_local(mood_contrast, lyric_contrast, lyric_strength * 0.40);

        Some([brightness, saturation, warmth, contrast])
    }

    pub fn rain_visibility_floor(&self) -> f32 {
        if !self.active {
            return 0.62;
        }

        let semantic_protection = (self.lyric_motion * 0.30
            + self.lyric_tension * 0.22
            + self.lyric_transcendence * 0.30
            + self.lyric_release * 0.18)
            * self.lyric_guidance_strength();
        (0.54
            + self.performance_drive() * 0.18
            + self.transient_drive() * 0.05
            + semantic_protection * 0.10)
            .clamp(0.54, 0.84)
    }

    pub fn moodbar_channel_gains(&self) -> [f32; 3] {
        if !self.active || !self.moodbar_available {
            return [1.0; 3];
        }

        let mean =
            ((self.moodbar_rgb[0] + self.moodbar_rgb[1] + self.moodbar_rgb[2]) / 3.0).max(0.08);
        let strength = 0.18 + self.moodbar_change * 0.16;
        [
            (1.0 + (self.moodbar_rgb[0] / mean - 1.0) * strength).clamp(0.78, 1.28),
            (1.0 + (self.moodbar_rgb[1] / mean - 1.0) * strength).clamp(0.78, 1.28),
            (1.0 + (self.moodbar_rgb[2] / mean - 1.0) * strength).clamp(0.78, 1.28),
        ]
    }

    pub fn inspector_frame(&self) -> MusicInspectorFrame {
        MusicInspectorFrame {
            active: self.active,
            track: self.track.clone(),
            overall: self.overall,
            bass: self.bass,
            mid: self.mid,
            treble: self.treble,
            stereo: self.stereo,
            onset: self.onset,
            tempo_bpm: self.tempo_bpm,
            beat_confidence: self.beat_confidence,
            busyness: self.busyness,
            performance: self.performance,
            section_energy: self.section_energy,
            adaptive_floor: self.adaptive_floor,
            adaptive_peak: self.adaptive_peak,
            moodbar_rgb: self.moodbar_rgb,
            moodbar_energy: self.moodbar_energy,
            moodbar_change: self.moodbar_change,
            moodbar_progress: self.moodbar_progress,
            beat_pulse: self.beat_pulse,
            track_pulse: self.track_pulse,
            beat_phase: self.beat_phase,
            bar_phase: self.bar_phase,
            color_phase: self.color_phase,
            color_accent: self.color_accent,
            camera_section: self.camera_section,
            climax_pulse: self.climax_pulse,
            structure_drive: self.structure_drive(),
            detail_drive: self.detail_drive(),
            call_response_balance: self.call_response_balance(),
            signature_event: self.signature_event_strength(),
            speed_multiplier: self.speed_multiplier(),
            glow_multiplier: self.glow_multiplier(),
            exposure_multiplier: self.exposure_multiplier(),
            fov_offset: self.fov_offset(),
            cascade_multiplier: self.cascade_multiplier(),
            coupling_multiplier: self.coupling_multiplier(),
            rain_density_multiplier: self.rain_density_multiplier(),
            rain_energy_multiplier: self.rain_energy_multiplier(),
            head_activity_multiplier: self.head_activity_multiplier(),
            glyph_variation: self.glyph_variation_amount(),
            palette_mix: self.palette_mix(),
            wallpaper_color_multiplier: self.wallpaper_color_multiplier(),
            apparition_frequency_multiplier: self.apparition_frequency_multiplier(),
            apparition_opacity_multiplier: self.apparition_opacity_multiplier(),
            media_cycle_multiplier: self.media_cycle_multiplier(),
            cinematic_tempo_multiplier: self.cinematic_tempo_multiplier(),
            stereo_camera_drift: self.stereo_camera_drift(),
            spatial_strength: self.spatial_strength(),
            rain_visibility_floor: self.rain_visibility_floor(),
            primary_palette: self.primary_palette_color(),
            secondary_palette: self.secondary_palette_color(),
            timeline_confidence: self.timeline_confidence,
            timeline_change: self.timeline_change,
            timeline_novelty: self.timeline_novelty,
            timeline_trend: self.timeline_trend,
            operating_state: self.operating_state.label().to_string(),
            timeline_lookahead: self.timeline_lookahead,
        }
    }

    pub fn status_label(&self) -> String {
        let state = if !self.enabled {
            "off"
        } else if !self.capture_available {
            "waiting"
        } else if self.source_mode == MusicSourceMode::Strawberry
            && !self.strawberry_available
            && self.active
        {
            "live-fallback"
        } else if self.source_mode == MusicSourceMode::Strawberry && !self.strawberry_available {
            "no-player"
        } else if self.source_mode == MusicSourceMode::Strawberry && !self.strawberry_playing {
            "idle"
        } else if self.active {
            "live"
        } else {
            "quiet"
        };

        let track = if self.source_mode == MusicSourceMode::Strawberry && !self.track.is_empty() {
            format!(" — {}", self.track)
        } else {
            String::new()
        };

        let base = format!(
            "music:{}:{}:{}:color-{} state:{} timeline:{}:{:.2} profile:{} bpm:{:.0} conf:{:.2} busy:{:.2} lvl:{:.2} perf:{:.2} bass:{:.2} onset:{:.2} beat:{:.2} mood:{:.2}/{:.2}@{:.0}%:{}{}",
            state,
            self.source_mode.label(),
            self.intensity.label(),
            self.color_mode.label(),
            self.operating_state.label(),
            self.moodbar_source.as_str(),
            self.timeline_confidence,
            self.profile.label.as_str(),
            self.tempo_bpm,
            self.beat_confidence,
            self.busyness,
            self.overall,
            self.performance,
            self.bass,
            self.onset,
            self.beat_pulse,
            self.moodbar_energy,
            self.moodbar_change,
            self.moodbar_progress * 100.0,
            if self.moodbar_available {
                self.moodbar_source.as_str()
            } else {
                "none"
            },
            track,
        );
        let base = format!("{} | {}", base, self.conductor_status());

        format!(
            "{} lyric:{}:{}:{} warm:{:.2} dark:{:.2} move:{:.2} tense:{:.2}",
            base,
            self.lyric_semantic_label(),
            if self.lyric_timed { "timed" } else { "global" },
            self.lyric_source.as_str(),
            self.lyric_warmth_axis(),
            self.lyric_darkness * self.lyric_guidance_strength(),
            self.lyric_motion_axis(),
            self.lyric_tension * self.lyric_guidance_strength(),
        )
    }
}

fn spawn_pipewire_capture(shared: Arc<Mutex<MusicTelemetry>>) {
    thread::spawn(move || {
        loop {
            let pw_record = resolve_command("pw-record");
            let mut child = match Command::new(&pw_record)
            .args([
                "--raw",
                "--format=f32",
                "--rate=48000",
                "--channels=2",
                "--channel-map=stereo",
                "--latency=50ms",
                "--properties={\"stream.capture.sink\":true,\"media.role\":\"Music\",\"node.name\":\"neon-rain-music-reactor\"}",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => {
                if let Ok(mut telemetry) = shared.lock() {
                    telemetry.capture_available = false;
                    telemetry.live = false;
                }
                thread::sleep(Duration::from_secs(5));
                continue;
            }
        };

            if let Ok(mut telemetry) = shared.lock() {
                telemetry.capture_available = true;
            }

            let Some(mut stdout) = child.stdout.take() else {
                let _ = child.kill();
                thread::sleep(Duration::from_secs(2));
                continue;
            };

            let bass_alpha = lowpass_alpha(170.0);
            let mid_alpha = lowpass_alpha(2_300.0);
            let mut low_bass = 0.0f32;
            let mut low_mid = 0.0f32;
            let mut previous_bass = 0.0f32;
            let mut previous_overall = 0.0f32;
            let mut last_beat = Instant::now() - Duration::from_secs(1);
            let mut beat_period = 0.50f32;
            let mut beat_confidence = 0.0f32;
            let mut busyness = 0.0f32;
            let mut last_signal = Instant::now() - Duration::from_secs(2);
            let mut pending = Vec::<u8>::with_capacity(16_384);
            let mut buffer = [0u8; 16_384];

            loop {
                let read_count = match stdout.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(_) => break,
                };

                pending.extend_from_slice(&buffer[..read_count]);
                let complete_len = pending.len() / FRAME_BYTES * FRAME_BYTES;
                if complete_len == 0 {
                    continue;
                }

                let mut overall_sum = 0.0f32;
                let mut bass_sum = 0.0f32;
                let mut mid_sum = 0.0f32;
                let mut treble_sum = 0.0f32;
                let mut left_sum = 0.0f32;
                let mut right_sum = 0.0f32;
                let mut frame_count = 0usize;

                for frame in pending[..complete_len].chunks_exact(FRAME_BYTES) {
                    let left = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
                    let right = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
                    if !left.is_finite() || !right.is_finite() {
                        continue;
                    }

                    let mono = (left + right) * 0.5;
                    low_bass += bass_alpha * (mono - low_bass);
                    low_mid += mid_alpha * (mono - low_mid);
                    let mid_band = low_mid - low_bass;
                    let treble_band = mono - low_mid;

                    overall_sum += mono * mono;
                    bass_sum += low_bass * low_bass;
                    mid_sum += mid_band * mid_band;
                    treble_sum += treble_band * treble_band;
                    left_sum += left.abs();
                    right_sum += right.abs();
                    frame_count += 1;
                }

                let leftover = pending.split_off(complete_len);
                pending = leftover;

                if frame_count == 0 {
                    continue;
                }

                let divisor = frame_count as f32;
                let overall_raw = (overall_sum / divisor).sqrt();
                let bass_raw = (bass_sum / divisor).sqrt();
                let mid_raw = (mid_sum / divisor).sqrt();
                let treble_raw = (treble_sum / divisor).sqrt();

                let overall = (overall_raw * 5.4).clamp(0.0, 1.0).sqrt();
                let bass = (bass_raw * 8.4).clamp(0.0, 1.0).sqrt();
                let mid = (mid_raw * 9.2).clamp(0.0, 1.0).sqrt();
                let treble = (treble_raw * 11.0).clamp(0.0, 1.0).sqrt();
                let stereo_total = (left_sum + right_sum).max(0.000_001);
                let stereo = ((right_sum - left_sum) / stereo_total).clamp(-1.0, 1.0);

                if overall_raw > 0.0025 {
                    last_signal = Instant::now();
                }
                let live = last_signal.elapsed() < Duration::from_millis(700);

                let onset = ((bass - previous_bass).max(0.0) * 3.35
                    + (overall - previous_overall).max(0.0) * 1.70)
                    .clamp(0.0, 1.0);

                let busy_target = (onset * 0.72 + treble * 0.34 + mid * 0.16).clamp(0.0, 1.0);
                busyness = smooth_follow(busyness, busy_target, 0.20, 0.045);

                let beat_elapsed = last_beat.elapsed().as_secs_f32();
                let minimum_gap = (beat_period * 0.40).clamp(0.16, 0.30);
                let adaptive_threshold = 0.105 + busyness * 0.055;
                let beat = onset > adaptive_threshold && beat_elapsed > minimum_gap;

                if beat {
                    if (0.24..=1.40).contains(&beat_elapsed) {
                        let mut candidate = beat_elapsed;
                        while candidate < beat_period * 0.67 {
                            candidate *= 2.0;
                        }
                        while candidate > beat_period * 1.50 {
                            candidate *= 0.5;
                        }

                        let similarity = (1.0
                            - ((candidate - beat_period).abs() / beat_period.max(0.001)))
                        .clamp(0.0, 1.0);
                        beat_period += (candidate - beat_period) * (0.10 + similarity * 0.18);
                        beat_confidence =
                            (beat_confidence * 0.82 + similarity * 0.18).clamp(0.0, 1.0);
                    }
                    last_beat = Instant::now();
                } else {
                    beat_confidence *= 0.9995;
                }
                previous_bass = bass;
                previous_overall = overall;

                if let Ok(mut telemetry) = shared.lock() {
                    telemetry.capture_available = true;
                    telemetry.live = live;
                    telemetry.overall = smooth_follow(telemetry.overall, overall, 0.44, 0.16);
                    telemetry.bass = smooth_follow(telemetry.bass, bass, 0.40, 0.13);
                    telemetry.mid = smooth_follow(telemetry.mid, mid, 0.38, 0.17);
                    telemetry.treble = smooth_follow(telemetry.treble, treble, 0.48, 0.24);
                    telemetry.stereo = smooth_follow(telemetry.stereo, stereo, 0.18, 0.12);
                    telemetry.onset = smooth_follow(telemetry.onset, onset, 0.62, 0.18);
                    telemetry.tempo_bpm = (60.0 / beat_period.max(0.001)).clamp(45.0, 210.0);
                    telemetry.beat_confidence =
                        smooth_follow(telemetry.beat_confidence, beat_confidence, 0.20, 0.035);
                    telemetry.busyness = smooth_follow(telemetry.busyness, busyness, 0.22, 0.055);
                    if beat {
                        telemetry.beat_serial = telemetry.beat_serial.wrapping_add(1);
                    }
                }
            }

            let _ = child.kill();
            let _ = child.wait();
            if let Ok(mut telemetry) = shared.lock() {
                telemetry.capture_available = false;
                telemetry.live = false;
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

fn spawn_strawberry_monitor(shared: Arc<Mutex<MusicTelemetry>>) {
    thread::spawn(move || {
        let mut previous_track_key = String::new();
        let mut previous_lyric_key = String::new();
        let mut previous_player = String::new();
        let mut selected_player = String::new();
        let mut current_profile = TrackProfile::default();
        let mut current_moodbar = MoodbarTimeline::default();
        let mut current_lyric_semantics = LyricSemanticFrame::default();

        loop {
            let playerctl = resolve_command("playerctl");
            if selected_player.is_empty() {
                selected_player = select_mpris_player(&playerctl).unwrap_or_default();
            }

            let player_arg = format!("--player={}", selected_player);
            let playerctl_status = if selected_player.is_empty() {
                None
            } else {
                Command::new(&playerctl)
                    .args([player_arg.as_str(), "status"])
                    .output()
                    .ok()
            };

            let (available, playing) = match playerctl_status {
                Some(output) if output.status.success() => {
                    let status = String::from_utf8_lossy(&output.stdout);
                    (true, status.trim().eq_ignore_ascii_case("playing"))
                }
                _ => {
                    selected_player.clear();
                    strawberry_status_via_busctl()
                }
            };

            if selected_player != previous_player {
                if !selected_player.is_empty() {
                    println!("MPRIS player selected: {}", selected_player);
                }
                previous_player = selected_player.clone();
            }

            let metadata = if available && !selected_player.is_empty() {
                Command::new(&playerctl)
                    .args([
                        player_arg.as_str(),
                        "metadata",
                        "--format",
                        "{{artist}}\u{1f}{{title}}\u{1f}{{album}}\u{1f}{{genre}}\u{1f}{{xesam:url}}\u{1f}{{mpris:length}}\u{1f}{{xesam:asText}}",
                    ])
                    .output()
                    .ok()
                    .filter(|output| output.status.success())
                    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };

            let fields = metadata.split('\u{1f}').collect::<Vec<_>>();
            let artist = fields.first().copied().unwrap_or_default().trim();
            let title = fields.get(1).copied().unwrap_or_default().trim();
            let album = fields.get(2).copied().unwrap_or_default().trim();
            let genre = fields.get(3).copied().unwrap_or_default().trim();
            let url = fields.get(4).copied().unwrap_or_default().trim();
            let duration_microseconds = fields
                .get(5)
                .copied()
                .unwrap_or_default()
                .trim()
                .parse::<f32>()
                .unwrap_or(0.0);
            let track_duration = if duration_microseconds > 10_000.0 {
                duration_microseconds / 1_000_000.0
            } else {
                duration_microseconds
            };
            let metadata_lyrics = fields.get(6).copied().unwrap_or_default().trim();
            let playback_position = if available && !selected_player.is_empty() {
                Command::new(&playerctl)
                    .args([player_arg.as_str(), "position"])
                    .output()
                    .ok()
                    .filter(|output| output.status.success())
                    .and_then(|output| {
                        String::from_utf8_lossy(&output.stdout)
                            .trim()
                            .parse::<f32>()
                            .ok()
                    })
                    .unwrap_or(0.0)
            } else {
                0.0
            };

            let track_key = format!("{}\u{1f}{}\u{1f}{}\u{1f}{}", artist, title, album, url);
            let changed =
                track_key != previous_track_key && (!artist.is_empty() || !title.is_empty());

            if !available {
                current_lyric_semantics = LyricSemanticFrame::default();
            } else if changed
                || (playing && current_lyric_semantics.found && current_lyric_semantics.timed)
            {
                current_lyric_semantics =
                    resolve_lyric_semantics(url, playback_position, metadata_lyrics);
            }

            let lyric_key = format!(
                "{}\u{1f}{}\u{1f}{}",
                current_lyric_semantics.label.as_str(),
                current_lyric_semantics.timed,
                current_lyric_semantics.source.as_str(),
            );
            if current_lyric_semantics.found && lyric_key != previous_lyric_key {
                println!(
                    "Lyric semantics: {} mode={} source={} warm={:.2} cool={:.2} dark={:.2} intimate={:.2} motion={:.2} tension={:.2} release={:.2} transcend={:.2}",
                    current_lyric_semantics.label.as_str(),
                    if current_lyric_semantics.timed {
                        "timed"
                    } else {
                        "global"
                    },
                    current_lyric_semantics.source.as_str(),
                    current_lyric_semantics.warmth,
                    current_lyric_semantics.coolness,
                    current_lyric_semantics.darkness,
                    current_lyric_semantics.intimacy,
                    current_lyric_semantics.motion,
                    current_lyric_semantics.tension,
                    current_lyric_semantics.release,
                    current_lyric_semantics.transcendence,
                );
            }
            previous_lyric_key = lyric_key;

            let track = if !artist.is_empty() || !title.is_empty() {
                format!("{} — {}", artist, title)
                    .trim_matches(' ')
                    .trim_matches('—')
                    .trim()
                    .to_string()
            } else if available && !selected_player.is_empty() {
                selected_player.clone()
            } else {
                String::new()
            };

            if changed {
                current_profile = resolve_track_profile(artist, title, album, genre, url);
                current_moodbar = resolve_moodbar_timeline(url);
                println!(
                    "Music profile: {} camera={:.2} pulse={:.2} color={:.2} density={:.2} apparitions={:.2} fluidity={:.2} tags={}",
                    current_profile.label.as_str(),
                    current_profile.camera,
                    current_profile.pulse,
                    current_profile.color,
                    current_profile.density,
                    current_profile.apparition,
                    current_profile.fluidity,
                    current_profile.tags.as_str(),
                );
                println!(
                    "Timeline enrichment: {} samples={} source={}",
                    if current_moodbar.samples.is_empty() {
                        "live/learned fallback"
                    } else {
                        "external ready"
                    },
                    current_moodbar.samples.len(),
                    current_moodbar.source.as_str(),
                );
            }

            if let Ok(mut telemetry) = shared.lock() {
                telemetry.strawberry_available = available;
                telemetry.strawberry_playing = playing;
                telemetry.track = track.clone();
                telemetry.profile = current_profile.clone();
                telemetry.lyric_semantics = current_lyric_semantics.clone();
                telemetry.moodbar = current_moodbar.clone();
                telemetry.playback_position = playback_position;
                telemetry.track_duration = track_duration;
                if changed {
                    telemetry.track_serial = telemetry.track_serial.wrapping_add(1);
                }
            }
            previous_track_key = track_key;
            thread::sleep(Duration::from_millis(650));
        }
    });
}

fn select_mpris_player(playerctl: &PathBuf) -> Option<String> {
    let output = Command::new(playerctl).arg("--list-all").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let players = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if players.is_empty() {
        return None;
    }

    let mut playing = Vec::<String>::new();
    for player in &players {
        let argument = format!("--player={}", player);
        let status = Command::new(playerctl)
            .args([argument.as_str(), "status"])
            .output()
            .ok();
        if status
            .as_ref()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .eq_ignore_ascii_case("playing")
            })
            .unwrap_or(false)
        {
            playing.push(player.clone());
        }
    }

    playing
        .iter()
        .find(|player| player.to_ascii_lowercase().contains("strawberry"))
        .cloned()
        .or_else(|| playing.first().cloned())
        .or_else(|| {
            players
                .iter()
                .find(|player| player.to_ascii_lowercase().contains("strawberry"))
                .cloned()
        })
        .or_else(|| players.first().cloned())
}

fn resolve_lyric_semantics(
    url: &str,
    playback_position: f32,
    metadata_lyrics: &str,
) -> LyricSemanticFrame {
    let helper = resolve_command("neon-rain-lyric-runtime");
    let position = format!("{:.3}", playback_position.max(0.0));
    if let Ok(output) = Command::new(helper)
        .args([url, position.as_str(), metadata_lyrics])
        .output()
    {
        if output.status.success() {
            if let Some(frame) =
                LyricSemanticFrame::from_helper_output(&String::from_utf8_lossy(&output.stdout))
            {
                return frame;
            }
        }
    }

    LyricSemanticFrame::default()
}

fn resolve_moodbar_timeline(url: &str) -> MoodbarTimeline {
    if url.is_empty() {
        return MoodbarTimeline::default();
    }

    let helper = resolve_command("neon-rain-moodbar-profile");
    if let Ok(output) = Command::new(helper).arg(url).output() {
        if output.status.success() {
            return load_moodbar_timeline(&String::from_utf8_lossy(&output.stdout));
        }
    }

    MoodbarTimeline::default()
}

fn resolve_track_profile(
    artist: &str,
    title: &str,
    album: &str,
    genre: &str,
    url: &str,
) -> TrackProfile {
    let helper = resolve_command("neon-rain-track-profile");
    if let Ok(output) = Command::new(helper)
        .args([artist, title, album, genre, url])
        .output()
    {
        if output.status.success() {
            if let Some(profile) =
                TrackProfile::from_helper_output(&String::from_utf8_lossy(&output.stdout))
            {
                return profile;
            }
        }
    }

    infer_track_profile(&format!("{} {} {} {}", artist, title, album, genre))
}

fn strawberry_status_via_busctl() -> (bool, bool) {
    let busctl = resolve_command("busctl");
    let output = Command::new(busctl)
        .args([
            "--user",
            "get-property",
            "org.mpris.MediaPlayer2.strawberry",
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
            "PlaybackStatus",
        ])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let value = String::from_utf8_lossy(&output.stdout);
            (true, value.contains("Playing"))
        }
        _ => (false, false),
    }
}

fn capability_flag(name: &str) -> &'static str {
    if resolve_command(name).is_file() {
        "yes"
    } else {
        "no"
    }
}

fn resolve_command(name: &str) -> PathBuf {
    let override_key = format!(
        "NEON_RAIN_COMMAND_{}",
        name.to_ascii_uppercase().replace('-', "_"),
    );
    if let Some(candidate) = env::var_os(&override_key).map(PathBuf::from)
        && candidate.is_file()
    {
        return candidate;
    }

    if let Some(path) = env::var_os("NEON_RAIN_HELPER_PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    }) {
        return path;
    }

    if let Ok(executable) = env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let adjacent = directory.join("helpers").join(name);
        if adjacent.is_file() {
            return adjacent;
        }

        if let Some(prefix) = directory.parent() {
            let libexec = prefix.join("libexec").join("neon-rain").join(name);
            if libexec.is_file() {
                return libexec;
            }
        }
    }

    if let Some(path) = env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    }) {
        return path;
    }

    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        let local_candidate = home.join(".local/bin").join(name);
        if local_candidate.is_file() {
            return local_candidate;
        }

        let candidate = home.join(".nix-profile/bin").join(name);
        if candidate.is_file() {
            return candidate;
        }
    }

    let system_candidate = PathBuf::from("/run/current-system/sw/bin").join(name);
    if system_candidate.is_file() {
        return system_candidate;
    }

    PathBuf::from(name)
}

fn lowpass_alpha(cutoff_hz: f32) -> f32 {
    let angular = std::f32::consts::TAU * cutoff_hz;
    angular / (angular + SAMPLE_RATE)
}

fn smooth_follow(current: f32, target: f32, attack: f32, release: f32) -> f32 {
    let amount = if target > current { attack } else { release };
    current + (target - current) * amount
}
