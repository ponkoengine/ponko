//! Contact ripples on the CPU: the ripple compute pass's simulation
//! (`ripples.wgsl`) for GPUs without compute shaders (WebGL2). The same
//! damped wave equation on the same grid, one step a frame, uploaded into
//! the ripple target as the compute pass would leave it: red the height
//! now, green the height a step ago.

use glam::{Vec2, Vec4};

/// The ripple field, stepped on the CPU.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CpuRipples {
    side: usize,
    now: Vec<f32>,
    before: Vec<f32>,
    next: Vec<f32>,
    /// The upload, kept between frames (RGBA half floats).
    pub upload: Vec<u8>,
    /// Whether the field is all still (nothing to step or upload).
    still: bool,
}

impl CpuRipples {
    pub fn new(side: u32) -> Self {
        let n = side.max(1) as usize;
        Self {
            side: n,
            now: vec![0.0; n * n],
            before: vec![0.0; n * n],
            next: vec![0.0; n * n],
            upload: vec![0; n * n * 8],
            still: true,
        }
    }

    /// The height now at texel (x, z).
    #[cfg(test)]
    pub fn height(&self, x: usize, z: usize) -> f32 {
        self.now[z * self.side + x]
    }

    /// One step of the simulation over `region` (its −x, −z corner, side
    /// and damping, as the compute pass's `custom[0]`) with `contacts` (x,
    /// z, radius, strength). Returns whether anything changed (and the
    /// field needs uploading).
    pub fn step(&mut self, region: Vec4, contacts: &[Vec4]) -> bool {
        if self.still && contacts.is_empty() {
            return false;
        }
        let n = self.side;
        let damping = region.w;
        let mut moving = false;
        for z in 0..n {
            for x in 0..n {
                let at = |dx: isize, dz: isize| {
                    let cx = (x as isize + dx).clamp(0, n as isize - 1) as usize;
                    let cz = (z as isize + dz).clamp(0, n as isize - 1) as usize;
                    self.now[cz * n + cx]
                };
                let around = at(1, 0) + at(-1, 0) + at(0, 1) + at(0, -1);
                let i = z * n + x;
                let edge = x.min(z).min(n - 1 - x).min(n - 1 - z);
                let mut next = if edge < 1 {
                    0.0
                } else {
                    (around * 0.5 - self.before[i]) * damping
                };
                let uv = Vec2::new((x as f32 + 0.5) / n as f32, (z as f32 + 0.5) / n as f32);
                let world = Vec2::new(region.x, region.y) + uv * region.z;
                for c in contacts.iter().filter(|c| c.z > 0.0) {
                    let d = world.distance(Vec2::new(c.x, c.y)) / c.z;
                    next -= c.w * (1.0 - d * d).max(0.0);
                }
                // Below what a half float tells from zero: still.
                if next.abs() < 1e-5 {
                    next = 0.0;
                }
                moving |= next != 0.0;
                self.next[i] = next;
            }
        }
        // before <- now <- next.
        std::mem::swap(&mut self.before, &mut self.now);
        std::mem::swap(&mut self.now, &mut self.next);
        let was_still = self.still;
        self.still = !moving && self.before.iter().all(|v| *v == 0.0);
        for (texel, (now, before)) in self.upload.chunks_exact_mut(8).zip(self.now.iter().zip(&self.before)) {
            let [r, g, b, a] = [*now, *before, 0.0, 1.0].map(super::spectrum::f16_bits);
            texel[0..2].copy_from_slice(&r.to_le_bytes());
            texel[2..4].copy_from_slice(&g.to_le_bytes());
            texel[4..6].copy_from_slice(&b.to_le_bytes());
            texel[6..8].copy_from_slice(&a.to_le_bytes());
        }
        // A field that just went still uploads once more (all zero).
        !(was_still && self.still)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_contact_spreads_into_a_ring_that_fades() {
        let mut field = CpuRipples::new(64);
        let region = Vec4::new(-8.0, -8.0, 16.0, 0.985);
        assert!(!field.step(region, &[]), "a still field has nothing to do");
        assert!(field.step(region, &[Vec4::new(0.0, 0.0, 0.6, 0.3)]));
        let centre = field.height(32, 32);
        assert!(centre < -0.1, "the touch pushes the water down: {centre}");
        for _ in 0..12 {
            field.step(region, &[]);
        }
        // A ring: the water moves farther out than the contact reached.
        let out = (0..64).map(|x| field.height(x, 32).abs()).fold(0.0f32, f32::max);
        let far = field.height(32 + 12, 32).abs() + field.height(32 - 12, 32).abs();
        assert!(far > 1e-3, "the ring reached 12 texels out: {far}");
        assert!(out < 0.3, "and spread its height: {out}");
        for _ in 0..2000 {
            field.step(region, &[]);
        }
        assert!(
            (0..64 * 64).all(|i| field.height(i % 64, i / 64).abs() < 1e-3),
            "damped, it settles"
        );
    }
}
