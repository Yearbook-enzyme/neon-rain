struct Uniforms {
    time: f32,
    aspect: f32,
    resolution: vec2<f32>,
    controls: vec4<f32>,
    stream_count: u32,
    padding0: u32,
    padding1: u32,
    padding2: u32,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var glyph_atlas: texture_2d<f32>;

@group(0) @binding(2)
var glyph_sampler: sampler;

struct GpuStream {
    // x, head, speed, brightness
    position: vec4<f32>,

    // length, personality, age, phase
    parameters: vec4<f32>,


    extras: vec4<f32>,

    glyphs: array<u32, 64>,
};

@group(0) @binding(3)
var<storage, read> streams: array<GpuStream>;

struct VertexOutput {
    @builtin(position)
    position: vec4<f32>,

    @location(0)
    uv: vec2<f32>,
};

fn random(value: f32) -> f32 {
    return fract(
        sin(value * 12.9898)
        * 43758.5453
    );
}

fn rectangle_distance(
    point: vec2<f32>,
    half_size: vec2<f32>,
) -> f32 {
    let distance =
        abs(point) - half_size;

    let outside =
        length(
            max(
                distance,
                vec2<f32>(0.0),
            ),
        );

    let inside =
        min(
            max(
                distance.x,
                distance.y,
            ),
            0.0,
        );

    return outside + inside;
}

fn atlas_uv(
    local_uv: vec2<f32>,
    glyph_index: u32,
) -> vec2<f32> {
    let atlas_columns = 8u;
    let atlas_rows = 8u;

    let glyph_column =
        glyph_index % atlas_columns;

    let glyph_row =
        glyph_index / atlas_columns;

    let cell_size =
        vec2<f32>(
            1.0 / f32(atlas_columns),
            1.0 / f32(atlas_rows),
        );

    let cell_origin =
        vec2<f32>(
            f32(glyph_column),
            f32(glyph_row),
        ) * cell_size;

    /*
    Leave a small inset so linear filtering does not
    sample a neighboring atlas cell.
    */
    let inset =
        vec2<f32>(
            0.00390625,
            0.003125,
        );

    return
        cell_origin
        + inset
        + local_uv
            * (
                cell_size
                - inset * 2.0
            );
}

fn sample_glyph(
    local_point: vec2<f32>,
    glyph_index: u32,
) -> f32 {
    let local_uv =
        local_point * 0.5
        + vec2<f32>(0.5);

    if (
        local_uv.x < 0.0
        || local_uv.x > 1.0
        || local_uv.y < 0.0
        || local_uv.y > 1.0
    ) {
        return 0.0;
    }

    let uv =
        atlas_uv(
            local_uv,
            glyph_index,
        );

    return textureSample(
        glyph_atlas,
        glyph_sampler,
        uv,
    ).r;
}

fn sample_glyph_glow(
    local_point: vec2<f32>,
    glyph_index: u32,
    radius: f32,
) -> f32 {
    let center =
        sample_glyph(
            local_point,
            glyph_index,
        );

    let horizontal =
        sample_glyph(
            local_point
                + vec2<f32>(
                    radius,
                    0.0,
                ),
            glyph_index,
        )
        + sample_glyph(
            local_point
                - vec2<f32>(
                    radius,
                    0.0,
                ),
            glyph_index,
        );

    let vertical =
        sample_glyph(
            local_point
                + vec2<f32>(
                    0.0,
                    radius,
                ),
            glyph_index,
        )
        + sample_glyph(
            local_point
                - vec2<f32>(
                    0.0,
                    radius,
                ),
            glyph_index,
        );

    let diagonal =
        sample_glyph(
            local_point
                + vec2<f32>(
                    radius,
                    radius,
                ),
            glyph_index,
        )
        + sample_glyph(
            local_point
                + vec2<f32>(
                    -radius,
                    radius,
                ),
            glyph_index,
        )
        + sample_glyph(
            local_point
                + vec2<f32>(
                    radius,
                    -radius,
                ),
            glyph_index,
        )
        + sample_glyph(
            local_point
                - vec2<f32>(
                    radius,
                    radius,
                ),
            glyph_index,
        );

    return
        center * 0.28
        + horizontal * 0.13
        + vertical * 0.13
        + diagonal * 0.055;
}

@vertex
fn vs_main(
    @builtin(vertex_index)
    vertex_index: u32,
) -> VertexOutput {
    var positions =
        array<vec2<f32>, 3>(
            vec2<f32>(-1.0, -1.0),
            vec2<f32>(3.0, -1.0),
            vec2<f32>(-1.0, 3.0),
        );

    let position =
        positions[vertex_index];

    var output: VertexOutput;

    output.position =
        vec4<f32>(
            position,
            0.0,
            1.0,
        );

    output.uv =
        position * 0.5
        + vec2<f32>(0.5);

    return output;
}

@fragment
fn fs_main(
    input: VertexOutput,
) -> @location(0) vec4<f32> {
    let background =
        vec3<f32>(
            0.00025,
            0.0022,
            0.0007,
        );

    let pixel =
        vec2<f32>(
            input.uv.x * uniforms.resolution.x,
            input.uv.y * uniforms.resolution.y,
        );

    var core_energy = 0.0;
    var glow_energy = 0.0;
    var head_energy = 0.0;
    var cascade_energy = 0.0;

    let glyph_width = 16.0;
    let glyph_height = 24.0;

    for (
        var stream_index = 0u;
        stream_index < 512u;
        stream_index = stream_index + 1u
    ) {
        if (
            stream_index
            >= uniforms.stream_count
        ) {
            break;
        }

        let stream =
            streams[stream_index];


        let depth =
            stream.extras.x;

        let cascade_position =
            stream.extras.y;

        let cascade_intensity =
            stream.extras.z;

        let stream_x =
            stream.position.x;

        let stream_head =
            stream.position.y;

        let brightness =
            stream.position.w;

        let stream_length =
            u32(stream.parameters.x);

        let horizontal_distance =
            abs(pixel.x - stream_x);

        /*
        Skip texture work for pixels nowhere near
        this stream.
        */
        if (
            horizontal_distance
            > glyph_width * 1.8
        ) {
            continue;
        }

        let distance_behind_head =
            stream_head - pixel.y;

        if (
            distance_behind_head
            < -glyph_height * 0.55
        ) {
            continue;
        }

        let segment_float =
            floor(
                distance_behind_head
                / glyph_height
                + 0.5
            );

        if (segment_float < 0.0) {
            continue;
        }

        let segment =
            u32(segment_float);

        if (segment >= stream_length) {
            continue;
        }

        let glyph_center_y =
            stream_head
            - f32(segment)
                * glyph_height;

        let local_point =
            vec2<f32>(
                (pixel.x - stream_x)
                    / (glyph_width * 0.5),

                (pixel.y - glyph_center_y)
                    / (glyph_height * 0.5),
            );

        let glyph_index =
            stream.glyphs[
                segment % 64u
            ];

        let trail_position =
            f32(segment)
            / max(
                f32(stream_length - 1u),
                1.0,
            );

        let trail_fade =
            pow(
                1.0 - trail_position,
                1.65,
            );

        // A rare simulation-driven energy packet travels from the
        // stream head toward its tail. The narrow core gives it a
        // recognizable location while the asymmetric wake leaves a
        // small amount of residual light behind it.
        let cascade_delta =
            trail_position - cascade_position;

        let cascade_core =
            exp(
                -cascade_delta
                * cascade_delta
                * 360.0
            );

        let cascade_wake_mask =
            select(
                0.0,
                1.0,
                cascade_delta < 0.0,
            );

        let cascade_wake =
            cascade_wake_mask
            * exp(cascade_delta * 14.0);

        let cascade_packet =
            cascade_intensity
            * (
                cascade_core * 1.20
                + cascade_wake * 0.16
            );

        let glyph_core =
            sample_glyph(
                local_point,
                glyph_index,
            );

        let glyph_glow =
            sample_glyph_glow(
                local_point,
                glyph_index,
                0.12,
            );

        // Real layer depth:
        // 0.0 = distant, 1.0 = foreground.
        let atmosphere =
            mix(
                0.58,
                1.0,
                pow(
                    clamp(depth, 0.0, 1.0),
                    0.75,
                ),
            );

        // A packet of energy travels backward through the trail.
        // The asymmetric shaping keeps it from looking like a uniform
        // brightness oscillation across the entire stream.
        let pulse_phase =
            uniforms.time
                * (5.0 + stream.parameters.y * 0.32)
            - trail_position * 15.0
            + stream.parameters.w * 6.2831853;

        let pulse_wave =
            0.5 + 0.5 * sin(pulse_phase);

        let electrical_pulse =
            0.76
            + 0.30
                * pow(
                    pulse_wave,
                    3.0,
                );

        // Shimmer changes only several times per second. This gives
        // individual glyphs slight electrical instability without
        // creating frame-by-frame television noise.
        let shimmer_frame =
            floor(
                uniforms.time
                * (6.0 + stream.parameters.y * 0.35)
            );

        let shimmer_seed =
            f32(stream_index) * 131.0
            + f32(segment) * 17.0
            + stream.parameters.w * 29.0
            + shimmer_frame * 0.73;

        let glyph_variation =
            mix(
                0.86,
                1.10,
                random(shimmer_seed),
            );

        // Energy originates near the head and diffuses into the trail.
        let head_injection =
            exp(
                -f32(segment) * 0.38
            );

        let propagation_profile =
            0.80
            + head_injection * 0.42;

        let stream_energy =
            brightness
            * trail_fade
            * electrical_pulse
            * glyph_variation
            * propagation_profile
            * atmosphere;

        core_energy +=
            glyph_core
            * stream_energy
            * (
                1.0
                + cascade_packet * 0.35
            );

        glow_energy +=
            glyph_glow
            * (
                stream_energy * 0.42
                + brightness
                    * atmosphere
                    * cascade_packet
                    * 0.50
            );

        cascade_energy +=
            glyph_core
            * brightness
            * atmosphere
            * glyph_variation
            * cascade_packet
            * (
                0.45
                + trail_fade * 0.55
            );

        if (segment == 0u) {
            let head_flash =
                1.85
                + electrical_pulse * 0.45;

            head_energy +=
                glyph_core
                * brightness
                * atmosphere
                * glyph_variation
                * head_flash;
        }
    }

    let green =
        vec3<f32>(
            0.03,
            1.0,
            0.27,
        );

    let glow_green =
        vec3<f32>(
            0.0,
            0.38,
            0.075,
        );

    let white_head =
        vec3<f32>(
            0.78,
            1.0,
            0.84,
        );

    let glow_control =
        uniforms.controls.y;

    var color =
        background
        + green * core_energy
        + glow_green
            * glow_energy
            * glow_control
        + white_head * head_energy
        + white_head
            * cascade_energy
            * 0.82;

    /*
    Soft compression keeps overlapping streams bright
    without clipping everything directly to white.
    */
    let exposure =
        max(uniforms.controls.z, 0.01);

    color =
        vec3<f32>(1.0)
        - exp(-color * exposure);

    return vec4<f32>(
        color,
        1.0,
    );
}
