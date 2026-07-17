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
    output.uv = position * 0.5 + vec2<f32>(0.5);
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

@fragment
fn fs_history(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(previous_history_texture));
    let texel = vec2<f32>(1.0 / dimensions.x, 1.0 / dimensions.y);

    let current = textureSample(
        current_wide_texture,
        history_sampler,
        input.uv
    ).rgb;

    // Pull old light slightly upward in texture space so its screen-space trail
    // lingers behind the downward-moving glyphs.
    let history_uv = clamp(
        input.uv,
        vec2<f32>(0.0),
        vec2<f32>(1.0)
    );

    let previous = textureSample(
        previous_history_texture,
        history_sampler,
        history_uv
    ).rgb;

    let retained = previous * 0.925;
    let deposited = current * 0.34;

    return vec4<f32>(max(current * 0.55, retained + deposited), 1.0);
}

@group(0) @binding(0)
var original_texture: texture_2d<f32>;

@group(0) @binding(1)
var near_bloom_texture: texture_2d<f32>;

@group(0) @binding(2)
var wide_bloom_texture: texture_2d<f32>;

@group(0) @binding(3)
var composite_sampler: sampler;

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
    let original = textureSample(
        original_texture,
        composite_sampler,
        input.uv
    ).rgb;

    // The bloom render targets use the opposite texture Y orientation from the
    // original HDR target, so both bloom layers are corrected here.
    let bloom_uv = vec2<f32>(input.uv.x, 1.0 - input.uv.y);

    let near_bloom = textureSample(
        near_bloom_texture,
        composite_sampler,
        bloom_uv
    ).rgb;

    let wide_bloom = textureSample(
        wide_bloom_texture,
        composite_sampler,
        bloom_uv
    ).rgb;

    // Near bloom hugs glyphs; temporal wide bloom leaves a descending light trail.
    let near_strength = 0.95;
    let wide_strength = 0.42;

    let hdr_color = original
        + near_bloom * near_strength
        + wide_bloom * wide_strength;

    return vec4<f32>(aces_tonemap(hdr_color), 1.0);
}

@fragment
fn fs_blur_vertical_wide(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(input_texture));
    let texel = vec2<f32>(0.0, 1.0 / dimensions.y);

    // Directional kernel: most energy trails in one direction rather than
    // expanding equally above and below the source.
    var color = textureSample(
        input_texture,
        input_sampler,
        input.uv
    ).rgb * 0.32;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv - texel * 1.5
    ).rgb * 0.28;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv - texel * 3.5
    ).rgb * 0.22;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv - texel * 6.5
    ).rgb * 0.12;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv + texel * 1.0
    ).rgb * 0.06;

    return vec4<f32>(color, 1.0);
}

