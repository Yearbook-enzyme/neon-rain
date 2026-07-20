use std::{
    collections::VecDeque,
    env, fs,
    hash::{Hash, Hasher},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureSource {
    Unavailable,
    LiveAudio,
    RollingHistory,
    PlayerMetadata,
    ExternalTimeline,
    LearnedTimeline,
    Semantic,
}

impl FeatureSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::LiveAudio => "live-audio",
            Self::RollingHistory => "live-history",
            Self::PlayerMetadata => "player-metadata",
            Self::ExternalTimeline => "external-timeline",
            Self::LearnedTimeline => "learned-timeline",
            Self::Semantic => "semantic",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FeatureValue {
    pub value: f32,
    pub confidence: f32,
    pub age_seconds: f32,
    pub source: FeatureSource,
}

impl FeatureValue {
    pub fn unavailable() -> Self {
        Self {
            value: 0.0,
            confidence: 0.0,
            age_seconds: f32::INFINITY,
            source: FeatureSource::Unavailable,
        }
    }

    pub fn new(value: f32, confidence: f32, source: FeatureSource) -> Self {
        Self {
            value,
            confidence: confidence.clamp(0.0, 1.0),
            age_seconds: 0.0,
            source,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperatingState {
    Autonomous,
    Listening,
    Calibrating,
    Performing,
    Enriched,
    Silence,
    Recovering,
}

impl OperatingState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Autonomous => "autonomous",
            Self::Listening => "listening",
            Self::Calibrating => "calibrating",
            Self::Performing => "performing",
            Self::Enriched => "enriched",
            Self::Silence => "silence",
            Self::Recovering => "recovering",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct HistorySample {
    energy: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    onset: f32,
    busyness: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct RollingFrame {
    pub energy: FeatureValue,
    pub change: FeatureValue,
    pub novelty: FeatureValue,
    pub trend: FeatureValue,
    pub rgb: [f32; 3],
    pub color_confidence: f32,
    pub learned_energy: f32,
    pub learned_change: f32,
    pub learned_rgb: [f32; 3],
    pub learned_confidence: f32,
    pub lookahead_release: f32,
    pub progress: f32,
    pub state: OperatingState,
}

impl Default for RollingFrame {
    fn default() -> Self {
        Self {
            energy: FeatureValue::unavailable(),
            change: FeatureValue::unavailable(),
            novelty: FeatureValue::unavailable(),
            trend: FeatureValue::unavailable(),
            rgb: [0.15, 0.34, 0.72],
            color_confidence: 0.0,
            learned_energy: 0.0,
            learned_change: 0.0,
            learned_rgb: [0.15, 0.34, 0.72],
            learned_confidence: 0.0,
            lookahead_release: 0.0,
            progress: 0.0,
            state: OperatingState::Autonomous,
        }
    }
}

pub struct RollingAnalyzer {
    history: VecDeque<HistorySample>,
    sample_accumulator: f32,
    active_seconds: f32,
    silence_seconds: f32,
    recovering_seconds: f32,
    total_seconds: f32,
    was_active: bool,
    last_frame: RollingFrame,
    track_key: String,
    track_duration: f32,
    learned_samples: Vec<Option<HistorySample>>,
    cache_dirty: bool,
    cache_write_timer: f32,
    cache_dir: PathBuf,
}

impl RollingAnalyzer {
    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(480),
            sample_accumulator: 0.0,
            active_seconds: 0.0,
            silence_seconds: 0.0,
            recovering_seconds: 0.0,
            total_seconds: 0.0,
            was_active: false,
            last_frame: RollingFrame::default(),
            track_key: String::new(),
            track_duration: 0.0,
            learned_samples: Vec::new(),
            cache_dirty: false,
            cache_write_timer: 0.0,
            cache_dir: analysis_cache_dir(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f32,
        capture_available: bool,
        active: bool,
        player_available: bool,
        enriched: bool,
        track_identity: &str,
        playback_position: f32,
        track_duration: f32,
        overall: f32,
        bass: f32,
        mid: f32,
        treble: f32,
        onset: f32,
        busyness: f32,
        beat_confidence: f32,
        section_energy: f32,
        performance: f32,
    ) -> RollingFrame {
        let dt = dt.clamp(0.0, 0.1);
        self.total_seconds += dt;
        self.cache_write_timer += dt;
        self.update_track_memory(track_identity, track_duration);

        if active {
            self.active_seconds += dt;
            self.silence_seconds = 0.0;
            self.recovering_seconds = 0.0;
        } else {
            self.active_seconds = 0.0;
            self.silence_seconds += dt;
            if self.was_active {
                self.recovering_seconds = 2.0;
            }
            self.recovering_seconds = (self.recovering_seconds - dt).max(0.0);
        }
        self.was_active = active;

        let instantaneous_energy =
            (overall * 0.34 + bass * 0.24 + mid * 0.22 + treble * 0.10 + performance * 0.10)
                .clamp(0.0, 1.2);

        self.sample_accumulator += dt;
        if active && self.sample_accumulator >= 0.25 {
            self.sample_accumulator = 0.0;
            let learned_sample = HistorySample {
                energy: (instantaneous_energy * 0.62 + section_energy * 0.38).clamp(0.0, 1.0),
                bass: bass.clamp(0.0, 1.0),
                mid: mid.clamp(0.0, 1.0),
                treble: treble.clamp(0.0, 1.0),
                onset: onset.clamp(0.0, 1.0),
                busyness: busyness.clamp(0.0, 1.0),
            };
            self.history.push_back(learned_sample);
            self.record_track_sample(playback_position, learned_sample);
            while self.history.len() > 480 {
                self.history.pop_front();
            }
        }

        let short = mean_tail(&self.history, 8);
        let medium = mean_tail(&self.history, 32);
        let long = mean_tail(&self.history, 96);
        let sample_confidence = (self.history.len() as f32 / 80.0).clamp(0.0, 1.0);
        let rhythm_confidence = (0.38 + beat_confidence * 0.62).clamp(0.0, 1.0);
        let confidence = if active {
            sample_confidence * rhythm_confidence
        } else {
            sample_confidence * (-self.silence_seconds * 0.7).exp()
        };

        let energy =
            (short.energy * 0.56 + medium.energy * 0.28 + section_energy * 0.16).clamp(0.0, 1.0);
        let change = ((short.energy - medium.energy).abs() * 1.65
            + (short.bass - medium.bass).abs() * 0.42
            + (short.mid - medium.mid).abs() * 0.34
            + (short.treble - medium.treble).abs() * 0.28
            + short.onset * 0.16)
            .clamp(0.0, 1.0);
        let novelty = ((short.energy - long.energy).abs() * 1.10
            + (short.bass - long.bass).abs() * 0.42
            + (short.mid - long.mid).abs() * 0.42
            + (short.treble - long.treble).abs() * 0.42
            + (short.busyness - long.busyness).abs() * 0.24)
            .clamp(0.0, 1.0);
        let trend = ((short.energy - long.energy) * 2.2).clamp(-1.0, 1.0);

        let red = short.bass * 0.68 + short.mid * 0.24 + energy * 0.08;
        let green = short.mid * 0.62 + short.treble * 0.24 + energy * 0.14;
        let blue = short.treble * 0.66 + short.mid * 0.22 + (1.0 - energy) * 0.12;
        let maximum = red.max(green).max(blue).max(0.10);
        let rgb = [
            (red / maximum).clamp(0.0, 1.0),
            (green / maximum).clamp(0.0, 1.0),
            (blue / maximum).clamp(0.0, 1.0),
        ];

        let learned = self.learned_frame(playback_position);
        if self.cache_dirty && self.cache_write_timer >= 12.0 {
            self.save_cache();
        }

        let state = if !capture_available {
            OperatingState::Autonomous
        } else if self.recovering_seconds > 0.0 {
            OperatingState::Recovering
        } else if !active && self.silence_seconds > 1.2 {
            OperatingState::Silence
        } else if active && self.active_seconds < 1.5 {
            OperatingState::Listening
        } else if active && self.active_seconds < 7.0 {
            OperatingState::Calibrating
        } else if active && (enriched || player_available && confidence > 0.72) {
            OperatingState::Enriched
        } else if active {
            OperatingState::Performing
        } else {
            OperatingState::Autonomous
        };

        self.last_frame = RollingFrame {
            energy: FeatureValue::new(energy, confidence, FeatureSource::RollingHistory),
            change: FeatureValue::new(change, confidence * 0.92, FeatureSource::RollingHistory),
            novelty: FeatureValue::new(novelty, confidence * 0.88, FeatureSource::RollingHistory),
            trend: FeatureValue::new(trend, confidence * 0.82, FeatureSource::RollingHistory),
            rgb,
            color_confidence: confidence * 0.76,
            learned_energy: learned.0,
            learned_change: learned.1,
            learned_rgb: learned.2,
            learned_confidence: learned.3,
            lookahead_release: learned.4,
            progress: if track_duration > 0.1 {
                (playback_position / track_duration).clamp(0.0, 1.0)
            } else {
                (self.total_seconds / 120.0).rem_euclid(1.0)
            },
            state,
        };
        self.last_frame
    }

    fn update_track_memory(&mut self, track_identity: &str, duration: f32) {
        let normalized_key = track_identity.trim();
        if normalized_key.is_empty() {
            return;
        }

        if normalized_key != self.track_key {
            if self.cache_dirty {
                self.save_cache();
            }
            self.track_key = normalized_key.to_string();
            self.track_duration = duration.max(0.0);
            self.learned_samples = self.load_cache();
            self.cache_dirty = false;
            self.cache_write_timer = 0.0;
        } else if duration > 1.0 {
            self.track_duration = duration;
        }

        let desired_len = self.track_duration.ceil().clamp(0.0, 21_600.0) as usize + 1;
        if desired_len > self.learned_samples.len() {
            self.learned_samples.resize(desired_len, None);
        }
    }

    fn record_track_sample(&mut self, position: f32, sample: HistorySample) {
        if self.track_key.is_empty() || position < 0.0 || !position.is_finite() {
            return;
        }
        let index = position.floor().clamp(0.0, 21_599.0) as usize;
        if index >= self.learned_samples.len() {
            self.learned_samples.resize(index + 1, None);
        }

        self.learned_samples[index] = Some(match self.learned_samples[index] {
            Some(previous) => blend_sample(previous, sample, 0.28),
            None => sample,
        });
        self.cache_dirty = true;
    }

    fn learned_frame(&self, position: f32) -> (f32, f32, [f32; 3], f32, f32) {
        if self.track_key.is_empty() || self.learned_samples.is_empty() || position < 0.0 {
            return (0.0, 0.0, [0.15, 0.34, 0.72], 0.0, 0.0);
        }

        let index = position.floor().max(0.0) as usize;
        let Some(current) = nearby_sample(&self.learned_samples, index, 2) else {
            return (0.0, 0.0, [0.15, 0.34, 0.72], 0.0, 0.0);
        };
        let previous =
            nearby_sample(&self.learned_samples, index.saturating_sub(4), 3).unwrap_or(current);
        let future =
            nearby_sample(&self.learned_samples, index.saturating_add(8), 3).unwrap_or(current);
        let coverage = self
            .learned_samples
            .iter()
            .filter(|sample| sample.is_some())
            .count() as f32
            / self.learned_samples.len().max(1) as f32;
        let confidence = (0.54 + coverage.sqrt() * 0.42).clamp(0.0, 0.94);
        let change = ((current.energy - previous.energy).abs() * 1.45
            + (current.bass - previous.bass).abs() * 0.36
            + (current.mid - previous.mid).abs() * 0.32
            + (current.treble - previous.treble).abs() * 0.28)
            .clamp(0.0, 1.0);
        let lookahead_release = ((future.energy - current.energy).max(0.0) * 1.5
            + (future.onset - current.onset).max(0.0) * 0.45)
            .clamp(0.0, 1.0);
        let maximum = current.bass.max(current.mid).max(current.treble).max(0.10);
        let rgb = [
            (current.bass / maximum).clamp(0.0, 1.0),
            (current.mid / maximum).clamp(0.0, 1.0),
            (current.treble / maximum).clamp(0.0, 1.0),
        ];
        (current.energy, change, rgb, confidence, lookahead_release)
    }

    fn cache_path(&self) -> Option<PathBuf> {
        if self.track_key.is_empty() {
            return None;
        }
        Some(
            self.cache_dir
                .join(format!("v1-{:016x}.nra", stable_hash(&self.track_key))),
        )
    }

    fn load_cache(&self) -> Vec<Option<HistorySample>> {
        let Some(path) = self.cache_path() else {
            return Vec::new();
        };
        let Ok(text) = fs::read_to_string(path) else {
            return Vec::new();
        };
        let mut samples = Vec::<Option<HistorySample>>::new();
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 7 {
                continue;
            }
            let Some(index) = fields[0].parse::<usize>().ok() else {
                continue;
            };
            let parse = |i: usize| fields[i].parse::<f32>().ok();
            let (Some(energy), Some(bass), Some(mid), Some(treble), Some(onset), Some(busyness)) =
                (parse(1), parse(2), parse(3), parse(4), parse(5), parse(6))
            else {
                continue;
            };
            if index >= samples.len() {
                samples.resize(index + 1, None);
            }
            samples[index] = Some(HistorySample {
                energy,
                bass,
                mid,
                treble,
                onset,
                busyness,
            });
        }
        samples
    }

    fn save_cache(&mut self) {
        let Some(path) = self.cache_path() else {
            return;
        };
        if fs::create_dir_all(&self.cache_dir).is_err() {
            return;
        }
        let mut text = format!(
            "# neon-rain-analysis-v1 duration={:.3}\n",
            self.track_duration
        );
        for (index, sample) in self.learned_samples.iter().enumerate() {
            let Some(sample) = sample else {
                continue;
            };
            text.push_str(&format!(
                "{} {:.6} {:.6} {:.6} {:.6} {:.6} {:.6}\n",
                index,
                sample.energy,
                sample.bass,
                sample.mid,
                sample.treble,
                sample.onset,
                sample.busyness,
            ));
        }
        let temporary = path.with_extension("tmp");
        if fs::write(&temporary, text).is_ok() && fs::rename(&temporary, &path).is_ok() {
            self.cache_dirty = false;
            self.cache_write_timer = 0.0;
        }
    }
}

fn analysis_cache_dir() -> PathBuf {
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path).join("neon-rain/analysis");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".cache/neon-rain/analysis");
    }
    env::temp_dir().join("neon-rain/analysis")
}

fn stable_hash(value: &str) -> u64 {
    struct FnvHasher(u64);
    impl Hasher for FnvHasher {
        fn finish(&self) -> u64 {
            self.0
        }
        fn write(&mut self, bytes: &[u8]) {
            for byte in bytes {
                self.0 ^= *byte as u64;
                self.0 = self.0.wrapping_mul(0x100000001b3);
            }
        }
    }
    let mut hasher = FnvHasher(0xcbf29ce484222325);
    value.hash(&mut hasher);
    hasher.finish()
}

fn blend_sample(a: HistorySample, b: HistorySample, amount: f32) -> HistorySample {
    let amount = amount.clamp(0.0, 1.0);
    HistorySample {
        energy: a.energy + (b.energy - a.energy) * amount,
        bass: a.bass + (b.bass - a.bass) * amount,
        mid: a.mid + (b.mid - a.mid) * amount,
        treble: a.treble + (b.treble - a.treble) * amount,
        onset: a.onset + (b.onset - a.onset) * amount,
        busyness: a.busyness + (b.busyness - a.busyness) * amount,
    }
}

fn nearby_sample(
    samples: &[Option<HistorySample>],
    index: usize,
    radius: usize,
) -> Option<HistorySample> {
    if samples.is_empty() {
        return None;
    }
    let start = index.saturating_sub(radius);
    let end = index
        .saturating_add(radius)
        .min(samples.len().saturating_sub(1));
    for distance in 0..=radius {
        let left = index.saturating_sub(distance);
        if left >= start {
            if let Some(sample) = samples.get(left).and_then(|sample| *sample) {
                return Some(sample);
            }
        }
        let right = index.saturating_add(distance);
        if right <= end {
            if let Some(sample) = samples.get(right).and_then(|sample| *sample) {
                return Some(sample);
            }
        }
    }
    None
}

fn mean_tail(history: &VecDeque<HistorySample>, count: usize) -> HistorySample {
    if history.is_empty() {
        return HistorySample::default();
    }

    let mut result = HistorySample::default();
    let mut used = 0.0f32;
    for sample in history.iter().rev().take(count) {
        result.energy += sample.energy;
        result.bass += sample.bass;
        result.mid += sample.mid;
        result.treble += sample.treble;
        result.onset += sample.onset;
        result.busyness += sample.busyness;
        used += 1.0;
    }

    if used > 0.0 {
        result.energy /= used;
        result.bass /= used;
        result.mid /= used;
        result.treble /= used;
        result.onset /= used;
        result.busyness /= used;
    }
    result
}
