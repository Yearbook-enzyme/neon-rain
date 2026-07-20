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

struct VertexInput {
    @location(0)
    position_size: vec4<f32>,

    @location(1)
    color_glow: vec4<f32>,

    @location(2)
    glow_color: vec4<f32>,

    @location(3)
    glyph_data: vec4<u32>,
};

struct VertexOutput {
    @builtin(position)
    position: vec4<f32>,

    @location(0)
    local_point: vec2<f32>,

    @location(1)
    color_glow: vec4<f32>,

    @location(2)
    @interpolate(flat)
    glyph_index: u32,

    @location(3)
    @interpolate(flat)
    depth_band: u32,

    @location(4)
    glow_color: vec3<f32>,

    @location(5)
    depth01: f32,

    @location(6)
    @interpolate(flat)
    render_kind: u32,
};

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

    let inset =
        vec2<f32>(
            0.00390625,
            0.003125,
        );

    return
        cell_origin
        + inset
        + local_uv
            * (cell_size - inset * 2.0);
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

    return textureSample(
        glyph_atlas,
        glyph_sampler,
        atlas_uv(local_uv, glyph_index),
    ).r;
}

fn sample_glyph_glow(
    local_point: vec2<f32>,
    glyph_index: u32,
    radius: f32,
) -> f32 {

    let center =
        sample_glyph(local_point, glyph_index);

    let horizontal =
        sample_glyph(
            local_point + vec2<f32>(radius, 0.0),
            glyph_index,
        )
        + sample_glyph(
            local_point - vec2<f32>(radius, 0.0),
            glyph_index,
        );

    let vertical =
        sample_glyph(
            local_point + vec2<f32>(0.0, radius),
            glyph_index,
        )
        + sample_glyph(
            local_point - vec2<f32>(0.0, radius),
            glyph_index,
        );

    let diagonal =
        sample_glyph(
            local_point + vec2<f32>(radius, radius),
            glyph_index,
        )
        + sample_glyph(
            local_point + vec2<f32>(-radius, radius),
            glyph_index,
        )
        + sample_glyph(
            local_point + vec2<f32>(radius, -radius),
            glyph_index,
        )
        + sample_glyph(
            local_point - vec2<f32>(radius, radius),
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
    input: VertexInput,
) -> VertexOutput {
    var quad_uv = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );

    let uv = quad_uv[vertex_index];

    // Leave room around each glyph for the inexpensive local
    // glow taps while still drawing only one small quad per glyph.
    let local_point =
        (uv * 2.0 - vec2<f32>(1.0)) * 1.28;

    let pixel_position =
        input.position_size.xy
        + local_point
            * input.position_size.zw
            * 0.5;

    let safe_resolution =
        max(uniforms.resolution, vec2<f32>(1.0));

    let ndc_position =
        vec2<f32>(
            pixel_position.x / safe_resolution.x
                * 2.0 - 1.0,
            1.0 - pixel_position.y / safe_resolution.y
                * 2.0,
        );

    var output: VertexOutput;
    output.position =
        vec4<f32>(ndc_position, 0.0, 1.0);
    output.local_point = local_point;
    output.color_glow = input.color_glow;
    output.glyph_index = input.glyph_data.x;
    output.depth_band = input.glyph_data.y;
    output.glow_color = input.glow_color.rgb;
    output.depth01 =
        clamp(
            input.glow_color.a,
            0.0,
            1.0,
        );
    output.render_kind = input.glyph_data.z;

    return output;
}

@fragment
fn fs_main(
    input: VertexOutput,
) -> @location(0) vec4<f32> {
    if (input.render_kind == 1u) {
        let point = abs(input.local_point);

        let horizontal =
            (1.0 - smoothstep(0.045, 0.105, point.y))
            * smoothstep(0.18, 0.28, point.x)
            * (1.0 - smoothstep(0.64, 0.82, point.x));

        let vertical =
            (1.0 - smoothstep(0.045, 0.105, point.x))
            * smoothstep(0.18, 0.28, point.y)
            * (1.0 - smoothstep(0.64, 0.82, point.y));

        let reticle = max(horizontal, vertical);

        if (reticle < 0.001) {
            discard;
        }

        let reticle_color =
            input.color_glow.rgb * reticle
            + input.glow_color * input.color_glow.a * reticle * 0.35;

        return vec4<f32>(reticle_color, 1.0);
    }

    let depth01 =
        input.depth01;

    let glyph_sample =
        sample_glyph(
            input.local_point,
            input.glyph_index,
        );

    // Distant glyphs carry a slightly broader local halo.
    // Foreground glyphs remain tighter and more legible.
    let glow_radius =
        mix(
            0.155,
            0.095,
            depth01,
        );

    let glyph_glow =
        sample_glyph_glow(
            input.local_point,
            input.glyph_index,
            glow_radius,
        );

    // Mix a little halo into the distant glyph core, creating
    // atmospheric softness without blurring the entire frame.
    let softened_core =
        max(
            glyph_sample * 0.82,
            glyph_glow * 0.22,
        );

    let focus =
        smoothstep(
            0.10,
            0.90,
            depth01,
        );

    let glyph_core =
        mix(
            softened_core,
            glyph_sample,
            focus,
        );

    let core_weight =
        mix(
            0.72,
            1.05,
            depth01,
        );

    let glow_weight =
        mix(
            1.18,
            0.90,
            depth01,
        );

    // Theme-provided glow color remains slightly cooler and more
    // diffuse in the distant planes.
    let atmospheric_glow =
        mix(
            input.glow_color
                * vec3<f32>(
                    0.72,
                    0.80,
                    1.20,
                ),
            input.glow_color,
            depth01,
        );

    let color =
        input.color_glow.rgb
            * glyph_core
            * core_weight
        + atmospheric_glow
            * input.color_glow.a
            * glyph_glow
            * glow_weight;

    if (
        max(
            color.r,
            max(
                color.g,
                color.b,
            ),
        ) < 0.00001
    ) {
        discard;
    }

    return vec4<f32>(
        color,
        1.0,
    );
}

