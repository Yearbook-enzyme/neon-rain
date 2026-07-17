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
}

pub struct Simulation {
    pub streams: Vec<Stream>,
    rng: Random,
    width: f32,
    height: f32,
    columns: usize,
}

impl Simulation {
    pub fn new(width: u32, height: u32) -> Self {
        let mut simulation = Self {
            streams: Vec::new(),
            rng: Random::new(0x4d41_5452_4958_2026),
            width: width.max(1) as f32,
            height: height.max(1) as f32,
            columns: 0,
        };

        simulation.resize(width, height);
        simulation
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1) as f32;
        self.height = height.max(1) as f32;

        // Roughly one stream lane every 18 pixels.
        let desired_columns = (self.width / 18.0).ceil() as usize;
        let desired_streams = desired_columns.saturating_mul(3);

        self.columns = desired_columns.max(1);

        if self.streams.len() < desired_streams {
            while self.streams.len() < desired_streams {
                let index = self.streams.len();
                let stream = self.create_stream(index, true);
                self.streams.push(stream);
            }
        } else {
            self.streams.truncate(desired_streams);
        }

        let columns = self.columns;
        let width = self.width;

        for (index, stream) in self.streams.iter_mut().enumerate() {
            let column = index % columns;
            let layer = index / columns;

            let lane_width = width / columns as f32;
            let layer_offset = layer as f32 * lane_width * 0.27;

            stream.x = ((column as f32 + 0.5) * lane_width + layer_offset) % width;
        }
    }

    pub fn update(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);

        for index in 0..self.streams.len() {
            let mut respawn = false;

            {
                let stream = &mut self.streams[index];

                stream.age += dt;
                stream.phase += dt;
                stream.mutation_timer -= dt;

                let personality_speed = match stream.personality {
                    Personality::Steady => 1.0,
                    Personality::Fast => 1.55,
                    Personality::Lazy => 0.62,
                    Personality::Nervous => 1.0 + (stream.phase * 9.0).sin() * 0.18,
                    Personality::Pulse => 0.92 + (stream.phase * 2.2).sin() * 0.12,
                    Personality::Ghost => 0.78,
                };

                stream.head += stream.speed * personality_speed * dt;

                let fade_in = (stream.age / 0.45).clamp(0.0, 1.0);
                let remaining = stream.lifetime - stream.age;
                let fade_out = (remaining / 0.8).clamp(0.0, 1.0);

                let pulse = match stream.personality {
                    Personality::Pulse => 0.72 + 0.28 * (stream.phase * 3.4).sin().abs(),
                    Personality::Nervous => 0.82 + 0.18 * (stream.phase * 11.0).sin().abs(),
                    _ => 1.0,
                };

                let personality_brightness = match stream.personality {
                    Personality::Ghost => 0.32,
                    Personality::Lazy => 0.72,
                    Personality::Fast => 1.08,
                    _ => 1.0,
                };

                stream.brightness = fade_in * fade_out * pulse * personality_brightness;

                let trail_height = stream.length as f32 * 24.0;

                if stream.head - trail_height > self.height || stream.age >= stream.lifetime {
                    respawn = true;
                }
            }

            if self.streams[index].mutation_timer <= 0.0 {
                self.mutate_stream(index);
            }

            if respawn {
                self.streams[index] = self.create_stream(index, false);
            }
        }
    }

    fn mutate_stream(&mut self, index: usize) {
        let mutation_count = match self.streams[index].personality {
            Personality::Nervous => {
                if self.rng.chance(0.3) {
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

        let next_timer = match self.streams[index].personality {
            Personality::Steady => self.rng.range(0.18, 0.65),
            Personality::Fast => self.rng.range(0.08, 0.28),
            Personality::Lazy => self.rng.range(0.55, 1.45),
            Personality::Nervous => self.rng.range(0.035, 0.14),
            Personality::Pulse => self.rng.range(0.2, 0.7),
            Personality::Ghost => self.rng.range(0.65, 1.8),
        };

        self.streams[index].mutation_timer = next_timer;
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
        let layer = index / self.columns.max(1);

        let lane_width = self.width / self.columns.max(1) as f32;
        let layer_offset = layer as f32 * lane_width * 0.27;

        let x = ((column as f32 + 0.5) * lane_width + layer_offset) % self.width;

        let head = if initial {
            self.rng.range(-self.height, self.height)
        } else {
            self.rng.range(-self.height * 0.7, -30.0)
        };

        let mut glyphs = [0u32; GLYPHS_PER_STREAM];

        for glyph in &mut glyphs {
            *glyph = self.rng.u32(64);
        }

        let length = match personality {
            Personality::Fast => self.rng.u32_range(8, 19),
            Personality::Lazy => self.rng.u32_range(24, 49),
            Personality::Ghost => self.rng.u32_range(14, 42),
            _ => self.rng.u32_range(12, 34),
        };

        let speed = match personality {
            Personality::Fast => self.rng.range(150.0, 245.0),
            Personality::Lazy => self.rng.range(38.0, 82.0),
            Personality::Nervous => self.rng.range(95.0, 175.0),
            Personality::Pulse => self.rng.range(75.0, 145.0),
            Personality::Ghost => self.rng.range(52.0, 105.0),
            Personality::Steady => self.rng.range(72.0, 155.0),
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
            lifetime: self.rng.range(7.0, 22.0),
            phase: self.rng.range(0.0, std::f32::consts::TAU),
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
        // xorshift64*
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

        // Most frames should retain the same stored sequence.
        // Mutations happen only when a stream timer expires.
        let changed = simulation.streams[0]
            .glyphs
            .iter()
            .zip(original_glyphs.iter())
            .filter(|(left, right)| left != right)
            .count();

        assert!(changed <= 3);
    }
}
