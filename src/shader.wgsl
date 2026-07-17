struct Uniforms {
    time: f32,
    aspect: f32,
    padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var glyph_atlas: texture_2d<f32>;

@group(0) @binding(2)
var glyph_sampler: sampler;

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
            0.0003,
            0.003,
            0.001,
        );

    var core_energy = 0.0;
    var near_glow_energy = 0.0;
    var wide_glow_energy = 0.0;
    var head_energy = 0.0;

    let layer_count = 3u;
    let columns_per_layer = 26u;
    let trail_segments = 18u;

    for (
        var layer_index = 0u;
        layer_index < layer_count;
        layer_index = layer_index + 1u
    ) {
        let layer =
            f32(layer_index);

        let depth =
            layer
            / f32(
                layer_count - 1u,
            );

        let perspective_scale =
            mix(
                0.58,
                1.32,
                depth,
            );

        let layer_brightness =
            mix(
                0.22,
                1.0,
                depth,
            );

        let layer_speed =
            mix(
                0.42,
                1.30,
                depth,
            );

        for (
            var column_index = 0u;
            column_index
                < columns_per_layer;
            column_index =
                column_index + 1u
        ) {
            let column =
                f32(column_index);

            let seed =
                column
                + layer * 127.0
                + 1.0;

            let regular_x =
                (
                    column + 0.5
                )
                / f32(
                    columns_per_layer,
                );

            let horizontal_drift =
                sin(
                    uniforms.time
                        * mix(
                            0.025,
                            0.075,
                            depth,
                        )
                    + seed,
                )
                * mix(
                    0.003,
                    0.014,
                    depth,
                );

            let perspective_x =
                0.5
                + (
                    regular_x - 0.5
                )
                    * perspective_scale
                + horizontal_drift;

            let speed =
                (
                    0.07
                    + random(
                        seed * 3.17,
                    ) * 0.21
                )
                * layer_speed;

            let phase =
                random(
                    seed * 8.41,
                );

            let stream_length =
                (
                    0.15
                    + random(
                        seed * 4.73,
                    ) * 0.29
                )
                * mix(
                    0.75,
                    1.15,
                    depth,
                );

            let head_y =
                1.25
                - fract(
                    uniforms.time
                        * speed
                    + phase,
                ) * 1.65;

            let cell_height =
                stream_length
                / f32(
                    trail_segments,
                );

            for (
                var segment_index = 0u;
                segment_index
                    < trail_segments;
                segment_index =
                    segment_index + 1u
            ) {
                let segment =
                    f32(segment_index);

                let segment_y =
                    head_y
                    + segment
                        * cell_height;

                var offset =
                    input.uv
                    - vec2<f32>(
                        perspective_x,
                        segment_y,
                    );

                offset.x *=
                    uniforms.aspect;

                let half_width =
                    (
                        0.0065
                        + random(
                            seed * 13.1,
                        ) * 0.0022
                    )
                    * mix(
                        0.65,
                        1.22,
                        depth,
                    );

                let half_height =
                    cell_height
                    * mix(
                        0.34,
                        0.44,
                        depth,
                    );

                let cell_distance =
                    rectangle_distance(
                        offset,
                        vec2<f32>(
                            half_width,
                            half_height,
                        ),
                    );

                let local_point =
                    vec2<f32>(
                        offset.x
                            / half_width,

                        offset.y
                            / half_height,
                    );

                let mutation_rate =
                    mix(
                        3.0,
                        8.0,
                        depth,
                    );

                let animation_step =
                    floor(
                        uniforms.time
                            * mutation_rate,
                    );

                let glyph_random =
                    random(
                        seed * 31.7
                        + segment * 17.3
                        + animation_step
                            * 0.71,
                    );

                let glyph_index =
                    min(
                        u32(
                            floor(
                                glyph_random * 64.0,
                            ),
                        ),
                        63u,
                    );

                let glyph =
                    sample_glyph(
                        local_point,
                        glyph_index,
                    );

                let near_glyph_glow =
                    sample_glyph_glow(
                        local_point,
                        glyph_index,
                        0.13,
                    );

                let wide_glyph_glow =
                    sample_glyph_glow(
                        local_point,
                        glyph_index,
                        0.28,
                    );

                let edge_softness =
                    mix(
                        0.0006,
                        0.0012,
                        depth,
                    );

                let cell_mask =
                    1.0
                    - smoothstep(
                        -edge_softness,
                        edge_softness,
                        cell_distance,
                    );

                let core =
                    smoothstep(
                        0.18,
                        0.72,
                        glyph,
                    ) * cell_mask;

                let trail_position =
                    segment
                    / f32(
                        trail_segments - 1u,
                    );

                let fade =
                    pow(
                        1.0
                            - trail_position,
                        2.15,
                    );

                let flicker =
                    0.66
                    + random(
                        seed * 17.0
                        + segment * 23.0
                        + floor(
                            uniforms.time
                                * mix(
                                    4.0,
                                    9.0,
                                    depth,
                                ),
                        ),
                    ) * 0.34;

                let segment_energy =
                    fade
                    * flicker
                    * layer_brightness;

                core_energy =
                    max(
                        core_energy,
                        core
                            * segment_energy,
                    );

                near_glow_energy +=
                    near_glyph_glow
                    * segment_energy
                    * mix(
                        0.025,
                        0.10,
                        depth,
                    );

                wide_glow_energy +=
                    wide_glyph_glow
                    * segment_energy
                    * mix(
                        0.008,
                        0.04,
                        depth,
                    );

                let is_head =
                    1.0
                    - step(
                        0.5,
                        segment,
                    );

                head_energy =
                    max(
                        head_energy,

                        core
                        * is_head
                        * layer_brightness
                        * mix(
                            0.25,
                            1.0,
                            depth,
                        ),
                    );
            }
        }
    }

    near_glow_energy =
        min(
            near_glow_energy,
            1.6,
        );

    wide_glow_energy =
        min(
            wide_glow_energy,
            1.5,
        );

    let atmospheric_green =
        vec3<f32>(
            0.0,
            0.22,
            0.024,
        );

    let near_glow_green =
        vec3<f32>(
            0.0,
            0.44,
            0.045,
        );

    let matrix_green =
        vec3<f32>(
            0.012,
            1.0,
            0.11,
        );

    let head_white =
        vec3<f32>(
            0.70,
            1.0,
            0.77,
        );

    let color =
        background
        + atmospheric_green
            * wide_glow_energy
        + near_glow_green
            * near_glow_energy
        + matrix_green
            * core_energy
        + head_white
            * head_energy;

    return vec4<f32>(
        color,
        1.0,
    );
}
