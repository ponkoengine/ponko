//! Material layers (glTF's material extensions) drawn offscreen, each
//! against the same material without it (the negative control):
//!
//! - unlit: shows its color whatever the light, where the lit one is dark;
//! - clear coat: a sharp highlight over a rough base, moved by the coat's
//!   own normal map (a flat map draws as no map);
//! - sheen: tints the rim of a dark ball with the sheen's color;
//! - specular: strength 0 takes a dielectric's highlight away, a color
//!   tints it, a higher index of refraction brightens it;
//! - anisotropy: stretches the highlight, across for one direction and
//!   along for the other;
//! - iridescence: colors the reflection of a white sky.
//!
//! Every layer that is off draws exactly as the plain material does. Skips
//! without a GPU (a skip is not a pass).
use super::gpu::GpuTexture;
use super::regression_stage::*;
use super::render_test_support::{Image, Tolerance};
use crate::prelude::*;
use crate::render::{MaterialDesc, MaterialLayers, MeshData};
use crate::texture::Texture;

/// A stage looking straight at a ball at the origin, lit by the sun from
/// the upper left front and by a white sky.
fn stage() -> Option<Stage> {
    let mut stage = Stage::new()?;
    stage.ctx.scene3d.camera.position = vec3(0.0, 0.0, 3.0);
    stage.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let lighting = &mut stage.ctx.scene3d.lighting;
    lighting.sun_direction = vec3(0.4, -0.5, -1.0).normalize();
    lighting.sun_intensity = 2.0;
    lighting.ambient_intensity = 0.3;
    Some(stage)
}

/// The picture of a ball 1 across with `desc`, alone on the stage.
fn ball(stage: &mut Stage, desc: MaterialDesc) -> Image {
    let mesh = stage.mesh(&MeshData::sphere(0.5, 48), desc);
    let e = stage.block(mesh, Vec3::splat(-0.5), Vec3::splat(0.5));
    let image = stage.image();
    stage.ctx.world.despawn(e);
    image
}

/// Every pixel's color (0 to 255).
fn pixels(image: &Image) -> impl Iterator<Item = (u32, u32, [f32; 3])> + '_ {
    (0..image.height).flat_map(move |y| {
        (0..image.width).map(move |x| {
            let [r, g, b, _] = image.pixel(x, y);
            (x, y, [f32::from(r), f32::from(g), f32::from(b)])
        })
    })
}

/// The brightest pixel's luma.
fn peak(image: &Image) -> f32 {
    pixels(image).map(|(_, _, c)| luma(c)).fold(0.0, f32::max)
}

/// The width and height of the pixels brighter than `share` of the peak.
fn highlight(image: &Image, share: f32) -> (u32, u32) {
    let bar = peak(image) * share;
    let (mut lo, mut hi) = ((u32::MAX, u32::MAX), (0, 0));
    for (x, y, c) in pixels(image) {
        if luma(c) >= bar {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
    }
    (hi.0 + 1 - lo.0, hi.1 + 1 - lo.1)
}

/// Drawn the same, pixel for pixel (one step of rounding).
const SAME: Tolerance = Tolerance {
    channel: 1,
    outliers: 0.0,
    edge: 0,
};

/// A dark, rough plastic.
fn dark() -> MaterialDesc {
    MaterialDesc {
        color: Color::rgb(0.1, 0.1, 0.1),
        roughness: 1.0,
        metallic: 0.0,
        ..Default::default()
    }
}

/// A linear 1x1 texture of `rgba`, in the stage's texture list.
fn data_texture(stage: &mut Stage, rgba: [u8; 4]) -> Texture {
    let size = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = stage.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("layer map"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    stage.queue.write_texture(
        texture.as_image_copy(),
        &rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        size,
    );
    let id = stage.textures.len() as u32;
    let bind_group = stage.textures[0].bind_group.clone();
    stage.textures.push(GpuTexture {
        texture,
        nearest: false,
        mips: None,
        bind_group,
    });
    Texture::new(id, 1, 1)
}

#[test]
fn an_unlit_material_shows_its_color_in_the_dark() {
    let Some(mut stage) = stage() else { return };
    stage.ctx.scene3d.lighting.sun_intensity = 0.0;
    stage.ctx.scene3d.lighting.ambient_intensity = 0.02;
    let red = MaterialDesc::color(Color::rgb(1.0, 0.0, 0.0));
    let lit = ball(&mut stage, red);
    let unlit = ball(&mut stage, red.with_unlit(true));
    let at = |image: &Image| stage.color_at(image, Vec3::ZERO);
    let (lit, unlit) = (at(&lit), at(&unlit));
    assert!(
        unlit[0] > 245.0 && unlit[1] < 10.0 && unlit[2] < 10.0,
        "unlit red: {unlit:?}"
    );
    assert!(lit[0] < 80.0, "the same material lit in the dark: {lit:?}");
    stage.finish();
}

#[test]
fn a_clear_coat_adds_a_sharp_highlight_its_normal_map_moves() {
    let Some(mut stage) = stage() else { return };
    let plain = ball(&mut stage, dark());
    let off = ball(
        &mut stage,
        dark().with_layers(MaterialLayers::NONE.with_clearcoat(0.0, 0.0)),
    );
    assert_similar(&plain, &off, SAME, "a clear coat of 0 and none");
    let coat = MaterialLayers::NONE.with_clearcoat(1.0, 0.05);
    let coated = ball(&mut stage, dark().with_layers(coat));
    let (plain_peak, coated_peak) = (peak(&plain), peak(&coated));
    assert!(
        coated_peak > plain_peak + 60.0,
        "coated peak {coated_peak}, rough base alone {plain_peak}"
    );
    // The coat's own normal map: flat draws as no map, tilted moves the
    // highlight.
    let flat = data_texture(&mut stage, [128, 128, 255, 255]);
    let tilted = data_texture(&mut stage, [218, 128, 200, 255]);
    let flat = ball(
        &mut stage,
        dark().with_layers(coat.with_clearcoat_normal_map(Some(flat))),
    );
    let tilted = ball(
        &mut stage,
        dark().with_layers(coat.with_clearcoat_normal_map(Some(tilted))),
    );
    let loose = Tolerance {
        channel: 3,
        outliers: 0.002,
        edge: 0,
    };
    assert_similar(&coated, &flat, loose, "a flat clear coat normal map and none");
    assert_differs(&coated, &tilted, loose, "a tilted clear coat normal map and none");
    stage.finish();
}

#[test]
fn sheen_tints_the_rim_with_its_color() {
    let Some(mut stage) = stage() else { return };
    let black = MaterialDesc {
        color: Color::BLACK,
        ..dark()
    };
    let plain = ball(&mut stage, black);
    let off = ball(
        &mut stage,
        black.with_layers(MaterialLayers::NONE.with_sheen(Color::BLACK, 0.5)),
    );
    assert_similar(&plain, &off, SAME, "a black sheen and none");
    let sheen = ball(
        &mut stage,
        black.with_layers(MaterialLayers::NONE.with_sheen(Color::rgb(1.0, 0.0, 0.0), 0.5)),
    );
    let redness = |image: &Image| pixels(image).map(|(_, _, [r, g, _])| r - g).fold(0.0, f32::max);
    let (plain, sheen) = (redness(&plain), redness(&sheen));
    assert!(sheen > 40.0, "a red sheen: red over green by at most {sheen}");
    assert!(plain < 3.0, "no sheen: red over green by at most {plain}");
    stage.finish();
}

#[test]
fn specular_strength_color_and_index_of_refraction_change_the_highlight() {
    let Some(mut stage) = stage() else { return };
    let glossy = MaterialDesc {
        color: Color::rgb(0.05, 0.05, 0.05),
        roughness: 0.25,
        metallic: 0.0,
        ..Default::default()
    };
    let plain = ball(&mut stage, glossy);
    let same = ball(
        &mut stage,
        glossy.with_layers(MaterialLayers::NONE.with_specular(1.0, Color::WHITE).with_ior(1.5)),
    );
    assert_similar(&plain, &same, SAME, "glTF's default specular and none");
    let none = ball(
        &mut stage,
        glossy.with_layers(MaterialLayers::NONE.with_specular(0.0, Color::WHITE)),
    );
    let (plain_peak, none_peak) = (peak(&plain), peak(&none));
    assert!(
        none_peak + 40.0 < plain_peak,
        "specular 0 peaks at {none_peak}, the default at {plain_peak}"
    );
    let red = ball(
        &mut stage,
        glossy.with_layers(MaterialLayers::NONE.with_specular(1.0, Color::rgb(1.0, 0.0, 0.0))),
    );
    let tint = pixels(&red).map(|(_, _, [r, g, _])| r - g).fold(0.0, f32::max);
    assert!(tint > 40.0, "a red specular color: red over green by at most {tint}");
    let dense = ball(&mut stage, glossy.with_layers(MaterialLayers::NONE.with_ior(2.5)));
    let total = |image: &Image| pixels(image).map(|(_, _, c)| luma(c)).sum::<f32>();
    let (dense, plain) = (total(&dense), total(&plain));
    assert!(
        dense > plain * 1.2,
        "index of refraction 2.5 gives {dense} in all, 1.5 {plain}"
    );
    stage.finish();
}

#[test]
fn anisotropy_stretches_the_highlight_in_its_direction() {
    let Some(mut stage) = stage() else { return };
    let metal = MaterialDesc {
        color: Color::rgb(0.9, 0.9, 0.9),
        roughness: 0.2,
        metallic: 1.0,
        ..Default::default()
    };
    stage.ctx.scene3d.lighting.ambient_intensity = 0.0;
    let plain = ball(&mut stage, metal);
    let off = ball(
        &mut stage,
        metal.with_layers(MaterialLayers::NONE.with_anisotropy(0.0, 1.0)),
    );
    assert_similar(&plain, &off, SAME, "anisotropy 0 and none");
    let along = ball(
        &mut stage,
        metal.with_layers(MaterialLayers::NONE.with_anisotropy(0.9, 0.0)),
    );
    let across = ball(
        &mut stage,
        metal.with_layers(MaterialLayers::NONE.with_anisotropy(0.9, std::f32::consts::FRAC_PI_2)),
    );
    let aspect = |image: &Image| {
        let (w, h) = highlight(image, 0.6);
        w as f32 / h as f32
    };
    let (plain, along, across) = (aspect(&plain), aspect(&along), aspect(&across));
    assert!(
        along > plain * 1.5 && across < plain / 1.5,
        "highlight width over height: {plain} isotropic, {along} stretched along u, {across} along v"
    );
    stage.finish();
}

#[test]
fn iridescence_colors_the_reflection_of_a_white_sky() {
    let Some(mut stage) = stage() else { return };
    stage.ctx.scene3d.lighting.ambient_intensity = 1.0;
    let mirror = MaterialDesc {
        color: Color::rgb(0.9, 0.9, 0.9),
        roughness: 0.3,
        metallic: 1.0,
        ..Default::default()
    };
    let spread = |image: &Image| {
        pixels(image)
            .map(|(_, _, [r, g, b])| r.max(g).max(b) - r.min(g).min(b))
            .fold(0.0, f32::max)
    };
    let plain = ball(&mut stage, mirror);
    let off = ball(
        &mut stage,
        mirror.with_layers(MaterialLayers::NONE.with_iridescence(0.0, 1.3, 400.0)),
    );
    assert_similar(&plain, &off, SAME, "iridescence 0 and none");
    let film = ball(
        &mut stage,
        mirror.with_layers(MaterialLayers::NONE.with_iridescence(1.0, 1.3, 400.0)),
    );
    let (plain, film) = (spread(&plain), spread(&film));
    assert!(film > 30.0, "a thin film: channels apart by up to {film}");
    assert!(plain < 4.0, "no film: channels apart by up to {plain}");
    stage.finish();
}
