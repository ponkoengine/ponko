//! Baked lighting: a room lit through an opening in its roof, its bounced
//! light baked ahead of time into lightmaps and a probe volume.
//!
//!   `cargo run --release --features 3d --example bake_lighting -- bake room.lit`
//!     bakes the room's light into `room.lit` without opening a window, and
//!     prints how long it took and how big it is;
//!   `cargo run --release --features 3d --example bake_lighting -- room.lit`
//!     shows the room lit by it (without a file: by the ambient light only).
//!
//! The ball rolling around the room has no lightmap: it takes its light
//! from the probe volume. Escape quits.

use ponko::d3::Transform;
use ponko::prelude::*;
use ponko::render::{
    BakeModel, BakedLighting, LightBakeScene, LightBakeSettings, Lighting, MaterialDesc, MeshData, Model, Sky,
};

/// The room's boxes, `lo` to `hi`, and their colors.
const BOXES: [([f32; 3], [f32; 3], [f32; 3]); 10] = [
    ([-4.0, -0.4, -4.0], [4.0, 0.0, 4.0], [0.8, 0.8, 0.8]),   // floor
    ([-4.0, 0.0, -4.0], [-3.6, 3.0, 4.0], [0.85, 0.1, 0.08]), // west wall, red
    ([3.6, 0.0, -4.0], [4.0, 3.0, 4.0], [0.1, 0.35, 0.8]),    // east wall, blue
    ([-3.6, 0.0, -4.0], [3.6, 3.0, -3.6], [0.8, 0.8, 0.8]),   // north wall
    ([-3.6, 0.0, 3.6], [3.6, 3.0, 4.0], [0.8, 0.8, 0.8]),     // south wall
    ([-4.0, 3.0, -4.0], [-1.0, 3.4, 4.0], [0.8, 0.8, 0.8]),   // roof ...
    ([1.0, 3.0, -4.0], [4.0, 3.4, 4.0], [0.8, 0.8, 0.8]),     //
    ([-1.0, 3.0, -4.0], [1.0, 3.4, -1.0], [0.8, 0.8, 0.8]),   //
    ([-1.0, 3.0, 1.0], [1.0, 3.4, 4.0], [0.8, 0.8, 0.8]),     // ... with an opening
    ([-1.5, 0.0, -1.0], [-0.5, 1.2, 0.0], [0.9, 0.75, 0.3]),  // a crate
];

/// Lightmap texels per world unit.
const TEXELS_PER_UNIT: f32 = 8.0;

/// The box `i`: its mesh (with lightmap coordinates), where it stands and
/// what it's made of.
fn room_box(i: usize) -> (MeshData, Vec3, MaterialDesc) {
    let (lo, hi, c) = BOXES[i];
    let (lo, hi) = (Vec3::from(lo), Vec3::from(hi));
    let mut mesh = MeshData::cube(1.0);
    for p in &mut mesh.positions {
        *p *= hi - lo;
    }
    mesh.generate_lightmap_uvs(TEXELS_PER_UNIT);
    let material = MaterialDesc::color(Color::rgb(c[0], c[1], c[2])).with_roughness(0.9);
    (mesh, (lo + hi) * 0.5, material)
}

/// The sun, low in the sky, shining through the opening onto the red wall.
fn light(lighting: &mut Lighting, sky: &mut Sky) {
    lighting.sun_direction = vec3(-1.0, -1.4, 0.3);
    lighting.sun_intensity = 5.0;
    lighting.ambient_intensity = 0.6;
    sky.atmosphere = false;
    sky.sky_color = Color::rgb(0.45, 0.6, 0.9);
    sky.ground_color = Color::rgb(0.35, 0.3, 0.25);
}

/// Bakes the room's light into `path`. No window, no GPU.
fn bake(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (mut lighting, mut sky) = (Lighting::default(), Sky::default());
    light(&mut lighting, &mut sky);
    let mut scene = LightBakeScene::new(&lighting, &sky);
    for i in 0..BOXES.len() {
        let (mesh, at, material) = room_box(i);
        scene.add(BakeModel::new(&mesh, Mat4::from_translation(at)).with_material(&material));
    }
    let settings = LightBakeSettings::default()
        .with_texels_per_unit(TEXELS_PER_UNIT)
        .with_probe_spacing(1.0);
    let baked = ponko::render::bake_lighting(&scene, &settings)?;
    std::fs::write(path, baked.to_bytes())?;
    let stats = baked.stats();
    println!(
        "baked {path}: {:.2} s, {} texels on {} page(s) of {}², {} probes, {:.1} M rays, {} KiB on the GPU",
        stats.seconds,
        stats.texels,
        baked.pages(),
        baked.page_size(),
        stats.probes,
        stats.rays as f64 / 1e6,
        baked.gpu_bytes() / 1024
    );
    Ok(())
}

/// The ball that rolls around the room, lit by the probes.
#[derive(Component)]
struct Ball;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [command, path] = args.as_slice()
        && command == "bake"
    {
        return bake(path);
    }
    // A bake made for this room, checked as it's read.
    let baked = match args.first() {
        Some(path) => Some(BakedLighting::from_bytes(&std::fs::read(path)?)?),
        None => None,
    };
    Game::new("Ponko: baked lighting", 1280, 720)
        .setup(move |ctx| {
            light(&mut ctx.scene3d.lighting, &mut ctx.scene3d.sky);
            for i in 0..BOXES.len() {
                let (data, at, desc) = room_box(i);
                let material = ctx.gfx.create_material(desc);
                let mesh = ctx.gfx.create_mesh(&data, material);
                let entity = ctx.world.spawn((Transform::at(at), Model::new(mesh)));
                // Entry i of the bake is box i: it reads its own lightmap.
                if let Some(lightmap) = baked.as_ref().and_then(|b| b.lightmap(i)) {
                    ctx.world.insert(entity, lightmap);
                }
            }
            // Fails only on a GPU whose texture arrays can't hold the bake's
            // pages: the room is lit by its ambient light then.
            if let Err(e) = ctx.gfx.set_baked_lighting(baked.as_ref()) {
                eprintln!("{e}");
            }
            let ball = ctx
                .gfx
                .create_material(MaterialDesc::color(Color::WHITE).with_roughness(0.4));
            let ball = ctx.gfx.create_mesh(&MeshData::sphere(0.4, 24), ball);
            ctx.world
                .spawn((Transform::at(vec3(0.0, 0.4, 0.0)), Model::new(ball), Ball));
            ctx.scene3d.camera.position = vec3(2.8, 1.6, 2.8);
            ctx.scene3d.camera.look_at(vec3(-1.0, 0.8, -0.5));
        })
        .system(|ctx: &mut Ctx| {
            if ctx.input.key_pressed(Key::Escape) {
                ctx.quit();
            }
            let t = ctx.time.elapsed() as f32;
            ctx.world.query::<(&mut Transform, &Ball)>().for_each(|(transform, _)| {
                transform.position = vec3(t.cos() * 2.5, 0.4, t.sin() * 2.5);
            });
        })
        .run();
    Ok(())
}
