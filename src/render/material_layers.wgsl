// ---- Material layers ----
// Layers over the metallic-roughness surface, as glTF's material
// extensions describe them: clear coat, sheen, specular strength and color
// with the index of refraction, anisotropy and iridescence
// (`MaterialLayers`). `MATERIAL_LAYERS` is false in every shader but the
// layered build of the built-in one (see `compose_material`): a constant,
// so the others run none of the code it guards (Safari still checks it:
// see `ScreenSteps`).
const MATERIAL_LAYERS: bool = false;

// Which layers are on (`material.layer_info.x`).
const LAYER_CLEARCOAT: u32 = 1u;
const LAYER_SHEEN: u32 = 2u;
const LAYER_ANISOTROPY: u32 = 8u;
const LAYER_IRIDESCENCE: u32 = 16u;
const LAYER_TRANSMISSION: u32 = 32u;

// The layer maps, in `material.layer_info.y`'s order (two bits each: 0
// none, 1 or 2 the slot it is bound in).
const MAP_CLEARCOAT: u32 = 0u;
const MAP_CLEARCOAT_ROUGHNESS: u32 = 1u;
const MAP_CLEARCOAT_NORMAL: u32 = 2u;
const MAP_SHEEN_COLOR: u32 = 3u;
const MAP_SHEEN_ROUGHNESS: u32 = 4u;
const MAP_SPECULAR: u32 = 5u;
const MAP_SPECULAR_COLOR: u32 = 6u;
const MAP_ANISOTROPY: u32 = 7u;
const MAP_IRIDESCENCE: u32 = 8u;
const MAP_IRIDESCENCE_THICKNESS: u32 = 9u;

// A pixel's layers, from the material and its maps (see `fill_layers`).
struct Layers {
    on: u32,
    clearcoat: f32,
    clearcoat_roughness: f32,
    // Unit, world space: the coat's own normal map, else the mesh's.
    clearcoat_normal: vec3<f32>,
    sheen: vec3<f32>,
    sheen_roughness: f32,
    // A non-metal's reflectance head-on, and how strong its reflection is.
    f0: vec3<f32>,
    specular: f32,
    // 0 to 1, along `bitangent` across `tangent` (unit, world space).
    anisotropy: f32,
    tangent: vec3<f32>,
    bitangent: vec3<f32>,
    iridescence: f32,
    // The thin film's reflectance at this view, as metal or non-metal.
    film: vec3<f32>,
};

var<private> layers: Layers;
// The two layer images at this pixel (white where not bound).
var<private> layer_texel0: vec4<f32> = vec4<f32>(1.0);
var<private> layer_texel1: vec4<f32> = vec4<f32>(1.0);
var<private> layer_uv: vec2<f32>;
var<private> layer_uv1: vec2<f32>;

// How the mesh's position and texture coordinates change from this pixel to
// the next across (`*_x`) and down (`*_y`) the screen. `surface_at` takes
// them and passes them in: nothing in this file uses `dpdx`, `dpdy` or an
// implicit-derivative `textureSample*`. Browsers check those for uniform
// control flow, and Safari's WebGPU also rejects them in functions that no
// fragment shader reaches, such as these in a compute shader or in a
// material without layers.
struct ScreenSteps {
    world_x: vec3<f32>,
    world_y: vec3<f32>,
    uv_x: vec2<f32>,
    uv_y: vec2<f32>,
    uv1_x: vec2<f32>,
    uv1_y: vec2<f32>,
};
var<private> layer_steps: ScreenSteps;

// The screen steps of `map_uv(uv, uv1, kind)`. The map is affine (a 2 x 3
// matrix on the coordinates), so a step goes through its linear part.
fn layer_uv_steps(kind: u32) -> mat2x2<f32> {
    let origin = map_uv(vec2<f32>(0.0), vec2<f32>(0.0), kind);
    let x = map_uv(layer_steps.uv_x, layer_steps.uv1_x, kind) - origin;
    let y = map_uv(layer_steps.uv_y, layer_steps.uv1_y, kind) - origin;
    return mat2x2<f32>(x, y);
}

// `t`'s texel at `uv`, whose screen steps are `steps`, with the mip bias of
// `textureSampleBias(t, material_sampler, uv, texture_mip_bias())`: scaling
// the steps by 2^bias moves the level of detail by the bias.
fn layer_sample0(uv: vec2<f32>, steps: mat2x2<f32>) -> vec4<f32> {
    let scale = exp2(texture_mip_bias());
    return textureSampleGrad(custom_texture0, material_sampler, uv, steps[0] * scale, steps[1] * scale);
}

fn layer_sample1(uv: vec2<f32>, steps: mat2x2<f32>) -> vec4<f32> {
    let scale = exp2(texture_mip_bias());
    return textureSampleGrad(custom_texture1, material_sampler, uv, steps[0] * scale, steps[1] * scale);
}

// Map `kind`'s texel, or `otherwise` without one.
fn layer_map(kind: u32, otherwise: vec4<f32>) -> vec4<f32> {
    let slot = (material.layer_info.y >> (2u * kind)) & 3u;
    if MATERIAL_MULTI_UV {
        let uv = map_uv(layer_uv, layer_uv1, UV_LAYER_START + kind);
        let steps = layer_uv_steps(UV_LAYER_START + kind);
        if slot == 1u {
            return layer_sample0(uv, steps);
        }
        if slot == 2u {
            return layer_sample1(uv, steps);
        }
        return otherwise;
    }
    if slot == 1u {
        return layer_texel0;
    }
    if slot == 2u {
        return layer_texel1;
    }
    return otherwise;
}

fn has_layer_map(kind: u32) -> bool {
    return ((material.layer_info.y >> (2u * kind)) & 3u) != 0u;
}

fn layer_max3(v: vec3<f32>) -> f32 {
    return max(v.x, max(v.y, v.z));
}

// The texture directions at this pixel (Schüler's cotangent frame), for
// texture coordinates with the screen steps `uv_steps`.
fn layer_frame(uv_steps: mat2x2<f32>, geometric: vec3<f32>) -> mat2x3<f32> {
    let dp1 = layer_steps.world_x;
    let dp2 = -layer_steps.world_y;
    let duv1 = uv_steps[0];
    let duv2 = -uv_steps[1];
    let dp2perp = cross(dp2, geometric);
    let dp1perp = cross(geometric, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-20));
    return mat2x3<f32>(t * inv, b * inv);
}

// Fills `layers`. `steps` are the screen steps of `uv`, `uv1` and the
// position (see `ScreenSteps`).
fn fill_layers(s: Surface, uv: vec2<f32>, uv1: vec2<f32>, steps: ScreenSteps, geometric: vec3<f32>, transformed_normal: vec3<f32>) {
    let info = material.layer_info;
    layer_steps = steps;
    if MATERIAL_MULTI_UV {
        layer_uv = uv;
        layer_uv1 = uv1;
    } else {
        let uv_steps = mat2x2<f32>(steps.uv_x, steps.uv_y);
        if (info.z & 1u) != 0u {
            layer_texel0 = layer_sample0(uv, uv_steps);
        }
        if (info.z & 2u) != 0u {
            layer_texel1 = layer_sample1(uv, uv_steps);
        }
    }
    var anisotropy_steps = mat2x2<f32>(steps.uv_x, steps.uv_y);
    if MATERIAL_MULTI_UV {
        anisotropy_steps = layer_uv_steps(select(UV_BASE, UV_LAYER_START + MAP_ANISOTROPY, has_layer_map(MAP_ANISOTROPY)));
    }
    let basis = layer_frame(anisotropy_steps, geometric);
    let t = basis[0];
    let b = basis[1];
    var l: Layers;
    l.on = info.x;
    let values = material.layers;
    let n_v = max(dot(s.normal, s.view), 1e-4);

    // Clear coat: amount in red, roughness in green.
    l.clearcoat = values[0].x * layer_map(MAP_CLEARCOAT, vec4<f32>(1.0)).r;
    l.clearcoat_roughness = clamp(values[0].y * layer_map(MAP_CLEARCOAT_ROUGHNESS, vec4<f32>(1.0)).g, 0.03, 1.0);
    l.clearcoat_normal = geometric;
    if has_layer_map(MAP_CLEARCOAT_NORMAL) {
        var m = layer_map(MAP_CLEARCOAT_NORMAL, vec4<f32>(0.5, 0.5, 1.0, 1.0)).xyz * 2.0 - 1.0;
        m = vec3<f32>(m.xy * values[0].z, m.z);
        // glTF: +Y up the image, along -v (see `surface_at`).
        var coat_basis = basis;
        if MATERIAL_MULTI_UV {
            coat_basis = layer_frame(layer_uv_steps(UV_LAYER_START + MAP_CLEARCOAT_NORMAL), geometric);
        }
        l.clearcoat_normal = normalize(coat_basis[0] * m.x - coat_basis[1] * m.y + geometric * m.z);
    }

    // Sheen: color in red, green and blue (read as sRGB), roughness in alpha.
    l.sheen = values[1].rgb * layer_map(MAP_SHEEN_COLOR, vec4<f32>(1.0)).rgb;
    l.sheen_roughness = clamp(values[1].w * layer_map(MAP_SHEEN_ROUGHNESS, vec4<f32>(1.0)).a, 0.0, 1.0);

    // Specular: color in red, green and blue (sRGB), strength in alpha;
    // the reflectance head-on from the index of refraction.
    let ior = values[3].x;
    let reflectance = (ior - 1.0) / (ior + 1.0);
    l.f0 = min(reflectance * reflectance * values[2].rgb * layer_map(MAP_SPECULAR_COLOR, vec4<f32>(1.0)).rgb, vec3<f32>(1.0));
    l.specular = values[2].w * layer_map(MAP_SPECULAR, vec4<f32>(1.0)).a;

    // Anisotropy: a direction in the texture's directions (red, green:
    // -1 to 1, +Y up the image), turned by the rotation, and a strength
    // (blue). Negative strengths stretch across the direction.
    l.anisotropy = 0.0;
    l.tangent = t;
    l.bitangent = b;
    if (info.x & LAYER_ANISOTROPY) != 0u {
        let texel = layer_map(MAP_ANISOTROPY, vec4<f32>(1.0, 0.5, 1.0, 1.0));
        let d = texel.rg * 2.0 - 1.0;
        let c = values[3].z;
        let sn = values[3].w;
        let turned = vec2<f32>(c * d.x - sn * d.y, sn * d.x + c * d.y);
        var strength = values[3].y * texel.b;
        let along = t * turned.x - b * turned.y;
        var tangent = normalize(along - s.normal * dot(s.normal, along) + vec3<f32>(1e-6, 0.0, 0.0));
        var bitangent = cross(s.normal, tangent);
        if strength < 0.0 {
            let swap = tangent;
            tangent = bitangent;
            bitangent = swap;
            strength = -strength;
        }
        l.anisotropy = clamp(strength, 0.0, 1.0);
        l.tangent = tangent;
        l.bitangent = bitangent;
    }

    // Iridescence: amount in red, thickness (between the two) in green.
    l.iridescence = 0.0;
    l.film = vec3<f32>(0.0);
    if (info.x & LAYER_IRIDESCENCE) != 0u {
        l.iridescence = clamp(values[4].x * layer_map(MAP_IRIDESCENCE, vec4<f32>(1.0)).r, 0.0, 1.0);
        let thickness = mix(values[4].z, values[4].w, layer_map(MAP_IRIDESCENCE_THICKNESS, vec4<f32>(1.0)).g);
        let base = mix(l.f0, s.albedo, s.metallic);
        l.film = thin_film(1.0, values[4].y, n_v, thickness, base);
    }
    layers = l;
    // `info`, not `l`: `l` holds per-pixel values (the texture directions),
    // so a browser's uniformity analysis counts a branch on any of it as
    // non-uniform. The material's own flags keep the branch uniform, as
    // the other layers' branches are.
    if (info.x & LAYER_TRANSMISSION) != 0u {
        fill_transmission(transformed_normal);
    }
}

// ---- Direct light ----

// GGX with the height-correlated Smith visibility, stretched to roughness
// `at` along `layers.tangent` and `ab` along `layers.bitangent`: the
// specular term D * V (V holds the 1 / (4 n.l n.v)).
fn anisotropic_ggx(l: vec3<f32>, v: vec3<f32>, n: vec3<f32>, h: vec3<f32>, at: f32, ab: f32) -> f32 {
    let t = layers.tangent;
    let b = layers.bitangent;
    let n_l = max(dot(n, l), 1e-4);
    let n_v = max(dot(n, v), 1e-4);
    let a2 = at * ab;
    let f = vec3<f32>(ab * dot(t, h), at * dot(b, h), a2 * max(dot(n, h), 0.0));
    let w2 = a2 / max(dot(f, f), 1e-12);
    let d = a2 * w2 * w2 / PI;
    let gv = n_l * length(vec3<f32>(at * dot(t, v), ab * dot(b, v), n_v));
    let gl = n_v * length(vec3<f32>(at * dot(t, l), ab * dot(b, l), n_l));
    return d * clamp(0.5 / max(gv + gl, 1e-6), 0.0, 1.0);
}

// The specular term D * G / (4 n.l n.v) of GGX at `roughness`, as
// `direct_light` has it.
fn ggx_specular(n_l: f32, n_v: f32, n_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d_denom = n_h * n_h * (a2 - 1.0) + 1.0;
    let d = a2 / (PI * d_denom * d_denom);
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let g = n_v / (n_v * (1.0 - k) + k) * n_l / (n_l * (1.0 - k) + k);
    return d * g / max(4.0 * n_v * n_l, 1e-4);
}

// The sheen's microfiber distribution (Estevez and Kulla's "Charlie").
fn sheen_distribution(roughness: f32, n_h: f32) -> f32 {
    let inv = 1.0 / max(roughness * roughness, 1e-6);
    let sin2 = max(1.0 - n_h * n_h, 0.0);
    return (2.0 + inv) * pow(sin2, inv * 0.5) / (2.0 * PI);
}

fn sheen_lambda_fit(x: f32, a: f32) -> f32 {
    let w = (1.0 - a) * (1.0 - a);
    let p = mix(21.5473, 25.3245, w);
    let q = mix(3.82987, 3.32435, w);
    let r = mix(0.19823, 0.16801, w);
    let s = mix(-1.97760, -1.27393, w);
    let u = mix(-4.32054, -4.85967, w);
    return p / (1.0 + q * pow(x, r)) + s * x + u;
}

fn sheen_lambda(c: f32, a: f32) -> f32 {
    if c < 0.5 {
        return exp(sheen_lambda_fit(c, a));
    }
    return exp(2.0 * sheen_lambda_fit(0.5, a) - sheen_lambda_fit(1.0 - c, a));
}

// The sheen's visibility (Estevez and Kulla), with the 1 / (4 n.l n.v).
fn sheen_visibility(n_l: f32, n_v: f32, roughness: f32) -> f32 {
    let a = max(roughness * roughness, 1e-6);
    return clamp(1.0 / ((1.0 + sheen_lambda(n_v, a) + sheen_lambda(n_l, a)) * (4.0 * n_v * n_l)), 0.0, 1.0);
}

// How much of the light coming from `mu` (its cosine to the normal) a
// white sheen of `roughness` reflects, over every direction: what it takes
// from the layer under it. A fit (RMS error 0.02) of the integral of the
// two functions above, made for this shader, which has no texture slot to
// spare for a table.
fn sheen_albedo(mu: f32, roughness: f32) -> f32 {
    let r = clamp(roughness, 0.05, 1.0);
    let a = 1.4924 + r * (-2.9561 + r * 2.3887);
    let b = 11.3448 + r * (-22.1403 + r * 12.57);
    let c = -0.1429 + r * (1.1099 - r * 1.0403);
    return clamp(a * exp(-b * mu) + c * (1.0 - mu), 0.0, 1.0);
}

// The light a layered surface reflects toward the viewer from a light in
// direction `l` bringing `radiance`: `direct_light` for the layered build.
fn layered_light(s: Surface, l: vec3<f32>, radiance: vec3<f32>) -> vec3<f32> {
    let n = s.normal;
    let v = s.view;
    let h = (l + v) / max(length(l + v), 1e-6);
    let n_l = max(dot(n, l), 0.0);
    let n_v = max(dot(n, v), 1e-4);
    // At most 1, as in `direct_light`.
    let v_h = clamp(dot(v, h), 0.0, 1.0);
    let schlick = pow(1.0 - v_h, 5.0);
    var color = vec3<f32>(0.0);
    if transmission.diffuse > 0.0 || transmission.scatter > 0.0 {
        let exit_fresnel = pow(1.0 - clamp(n_v, 0.0, 1.0), 5.0);
        let reflected = layers.specular * layer_max3(layers.f0 + (1.0 - layers.f0) * exit_fresnel);
        color = transmitted_light(s, l, reflected);
    }
    if n_l > 0.0 {
        var specular = 0.0;
        if (layers.on & LAYER_ANISOTROPY) != 0u {
            let a = s.roughness * s.roughness;
            let at = mix(a, 1.0, layers.anisotropy * layers.anisotropy);
            specular = anisotropic_ggx(l, v, n, h, at, clamp(a, 0.001, 1.0));
        } else {
            specular = ggx_specular(n_l, n_v, max(dot(n, h), 0.0), s.roughness);
        }
        var dielectric_f = layers.f0 + (vec3<f32>(1.0) - layers.f0) * schlick;
        var metal_f = s.albedo + (vec3<f32>(1.0) - s.albedo) * schlick;
        if (layers.on & LAYER_IRIDESCENCE) != 0u {
            dielectric_f = mix(dielectric_f, layers.film, layers.iridescence);
            metal_f = mix(metal_f, layers.film, layers.iridescence);
        }
        // A non-metal: diffuse under a reflection as strong as `specular`.
        let diffuse_share = (1.0 - transmission.factor) * (1.0 - transmission.diffuse);
        let dielectric = diffuse_share * s.albedo / PI * (1.0 - layers.specular * layer_max3(dielectric_f))
            + dielectric_f * (layers.specular * specular);
        var base = mix(dielectric, metal_f * specular, s.metallic);
        if (layers.on & LAYER_SHEEN) != 0u {
            let strongest = layer_max3(layers.sheen);
            let r = layers.sheen_roughness;
            let under = min(1.0 - strongest * sheen_albedo(n_v, r), 1.0 - strongest * sheen_albedo(n_l, r));
            base = base * under + layers.sheen * (sheen_distribution(r, max(dot(n, h), 0.0)) * sheen_visibility(n_l, n_v, r));
        }
        color += base * n_l;
    }
    if (layers.on & LAYER_CLEARCOAT) != 0u {
        let c = layers.clearcoat_normal;
        let c_v = max(dot(c, v), 1e-4);
        let c_l = max(dot(c, l), 0.0);
        // What the coat reflects head-on (4%: an index of refraction of 1.5)
        // it takes from the light reaching the base.
        let coat_f = 0.04 + 0.96 * pow(1.0 - c_v, 5.0);
        color *= 1.0 - layers.clearcoat * coat_f;
        if c_l > 0.0 {
            let f = 0.04 + 0.96 * schlick;
            color += layers.clearcoat * f * ggx_specular(c_l, c_v, max(dot(c, h), 0.0), layers.clearcoat_roughness) * c_l;
        }
    }
    return color * radiance;
}

// ---- Environment light ----

// The direction a layered surface reflects the environment from: for an
// anisotropic one, the normal bent toward the stretch (as the glTF Sample
// Viewer and Filament do), so reflections smear along it.
fn layered_reflection(s: Surface, ray: vec3<f32>) -> vec3<f32> {
    if (layers.on & LAYER_ANISOTROPY) == 0u || layers.anisotropy <= 0.0 {
        return ray;
    }
    let along = layers.bitangent;
    let across = cross(along, s.view);
    let bent = cross(across, along);
    let bend = 1.0 - layers.anisotropy * (1.0 - s.roughness);
    let bend4 = bend * bend * bend * bend;
    let n = normalize(mix(bent, s.normal, bend4));
    return reflect(-s.view, n);
}

// The environment's light on a layered surface: `environment_light`'s end
// for the layered build. `diffuse` and `specular` are the environment's
// light around the normal and from the reflection, `brdf` the split-sum
// lookup at the view.
fn layered_ambient(s: Surface, diffuse: vec3<f32>, specular: vec3<f32>, brdf: vec2<f32>) -> vec3<f32> {
    let n_v = max(dot(s.normal, s.view), 1e-4);
    var f0 = layers.f0;
    var metal = s.albedo;
    if (layers.on & LAYER_IRIDESCENCE) != 0u {
        f0 = mix(f0, layers.film, layers.iridescence);
        metal = mix(metal, layers.film, layers.iridescence);
    }
    let dielectric_f = f0 * brdf.x + brdf.y;
    let through = (1.0 - transmission.factor) * transmission.diffuse;
    let diffuse_share = (1.0 - transmission.factor) * (1.0 - transmission.diffuse);
    var transmitted = vec3<f32>(0.0);
    if through > 0.0 || transmission.scatter > 0.0 {
        let behind = environment_sample(0.0, -s.normal, 0.0, true);
        let clear = volume_transmittance(transmission.thickness);
        let thin = transmission.thickness <= 0.0;
        let scattered = mix(diffuse, behind, 0.5 + select(0.0, 0.5 * transmission.anisotropy, thin));
        transmitted = through * behind * transmission.diffuse_color * clear
            + transmission.scatter * transmission.scatter_color * scatter_extinction() * scattered;
    }
    let dielectric = (diffuse_share * diffuse * s.albedo + transmitted) *
        (1.0 - layers.specular * layer_max3(dielectric_f)) + specular * dielectric_f * layers.specular;
    var base = mix(dielectric, specular * (metal * brdf.x + brdf.y), s.metallic);
    if (layers.on & LAYER_SHEEN) != 0u {
        // The sheen's lobe is wide: lit by the light around the normal.
        let e = sheen_albedo(n_v, layers.sheen_roughness);
        base = base * (1.0 - layer_max3(layers.sheen) * e) + layers.sheen * diffuse * e;
    }
    base *= s.occlusion;
    if (layers.on & LAYER_CLEARCOAT) != 0u {
        let c = layers.clearcoat_normal;
        let c_v = max(dot(c, s.view), 1e-4);
        let coat_f = 0.04 + 0.96 * pow(1.0 - c_v, 5.0);
        let lookup = environment_brdf(c_v, layers.clearcoat_roughness);
        let coat = environment_reflection(s.world, reflect(-s.view, c), layers.clearcoat_roughness) * (0.04 * lookup.x + lookup.y);
        base = base * (1.0 - layers.clearcoat * coat_f) + coat * (layers.clearcoat * s.occlusion);
    }
    return base;
}

// ---- Iridescence ----
// A thin film over the surface (Belcour and Barla, "A Practical Extension
// to Microfacet Theory for the Modeling of Varying Iridescence", 2017, as
// the glTF Sample Viewer evaluates it).

fn film_square(x: f32) -> f32 {
    return x * x;
}

fn film_f0(transmitted: f32, incident: f32) -> f32 {
    return film_square((transmitted - incident) / (transmitted + incident));
}

fn film_f0_rgb(transmitted: vec3<f32>, incident: f32) -> vec3<f32> {
    let r = (transmitted - vec3<f32>(incident)) / (transmitted + vec3<f32>(incident));
    return r * r;
}

// The XYZ color matching functions' Fourier transform at optical path
// difference `opd` (nanometers) and phase `shift`, in linear sRGB.
fn film_sensitivity(opd: f32, shift: vec3<f32>) -> vec3<f32> {
    let phase = 2.0 * PI * opd * 1.0e-9;
    let val = vec3<f32>(5.4856e-13, 4.4201e-13, 5.2481e-13);
    let pos = vec3<f32>(1.6810e+06, 1.7953e+06, 2.2084e+06);
    let spread = vec3<f32>(4.3278e+09, 9.3046e+09, 6.6121e+09);
    var xyz = val * sqrt(2.0 * PI * spread) * cos(pos * phase + shift) * exp(-phase * phase * spread);
    xyz.x += 9.7470e-14 * sqrt(2.0 * PI * 4.5282e+09) * cos(2.2399e+06 * phase + shift.x) * exp(-4.5282e+09 * phase * phase);
    xyz /= 1.0685e-7;
    let to_rgb = mat3x3<f32>(
        3.2404542, -0.9692660, 0.0556434,
        -1.5371385, 1.8760108, -0.2040259,
        -0.4985314, 0.0415560, 1.0572252,
    );
    return to_rgb * xyz;
}

// The reflectance of a film of index `film_ior` and `thickness` (nm), over
// a base of reflectance `base_f0`, seen at `cos1` from a medium of index
// `outside`.
fn thin_film(outside: f32, film_ior: f32, cos1: f32, thickness: f32, base_f0: vec3<f32>) -> vec3<f32> {
    // A film thinning to nothing turns into the medium outside.
    let ior = mix(outside, film_ior, smoothstep(0.0, 0.03, thickness));
    let sin2_sq = film_square(outside / ior) * (1.0 - cos1 * cos1);
    let cos2_sq = 1.0 - sin2_sq;
    if cos2_sq < 0.0 {
        // Total internal reflection.
        return vec3<f32>(1.0);
    }
    let cos2 = sqrt(cos2_sq);
    // The first interface.
    let r0 = film_f0(ior, outside);
    let r12 = r0 + (1.0 - r0) * pow(1.0 - cos1, 5.0);
    let t121 = 1.0 - r12;
    let phi12 = select(0.0, PI, ior < outside);
    let phi21 = PI - phi12;
    // The second interface.
    let root = sqrt(clamp(base_f0, vec3<f32>(0.0), vec3<f32>(0.9999)));
    let base_ior = (vec3<f32>(1.0) + root) / (vec3<f32>(1.0) - root);
    let r1 = film_f0_rgb(base_ior, ior);
    let r23 = r1 + (vec3<f32>(1.0) - r1) * pow(1.0 - cos2, 5.0);
    let phi23 = select(vec3<f32>(0.0), vec3<f32>(PI), base_ior < vec3<f32>(ior));
    // The phase shift.
    let opd = 2.0 * ior * thickness * cos2;
    let phi = vec3<f32>(phi21) + phi23;
    // The compound terms.
    let r123 = clamp(r12 * r23, vec3<f32>(1e-5), vec3<f32>(0.9999));
    let root123 = sqrt(r123);
    let rs = t121 * t121 * r23 / (vec3<f32>(1.0) - r123);
    // The reflectance for m = 0 (the DC term), then m = 1 and 2.
    var reflectance = vec3<f32>(r12) + rs;
    var cm = rs - vec3<f32>(t121);
    for (var m = 1; m <= 2; m++) {
        cm *= root123;
        reflectance += cm * 2.0 * film_sensitivity(f32(m) * opd, f32(m) * phi);
    }
    // Colors out of gamut can come out negative.
    return max(reflectance, vec3<f32>(0.0));
}
