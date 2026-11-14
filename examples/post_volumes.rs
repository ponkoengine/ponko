//! Post volumes: the camera walks from a meadow through a cave into a
//! sunset garden and back, and the picture's look blends smoothly as it
//! crosses each volume's blend distance. The cave is darker-adapted, cold
//! and desaturated, with depth of field; the garden is warm and glowing,
//! with orange highlights.
//!
//!   `cargo run --release --example post_volumes --features 3d`
//!
//! - Space: stop or go on walking.  Escape: quit.
//! - `SHOT=volumes.png` saves the frame halfway into the cave and exits.
use ponko::d3::Transform;
use ponko::prelude::*;
use ponko::render::{MaterialDesc, MeshData, Model, Post, PostVolume};

/// Where the walk turns back, and how fast it goes (meters a second).
const START: f32 = 12.0;
const END: f32 = -62.0;
const SPEED: f32 = 4.0;

/// The level's looks: outdoors everywhere, and the volumes over it.
struct Looks {
    outdoors: Post,
    volumes: Vec<PostVolume>,
}

/// The walk: how far along it the camera is, and whether it is walking.
struct Walk {
    z: f32,
    direction: f32,
    walking: bool,
}

fn looks() -> Looks {
    let mut outdoors = Post::default();
    outdoors.bloom = 0.05;
    let mut cave = outdoors;
    cave.exposure = 1.2; // eyes adjusted to the dark
    cave.saturation = 0.55;
    cave.temperature = -0.3;
    cave.vignette = 0.45;
    cave.split_toning.shadows = Color::rgb(0.3, 0.45, 0.65);
    cave.depth_of_field.enabled = true;
    cave.depth_of_field.focus_distance = 4.0;
    cave.depth_of_field.focus_range = 1.0;
    cave.depth_of_field.blur = 0.012;
    let mut garden = outdoors;
    garden.temperature = 0.45;
    garden.bloom = 0.25;
    garden.saturation = 1.15;
    garden.split_toning.highlights = Color::rgb(0.78, 0.5, 0.32);
    garden.split_toning.shadows = Color::rgb(0.45, 0.4, 0.55);
    Looks {
        outdoors,
        volumes: vec![
            // Center, size, blend distance (how far outside it fades out), look.
            PostVolume::new(vec3(0.0, 2.5, -18.0), vec3(8.0, 5.0, 16.0), 5.0, cave),
            PostVolume::new(vec3(0.0, 3.0, -52.0), vec3(24.0, 10.0, 20.0), 8.0, garden),
        ],
    }
}

/// Walks the camera along the path and turns at its ends.
fn walk(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Escape) {
        ctx.quit();
    }
    let dt = ctx.time.dt();
    let toggle = ctx.input.key_pressed(Key::Space);
    let w = ctx.world.resource_mut::<Walk>();
    if toggle {
        w.walking = !w.walking;
    }
    if w.walking {
        w.z += w.direction * SPEED * dt;
        if w.z < END || w.z > START {
            w.z = w.z.clamp(END, START);
            w.direction = -w.direction;
        }
    }
    let (z, direction) = (w.z, w.direction);
    let camera = &mut ctx.scene3d.camera;
    camera.position = vec3(0.0, 1.7, z);
    camera.look_at(vec3(0.0, 1.5, z - 10.0 * direction));
}

/// Every frame, after the camera moved: the look where the camera is.
fn blend_looks(ctx: &mut Ctx) {
    let camera = ctx.scene3d.camera.position;
    let looks = ctx.world.resource::<Looks>();
    ctx.scene3d.post = PostVolume::blend(&looks.outdoors, &looks.volumes, camera);
}

fn setup(ctx: &mut Ctx) {
    ctx.scene3d.lighting.sun_direction = vec3(-0.5, -0.4, -0.75);
    ctx.world.insert_resource(looks());
    ctx.world.insert_resource(Walk {
        z: START,
        direction: -1.0,
        walking: true,
    });
    // `glow` above 0 makes it shine by itself (above 1: brighter than white).
    let material = |ctx: &mut Ctx, color: u32, glow: f32| {
        let mut desc = MaterialDesc::color(Color::hex(color)).with_roughness(0.9);
        let c = Color::hex(color);
        desc.emissive = Color::rgb(c.r * glow, c.g * glow, c.b * glow);
        ctx.gfx.create_material(desc)
    };
    let grass = material(ctx, 0x5E8C4A, 0.0);
    let ground = ctx.gfx.create_mesh(&MeshData::plane(200.0), grass);
    ctx.world.spawn((Transform::default(), Model::new(ground)));

    // The cave: rock walls and a roof around the path.
    let rock = material(ctx, 0x6B625A, 0.0);
    let slab = ctx.gfx.create_mesh(&MeshData::cube(1.0), rock);
    for (at, scale) in [
        (vec3(-4.0, 2.5, -18.0), vec3(1.0, 5.0, 16.0)),
        (vec3(4.0, 2.5, -18.0), vec3(1.0, 5.0, 16.0)),
        (vec3(0.0, 5.2, -18.0), vec3(9.0, 0.6, 16.0)),
    ] {
        let mut t = Transform::at(at);
        t.scale = scale;
        ctx.world.spawn((t, Model::new(slab)));
    }
    // Crystals along the cave walls, near and far, so depth of field shows.
    let crystal = material(ctx, 0x7FD4FF, 2.5);
    let gem = ctx.gfx.create_mesh(&MeshData::sphere(0.25, 12), crystal);
    for i in 0..8 {
        let side = if i % 2 == 0 { -3.2 } else { 3.2 };
        ctx.world.spawn((
            Transform::at(vec3(side, 1.0 + (i % 3) as f32, -11.0 - i as f32 * 2.0)),
            Model::new(gem),
        ));
    }

    // The garden: lanterns and trees in the evening sun.
    let lantern = material(ctx, 0xFFB050, 4.0);
    let light = ctx.gfx.create_mesh(&MeshData::sphere(0.3, 16), lantern);
    let leaves = material(ctx, 0x3F7A3A, 0.0);
    let tree = ctx.gfx.create_mesh(&MeshData::sphere(1.4, 16), leaves);
    for i in 0..10 {
        let side = if i % 2 == 0 { -5.0 } else { 5.0 };
        let z = -44.0 - (i / 2) as f32 * 4.0;
        ctx.world
            .spawn((Transform::at(vec3(side * 0.6, 2.2, z)), Model::new(light)));
        ctx.world
            .spawn((Transform::at(vec3(side, 1.6, z - 2.0)), Model::new(tree)));
    }
}

fn main() {
    let shot = std::env::var("SHOT").ok();
    Game::new("Ponko: post volumes", 1280, 720)
        .system(walk)
        .system(blend_looks)
        .system(move |ctx: &mut Ctx| {
            // Halfway into the cave (from the start, at walking speed).
            let into_cave = ((START + 14.0) / SPEED / ctx.time.dt().max(1e-3)) as u64;
            if let Some(path) = &shot
                && ctx.time.frame() == into_cave
            {
                ctx.gfx.save_screenshot(path);
            }
            if shot.is_some() && ctx.time.frame() > into_cave + 2 {
                ctx.quit();
            }
        })
        .setup(setup)
        .run();
}
