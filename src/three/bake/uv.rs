//! Lightmap texture coordinates for meshes that don't bring their own: the
//! triangles split into charts by the way they face, each chart laid flat
//! and packed into one square with room around it.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use super::pack::shelf_pack;
use crate::three::MeshData;

/// Texels left between charts (and around the edge): enough for the
/// bilinear lookup and the baker's dilation never to reach a neighbour.
pub(super) const CHART_PADDING: u32 = 3;

impl MeshData {
    /// Gives the mesh lightmap texture coordinates ([`uvs1`](Self::uvs1)):
    /// what [`bake_lighting`](crate::render::bake_lighting) bakes light into
    /// and the renderer reads it back from. glTF files made in a modelling
    /// program often bring a second UV set made for this; use this for
    /// meshes without one (the built-in shapes, generated geometry).
    ///
    /// The triangles are split into charts by the way they face (one of the
    /// six axis directions), each laid flat and packed into the 0..1
    /// square, `texels_per_unit` texels for each unit of the mesh's own
    /// size with three texels between charts. Bake with the same
    /// `texels_per_unit` ([`LightBakeSettings::texels_per_unit`](crate::render::LightBakeSettings::texels_per_unit))
    /// so the charts keep that room. Corners shared between charts are
    /// split (the mesh may get more corners); other data per corner comes
    /// along. Returns the side of the square, in texels.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut cube = ponko::render::MeshData::cube(1.0);
    /// let side = cube.generate_lightmap_uvs(8.0);
    /// assert_eq!(cube.uvs1.len(), cube.positions.len());
    /// assert!(cube.uvs1.iter().all(|uv| (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)));
    /// assert!(side >= 16, "six 8x8 faces need at least 16 texels across");
    /// ```
    pub fn generate_lightmap_uvs(&mut self, texels_per_unit: f32) -> u32 {
        let tpu = if texels_per_unit.is_finite() && texels_per_unit > 0.0 {
            texels_per_unit
        } else {
            1.0
        };
        let tris: Vec<[u32; 3]> = self
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .copied()
            .filter(|t| t.iter().all(|&i| (i as usize) < self.positions.len()))
            .collect();
        if tris.is_empty() {
            self.uvs1 = vec![Vec2::ZERO; self.positions.len()];
            return 1;
        }
        let p = |i: u32| self.positions[i as usize];
        // The axis direction each triangle faces most: 0..6 for +X, -X, +Y, -Y, +Z, -Z.
        let facing: Vec<usize> = tris
            .iter()
            .map(|t| {
                let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
                let a = n.abs();
                let axis = if a.x >= a.y && a.x >= a.z {
                    0
                } else if a.y >= a.z {
                    1
                } else {
                    2
                };
                axis * 2 + usize::from(n[axis] < 0.0)
            })
            .collect();
        // Charts: triangles facing the same way that share an edge (by
        // position, so corners split for their normals still join).
        let key = |v: Vec3| (v * 1e4).round().as_ivec3();
        let mut parent: Vec<usize> = (0..tris.len()).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let mut edges: HashMap<(glam::IVec3, glam::IVec3, usize), usize> = HashMap::new();
        for (ti, t) in tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (key(p(t[k])), key(p(t[(k + 1) % 3])));
                let edge = if (a.x, a.y, a.z) < (b.x, b.y, b.z) {
                    (a, b, facing[ti])
                } else {
                    (b, a, facing[ti])
                };
                match edges.get(&edge) {
                    Some(&other) => {
                        let (ra, rb) = (root(&mut parent, ti), root(&mut parent, other));
                        parent[ra] = rb;
                    }
                    None => {
                        edges.insert(edge, ti);
                    }
                }
            }
        }
        let mut chart_of_root: HashMap<usize, usize> = HashMap::new();
        let mut chart_tris: Vec<Vec<usize>> = Vec::new();
        for ti in 0..tris.len() {
            let r = root(&mut parent, ti);
            let c = *chart_of_root.entry(r).or_insert_with(|| {
                chart_tris.push(Vec::new());
                chart_tris.len() - 1
            });
            chart_tris[c].push(ti);
        }
        // Each chart flat on its axis plane, in texels.
        let flat = |v: Vec3, f: usize| -> Vec2 {
            let s = if f.is_multiple_of(2) { 1.0 } else { -1.0 };
            match f / 2 {
                0 => Vec2::new(-v.z * s, -v.y),
                1 => Vec2::new(v.x, v.z * s),
                _ => Vec2::new(v.x * s, -v.y),
            }
        };
        let mut chart_min = Vec::with_capacity(chart_tris.len());
        let mut sizes = Vec::with_capacity(chart_tris.len());
        for list in &chart_tris {
            let f = facing[list[0]];
            let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
            for &ti in list {
                for &i in &tris[ti] {
                    let q = flat(p(i), f) * tpu;
                    lo = lo.min(q);
                    hi = hi.max(q);
                }
            }
            chart_min.push(lo);
            let size = (hi - lo).ceil().max(Vec2::ONE);
            sizes.push((size.x as u32 + CHART_PADDING, size.y as u32 + CHART_PADDING));
        }
        // The smallest square (growing by an eighth) the charts pack into.
        let area: u64 = sizes.iter().map(|&(w, h)| u64::from(w) * u64::from(h)).sum();
        let widest = sizes.iter().map(|s| s.0.max(s.1)).max().unwrap_or(1);
        let mut side = ((area as f64).sqrt().ceil() as u32).max(widest) + CHART_PADDING;
        let places = loop {
            if let Some(places) = shelf_pack(&sizes, side - CHART_PADDING, side - CHART_PADDING) {
                break places;
            }
            side += (side / 8).max(1);
        };
        // New corners: one per (old corner, chart).
        let mut remap: HashMap<(u32, usize), u32> = HashMap::new();
        let mut order: Vec<u32> = Vec::new();
        let mut uvs1: Vec<Vec2> = Vec::new();
        let mut indices = Vec::with_capacity(tris.len() * 3);
        for (c, list) in chart_tris.iter().enumerate() {
            let f = facing[list[0]];
            let (px, py, _) = places[c];
            let corner = Vec2::new(px as f32, py as f32) + Vec2::splat(CHART_PADDING as f32);
            for &ti in list {
                for &i in &tris[ti] {
                    let next = order.len() as u32;
                    let at = *remap.entry((i, c)).or_insert_with(|| {
                        order.push(i);
                        let texel = corner + flat(p(i), f) * tpu - chart_min[c];
                        uvs1.push(texel / side as f32);
                        next
                    });
                    indices.push(at);
                }
            }
        }
        self.remap_corners(&order);
        self.indices = indices;
        self.uvs1 = uvs1;
        side
    }

    /// Keeps corner `order[k]` as corner `k` in every per-corner list.
    fn remap_corners(&mut self, order: &[u32]) {
        fn pick<T: Clone>(list: &mut Vec<T>, order: &[u32]) {
            if !list.is_empty() {
                *list = order.iter().map(|&i| list[i as usize].clone()).collect();
            }
        }
        let n = self.positions.len();
        let normals = self.normals_or_computed().into_owned();
        self.normals = normals;
        pick(&mut self.positions, order);
        pick(&mut self.normals, order);
        if self.uvs.len() == n {
            pick(&mut self.uvs, order);
        }
        if self.colors.len() == n {
            pick(&mut self.colors, order);
        }
        if self.joints.len() == n {
            pick(&mut self.joints, order);
        }
        if self.weights.len() == n {
            pick(&mut self.weights, order);
        }
        for target in &mut self.morph_targets {
            if target.positions.len() == n {
                pick(&mut target.positions, order);
            }
            if target.normals.len() == n {
                pick(&mut target.normals, order);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many texel centers lie inside two triangles (not on a shared edge).
    fn overlapping_texels(mesh: &MeshData, side: u32) -> usize {
        let mut owner = vec![u32::MAX; (side * side) as usize];
        let mut overlaps = 0;
        for (ti, t) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            let q = |i: u32| mesh.uvs1[i as usize] * side as f32;
            let (a, b, c) = (q(t[0]), q(t[1]), q(t[2]));
            let lo = a.min(b).min(c).floor().max(Vec2::ZERO);
            let hi = a.max(b).max(c).ceil().min(Vec2::splat(side as f32));
            for y in lo.y as u32..hi.y as u32 {
                for x in lo.x as u32..hi.x as u32 {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let e = |a: Vec2, b: Vec2| (b - a).perp_dot(p - a);
                    let (w0, w1, w2) = (e(b, c), e(c, a), e(a, b));
                    let eps = 1e-3;
                    let inside = (w0 > eps && w1 > eps && w2 > eps) || (w0 < -eps && w1 < -eps && w2 < -eps);
                    if inside {
                        let slot = &mut owner[(y * side + x) as usize];
                        if *slot != u32::MAX {
                            overlaps += 1;
                        }
                        *slot = ti as u32;
                    }
                }
            }
        }
        overlaps
    }

    #[test]
    fn generated_lightmap_uvs_keep_charts_apart() {
        let mut sphere = MeshData::sphere(1.0, 16);
        let side = sphere.generate_lightmap_uvs(16.0);
        assert!(sphere.check().is_ok());
        assert_eq!(sphere.uvs1.len(), sphere.positions.len());
        assert!(
            sphere
                .uvs1
                .iter()
                .all(|uv| uv.x > 0.0 && uv.x < 1.0 && uv.y > 0.0 && uv.y < 1.0)
        );
        assert_eq!(overlapping_texels(&sphere, side), 0, "no texel belongs to two charts");
    }

    #[test]
    fn generated_lightmap_uvs_give_each_cube_face_its_texels() {
        let mut cube = MeshData::cube(2.0);
        let side = cube.generate_lightmap_uvs(4.0);
        // Six 8x8 faces with 3 texels of room each: 11x11 each, in a square.
        assert!((33..=48).contains(&side), "side {side}");
        for face in cube.indices.as_chunks::<6>().0 {
            let (lo, hi) = face.iter().fold((Vec2::splat(9.0), Vec2::splat(-9.0)), |(lo, hi), &i| {
                (lo.min(cube.uvs1[i as usize]), hi.max(cube.uvs1[i as usize]))
            });
            let texels = (hi - lo) * side as f32;
            assert!((texels - Vec2::splat(8.0)).abs().max_element() < 1e-3, "{texels}");
        }
        assert_eq!(cube.indices.len(), 36);
        assert_eq!(
            cube.positions.len(),
            24,
            "a cube's faces already have corners of their own"
        );
    }
}
