pub const GLYPHS_PER_STREAM: usize = 64;

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
}

pub struct Simulation {
    pub streams: Vec<Stream>,
    rng: Random,
    width: f32,
    height: f32,
    columns: usize,
    layers: usize,
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
        };

        simulation.resize(width, height);
        simulation
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1) as f32;
        self.height = height.max(1) as f32;

        // Slightly denser than before.
        self.columns = (self.width / 16.0).ceil().max(1.0) as usize;

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

            stream.depth = if layers > 1 {
                layer as f32 / (layers - 1) as f32
            } else {
                1.0
            };

            let lane_width = width / columns as f32;

            let depth_stagger = layer as f32 * lane_width * 0.37;

            stream.x = ((column as f32 + 0.5) * lane_width + depth_stagger + stream.lane_offset)
                .rem_euclid(width);
        }
    }

    pub fn update(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);

        for index in 0..self.streams.len() {
            let mut should_respawn = false;

            {
                let stream = &mut self.streams[index];

                stream.age += dt;
                stream.phase += dt;
                stream.mutation_timer -= dt;

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

                stream.head += stream.speed * personality_speed * depth_speed * dt;

                let drift_wave = (stream.phase * (0.22 + stream.depth * 0.16)).sin();

                let nervous_jitter = match stream.personality {
                    Personality::Nervous => (stream.phase * 3.7).sin() * 0.35,
                    _ => 0.0,
                };

                stream.x += (stream.drift * drift_wave + nervous_jitter) * dt;

                stream.x = stream.x.rem_euclid(self.width);

                let fade_in = (stream.age / 0.55).clamp(0.0, 1.0);

                let remaining = stream.lifetime - stream.age;

                let fade_out = (remaining / 1.0).clamp(0.0, 1.0);

                let pulse = match stream.personality {
                    Personality::Pulse => 0.66 + 0.34 * (stream.phase * 3.2).sin().abs(),
                    Personality::Nervous => 0.78 + 0.22 * (stream.phase * 10.0).sin().abs(),
                    _ => 1.0,
                };

                let personality_brightness = match stream.personality {
                    Personality::Ghost => 0.32,
                    Personality::Lazy => 0.74,
                    Personality::Fast => 1.08,
                    _ => 1.0,
                };

                // Distant streams remain dim while foreground streams pop.
                let depth_brightness = 0.26 + stream.depth * 0.86;

                stream.brightness =
                    fade_in * fade_out * pulse * personality_brightness * depth_brightness;

                let glyph_height = 13.0 + stream.depth * 15.0;

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
            }
        }
    }

    fn mutate_stream(&mut self, index: usize) {
        let mutation_count = match self.streams[index].personality {
            Personality::Nervous => {
                if self.rng.chance(0.30) {
                    3
                } else {
                    2
                }
            }
            Personality::Fast => 2,
            _ => 1,
        };

        for _ in 0..mutation_count {
            let glyph_index = self.rng.usize(GLYPHS_PER_STREAM);

            self.streams[index].glyphs[glyph_index] = self.rng.u32(64);
        }

        self.streams[index].mutation_timer = match self.streams[index].personality {
            Personality::Steady => self.rng.range(0.18, 0.65),
            Personality::Fast => self.rng.range(0.08, 0.28),
            Personality::Lazy => self.rng.range(0.55, 1.45),
            Personality::Nervous => self.rng.range(0.035, 0.14),
            Personality::Pulse => self.rng.range(0.20, 0.70),
            Personality::Ghost => self.rng.range(0.65, 1.80),
        };
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

        Stream {
            x,
            head,
            speed,
            length,
            brightness: 0.0,
            glyphs,
            personality,
            mutation_timer: self.rng.range(0.02, 0.8),
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
        }
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
