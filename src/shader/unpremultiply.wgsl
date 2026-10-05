@group(0)
@binding(0)
var t_diffuse: texture_2d<f32>;

@group(0)
@binding(1)
var s_diffuse: sampler;

@fragment
fn fs_main(@builtin(position) pos: vec2<f32>) -> @location(0) vec4<f32> {
    let uv = pos / vec2<f32>(textureDimensions(t_diffuse, 0));
    let color = textureSample(t_diffuse, s_diffuse, uv);
    if (color.a <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(color.rgb / color.a, color.a);
}
