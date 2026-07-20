struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );

    let position = positions[vertex_index];

    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv =
        vec2<f32>(
            position.x * 0.5 + 0.5,
            0.5 - position.y * 0.5,
        );
    return output;
}

@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var input_sampler: sampler;

@fragment
fn fs_bright(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_texture, input_sampler, input.uv);
    let brightness = max(color.r, max(color.g, color.b));

    // Soft-knee extraction keeps the glow selective without hard edges.
    let contribution = smoothstep(0.55, 1.20, brightness);
    return vec4<f32>(color.rgb * contribution, 1.0);
}

@fragment
fn fs_downsample(input: VertexOutput) -> @location(0) vec4<f32> {
    // Rendering into a smaller linear-filtered target performs the scale change.
    // A small cross sample reduces shimmer and gives the wide bloom a smoother base.
    let dimensions = vec2<f32>(textureDimensions(input_texture));
    let texel = 1.0 / dimensions;

    var color = textureSample(input_texture, input_sampler, input.uv).rgb * 0.40;
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>( texel.x, 0.0)).rgb * 0.15;
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(-texel.x, 0.0)).rgb * 0.15;
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(0.0,  texel.y)).rgb * 0.15;
    color += textureSample(input_texture, input_sampler, input.uv + vec2<f32>(0.0, -texel.y)).rgb * 0.15;

    return vec4<f32>(color, 1.0);
}

@fragment
fn fs_blur_horizontal(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(input_texture));
    let texel = vec2<f32>(1.0 / dimensions.x, 0.0);

    var color = textureSample(input_texture, input_sampler, input.uv).rgb * 0.227027;
    color += textureSample(input_texture, input_sampler, input.uv + texel * 0.70).rgb * 0.316216;
    color += textureSample(input_texture, input_sampler, input.uv - texel * 0.70).rgb * 0.316216;
    color += textureSample(input_texture, input_sampler, input.uv + texel * 1.60).rgb * 0.070270;
    color += textureSample(input_texture, input_sampler, input.uv - texel * 1.60).rgb * 0.070270;

    return vec4<f32>(color, 1.0);
}

@fragment
fn fs_blur_vertical(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(input_texture));
    let texel = vec2<f32>(0.0, 1.0 / dimensions.y);

    var color = textureSample(input_texture, input_sampler, input.uv).rgb * 0.227027;
    color += textureSample(input_texture, input_sampler, input.uv + texel * 2.20).rgb * 0.316216;
    color += textureSample(input_texture, input_sampler, input.uv - texel * 2.20).rgb * 0.316216;
    color += textureSample(input_texture, input_sampler, input.uv + texel * 5.20).rgb * 0.070270;
    color += textureSample(input_texture, input_sampler, input.uv - texel * 5.20).rgb * 0.070270;

    return vec4<f32>(color, 1.0);
}

// Temporal light-history resources.
@group(0) @binding(0)
var current_wide_texture: texture_2d<f32>;

@group(0) @binding(1)
var previous_history_texture: texture_2d<f32>;

@group(0) @binding(2)
var history_sampler: sampler;

struct BloomSettings {
    near_strength: f32,
    wide_strength: f32,
    history_retention: f32,
    history_deposit: f32,
    background_color: vec4<f32>,
};

@group(0) @binding(3)
var<uniform> history_settings: BloomSettings;

@fragment
fn fs_history(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions =
        vec2<f32>(
            textureDimensions(
                previous_history_texture,
            ),
        );

    let texel =
        1.0 / max(
            dimensions,
            vec2<f32>(1.0),
        );

    let current =
        textureSample(
            current_wide_texture,
            history_sampler,
            input.uv,
        ).rgb;

    // Directional bloom in unified top-left coordinates.
    //
    // Pull retained light from slightly above the destination
    // pixel so temporal glow progresses downward with the rain.
    let history_uv =
        clamp(
            input.uv
                - vec2<f32>(
                    0.0,
                    texel.y * 0.65,
                ),
            vec2<f32>(0.0),
            vec2<f32>(1.0),
        );

    let previous =
        textureSample(
            previous_history_texture,
            history_sampler,
            history_uv,
        ).rgb;

    let previous_brightness =
        max(
            previous.r,
            max(
                previous.g,
                previous.b,
            ),
        );

    // Strong coherent trails persist longer than faint background
    // residue, preventing a permanent pale atmospheric pattern.
    let retention =
        mix(
            history_settings.history_retention * 0.90,
            history_settings.history_retention,
            smoothstep(
                0.08,
                0.85,
                previous_brightness,
            ),
        );

    let retained =
        previous * retention;

    let deposited =
        current * history_settings.history_deposit;

    let history =
        max(
            current * 0.52,
            retained + deposited,
        );

    return vec4<f32>(
        min(
            history,
            vec3<f32>(2.4),
        ),
        1.0,
    );
}


@group(0) @binding(0)
var original_texture: texture_2d<f32>;

@group(0) @binding(1)
var near_bloom_texture: texture_2d<f32>;

@group(0) @binding(2)
var wide_bloom_texture: texture_2d<f32>;

@group(0) @binding(3)
var composite_sampler: sampler;

@group(0) @binding(4)
var<uniform> composite_settings: BloomSettings;

fn aces_tonemap(color: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;

    return clamp(
        (color * (a * color + b)) /
        (color * (c * color + d) + e),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
}

@fragment
fn fs_composite(input: VertexOutput) -> @location(0) vec4<f32> {
    let original =
        textureSample(
            original_texture,
            composite_sampler,
            input.uv,
        ).rgb;

    let near_bloom =
        textureSample(
            near_bloom_texture,
            composite_sampler,
            input.uv,
        ).rgb;

    let wide_bloom =
        textureSample(
            wide_bloom_texture,
            composite_sampler,
            input.uv,
        ).rgb;

    let hdr_light =
        original
        + near_bloom
            * composite_settings.near_strength
        + wide_bloom
            * composite_settings.wide_strength;

    // Final composition grading.
    let background_floor =
        composite_settings.background_color.rgb;

    let screen_point =
        input.uv * 2.0
        - vec2<f32>(1.0);

    let vignette_point =
        screen_point
            * vec2<f32>(
                0.82,
                1.0,
            );

    let elliptical_radius =
        dot(
            vignette_point,
            vignette_point,
        );

    let vignette =
        1.0
        - smoothstep(
            0.48,
            1.42,
            elliptical_radius,
        ) * composite_settings.background_color.a;

    let vertical_grade =
        mix(
            1.025,
            0.975,
            smoothstep(
                0.0,
                1.0,
                input.uv.y,
            ),
        );

    let graded_hdr =
        (
            background_floor
            + hdr_light
        )
        * vignette
        * vertical_grade;

    return vec4<f32>(
        aces_tonemap(
            graded_hdr,
        ),
        1.0,
    );
}


@fragment
fn fs_blur_vertical_wide(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions =
        vec2<f32>(
            textureDimensions(
                input_texture,
            ),
        );

    let texel =
        vec2<f32>(
            0.0,
            1.0 / max(
                dimensions.y,
                1.0,
            ),
        );

    var color =
        textureSample(
            input_texture,
            input_sampler,
            input.uv,
        ).rgb * 0.34;

    // Pull more light from below, extending the glow upward behind
    // glyphs that are traveling downward.
    color +=
        textureSample(
            input_texture,
            input_sampler,
            input.uv + texel * 1.5,
        ).rgb * 0.22;

    color +=
        textureSample(
            input_texture,
            input_sampler,
            input.uv + texel * 3.5,
        ).rgb * 0.15;

    color +=
        textureSample(
            input_texture,
            input_sampler,
            input.uv + texel * 6.5,
        ).rgb * 0.07;

    // Smaller opposite taps keep the result soft.
    color +=
        textureSample(
            input_texture,
            input_sampler,
            input.uv - texel * 1.5,
        ).rgb * 0.14;

    color +=
        textureSample(
            input_texture,
            input_sampler,
            input.uv - texel * 3.5,
        ).rgb * 0.06;

    color +=
        textureSample(
            input_texture,
            input_sampler,
            input.uv - texel * 6.5,
        ).rgb * 0.02;

    return vec4<f32>(
        color,
        1.0,
    );
}

