
// The water surface (the water kit's material shader). Its switches and
// the wave functions come before this part; the material's values:
// custom[0]: absorption per metre (rgb), refraction strength
// custom[1]: the water's own colour (linear rgb), foam amount
// custom[2]: current (x, z, m/s), the flow map's top speed, shore foam depth
// custom[3]: the ripple field's -x, -z corner, side, ripple strength
// custom_texture0: the planar reflection; custom_texture1: the ripple field;
// custom_array0: the flow map.

// The current at ground point `xz`, in m/s.
fn water_flow(xz: vec2<f32>) -> vec2<f32> {
    if WATER_FLOW_MAP {
        let uv = (xz - WATER_FLOW_AREA.xy) / WATER_FLOW_AREA.zw;
        let texel = textureSampleLevel(custom_array0, material_sampler, uv, 0, 0.0).xy;
        // Stored in bytes: 128 is still water, 1 and 255 the top speed.
        return (texel * 255.0 - 128.0) / 127.0 * material.custom[2].z;
    }
    return material.custom[2].xy;
}

// How wide one pixel is on the water at `p` seen along `view`, in metres
// (stretched where the view grazes the surface).
fn water_footprint(p: vec3<f32>, view: vec3<f32>) -> f32 {
    let clip = frame.view_proj * vec4<f32>(p, 1.0);
    let a = frame.inv_view_proj * clip;
    let b = frame.inv_view_proj * vec4<f32>(clip.x, clip.y + 2.0 * frame.screen.w * clip.w, clip.z, clip.w);
    return distance(a.xyz / a.w, b.xyz / b.w) / max(abs(view.y), 0.25);
}

// How much of a pattern of wavelength `length` to keep where a pixel
// covers `footprint` metres: all of it over 8 pixels a wave, none under 3
// (finer ripples would only flicker).
fn water_keep(length: f32, footprint: f32) -> f32 {
    return smoothstep(3.0, 8.0, length / max(footprint, 1e-4));
}

// The slope of small wind ripples at `p`: crossing waves, three per
// octave at unrelated lengths (equal ones beat into broad bands), warped
// by two scales of noise so they don't line up; octaves finer than the
// pixels fade out.
fn water_ripple_octaves(p: vec2<f32>, t: f32, footprint: f32) -> vec2<f32> {
    let q = p + 1.4 * vec2<f32>(noise2(p * 0.21), noise2(p.yx * 0.21 + 7.0))
        + 0.35 * vec2<f32>(noise2(p * 0.9 + 3.1), noise2(p.yx * 0.9 + 11.0));
    var slope = vec2<f32>(0.0);
    // The longest ripples 1.4 m, the finest (third octave) 25 cm.
    var freq = 4.5;
    var amp = 0.026;
    for (var i = 0; i < WATER_OCTAVES; i++) {
        let turn = f32(i) * 1.7;
        for (var k = 0; k < 3; k++) {
            let a = turn + f32(k) * 2.27;
            let d = vec2<f32>(cos(a), sin(a));
            let f = freq * (1.0 + 0.23 * f32(k));
            let keep = water_keep(6.2832 / f, footprint);
            let th = dot(d, q) * f + t * sqrt(9.81 * f) + f32(k) * 1.3;
            slope += d * cos(th) * amp * f * keep;
        }
        freq *= 2.3;
        amp *= 0.42;
    }
    return slope;
}

// The ripples carried by a current `flow`: two layers slide downstream,
// each restarting every two seconds while the other one shows.
fn water_detail(xz: vec2<f32>, flow: vec2<f32>, t: f32, footprint: f32) -> vec2<f32> {
    if dot(flow, flow) < 1e-6 {
        return water_ripple_octaves(xz, t, footprint);
    }
    let period = 2.0;
    let f0 = fract(t / period);
    let f1 = fract(t / period + 0.5);
    let w0 = 1.0 - abs(2.0 * f0 - 1.0);
    let a = water_ripple_octaves(xz - flow * f0 * period, t, footprint);
    let b = water_ripple_octaves(xz - flow * f1 * period + vec2<f32>(3.7, 1.9), t, footprint);
    return a * w0 + b * (1.0 - w0);
}

// The slope of the contact ripples at `xz`.
fn water_contact_slope(xz: vec2<f32>) -> vec2<f32> {
    if !WATER_RIPPLES {
        return vec2<f32>(0.0);
    }
    let region = material.custom[3];
    let uv = (xz - region.xy) / region.z;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return vec2<f32>(0.0);
    }
    let texel = 1.0 / vec2<f32>(textureDimensions(custom_texture1));
    let dx = textureSampleLevel(custom_texture1, material_sampler, uv + vec2<f32>(texel.x, 0.0), 0.0).r
        - textureSampleLevel(custom_texture1, material_sampler, uv - vec2<f32>(texel.x, 0.0), 0.0).r;
    let dz = textureSampleLevel(custom_texture1, material_sampler, uv + vec2<f32>(0.0, texel.y), 0.0).r
        - textureSampleLevel(custom_texture1, material_sampler, uv - vec2<f32>(0.0, texel.y), 0.0).r;
    let metres = region.z * texel.x;
    // Fade out towards the field's edges.
    let edge = clamp(min(min(uv.x, uv.y), min(1.0 - uv.x, 1.0 - uv.y)) * 20.0, 0.0, 1.0);
    return vec2<f32>(dx, dz) / (2.0 * metres) * region.w * edge;
}

// The sky in direction `dir`: the sky texture, or the flat sky's light.
fn water_sky(dir: vec3<f32>) -> vec3<f32> {
    let up = normalize(vec3<f32>(dir.x, max(dir.y, 0.02), dir.z));
    let sky = sky_radiance(up);
    if dot(sky, sky) > 0.0 {
        return sky;
    }
    return frame.sky.rgb;
}

// Whether a world point from the scene's depth is a real surface (the sky
// is at infinity there).
fn water_solid(p: vec3<f32>) -> bool {
    return all(abs(p) < vec3<f32>(1e7));
}

// Where world point `p` is on screen, in pixels (negative x: behind the
// camera or off screen).
fn water_on_screen(p: vec3<f32>) -> vec2<f32> {
    let clip = frame.view_proj * vec4<f32>(p, 1.0);
    if clip.w <= 0.0 {
        return vec2<f32>(-1.0);
    }
    let ndc = clip.xy / clip.w;
    if any(abs(ndc) > vec2<f32>(1.0)) {
        return vec2<f32>(-1.0);
    }
    return vec2<f32>((ndc.x * 0.5 + 0.5) * frame.screen.x, (0.5 - ndc.y * 0.5) * frame.screen.y);
}

// How far in front of the ray point at pixel `px` the scene is (positive:
// the ray has gone behind something), or a large negative number for the
// sky.
fn water_behind(p: vec3<f32>, px: vec2<f32>) -> f32 {
    let scene = scene_position(px);
    if !water_solid(scene) {
        return -1e9;
    }
    return distance(p, frame.camera.xyz) - distance(scene, frame.camera.xyz);
}

// What the reflected ray from `start` along `dir` hits on screen: its
// colour and how much to trust it (0: nothing found). Steps grow from a
// few centimetres to cover 30 m; a step that passes behind something is
// narrowed down by halving, and kept if the ray is close behind its
// surface (not passing far behind a thin object).
fn water_march(start: vec3<f32>, dir: vec3<f32>) -> vec4<f32> {
    let grow = 1.2;
    var step = 30.0 * (grow - 1.0) / (pow(grow, f32(WATER_SSR_STEPS)) - 1.0);
    var t = 0.0;
    for (var i = 0; i < WATER_SSR_STEPS; i++) {
        let next = t + step;
        let p = start + dir * next;
        let px = water_on_screen(p);
        if px.x < 0.0 {
            break;
        }
        let behind = water_behind(p, px);
        if behind > 0.0 {
            // Somewhere between t and next: halve four times.
            var lo = t;
            var hi = next;
            for (var k = 0; k < 4; k++) {
                let mid = (lo + hi) * 0.5;
                let q = start + dir * mid;
                let qx = water_on_screen(q);
                if qx.x >= 0.0 && water_behind(q, qx) > 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let hit = start + dir * hi;
            let hx = water_on_screen(hit);
            if hx.x >= 0.0 && water_behind(hit, hx) < (hi - lo) * 2.0 + 0.15 {
                let edge = min(min(hx.x, frame.screen.x - hx.x), min(hx.y, frame.screen.y - hx.y));
                let fade = clamp(edge / (0.08 * frame.screen.y), 0.0, 1.0);
                return vec4<f32>(scene_color(hx), fade);
            }
        }
        t = next;
        step *= grow;
    }
    return vec4<f32>(0.0);
}

// Light reaching the water from above, for its colour and the foam.
fn water_ambient() -> vec3<f32> {
    return frame.sky.rgb + frame.sun_color.rgb * max(frame.sun_dir.y, 0.0) * 0.3;
}

fn surface(s: Surface) -> Surface {
    var out = s;
    let t = frame.time.x;
    let footprint = water_footprint(s.world, s.view);
    let wave = water_wave_seen(s.uv1, t, footprint);
    let flow = water_flow(s.world.xz);
    let slope = water_detail(s.world.xz, flow, t, footprint) + water_contact_slope(s.world.xz);
    let n = normalize(wave.normal + vec3<f32>(-slope.x, 0.0, -slope.y) * wave.normal.y);
    let view = s.view;
    let own = distance(s.world, frame.camera.xyz);
    let absorb = material.custom[0].rgb;
    let scatter = material.custom[1].rgb * water_ambient();
    var color = vec3<f32>(0.0);

    // The engine flips the mesh's up normal on back faces. Use that side
    // near the surface: the filtered wave normal can point away from the
    // viewer even on a front face. Outside the waves' height range, use
    // the camera's side so overlapping triangles cannot draw the underside
    // over water seen from above (or vice versa).
    let camera_height = frame.camera.y - s.origin.y;
    let from_above = select(s.normal.y > 0.0, camera_height > 0.0, abs(camera_height) > WATER_WAVE_REACH);
    if from_above {
        // Seen from above. What lies under the surface, bent by its normal.
        let behind = scene_position(s.screen);
        let solid = water_solid(behind);
        let depth = select(1e4, max(s.world.y - behind.y, 0.0), solid);
        var bent = s.screen;
        if WATER_REFRACT {
            let push = n.xz * material.custom[0].w * frame.screen.y * 0.03 * clamp(depth, 0.0, 1.0);
            let tried = scene_position(s.screen + push);
            // Keep the bend only where it shows something under the water too.
            if !water_solid(tried) || tried.y < s.world.y {
                bent = s.screen + push;
            }
        }
        let bed = scene_position(bent);
        var path = 1e4;
        if water_solid(bed) {
            path = max(distance(bed, frame.camera.xyz) - own, 0.0) + max(s.world.y - bed.y, 0.0);
        }
        let through = exp(-absorb * path);
        let under = scene_color(bent) * through + scatter * (1.0 - through);

        // What it reflects.
        let r = reflect(-view, n);
        var mirror = water_sky(r);
        if WATER_PLANAR {
            let uv = s.screen * frame.screen.zw;
            let flipped = vec2<f32>(uv.x, 1.0 - uv.y) + n.xz * 0.03 * material.custom[0].w;
            mirror = textureSampleLevel(custom_texture0, material_sampler, clamp(flipped, vec2<f32>(0.001), vec2<f32>(0.999)), 0.0).rgb;
        }
        if WATER_SSR_STEPS > 0 {
            // The ray follows the waves but only part of the fine ripples:
            // marched along every ripple's slope, a thin object's
            // reflection breaks up into specks.
            let smooth_n = normalize(mix(wave.normal, n, 0.3));
            let hit = water_march(s.world, reflect(-view, smooth_n));
            mirror = mix(mirror, hit.rgb, hit.a);
        }
        let facing = clamp(dot(n, view), 0.0, 1.0);
        let fresnel = 0.02 + 0.98 * pow(1.0 - facing, 5.0);
        color = mix(under, mirror, fresnel);

        // The sun's glint.
        let h = normalize(view + frame.sun_dir.xyz);
        let glint = pow(max(dot(n, h), 0.0), 600.0) * 60.0 * step(0.0, frame.sun_dir.y);
        color += frame.sun_color.rgb * glint * fresnel;

        // Foam: where the water is shallow, and on squeezed crests, in a
        // pattern carried by the current.
        let shore = 1.0 - smoothstep(0.0, material.custom[2].w, depth);
        var crest = 0.0;
        if WATER_CREST_FOAM {
            crest = smoothstep(0.8, 0.35, wave.squeeze);
        }
        let drift = s.world.xz - flow * t;
        let pattern = 0.6 * noise2(drift * 2.7) + 0.4 * noise2(drift * 6.1 + 13.0);
        let cover = max(shore, crest);
        let foam = smoothstep(1.0 - cover, 1.15 - cover * 0.9, pattern) * cover * material.custom[1].w;
        color = mix(color, vec3<f32>(0.92) * water_ambient(), clamp(foam, 0.0, 1.0));
    } else {
        // Seen from below: the world above through Snell's window, the
        // water's own colour mirrored outside it.
        let down = -n;
        let facing = clamp(dot(down, view), 0.0, 1.0);
        let bent = refract(-view, down, 1.33);
        let window = select(0.0, 1.0 - pow(1.0 - facing, 3.0), dot(bent, bent) > 0.0);
        let above = scene_color(s.screen + down.xz * frame.screen.y * 0.02);
        color = mix(scatter, above, window);
    }

    out.unlit = true;
    out.albedo = vec3<f32>(0.0);
    out.emissive = color;
    out.normal = n;
    out.alpha = 1.0;
    out.roughness = 0.03;
    out.metallic = 0.0;
    out.occlusion = 1.0;
    return out;
}
