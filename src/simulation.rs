pub const GLYPHS_PER_STREAM: usize = 64;

// Five layers × 102 horizontal lanes. Keeping the simulation
// beneath the GPU limit prevents larger windows from silently
// losing the foreground layers.
pub const MAX_STREAMS: usize = 510;

#[derive(Clone, Copy, Debug)]
pub enum Personality {
    Steady,
    Fast,
    Lazy,
    Nervous,
    Pulse,
    Ghost,
}

#[derive(Clone, Debug)]
pub struct Stream {
    pub x: f32,
    pub head: f32,
    pub speed: f32,
    pub length: u32,
    pub brightness: f32,
    pub glyphs: [u32; GLYPHS_PER_STREAM],
    pub personality: Personality,
    pub mutation_timer: f32,
    pub age: f32,
    pub lifetime: f32,
    pub phase: f32,

    // 0.0 = distant background, 1.0 = foreground.
    pub depth: f32,

    // Horizontal movement measured in pixels.
    pub drift: f32,

    // Persistent lane offset prevents perfectly regular spacing.
    pub lane_offset: f32,

    // Rare energy packet travelling from the head toward the tail.
    // Position is normalized: 0.0 = head, 1.0 = end of trail.
    pub cascade_position: f32,
    pub cascade_speed: f32,
    pub cascade_intensity: f32,
    cascade_timer: f32,
}

pub struct Simulation {
    pub streams: Vec<Stream>,
    rng: Random,
    width: f32,
    height: f32,
    columns: usize,
    layers: usize,

    // Shared environmental clock for broad, coherent rain behavior.
    weather_time: f32,
}

impl Simulation {
    pub fn new(width: u32, height: u32) -> Self {
        let mut simulation = Self {
            streams: Vec::new(),
            rng: Random::new(0x4d41_5452_4958_2026),
            width: width.max(1) as f32,
            height: height.max(1) as f32,
            columns: 1,
            layers: 5,
            weather_time: 0.0,
        };

        simulation.resize(width, height);
        simulation
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let old_width = self.width.max(1.0);
        let old_height = self.height.max(1.0);
        let existing_streams = self.streams.len();

        self.width = width.max(1) as f32;
        self.height = height.max(1) as f32;

        let width_ratio = self.width / old_width;
        let height_ratio = self.height / old_height;

        // 1600 × 900 is the reference viewport. At matching aspect
        // ratios this yields approximately 100 lanes at any normal
        // window or fullscreen resolution.
        let visual_scale = ((self.width / 1600.0) * (self.height / 900.0))
            .sqrt()
            .clamp(0.72, 2.40);

        let max_columns = (MAX_STREAMS / self.layers.max(1)).max(1);

        self.columns = (self.width / (16.0 * visual_scale)).ceil().max(1.0) as usize;

        self.columns = self.columns.min(max_columns);

        // Five persistent depth layers.
        let desired_streams = self.columns.saturating_mul(self.layers);

        if self.streams.len() < desired_streams {
            while self.streams.len() < desired_streams {
                let index = self.streams.len();
                let stream = self.create_stream(index, true);
                self.streams.push(stream);
            }
        } else {
            self.streams.truncate(desired_streams);
        }

        let width = self.width;
        let columns = self.columns;
        let layers = self.layers;

        for (index, stream) in self.streams.iter_mut().enumerate() {
            let column = index % columns;
            let layer = (index / columns).min(layers - 1);

            if index < existing_streams {
                stream.head *= height_ratio;
                stream.lane_offset *= width_ratio;
            }

            stream.depth = if layers > 1 {
                layer as f32 / (layers - 1) as f32
            } else {
                1.0
            };

            let lane_width = width / columns as f32;

            stream.lane_offset = stream
                .lane_offset
                .clamp(-lane_width * 0.42, lane_width * 0.42);

            let depth_stagger = layer as f32 * lane_width * 0.37;

            stream.x = ((column as f32 + 0.5) * lane_width + depth_stagger + stream.lane_offset)
                .rem_euclid(width);
        }
    }

    pub fn update(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);

        self.weather_time = (self.weather_time + dt).rem_euclid(10_000.0);

        // Copy these before borrowing individual streams mutably.
        let weather_time = self.weather_time;
        let weather_width = self.width;

        let visual_scale = ((self.width / 1600.0) * (self.height / 900.0))
            .sqrt()
            .clamp(0.72, 2.40);

        for index in 0..self.streams.len() {
            let mut should_respawn = false;
            let mut should_start_cascade = false;
            let mut should_finish_cascade = false;

            {
                let stream = &mut self.streams[index];

                stream.age += dt;
                stream.phase += dt;
                stream.mutation_timer -= dt;
                stream.cascade_timer -= dt;

                if stream.cascade_intensity > 0.0 {
                    stream.cascade_position += stream.cascade_speed * dt;

                    if stream.cascade_position > 1.12 {
                        should_finish_cascade = true;
                    }
                }

                let personality_speed = match stream.personality {
                    Personality::Steady => 1.0,
                    Personality::Fast => 1.48,
                    Personality::Lazy => 0.64,
                    Personality::Nervous => 1.0 + (stream.phase * 8.7).sin() * 0.17,
                    Personality::Pulse => 0.94 + (stream.phase * 2.1).sin() * 0.11,
                    Personality::Ghost => 0.76,
                };

                // Background streams move more slowly.
                let depth_speed = 0.52 + stream.depth * 0.70;

                // Nearby streams sample the same broad environmental field.
                // This produces moving rain fronts instead of independent noise.
                let weather = sample_weather(stream.x, stream.depth, weather_time, weather_width);

                stream.head += stream.speed
                    * personality_speed
                    * depth_speed
                    * weather.speed
                    * visual_scale
                    * dt;

                let drift_wave = (stream.phase * (0.22 + stream.depth * 0.16)).sin();

                let nervous_jitter = match stream.personality {
                    Personality::Nervous => (stream.phase * 3.7).sin() * 0.35,
                    _ => 0.0,
                };

                stream.x +=
                    (stream.drift * drift_wave + nervous_jitter + weather.wind) * visual_scale * dt;

                stream.x = stream.x.rem_euclid(self.width);

                let fade_in = (stream.age / 0.55).clamp(0.0, 1.0);

                let remaining = stream.lifetime - stream.age;

                let fade_out = (remaining / 1.0).clamp(0.0, 1.0);

                // Keep ordinary stream luminance stable. Expressive
                // brightness returns through coherent weather and rare
                // cascades instead of independent rapid pulsing.
                let pulse = 1.0;

                let personality_brightness = match stream.personality {
                    Personality::Ghost => 0.32,
                    Personality::Lazy => 0.74,
                    Personality::Fast => 1.08,
                    _ => 1.0,
                };

                // Distant streams remain dim while foreground streams pop.
                let depth_brightness = 0.26 + stream.depth * 0.86;

                stream.brightness = fade_in
                    * fade_out
                    * pulse
                    * personality_brightness
                    * depth_brightness
                    * weather.brightness;

                if stream.cascade_intensity <= 0.0
                    && stream.cascade_timer <= 0.0
                    && stream.age > 1.0
                    && remaining > 2.0
                    && stream.brightness > 0.18
                {
                    should_start_cascade = true;
                }

                let glyph_height = (13.0 + stream.depth * 15.0) * visual_scale;

                let trail_height = stream.length as f32 * glyph_height;

                if stream.head - trail_height > self.height || stream.age >= stream.lifetime {
                    should_respawn = true;
                }
            }

            if self.streams[index].mutation_timer <= 0.0 {
                self.mutate_stream(index);
            }

            if should_respawn {
                self.streams[index] = self.create_stream(index, false);
                continue;
            }

            if should_finish_cascade {
                self.finish_cascade(index);
            } else if should_start_cascade {
                self.start_cascade(index);
            }
        }
    }

    fn next_mutation_delay(&mut self, personality: Personality, depth: f32) -> f32 {
        let base = match personality {
            Personality::Steady => self.rng.range(7.0, 14.0),
            Personality::Fast => self.rng.range(4.5, 9.0),
            Personality::Lazy => self.rng.range(12.0, 24.0),
            Personality::Nervous => self.rng.range(3.0, 6.0),
            Personality::Pulse => self.rng.range(6.0, 12.0),
            Personality::Ghost => self.rng.range(14.0, 28.0),
        };

        base * (1.80 - depth.clamp(0.0, 1.0) * 0.80)
    }

    fn next_cascade_delay(&mut self, personality: Personality) -> f32 {
        match personality {
            Personality::Steady => self.rng.range(50.0, 100.0),
            Personality::Fast => self.rng.range(35.0, 80.0),
            Personality::Lazy => self.rng.range(70.0, 140.0),
            Personality::Nervous => self.rng.range(30.0, 70.0),
            Personality::Pulse => self.rng.range(15.0, 35.0),
            Personality::Ghost => self.rng.range(90.0, 180.0),
        }
    }

    fn start_cascade(&mut self, index: usize) {
        let personality = self.streams[index].personality;

        let cascade_speed = match personality {
            Personality::Fast => self.rng.range(0.95, 1.45),
            Personality::Lazy => self.rng.range(0.58, 0.90),
            Personality::Nervous => self.rng.range(0.90, 1.38),
            Personality::Pulse => self.rng.range(0.78, 1.28),
            Personality::Ghost => self.rng.range(0.62, 0.96),
            Personality::Steady => self.rng.range(0.72, 1.12),
        };

        let cascade_intensity = match personality {
            Personality::Pulse => self.rng.range(1.10, 1.48),
            Personality::Ghost => self.rng.range(0.62, 0.92),
            _ => self.rng.range(0.86, 1.24),
        };

        let stream = &mut self.streams[index];

        stream.cascade_position = -0.08;
        stream.cascade_speed = cascade_speed;
        stream.cascade_intensity = cascade_intensity;
    }

    fn finish_cascade(&mut self, index: usize) {
        let personality = self.streams[index].personality;
        let next_delay = self.next_cascade_delay(personality);

        let stream = &mut self.streams[index];

        stream.cascade_position = -1.0;
        stream.cascade_speed = 0.0;
        stream.cascade_intensity = 0.0;
        stream.cascade_timer = next_delay;
    }

    fn mutate_stream(&mut self, index: usize) {
        let personality = self.streams[index].personality;
        let depth = self.streams[index].depth;
        let visible_length = (self.streams[index].length as usize).min(GLYPHS_PER_STREAM);

        const PROTECTED_HEAD_GLYPHS: usize = 5;

        if visible_length > PROTECTED_HEAD_GLYPHS {
            let mutable_count = visible_length - PROTECTED_HEAD_GLYPHS;
            let glyph_index = PROTECTED_HEAD_GLYPHS + self.rng.usize(mutable_count);

            self.streams[index].glyphs[glyph_index] = self.rng.u32(64);
        }

        let next_delay = self.next_mutation_delay(personality, depth);
        self.streams[index].mutation_timer = next_delay;
    }

    fn create_stream(&mut self, index: usize, initial: bool) -> Stream {
        let personality = match self.rng.u32(100) {
            0..=34 => Personality::Steady,
            35..=49 => Personality::Fast,
            50..=64 => Personality::Lazy,
            65..=76 => Personality::Nervous,
            77..=89 => Personality::Pulse,
            _ => Personality::Ghost,
        };

        let column = index % self.columns.max(1);
        let layer = (index / self.columns.max(1)).min(self.layers - 1);

        let depth = if self.layers > 1 {
            layer as f32 / (self.layers - 1) as f32
        } else {
            1.0
        };

        let lane_width = self.width / self.columns.max(1) as f32;

        let lane_offset = self.rng.range(-lane_width * 0.42, lane_width * 0.42);

        let depth_stagger = layer as f32 * lane_width * 0.37;

        let x = ((column as f32 + 0.5) * lane_width + depth_stagger + lane_offset)
            .rem_euclid(self.width);

        let head = if initial {
            self.rng.range(-self.height, self.height * 1.15)
        } else {
            self.rng.range(-self.height * 0.8, -35.0)
        };

        let mut glyphs = [0u32; GLYPHS_PER_STREAM];

        for glyph in &mut glyphs {
            *glyph = self.rng.u32(64);
        }

        let length = match personality {
            Personality::Fast => self.rng.u32_range(9, 23),
            Personality::Lazy => self.rng.u32_range(24, 52),
            Personality::Ghost => self.rng.u32_range(16, 45),
            _ => self.rng.u32_range(13, 38),
        };

        let speed = match personality {
            Personality::Fast => self.rng.range(150.0, 245.0),
            Personality::Lazy => self.rng.range(42.0, 86.0),
            Personality::Nervous => self.rng.range(95.0, 175.0),
            Personality::Pulse => self.rng.range(78.0, 145.0),
            Personality::Ghost => self.rng.range(52.0, 108.0),
            Personality::Steady => self.rng.range(74.0, 158.0),
        };

        let initial_delay_scale = if initial {
            self.rng.range(0.08, 1.0)
        } else {
            1.0
        };

        let cascade_timer = self.next_cascade_delay(personality) * initial_delay_scale;

        Stream {
            x,
            head,
            speed,
            length,
            brightness: 0.0,
            glyphs,
            personality,
            mutation_timer: {
                let initial_scale = self.rng.range(0.35, 1.0);
                self.next_mutation_delay(personality, depth) * initial_scale
            },
            age: if initial {
                self.rng.range(0.0, 4.0)
            } else {
                0.0
            },
            lifetime: self.rng.range(8.0, 24.0),
            phase: self.rng.range(0.0, std::f32::consts::TAU),
            depth,
            drift: self.rng.range(-3.2, 3.2),
            lane_offset,
            cascade_position: -1.0,
            cascade_speed: 0.0,
            cascade_intensity: 0.0,
            cascade_timer,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct WeatherSample {
    speed: f32,
    brightness: f32,

    // Horizontal velocity in pixels per second.
    wind: f32,
}

fn sample_weather(x: f32, depth: f32, time: f32, width: f32) -> WeatherSample {
    let normalized_x = x / width.max(1.0);
    let tau = std::f32::consts::TAU;

    // Roughly one broad front spans the screen. It moves slowly from
    // left to right, brightening and accelerating many streams together.
    let front_phase = normalized_x * tau * 1.35 - time * 0.18;
    let front_wave = 0.5 + 0.5 * front_phase.sin();

    // Smoothstep keeps the front broad while giving it a recognizable center.
    let front = front_wave * front_wave * (3.0 - 2.0 * front_wave);

    // Smaller regional variation prevents the front from looking like a
    // perfectly uniform vertical brightness mask.
    let turbulence = (normalized_x * tau * 2.7 + time * 0.31 + depth * 0.85).sin();

    // A very slow wind layer gently shifts whole regions sideways.
    let wind_wave = (normalized_x * tau * 0.62 - time * 0.07).sin();

    WeatherSample {
        speed: (0.82 + front * 0.38 + turbulence * 0.08).clamp(0.68, 1.32),
        brightness: (0.78 + front * 0.62 + turbulence * 0.10).clamp(0.62, 1.55),
        wind: wind_wave * 4.5 + turbulence * 2.0 * (0.4 + depth * 0.6),
    }
}

struct Random {
    state: u64,
}

impl Random {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next_u64(&mut self) -> u64 {
        let mut value = self.state;

        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;

        self.state = value;

        value.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn unit(&mut self) -> f32 {
        let value = self.next_u64() >> 40;
        value as f32 / 16_777_216.0
    }

    fn range(&mut self, minimum: f32, maximum: f32) -> f32 {
        minimum + (maximum - minimum) * self.unit()
    }

    fn u32(&mut self, maximum_exclusive: u32) -> u32 {
        if maximum_exclusive == 0 {
            return 0;
        }

        (self.next_u64() % maximum_exclusive as u64) as u32
    }

    fn u32_range(&mut self, minimum: u32, maximum_exclusive: u32) -> u32 {
        minimum + self.u32(maximum_exclusive.saturating_sub(minimum))
    }

    fn usize(&mut self, maximum_exclusive: usize) -> usize {
        if maximum_exclusive == 0 {
            return 0;
        }

        (self.next_u64() % maximum_exclusive as u64) as usize
    }

    fn chance(&mut self, probability: f32) -> bool {
        self.unit() < probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_move_and_persist() {
        let mut simulation = Simulation::new(1920, 1080);

        assert!(!simulation.streams.is_empty());

        let original_head = simulation.streams[0].head;
        let original_glyphs = simulation.streams[0].glyphs;

        simulation.update(1.0 / 60.0);

        assert_ne!(simulation.streams[0].head, original_head);

        let changed = simulation.streams[0]
            .glyphs
            .iter()
            .zip(original_glyphs.iter())
            .filter(|(left, right)| left != right)
            .count();

        assert!(changed <= 3);
    }
}
