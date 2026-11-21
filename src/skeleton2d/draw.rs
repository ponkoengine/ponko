//! Drawing skeletons into the sprite draw list: every part is a triangle
//! list in the same list, batches and `z` order as sprites.
//!
//! Many skeletons are drawn on all cores: the skeletons, in query order,
//! are split into runs; each thread builds its runs' vertices into a chunk
//! of its own, and the chunks are copied into the draw list in order, so
//! the list is the same as drawing them one by one.

use glam::{Affine2, Vec2};
use ponko_ecs::Entity;
use rayon::prelude::*;

use super::Skeleton;
use super::data::{Attachment, Named};
use crate::color::Color;
use crate::ctx::Ctx;
use crate::ecs::Transform2D;
use crate::render::batch::{DrawList, MESH, SpriteInstance};

/// The quad corners in the order sprites draw them (two triangles).
const CORNERS: [Vec2; 6] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(0.0, 1.0),
    Vec2::new(0.0, 1.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(1.0, 1.0),
];

/// Below this many skeletons they are drawn on the calling thread: the
/// fork and the copy cost more than they save.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub(crate) const PARALLEL_MIN: usize = 64;

/// In a web build with threads, handing the drawing to the frame pool's
/// workers and waiting for them costs about 0.1 ms. In headless Chromium
/// with 13 threads, 256 skeletons of two sprites took 0.08 ms on the page's
/// thread and 0.10 to 0.17 ms shared out, 512 took 0.16 ms and 0.10 to 0.13
/// ms, 1,024 took 0.32 ms and 0.16 to 0.24 ms (two runs;
/// `web/smoke/threads.mjs --case thresholds`; the guide's "Threads on the
/// web").
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub(crate) const PARALLEL_MIN: usize = 512;

/// The fewest skeletons a thread's run holds.
const RUN_MIN: usize = 16;

/// Where drawn triangles go: the draw list itself, or a thread's chunk.
pub(crate) trait Sink {
    /// Every three of `vertices` are one triangle, drawn with `texture` and
    /// `style` at `z`, after what came before.
    fn mesh(&mut self, texture: u32, style: u32, z: f32, vertices: &[SpriteInstance]);
}

impl Sink for DrawList {
    fn mesh(&mut self, texture: u32, style: u32, z: f32, vertices: &[SpriteInstance]) {
        self.push_mesh(texture, style, z, vertices);
    }
}

/// Vertices with one texture, style and `z`, up to `end` in their chunk.
#[derive(Debug, Clone, Copy)]
struct Run {
    texture: u32,
    style: u32,
    z: f32,
    end: usize,
}

/// One thread's share of a frame's skeleton vertices, in draw order.
#[derive(Debug, Default)]
pub(crate) struct Chunk {
    vertices: Vec<SpriteInstance>,
    runs: Vec<Run>,
    scratch: Scratch,
}

impl Sink for Chunk {
    fn mesh(&mut self, texture: u32, style: u32, z: f32, vertices: &[SpriteInstance]) {
        self.vertices.extend_from_slice(vertices);
        let end = self.vertices.len();
        match self.runs.last_mut() {
            Some(r) if r.texture == texture && r.style == style && r.z.to_bits() == z.to_bits() => r.end = end,
            _ => self.runs.push(Run { texture, style, z, end }),
        }
    }
}

impl Chunk {
    fn clear(&mut self) {
        self.vertices.clear();
        self.runs.clear();
    }

    /// Pushes its runs onto `list` one by one.
    fn push_into(&self, list: &mut DrawList) {
        let mut start = 0;
        for r in &self.runs {
            list.push_mesh(r.texture, r.style, r.z, &self.vertices[start..r.end]);
            start = r.end;
        }
    }
}

/// A vertex before it is packed for the GPU: where it lands, where it is
/// in the image, and where it is in its part (sprite shaders' `local`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Raw {
    pos: Vec2,
    uv: Vec2,
    local: Vec2,
}

impl Raw {
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            pos: self.pos.lerp(other.pos, t),
            uv: self.uv.lerp(other.uv, t),
            local: self.local.lerp(other.local, t),
        }
    }

    fn pack(self, color: [u8; 4]) -> SpriteInstance {
        SpriteInstance::vertex(self.pos.into(), self.uv.into(), self.local.into(), color)
    }
}

/// Space reused from skeleton to skeleton and frame to frame.
#[derive(Debug, Default)]
pub(crate) struct Scratch {
    /// A mesh's vertices, posed.
    skinned: Vec<Vec2>,
    /// A mesh's vertices, packed.
    packed: Vec<SpriteInstance>,
    /// A part's triangles, ready to go.
    out: Vec<SpriteInstance>,
    /// A part's triangle corners before clipping.
    raw: Vec<Raw>,
    clipper: Clipper,
}

/// The frame's skeletons to draw in parallel, and their chunks.
#[derive(Default)]
pub(crate) struct DrawChunks {
    /// Entity, its transform and look, in query order.
    items: Vec<(Entity, Transform2D, u32)>,
    chunks: Vec<Chunk>,
    /// Where each chunk's vertices start in the draw list.
    starts: Vec<usize>,
    /// For drawing on the calling thread.
    serial: Chunk,
}

/// How [`draw_skeletons_with`] spreads the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spread {
    /// On all cores when there are many skeletons.
    Auto,
    /// On the calling thread, straight into the draw list.
    #[cfg(test)]
    Serial,
    /// In chunks of this many skeletons on all cores, however few there are.
    #[cfg(test)]
    Runs(usize),
}

/// How many skeletons each thread's run holds, of `n` on `threads`
/// threads: about four runs per thread, so uneven skeletons still spread.
fn run_length(n: usize, threads: usize) -> usize {
    n.div_ceil(threads * 4).max(RUN_MIN)
}

/// Queues every skeleton's visible parts. Out of line and cheap when the
/// world has no skeleton (one archetype count).
#[inline(never)]
pub(crate) fn draw_skeletons(ctx: &mut Ctx) {
    draw_skeletons_with(ctx, Spread::Auto);
}

/// [`draw_skeletons`], spread as `spread` says.
pub(crate) fn draw_skeletons_with(ctx: &mut Ctx, spread: Spread) {
    use crate::sprite_material::{BlendMode, SpriteMaterial, style_key};
    let style = |s: &Skeleton, b: Option<&BlendMode>, m: Option<&SpriteMaterial>| {
        let blend = b.copied().unwrap_or(if s.data().premultiplied {
            BlendMode::Premultiplied
        } else {
            BlendMode::default()
        });
        style_key(blend, m.copied())
    };
    let n = {
        let mut query = ctx
            .world
            .query::<(&Transform2D, &Skeleton, Option<&BlendMode>, Option<&SpriteMaterial>)>();
        let n = query.count();
        if n == 0 {
            return;
        }
        n
    };
    let threads = crate::frame_pool::threads();
    let per = match spread {
        Spread::Auto => crate::frame_pool::share(|| n >= PARALLEL_MIN && threads > 1).then(|| run_length(n, threads)),
        #[cfg(test)]
        Spread::Serial => None,
        #[cfg(test)]
        Spread::Runs(per) => Some(per.max(1)),
    };
    if ctx.world.get_resource::<DrawChunks>().is_none() {
        ctx.world.insert_resource(DrawChunks::default());
    }
    // Taken out for the frame (moving the vectors allocates nothing).
    let mut d = std::mem::take(ctx.world.resource_mut::<DrawChunks>());
    let list = &mut ctx.gfx.draw_list;
    if let Some(per) = per {
        d.items.clear();
        let items = &mut d.items;
        ctx.world
            .query::<(
                Entity,
                &Transform2D,
                &Skeleton,
                Option<&BlendMode>,
                Option<&SpriteMaterial>,
            )>()
            .for_each(|(e, t, s, b, m)| items.push((e, *t, style(s, b, m))));
        let used = n.div_ceil(per);
        if d.chunks.len() < used {
            d.chunks.resize_with(used, Chunk::default);
        }
        let world = &ctx.world;
        let DrawChunks {
            items, chunks, starts, ..
        } = &mut d;
        // Drawn and merged on the frame pool (on the web, its workers, not
        // the page's thread), as parallel queries run.
        crate::frame_pool::run(|| {
            crate::profile_scope!("skeleton2d/shared_draw");
            chunks[..used]
                .par_iter_mut()
                .zip(items.par_chunks(per))
                .for_each(|(chunk, items)| {
                    // Counts this chunk's allocations on whichever thread draws it.
                    crate::profile_scope!("skeleton2d/draw_chunk");
                    chunk.clear();
                    let mut scratch = std::mem::take(&mut chunk.scratch);
                    for (e, t, style) in items {
                        if let Some(s) = world.get::<Skeleton>(*e) {
                            draw(chunk, &mut scratch, t, s, *style);
                        }
                    }
                    chunk.scratch = scratch;
                });
            merge(list, &chunks[..used], starts);
        });
    } else {
        let scratch = &mut d.serial.scratch;
        ctx.world
            .query::<(&Transform2D, &Skeleton, Option<&BlendMode>, Option<&SpriteMaterial>)>()
            .for_each(|(t, s, b, m)| draw(list, scratch, t, s, style(s, b, m)));
    }
    *ctx.world.resource_mut::<DrawChunks>() = d;
}

/// Copies `chunks`' vertices onto `list` in order, on all cores; `starts`
/// is space for where each begins.
fn merge(list: &mut DrawList, chunks: &[Chunk], starts: &mut Vec<usize>) {
    if list.clip().is_some() || list.layers() != crate::camera::RenderLayers::DEFAULT.0 {
        // Clipped or layered sprites need a state of their own per vertex.
        for chunk in chunks {
            chunk.push_into(list);
        }
        return;
    }
    starts.clear();
    let mut total = 0;
    for chunk in chunks {
        starts.push(total);
        total += chunk.vertices.len();
    }
    // SAFETY: the chunks' vertex ranges, at their starts, cover `0..total`
    // exactly once between them.
    unsafe {
        list.extend(total, |slots| {
            chunks.par_iter().zip(starts.par_iter()).for_each(|(chunk, &start)| {
                let mut from = 0;
                for r in &chunk.runs {
                    for (i, v) in chunk.vertices[from..r.end].iter().enumerate() {
                        slots.write_styled(start + from + i, r.texture, r.style | MESH, r.z, *v);
                    }
                    from = r.end;
                }
            });
        });
    }
}

fn tint(a: Color, b: Color) -> Color {
    Color::rgba(a.r * b.r, a.g * b.g, a.b * b.b, a.a * b.a)
}

/// Where vertex `v` of mesh or mask `named` is now: its influences' bones
/// as posed, plus a deform's offsets in those bones' spaces.
fn skin(s: &Skeleton, named: &Named, deform: Option<&[Vec2]>, v: usize) -> Vec2 {
    let mut at = Vec2::ZERO;
    for k in 0..4 {
        let (b, local, w) = named.binds[4 * v + k];
        if w > 0.0 {
            let local = deform.map_or(local, |d| local + d[4 * v + k]);
            at += s.bone_matrix(usize::from(b)).transform_point2(local) * w;
        }
    }
    at
}

/// Queues skeleton `s` at transform `t` with look `style` into `out`.
pub(crate) fn draw(out: &mut impl Sink, scratch: &mut Scratch, t: &Transform2D, s: &Skeleton, style: u32) {
    let entity = Affine2::from_scale_angle_translation(t.scale, t.rotation, t.position);
    let data = s.data();
    let z = s.z;
    let Scratch {
        skinned,
        packed,
        out: verts,
        raw,
        clipper,
    } = scratch;
    clipper.stop();
    for &slot in s.draw_order() {
        let slot = usize::from(slot);
        if let Some(a) = s.shown(slot) {
            let named = &data.attachments[a];
            let bone = data.slots[slot].bone;
            let base = tint(s.color, s.slot_color(slot));
            match &named.attachment {
                Attachment::Sprite(p) => {
                    let m = entity * s.bone_matrix(bone) * p.offset.matrix();
                    let color = tint(base, p.color).to_rgba8();
                    let size = p.texture.size().max(Vec2::ONE);
                    let (uv0, uvs) = p
                        .region
                        .map_or((Vec2::ZERO, Vec2::ONE), |r| (r.min / size, r.size() / size));
                    let corner = |c: Vec2| {
                        // A region packed a quarter turn counter-clockwise:
                        // the image's top edge runs up its left side.
                        let st = if p.rotated { Vec2::new(c.y, 1.0 - c.x) } else { c };
                        Raw {
                            pos: m.transform_point2((c - p.pivot) * p.size),
                            uv: uv0 + st * uvs,
                            local: c,
                        }
                    };
                    verts.clear();
                    if clipper.active {
                        raw.clear();
                        raw.extend(CORNERS.map(corner));
                        clipper.clip(raw, color, verts);
                    } else {
                        verts.extend(CORNERS.map(|c| corner(c).pack(color)));
                    }
                    if !verts.is_empty() {
                        out.mesh(p.texture.id, style, z, verts);
                    }
                }
                Attachment::Mesh(mesh) => {
                    let color = tint(base, mesh.color).to_rgba8();
                    let deform = s.deform(slot, a);
                    let (min, extent) = named.bounds;
                    let corner = |v: usize, at: Vec2| {
                        let rest = &mesh.vertices[v];
                        Raw {
                            pos: at,
                            uv: rest.uv,
                            local: (rest.position - min) / extent,
                        }
                    };
                    skinned.clear();
                    skinned
                        .extend((0..mesh.vertices.len()).map(|v| entity.transform_point2(skin(s, named, deform, v))));
                    verts.clear();
                    if clipper.active {
                        raw.clear();
                        raw.extend(
                            mesh.triangles
                                .iter()
                                .map(|&i| corner(usize::from(i), skinned[usize::from(i)])),
                        );
                        clipper.clip(raw, color, verts);
                    } else {
                        packed.clear();
                        packed.extend(skinned.iter().enumerate().map(|(v, &at)| corner(v, at).pack(color)));
                        verts.extend(mesh.triangles.iter().map(|&i| packed[usize::from(i)]));
                    }
                    if !verts.is_empty() {
                        out.mesh(mesh.texture.id, style, z, verts);
                    }
                }
                Attachment::Path(_) => {}
                Attachment::Clipping(mask) => {
                    if !clipper.active {
                        let deform = s.deform(slot, a);
                        clipper.start(
                            (0..mask.vertices.len()).map(|v| entity.transform_point2(skin(s, named, deform, v))),
                            mask.end,
                        );
                    }
                }
            }
        }
        if clipper.active && clipper.end == Some(slot) {
            clipper.stop();
        }
    }
    clipper.stop();
}

/// Clips triangles to a polygon: the polygon is cut into triangles, and
/// each drawn triangle is cut to each of them (Sutherland-Hodgman), so any
/// simple polygon works, convex or not.
#[derive(Debug, Default)]
pub(crate) struct Clipper {
    active: bool,
    /// The last slot it clips; `None`: to the end of the skeleton.
    end: Option<usize>,
    polygon: Vec<Vec2>,
    /// The polygon's triangles, each wound with positive area.
    triangles: Vec<[Vec2; 3]>,
    /// The corners left while cutting ears off.
    left: Vec<usize>,
    a: Vec<Raw>,
    b: Vec<Raw>,
}

/// Twice the signed area of the triangle `a`, `b`, `c`.
fn cross(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    (b - a).perp_dot(c - a)
}

impl Clipper {
    /// Clips from now on to the polygon through `corners` (in order),
    /// until slot `end` has been drawn.
    fn start(&mut self, corners: impl Iterator<Item = Vec2>, end: Option<usize>) {
        self.polygon.clear();
        self.polygon.extend(corners);
        self.triangles.clear();
        let n = self.polygon.len();
        if n < 3 {
            // No polygon: nothing is clipped (as in Spine).
            return;
        }
        let area: f32 = (0..n)
            .map(|i| self.polygon[i].perp_dot(self.polygon[(i + 1) % n]))
            .sum();
        if area < 0.0 {
            self.polygon.reverse();
        }
        // Ear clipping: cut off a convex corner with no other corner inside.
        self.left.clear();
        self.left.extend(0..n);
        let mut guard = 0;
        while self.left.len() > 3 && guard < n * n {
            guard += 1;
            let m = self.left.len();
            let ear = (0..m).find(|&i| {
                let (p, c, q) = (self.left[(i + m - 1) % m], self.left[i], self.left[(i + 1) % m]);
                let (p, c, q) = (self.polygon[p], self.polygon[c], self.polygon[q]);
                cross(p, c, q) > 0.0
                    && !self.left.iter().any(|&o| {
                        let o = self.polygon[o];
                        o != p
                            && o != c
                            && o != q
                            && cross(p, c, o) >= 0.0
                            && cross(c, q, o) >= 0.0
                            && cross(q, p, o) >= 0.0
                    })
            });
            let Some(i) = ear else {
                break;
            };
            let (p, c, q) = (self.left[(i + m - 1) % m], self.left[i], self.left[(i + 1) % m]);
            self.triangles.push([self.polygon[p], self.polygon[c], self.polygon[q]]);
            self.left.remove(i);
        }
        // The rest (three corners, or a polygon too degenerate to cut) as a fan.
        for i in 1..self.left.len().saturating_sub(1) {
            let t = [
                self.polygon[self.left[0]],
                self.polygon[self.left[i]],
                self.polygon[self.left[i + 1]],
            ];
            if cross(t[0], t[1], t[2]) > 0.0 {
                self.triangles.push(t);
            }
        }
        self.active = true;
        self.end = end;
    }

    fn stop(&mut self) {
        self.active = false;
        self.end = None;
    }

    /// Appends to `out` the parts of the triangles `corners` (three per
    /// triangle) inside the polygon, as triangles with color `color`.
    fn clip(&mut self, corners: &[Raw], color: [u8; 4], out: &mut Vec<SpriteInstance>) {
        for tri in corners.as_chunks::<3>().0 {
            for mask in &self.triangles {
                self.a.clear();
                self.a.extend_from_slice(tri);
                for e in 0..3 {
                    let (from, to) = (mask[e], mask[(e + 1) % 3]);
                    self.b.clear();
                    let side = |r: &Raw| cross(from, to, r.pos);
                    for i in 0..self.a.len() {
                        let (p, q) = (self.a[i], self.a[(i + 1) % self.a.len()]);
                        let (sp, sq) = (side(&p), side(&q));
                        if sp >= 0.0 {
                            self.b.push(p);
                        }
                        if (sp >= 0.0) != (sq >= 0.0) {
                            self.b.push(p.lerp(q, sp / (sp - sq)));
                        }
                    }
                    std::mem::swap(&mut self.a, &mut self.b);
                    if self.a.len() < 3 {
                        break;
                    }
                }
                for i in 1..self.a.len().saturating_sub(1) {
                    out.extend([self.a[0], self.a[i], self.a[i + 1]].map(|r| r.pack(color)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::{Vec2, vec2};

    use super::*;
    use crate::render::batch::f16_bits;
    use crate::skeleton2d::{BonePose, ClippingMask, MeshVertex, SkeletonData, SkinnedMesh, SpritePart};
    use crate::texture::Texture;

    /// root -> bend (10, 0); a 3-vertex mesh: one vertex on each bone and
    /// one split half and half; and a rigid part on `bend`.
    fn rig() -> Skeleton {
        let mut data = SkeletonData::new();
        let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
        let bend = data.add_bone("bend", Some(root), BonePose::at(vec2(10.0, 0.0)), 10.0);
        let skin = data.add_slot("skin", root);
        let vertices = vec![
            MeshVertex::new(vec2(0.0, 0.0), vec2(0.0, 0.0), &[(root, 1.0)]),
            MeshVertex::new(vec2(20.0, 0.0), vec2(1.0, 0.0), &[(bend, 1.0)]),
            MeshVertex::new(vec2(10.0, 4.0), vec2(0.5, 1.0), &[(root, 1.0), (bend, 1.0)]),
        ];
        data.add_attachment(
            skin,
            "skin",
            SkinnedMesh::new(Texture::new(7, 4, 4), vertices, vec![0, 1, 2]),
        );
        let part = data.add_slot("part", bend);
        let sprite = SpritePart::new(Texture::new(8, 4, 4))
            .with_size(vec2(4.0, 2.0))
            .with_pivot(Vec2::ZERO);
        data.add_attachment(part, "part", sprite);
        Skeleton::new(Arc::new(data))
    }

    fn positions(v: &[SpriteInstance]) -> Vec<Vec2> {
        v.iter().map(|i| Vec2::from(i.pos)).collect()
    }

    fn near(a: &[Vec2], b: &[Vec2]) {
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert!(a.distance(*b) < 1e-3, "{a:?} != {b:?}");
        }
    }

    #[test]
    fn skinned_vertices_follow_their_bones_by_weight() {
        let mut s = rig();
        let mut list = DrawList::default();
        draw(
            &mut list,
            &mut Scratch::default(),
            &Transform2D::at(vec2(100.0, 0.0)),
            &s,
            0,
        );
        let (v, _) = list.finish();
        near(
            &positions(&v[..3]),
            &[vec2(100.0, 0.0), vec2(120.0, 0.0), vec2(110.0, 4.0)],
        );
        // A quarter turn of `bend` (clockwise, y down) swings its vertex
        // to below the joint; the half-weighted one moves half way.
        s.set_bone_pose(
            1,
            BonePose::at(vec2(10.0, 0.0)).with_rotation(std::f32::consts::FRAC_PI_2),
        );
        list.clear();
        draw(
            &mut list,
            &mut Scratch::default(),
            &Transform2D::at(vec2(100.0, 0.0)),
            &s,
            0,
        );
        let (v, _) = list.finish();
        // Bound at rest: (20, 0) is (10, 0) in bend; (10, 4) is (0, 4) in bend.
        let bent_tip = vec2(110.0, 10.0);
        let split = (vec2(110.0, 4.0) + vec2(110.0 - 4.0, 0.0)) * 0.5;
        near(&positions(&v[..3]), &[vec2(100.0, 0.0), bent_tip, split]);
    }

    #[test]
    fn mesh_vertices_carry_their_uv_and_local_position_and_no_size() {
        let s = rig();
        let mut list = DrawList::default();
        draw(&mut list, &mut Scratch::default(), &Transform2D::default(), &s, 0);
        let (v, batches) = list.finish();
        assert_eq!(v[2].size, [0.0, 0.0], "size 0 marks a mesh vertex");
        assert_eq!(
            v[2].pivot,
            [f16_bits(0.5), f16_bits(1.0)],
            "local position in its bounds"
        );
        assert_eq!(batches.len(), 2, "mesh, then the part");
        assert!(batches.iter().all(|b| b.mesh));
        assert_eq!((batches[0].texture, batches[0].start, batches[0].end), (7, 0, 3));
        // The rigid part is two triangles at the bone, 4 x 2 from its pivot.
        assert_eq!((batches[1].texture, batches[1].end - batches[1].start), (8, 6));
        near(
            &positions(&v[3..9]),
            &[
                vec2(10.0, 0.0),
                vec2(14.0, 0.0),
                vec2(10.0, 2.0),
                vec2(10.0, 2.0),
                vec2(14.0, 0.0),
                vec2(14.0, 2.0),
            ],
        );
    }

    #[test]
    fn skeletons_sort_by_z_with_sprites_and_keep_their_draw_order() {
        let mut s = rig();
        s.z = 1.0;
        let mut list = DrawList::default();
        let sprite = SpriteInstance::new([0.0; 2], [1.0; 2], [0.5; 2], 0.0, [255; 4]);
        list.push(7, 2.0, sprite);
        draw(&mut list, &mut Scratch::default(), &Transform2D::default(), &s, 0);
        list.push(7, 0.0, sprite);
        let (v, batches) = list.finish();
        let kinds: Vec<(bool, u32)> = batches.iter().map(|b| (b.mesh, b.end - b.start)).collect();
        assert_eq!(kinds, [(false, 1), (true, 3), (true, 6), (false, 1)]);
        assert_eq!(v.len(), 11);
        // Hiding a slot draws nothing for it; reordering swaps the parts.
        s.set_attachment(0, Some(None));
        list.clear();
        draw(&mut list, &mut Scratch::default(), &Transform2D::default(), &s, 0);
        assert_eq!(list.finish().0.len(), 6);
    }

    /// root -> bend; slot 0 a clipping mask on root ending at slot 2,
    /// slot 1 the mesh, slot 2 a 10 x 10 sprite at the origin, slot 3 the
    /// same sprite again (after the mask's end).
    fn masked(mask: &[Vec2]) -> Skeleton {
        let mut data = SkeletonData::new();
        let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
        let bend = data.add_bone("bend", Some(root), BonePose::at(vec2(10.0, 0.0)), 10.0);
        let clip = data.add_slot("mask", root);
        let skin = data.add_slot("skin", root);
        let inside = data.add_slot("inside", root);
        let after = data.add_slot("after", root);
        let corners = mask
            .iter()
            .map(|&p| MeshVertex::new(p, Vec2::ZERO, &[(root, 1.0)]))
            .collect();
        data.add_attachment(clip, "mask", ClippingMask::new(corners, Some(inside)));
        let vertices = vec![
            MeshVertex::new(vec2(0.0, 0.0), vec2(0.0, 0.0), &[(root, 1.0)]),
            MeshVertex::new(vec2(20.0, 0.0), vec2(1.0, 0.0), &[(bend, 1.0)]),
            MeshVertex::new(vec2(10.0, 4.0), vec2(0.5, 1.0), &[(root, 1.0), (bend, 1.0)]),
        ];
        data.add_attachment(
            skin,
            "skin",
            SkinnedMesh::new(Texture::new(7, 4, 4), vertices, vec![0, 1, 2]),
        );
        let square = SpritePart::new(Texture::new(8, 4, 4))
            .with_size(vec2(10.0, 10.0))
            .with_pivot(Vec2::ZERO);
        data.add_attachment(inside, "square", square.clone());
        data.add_attachment(after, "square", square);
        Skeleton::new(Arc::new(data))
    }

    /// The triangles' total area (absolute), and their x and y bounds.
    fn area_and_bounds(v: &[SpriteInstance]) -> (f32, Vec2, Vec2) {
        let p = positions(v);
        let area = p
            .chunks(3)
            .map(|t| (t[1] - t[0]).perp_dot(t[2] - t[0]).abs() / 2.0)
            .sum();
        let min = p.iter().fold(Vec2::splat(f32::MAX), |a, b| a.min(*b));
        let max = p.iter().fold(Vec2::splat(f32::MIN), |a, b| a.max(*b));
        (area, min, max)
    }

    #[test]
    fn a_clipping_mask_keeps_only_what_is_inside_it_until_its_end_slot() {
        // A square mask over the right half of the 10 x 10 sprite.
        let mut s = masked(&[vec2(5.0, 0.0), vec2(15.0, 0.0), vec2(15.0, 10.0), vec2(5.0, 10.0)]);
        s.set_attachment(1, Some(None));
        let mut list = DrawList::default();
        draw(&mut list, &mut Scratch::default(), &Transform2D::default(), &s, 0);
        let (v, _) = list.finish();
        // The last slot's whole quad comes after the clipped part.
        let clipped = v.len() - 6;
        let (area, min, max) = area_and_bounds(&v[..clipped]);
        assert!((area - 50.0).abs() < 1e-3, "half of 100: {area}");
        assert!(min.distance(vec2(5.0, 0.0)) < 1e-4 && max.distance(vec2(10.0, 10.0)) < 1e-4);
        // Where it is cut, half way across, the image is half way across too.
        for i in &v[..clipped] {
            if (i.pos[0] - 5.0).abs() < 1e-4 {
                assert_eq!(i.uv[0], 32768, "u = 0.5");
                assert_eq!(i.pivot[0], f16_bits(0.5), "local x = 0.5");
            }
        }
        // The slot after its end slot draws whole: one quad.
        let (area, _, _) = area_and_bounds(&v[clipped..]);
        assert_eq!(v.len() - clipped, 6);
        assert!((area - 100.0).abs() < 1e-3);
    }

    #[test]
    fn a_concave_mask_clips_to_its_own_shape_either_way_round() {
        // An L over the 10 x 10 sprite: all of it but the top-right quarter
        // (75 of 100), wound one way, then the other.
        let l = [
            vec2(0.0, 0.0),
            vec2(5.0, 0.0),
            vec2(5.0, 5.0),
            vec2(10.0, 5.0),
            vec2(10.0, 10.0),
            vec2(0.0, 10.0),
        ];
        let mut reversed = l;
        reversed.reverse();
        for mask in [l, reversed] {
            let mut s = masked(&mask);
            s.set_attachment(1, Some(None));
            let mut list = DrawList::default();
            draw(&mut list, &mut Scratch::default(), &Transform2D::default(), &s, 0);
            let (v, _) = list.finish();
            let clipped = v.len() - 6;
            let (area, _, _) = area_and_bounds(&v[..clipped]);
            assert!((area - 75.0).abs() < 1e-3, "{area}");
            // Nothing of the cut-out quarter (x > 5, y < 5) is drawn.
            for t in positions(&v[..clipped]).chunks(3) {
                let center = (t[0] + t[1] + t[2]) / 3.0;
                assert!(!(center.x > 5.0 && center.y < 5.0), "{t:?}");
            }
        }
    }

    #[test]
    fn a_mask_clips_meshes_too_and_a_slot_without_one_draws_whole() {
        // The mask covers x < 10: of the mesh (0,0) (20,0) (10,4), area 40,
        // the part left of x = 10 is half.
        let mut s = masked(&[vec2(-1.0, -1.0), vec2(10.0, -1.0), vec2(10.0, 20.0), vec2(-1.0, 20.0)]);
        s.set_attachment(2, Some(None));
        s.set_attachment(3, Some(None));
        let mut list = DrawList::default();
        draw(&mut list, &mut Scratch::default(), &Transform2D::default(), &s, 0);
        let (v, _) = list.finish();
        let (area, _, max) = area_and_bounds(v);
        assert!((area - 20.0).abs() < 1e-3, "{area}");
        assert!((max.x - 10.0).abs() < 1e-4);
        // Without the mask the mesh draws whole.
        s.set_attachment(0, Some(None));
        s.update(0.0);
        list.clear();
        draw(&mut list, &mut Scratch::default(), &Transform2D::default(), &s, 0);
        let (v, _) = list.finish();
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn many_skeletons_drawn_in_chunks_match_drawing_them_one_by_one() {
        use crate::sprite_material::BlendMode;
        // Fewer under Miri, which runs it a thousand times slower.
        let count = if cfg!(miri) { 24 } else { 300 };
        crate::game::Game::new("chunks", 64, 64).run_headless(0, |ctx| {
            let plain = rig();
            let clipped = masked(&[
                vec2(0.0, 0.0),
                vec2(8.0, 0.0),
                vec2(8.0, 8.0),
                vec2(4.0, 3.0),
                vec2(0.0, 8.0),
            ]);
            for i in 0..count {
                let mut s = if i % 4 == 0 { clipped.clone() } else { plain.clone() };
                s.z = (i % 3) as f32;
                let bent = BonePose::at(vec2(10.0, 0.0)).with_rotation(i as f32 * 0.1);
                s.set_bone_pose(1, bent);
                if i % 7 == 0 {
                    s.set_attachment(0, Some(None));
                }
                let e = ctx.world.spawn((Transform2D::at(vec2(i as f32, (i % 13) as f32)), s));
                if i % 5 == 0 {
                    ctx.world.insert(e, BlendMode::Additive);
                }
            }
            draw_skeletons_with(ctx, Spread::Serial);
            let (v, b) = ctx.gfx.draw_list.finish();
            let (serial, batches) = (v.to_vec(), b.to_vec());
            ctx.gfx.draw_list.clear();
            assert!(batches.len() > 3, "several textures, looks and z");
            // Runs as for 1, 2, 3 and 8 threads, and one skeleton per run.
            let runs = [1, 2, 3, 8].map(|threads| run_length(count, threads));
            for per in runs.into_iter().chain([1, 7, count]) {
                draw_skeletons_with(ctx, Spread::Runs(per));
                let (v, b) = ctx.gfx.draw_list.finish();
                assert!(
                    v == serial.as_slice(),
                    "the same vertices in the same order ({per} per run)"
                );
                assert_eq!(b, batches.as_slice(), "{per} per run");
                ctx.gfx.draw_list.clear();
            }
            // And as the engine picks on this machine.
            draw_skeletons(ctx);
            assert!(ctx.gfx.draw_list.finish().0 == serial.as_slice());
            ctx.gfx.draw_list.clear();
        });
    }

    #[test]
    fn styles_apply_to_every_part_and_keep_the_mesh_flag_apart() {
        let s = rig();
        let mut list = DrawList::default();
        draw(&mut list, &mut Scratch::default(), &Transform2D::default(), &s, 5);
        let (_, batches) = list.finish();
        assert!(batches.iter().all(|b| b.material == 5 && b.mesh));
    }
}
