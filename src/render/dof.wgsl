// Depth of field, appended to post.wgsl (its post settings, its frame
// `source` and its full-screen triangle). One gather pass at the scene's
// resolution: each pixel averages a disc of samples around it (a golden
// angle spiral), taking each sample only as far as that sample's own blur
// reaches, so sharp things don't smear and blurred ones spread over what
// is behind them. After Dennis Gustafsson's "Bokeh depth of field in a
// single pass", with a fixed number of samples.

@group(2) @binding(1) var scene_depth_texture: texture_depth_multisampled_2d;

const DOF_TAPS: i32 = 32;
const GOLDEN_ANGLE: f32 = 2.39996323;

// How far in front of the camera (along its view) the pixel at `at` is.
fn dof_distance(at: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(scene_depth_texture));
    let d = textureLoad(scene_depth_texture, clamp(at, vec2<i32>(0), size - 1), 0);
    let near = post.camera.x;
    let far = post.camera.y;
    return near * far / max(far - d * (far - near), 1e-6);
}

// The blur radius, in pixels, of something `distance` away: none within
// the sharp range around the focus, then growing as through a lens to the
// largest blur at twice the focus distance (and at two thirds of it).
fn dof_radius(distance: f32, largest: f32) -> f32 {
    let off = max(abs(distance - post.dof.x) - post.dof.y, 0.0);
    return largest * min(2.0 * off / max(distance, 1e-3), 1.0);
}

@fragment
fn fs_depth_of_field(in: Full) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    let largest = post.dof.z * size.y;
    let at = vec2<i32>(in.at.xy);
    let center = textureLoad(source, at, 0);
    let center_distance = dof_distance(at);
    let center_radius = dof_radius(center_distance, largest);
    // About the distance between neighbouring samples: the blend between
    // "this sample reaches here" and "it doesn't" is that wide.
    let spacing = max(largest / sqrt(f32(DOF_TAPS)), 1.0);
    var sum = center.rgb;
    var total = 1.0;
    for (var i = 0; i < DOF_TAPS; i++) {
        let r = largest * sqrt((f32(i) + 0.5) / f32(DOF_TAPS));
        let angle = f32(i) * GOLDEN_ANGLE;
        // Color and depth from the same texel: a filtered color would mix
        // in a neighbour at another depth (a halo around sharp edges).
        let p = clamp(vec2<i32>(floor(in.at.xy + vec2<f32>(cos(angle), sin(angle)) * r)), vec2<i32>(0), vec2<i32>(size) - 1);
        let color = textureLoad(source, p, 0).rgb;
        let distance = dof_distance(p);
        var radius = dof_radius(distance, largest);
        // Something behind this pixel blurs over it no further than this
        // pixel's own blur: a sharp thing in front keeps its edge.
        if distance > center_distance {
            radius = min(radius, center_radius * 2.0);
        }
        let reach = smoothstep(r - spacing * 0.5, r + spacing * 0.5, radius);
        sum += mix(sum / total, color, reach);
        total += 1.0;
    }
    return vec4<f32>(sum / total, center.a);
}
