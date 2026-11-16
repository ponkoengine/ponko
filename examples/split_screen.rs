//! Split screen: two players drive a box each across a field, each seen by
//! their own camera in their half of the window, with a minimap in the top
//! middle that shows only icons the players' cameras leave out. A label
//! sits in each player's half, and a click names the half it hit and the
//! ground point under it.
//!
//!   `cargo run --release --example split_screen --features 3d`
//!
//! - WASD: player one.  Arrow keys: player two.
//! - Tab: four players (two more, standing still) and back.
//! - Click: the ground under the mouse, through the camera that draws there.
//! - Escape: quit.
use ponko::camera::{RenderLayers, Viewport};
use ponko::d3::Transform;
use ponko::prelude::*;
use ponko::render::{MaterialDesc, MeshData, Model};

/// The layer only the minimap draws.
const MAP_ICONS: u32 = 1;
/// Driving speed, meters a second.
const SPEED: f32 = 8.0;

/// The players' boxes and their minimap icons.
struct Players {
    boxes: Vec<Entity>,
    icons: Vec<Entity>,
    /// Whether four players are on screen (else two).
    four: bool,
    /// The last click: which camera drew there, and the ground under it.
    clicked: Option<(String, Vec3)>,
}

/// A copy of `base` behind and above `target`, drawing into `rect`.
fn chase(base: d3::Camera, target: Vec3, rect: Viewport) -> d3::Camera {
    let mut camera = base;
    camera.position = target + vec3(0.0, 6.0, 9.0);
    camera.look_at(target);
    camera.rect = rect;
    camera.layers = RenderLayers::ALL.without(MAP_ICONS);
    camera
}

/// The minimap: straight down over the field, every layer, cheap post.
fn minimap(base: d3::Camera) -> d3::View {
    let mut camera = base;
    camera.position = vec3(0.0, 70.0, 0.0);
    camera.rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    camera.rect = Viewport::new(0.4, 0.0, 0.2, 0.2 * 16.0 / 9.0);
    camera.layers = RenderLayers::ALL;
    camera.far = 200.0;
    let mut post = ponko::render::Post::default();
    post.bloom = 0.0;
    post.ambient_occlusion.enabled = false;
    d3::View::new(camera).with_post(post).with_clear(Color::hex(0x1B2430))
}

fn drive(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Escape) {
        ctx.quit();
    }
    let dt = ctx.time.dt();
    let keys = [
        [Key::W, Key::S, Key::A, Key::D],
        [Key::Up, Key::Down, Key::Left, Key::Right],
    ];
    let mut moves = [Vec3::ZERO; 2];
    for (m, [up, down, left, right]) in moves.iter_mut().zip(keys) {
        let axis =
            |plus: Key, minus: Key| ctx.input.key_down(plus) as i32 as f32 - ctx.input.key_down(minus) as i32 as f32;
        *m = vec3(axis(right, left), 0.0, axis(down, up)) * SPEED * dt;
    }
    if ctx.input.key_pressed(Key::Tab) {
        let players = ctx.world.resource_mut::<Players>();
        players.four = !players.four;
        layout(ctx);
    }
    let players = ctx.world.resource::<Players>();
    let count = if players.four { 4 } else { 2 };
    let (boxes, icons) = (players.boxes.clone(), players.icons.clone());
    for i in 0..count {
        let step = moves.get(i).copied().unwrap_or(Vec3::ZERO);
        let Some(t) = ctx.world.get_mut::<Transform>(boxes[i]) else {
            continue;
        };
        t.position += step;
        let at = t.position;
        if let Some(t) = ctx.world.get_mut::<Transform>(icons[i]) {
            t.position = at + vec3(0.0, 2.0, 0.0);
        }
        // Player one is the main camera, the others views 0..; the
        // minimap is the last view.
        let camera = if i == 0 {
            &mut ctx.scene3d.camera
        } else {
            &mut ctx.scene3d.views[i - 1].camera
        };
        *camera = chase(*camera, at, camera.rect);
    }
}

/// Gives each player a part of the window: halves for two, quarters for
/// four, the minimap on top.
fn layout(ctx: &mut Ctx) {
    let players = ctx.world.resource::<Players>();
    let count = if players.four { 4 } else { 2 };
    let boxes = players.boxes.clone();
    let at = |ctx: &Ctx, i: usize| ctx.world.get::<Transform>(boxes[i]).map_or(Vec3::ZERO, |t| t.position);
    let base = ctx.scene3d.camera;
    ctx.scene3d.camera = chase(base, at(ctx, 0), Viewport::split(count, 0));
    ctx.scene3d.views.clear();
    for i in 1..count as usize {
        let camera = chase(base, at(ctx, i), Viewport::split(count, i as u32));
        ctx.scene3d.views.push(d3::View::new(camera));
    }
    ctx.scene3d.views.push(minimap(base));
}

/// A label in each player's part of the window, and the last click.
fn hud(ctx: &mut Ctx) {
    let mut rects = vec![ctx.scene3d.camera.screen_rect()];
    let views = ctx.scene3d.views.len().saturating_sub(1);
    rects.extend(ctx.scene3d.views[..views].iter().map(|v| v.camera.screen_rect()));
    for (i, rect) in rects.iter().enumerate() {
        let at = vec2(rect.min.x + 12.0, rect.max.y - 32.0);
        ctx.gfx.draw_text(&format!("Player {}", i + 1), at, 22.0, Color::WHITE);
    }
    if ctx.input.mouse_pressed(MouseButton::Left) {
        let mouse = ctx.input.mouse_pos();
        let camera = ctx.scene3d.camera_at(mouse);
        let name = if camera.layers == RenderLayers::ALL {
            "the minimap".to_string()
        } else {
            let i = rects.iter().position(|r| r.contains(mouse)).unwrap_or(0);
            format!("player {}", i + 1)
        };
        let ground = camera.screen_to_ground(mouse, 0.0);
        ctx.world.resource_mut::<Players>().clicked = ground.map(|g| (name, g));
    }
    if let Some((name, ground)) = &ctx.world.resource::<Players>().clicked {
        let text = format!("clicked {name}'s view at ({:.1}, {:.1})", ground.x, ground.z);
        ctx.gfx.draw_text(&text, vec2(12.0, 12.0), 18.0, Color::WHITE);
    }
}

fn setup(ctx: &mut Ctx) {
    ctx.scene3d.lighting.sun_direction = vec3(-0.5, -1.0, -0.35);
    let grass = ctx
        .gfx
        .create_material(MaterialDesc::color(Color::hex(0x5E8C4A)).with_roughness(0.9));
    let field = ctx.gfx.create_mesh(&MeshData::plane(120.0), grass);
    ctx.world.spawn((Transform::default(), Model::new(field)));
    let rock = ctx.gfx.create_material(MaterialDesc::color(Color::hex(0x8A8178)));
    let pillar = ctx.gfx.create_mesh(&MeshData::cube(1.0), rock);
    for i in 0..24 {
        let angle = i as f32 * 0.9;
        let mut t = Transform::at(vec3(
            angle.cos() * (10.0 + i as f32 * 1.5),
            1.5,
            angle.sin() * (10.0 + i as f32 * 1.5),
        ));
        t.scale = vec3(1.5, 3.0, 1.5);
        ctx.world.spawn((t, Model::new(pillar)));
    }

    let colors = [0xE04040, 0x40A0E0, 0xE0C040, 0xA060E0];
    let mut boxes = Vec::new();
    let mut icons = Vec::new();
    for (i, color) in colors.into_iter().enumerate() {
        let paint = ctx.gfx.create_material(MaterialDesc::color(Color::hex(color)));
        let body = ctx.gfx.create_mesh(&MeshData::cube(1.0), paint);
        let at = vec3(i as f32 * 4.0 - 6.0, 0.5, 0.0);
        boxes.push(ctx.world.spawn((Transform::at(at), Model::new(body))));
        // A big flat marker over the box, on the minimap's layer only.
        let mut glow = MaterialDesc::color(Color::hex(color));
        glow.emissive = Color::hex(color);
        let glow = ctx.gfx.create_material(glow);
        let marker = ctx.gfx.create_mesh(&MeshData::cube(1.0), glow);
        let mut t = Transform::at(at + vec3(0.0, 2.0, 0.0));
        t.scale = vec3(4.0, 0.2, 4.0);
        icons.push(ctx.world.spawn((t, Model::new(marker), RenderLayers::layer(MAP_ICONS))));
    }
    ctx.world.insert_resource(Players {
        boxes,
        icons,
        four: false,
        clicked: None,
    });
    layout(ctx);
}

fn main() {
    Game::new("Ponko: split screen", 1280, 720)
        .system(drive)
        .system(hud)
        .setup(setup)
        .run();
}
