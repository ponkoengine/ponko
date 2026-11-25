//! Debug drawing: lines, shapes and text labels in the 2D world, the 3D
//! world or on the screen, from any system, through [`ctx.debug`](crate::ctx::Ctx::debug).
//!
//! **Maturity: tested**, from the guide chapter `debug-draw.md`. Game: none. Tests:
//! `tests/debug_draw.rs`, `debug_lines_render_test.rs`. Platforms: Linux, macOS and Windows,
//! headless (the workspace tests in the list of checks). Graphics: offscreen render tests, required
//! on Windows (DirectX 12 on the WARP software adapter) and run on Linux and macOS only where a GPU
//! is found. Missing: a reference game that uses it.
//!
//! ```
//! use ponko::prelude::*;
//! fn show(ctx: &mut Ctx) {
//!     // 2D: points are `Vec2`, in world units (or window pixels with `.screen()`).
//!     ctx.debug.line(vec2(0.0, 0.0), vec2(100.0, 50.0), Color::RED);
//!     ctx.debug.circle(vec2(200.0, 200.0), 40.0, Color::GREEN).seconds(2.0);
//!     ctx.debug.text(vec2(10.0, 10.0), "debug", Color::WHITE).screen();
//!     // 3D: points are `Vec3`, depth-tested unless `.on_top()`.
//!     ctx.debug.arrow(Vec3::ZERO, Vec3::Y * 2.0, Color::YELLOW).on_top();
//!     ctx.debug.sphere(vec3(0.0, 1.0, 0.0), 0.5, Color::BLUE);
//! }
//! ```
//!
//! # What a shape is, and how long it stays
//!
//! Every shape is a set of straight lines (a circle is 32 of them), plus
//! text for [`DebugDraw::text`]. A call returns a [`DebugShape`] whose
//! methods change what was just drawn: [`seconds`](DebugShape::seconds)
//! (how long it stays), [`on_top`](DebugShape::on_top) (not hidden by
//! nearer 3D surfaces), [`screen`](DebugShape::screen) (2D points are
//! window pixels) and [`width`](DebugShape::width) (in pixels).
//!
//! Without `seconds`, a shape stays until the code that drew it runs again:
//! one drawn by a simulation system stays until the next simulation tick
//! (so frames between ticks still show it), one drawn by a frame system for
//! one frame. With `seconds`, it stays that long (frame time, not paused by
//! [`Time::pause`](crate::time::Time::pause)), whoever drew it.
//!
//! # Batching and cost
//!
//! The shapes are kept in a few flat arrays: nothing is allocated per shape
//! once the arrays have grown, and text shares one string buffer. When the
//! frame is drawn, every 3D line goes to the GPU in one buffer and is drawn
//! with one draw call for the depth-tested lines and one for those on top,
//! inside the lit 3D frame (so exposure and tone mapping apply, as to an
//! unlit surface of that color); 2D lines become one run of sprites, which
//! the sprite pass draws as one batch. Labels are drawn as screen text
//! (in a target camera's render target, as 3D text facing it: see
//! [`DebugDraw::text`]).
//!
//! Turned off ([`set_enabled`](DebugDraw::set_enabled)), every call is one
//! branch and draws nothing. It is on by default in debug builds and off in
//! release builds; [`Game::debug_draw`](crate::game::Game::debug_draw)
//! picks for a game, and the control endpoint's `debug.draw` method turns
//! it and the built-in views on and off while the game runs.
//!
//! # Built-in views
//!
//! [`DebugDraw::show`] turns on what the engine draws by itself each frame:
//! physics colliders (2D and 3D), the navigation mesh, bounds, light
//! volumes and the transform hierarchy's axes. See [`DebugView`].
//!
//! # From view systems
//!
//! A system that takes a [`SystemView`](crate::system_view::SystemView)
//! draws through `sys.debug`, the same type. In a parallel batch each system
//! draws into a buffer of its own, added to `ctx.debug` when the batch ends,
//! so drawing never waits on a lock or races another thread.

#![deny(missing_docs)]

#[cfg(feature = "3d")]
use glam::Mat4;
use glam::{Quat, Vec2, Vec3};

use crate::color::Color;

mod builtin;
pub(crate) mod control;
mod flush;
mod toggles;
pub(crate) use toggles::MAX_TOGGLES;

#[cfg(test)]
mod tests;

pub use crate::draw_space::DrawSpace;
pub use builtin::DebugView;
#[cfg_attr(not(all(feature = "window", feature = "3d")), allow(unused_imports))]
pub(crate) use flush::{GpuLines, LineInstance};

/// Segments of a full circle.
const CIRCLE_SEGMENTS: usize = 32;
/// Default label height in pixels.
const LABEL_SIZE: f32 = 14.0;
/// Where an infinite far plane is drawn, in world units past the near plane.
#[cfg(feature = "3d")]
const INFINITE_FAR: f32 = 100.0;

/// Line flag: the points are 3D.
const THREE_D: u8 = 1;
/// Line flag: 2D points are window pixels.
const SCREEN: u8 = 2;
/// Line flag: not hidden by nearer 3D surfaces (2D: drawn over the UI).
const ON_TOP: u8 = 4;
/// Label flag: a 3D label hidden by nearer 3D surfaces, as lines are.
const TESTED: u8 = 8;
/// How far from the 3D camera the colliders view draws mesh triangles, by default.
const COLLIDER_DISTANCE: f32 = 50.0;

/// A point type a debug shape takes: [`Vec2`] for the 2D world or the
/// screen, [`Vec3`] for the 3D world. Implemented for those two only.
pub trait DebugPoint: Copy + sealed::Point {}

impl DebugPoint for Vec2 {}
impl DebugPoint for Vec3 {}

/// What the shapes need of a point, out of the public API.
mod sealed {
    use glam::{Vec2, Vec3};

    pub trait Point {
        /// The point as a 3D one (`z` 0 for a [`Vec2`]).
        fn to_vec3(self) -> Vec3;
        /// Whether the point is a 3D one.
        fn is_3d() -> bool;
        /// The plane flat shapes (circles, grids) lie in: the XY plane in
        /// 2D, the ground (XZ) plane in 3D.
        fn plane() -> (Vec3, Vec3);
    }

    impl Point for Vec2 {
        fn to_vec3(self) -> Vec3 {
            self.extend(0.0)
        }
        fn is_3d() -> bool {
            false
        }
        fn plane() -> (Vec3, Vec3) {
            (Vec3::X, Vec3::Y)
        }
    }

    impl Point for Vec3 {
        fn to_vec3(self) -> Vec3 {
            self
        }
        fn is_3d() -> bool {
            true
        }
        fn plane() -> (Vec3, Vec3) {
            (Vec3::X, Vec3::Z)
        }
    }
}

/// One drawn line, as the buffer keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DebugLine {
    pub a: Vec3,
    pub b: Vec3,
    /// sRGB, 8 bits a channel.
    pub color: [u8; 4],
    /// In pixels.
    pub width: f32,
    /// When a timed line goes, in seconds of frame time.
    pub until: f32,
    /// `THREE_D`, `SCREEN`, `ON_TOP`.
    pub flags: u8,
}

/// One drawn label: where it is and its text in the batch's string.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DebugLabel {
    pub at: Vec3,
    pub color: Color,
    pub size: f32,
    pub until: f32,
    pub flags: u8,
    pub text: std::ops::Range<usize>,
}

/// Lines and labels with the same lifetime. Cleared without freeing.
#[derive(Default, Clone, Debug)]
pub(crate) struct Batch {
    pub lines: Vec<DebugLine>,
    pub labels: Vec<DebugLabel>,
    pub text: String,
}

impl Batch {
    fn clear(&mut self) {
        self.lines.clear();
        self.labels.clear();
        self.text.clear();
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.labels.is_empty()
    }

    /// Moves everything from `lines` and `labels` on to `to`, timed to go at `until`.
    fn move_tail(&mut self, lines: usize, labels: usize, to: &mut Batch, until: f32) {
        to.lines
            .extend(self.lines.drain(lines..).map(|l| DebugLine { until, ..l }));
        for mut label in self.labels.drain(labels..) {
            let start = to.text.len();
            to.text.push_str(&self.text[label.text.clone()]);
            label.text = start..to.text.len();
            label.until = until;
            to.labels.push(label);
        }
        if self.labels.is_empty() {
            self.text.clear();
        }
    }

    /// Adds everything of `other`.
    fn append(&mut self, other: &Batch) {
        self.lines.extend_from_slice(&other.lines);
        let base = self.text.len();
        self.text.push_str(&other.text);
        self.labels.extend(other.labels.iter().map(|l| DebugLabel {
            text: l.text.start + base..l.text.end + base,
            ..l.clone()
        }));
    }

    /// Drops the timed entries that are gone at `now`.
    fn expire(&mut self, now: f32) {
        self.lines.retain(|l| l.until > now);
        self.labels.retain(|l| l.until > now);
        if self.labels.is_empty() {
            self.text.clear();
        }
    }
}

/// Which batch a shape went to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Frame,
    Tick,
    Timed,
}

/// How many lines and labels debug drawing shows now: see [`DebugDraw::stats`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[non_exhaustive]
pub struct DebugStats {
    /// Lines in the 2D world or on the screen.
    pub lines_2d: usize,
    /// Lines in the 3D world.
    pub lines_3d: usize,
    /// Text labels.
    pub labels: usize,
}

/// Debug drawing: `ctx.debug` (and `sys.debug` in view systems). See the
/// [module documentation](self).
///
/// ```
/// use ponko::prelude::*;
/// let mut lines = 0;
/// Game::new("debug", 64, 64).debug_draw(true).run_headless(1, |ctx| {
///     ctx.debug.aabb(Vec3::ZERO, Vec3::ONE, Color::GREEN).seconds(5.0);
///     lines = ctx.debug.stats().lines_3d;
/// });
/// assert_eq!(lines, 12); // a box is its 12 edges
/// ```
#[derive(Debug, Clone)]
pub struct DebugDraw {
    enabled: bool,
    /// Set while simulation systems run.
    in_sim: bool,
    /// Frame time at the start of this frame, for timed shapes.
    now: f32,
    /// The built-in views that are on.
    views: builtin::Views,
    /// The game's own toggles ([`Game::debug_toggle`](crate::game::Game::debug_toggle)).
    toggles: toggles::Toggles,
    /// How far from the 3D camera the colliders view draws mesh triangles.
    collider_distance: f32,
    /// What was drawn, boxed so `Ctx` stays small.
    buf: Box<Buffers>,
}

/// The buffers of a [`DebugDraw`]. Cleared, never freed: capacity is kept.
#[derive(Debug, Clone, Default)]
struct Buffers {
    /// From frame systems, without a lifetime: until the next frame.
    frame: Batch,
    /// From the running simulation tick.
    tick_live: Batch,
    /// From the last finished simulation tick: shown until the next one ends.
    tick_done: Batch,
    /// With a lifetime.
    timed: Batch,
    /// Scratch of the hierarchy view: entities and where they are.
    placed: Vec<(ponko_ecs::Entity, Vec3)>,
}

impl Default for DebugDraw {
    fn default() -> Self {
        Self::new(cfg!(debug_assertions))
    }
}

impl DebugDraw {
    /// A buffer that draws if `enabled`. The engine makes `ctx.debug`; a
    /// test or tool can make its own.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            in_sim: false,
            now: 0.0,
            views: builtin::Views::default(),
            toggles: toggles::Toggles::default(),
            collider_distance: COLLIDER_DISTANCE,
            buf: Box::default(),
        }
    }

    /// Whether drawing does anything.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Turns debug drawing on or off. Off, every drawing call returns at
    /// once and what was drawn is dropped (the built-in views stay chosen
    /// for when it is back on).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// if ctx.input.key_pressed(Key::F3) {
    ///     let on = ctx.debug.enabled();
    ///     ctx.debug.set_enabled(!on);
    /// }
    /// # }
    /// ```
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.clear();
        }
    }

    /// Drops everything drawn, timed shapes too.
    pub fn clear(&mut self) {
        self.buf.frame.clear();
        self.buf.tick_live.clear();
        self.buf.tick_done.clear();
        self.buf.timed.clear();
    }

    /// How many lines and labels the next drawn frame shows (built-in views
    /// are added when it is drawn, so they are not counted).
    #[must_use]
    pub fn stats(&self) -> DebugStats {
        let mut s = DebugStats::default();
        for b in self.visible() {
            for l in &b.lines {
                if l.flags & THREE_D != 0 {
                    s.lines_3d += 1;
                } else {
                    s.lines_2d += 1;
                }
            }
            s.labels += b.labels.len();
        }
        s
    }

    /// The batches the next frame shows.
    pub(crate) fn visible(&self) -> [&Batch; 4] {
        // `tick_live` is shown too: a frame drawn mid-tick (tools, tests)
        // sees what the tick drew so far.
        [
            &self.buf.frame,
            &self.buf.tick_done,
            &self.buf.tick_live,
            &self.buf.timed,
        ]
    }

    // ------------------------------------------------------------ engine side

    /// A frame starts at frame time `now`: last frame's shapes go, and the
    /// timed ones that ran out.
    pub(crate) fn begin_frame(&mut self, now: f64) {
        self.now = now as f32;
        self.buf.frame.clear();
        if !self.buf.timed.is_empty() {
            self.buf.timed.expire(self.now);
        }
    }

    /// Simulation systems start (`true`) or stop running.
    pub(crate) fn set_in_sim(&mut self, in_sim: bool) {
        self.in_sim = in_sim;
    }

    /// A simulation tick ended: what it drew is shown until the next one ends.
    pub(crate) fn end_tick(&mut self) {
        let b = &mut *self.buf;
        std::mem::swap(&mut b.tick_live, &mut b.tick_done);
        self.buf.tick_live.clear();
    }

    /// Makes this a view system's own buffer in a parallel batch: drawing
    /// when `main` draws, into the batch `main` would use.
    pub(crate) fn follow(&mut self, main: &DebugDraw) {
        self.enabled = main.enabled;
        self.in_sim = main.in_sim;
        self.now = main.now;
        self.toggles.follow(&main.toggles);
    }

    /// Takes what a view system drew into its own buffer ([`follow`](Self::follow)).
    pub(crate) fn absorb(&mut self, other: &mut DebugDraw) {
        if self.enabled {
            self.buf.frame.append(&other.buf.frame);
            self.buf.tick_live.append(&other.buf.tick_live);
            self.buf.timed.append(&other.buf.timed);
        }
        other.clear();
    }

    /// The batch new shapes without a lifetime go to.
    fn slot(&self) -> Slot {
        if self.in_sim { Slot::Tick } else { Slot::Frame }
    }

    fn batch(&mut self, slot: Slot) -> &mut Batch {
        match slot {
            Slot::Frame => &mut self.buf.frame,
            Slot::Tick => &mut self.buf.tick_live,
            Slot::Timed => &mut self.buf.timed,
        }
    }

    /// Starts a shape: where its lines and labels begin. `None` while off.
    #[inline]
    fn begin(&mut self) -> Option<(Slot, usize, usize)> {
        if !self.enabled {
            return None;
        }
        let slot = self.slot();
        let b = self.batch(slot);
        Some((slot, b.lines.len(), b.labels.len()))
    }

    fn shape(&mut self, start: Option<(Slot, usize, usize)>) -> DebugShape<'_> {
        DebugShape { draw: self, start }
    }

    /// Adds one line to the current batch.
    #[inline]
    fn push(&mut self, a: Vec3, b: Vec3, color: [u8; 4], three_d: bool) {
        let slot = self.slot();
        self.batch(slot).lines.push(DebugLine {
            a,
            b,
            color,
            width: 1.0,
            until: 0.0,
            flags: if three_d { THREE_D } else { 0 },
        });
    }

    /// Adds a closed loop through `points`.
    fn push_loop(&mut self, points: impl IntoIterator<Item = Vec3>, color: [u8; 4], three_d: bool) {
        let mut first = None;
        let mut last = None;
        for p in points {
            if let Some(l) = last {
                self.push(l, p, color, three_d);
            } else {
                first = Some(p);
            }
            last = Some(p);
        }
        if let (Some(f), Some(l)) = (first, last) {
            self.push(l, f, color, three_d);
        }
    }

    /// Adds an arc around `center` from angle `from` to `to` (radians) in
    /// the plane of `u` and `v`, each `radius` long, in `segments` pieces.
    #[allow(clippy::too_many_arguments)]
    fn push_arc(
        &mut self,
        center: Vec3,
        (u, v): (Vec3, Vec3),
        radius: f32,
        (from, to): (f32, f32),
        segments: usize,
        color: [u8; 4],
        three_d: bool,
    ) {
        let at = |t: f32| center + (u * t.cos() + v * t.sin()) * radius;
        let mut prev = at(from);
        for i in 1..=segments {
            let p = at(from + (to - from) * i as f32 / segments as f32);
            self.push(prev, p, color, three_d);
            prev = p;
        }
    }

    // ------------------------------------------------------------ the shapes

    /// A line from `a` to `b`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, from: Vec3, hit: Vec3) {
    /// ctx.debug.line(from, hit, Color::RED);
    /// # }
    /// ```
    pub fn line<P: DebugPoint>(&mut self, a: P, b: P, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            self.push(a.to_vec3(), b.to_vec3(), color.to_rgba8(), P::is_3d());
        }
        self.shape(start)
    }

    /// A line from `origin` along `direction` (its length is the line's).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, eye: Vec3, look: Vec3) {
    /// ctx.debug.ray(eye, look * 10.0, Color::YELLOW);
    /// # }
    /// ```
    pub fn ray<P: DebugPoint>(&mut self, origin: P, direction: P, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            let o = origin.to_vec3();
            self.push(o, o + direction.to_vec3(), color.to_rgba8(), P::is_3d());
        }
        self.shape(start)
    }

    /// A line from `from` to `to` with a head at `to`, a fifth of its length.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, at: Vec2, velocity: Vec2) {
    /// ctx.debug.arrow(at, at + velocity, Color::GREEN);
    /// # }
    /// ```
    pub fn arrow<P: DebugPoint>(&mut self, from: P, to: P, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            let (a, b) = (from.to_vec3(), to.to_vec3());
            let c = color.to_rgba8();
            let three_d = P::is_3d();
            self.push(a, b, c, three_d);
            let d = b - a;
            let len = d.length();
            if len > 0.0 && len.is_finite() {
                let dir = d / len;
                let head = len * 0.2;
                let back = b - dir * head;
                let sides: [Vec3; 2] = if three_d {
                    let p = dir.any_orthonormal_vector();
                    [p, dir.cross(p)]
                } else {
                    [Vec3::new(-dir.y, dir.x, 0.0), Vec3::ZERO]
                };
                for side in sides.into_iter().filter(|s| *s != Vec3::ZERO) {
                    self.push(b, back + side * head * 0.5, c, three_d);
                    self.push(b, back - side * head * 0.5, c, three_d);
                }
            }
        }
        self.shape(start)
    }

    /// Lines through `points`, in order (closed with `closed`).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, path: &[Vec3]) {
    /// ctx.debug.polyline(path, false, Color::WHITE);
    /// # }
    /// ```
    pub fn polyline<P: DebugPoint>(&mut self, points: &[P], closed: bool, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            let c = color.to_rgba8();
            for w in points.windows(2) {
                self.push(w[0].to_vec3(), w[1].to_vec3(), c, P::is_3d());
            }
            if closed && points.len() > 2 {
                self.push(points[points.len() - 1].to_vec3(), points[0].to_vec3(), c, P::is_3d());
            }
        }
        self.shape(start)
    }

    /// An axis-aligned box: a rectangle in 2D, a box of 12 edges in 3D.
    /// `size` is its full width, height (and depth).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.debug.aabb(vec2(100.0, 100.0), vec2(64.0, 32.0), Color::GREEN);
    /// ctx.debug.aabb(Vec3::ZERO, Vec3::splat(2.0), Color::GREEN);
    /// # }
    /// ```
    pub fn aabb<P: DebugPoint>(&mut self, center: P, size: P, color: Color) -> DebugShape<'_> {
        self.obb(center, size, Quat::IDENTITY, color)
    }

    /// A turned box: `rotation` about its center. In 2D, make the rotation
    /// with `Quat::from_rotation_z(radians)` (only that turn is used).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, crate_at: Vec3, crate_size: Vec3, crate_turn: Quat) {
    /// ctx.debug.obb(crate_at, crate_size, crate_turn, Color::YELLOW);
    /// # }
    /// ```
    pub fn obb<P: DebugPoint>(&mut self, center: P, size: P, rotation: Quat, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            let (o, h) = (center.to_vec3(), size.to_vec3() * 0.5);
            let c = color.to_rgba8();
            if P::is_3d() {
                let corner = |i: usize| {
                    let s = Vec3::new(
                        if i & 1 == 0 { -1.0 } else { 1.0 },
                        if i & 2 == 0 { -1.0 } else { 1.0 },
                        if i & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    o + rotation * (s * h)
                };
                for i in 0..8 {
                    for bit in [1, 2, 4] {
                        if i & bit == 0 {
                            self.push(corner(i), corner(i | bit), c, true);
                        }
                    }
                }
            } else {
                let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .map(|(x, y)| o + rotation * Vec3::new(x * h.x, y * h.y, 0.0));
                self.push_loop(corners, c, false);
            }
        }
        self.shape(start)
    }

    /// A circle of `radius`: in the XY plane in 2D, flat on the ground (the
    /// XZ plane) in 3D.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, tower: Vec2) {
    /// ctx.debug.circle(tower, 120.0, Color::BLUE); // its range
    /// # }
    /// ```
    pub fn circle<P: DebugPoint>(&mut self, center: P, radius: f32, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            let full = std::f32::consts::TAU;
            self.push_arc(
                center.to_vec3(),
                P::plane(),
                radius,
                (0.0, full),
                CIRCLE_SEGMENTS,
                color.to_rgba8(),
                P::is_3d(),
            );
        }
        self.shape(start)
    }

    /// A sphere of `radius`, as its three circles around the axes.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, blast: Vec3) {
    /// ctx.debug.sphere(blast, 4.0, Color::RED).seconds(1.0);
    /// # }
    /// ```
    pub fn sphere(&mut self, center: Vec3, radius: f32, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            let c = color.to_rgba8();
            let full = (0.0, std::f32::consts::TAU);
            for plane in [(Vec3::X, Vec3::Y), (Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z)] {
                self.push_arc(center, plane, radius, full, CIRCLE_SEGMENTS, c, true);
            }
        }
        self.shape(start)
    }

    /// A capsule: the points within `radius` of the segment from `a` to `b`.
    /// In 2D its two sides and round ends; in 3D a ring at each end, four
    /// sides and the round ends' outlines.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, feet: Vec3) {
    /// ctx.debug.capsule(feet + Vec3::Y * 0.4, feet + Vec3::Y * 1.4, 0.4, Color::GREEN);
    /// # }
    /// ```
    pub fn capsule<P: DebugPoint>(&mut self, a: P, b: P, radius: f32, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            let (a, b) = (a.to_vec3(), b.to_vec3());
            let c = color.to_rgba8();
            let half = CIRCLE_SEGMENTS / 2;
            let pi = std::f32::consts::PI;
            let axis = (b - a).try_normalize().unwrap_or(P::plane().1);
            if P::is_3d() {
                let p = axis.any_orthonormal_vector();
                let q = axis.cross(p);
                for end in [a, b] {
                    self.push_arc(end, (p, q), radius, (0.0, 2.0 * pi), CIRCLE_SEGMENTS, c, true);
                }
                for side in [p, q, -p, -q] {
                    self.push(a + side * radius, b + side * radius, c, true);
                }
                for side in [p, q] {
                    // From `side` over the end to `-side`.
                    self.push_arc(b, (side, axis), radius, (0.0, pi), half, c, true);
                    self.push_arc(a, (side, -axis), radius, (0.0, pi), half, c, true);
                }
            } else {
                let side = Vec3::new(-axis.y, axis.x, 0.0);
                self.push(a + side * radius, b + side * radius, c, false);
                self.push(a - side * radius, b - side * radius, c, false);
                self.push_arc(b, (side, axis), radius, (0.0, pi), half, c, false);
                self.push_arc(a, (side, -axis), radius, (0.0, -pi), half, c, false);
            }
        }
        self.shape(start)
    }

    /// What `camera` sees, between its near and far planes: the frustum's
    /// 12 edges. An infinite far plane is drawn 100 units past the near one.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, security_cam: &ponko::d3::Camera) {
    /// ctx.debug.frustum(security_cam, Color::rgb(1.0, 0.5, 0.0));
    /// # }
    /// ```
    #[cfg(feature = "3d")]
    pub fn frustum(&mut self, camera: &crate::d3::Camera, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            let mut camera = *camera;
            if !camera.far.is_finite() {
                camera.far = camera.near + INFINITE_FAR;
            }
            self.frustum_of(camera.view_proj(), color.to_rgba8());
        }
        self.shape(start)
    }

    /// The edges of the box clip space `view_proj` maps to (reverse-Z or
    /// not: both depth ends 0 and 1).
    #[cfg(feature = "3d")]
    fn frustum_of(&mut self, view_proj: Mat4, color: [u8; 4]) {
        let inv = view_proj.inverse();
        let corner = |i: usize| {
            let ndc = Vec3::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { 0.0 } else { 1.0 },
            );
            inv.project_point3(ndc)
        };
        let corners: [Vec3; 8] = std::array::from_fn(corner);
        if corners.iter().any(|c| !c.is_finite()) {
            return;
        }
        for i in 0..8 {
            for bit in [1, 2, 4] {
                if i & bit == 0 {
                    self.push(corners[i], corners[i | bit], color, true);
                }
            }
        }
    }

    /// A grid of `cells` by `cells` squares `cell` wide, centered on
    /// `center`: in the XY plane in 2D, on the ground (XZ) in 3D.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.debug.grid(Vec3::ZERO, 1.0, 20, Color::GRAY);
    /// # }
    /// ```
    pub fn grid<P: DebugPoint>(&mut self, center: P, cell: f32, cells: u32, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() && cells > 0 {
            let (u, v) = P::plane();
            let o = center.to_vec3();
            let half = cell * cells as f32 * 0.5;
            let c = color.to_rgba8();
            for i in 0..=cells {
                let t = -half + i as f32 * cell;
                self.push(o + u * t - v * half, o + u * t + v * half, c, P::is_3d());
                self.push(o + v * t - u * half, o + v * t + u * half, c, P::is_3d());
            }
        }
        self.shape(start)
    }

    /// Text with its top-left corner at `at` (a 3D point is projected to
    /// the screen; one behind the camera isn't drawn). Labels are drawn over
    /// everything, unless a 3D one is [`depth_tested`](DebugShape::depth_tested),
    /// 14 pixels high unless [`size`](DebugShape::size) says otherwise. A
    /// 3D label also shows in the render target of every target camera
    /// that draws particles and shows the default layer, over everything
    /// it draws there as on screen, laid out facing that camera (as linear
    /// light: exposure and tone mapping are the target's reader's).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, unit: Vec3, hp: i32) {
    /// ctx.debug.text(unit + Vec3::Y * 2.0, &format!("{hp} HP"), Color::WHITE);
    /// # }
    /// ```
    pub fn text<P: DebugPoint>(&mut self, at: P, text: &str, color: Color) -> DebugShape<'_> {
        let start = self.begin();
        if let Some((slot, ..)) = start {
            let b = self.batch(slot);
            let from = b.text.len();
            b.text.push_str(text);
            b.labels.push(DebugLabel {
                at: at.to_vec3(),
                color,
                size: LABEL_SIZE,
                until: 0.0,
                flags: if P::is_3d() { THREE_D } else { 0 },
                text: from..b.text.len(),
            });
        }
        self.shape(start)
    }

    /// The axes of a transform: x red, y green, z blue, each `length` long.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, at: Vec3, rotation: Quat) {
    /// ctx.debug.axes(at, rotation, 1.0);
    /// # }
    /// ```
    pub fn axes(&mut self, at: Vec3, rotation: Quat, length: f32) -> DebugShape<'_> {
        let start = self.begin();
        if start.is_some() {
            for (axis, color) in [(Vec3::X, Color::RED), (Vec3::Y, Color::GREEN), (Vec3::Z, Color::BLUE)] {
                self.push(at, at + rotation * axis * length, color.to_rgba8(), true);
            }
        }
        self.shape(start)
    }

    /// Turns a built-in view on or off (see [`DebugView`]); it draws every
    /// frame while debug drawing is enabled.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.debug.show(ponko::debug::DebugView::Colliders, true);
    /// assert!(ctx.debug.showing(ponko::debug::DebugView::Colliders));
    /// # }
    /// ```
    pub fn show(&mut self, view: DebugView, on: bool) {
        self.views.set(view, on);
    }

    /// Whether a built-in view is on.
    #[must_use]
    pub fn showing(&self, view: DebugView) -> bool {
        self.views.get(view)
    }

    /// Whether to draw what debug toggle `name` stands for: debug drawing
    /// is on, and so is the toggle. The game and its plugins add toggles
    /// with [`Game::debug_toggle`](crate::game::Game::debug_toggle); tools
    /// turn them on and off with the control endpoint's `debug.draw`
    /// method, code with [`set_toggle`](Self::set_toggle). `false` for a
    /// name no one added. Check it before work done only to be drawn.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// fn draw_paths(ctx: &mut Ctx) {
    ///     if ctx.debug.toggled("ai.paths") {
    ///         ctx.debug.line(vec2(0.0, 0.0), vec2(10.0, 0.0), Color::YELLOW);
    ///     }
    /// }
    /// Game::new("toggles", 1, 1).debug_draw(true).debug_toggle("ai.paths", true).run_headless(0, |ctx| {
    ///     draw_paths(ctx);
    ///     assert_eq!(ctx.debug.stats().lines_2d, 1);
    ///     ctx.debug.set_toggle("ai.paths", false).unwrap();
    ///     ctx.debug.clear();
    ///     draw_paths(ctx);
    ///     assert_eq!(ctx.debug.stats().lines_2d, 0);
    /// });
    /// ```
    #[must_use]
    pub fn toggled(&self, name: &str) -> bool {
        self.enabled && self.toggles.get(name)
    }

    /// Turns debug toggle `name` on or off, as a key binding or a menu
    /// would.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("toggles", 1, 1).debug_draw(true).debug_toggle("ai.paths", false).run_headless(0, |ctx| {
    ///     ctx.debug.set_toggle("ai.paths", true).unwrap();
    ///     assert!(ctx.debug.toggled("ai.paths"));
    ///     assert_eq!(ctx.debug.set_toggle("ai.nope", true).unwrap_err().kind(), ErrorKind::NotFound);
    /// });
    /// ```
    ///
    /// # Errors
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if no toggle
    /// of that name was added; the message lists the ones that were.
    pub fn set_toggle(&mut self, name: &str, on: bool) -> Result<(), crate::error::Error> {
        if self.toggles.set(name, on) {
            Ok(())
        } else {
            Err(crate::error::Error::new(
                crate::error::ErrorKind::NotFound,
                format!("no debug toggle `{name}` (toggles: {})", self.toggles.names()),
            ))
        }
    }

    /// Every debug toggle the game added and whether it is on (whatever
    /// [`enabled`](Self::enabled) says), in the order they were added.
    pub fn toggles(&self) -> impl Iterator<Item = (&str, bool)> + '_ {
        self.toggles.iter()
    }

    /// The game's toggles, when it starts.
    pub(crate) fn set_toggles(&mut self, toggles: &[(String, bool)]) {
        self.toggles = toggles::Toggles::new(toggles);
    }

    /// The `toggles` names, for messages.
    pub(crate) fn toggle_names(&self) -> String {
        self.toggles.names()
    }

    /// How far from the 3D camera (in world units) the
    /// [colliders view](DebugView::Colliders) draws the triangles of 3D
    /// collision meshes and terrain: a triangle farther than that from the
    /// camera is left out, so a big level or terrain costs what is near.
    /// Default 50. Values below 0 (or not finite) are taken as 0.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.debug.show(ponko::debug::DebugView::Colliders, true);
    /// ctx.debug.set_collider_distance(20.0);
    /// assert_eq!(ctx.debug.collider_distance(), 20.0);
    /// # }
    /// ```
    pub fn set_collider_distance(&mut self, distance: f32) {
        self.collider_distance = if distance.is_finite() { distance.max(0.0) } else { 0.0 };
    }

    /// See [`set_collider_distance`](Self::set_collider_distance).
    #[must_use]
    pub fn collider_distance(&self) -> f32 {
        self.collider_distance
    }
}

/// What [`DebugDraw`]'s calls return: the shape just drawn, to change.
/// Dropping it keeps the shape as it is.
///
/// ```
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx, at: Vec3) {
/// ctx.debug.sphere(at, 1.0, Color::RED).seconds(3.0).on_top().width(2.0);
/// # }
/// ```
pub struct DebugShape<'a> {
    draw: &'a mut DebugDraw,
    /// The batch it went to and where its lines and labels start; `None`
    /// while drawing is off.
    start: Option<(Slot, usize, usize)>,
}

impl DebugShape<'_> {
    /// Changes the shape's lines and labels.
    fn each(&mut self, line: impl Fn(&mut DebugLine), label: impl Fn(&mut DebugLabel)) {
        if let Some((slot, lines, labels)) = self.start {
            let b = self.draw.batch(slot);
            b.lines[lines..].iter_mut().for_each(line);
            b.labels[labels..].iter_mut().for_each(label);
        }
    }

    /// Keeps the shape for `seconds` of frame time instead of until its
    /// system runs again. Values of 0 or less (or not finite) change nothing.
    pub fn seconds(mut self, seconds: f32) -> Self {
        if let Some((slot, lines, labels)) = self.start
            && slot != Slot::Timed
            && seconds > 0.0
            && seconds.is_finite()
        {
            let until = self.draw.now + seconds;
            let mut from = std::mem::take(self.draw.batch(slot));
            let to = &mut self.draw.buf.timed;
            let start = (Slot::Timed, to.lines.len(), to.labels.len());
            from.move_tail(lines, labels, to, until);
            *self.draw.batch(slot) = from;
            self.start = Some(start);
        }
        self
    }

    /// Draws 3D lines over nearer surfaces instead of hidden by them, and 2D
    /// lines over the UI instead of under it.
    pub fn on_top(mut self) -> Self {
        self.each(|l| l.flags |= ON_TOP, |l| l.flags = (l.flags | ON_TOP) & !TESTED);
        self
    }

    /// Hides 3D labels behind nearer 3D surfaces, as 3D lines are (labels
    /// are otherwise drawn over everything). A depth-tested label is drawn
    /// in the lit 3D frame of every 3D camera that shows the default layer
    /// (split-screen views, and target cameras that draw particles), facing
    /// that camera, `size` pixels high where it is, with its top-left
    /// corner at its point; exposure and tone mapping
    /// apply to its color, as to the lines. Lines are depth-tested already
    /// (unless [`on_top`](Self::on_top)); 2D shapes don't change. The last
    /// of `on_top` and `depth_tested` wins.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, crate_at: Vec3) {
    /// ctx.debug.text(crate_at + Vec3::Y, "crate", Color::WHITE).depth_tested();
    /// # }
    /// ```
    pub fn depth_tested(mut self) -> Self {
        self.each(
            |_| {},
            |l| {
                if l.flags & THREE_D != 0 {
                    l.flags = (l.flags | TESTED) & !ON_TOP;
                }
            },
        );
        self
    }

    /// 2D points are window pixels from the top-left corner instead of
    /// world units. 3D shapes stay in the 3D world.
    pub fn screen(self) -> Self {
        self.space(DrawSpace::Screen)
    }

    /// Where 2D points are ([`DrawSpace`]); 3D shapes stay in the 3D world.
    pub fn space(mut self, space: DrawSpace) -> Self {
        let set = |flags: &mut u8| {
            if *flags & THREE_D == 0 {
                match space {
                    DrawSpace::World => *flags &= !SCREEN,
                    DrawSpace::Screen => *flags |= SCREEN,
                }
            }
        };
        self.each(|l| set(&mut l.flags), |l| set(&mut l.flags));
        self
    }

    /// Line width in pixels (default 1, at least 1 on the GPU).
    pub fn width(mut self, pixels: f32) -> Self {
        let w = if pixels.is_finite() { pixels.max(0.0) } else { 1.0 };
        self.each(|l| l.width = w, |_| {});
        self
    }

    /// Label height in pixels (default 14).
    pub fn size(mut self, pixels: f32) -> Self {
        let s = if pixels.is_finite() {
            pixels.max(1.0)
        } else {
            LABEL_SIZE
        };
        self.each(|_| {}, |l| l.size = s);
        self
    }
}
