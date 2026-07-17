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


// Bright-pass and blur input.
@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var input_sampler: sampler;


// Preserve only the bright portions of the Matrix image.
//
// The soft threshold prevents a harsh edge between blooming and
// non-blooming pixels.
@fragment
fn fs_bright(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(input_texture, input_sampler, input.uv);
    let brightness = max(color.r, max(color.g, color.b));

    let contribution = smoothstep(0.55, 1.20, brightness);

    return vec4<f32>(color.rgb * contribution, 1.0);
}


// Nine-tap horizontal Gaussian blur.
@fragment
fn fs_blur_horizontal(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(input_texture));
    let texel = vec2<f32>(1.0 / dimensions.x, 0.0);

    var color = textureSample(
        input_texture,
        input_sampler,
        input.uv
    ).rgb * 0.227027;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv + texel * 0.70
    ).rgb * 0.316216;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv - texel * 0.70
    ).rgb * 0.316216;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv + texel * 1.60
    ).rgb * 0.070270;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv - texel * 1.60
    ).rgb * 0.070270;

    return vec4<f32>(color, 1.0);
}


// Nine-tap vertical Gaussian blur.
@fragment
fn fs_blur_vertical(input: VertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(input_texture));
    let texel = vec2<f32>(0.0, 1.0 / dimensions.y);

    var color = textureSample(
        input_texture,
        input_sampler,
        input.uv
    ).rgb * 0.227027;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv + texel * 2.20
    ).rgb * 0.316216;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv - texel * 2.20
    ).rgb * 0.316216;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv + texel * 5.20
    ).rgb * 0.070270;

    color += textureSample(
        input_texture,
        input_sampler,
        input.uv - texel * 5.20
    ).rgb * 0.070270;

    return vec4<f32>(color, 1.0);
}


// Final composite resources.
@group(0) @binding(0)
var original_texture: texture_2d<f32>;

@group(0) @binding(1)
var bloom_texture: texture_2d<f32>;

@group(0) @binding(2)
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

    let bloom = textureSample(
        bloom_texture,
        composite_sampler,
        vec2<f32>(input.uv.x, 1.0 - input.uv.y)
    ).rgb;

    // Increase or reduce this number later to tune real bloom intensity.
    let bloom_strength = 1.15;

    let hdr_color = original + bloom * bloom_strength;
    let mapped_color = aces_tonemap(hdr_color);

    return vec4<f32>(mapped_color, 1.0);
}
