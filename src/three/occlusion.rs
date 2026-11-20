//! Occlusion culling against an earlier frame's depth.
//!
//! The renderer shrinks the camera's depth buffer to a small grid that keeps
//! the farthest depth of each cell, and reads it back without waiting for the
//! GPU: it arrives one to three frames late. [`OcclusionGrid`] builds a
//! pyramid of coarser grids from it, each cell again the farthest of the four
//! below, and tests a model's bounding box against the level where the box
//! covers at most a few cells: if the box's nearest point is behind the
//! farthest depth everywhere it covers, something drawn in that frame hid it.
//! The depth is reverse-Z (1 near, 0 far): the farthest depth is the
//! smallest, and behind means smaller.
//!
//! The test uses the view the depth was drawn from, so turning the camera
//! changes nothing; only moving it does (things behind an occluder shift
//! against it). The box is grown by how far the camera moved since, and
//! anything outside that frame's view counts as seen.

use glam::{Mat4, Vec2, Vec3};

/// Farthest-depth pyramid of an earlier frame's camera view.
#[derive(Clone, Debug, Default)]
pub(crate) struct OcclusionGrid {
    /// The view-projection the depth was drawn with.
    pub view_proj: Mat4,
    /// Where the camera was.
    pub camera: Vec3,
    /// Each level's size and depths (reverse-Z: 1 near, 0 far), row by row from the top;
    /// level 0 is the grid the GPU made, each next one half its size.
    levels: Vec<(u32, u32, Vec<f32>)>,
}

/// The widest box, in cells of the level it is tested on.
const MAX_CELLS: u32 = 4;

impl OcclusionGrid {
    pub(crate) fn base(&self) -> Option<&(u32, u32, Vec<f32>)> {
        self.levels.first()
    }
    /// A grid of `width` x `height` farthest depths, row by row from the top.
    #[cfg(test)]
    pub fn new(view_proj: Mat4, camera: Vec3, width: u32, height: u32, depth: Vec<f32>) -> Self {
        let mut grid = Self::default();
        grid.rebuild(view_proj, camera, width, height, &depth);
        grid
    }

    /// Makes this the grid [`new`](Self::new) makes, in the storage it
    /// already has: a grid read back every frame allocates nothing once
    /// its levels have grown.
    pub fn rebuild(&mut self, view_proj: Mat4, camera: Vec3, width: u32, height: u32, depth: &[f32]) {
        assert_eq!(depth.len(), (width * height) as usize);
        self.view_proj = view_proj;
        self.camera = camera;
        let levels = &mut self.levels;
        if levels.is_empty() {
            levels.push((0, 0, Vec::new()));
        }
        levels[0].0 = width;
        levels[0].1 = height;
        levels[0].2.clear();
        levels[0].2.extend_from_slice(depth);
        let mut n = 1;
        loop {
            let (w, h) = (levels[n - 1].0, levels[n - 1].1);
            if w <= 1 && h <= 1 {
                break;
            }
            let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
            if levels.len() <= n {
                levels.push((0, 0, Vec::new()));
            }
            let (done, rest) = levels.split_at_mut(n);
            let below = &done[n - 1].2;
            let (lw, lh, next) = &mut rest[0];
            (*lw, *lh) = (nw, nh);
            next.clear();
            next.resize((nw * nh) as usize, 0.0);
            for y in 0..nh {
                for x in 0..nw {
                    let mut far = 1.0f32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(w - 1), (2 * y + dy).min(h - 1));
                        far = far.min(below[(sy * w + sx) as usize]);
                    }
                    next[(y * nw + x) as usize] = far;
                }
            }
            n += 1;
        }
        levels.truncate(n);
    }

    /// Whether a box (`min`, `max` in world space) was hidden. `camera`:
    /// where the camera is now; the box grows by how far it moved.
    pub fn hides(&self, min: Vec3, max: Vec3, camera: Vec3) -> bool {
        let grow = Vec3::splat(camera.distance(self.camera));
        let (min, max) = (min - grow, max + grow);
        let (mut lo, mut hi, mut nearest) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN), f32::MIN);
        for i in 0..8 {
            let corner = Vec3::new(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            );
            let clip = self.view_proj * corner.extend(1.0);
            // A corner at or behind the camera: the box reaches it.
            if clip.w <= 1e-5 {
                return false;
            }
            let ndc = Vec2::new(clip.x, clip.y) / clip.w;
            lo = lo.min(ndc);
            hi = hi.max(ndc);
            nearest = nearest.max(clip.z / clip.w);
        }
        // Partly outside that frame's view (or nearer than its near plane):
        // no depth there to go by.
        if lo.x < -1.0 || lo.y < -1.0 || hi.x > 1.0 || hi.y > 1.0 || nearest > 1.0 {
            return false;
        }
        let (w0, h0, _) = &self.levels[0];
        // Cells of level 0 the box covers (y down from the top).
        let x0 = (lo.x * 0.5 + 0.5) * *w0 as f32;
        let x1 = (hi.x * 0.5 + 0.5) * *w0 as f32;
        let y0 = (0.5 - hi.y * 0.5) * *h0 as f32;
        let y1 = (0.5 - lo.y * 0.5) * *h0 as f32;
        let span = (x1 - x0).max(y1 - y0).max(1.0);
        let level = ((span / MAX_CELLS as f32).log2().ceil().max(0.0) as usize).min(self.levels.len() - 1);
        let (w, h, depth) = &self.levels[level];
        let scale = 1.0 / (1u32 << level) as f32;
        let cell = |v: f32, n: u32| ((v * scale).floor().max(0.0) as u32).min(n - 1);
        let (cx0, cx1) = (cell(x0, *w), cell(x1, *w));
        let (cy0, cy1) = (cell(y0, *h), cell(y1, *h));
        for y in cy0..=cy1 {
            for x in cx0..=cx1 {
                if nearest >= depth[(y * w + x) as usize] {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> (Mat4, Vec3) {
        let eye = Vec3::new(0.0, 0.0, 10.0);
        let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
        let proj = super::super::reverse_z_perspective(60f32.to_radians(), 1.0, 0.1, 100.0);
        (proj * view, eye)
    }

    /// The depth of a point `distance` in front of the camera.
    fn depth_at(view_proj: Mat4, eye: Vec3, distance: f32) -> f32 {
        let p = view_proj * (eye - Vec3::Z * distance).extend(1.0);
        p.z / p.w
    }

    /// A 64 x 64 grid: a wall 5 units in front over the middle half, far
    /// plane elsewhere.
    fn wall_grid() -> OcclusionGrid {
        let (m, eye) = camera();
        let wall = depth_at(m, eye, 5.0);
        let depth = (0..64 * 64)
            .map(|i| {
                let (x, y) = (i % 64, i / 64);
                if (16..48).contains(&x) && (16..48).contains(&y) {
                    wall
                } else {
                    0.0
                }
            })
            .collect();
        OcclusionGrid::new(m, eye, 64, 64, depth)
    }

    #[test]
    fn a_box_behind_the_wall_is_hidden() {
        let grid = wall_grid();
        let eye = grid.camera;
        assert!(grid.hides(Vec3::splat(-0.5), Vec3::splat(0.5), eye));
    }

    #[test]
    fn a_box_in_front_of_the_wall_is_seen() {
        let grid = wall_grid();
        let eye = grid.camera;
        assert!(!grid.hides(Vec3::new(-0.2, -0.2, 6.0), Vec3::new(0.2, 0.2, 6.5), eye));
    }

    #[test]
    fn a_box_beside_the_wall_is_seen() {
        let grid = wall_grid();
        let eye = grid.camera;
        // Behind the wall's depth, but off to the side where nothing hides it.
        assert!(!grid.hides(Vec3::new(4.0, -0.5, -0.5), Vec3::new(5.0, 0.5, 0.5), eye));
    }

    #[test]
    fn a_box_partly_out_of_view_or_behind_the_camera_is_seen() {
        let grid = wall_grid();
        let eye = grid.camera;
        assert!(!grid.hides(Vec3::new(-0.5, -0.5, 8.0), Vec3::new(0.5, 0.5, 12.0), eye));
        assert!(!grid.hides(Vec3::new(-100.0, -0.5, -0.5), Vec3::new(0.5, 0.5, 0.5), eye));
    }

    #[test]
    fn a_box_reaching_past_the_walls_edge_is_seen() {
        let grid = wall_grid();
        let eye = grid.camera;
        // The wall covers x in about +-1.44 at z = 5; behind it at z = 0,
        // its shadow reaches about +-2.9. This box sticks out of it.
        assert!(!grid.hides(Vec3::new(0.0, -0.5, -0.5), Vec3::new(3.5, 0.5, 0.5), eye));
    }

    #[test]
    fn moving_the_camera_grows_the_box() {
        let grid = wall_grid();
        // A small box just inside the wall's shadow.
        let (min, max) = (Vec3::new(2.0, -0.2, -0.2), Vec3::new(2.4, 0.2, 0.2));
        assert!(grid.hides(min, max, grid.camera));
        assert!(!grid.hides(min, max, grid.camera + Vec3::X * 1.0));
    }

    #[test]
    fn the_pyramid_keeps_the_farthest_depth() {
        let grid = OcclusionGrid::new(
            Mat4::IDENTITY,
            Vec3::ZERO,
            3,
            3,
            vec![0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2, 0.1],
        );
        // Reverse-Z: the farthest is the smallest.
        assert_eq!(grid.levels.len(), 3);
        assert_eq!(grid.levels[1].2, vec![0.5, 0.4, 0.2, 0.1]);
        assert_eq!(grid.levels[2].2, vec![0.1]);
    }
}
