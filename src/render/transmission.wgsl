// Transmission is evaluated by the layered material only. These values are
// filled alongside the other material layers, before any lighting branches.
const MAP_TRANSMISSION: u32 = 10u;
const MAP_THICKNESS: u32 = 11u;
const MAP_DIFFUSE_TRANSMISSION_COLOR: u32 = 12u;
const MAP_DIFFUSE_TRANSMISSION: u32 = 13u;
const MAP_SCATTER_COLOR: u32 = 14u;
const MAP_SCATTER: u32 = 15u;

struct Transmission {
    factor: f32,
    thickness: f32,
    distance: f32,
    attenuation: vec3<f32>,
    dispersion: f32,
    diffuse: f32,
    diffuse_color: vec3<f32>,
    scatter: f32,
    scatter_color: vec3<f32>,
    anisotropy: f32,
};
var<private> transmission: Transmission;

fn fill_transmission(transformed_normal: vec3<f32>) {
    let v = material.layers;
    transmission.factor = v[5].x * layer_map(MAP_TRANSMISSION, vec4<f32>(1.0)).r;
    // Its reciprocal length estimates world units per mesh unit along
    // the normal (exact for uniform model scaling).
    let scale = 1.0 / max(length(transformed_normal), 1e-6);
    transmission.thickness = v[5].y * layer_map(MAP_THICKNESS, vec4<f32>(1.0)).g * scale;
    transmission.distance = v[5].z;
    transmission.dispersion = v[5].w;
    transmission.attenuation = v[6].rgb;
    transmission.diffuse = v[6].a * layer_map(MAP_DIFFUSE_TRANSMISSION, vec4<f32>(1.0)).a;
    transmission.diffuse_color = v[7].rgb * layer_map(MAP_DIFFUSE_TRANSMISSION_COLOR, vec4<f32>(1.0)).rgb;
    transmission.anisotropy = v[7].a;
    transmission.scatter_color = v[8].rgb * layer_map(MAP_SCATTER_COLOR, vec4<f32>(1.0)).rgb;
    transmission.scatter = transmission.factor * v[8].a * layer_map(MAP_SCATTER, vec4<f32>(1.0)).a;
}

// Linear radiance at the entrance of the medium after it traveled a world
// distance. An infinite attenuation distance is encoded as zero.
fn volume_transmittance(travel: f32) -> vec3<f32> {
    if transmission.distance <= 0.0 || travel <= 0.0 {
        return vec3<f32>(1.0);
    }
    return pow(transmission.attenuation, vec3<f32>(travel / transmission.distance));
}

// Some draft scatter materials give a thickness and color but no finite
// attenuation distance. Give their authored scatter lobe a half share of
// transmission in that case, with matching loss from the clear path.
fn scatter_without_density() -> bool {
    return transmission.thickness > 0.0 && transmission.distance <= 0.0
        && layer_max3(transmission.scatter_color) > 0.0;
}

fn scatter_extinction() -> vec3<f32> {
    if transmission.thickness <= 0.0 {
        return vec3<f32>(1.0);
    }
    if scatter_without_density() {
        return vec3<f32>(0.5);
    }
    return 1.0 - volume_transmittance(transmission.thickness);
}

fn refraction_level(uv: vec2<f32>, level: i32) -> vec3<f32> {
    let center = 0.5 / vec2<f32>(textureDimensions(scene_color_texture, level));
    return textureSampleLevel(scene_color_texture, sky_sampler, clamp(uv, center, 1.0 - center), f32(level)).rgb;
}

// Bilinear within each mip, with an explicit blend between mips (the
// environment sampler has nearest mip filtering). WebGL uses level zero.
fn refraction_color(uv: vec2<f32>, roughness: f32) -> vec3<f32> {
    let last = f32(textureNumLevels(scene_color_texture) - 1u);
    let lod = clamp(roughness * roughness * last, 0.0, last);
    let low = i32(lod);
    var color = refraction_level(uv, low);
    if fract(lod) > 0.0 {
        color = mix(color, refraction_level(uv, low + 1), fract(lod));
    }
    return color;
}

// Project the ray's estimated exit from the volume. A wall of zero
// thickness keeps the incident direction and samples at the surface.
fn refracted_radiance(s: Surface, travel: f32, ior: f32) -> vec3<f32> {
    let ray = refract(-s.view, s.normal, 1.0 / ior);
    let projection = frame.view_proj * vec4<f32>(s.world + ray * travel, 1.0);
    let uv = projection.xy / max(projection.w, 1e-6) * vec2<f32>(0.5, -0.5) + 0.5;
    // Approximate the transmission lobe narrowing as IOR approaches 1.
    let roughness = s.roughness * clamp(2.0 * (ior - 1.0), 0.0, 1.0);
    let size = vec2<f32>(textureDimensions(scene_color_texture));
    if all(size == vec2<f32>(1.0)) || projection.w <= 0.0
        || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return environment_reflection(s.world, ray, roughness);
    }
    if travel > 0.0 && scene_distance(uv * size) + 0.001 < distance(s.world, frame.camera.xyz) {
        // A foreground object is not behind the glass.
        return refraction_color(s.screen / size, roughness);
    }
    return refraction_color(uv, roughness);
}

// Direct backlighting and a broad scatter lobe. Local extinction is
// represented by the authored thickness, rather than a second light march.
fn transmitted_light(s: Surface, l: vec3<f32>, reflected: f32) -> vec3<f32> {
    let back = max(-dot(s.normal, l), 0.0);
    let scatter = transmission.scatter;
    var diffuse = transmission.diffuse * (1.0 - transmission.factor) *
        transmission.diffuse_color * back * volume_transmittance(transmission.thickness);
    if scatter > 0.0 {
        let facing = dot(s.normal, l);
        if transmission.thickness <= 0.0 {
            let through = 0.5 + 0.5 * transmission.anisotropy;
            diffuse += transmission.scatter_color * scatter * mix(max(facing, 0.0), back, through);
        } else {
            // A broad single surface lobe approximates light redistributed
            // by the medium. Keep it finite at the ends of the phase range.
            let g = transmission.anisotropy * 0.75;
            let cosine = clamp(dot(-l, s.view), -1.0, 1.0);
            let phase = (1.0 - g * g) / pow(1.0 + g * g - 2.0 * g * cosine, 1.5);
            let extinguished = scatter_extinction();
            diffuse += extinguished * transmission.scatter_color * scatter * (0.25 + 0.75 * back) * phase * 0.25;
        }
    }
    return diffuse * (1.0 - reflected) * (1.0 - s.metallic) / PI;
}

// This is added to shading, whose diffuse lobe already relinquished the
// transmitted energy. Base color tints the light at the boundary, and
// Beer-Lambert absorption tints it along the ray inside.
fn specular_transmission(s: Surface) -> vec3<f32> {
    let travel = transmission.thickness;
    var scattered = select(transmission.scatter, 0.0, travel > 0.0);
    if scatter_without_density() {
        scattered = transmission.scatter * 0.5;
    }
    let amount = (transmission.factor - scattered) * (1.0 - s.metallic);
    if amount <= 0.0 {
        return vec3<f32>(0.0);
    }
    let ior = material.layers[3].x;
    var rgb = refracted_radiance(s, travel, ior);
    if travel > 0.0 && transmission.dispersion > 0.0 {
        // Two more lookups for the red and blue rays, with green at the
        // material's IOR. A zero dispersion only samples once.
        let spread = (ior - 1.0) * 0.025 * transmission.dispersion;
        let red = refracted_radiance(s, travel, max(1.0, ior - spread));
        let blue = refracted_radiance(s, travel, ior + spread);
        rgb = vec3<f32>(red.r, rgb.g, blue.b);
    }
    let n_v = clamp(dot(s.normal, s.view), 0.0, 1.0);
    let lookup = environment_brdf(n_v, s.roughness);
    var f0 = layers.f0;
    if (layers.on & LAYER_IRIDESCENCE) != 0u {
        f0 = mix(f0, layers.film, layers.iridescence);
    }
    let reflected = clamp((f0 * lookup.x + lookup.y) * layers.specular, vec3<f32>(0.0), vec3<f32>(1.0));
    let coat = 1.0 - layers.clearcoat * (0.04 + 0.96 * pow(1.0 - n_v, 5.0));
    return rgb * s.albedo * volume_transmittance(travel) * (1.0 - reflected) * amount * coat;
}
