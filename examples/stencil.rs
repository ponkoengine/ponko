//! Stencil state on materials: crossing glass strips that blend once where
//! they overlap, beside strips without a stencil that blend twice, and a
//! turning crate with an outline drawn only around it.
//!
//!   `cargo run --release --example stencil --features 3d`
//!
//! `SHOT=path.png` saves the second frame and quits.

use ponko::d3::*;
use ponko::prelude::*;
use ponko::render::{Blend, MaterialDesc, MeshData, Model, Stencil};

/// The outlined crate turns about its vertical axis.
#[derive(Clone, Copy, Component)]
struct Turning;

/// Two see-through strips crossing in a plus at `at`, drawn with `material`.
fn crossing_strips(ctx: &mut Ctx, at: Vec3, material: MaterialDesc) {
    let material = ctx.gfx.create_material(material);
    let strip = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
    for (offset, scale) in [(0.0, vec3(3.0, 0.5, 0.05)), (0.3, vec3(0.5, 3.0, 0.05))] {
        let transform = Transform {
            scale,
            ..Transform::at(at + vec3(0.0, 0.0, offset))
        };
        ctx.world.spawn((transform, Model::new(strip)));
    }
}

fn main() {
    Game::new("Ponko: stencil", 1280, 720)
        .system(|ctx: &mut Ctx| {
            if ctx.input.key_pressed(Key::Escape) {
                ctx.quit();
            }
            let turn = Quat::from_rotation_y(ctx.time.dt() * 0.6);
            ctx.world
                .query::<(&Turning, &mut Transform)>()
                .for_each(|(_, transform)| transform.rotation = turn * transform.rotation);
            if ctx.time.frame() == 2
                && let Ok(path) = std::env::var("SHOT")
            {
                ctx.gfx.save_screenshot(path);
            }
            if ctx.time.frame() == 4 && std::env::var("SHOT").is_ok() {
                ctx.quit();
            }
        })
        .setup(|ctx| {
            ctx.scene3d.camera.position = vec3(0.0, 2.0, 9.0);
            ctx.scene3d.camera.look_at(vec3(0.0, 0.5, 0.0));
            let ground = ctx
                .gfx
                .create_material(MaterialDesc::color(Color::hex(0x5A6B4A)).with_roughness(0.9));
            let floor = ctx.gfx.create_mesh(&MeshData::plane(30.0), ground);
            ctx.world
                .spawn((Transform::at(vec3(0.0, -1.0, 0.0)), Model::new(floor)));

            // Glass: each strip blends over what's behind it.
            let glass = MaterialDesc::color(Color::rgba(0.3, 0.7, 1.0, 0.45)).with_blend(Blend::Transparent);
            // Left: without a stencil the crossing blends twice (darker, bluer).
            crossing_strips(ctx, vec3(-3.2, 1.2, 0.0), glass);
            // Right: `Stencil::once` lets each pixel blend at most once.
            crossing_strips(ctx, vec3(3.2, 1.2, 0.0), glass.with_stencil(Stencil::once(1)));

            // The crate marks the stencil where it draws; the outline, a
            // larger copy drawn after it (order 1), shows only where the
            // crate didn't.
            let wood = ctx.gfx.create_material(
                MaterialDesc::color(Color::hex(0xB07A45))
                    .with_roughness(0.8)
                    .with_stencil(Stencil::mark(2)),
            );
            let glow = ctx.gfx.create_material(
                MaterialDesc::color(Color::rgb(0.0, 0.0, 0.0))
                    .with_emissive(Color::hex(0xFFD040))
                    .with_stencil(Stencil::outside(2).with_order(1)),
            );
            let crate_mesh = ctx.gfx.create_mesh(&MeshData::cube(1.4), wood);
            let outline = ctx.gfx.create_mesh(&MeshData::cube(1.55), glow);
            let at = Transform::at(vec3(0.0, -0.2, 0.0));
            ctx.world.spawn((at, Model::new(crate_mesh), Turning));
            ctx.world.spawn((at, Model::new(outline), Turning));
        })
        .run();
}
