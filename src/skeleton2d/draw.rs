//! Drawing skeletons into the sprite draw list: every part is a triangle
//! list in the same list, batches and `z` order as sprites.

use glam::{Affine2, Vec2};

use super::Skeleton;
use super::data::Attachment;
use crate::color::Color;
use crate::ctx::Ctx;
use crate::ecs::Transform2D;
use crate::render::batch::{DrawList, SpriteInstance};

/// The quad corners in the order sprites draw them (two triangles).
const CORNERS: [Vec2; 6] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(0.0, 1.0),
    Vec2::new(0.0, 1.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(1.0, 1.0),
];

/// Queues every skeleton's visible parts. Out of line and cheap when the
/// world has no skeleton (one archetype count).
#[inline(never)]
pub(crate) fn draw_skeletons(ctx: &mut Ctx) {
    use crate::sprite_material::{BlendMode, SpriteMaterial, style_key};
    let mut query = ctx
        .world
        .query::<(&Transform2D, &mut Skeleton, Option<&BlendMode>, Option<&SpriteMaterial>)>();
    if query.count() == 0 {
        return;
    }
    let list = &mut ctx.gfx.draw_list;
    query.for_each(|(t, s, b, m)| {
        let blend = b.copied().unwrap_or(if s.data().premultiplied {
            BlendMode::Premultiplied
        } else {
            BlendMode::default()
        });
        let style = style_key(blend, m.copied());
        draw(list, t, s, style);
    });
}

fn tint(a: Color, b: Color) -> Color {
    Color::rgba(a.r * b.r, a.g * b.g, a.b * b.b, a.a * b.a)
}

/// Queues skeleton `s` at transform `t` with look `style`.
pub(crate) fn draw(list: &mut DrawList, t: &Transform2D, s: &mut Skeleton, style: u32) {
    let entity = Affine2::from_scale_angle_translation(t.scale, t.rotation, t.position);
    let data = s.data().clone();
    let z = s.z;
    let mut deformed = std::mem::take(&mut s.deformed);
    for k in 0..s.draw_order().len() {
        let slot = usize::from(s.draw_order()[k]);
        let Some(a) = s.shown(slot) else {
            continue;
        };
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
                let verts = CORNERS.map(|c| {
                    let at = m.transform_point2((c - p.pivot) * p.size);
                    // A region packed a quarter turn counter-clockwise: the
                    // image's top edge runs up its left side.
                    let st = if p.rotated { Vec2::new(c.y, 1.0 - c.x) } else { c };
                    SpriteInstance::vertex(at.into(), (uv0 + st * uvs).into(), c.into(), color)
                });
                list.push_mesh(p.texture.id, style, z, &verts);
            }
            Attachment::Mesh(mesh) => {
                let color = tint(base, mesh.color).to_rgba8();
                deformed.clear();
                for influences in named.binds.as_chunks::<4>().0 {
                    let mut at = Vec2::ZERO;
                    for &(b, local, w) in influences {
                        if w > 0.0 {
                            at += s.bone_matrix(usize::from(b)).transform_point2(local) * w;
                        }
                    }
                    deformed.push(entity.transform_point2(at));
                }
                let (min, extent) = named.bounds;
                for tri in mesh.triangles.as_chunks::<3>().0 {
                    let verts = [0, 1, 2].map(|i| {
                        let v = usize::from(tri[i]);
                        let rest = &mesh.vertices[v];
                        let local = (rest.position - min) / extent;
                        SpriteInstance::vertex(deformed[v].into(), rest.uv.into(), local.into(), color)
                    });
                    list.push_mesh(mesh.texture.id, style, z, &verts);
                }
            }
        }
    }
    s.deformed = deformed;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::{Vec2, vec2};

    use super::*;
    use crate::render::batch::f16_bits;
    use crate::skeleton2d::{BonePose, MeshVertex, SkeletonData, SkinnedMesh, SpritePart};
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
        draw(&mut list, &Transform2D::at(vec2(100.0, 0.0)), &mut s, 0);
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
        draw(&mut list, &Transform2D::at(vec2(100.0, 0.0)), &mut s, 0);
        let (v, _) = list.finish();
        // Bound at rest: (20, 0) is (10, 0) in bend; (10, 4) is (0, 4) in bend.
        let bent_tip = vec2(110.0, 10.0);
        let split = (vec2(110.0, 4.0) + vec2(110.0 - 4.0, 0.0)) * 0.5;
        near(&positions(&v[..3]), &[vec2(100.0, 0.0), bent_tip, split]);
    }

    #[test]
    fn mesh_vertices_carry_their_uv_and_local_position_and_no_size() {
        let mut s = rig();
        let mut list = DrawList::default();
        draw(&mut list, &Transform2D::default(), &mut s, 0);
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
        draw(&mut list, &Transform2D::default(), &mut s, 0);
        list.push(7, 0.0, sprite);
        let (v, batches) = list.finish();
        let kinds: Vec<(bool, u32)> = batches.iter().map(|b| (b.mesh, b.end - b.start)).collect();
        assert_eq!(kinds, [(false, 1), (true, 3), (true, 6), (false, 1)]);
        assert_eq!(v.len(), 11);
        // Hiding a slot draws nothing for it; reordering swaps the parts.
        s.set_attachment(0, Some(None));
        list.clear();
        draw(&mut list, &Transform2D::default(), &mut s, 0);
        assert_eq!(list.finish().0.len(), 6);
    }

    #[test]
    fn styles_apply_to_every_part_and_keep_the_mesh_flag_apart() {
        let mut s = rig();
        let mut list = DrawList::default();
        draw(&mut list, &Transform2D::default(), &mut s, 5);
        let (_, batches) = list.finish();
        assert!(batches.iter().all(|b| b.material == 5 && b.mesh));
    }
}
