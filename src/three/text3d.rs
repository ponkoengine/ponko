//! Text in the 3D world: signs, labels, name tags and damage numbers, sharp
//! at any size and distance. Glyphs are signed distance fields made once
//! per font and character, so one atlas serves every size, and outlines
//! and soft edges come from the same glyphs.

use glam::{Vec2, Vec3};

use super::{Camera3D, Transform3D, linear};
use crate::text::{SDF_PX, SDF_SPREAD, Text};
use crate::{color::Color, ctx::Ctx, text::Font, texture::Texture};
use ponko_ecs::Component;

/// How the lines of a [`Text3D`] line up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum TextAlign3D {
    /// Along the block's left edge.
    Left = 0,
    /// Each line centered. The default.
    #[default]
    Center = 1,
    /// Along the block's right edge.
    Right = 2,
}

/// Text in the world, drawn on its entity's [`d3::Transform`](crate::d3::Transform): a shop sign,
/// a name over a head, a damage number. It stays sharp at any size and
/// distance, and can have an outline and a soft edge (for glows and
/// shadows). Any loaded [`Font`] works, and one entity's text can't mix
/// fonts; use several entities for that.
///
/// The text block is centered on the entity, in its XY plane, read from
/// +Z (like [`UiPanel3D`](crate::d3::UiPanel)), and scaled by its transform.
/// With `billboard` it turns to face the camera instead, keeping upright.
/// It's see-through where there's no text and is drawn with the particles,
/// after solid models; it casts no shadow.
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let label = ponko::d3::Text {
///         size: 0.3,
///         outline: Color::BLACK,
///         outline_width: 0.06,
///         billboard: true,
///         ..ponko::d3::Text::new("Shop")
///     };
///     ctx.world.spawn((ponko::d3::Transform::at(vec3(0.0, 2.5, 0.0)), label));
/// });
/// ```
#[derive(Debug, Clone, PartialEq, Component)]
pub struct Text3D {
    /// What it says; `\n` starts a new line.
    pub text: String,
    /// Default [`Font::DEFAULT`].
    pub font: Font,
    /// Height of a line, in meters. Default 0.25.
    pub size: f32,
    /// Default white.
    pub color: Color,
    /// The outline's color. Default transparent (no outline).
    pub outline: Color,
    /// How far the outline reaches out from the letters, in lines (0.05 is
    /// a twentieth of a line); at most 0.12. Default 0.
    pub outline_width: f32,
    /// How soft the edges are, in lines: 0 is sharp; around 0.1 makes a
    /// glow or shadow (use a soft copy behind sharp text). At most 0.12.
    /// Default 0.
    pub softness: f32,
    /// Default [`TextAlign3D::Center`].
    pub align: TextAlign3D,
    /// Turn to face the camera (upright). Default false.
    pub billboard: bool,
}

impl Text3D {
    /// `text` in the default font, a quarter meter per line, white.
    /// ```
    /// # use ponko::prelude::*;
    /// let sign = ponko::d3::Text::new("OPEN");
    /// assert_eq!(sign.size, 0.25);
    /// ```
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font: Font::DEFAULT,
            size: 0.25,
            color: Color::WHITE,
            outline: Color::TRANSPARENT,
            outline_width: 0.0,
            softness: 0.0,
            align: TextAlign3D::Center,
            billboard: false,
        }
    }
}

/// Most a [`Text3D`]'s outline or softness reaches, in lines: as far as the
/// glyphs record distances.
pub(crate) const MAX_REACH: f32 = (SDF_SPREAD as f32 - 0.5) / SDF_PX;

/// How much nearer the camera a depth-tested debug label is drawn than its
/// point, as a share of the distance: a label on a surface stays readable.
const LABEL_PULL: f32 = 0.005;

/// One corner of a glyph quad.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub outline: [f32; 4],
    /// The outline's reach and the softness, in the atlas' distance units
    /// (0.5 is the full spread).
    pub params: [f32; 2],
}

/// This frame's 3D text: glyph quads (a triangle list), farthest text
/// first, and the atlas they sample.
#[derive(Default)]
pub(crate) struct TextFrame {
    pub vertices: Vec<TextVertex>,
    /// How many of the last `vertices` are debug labels on top (target
    /// cameras only), drawn after everything else without a depth test.
    pub on_top: usize,
    pub atlas: Option<Texture>,
    /// Scratch: (depth, entity) of each text to draw.
    order: Vec<(f32, ponko_ecs::Entity)>,
}

impl TextFrame {
    /// Adds `t`'s glyph quads placed by `at`, laid out in `text` (whose
    /// SDF atlas is `ATLAS` pixels wide).
    pub(crate) fn push(&mut self, t: &Text3D, at: &Transform3D, camera: &Camera3D, text: &mut Text) {
        self.push_str(&t.text, t, at, camera, text, false);
    }

    /// A depth-tested debug label (`ctx.debug.text(..).depth_tested()`):
    /// `string` facing the camera, `pixels` high at its distance, its
    /// top-left corner at `at`, pulled a little toward the camera so a label
    /// on a surface isn't cut by it. Nothing while `at` is behind the near plane.
    pub(crate) fn push_label(
        &mut self,
        string: &str,
        at: Vec3,
        (color, pixels): (Color, f32),
        camera: &Camera3D,
        text: &mut Text,
    ) {
        let depth = (at - camera.position).dot(camera.forward());
        let height = camera.viewport().y;
        if !(depth > camera.near && height > 0.0) || string.is_empty() {
            return;
        }
        let at = camera.position + (at - camera.position) * (1.0 - LABEL_PULL);
        let depth = depth * (1.0 - LABEL_PULL);
        let meters_per_pixel = 2.0 * depth * (camera.fov_y * 0.5).tan() / height;
        let style = Text3D {
            text: String::new(),
            size: pixels * meters_per_pixel,
            color,
            align: TextAlign3D::Left,
            billboard: true,
            ..Text3D::new(String::new())
        };
        self.push_str(string, &style, &Transform3D::at(at), camera, text, true);
    }

    /// `string` placed and styled by `t` (its own text unused); with
    /// `top_left` the block's top-left corner is at the transform's
    /// position instead of its center.
    fn push_str(
        &mut self,
        string: &str,
        t: &Text3D,
        at: &Transform3D,
        camera: &Camera3D,
        text: &mut Text,
        top_left: bool,
    ) {
        let block = text.layout_sdf(string, t.font);
        let side = Text::ATLAS_SIZE as f32;
        let meters = t.size.max(0.0) / SDF_PX;
        let widths = text.line_widths();
        let place = |local: Vec2| -> Vec3 {
            let local = local * meters;
            if t.billboard {
                let (right, up) = (camera.rotation * Vec3::X, camera.rotation * Vec3::Y);
                at.position + (right * local.x * at.scale.x) + (up * local.y * at.scale.y)
            } else {
                at.matrix().transform_point3(local.extend(0.0))
            }
        };
        let color = premultiplied(t.color);
        let outline = premultiplied(t.outline);
        let units = |lines: f32| lines.clamp(0.0, MAX_REACH) * SDF_PX / (2.0 * SDF_SPREAD as f32);
        let params = [units(t.outline_width), units(t.softness)];
        for p in text.placed() {
            let Some(atlas_at) = p.glyph.at else { continue };
            let size = Vec2::new(p.glyph.size[0] as f32, p.glyph.size[1] as f32);
            let line = widths.get(p.line as usize).copied().unwrap_or(0.0);
            let shift = match t.align {
                TextAlign3D::Left => -block.x / 2.0,
                TextAlign3D::Center => -line / 2.0,
                TextAlign3D::Right => block.x / 2.0 - line,
            };
            // Pixels (y down from the block's top) to the block's plane
            // (y up from its center).
            let anchor = if top_left {
                Vec2::new(block.x / 2.0, -block.y / 2.0)
            } else {
                Vec2::ZERO
            };
            let corner = |dx: f32, dy: f32| {
                anchor + Vec2::new(p.pos.x + dx * size.x + shift, block.y / 2.0 - (p.pos.y + dy * size.y))
            };
            let uv0 = Vec2::new(atlas_at[0] as f32, atlas_at[1] as f32) / side;
            let uv = |dx: f32, dy: f32| (uv0 + Vec2::new(dx, dy) * size / side).to_array();
            let v = |dx: f32, dy: f32| TextVertex {
                position: place(corner(dx, dy)).to_array(),
                uv: uv(dx, dy),
                color,
                outline,
                params,
            };
            let (tl, tr, br, bl) = (v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 1.0));
            self.vertices.extend([tl, bl, br, tl, br, tr]);
        }
    }
}

fn premultiplied(c: Color) -> [f32; 4] {
    let [r, g, b, a] = linear(c);
    [r * a, g * a, b * a, a]
}

impl Ctx {
    /// Lays out this frame's [`Text3D`]s and `ctx.debug`'s depth-tested
    /// labels into glyph quads (farthest first) for every 3D camera, each
    /// facing it: the main camera, each window view
    /// ([`SceneSettings::views`](crate::render::SceneSettings::views)) and
    /// each target camera that draws particles. Labels go to the cameras
    /// that show the default layer. New glyphs go on the GPU once.
    pub(crate) fn collect_text3d(&mut self) {
        let frame3d = &mut self.gfx.frame3d;
        frame3d.particles.texts.clear();
        for frame in &mut frame3d.window_views {
            frame.particles.texts.clear();
        }
        for view in &mut frame3d.views {
            view.frame.particles.texts.clear();
        }
        let labels = !frame3d.debug_lines.labels.is_empty();
        let texts = self
            .world
            .query::<&Text3D>()
            .iter()
            .any(|t| !t.text.is_empty() && t.size > 0.0);
        if !texts && !labels {
            return;
        }
        let atlas = match frame3d.particles.texts.atlas {
            Some(t) => t,
            None => {
                let side = Text::ATLAS_SIZE;
                let t = self.gfx.make_texture(side, side, &vec![0; (side * side * 4) as usize]);
                self.gfx.frame3d.particles.texts.atlas = Some(t);
                t
            }
        };
        let (frame3d, text, world) = (&mut self.gfx.frame3d, &mut self.gfx.text, &mut self.world);
        let lines = &frame3d.debug_lines;
        let main = &self.scene3d.camera;
        let labels_for = |on: bool| (labels && on).then_some(lines);
        // The window's cameras draw the labels on top as screen text.
        frame3d
            .particles
            .texts
            .lay_out(world, main, (labels_for(!lines.hidden), false), true, text, atlas);
        for (frame, view) in frame3d.window_views.iter_mut().zip(&self.scene3d.views) {
            let labels = labels_for(view.camera.draws_debug(false));
            frame
                .particles
                .texts
                .lay_out(world, &view.camera, (labels, false), true, text, atlas);
        }
        for view in &mut frame3d.views {
            // Its 3D text is drawn with its particles; its labels with its
            // debug drawing, which has a flag of its own.
            if let Some(eye) = view.eye.filter(|_| view.particles || view.debug) {
                let labels = labels_for(view.debug);
                view.frame
                    .particles
                    .texts
                    .lay_out(world, &eye, (labels, true), view.particles, text, atlas);
            }
        }
        if let Some(renderer) = self.gfx.renderer.as_mut() {
            for (at, size, rgba) in self.gfx.text.sdf.uploads.drain(..) {
                renderer.update_texture_region(atlas.id, at, size, &rgba);
            }
        } else {
            self.gfx.text.sdf.uploads.clear();
        }
    }
}

impl TextFrame {
    /// Nothing to draw (keeping the storage and the atlas).
    pub(crate) fn clear(&mut self) {
        self.vertices.clear();
        self.on_top = 0;
    }

    /// Lays out every [`Text3D`] in `world` for `camera` (with `texts`),
    /// farthest first, then `labels`' depth-tested labels and, with
    /// `on_top`, its labels on top last (the last [`on_top`](Self::on_top)
    /// vertices), sampling `atlas`.
    fn lay_out(
        &mut self,
        world: &mut ponko_ecs::World,
        camera: &Camera3D,
        (labels, on_top): (Option<&crate::debug::GpuLines>, bool),
        texts: bool,
        text: &mut Text,
        atlas: Texture,
    ) {
        self.vertices.clear();
        self.atlas = Some(atlas);
        let mut order = std::mem::take(&mut self.order);
        order.clear();
        let (eye, forward) = (camera.position, camera.forward());
        world
            .query::<(ponko_ecs::Entity, &Text3D, &Transform3D)>()
            .for_each(|(e, t, at)| {
                if texts && !t.text.is_empty() && t.size > 0.0 {
                    order.push(((at.position - eye).dot(forward), e));
                }
            });
        order.sort_by(|a, b| b.0.total_cmp(&a.0));
        for &(_, e) in &order {
            let (Some(t), Some(at)) = (world.get::<Text3D>(e), world.get::<Transform3D>(e)) else {
                continue;
            };
            self.push(t, at, camera, text);
        }
        self.order = order;
        if let Some(lines) = labels {
            for l in lines.labels.iter().filter(|l| !l.on_top) {
                let string = &lines.text[l.text.clone()];
                self.push_label(string, l.at, (l.color, l.size), camera, text);
            }
        }
        let below = self.vertices.len();
        if let Some(lines) = labels.filter(|_| on_top) {
            for l in lines.labels.iter().filter(|l| l.on_top) {
                let string = &lines.text[l.text.clone()];
                self.push_label(string, l.at, (l.color, l.size), camera, text);
            }
        }
        self.on_top = self.vertices.len() - below;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn quads(t: &Text3D, at: &Transform3D) -> Vec<TextVertex> {
        let mut frame = TextFrame::default();
        let mut camera = Camera3D::new(Vec2::new(800.0, 600.0));
        // Level, looking down -Z.
        camera.rotation = Quat::IDENTITY;
        frame.push(t, at, &camera, &mut Text::new());
        frame.vertices
    }

    fn bounds(v: &[TextVertex]) -> (Vec3, Vec3) {
        v.iter()
            .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), v| {
                let p = Vec3::from_array(v.position);
                (lo.min(p), hi.max(p))
            })
    }

    #[test]
    fn text_is_centered_on_its_entity_and_sized_in_meters() {
        let t = Text3D::new("Hello");
        let at = Transform3D::at(Vec3::new(1.0, 2.0, 3.0));
        let v = quads(&t, &at);
        assert_eq!(v.len(), 5 * 6, "a quad per letter");
        let (lo, hi) = bounds(&v);
        // Centered (glyph padding is symmetric give or take a pixel).
        let center = (lo + hi) / 2.0;
        assert!(
            (center.x - 1.0).abs() < 0.05 && (center.y - 2.0).abs() < 0.08,
            "{center}"
        );
        assert!(lo.z == 3.0 && hi.z == 3.0, "flat in its XY plane");
        // A line is about `size` tall (plus the distance padding).
        let pad = 2.0 * SDF_SPREAD as f32 / SDF_PX * t.size;
        assert!(hi.y - lo.y < t.size + pad, "{}", hi.y - lo.y);
        // Twice the size, twice as big.
        let (lo2, hi2) = bounds(&quads(&Text3D { size: 0.5, ..t.clone() }, &at));
        assert!(((hi2.x - lo2.x) / (hi.x - lo.x) - 2.0).abs() < 1e-3);
    }

    #[test]
    fn lines_line_up_as_asked() {
        let at = Transform3D::default();
        let text = "a\nwide line";
        let left_of_first = |align| {
            let v = quads(
                &Text3D {
                    align,
                    ..Text3D::new(text)
                },
                &at,
            );
            // The first letter's quad comes first.
            v[..6].iter().map(|v| v.position[0]).fold(f32::MAX, f32::min)
        };
        let (l, c, r) = (
            left_of_first(TextAlign3D::Left),
            left_of_first(TextAlign3D::Center),
            left_of_first(TextAlign3D::Right),
        );
        assert!(l < c && c < r, "{l} {c} {r}");
        // The widest line is the same whatever the alignment.
        let wide = |align| {
            bounds(&quads(
                &Text3D {
                    align,
                    ..Text3D::new(text)
                },
                &at,
            ))
        };
        assert!((wide(TextAlign3D::Left).0.x - wide(TextAlign3D::Right).0.x).abs() < 1e-4);
    }

    #[test]
    fn turned_text_follows_its_transform_and_billboards_face_the_camera() {
        let turned = Transform3D::default().rotated(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let (lo, hi) = bounds(&quads(&Text3D::new("Hi"), &turned));
        assert!(hi.x - lo.x < 1e-4, "turned to face +X, it's flat in YZ");
        assert!(hi.z - lo.z > 0.05);
        // The camera looks down -Z: a billboard is flat in XY,
        // whatever its entity's rotation.
        let t = Text3D {
            billboard: true,
            ..Text3D::new("Hi")
        };
        let (lo, hi) = bounds(&quads(&t, &turned));
        assert!(hi.z - lo.z < 1e-4 && hi.x - lo.x > 0.05);
    }

    #[test]
    fn outlines_and_softness_are_kept_within_the_glyphs_reach() {
        let t = Text3D {
            outline: Color::BLACK,
            outline_width: 1.0,
            softness: 0.05,
            ..Text3D::new("x")
        };
        let v = quads(&t, &Transform3D::default());
        let [outline, soft] = v[0].params;
        assert!(outline < 0.5 && outline > 0.4, "clamped below the spread: {outline}");
        assert!((soft - 0.05 * SDF_PX / (2.0 * SDF_SPREAD as f32)).abs() < 1e-6);
        assert_eq!(v[0].outline, [0.0, 0.0, 0.0, 1.0]);
        // Spaces take room but draw nothing; empty text draws nothing.
        assert_eq!(quads(&Text3D::new("a b"), &Transform3D::default()).len(), 12);
        assert!(quads(&Text3D::new(""), &Transform3D::default()).is_empty());
    }

    /// The screen rectangle (min, max, in pixels) of `camera`'s view of quads.
    fn on_screen(v: &[TextVertex], camera: &Camera3D) -> (Vec2, Vec2) {
        v.iter()
            .fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), v| {
                let p = camera.world_to_screen(Vec3::from_array(v.position)).expect("in front");
                (lo.min(p), hi.max(p))
            })
    }

    #[test]
    fn a_debug_label_is_its_size_in_pixels_at_any_distance_with_its_corner_at_its_point() {
        let mut camera = Camera3D::new(Vec2::new(800.0, 600.0));
        camera.position = Vec3::ZERO;
        camera.rotation = Quat::IDENTITY;
        let (size, pad) = (20.0, 2.0 * SDF_SPREAD as f32 / SDF_PX * 20.0);
        let mut heights = Vec::new();
        for (at, depth) in [(Vec3::new(0.0, 0.0, -10.0), 10.0), (Vec3::new(4.0, 2.0, -40.0), 40.0)] {
            let mut frame = TextFrame::default();
            frame.push_label("Hi", at, (Color::WHITE, size), &camera, &mut Text::new());
            assert_eq!(frame.vertices.len(), 2 * 6, "a quad per letter");
            // Facing the camera: flat in its view plane, a little nearer than the point.
            for v in &frame.vertices {
                let z = -v.position[2];
                assert!(z < depth && z > depth * 0.99, "{z} vs {depth}");
            }
            let (lo, hi) = on_screen(&frame.vertices, &camera);
            let corner = camera.world_to_screen(at).expect("in front");
            assert!(
                (lo - corner).abs().max_element() <= pad,
                "top-left corner {lo} at the point {corner}"
            );
            let tall = hi.y - lo.y;
            assert!(tall > size * 0.5 && tall < size + 2.0 * pad, "{tall} px for {size} px");
            heights.push(tall);
        }
        assert!(
            (heights[0] - heights[1]).abs() < 0.5,
            "the same at any distance: {heights:?}"
        );
        // Behind the camera, or empty: nothing.
        let mut frame = TextFrame::default();
        frame.push_label(
            "Hi",
            Vec3::new(0.0, 0.0, 5.0),
            (Color::WHITE, size),
            &camera,
            &mut Text::new(),
        );
        frame.push_label(
            "",
            Vec3::new(0.0, 0.0, -5.0),
            (Color::WHITE, size),
            &camera,
            &mut Text::new(),
        );
        assert!(frame.vertices.is_empty());
    }

    #[test]
    fn depth_tested_debug_labels_become_3d_text() {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(800.0, 600.0));
        // No GPU: screen text (the label on top) is dropped, as in a headless game.
        ctx.gfx.headless = true;
        ctx.scene3d.camera.position = Vec3::new(0.0, 0.0, 5.0);
        ctx.scene3d.camera.rotation = Quat::IDENTITY;
        ctx.debug.set_enabled(true);
        ctx.debug.text(Vec3::ZERO, "tested", Color::WHITE).depth_tested();
        ctx.debug.text(Vec3::ZERO, "on top", Color::WHITE);
        ctx.gfx.frame3d.debug_lines.clear();
        ctx.hand_over_debug_draw();
        let lines = &ctx.gfx.frame3d.debug_lines;
        assert_eq!(lines.labels.len(), 1, "only the depth-tested one goes to the 3D frame");
        assert_eq!(&lines.text[lines.labels[0].text.clone()], "tested");
        // The atlas a running game makes on the GPU.
        ctx.gfx.frame3d.particles.texts.atlas = Some(Texture::new(0, Text::ATLAS_SIZE, Text::ATLAS_SIZE));
        ctx.collect_text3d();
        assert_eq!(
            ctx.gfx.frame3d.particles.texts.vertices.len(),
            6 * 6,
            "a quad per letter"
        );
    }

    #[test]
    fn a_target_camera_lays_out_debug_labels_by_its_debug_flag_and_3d_text_by_its_particles() {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(800.0, 600.0));
        ctx.gfx.headless = true;
        ctx.scene3d.camera.position = Vec3::new(0.0, 0.0, 5.0);
        ctx.scene3d.camera.rotation = Quat::IDENTITY;
        ctx.world
            .spawn((Text3D::new("abc"), Transform3D::at(Vec3::new(1.0, 0.0, 0.0))));
        ctx.debug.set_enabled(true);
        ctx.debug.text(Vec3::ZERO, "tested", Color::WHITE).depth_tested();
        ctx.gfx.frame3d.debug_lines.clear();
        ctx.hand_over_debug_draw();
        ctx.gfx.frame3d.particles.texts.atlas = Some(Texture::new(0, Text::ATLAS_SIZE, Text::ATLAS_SIZE));
        let eye = ctx.scene3d.camera;
        // (particles, debug drawing): the label's six quads with debug
        // drawing, the text's three with particles.
        for ((particles, debug), quads) in [
            ((true, true), 9),
            ((false, true), 6),
            ((true, false), 3),
            ((false, false), 0),
        ] {
            ctx.gfx.frame3d.views = vec![crate::three::collect::ViewFrame {
                particles,
                debug,
                eye: Some(eye),
                ..Default::default()
            }];
            ctx.collect_text3d();
            assert_eq!(
                ctx.gfx.frame3d.views[0].frame.particles.texts.vertices.len(),
                quads * 6,
                "particles {particles}, debug drawing {debug}"
            );
        }
    }
}
