//! Flow fields: one shortest-path computation that every agent shares.
//!
//! **Maturity: tested**, from the guide chapter `pathfinding.md`. Game: none (the Legions showcase
//! uses it). Tests: `tests/flow.rs`. Platforms: Linux, macOS and Windows, headless (the workspace
//! tests in the list of checks). Graphics: not involved. Missing: a reference game that uses it.
//!
//! A [`FlowField`] holds, for every cell of a [`NavGrid`], the way to the
//! nearest goal. Computing it costs about as much as one pathfinding search
//! over the whole map; after that, each of a million agents asks
//! [`FlowField::direction_at`], a lookup, from inside `par_for_each`.

use glam::{UVec2, Vec2};
use rayon::prelude::*;

/// A grid of walking costs: how hard each cell is to enter.
///
/// `1` is normal ground, higher is slower (mud, forest: up to 255), and
/// [`NavGrid::WALL`] can't be entered. Cell `(0, 0)`'s top-left corner is at
/// `origin` in world units.
///
/// ```
/// use ponko::prelude::*;
/// let mut nav = ponko::flow::NavGrid::new(64, 64, 16.0); // 64 × 64 cells of 16 units
/// nav.set_cost(10, 3, ponko::flow::NavGrid::WALL);
/// nav.set_cost(11, 3, 4); // mud: 4 times slower
/// assert_eq!(nav.cost(10, 3), Some(ponko::flow::NavGrid::WALL));
/// assert_eq!(nav.cell_at(vec2(170.0, 50.0)), Some(uvec2(10, 3)));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct NavGrid {
    size: UVec2,
    cell_size: f32,
    /// World position of cell (0, 0)'s top-left corner. Default (0, 0).
    pub origin: Vec2,
    costs: Vec<u8>,
}

impl NavGrid {
    /// A cell that can't be entered.
    pub const WALL: u8 = 0;

    /// `width` × `height` cells of normal ground (cost 1), each `cell_size`
    /// world units wide.
    ///
    /// # Panics
    /// If a side is 0, the grid has more than 2^28 cells, or `cell_size`
    /// isn't positive.
    #[track_caller]
    #[must_use]
    pub fn new(width: u32, height: u32, cell_size: f32) -> Self {
        assert!(
            width > 0 && height > 0 && (width as u64) * (height as u64) <= 1 << 28,
            "NavGrid::new: {width} x {height} cells (each side at least 1, at most 2^28 cells)"
        );
        assert!(
            cell_size > 0.0 && cell_size.is_finite(),
            "NavGrid::new: cell_size must be positive, got {cell_size}"
        );
        Self {
            size: UVec2::new(width, height),
            cell_size,
            origin: Vec2::ZERO,
            costs: vec![1; width as usize * height as usize],
        }
    }

    /// Width and height in cells.
    #[must_use]
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// Side of one cell in world units.
    #[must_use]
    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// Sets the cost of entering cell (x, y); cells outside the grid are ignored.
    pub fn set_cost(&mut self, x: u32, y: u32, cost: u8) {
        if x < self.size.x && y < self.size.y {
            self.costs[(y * self.size.x + x) as usize] = cost;
        }
    }

    /// The cost of cell (x, y), or `None` outside the grid.
    #[must_use]
    pub fn cost(&self, x: u32, y: u32) -> Option<u8> {
        (x < self.size.x && y < self.size.y).then(|| self.costs[(y * self.size.x + x) as usize])
    }

    /// Every cell's cost, row by row, for filling a map at once.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut nav = ponko::flow::NavGrid::new(4, 4, 1.0);
    /// nav.costs_mut()[..4].fill(ponko::flow::NavGrid::WALL); // top row
    /// assert_eq!(nav.cost(2, 0), Some(ponko::flow::NavGrid::WALL));
    /// ```
    pub fn costs_mut(&mut self) -> &mut [u8] {
        &mut self.costs
    }

    /// Whether a world point is inside the grid on a cell that isn't a wall.
    #[must_use]
    pub fn walkable(&self, world: Vec2) -> bool {
        self.cell_at(world)
            .is_some_and(|c| self.costs[self.index(c)] != Self::WALL)
    }

    /// The cell under a world point, or `None` outside the grid.
    #[must_use]
    pub fn cell_at(&self, world: Vec2) -> Option<UVec2> {
        let c = ((world - self.origin) / self.cell_size).floor();
        (c.x >= 0.0 && c.y >= 0.0 && c.x < self.size.x as f32 && c.y < self.size.y as f32)
            .then(|| UVec2::new(c.x as u32, c.y as u32))
    }

    /// The world position of a cell's center.
    #[must_use]
    pub fn center_of(&self, cell: UVec2) -> Vec2 {
        self.origin + (cell.as_vec2() + 0.5) * self.cell_size
    }

    fn index(&self, cell: UVec2) -> usize {
        (cell.y * self.size.x + cell.x) as usize
    }
}

/// Neighbor offsets and unit directions, clockwise from east. Odd ones are diagonal.
const STEPS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const S: f32 = std::f32::consts::FRAC_1_SQRT_2;
const DIRS: [Vec2; 9] = [
    Vec2::new(1.0, 0.0),
    Vec2::new(S, S),
    Vec2::new(0.0, 1.0),
    Vec2::new(-S, S),
    Vec2::new(-1.0, 0.0),
    Vec2::new(-S, -S),
    Vec2::new(0.0, -1.0),
    Vec2::new(S, -S),
    Vec2::ZERO,
];
/// Direction index meaning "stay": at a goal, unreachable, or a wall.
const NONE: u8 = 8;
/// Distance units per cell: straight steps cost 10, diagonal 14 (≈ 10·√2).
const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;
const UNREACHED: u32 = u32::MAX;

/// The way to the nearest goal from every cell of a [`NavGrid`].
///
/// Compute it when goals or the map change, then ask it from anywhere,
/// including `par_for_each`. Agents move diagonally around corners but never
/// cut through them.
///
/// ```
/// use ponko::prelude::*;
/// let mut nav = ponko::flow::NavGrid::new(32, 32, 10.0);
/// for y in 0..30 { nav.set_cost(16, y, ponko::flow::NavGrid::WALL); } // a wall with a gap at the bottom
///
/// let mut flow = ponko::flow::FlowField::new();
/// flow.compute(&nav, &[vec2(300.0, 15.0)]); // goal on the right
///
/// let dir = flow.direction_at(vec2(100.0, 15.0)); // left of the wall
/// assert!(dir.y > 0.0, "heads down toward the gap first");
/// assert!(flow.distance_at(vec2(100.0, 15.0)).unwrap() > 200.0);
/// ```
#[derive(Debug, Clone, Default)]
pub struct FlowField {
    size: UVec2,
    cell_size: f32,
    origin: Vec2,
    /// Row length of the padded arrays: `size.x + 2` (a wall border all
    /// around, so neighbor lookups need no bounds checks).
    stride: usize,
    /// Padded copy of the grid's costs.
    costs: Vec<u8>,
    /// Distance to the nearest goal in tenths of a cell; `UNREACHED` if none. Padded.
    dist: Vec<u32>,
    /// Index into `DIRS` per cell. Padded.
    dir: Vec<u8>,
    /// Dial's bucket queue, kept between computes.
    buckets: Vec<Vec<u32>>,
}

impl FlowField {
    /// An empty field: every direction is zero until [`compute`](Self::compute).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Recomputes the field for `goals` (world points; ones outside the grid
    /// or on walls are skipped). Reuses its memory, so recomputing every few
    /// ticks allocates nothing once the sizes settle.
    ///
    /// Costs O(cells); see the guide's pathfinding chapter for timings.
    pub fn compute(&mut self, nav: &NavGrid, goals: &[Vec2]) {
        let (w, h) = (nav.size.x as usize, nav.size.y as usize);
        let stride = w + 2;
        let n = stride * (h + 2);
        self.size = nav.size;
        self.cell_size = nav.cell_size;
        self.origin = nav.origin;
        self.stride = stride;
        self.costs.clear();
        self.costs.resize(n, NavGrid::WALL);
        for (row, src) in self.costs[stride..].chunks_mut(stride).zip(nav.costs.chunks(w)) {
            row[1..=w].copy_from_slice(src);
        }
        self.dist.clear();
        self.dist.resize(n, UNREACHED);
        self.dir.clear();
        self.dir.resize(n, NONE);

        // Neighbor offsets in the padded arrays, and for diagonals the two
        // side cells that must be open (no corner cutting).
        let s = stride as isize;
        let off: [isize; 8] = STEPS.map(|(dx, dy)| dy as isize * s + dx as isize);
        let sides: [(isize, isize); 8] = STEPS.map(|(dx, dy)| (dx as isize, dy as isize * s));

        // Dial's algorithm: edge weights are small integers, so a ring of
        // buckets indexed by distance replaces a heap.
        // A power of two above the largest step (14 · 255), so slots are a mask.
        let ring = ((DIAGONAL * 255 + 1) as usize).next_power_of_two();
        let mask = ring - 1;
        if self.buckets.len() != ring {
            self.buckets = (0..ring).map(|_| Vec::new()).collect();
        }
        for b in &mut self.buckets {
            b.clear();
        }
        let mut pending = 0usize;
        for &g in goals {
            if let Some(c) = nav.cell_at(g) {
                let i = (c.y as usize + 1) * stride + c.x as usize + 1;
                if self.costs[i] != NavGrid::WALL && self.dist[i] != 0 {
                    self.dist[i] = 0;
                    self.buckets[0].push(i as u32);
                    pending += 1;
                }
            }
        }

        let costs = &self.costs;
        let dist = &mut self.dist;
        let mut d = 0u32;
        while pending > 0 {
            let slot = d as usize & mask;
            while let Some(i) = self.buckets[slot].pop() {
                pending -= 1;
                let i = i as usize;
                if dist[i] != d {
                    continue; // a shorter way was found after this entry was queued
                }
                for k in 0..8 {
                    // In bounds: `i` is an inner cell, so every neighbor is in the padding at worst.
                    let j = (i as isize + off[k]) as usize;
                    let cost = costs[j] as u32;
                    if cost == 0 {
                        continue;
                    }
                    let step = if k % 2 == 1 {
                        let (sx, sy) = sides[k];
                        if costs[(i as isize + sx) as usize] == 0 || costs[(i as isize + sy) as usize] == 0 {
                            continue;
                        }
                        DIAGONAL
                    } else {
                        STRAIGHT
                    };
                    let nd = d + step * cost;
                    if nd < dist[j] {
                        dist[j] = nd;
                        self.buckets[nd as usize & mask].push(j as u32);
                        pending += 1;
                    }
                }
            }
            d += 1;
        }

        // Directions: each reached cell points at its closest neighbor, in parallel.
        let dist = &self.dist;
        let dir = &mut self.dir;
        // On the frame pool (on the web, its workers, not the page's
        // thread), as parallel queries run.
        crate::frame_pool::run(|| {
            crate::profile_scope!("flow/shared_directions");
            dir[stride..n - stride]
                .par_chunks_mut(stride)
                .enumerate()
                .for_each(|(y, row)| {
                    let base = (y + 1) * stride;
                    for (x, out) in row.iter_mut().enumerate().take(w + 1).skip(1) {
                        let i = base + x;
                        let here = dist[i];
                        if here == 0 || here == UNREACHED {
                            continue;
                        }
                        let mut best = (here, NONE);
                        for k in 0..8 {
                            if k % 2 == 1 {
                                let (sx, sy) = sides[k];
                                if costs[(i as isize + sx) as usize] == 0 || costs[(i as isize + sy) as usize] == 0 {
                                    continue;
                                }
                            }
                            let there = dist[(i as isize + off[k]) as usize];
                            if there < best.0 {
                                best = (there, k as u8);
                            }
                        }
                        *out = best.1;
                    }
                });
        });
    }

    /// The field's raw arrays, for the C API's inline lookups.
    #[doc(hidden)]
    #[must_use]
    pub fn raw_view(&self) -> RawFlowView<'_> {
        RawFlowView {
            dir: &self.dir,
            directions: &DIRS,
            size: self.size,
            stride: self.stride,
            origin: self.origin,
            cell_size: self.cell_size,
        }
    }

    fn cell(&self, world: Vec2) -> Option<usize> {
        if self.dir.is_empty() {
            return None;
        }
        let c = ((world - self.origin) / self.cell_size).floor();
        (c.x >= 0.0 && c.y >= 0.0 && c.x < self.size.x as f32 && c.y < self.size.y as f32)
            .then(|| (c.y as usize + 1) * self.stride + c.x as usize + 1)
    }

    /// Unit direction toward the nearest goal from `world` (one of 8), or
    /// zero at a goal, on a wall, outside the grid, or where no goal can be
    /// reached.
    #[inline]
    #[must_use]
    pub fn direction_at(&self, world: Vec2) -> Vec2 {
        self.cell(world).map_or(Vec2::ZERO, |i| DIRS[self.dir[i] as usize])
    }

    /// Length of the shortest way from `world` to a goal in world units, or
    /// `None` if no goal can be reached (or `world` is outside the grid).
    /// Slow ground counts as longer: a cell of cost 4 counts 4 times.
    #[inline]
    #[must_use]
    pub fn distance_at(&self, world: Vec2) -> Option<f32> {
        let d = self.dist[self.cell(world)?];
        (d != UNREACHED).then_some(d as f32 / STRAIGHT as f32 * self.cell_size)
    }
}

/// See [`FlowField::raw_view`]. `dir` is padded: cell (x, y) is at
/// `(y + 1) * stride + x + 1`, and indexes `directions`.
#[doc(hidden)]
pub struct RawFlowView<'a> {
    pub dir: &'a [u8],
    pub directions: &'static [Vec2; 9],
    pub size: UVec2,
    pub stride: usize,
    pub origin: Vec2,
    pub cell_size: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_steps_cost_about_root_two() {
        let nav = NavGrid::new(11, 11, 1.0);
        let mut f = FlowField::new();
        f.compute(&nav, &[vec(0.5, 0.5)]);
        let at = |x: usize, y: usize| (y + 1) * f.stride + x + 1;
        assert_eq!(f.dist[at(10, 10)], 10 * DIAGONAL);
        assert_eq!(f.dist[at(10, 0)], 10 * STRAIGHT);
        assert_eq!(f.dir[at(1, 1)], 5, "(1, 1) points north-west, at the goal");
    }

    #[test]
    fn diagonals_never_cut_corners() {
        // . #
        // G .   from (1, 1) the goal is diagonal, but (1, 0) is a wall... and
        //       (0, 1) is the goal itself, so the path is straight left.
        let mut nav = NavGrid::new(2, 2, 1.0);
        nav.set_cost(1, 0, NavGrid::WALL);
        nav.set_cost(0, 0, NavGrid::WALL);
        let mut f = FlowField::new();
        f.compute(&nav, &[vec(0.5, 1.5)]);
        // (1, 1) → goal at (0, 1) is straight; no route through (0, 0).
        let at = (1 + 1) * f.stride + 1 + 1;
        assert_eq!(f.dir[at], 4);
        assert_eq!(f.dist[at], STRAIGHT);
    }

    #[test]
    fn recomputing_reuses_memory() {
        let nav = NavGrid::new(64, 64, 1.0);
        let mut f = FlowField::new();
        f.compute(&nav, &[vec(1.0, 1.0)]);
        let ptrs = (f.dist.as_ptr(), f.dir.as_ptr(), f.buckets[0].capacity());
        f.compute(&nav, &[vec(60.0, 60.0)]);
        assert_eq!((f.dist.as_ptr(), f.dir.as_ptr()), (ptrs.0, ptrs.1));
        assert!(f.buckets[0].capacity() >= ptrs.2);
    }

    fn vec(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }
}
