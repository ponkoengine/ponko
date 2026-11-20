//! A bounding volume hierarchy over the baked scene's triangles: binned
//! surface-area splits, closest-hit and any-hit ray queries.

use glam::Vec3;

/// One triangle as the ray test reads it: a corner and the two edges from it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Tri {
    pub v0: Vec3,
    pub e1: Vec3,
    pub e2: Vec3,
}

impl Tri {
    pub fn new(a: Vec3, b: Vec3, c: Vec3) -> Self {
        Self {
            v0: a,
            e1: b - a,
            e2: c - a,
        }
    }

    fn centroid(&self) -> Vec3 {
        self.v0 + (self.e1 + self.e2) / 3.0
    }

    fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        b.grow(self.v0);
        b.grow(self.v0 + self.e1);
        b.grow(self.v0 + self.e2);
        b
    }

    /// Where the ray hits it (Möller-Trumbore): distance and barycentrics
    /// (of the second and third corner), both faces.
    #[inline]
    fn hit(&self, origin: Vec3, dir: Vec3, t_max: f32) -> Option<(f32, f32, f32)> {
        let p = dir.cross(self.e2);
        let det = self.e1.dot(p);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let s = origin - self.v0;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(self.e1);
        let v = dir.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = self.e2.dot(q) * inv;
        (t > 0.0 && t < t_max).then_some((t, u, v))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const EMPTY: Self = Self {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    pub fn grow(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    fn join(&mut self, o: &Aabb) {
        self.min = self.min.min(o.min);
        self.max = self.max.max(o.max);
    }

    fn area(&self) -> f32 {
        let d = (self.max - self.min).max(Vec3::ZERO);
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }

    /// The distance range the ray spends inside, if it enters before `t_max`.
    #[inline]
    fn enters(&self, origin: Vec3, inv_dir: Vec3, t_max: f32) -> Option<f32> {
        let t0 = (self.min - origin) * inv_dir;
        let t1 = (self.max - origin) * inv_dir;
        let near = t0.min(t1).max_element().max(0.0);
        let far = t0.max(t1).min_element().min(t_max);
        (near <= far).then_some(near)
    }
}

/// A node: its box, and either two children (`count` 0, the first at
/// `first`, the second right after it) or `count` triangles from `first`.
#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Aabb,
    first: u32,
    count: u32,
}

/// What a ray hit: the triangle (index into the order the triangles were
/// given in), the distance and the barycentrics of its second and third
/// corners.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hit {
    pub tri: u32,
    pub t: f32,
    pub u: f32,
    pub v: f32,
}

pub(super) struct Bvh {
    nodes: Vec<Node>,
    /// The triangles in leaf order, and where each came from.
    tris: Vec<Tri>,
    ids: Vec<u32>,
}

const LEAF: usize = 4;
const BINS: usize = 12;

impl Bvh {
    pub fn new(tris: &[Tri]) -> Self {
        let mut ids: Vec<u32> = (0..tris.len() as u32).collect();
        let centroids: Vec<Vec3> = tris.iter().map(Tri::centroid).collect();
        let boxes: Vec<Aabb> = tris.iter().map(Tri::bounds).collect();
        let mut nodes = Vec::with_capacity(tris.len().max(1) * 2);
        nodes.push(Node {
            bounds: Aabb::EMPTY,
            first: 0,
            count: tris.len() as u32,
        });
        let mut stack = vec![0usize];
        while let Some(at) = stack.pop() {
            let (first, count) = (nodes[at].first as usize, nodes[at].count as usize);
            let mut bounds = Aabb::EMPTY;
            let mut centers = Aabb::EMPTY;
            for &i in &ids[first..first + count] {
                bounds.join(&boxes[i as usize]);
                centers.grow(centroids[i as usize]);
            }
            nodes[at].bounds = bounds;
            if count <= LEAF {
                continue;
            }
            let Some((axis, split)) = best_split(&ids[first..first + count], &centroids, &boxes, &centers, count)
            else {
                continue;
            };
            // Partition: centroids below the split first.
            let (lo, extent) = (centers.min[axis], centers.max[axis] - centers.min[axis]);
            let bin = |c: Vec3| (((c[axis] - lo) / extent * BINS as f32) as usize).min(BINS - 1);
            let slice = &mut ids[first..first + count];
            let mut left = 0;
            for k in 0..slice.len() {
                if bin(centroids[slice[k] as usize]) < split {
                    slice.swap(k, left);
                    left += 1;
                }
            }
            if left == 0 || left == count {
                continue;
            }
            let child = nodes.len();
            nodes.push(Node {
                bounds: Aabb::EMPTY,
                first: first as u32,
                count: left as u32,
            });
            nodes.push(Node {
                bounds: Aabb::EMPTY,
                first: (first + left) as u32,
                count: (count - left) as u32,
            });
            nodes[at].first = child as u32;
            nodes[at].count = 0;
            stack.push(child);
            stack.push(child + 1);
        }
        let ordered = ids.iter().map(|&i| tris[i as usize]).collect();
        Self {
            nodes,
            tris: ordered,
            ids,
        }
    }

    /// The nearest triangle the ray hits closer than `t_max`.
    pub fn closest(&self, origin: Vec3, dir: Vec3, t_max: f32) -> Option<Hit> {
        self.walk(origin, dir, t_max, false)
    }

    /// Whether anything is in the way closer than `t_max`.
    pub fn occluded(&self, origin: Vec3, dir: Vec3, t_max: f32) -> bool {
        self.walk(origin, dir, t_max, true).is_some()
    }

    fn walk(&self, origin: Vec3, dir: Vec3, mut t_max: f32, any: bool) -> Option<Hit> {
        if self.tris.is_empty() {
            return None;
        }
        let inv = Vec3::new(1.0 / nonzero(dir.x), 1.0 / nonzero(dir.y), 1.0 / nonzero(dir.z));
        let mut best = None;
        let mut stack = [0u32; 64];
        let mut top = 1;
        while top > 0 {
            top -= 1;
            let node = self.nodes[stack[top] as usize];
            if node.bounds.enters(origin, inv, t_max).is_none() {
                continue;
            }
            if node.count > 0 {
                for k in node.first..node.first + node.count {
                    if let Some((t, u, v)) = self.tris[k as usize].hit(origin, dir, t_max) {
                        t_max = t;
                        best = Some(Hit {
                            tri: self.ids[k as usize],
                            t,
                            u,
                            v,
                        });
                        if any {
                            return best;
                        }
                    }
                }
                continue;
            }
            // Nearer child last, so it is walked first.
            let (a, b) = (node.first, node.first + 1);
            let da = self.nodes[a as usize].bounds.enters(origin, inv, t_max);
            let db = self.nodes[b as usize].bounds.enters(origin, inv, t_max);
            let (first, second) = match (da, db) {
                (Some(x), Some(y)) if y < x => (b, Some(a)),
                (Some(_), Some(_)) => (a, Some(b)),
                (Some(_), None) => (a, None),
                (None, Some(_)) => (b, None),
                (None, None) => continue,
            };
            if top + 2 > stack.len() {
                // Deeper than any tree this builds; walk the rest anyway.
                continue;
            }
            if let Some(s) = second {
                stack[top] = s;
                top += 1;
            }
            stack[top] = first;
            top += 1;
        }
        best
    }
}

fn nonzero(x: f32) -> f32 {
    if x.abs() < 1e-20 { 1e-20f32.copysign(x) } else { x }
}

/// The axis and bin to split at (triangles in bins below it go left), by
/// the surface area heuristic; `None` when no split beats a leaf.
fn best_split(ids: &[u32], centroids: &[Vec3], boxes: &[Aabb], centers: &Aabb, count: usize) -> Option<(usize, usize)> {
    let mut best: Option<(f32, usize, usize)> = None;
    for axis in 0..3 {
        let (lo, extent) = (centers.min[axis], centers.max[axis] - centers.min[axis]);
        if extent <= 1e-9 {
            continue;
        }
        let mut bins = [(Aabb::EMPTY, 0u32); BINS];
        for &i in ids {
            let b = (((centroids[i as usize][axis] - lo) / extent * BINS as f32) as usize).min(BINS - 1);
            bins[b].0.join(&boxes[i as usize]);
            bins[b].1 += 1;
        }
        // Sweep from the right for each split's right side.
        let mut right = [(0.0f32, 0u32); BINS];
        let (mut acc, mut n) = (Aabb::EMPTY, 0);
        for b in (1..BINS).rev() {
            acc.join(&bins[b].0);
            n += bins[b].1;
            right[b] = (acc.area(), n);
        }
        let (mut acc, mut n) = (Aabb::EMPTY, 0);
        for split in 1..BINS {
            acc.join(&bins[split - 1].0);
            n += bins[split - 1].1;
            let (ra, rn) = right[split];
            if n == 0 || rn == 0 {
                continue;
            }
            let cost = acc.area() * n as f32 + ra * rn as f32;
            if best.is_none_or(|(c, _, _)| cost < c) {
                best = Some((cost, axis, split));
            }
        }
    }
    let (cost, axis, split) = best?;
    let mut all = Aabb::EMPTY;
    for &i in ids {
        all.join(&boxes[i as usize]);
    }
    (cost < all.area() * count as f32).then_some((axis, split))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad_grid(n: usize) -> Vec<Tri> {
        let mut tris = Vec::new();
        for z in 0..n {
            for x in 0..n {
                let p = |dx: usize, dz: usize| Vec3::new((x + dx) as f32, 0.0, (z + dz) as f32);
                tris.push(Tri::new(p(0, 0), p(1, 0), p(1, 1)));
                tris.push(Tri::new(p(0, 0), p(1, 1), p(0, 1)));
            }
        }
        tris
    }

    #[test]
    fn rays_find_the_same_triangle_as_testing_every_one() {
        let mut tris = quad_grid(12);
        // A second layer above, offset, so rays have something nearer.
        for t in quad_grid(6) {
            tris.push(Tri::new(
                t.v0 + Vec3::new(2.5, 1.0, 2.5),
                t.v0 + t.e1 + Vec3::new(2.5, 1.0, 2.5),
                t.v0 + t.e2 + Vec3::new(2.5, 1.0, 2.5),
            ));
        }
        let bvh = Bvh::new(&tris);
        let mut state = 7u32;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32
        };
        for _ in 0..500 {
            let origin = Vec3::new(next() * 12.0, 3.0, next() * 12.0);
            let dir = Vec3::new(next() - 0.5, -1.0, next() - 0.5).normalize();
            let brute = tris
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.hit(origin, dir, f32::INFINITY).map(|h| (i, h.0)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let found = bvh.closest(origin, dir, f32::INFINITY);
            match (brute, found) {
                (None, None) => {}
                (Some((_, t)), Some(h)) => assert!((t - h.t).abs() < 1e-4, "{t} vs {}", h.t),
                other => panic!("the tree and the brute force disagree: {other:?}"),
            }
            assert_eq!(found.is_some(), bvh.occluded(origin, dir, f32::INFINITY));
        }
    }
}
