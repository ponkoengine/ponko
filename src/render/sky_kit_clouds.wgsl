// The sky kit's volumetric clouds (see `render/clouds.rs`): three passes
// over full-screen triangles (`vs_far`), composed after the mesh prelude,
// the sky helpers (sky.wgsl), the cloud model (sky_kit_common.wgsl) and the
// kit's sky (sky_kit.wgsl, for its lights).
//
// 1. `fs_cloud_trace`: marches each pixel's ray through the cloud layer at
//    half the frame's width and height, from a different offset each frame.
//    Out: the light the clouds add (rgb) and how much of the sky behind
//    them shows (a).
// 2. `fs_cloud_resolve`: blends that with the last frame's result, looked
//    up where the same direction was then (temporal reprojection), clamped
//    to this frame's neighbourhood so the clouds don't smear.
// 3. `fs_cloud_composite`: over the sky's pixels of the lit frame, behind
//    everything, through the height fog.

struct CloudPass {
    // Last frame's view-projection without jitter: where a direction was.
    previous: mat4x4<f32>,
    // x: steps per ray, y: steps toward the light, z: frame count, w: 1
    // when the history holds last frame's clouds.
    params: vec4<f32>,
    // The cloud image's width and height in pixels, then their inverses.
    size: vec4<f32>,
};

@group(3) @binding(0) var<uniform> cloud_pass: CloudPass;
@group(3) @binding(1) var cloud_sampler: sampler;
// The trace: this frame's march (trace and resolve: the resolve's input;
// composite: the resolved clouds).
@group(3) @binding(2) var cloud_image: texture_2d<f32>;
// Resolve: last frame's resolved clouds.
@group(3) @binding(3) var cloud_history: texture_2d<f32>;

// The farthest the march looks, in meters.
const CLOUD_FAR: f32 = 50000.0;

fn cloud_march(origin: vec3<f32>, dir: vec3<f32>, pixel: vec2<f32>) -> vec4<f32> {
    let layer = kit_layer(frame.sky_kit);
    if layer.coverage <= 0.0 || layer.density <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let span = kit_slab(origin, dir, layer, CLOUD_FAR);
    if span.x >= span.y {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let steps = max(i32(cloud_pass.params.x), 1);
    let light_steps = max(i32(cloud_pass.params.y), 1);
    // A different start in every pixel and frame (interleaved gradient
    // noise, moved on by the golden ratio): the resolve averages it away.
    let noise = fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))) + cloud_pass.params.z * 0.618034);
    let dt = (span.y - span.x) / f32(steps);
    let key = kit_key_light(layer.base + layer.thickness * 0.5);
    let to_light = kit_key_dir(key);
    let phase = kit_cloud_phase(dot(dir, to_light), layer.silver);
    let ambient = kit_cloud_ambient();
    let sun_up = key.rgb * max(to_light.y, 0.0);
    // Steps toward the light grow, reaching through the layer.
    let light_step = layer.thickness * 0.6 / f32(light_steps);
    var light = vec3<f32>(0.0);
    var through = 1.0;
    for (var i = 0; i < 256; i++) {
        if i >= steps || through < 0.01 {
            break;
        }
        let p = origin + dir * (span.x + dt * (f32(i) + noise));
        let d = kit_density(p, layer);
        if d <= 0.001 {
            continue;
        }
        let h = (p.y - layer.base) / layer.thickness;
        var depth = 0.0;
        for (var j = 0; j < 8; j++) {
            if j >= light_steps {
                break;
            }
            let reach = light_step * (f32(j) + 0.5);
            let q = p + to_light * reach;
            depth += kit_shape(q.xz, (q.y - layer.base) / layer.thickness, layer) * light_step;
        }
        let sunlit = kit_light_through(depth * layer.density * KIT_EXTINCTION * layer.self_shadow);
        // Edges facing away from the light are a little darker (the
        // "powder" effect): light has only just entered there.
        let powder = 1.0 - 0.3 * exp(-d * 6.0);
        let lit = key.rgb * sunlit * phase * powder * KIT_CLOUD_GAIN + kit_ambient_at(ambient, sun_up, h);
        let gone = exp(-d * KIT_EXTINCTION * dt);
        light += through * lit * (1.0 - gone);
        through *= gone;
    }
    // Far clouds fade into the haze, and (seen from below) into the horizon.
    var haze = exp(-span.x / 80000.0);
    if origin.y < layer.base {
        haze *= smoothstep(-0.02, 0.05, dir.y);
    }
    return vec4<f32>(light * haze, mix(1.0, through, haze));
}

@fragment
fn fs_cloud_trace(in: FullScreen) -> @location(0) vec4<f32> {
    return cloud_march(frame.camera.xyz, far_dir(in.uv), floor(in.clip.xy));
}

@fragment
fn fs_cloud_resolve(in: FullScreen) -> @location(0) vec4<f32> {
    let size = vec2<i32>(cloud_pass.size.xy);
    let pixel = vec2<i32>(in.clip.xy);
    let current = textureLoad(cloud_image, pixel, 0);
    if cloud_pass.params.w < 0.5 {
        return current;
    }
    // Where this direction was last frame: the clouds are far enough to
    // count as infinitely far, so only the camera's turn moves them.
    let dir = far_dir(in.uv);
    let clip = cloud_pass.previous * vec4<f32>(dir, 0.0);
    if clip.w <= 1e-6 {
        return current;
    }
    let ndc = clip.xy / clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return current;
    }
    var lo = current;
    var hi = current;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let s = textureLoad(cloud_image, clamp(pixel + vec2<i32>(x, y), vec2<i32>(0), size - 1), 0);
            lo = min(lo, s);
            hi = max(hi, s);
        }
    }
    let history = clamp(textureSampleLevel(cloud_history, cloud_sampler, uv, 0.0), lo, hi);
    return mix(history, current, 0.1);
}

@fragment
fn fs_cloud_composite(in: FullScreen) -> @location(0) vec4<f32> {
    let clouds = textureSampleLevel(cloud_image, cloud_sampler, in.uv, 0.0);
    // The sky behind went through the height fog (`background`); the clouds
    // go through it too: what the fog lets through of them, and the fog's
    // own light where they hide the sky.
    let far = frame.camera.xyz + far_dir(in.uv) * 20000.0;
    let fog_light = fog_along(vec3<f32>(0.0), far, false);
    let seen = fog_along(vec3<f32>(1.0), far, false) - fog_light;
    return vec4<f32>(clouds.rgb * seen + (1.0 - clouds.a) * fog_light, clouds.a);
}
