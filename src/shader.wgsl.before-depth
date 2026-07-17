struct Uniforms {
    time: f32,
    aspect: f32,
    resolution: vec2<f32>,
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

        let stream_energy =
            brightness
            * trail_fade;

        core_energy +=
            glyph_core
            * stream_energy;

        glow_energy +=
            glyph_glow
            * stream_energy
            * 0.42;

        if (segment == 0u) {
            head_energy +=
                glyph_core
                * brightness
                * 1.8;
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

    var color =
        background
        + green * core_energy
        + glow_green * glow_energy
        + white_head * head_energy;

    /*
    Soft compression keeps overlapping streams bright
    without clipping everything directly to white.
    */
    color =
        vec3<f32>(1.0)
        - exp(-color);

    return vec4<f32>(
        color,
        1.0,
    );
}
