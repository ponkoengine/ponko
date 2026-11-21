//! CPU side of sprite batching: collect draw calls, order them, split into
//! per-texture batches. No GPU types here, so it is unit tested directly.
//!
//! Ordering rule: higher `z` draws on top; for equal `z`, later draws go on
//! top (painter's order). Consecutive sprites with the same texture, clip
//! rectangle, blend mode and material become one instanced draw call.
//!
//! Triangle meshes (2D skeletons' parts) go through the same list: each
//! vertex is one entry, a `SpriteInstance` with zero size whose `pos` is the
//! vertex and `uv` its texture coordinate, flagged by [`MESH`] in its style.
//! A mesh's vertices share one `z` and are pushed together, so sorting
//! keeps them together and in order; their batches are drawn as triangle
//! lists from the same buffer, in the same order as the sprites around them.

use bytemuck::{Pod, Zeroable};

/// One sprite as the GPU sees it (one instance of a 6-vertex quad): 36
/// bytes, so a million sprites upload 36 MB per frame.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(crate) struct SpriteInstance {
    /// Where the pivot lands, in world units. Full `f32`: positions need it.
    pub pos: [f32; 2],
    /// Final size in world units (texture size × scale; may be negative to flip).
    pub size: [f32; 2],
    /// Pivot inside the sprite (0.5, 0.5 = center), as `f16` bits: plenty
    /// of precision for a point inside a sprite.
    pub pivot: [u16; 2],
    /// Rotation in radians; the vertex shader takes sin and cos.
    pub rotation: f32,
    /// sRGB tint, 8 bits per channel.
    pub color: [u8; 4],
    /// The part of the texture to show, `u0, v0, width, height`, in
    /// 1/65535ths of the texture (unorm16). Full texture by default.
    pub uv: [u16; 4],
}

/// The whole texture, as `SpriteInstance::uv`.
pub(crate) const FULL_UV: [u16; 4] = [0, 0, u16::MAX, u16::MAX];

/// A texture region in 0..1 coordinates as `SpriteInstance::uv`.
pub(crate) fn uv_region(min: [f32; 2], size: [f32; 2]) -> [u16; 4] {
    let q = |x: f32| (x.clamp(0.0, 1.0) * 65535.0).round() as u16;
    [q(min[0]), q(min[1]), q(size[0]), q(size[1])]
}

impl SpriteInstance {
    #[inline]
    pub fn new(pos: [f32; 2], size: [f32; 2], pivot: [f32; 2], rotation: f32, color: [u8; 4]) -> Self {
        // Converting to f16 costs about a nanosecond; skip it for the
        // center pivot nearly every sprite uses.
        const HALF: u16 = 0x3800;
        let pivot = if pivot == [0.5, 0.5] {
            [HALF; 2]
        } else {
            [f16_bits(pivot[0]), f16_bits(pivot[1])]
        };
        Self {
            pos,
            size,
            pivot,
            rotation,
            color,
            uv: FULL_UV,
        }
    }

    /// One vertex of a triangle mesh pushed with [`DrawList::push_mesh`]:
    /// at `pos` (world units), showing texture coordinate `uv` (0..1),
    /// tinted `color`. `local` is where it is inside the mesh's bounds
    /// (0..1), what sprite shaders see as the pixel's `local`.
    #[inline]
    pub fn vertex(pos: [f32; 2], uv: [f32; 2], local: [f32; 2], color: [u8; 4]) -> Self {
        let q = |x: f32| (x.clamp(0.0, 1.0) * 65535.0).round() as u16;
        Self {
            pos,
            size: [0.0; 2],
            pivot: [f16_bits(local[0]), f16_bits(local[1])],
            rotation: 0.0,
            color,
            uv: [q(uv[0]), q(uv[1]), 0, 0],
        }
    }

    /// The same sprite showing only part of its texture.
    #[inline]
    pub fn with_uv(mut self, uv: [u16; 4]) -> Self {
        self.uv = uv;
        self
    }
}

/// `x` as IEEE half-precision bits, rounded to nearest even. Too big for
/// `f16` becomes infinity, too small becomes zero.
#[inline]
/// The value of half-float `bits` (as [`f16_bits`] makes them).
pub(crate) fn f16_value(bits: u16) -> f32 {
    // The center pivot nearly every sprite uses.
    if bits == 0x3800 {
        return 0.5;
    }
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((bits >> 10) & 0x1f);
    let man = f32::from(bits & 0x3ff);
    sign * match exp {
        0 => man * 2f32.powi(-24),
        0x1f if man == 0.0 => f32::INFINITY,
        0x1f => f32::NAN,
        e => (1.0 + man / 1024.0) * 2f32.powi(e - 15),
    }
}

pub(crate) fn f16_bits(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32;
    let man = b & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = man | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = m >> shift;
        let rem = m & ((1 << shift) - 1);
        let mid = 1 << (shift - 1);
        let round = (rem > mid || (rem == mid && half & 1 == 1)) as u32;
        return sign | (half + round) as u16;
    }
    let half = ((e as u32) << 10) | (man >> 13);
    let rem = man & 0x1fff;
    // A carry out of the mantissa bumps the exponent, which is still right.
    let round = (rem > 0x1000 || (rem == 0x1000 && half & 1 == 1)) as u32;
    sign | (half + round) as u16
}

/// A run of instances that share one texture, clip, look and render layers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Batch {
    pub clip: Option<crate::camera::Rect>,
    /// The render layers its sprites are on (`RenderLayers` bits): a 2D
    /// camera draws it if they share one with the camera's.
    pub layers: u32,
    /// Blend mode and material (`sprite_material::style_key`); 0 for plain sprites.
    pub material: u32,
    /// The entries are triangle-list vertices ([`MESH`]), not sprites.
    pub mesh: bool,
    pub texture: u32,
    pub start: u32,
    pub end: u32,
}

/// A sprite as it was pushed on a [`DrawList`], with its clip and style:
/// kept to push it again ([`DrawList::replay`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Recorded {
    instance: SpriteInstance,
    texture: u32,
    z: f32,
    /// Blend mode, material and the [`MESH`] bit.
    style: u32,
    clip: Option<crate::camera::Rect>,
}

/// One world-space UI panel's drawing this frame: drawn into a render
/// target before the 3D frame that shows it.
#[derive(Default)]
pub(crate) struct PanelDraw {
    /// The render target's texture id.
    pub texture: u32,
    /// Its size in pixels.
    pub size: [u32; 2],
    pub list: DrawList,
}

/// Draw commands for one frame. All buffers are reused between frames,
/// so steady-state frames don't allocate.
#[derive(Debug, Default)]
pub(crate) struct DrawList {
    instances: Vec<SpriteInstance>,
    textures: Vec<u32>,
    zs: Vec<f32>,
    /// Each sprite's clip, layers and look: its style key
    /// (`sprite_material`), or with `CLIPPED` set, an index into `clipped`.
    /// 0 for plain sprites.
    states: Vec<u32>,
    /// This frame's clipped states (clipped or not on the default layer):
    /// the clip, the style key and the render layers.
    clipped: Vec<Clipped>,
    clip: Option<crate::camera::Rect>,
    /// The render layers sprites are pushed on; `None`: the default one.
    layers: Option<u32>,
    /// True once two different `z` values have been pushed this frame.
    needs_sort: bool,

    // Scratch space for the sorted path.
    order: Vec<(u32, u32)>,
    sorted: Vec<SpriteInstance>,
    batches: Vec<Batch>,

    /// Lists drawn into textures of their own before this one,
    /// which shows them (a UI group fading as one image); the first
    /// `offscreen_live` are this frame's. Empty unless a group fades.
    offscreen: Vec<Offscreen>,
    offscreen_live: usize,
}

/// A [`DrawList`] drawn into texture `texture` (cleared to transparent)
/// before the list that holds it: its sprites end up premultiplied by
/// their alpha there. The texture is `texture_size` pixels; the list draws
/// into its top-left `size` pixels, which show the target's pixels from
/// `origin` on (the list's coordinates are the target's, `scale` pixels to
/// a unit). `size` 0: nothing to draw.
#[derive(Debug, Default)]
pub(crate) struct Offscreen {
    pub texture: u32,
    pub list: DrawList,
    pub origin: [u32; 2],
    pub size: [u32; 2],
    pub texture_size: [u32; 2],
    pub scale: f32,
}

/// Room for `n` sprites at the end of a [`DrawList`], to be filled from
/// many threads at once by [`DrawList::extend`].
pub(crate) struct Slots {
    instances: *mut SpriteInstance,
    textures: *mut u32,
    zs: *mut f32,
    states: *mut u32,
    n: usize,
}

// SAFETY: `Slots` only hands out writes to distinct indices (see `write`),
// and the plain-data values it writes are `Send`.
unsafe impl Send for Slots {}
unsafe impl Sync for Slots {}

impl Slots {
    /// Writes sprite `i`.
    ///
    /// # Safety
    /// `i < n`, and each index is written by exactly one call: no two
    /// threads may write the same `i`.
    #[inline]
    pub unsafe fn write(&self, i: usize, texture: u32, z: f32, instance: SpriteInstance) {
        debug_assert!(i < self.n);
        // SAFETY: in bounds of the reserved capacity, and not shared (caller).
        unsafe {
            self.instances.add(i).write(instance);
            self.textures.add(i).write(texture);
            self.zs.add(i).write(z);
        }
    }

    /// Writes sprite `i` with blend mode and material `style`
    /// (`sprite_material::style_key`); [`write`](Self::write) leaves it 0.
    ///
    /// # Safety
    /// As for [`write`](Self::write).
    #[inline]
    pub unsafe fn write_styled(&self, i: usize, texture: u32, style: u32, z: f32, instance: SpriteInstance) {
        debug_assert!(style < CLIPPED);
        // SAFETY: forwarded caller contract; `states` has `n` slots too.
        unsafe {
            self.write(i, texture, z, instance);
            self.states.add(i).write(style);
        }
    }
}

/// A sprite state that is not one style key: its clip, style key and render
/// layers.
type Clipped = (Option<crate::camera::Rect>, u32, u32);

/// The render layers of a sprite without any (`RenderLayers::DEFAULT`).
const DEFAULT_LAYERS: u32 = 1;

/// A `DrawList` state with this bit is an index into its clipped states.
pub(crate) const CLIPPED: u32 = 1 << 31;

/// A style key with this bit is a triangle-list vertex, not a sprite (see
/// the module docs). Below [`CLIPPED`], above every style key.
pub(crate) const MESH: u32 = 1 << 30;

/// `z` moved `dz` along in depth; exactly `z` when `dz` is 0 (`-0.0 + 0.0`
/// would sort apart from `-0.0`).
fn shifted(z: f32, dz: f32) -> f32 {
    if dz == 0.0 { z } else { z + dz }
}

/// Maps an `f32` to a `u32` whose unsigned order matches the float order.
fn z_key(z: f32) -> u32 {
    let bits = z.to_bits();
    if bits >> 31 == 1 { !bits } else { bits | 0x8000_0000 }
}

impl DrawList {
    pub fn set_clip(&mut self, clip: Option<crate::camera::Rect>) {
        self.clip = clip;
    }

    /// The clip sprites are pushed with now.
    pub fn clip(&self) -> Option<crate::camera::Rect> {
        self.clip
    }

    /// The render layers the sprites pushed from now on are on (until
    /// [`clear`](Self::clear)); `RenderLayers::DEFAULT` is the default.
    pub fn set_layers(&mut self, layers: u32) {
        self.layers = (layers != DEFAULT_LAYERS).then_some(layers);
    }

    /// The render layers sprites are pushed on now.
    pub fn layers(&self) -> u32 {
        self.layers.unwrap_or(DEFAULT_LAYERS)
    }
    #[inline]
    pub fn push(&mut self, texture: u32, z: f32, instance: SpriteInstance) {
        self.push_styled(texture, 0, z, instance);
    }

    /// [`push`](Self::push) with blend mode and material `style`
    /// (`sprite_material::style_key`, 0 for plain sprites).
    // Always: with two callers the compiler stopped inlining it into
    // `push`, and a call per sprite made plain sprites ~20% slower.
    #[inline(always)]
    pub fn push_styled(&mut self, texture: u32, style: u32, z: f32, instance: SpriteInstance) {
        if let Some(&first_z) = self.zs.first()
            && first_z != z
        {
            self.needs_sort = true;
        }
        let state = if self.clip.is_none() && self.layers.is_none() {
            style
        } else {
            self.clipped_state(self.clip, style, self.layers.unwrap_or(DEFAULT_LAYERS))
        };
        self.states.push(state);
        self.instances.push(instance);
        self.textures.push(texture);
        self.zs.push(z);
    }

    /// Pushes a triangle list: every three of `vertices` (made with
    /// [`SpriteInstance::vertex`]) are one triangle, drawn with `texture`
    /// and `style` at `z`, after what was pushed before at the same `z`.
    pub fn push_mesh(&mut self, texture: u32, style: u32, z: f32, vertices: &[SpriteInstance]) {
        debug_assert!(vertices.len().is_multiple_of(3), "a triangle list");
        debug_assert!(style & MESH == 0);
        for v in vertices {
            self.push_styled(texture, style | MESH, z, *v);
        }
    }

    /// The state of a clipped sprite: the same index for the same clip and
    /// style all frame, so equal ones still batch together. Kept out of
    /// line: most sprites aren't clipped, and the push loop stays small.
    #[cold]
    #[inline(never)]
    fn clipped_state(&mut self, clip: Option<crate::camera::Rect>, style: u32, layers: u32) -> u32 {
        let wanted = (clip, style, layers);
        let at = match self.clipped.last() {
            Some(last) if *last == wanted => self.clipped.len() - 1,
            _ => match self.clipped.iter().position(|c| *c == wanted) {
                Some(at) => at,
                None => {
                    self.clipped.push(wanted);
                    self.clipped.len() - 1
                }
            },
        };
        CLIPPED | at as u32
    }

    /// The clip, style and render layers of `state`.
    #[inline]
    fn resolve(clipped: &[Clipped], state: u32) -> (Option<crate::camera::Rect>, u32, bool, u32) {
        let (clip, style, layers) = if state & CLIPPED == 0 {
            (None, state, DEFAULT_LAYERS)
        } else {
            clipped[(state & !CLIPPED) as usize]
        };
        (clip, style & !MESH, style & MESH != 0, layers)
    }

    /// Whether any sprite this frame has a blend mode or material other
    /// than the plain one, or is a mesh (the renderer then prepares their
    /// pipelines).
    #[cfg_attr(not(feature = "window"), allow(dead_code))]
    pub fn batches_styled(batches: &[Batch]) -> bool {
        batches.iter().any(|b| b.material != 0 || b.mesh)
    }

    /// Appends `n` sprites that `fill` writes through [`Slots::write`],
    /// typically from `par_for_each_indexed`.
    ///
    /// # Safety
    /// `fill` must write every index in `0..n` exactly once before it
    /// returns (unwritten slots would be read as uninitialized memory).
    pub unsafe fn extend(&mut self, n: usize, fill: impl FnOnce(&Slots)) {
        let start = self.instances.len();
        self.states.resize(start + n, 0);
        self.instances.reserve(n);
        self.textures.reserve(n);
        self.zs.reserve(n);
        let slots = Slots {
            // SAFETY: `reserve` made room for `n` more elements after `start`.
            instances: unsafe { self.instances.as_mut_ptr().add(start) },
            textures: unsafe { self.textures.as_mut_ptr().add(start) },
            zs: unsafe { self.zs.as_mut_ptr().add(start) },
            // SAFETY: `resize` made `n` initialized slots after `start`.
            states: unsafe { self.states.as_mut_ptr().add(start) },
            n,
        };
        fill(&slots);
        // SAFETY: the caller wrote all `n` slots; the types are plain data.
        unsafe {
            self.instances.set_len(start + n);
            self.textures.set_len(start + n);
            self.zs.set_len(start + n);
        }
        if !self.needs_sort
            && let Some(&first_z) = self.zs.first()
        {
            self.needs_sort = self.zs[start..].iter().any(|&z| z != first_z);
        }
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Copies the sprites pushed from index `start` on into `out` (emptied
    /// first), each with the clip and style it was pushed with, so that
    /// [`replay`](Self::replay) can push them again in a later frame.
    pub fn record_from(&self, start: usize, out: &mut Vec<Recorded>) {
        out.clear();
        out.extend((start..self.instances.len()).map(|i| {
            let state = self.states[i];
            let (clip, style) = if state & CLIPPED == 0 {
                (None, state)
            } else {
                let (clip, style, _) = self.clipped[(state & !CLIPPED) as usize];
                (clip, style)
            };
            Recorded {
                instance: self.instances[i],
                texture: self.textures[i],
                z: self.zs[i],
                style,
                clip,
            }
        }));
    }

    /// Pushes `sprites` (from [`record_from`](Self::record_from)) as they
    /// were pushed, `dz` further along in depth, on the render layers
    /// sprites are pushed on now. The list ends up as if they had been
    /// drawn again `dz` deeper; the clip set for the sprites pushed next
    /// stays as it was.
    pub fn replay(&mut self, sprites: &[Recorded], dz: f32) {
        let clip = self.clip;
        for s in sprites {
            self.clip = s.clip;
            self.push_styled(s.texture, s.style, shifted(s.z, dz), s.instance);
        }
        self.clip = clip;
    }

    /// Sprite `i` as pushed, to change it in place before the list is
    /// finished.
    pub fn instance_mut(&mut self, i: usize) -> Option<&mut SpriteInstance> {
        self.instances.get_mut(i)
    }

    /// The rectangle every sprite pushed this frame covers, each cut by
    /// its clip: what the list draws, its rotated corners and meshes'
    /// vertices included, plain sprites tinted fully transparent left out.
    /// `None` when it draws nothing.
    pub fn drawn_bounds(&self) -> Option<crate::camera::Rect> {
        let (mut min, mut max) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for (s, &state) in self.instances.iter().zip(&self.states) {
            let (clip, style, mesh, _) = Self::resolve(&self.clipped, state);
            // A plain sprite tinted fully transparent (a UI node's clear
            // fill) leaves its pixels as they were.
            if style == 0 && s.color[3] == 0 {
                continue;
            }
            let (mut lo, mut hi) = (s.pos, s.pos);
            if !mesh && s.rotation == 0.0 {
                // Nearly every UI sprite: no turning, two corners.
                let pivot = [f16_value(s.pivot[0]), f16_value(s.pivot[1])];
                for k in 0..2 {
                    let (a, b) = (-pivot[k] * s.size[k], (1.0 - pivot[k]) * s.size[k]);
                    (lo[k], hi[k]) = (s.pos[k] + a.min(b), s.pos[k] + a.max(b));
                }
            } else if !mesh {
                let pivot = [f16_value(s.pivot[0]), f16_value(s.pivot[1])];
                let (sin, cos) = s.rotation.sin_cos();
                for corner in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] {
                    let local = [(corner[0] - pivot[0]) * s.size[0], (corner[1] - pivot[1]) * s.size[1]];
                    let p = [
                        s.pos[0] + local[0] * cos - local[1] * sin,
                        s.pos[1] + local[0] * sin + local[1] * cos,
                    ];
                    for k in 0..2 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
            }
            if let Some(c) = clip {
                lo = [lo[0].max(c.min.x), lo[1].max(c.min.y)];
                hi = [hi[0].min(c.max.x), hi[1].min(c.max.y)];
            }
            // A mesh's vertex is a point of it; a sprite with no area
            // (or clipped away) draws nothing.
            let empty = if mesh {
                lo[0] > hi[0] || lo[1] > hi[1]
            } else {
                lo[0] >= hi[0] || lo[1] >= hi[1]
            };
            if empty {
                continue;
            }
            for k in 0..2 {
                min[k] = min[k].min(lo[k]);
                max[k] = max[k].max(hi[k]);
            }
        }
        (min[0] < max[0] && min[1] < max[1]).then(|| crate::camera::Rect {
            min: min.into(),
            max: max.into(),
        })
    }

    /// Moves the sprites pushed from index `start` on: each position goes
    /// to `pos * scale + shift` and each size is scaled by `scale` (an
    /// animated UI node's offset and size), and their alpha is multiplied
    /// by `alpha` (its opacity). Their clips are left as they were pushed.
    pub fn transform_from(&mut self, start: usize, scale: f32, shift: [f32; 2], alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        for s in self.instances.iter_mut().skip(start) {
            s.pos = [s.pos[0] * scale + shift[0], s.pos[1] * scale + shift[1]];
            s.size = [s.size[0] * scale, s.size[1] * scale];
            s.color[3] = (f32::from(s.color[3]) * alpha).round() as u8;
        }
    }

    pub fn clear(&mut self) {
        self.instances.clear();
        self.textures.clear();
        self.zs.clear();
        self.states.clear();
        self.clipped.clear();
        self.clip = None;
        self.layers = None;
        self.needs_sort = false;
        self.batches.clear();
        // The lists keep their memory, emptied when used again.
        self.offscreen_live = 0;
    }

    /// This frame's offscreen lists, `n` of them: the first `n` kept from
    /// earlier frames (emptied by whoever fills them), more made as needed.
    pub fn offscreen_slots(&mut self, n: usize) -> &mut [Offscreen] {
        if self.offscreen.len() < n {
            self.offscreen.resize_with(n, Offscreen::default);
        }
        self.offscreen_live = n;
        &mut self.offscreen[..n]
    }

    /// [`finish`](Self::finish), and the offscreen lists finished too: a
    /// list's sprites and batches as [`Offscreen::finished`] gives them.
    pub fn finish_with_offscreen(&mut self) -> (&[SpriteInstance], &[Batch], &[Offscreen]) {
        for layer in &mut self.offscreen[..self.offscreen_live] {
            layer.list.finish();
        }
        self.finish();
        let (sprites, batches) = self.finished();
        (sprites, batches, &self.offscreen[..self.offscreen_live])
    }

    /// What the last [`finish`](Self::finish) gave.
    fn finished(&self) -> (&[SpriteInstance], &[Batch]) {
        if self.needs_sort {
            (&self.sorted, &self.batches)
        } else {
            (&self.instances, &self.batches)
        }
    }

    /// Orders the frame's sprites and groups them into batches.
    /// Returns the instance data to upload and the draw ranges.
    pub fn finish(&mut self) -> (&[SpriteInstance], &[Batch]) {
        self.batches.clear();

        if !self.needs_sort {
            // Fast path (the common case): everything shares one z, so
            // submission order is already the draw order.
            Self::build_batches(&mut self.batches, &self.textures, &self.states, &self.clipped);
            return (&self.instances, &self.batches);
        }

        // (z, submission index) is unique, so an unstable sort is deterministic
        // and doesn't allocate.
        self.order.clear();
        self.order
            .extend(self.zs.iter().enumerate().map(|(i, &z)| (z_key(z), i as u32)));
        self.order.sort_unstable();

        self.sorted.clear();
        self.sorted
            .extend(self.order.iter().map(|&(_, i)| self.instances[i as usize]));
        self.build_sorted_batches();
        (&self.sorted, &self.batches)
    }

    /// Batches for the sorted order (reuses `order`'s texture lookups).
    fn build_sorted_batches(&mut self) {
        let textures = &self.textures;
        let batches = &mut self.batches;
        let mut run: Option<Batch> = None;
        let mut run_state = 0;
        for (i, &(_, src)) in self.order.iter().enumerate() {
            let texture = textures[src as usize];
            let state = self.states[src as usize];
            let i = i as u32;
            match &mut run {
                Some(b) if b.texture == texture && run_state == state => b.end = i + 1,
                _ => {
                    batches.extend(run);
                    let (clip, material, mesh, layers) = Self::resolve(&self.clipped, state);
                    run_state = state;
                    run = Some(Batch {
                        clip,
                        layers,
                        material,
                        mesh,
                        texture,
                        start: i,
                        end: i + 1,
                    });
                }
            }
        }
        batches.extend(run);
    }

    /// One batch per run of equal textures and states, in order.
    fn build_batches(batches: &mut Vec<Batch>, textures: &[u32], states: &[u32], clipped: &[Clipped]) {
        let mut start = 0;
        while start < textures.len() {
            let texture = textures[start];
            let state = states[start];
            let len = textures[start..]
                .iter()
                .zip(&states[start..])
                .position(|(&t, &s)| t != texture || s != state)
                .unwrap_or(textures.len() - start);
            let (clip, material, mesh, layers) = Self::resolve(clipped, state);
            batches.push(Batch {
                clip,
                layers,
                material,
                mesh,
                texture,
                start: start as u32,
                end: (start + len) as u32,
            });
            start += len;
        }
    }
}

impl Offscreen {
    /// Its list's sprites and batches, as its last finish gave them.
    pub fn finished(&self) -> (&[SpriteInstance], &[Batch]) {
        self.list.finished()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(tag: f32) -> SpriteInstance {
        SpriteInstance::new([tag, 0.0], [1.0, 1.0], [0.5, 0.5], 0.0, [255; 4])
    }

    /// Pushes a few sprites with clips, styles, meshes and z values.
    fn mixed(list: &mut DrawList) {
        let clip = crate::camera::Rect::new(10.0, 20.0, 30.0, 40.0);
        list.push(1, 2.0, inst(1.0));
        list.set_clip(Some(clip));
        list.push_styled(2, 7, 1.0, inst(2.0));
        list.push_mesh(3, 5, 1.5, &[inst(3.0), inst(4.0), inst(5.0)]);
        list.set_clip(None);
        list.push(1, 0.5, inst(6.0));
    }

    #[test]
    fn replayed_sprites_draw_as_pushed() {
        let mut drawn = DrawList::default();
        drawn.push(9, 0.0, inst(0.0));
        let start = drawn.len();
        mixed(&mut drawn);
        drawn.push(9, 3.0, inst(7.0));
        let mut recorded = Vec::new();
        drawn.record_from(start, &mut recorded);
        assert_eq!(recorded.len(), 7, "everything pushed from `start` on");

        let mut again = DrawList::default();
        again.push(9, 0.0, inst(0.0));
        again.set_clip(Some(crate::camera::Rect::new(0.0, 0.0, 1.0, 1.0)));
        again.replay(&recorded, 0.0);
        assert_eq!(
            again.clip(),
            Some(crate::camera::Rect::new(0.0, 0.0, 1.0, 1.0)),
            "the clip stays"
        );
        let (sprites, batches) = drawn.finish();
        let (sprites, batches) = (sprites.to_vec(), batches.to_vec());
        let (replayed, replayed_batches) = again.finish();
        assert_eq!(replayed, &sprites[..]);
        assert_eq!(replayed_batches, &batches[..]);
    }

    #[test]
    fn replayed_sprites_move_in_depth_together() {
        let mut drawn = DrawList::default();
        drawn.push(1, 10.0, inst(1.0));
        drawn.push(1, 10.7, inst(2.0));
        let mut recorded = Vec::new();
        drawn.record_from(0, &mut recorded);
        // Replayed 8 deeper, they go above what lies between.
        let mut again = DrawList::default();
        again.replay(&recorded, 8.0);
        again.push(2, 12.0, inst(3.0));
        let (sprites, _) = again.finish();
        let tags: Vec<f32> = sprites.iter().map(|s| s.pos[0]).collect();
        assert_eq!(tags, [3.0, 1.0, 2.0]);
    }

    #[test]
    fn clip_changes_split_batches_and_follow_sorted_instances() {
        let mut list = DrawList::default();
        let clip = crate::camera::Rect::new(10.0, 20.0, 30.0, 40.0);
        list.set_clip(Some(clip));
        list.push(1, 5.0, inst(1.0));
        list.set_clip(None);
        list.push(1, 0.0, inst(2.0));
        list.set_clip(Some(clip));
        list.push(1, 5.0, inst(3.0));
        let (instances, batches) = list.finish();
        assert_eq!(tags(instances), [2.0, 1.0, 3.0]);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].clip, None);
        assert_eq!(batches[1].clip, Some(clip));
        list.clear();
        list.push(1, 0.0, inst(0.0));
        assert_eq!(list.finish().1[0].clip, None);
        list.set_clip(Some(clip));
        list.push(1, 0.0, inst(1.0));
        assert_eq!(list.finish().1.len(), 2);
    }

    #[test]
    fn extend_fills_from_many_threads() {
        use rayon::prelude::*;
        let mut list = DrawList::default();
        list.push(1, 0.0, inst(-1.0));
        // Small under Miri, which runs it a thousand times slower.
        let n = if cfg!(miri) { 300 } else { 50_000 };
        // SAFETY: every index in 0..n is written once.
        unsafe {
            list.extend(n, |slots| {
                (0..n).into_par_iter().for_each(|i| {
                    let texture = if i < 100 { 1 } else { 2 };
                    slots.write(i, texture, 0.0, inst(i as f32));
                });
            });
        }
        let (instances, batches) = list.finish();
        assert_eq!(instances.len(), n + 1);
        assert_eq!(instances[0].pos[0], -1.0);
        assert!(instances[1..].iter().enumerate().all(|(i, s)| s.pos[0] == i as f32));
        assert_eq!(
            batches,
            &[
                Batch {
                    clip: None,
                    layers: 1,
                    material: 0,
                    mesh: false,
                    texture: 1,
                    start: 0,
                    end: 101
                },
                Batch {
                    clip: None,
                    layers: 1,
                    material: 0,
                    mesh: false,
                    texture: 2,
                    start: 101,
                    end: n as u32 + 1
                },
            ]
        );
    }

    #[test]
    fn layer_changes_split_batches_and_reset_with_the_frame() {
        let mut list = DrawList::default();
        list.push(1, 0.0, inst(1.0));
        list.set_layers(0b10);
        list.push(1, 0.0, inst(2.0));
        list.push(1, 0.0, inst(3.0));
        list.set_layers(1);
        list.push(1, 0.0, inst(4.0));
        let (_, batches) = list.finish();
        let layers: Vec<_> = batches.iter().map(|b| (b.layers, b.end - b.start)).collect();
        assert_eq!(layers, [(1, 1), (0b10, 2), (1, 1)]);
        assert!(batches.iter().all(|b| b.clip.is_none()));
        list.set_layers(0b100);
        list.clear();
        list.push(1, 0.0, inst(5.0));
        assert_eq!(
            list.finish().1[0].layers,
            1,
            "the next frame starts on the default layer"
        );
    }

    #[test]
    fn extend_notices_a_second_z() {
        let mut list = DrawList::default();
        list.push(1, 0.0, inst(0.0));
        // SAFETY: both indices written once.
        unsafe {
            list.extend(2, |slots| {
                slots.write(0, 1, 0.0, inst(1.0));
                slots.write(1, 1, -1.0, inst(2.0));
            });
        }
        let (instances, _) = list.finish();
        assert_eq!(tags(instances), [2.0, 0.0, 1.0]);
    }

    #[test]
    fn instances_are_36_bytes() {
        assert_eq!(size_of::<SpriteInstance>(), 36);
    }

    #[test]
    fn uv_regions_quantize_to_unorm16() {
        assert_eq!(uv_region([0.0, 0.0], [1.0, 1.0]), FULL_UV);
        assert_eq!(uv_region([0.5, 0.25], [0.25, 2.0]), [32768, 16384, 16384, 65535]);
    }

    #[test]
    fn the_center_pivot_shortcut_matches_the_conversion() {
        let fast = SpriteInstance::new([0.0; 2], [1.0; 2], [0.5, 0.5], 0.0, [0; 4]);
        assert_eq!(fast.pivot, [f16_bits(0.5); 2]);
        let slow = SpriteInstance::new([0.0; 2], [1.0; 2], [0.0, 1.0], 0.0, [0; 4]);
        assert_eq!(slow.pivot, [f16_bits(0.0), f16_bits(1.0)]);
    }

    #[test]
    fn half_floats_read_back_what_was_written() {
        for x in [0.0, 0.5, 1.0, -2.0, 0.25, 0.1, 65504.0, 2f32.powi(-24), 2f32.powi(-14)] {
            let back = f16_value(f16_bits(x));
            assert!((back - x).abs() <= x.abs() / 1024.0, "{x} came back {back}");
        }
        assert_eq!(f16_value(0x7c00), f32::INFINITY);
    }

    #[test]
    fn drawn_bounds_cover_what_the_sprites_draw_cut_by_their_clips() {
        let mut list = DrawList::default();
        assert_eq!(list.drawn_bounds(), None, "nothing drawn");
        // Centered 10 x 4 at (20, 10); a clear plain fill over everything
        // draws nothing.
        list.push(
            1,
            0.0,
            SpriteInstance::new([20.0, 10.0], [10.0, 4.0], [0.5, 0.5], 0.0, [255; 4]),
        );
        list.push(
            1,
            0.0,
            SpriteInstance::new([0.0, 0.0], [999.0, 999.0], [0.0, 0.0], 0.0, [9, 9, 9, 0]),
        );
        let r = list.drawn_bounds().unwrap();
        assert_eq!((r.min, r.max), (glam::vec2(15.0, 8.0), glam::vec2(25.0, 12.0)));
        // Turned a quarter: 4 wide, 10 high, about its pivot.
        let mut turned = DrawList::default();
        let quarter = std::f32::consts::FRAC_PI_2;
        turned.push(
            1,
            0.0,
            SpriteInstance::new([20.0, 10.0], [10.0, 4.0], [0.5, 0.5], quarter, [255; 4]),
        );
        let r = turned.drawn_bounds().unwrap();
        assert!(r.min.abs_diff_eq(glam::vec2(18.0, 5.0), 1e-4) && r.max.abs_diff_eq(glam::vec2(22.0, 15.0), 1e-4));
        // A flipped sprite (negative size) and a corner pivot.
        let mut flipped = DrawList::default();
        flipped.push(
            1,
            0.0,
            SpriteInstance::new([5.0, 5.0], [-4.0, 2.0], [0.0, 0.0], 0.0, [255; 4]),
        );
        let r = flipped.drawn_bounds().unwrap();
        assert_eq!((r.min, r.max), (glam::vec2(1.0, 5.0), glam::vec2(5.0, 7.0)));
        // Clipped: only the part inside its clip; a mesh by its vertices.
        list.set_clip(Some(crate::camera::Rect::new(0.0, 0.0, 30.0, 30.0)));
        list.push(
            1,
            0.0,
            SpriteInstance::new([25.0, 25.0], [20.0, 20.0], [0.5, 0.5], 0.0, [255; 4]),
        );
        list.set_clip(None);
        let v = |x: f32, y: f32| SpriteInstance::vertex([x, y], [0.0, 0.0], [0.0, 0.0], [255; 4]);
        list.push_mesh(2, 0, 0.0, &[v(-3.0, 1.0), v(0.0, 2.0), v(1.0, -1.0)]);
        let r = list.drawn_bounds().unwrap();
        assert_eq!((r.min, r.max), (glam::vec2(-3.0, -1.0), glam::vec2(30.0, 30.0)));
    }

    #[test]
    fn f16_conversion_matches_ieee() {
        for (x, bits) in [
            (0.0, 0x0000),
            (-0.0, 0x8000),
            (1.0, 0x3c00),
            (0.5, 0x3800),
            (-2.0, 0xc000),
            (32.0, 0x5000),
            (65504.0, 0x7bff),
            (1e6, 0x7c00),
            (f32::INFINITY, 0x7c00),
            (2f32.powi(-24), 0x0001), // smallest subnormal
            (2f32.powi(-14), 0x0400), // smallest normal
            (1e-9, 0x0000),
            (1.0 + 2f32.powi(-11), 0x3c00), // halfway: rounds to even
            (1.0 + 3.0 * 2f32.powi(-11), 0x3c02),
        ] {
            assert_eq!(f16_bits(x), bits, "{x}");
        }
        assert_eq!(f16_bits(f32::NAN) & 0x7c00, 0x7c00);
        assert_ne!(f16_bits(f32::NAN) & 0x3ff, 0);
    }

    #[test]
    fn a_late_z_change_keeps_earlier_batches() {
        let mut list = DrawList::default();
        list.push(1, 0.0, inst(0.0));
        list.push(2, 0.0, inst(1.0));
        list.push(2, 0.0, inst(2.0));
        list.push(1, -1.0, inst(3.0)); // sorts first
        let (instances, batches) = list.finish();
        assert_eq!(tags(instances), [3.0, 0.0, 1.0, 2.0]);
        assert_eq!(
            batches,
            &[
                Batch {
                    clip: None,
                    layers: 1,
                    material: 0,
                    mesh: false,
                    texture: 1,
                    start: 0,
                    end: 2
                },
                Batch {
                    clip: None,
                    layers: 1,
                    material: 0,
                    mesh: false,
                    texture: 2,
                    start: 2,
                    end: 4
                },
            ]
        );
    }

    fn tags(instances: &[SpriteInstance]) -> Vec<f32> {
        instances.iter().map(|i| i.pos[0]).collect()
    }

    #[test]
    fn same_texture_is_one_batch() {
        let mut list = DrawList::default();
        for i in 0..1000 {
            list.push(7, 0.0, inst(i as f32));
        }
        let (instances, batches) = list.finish();
        assert_eq!(instances.len(), 1000);
        assert_eq!(
            batches,
            &[Batch {
                clip: None,
                layers: 1,
                material: 0,
                mesh: false,
                texture: 7,
                start: 0,
                end: 1000
            }]
        );
    }

    #[test]
    fn texture_changes_split_batches_in_submission_order() {
        let mut list = DrawList::default();
        list.push(1, 0.0, inst(0.0));
        list.push(1, 0.0, inst(1.0));
        list.push(2, 0.0, inst(2.0));
        list.push(1, 0.0, inst(3.0));
        let (instances, batches) = list.finish();
        assert_eq!(tags(instances), [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(
            batches,
            &[
                Batch {
                    clip: None,
                    layers: 1,
                    material: 0,
                    mesh: false,
                    texture: 1,
                    start: 0,
                    end: 2
                },
                Batch {
                    clip: None,
                    layers: 1,
                    material: 0,
                    mesh: false,
                    texture: 2,
                    start: 2,
                    end: 3
                },
                Batch {
                    clip: None,
                    layers: 1,
                    material: 0,
                    mesh: false,
                    texture: 1,
                    start: 3,
                    end: 4
                },
            ]
        );
    }

    #[test]
    fn higher_z_draws_later_and_ties_keep_submission_order() {
        let mut list = DrawList::default();
        list.push(1, 5.0, inst(0.0));
        list.push(1, -2.0, inst(1.0));
        list.push(2, 5.0, inst(2.0));
        list.push(1, 0.0, inst(3.0));
        list.push(1, -2.0, inst(4.0));
        let (instances, batches) = list.finish();
        assert_eq!(tags(instances), [1.0, 4.0, 3.0, 0.0, 2.0]);
        assert_eq!(
            batches,
            &[
                Batch {
                    clip: None,
                    layers: 1,
                    material: 0,
                    mesh: false,
                    texture: 1,
                    start: 0,
                    end: 4
                },
                Batch {
                    clip: None,
                    layers: 1,
                    material: 0,
                    mesh: false,
                    texture: 2,
                    start: 4,
                    end: 5
                },
            ]
        );
    }

    #[test]
    fn clear_resets_for_next_frame() {
        let mut list = DrawList::default();
        list.push(1, 1.0, inst(0.0));
        list.push(1, 2.0, inst(1.0));
        list.clear();
        assert_eq!(list.len(), 0);
        list.push(3, 9.0, inst(0.0));
        let (instances, batches) = list.finish();
        assert_eq!(instances.len(), 1);
        assert_eq!(
            batches,
            &[Batch {
                clip: None,
                layers: 1,
                material: 0,
                mesh: false,
                texture: 3,
                start: 0,
                end: 1
            }]
        );
    }

    #[test]
    fn looks_split_batches_and_equal_looks_merge() {
        let mut list = DrawList::default();
        list.push(1, 0.0, inst(0.0));
        list.push_styled(1, 5, 0.0, inst(1.0));
        list.push_styled(1, 5, 0.0, inst(2.0));
        list.push(1, 0.0, inst(3.0));
        let (_, batches) = list.finish();
        let looks: Vec<_> = batches.iter().map(|b| (b.material, b.end - b.start)).collect();
        assert_eq!(looks, [(0, 1), (5, 2), (0, 1)]);
    }

    #[test]
    fn looks_follow_their_sprites_when_sorted_and_clipped() {
        let mut list = DrawList::default();
        let clip = crate::camera::Rect::new(1.0, 2.0, 3.0, 4.0);
        list.push_styled(1, 6, 2.0, inst(0.0));
        list.set_clip(Some(clip));
        list.push_styled(1, 6, 1.0, inst(1.0));
        list.push(1, 1.0, inst(2.0));
        list.set_clip(None);
        list.push_styled(1, 6, 2.0, inst(3.0));
        list.set_clip(Some(clip));
        // Same clip and look as the second sprite: one batch with it once sorted.
        list.push_styled(1, 6, 1.0, inst(4.0));
        let (instances, batches) = list.finish();
        assert_eq!(tags(instances), [1.0, 2.0, 4.0, 0.0, 3.0]);
        let got: Vec<_> = batches.iter().map(|b| (b.clip, b.material, b.start, b.end)).collect();
        assert_eq!(
            got,
            [
                (Some(clip), 6, 0, 1),
                (Some(clip), 0, 1, 2),
                (Some(clip), 6, 2, 3),
                (None, 6, 3, 5)
            ]
        );
        list.clear();
        list.push(1, 0.0, inst(0.0));
        assert_eq!(
            list.finish().1[0].material,
            0,
            "looks and clips are forgotten each frame"
        );
    }

    #[test]
    fn styled_writes_from_many_threads_keep_their_looks() {
        use rayon::prelude::*;
        let mut list = DrawList::default();
        let n = if cfg!(miri) { 300 } else { 30_000 };
        // SAFETY: every index in 0..n is written once.
        unsafe {
            list.extend(n, |slots| {
                (0..n).into_par_iter().for_each(|i| {
                    if i < n / 2 {
                        slots.write(i, 1, 0.0, inst(i as f32));
                    } else {
                        slots.write_styled(i, 1, 9, 0.0, inst(i as f32));
                    }
                });
            });
        }
        let (_, batches) = list.finish();
        let looks: Vec<_> = batches.iter().map(|b| (b.material, b.start, b.end)).collect();
        assert_eq!(looks, [(0, 0, n as u32 / 2), (9, n as u32 / 2, n as u32)]);
    }

    #[test]
    fn z_key_orders_like_floats() {
        let zs = [-100.0, -1.5, -0.0, 0.0, 0.25, 3.0, 1e9];
        for w in zs.windows(2) {
            assert!(z_key(w[0]) <= z_key(w[1]), "{} vs {}", w[0], w[1]);
        }
    }
}
