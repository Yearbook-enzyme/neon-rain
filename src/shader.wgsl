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

// Very subtle optical distortion and slow lens breathing.
// This should be felt more than consciously noticed.
fn camera_sample_uv(
    uv: vec2<f32>,
    time: f32,
    aspect: f32,
) -> vec2<f32> {
    var point =
        uv * 2.0
        - vec2<f32>(1.0);

    point.x *= aspect;

    let radius_squared =
        dot(point, point);

    let barrel =
        1.0
        + radius_squared * 0.0045;

    let breathing =
        1.0
        + sin(time * 0.11) * 0.0016;

    point *=
        barrel * breathing;

    point.x /= aspect;

    return
        point * 0.5
        + vec2<f32>(0.5);
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

    // Pixel-stable raster baseline.
    //
    // Optical distortion and breathing are temporarily disabled
    // because continuous subpixel warping makes thin glyph strokes
    // scintillate even while the simulation itself is paused.
    let camera_uv =
        input.uv;

    let pixel =
        camera_uv
        * uniforms.resolution;

    // Camera movement is disabled for the pixel-stable baseline.
    // It can return later as a snapped or post-process-only effect.
    let camera_sway =
        vec2<f32>(0.0);

    var core_energy = 0.0;
    var glow_energy = 0.0;
    var head_energy = 0.0;
    var cascade_energy = 0.0;

    // Resolution-stable visual scale. A larger viewport gains
    // proportionally larger glyphs instead of progressively wider gaps.
    let visual_scale = clamp(
        sqrt(
            max(
                uniforms.resolution.x
                    * uniforms.resolution.y,
                1.0,
            )
            / (1600.0 * 900.0)
        ),
        0.72,
        2.40,
    );

    // Glyph dimensions are selected per stream from depth.

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

        // Depth-aware composition pass.
        //
        // The simulation maintains five numerical layers, while
        // these curves organize them visually into distant,
        // middle, and foreground planes.
        let depth01 =
            clamp(
                depth,
                0.0,
                1.0,
            );

        let depth_shape =
            pow(
                depth01,
                1.45,
            );

        // Keep distant layers populated while allowing foreground
        // streams to become large without covering the frame.
        let density_sample =
            random(
                f32(stream_index) * 91.731
                + stream.parameters.w * 17.317
                + depth01 * 43.911
            );

        let density_keep =
            mix(
                0.96,
                0.28,
                depth_shape,
            );

        if (density_sample > density_keep) {
            continue;
        }

        // Distant glyphs are small and fine. Foreground glyphs
        // become noticeably larger than the primary rain plane.
        let glyph_width =
            max(
                1.0,
                floor(
                    mix(
                        9.5,
                        20.5,
                        depth_shape,
                    ) * visual_scale
                    + 0.5
                ),
            );

        let glyph_height =
            max(
                1.0,
                floor(
                    mix(
                        15.0,
                        30.0,
                        depth_shape,
                    ) * visual_scale
                    + 0.5
                ),
            );

        // Core light remains visible in every plane, but bloom,
        // white heads, and energetic events belong increasingly
        // to the foreground.
        let atmosphere =
            mix(
                0.42,
                1.08,
                pow(depth01, 0.78),
            );

        let glow_atmosphere =
            mix(
                0.06,
                1.18,
                pow(depth01, 1.55),
            );

        let head_probability =
            mix(
                0.015,
                0.42,
                pow(depth01, 1.40),
            );

        let head_sample =
            random(
                f32(stream_index) * 53.117
                + stream.parameters.w * 71.913
            );

        let white_head_presence =
            select(
                0.0,
                1.0,
                head_sample < head_probability,
            );

        let head_depth =
            mix(
                0.06,
                0.78,
                pow(depth01, 1.60),
            );

        let cascade_depth =
            mix(
                0.18,
                1.08,
                pow(depth01, 1.15),
            );

        // Foreground streams respond more strongly to camera motion,
        // creating gentle parallax between the five depth layers.
        let camera_depth_scale =
            mix(
                0.28,
                1.0,
                pow(
                    clamp(depth, 0.0, 1.0),
                    0.82,
                ),
            );

        let unsnapped_stream_x =
            stream.position.x
            + camera_sway.x
                * camera_depth_scale;

        let unsnapped_stream_head =
            stream.position.y
            + camera_sway.y
                * camera_depth_scale;

        let stream_x =
            floor(unsnapped_stream_x)
            + 0.5;

        let stream_head =
            floor(unsnapped_stream_head)
            + 0.5;

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

        // Per-plane atmospheric weights were selected above,
        // before sampling the glyph atlas.

        // Stable baseline: temporal brightness animation is
        // disabled while the depth hierarchy is being tuned.
        //
        // Motion, mutations, weather, and cascades remain active,
        // but ordinary glyph luminance no longer flashes at 6 Hz.
        let electrical_pulse = 1.0;
        let glyph_variation = 1.0;

        // Energy originates near the head and diffuses into the trail.
        let head_injection =
            exp(
                -f32(segment) * 0.38
            );

        let propagation_profile =
            0.80
            + head_injection * 0.42;

        let base_stream_energy =
            brightness
            * trail_fade
            * electrical_pulse
            * glyph_variation
            * propagation_profile;

        let stream_energy =
            base_stream_energy
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
                base_stream_energy
                    * glow_atmosphere
                    * 0.34
                + brightness
                    * glow_atmosphere
                    * cascade_packet
                    * 0.42
            );

        cascade_energy +=
            glyph_core
            * brightness
            * cascade_depth
            * glyph_variation
            * cascade_packet
            * (
                0.45
                + trail_fade * 0.55
            );

        if (segment == 0u) {
            let head_flash = 1.30;

            head_energy +=
                glyph_core
                * brightness
                * atmosphere
                * glyph_variation
                * head_flash
                * white_head_presence
                * head_depth;
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

    // Slight optical falloff at the edge of the lens.
    var lens_point =
        input.uv * 2.0
        - vec2<f32>(1.0);

    lens_point.x *=
        uniforms.aspect;

    let lens_radius_squared =
        dot(
            lens_point,
            lens_point,
        );

    let vignette =
        1.0
        - smoothstep(
            0.55,
            1.65,
            lens_radius_squared,
        ) * 0.10;

    color *= vignette;

    return vec4<f32>(
        color,
        1.0,
    );
}
