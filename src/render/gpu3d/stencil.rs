//! Materials with a stencil state ([`crate::render::Stencil`]).
//!
//! A stencil state is part of a render pipeline, so each stencil material
//! draws its color with pipelines of its own, built from its shader's
//! [`SideBuilder`](super::shaders::SideBuilder) the first time a frame
//! needs them (like the shader's own crossfade and shared variants). Only
//! the color roles have them: opaque (over the prepass or writing its own
//! depth), opaque with alpha to coverage, and transparent, the last one
//! also read-only for the split transparent pass, which reads the depth
//! and so can't write the stencil beside it. Every other pass draws stencil
//! materials with their shader's ordinary pipelines.
//!
//! The scene depth gets stencil bits ([`STENCIL_FORMAT`]) only once a
//! material has a stencil state: until then every pipeline and target is
//! as it was. The switch rebuilds what draws into the scene depth, as a
//! change of sample count does, and stays for the renderer's life.

use std::sync::{Arc, OnceLock};

use super::shaders::{ShaderPipelines, Shaders};
use super::{Instancing, PipelineRole, ShaderVariant, Sides};
use crate::render::{Stencil, StencilCompare, StencilFace, StencilOp};

/// The scene depth with stencil: 24-bit (or more) depth and 8-bit stencil,
/// on every backend including WebGL2.
pub(super) const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

/// The roles with stencil pipelines, in the order of a set's slots.
const ROLES: [PipelineRole; 4] = [
    PipelineRole::Opaque,
    PipelineRole::OpaqueSolid,
    PipelineRole::OpaqueCoverage,
    PipelineRole::Transparent,
];
const INSTANCING: [Instancing; 3] = [Instancing::Plain, Instancing::Crossfade, Instancing::Shared];
/// Pipelines per set: every instancing and role, writing and read-only.
const SLOTS: usize = ROLES.len() * INSTANCING.len() * 2;

/// One stencil material's pipelines.
struct StencilSet {
    /// The shader set they are built from; a different one in the shader's
    /// slot (a reload, a new sample count or depth format) makes them stale.
    shader: Arc<ShaderPipelines>,
    slot: u32,
    sides: Sides,
    state: wgpu::StencilState,
    /// [`Stencil::order`] and [`Stencil::reference`].
    order: i32,
    reference: u32,
    pipelines: [OnceLock<Option<wgpu::RenderPipeline>>; SLOTS],
}

impl StencilSet {
    fn new(shaders: &Shaders, slot: u32, sides: Sides, stencil: &Stencil) -> Self {
        Self {
            shader: shaders.sets[slot as usize].clone(),
            slot,
            sides,
            state: state(stencil),
            order: stencil.order,
            reference: u32::from(stencil.reference),
            pipelines: std::array::from_fn(|_| OnceLock::new()),
        }
    }
}

/// Every stencil material's pipelines, by material id.
#[derive(Default)]
pub(super) struct StencilPipelines {
    by_material: Vec<Option<StencilSet>>,
    /// The ids with a set, so a frame checks only those.
    ids: Vec<u32>,
    /// Whether some material ever had a stencil: the scene depth then has
    /// stencil bits.
    pub(super) wanted: bool,
}

impl StencilPipelines {
    /// Material `id` now draws with `stencil` (or none), shader `slot` and
    /// `double_sided`.
    pub(super) fn assign(
        &mut self,
        shaders: &Shaders,
        id: u32,
        stencil: Option<&Stencil>,
        slot: u32,
        double_sided: bool,
    ) {
        let i = id as usize;
        match stencil {
            Some(stencil) => {
                self.wanted = true;
                if self.by_material.len() <= i {
                    self.by_material.resize_with(i + 1, || None);
                }
                let state = state(stencil);
                let sides = Sides::of(double_sided);
                let same = self.by_material[i]
                    .as_ref()
                    .is_some_and(|s| s.state == state && s.slot == slot && s.sides == sides);
                if !same {
                    self.by_material[i] = Some(StencilSet::new(shaders, slot, sides, stencil));
                }
                if let Some(set) = self.by_material[i].as_mut() {
                    set.order = stencil.order;
                    set.reference = u32::from(stencil.reference);
                }
                if !self.ids.contains(&id) {
                    self.ids.push(id);
                }
            }
            None => {
                if let Some(set) = self.by_material.get_mut(i) {
                    *set = None;
                }
                self.ids.retain(|&m| m != id);
            }
        }
    }

    /// Drops the pipelines whose shader set changed (a reload, a freed
    /// shader, a new sample count or depth format): they are built again
    /// from the current one when drawn.
    pub(super) fn refresh(&mut self, shaders: &Shaders) {
        for &id in &self.ids {
            let Some(set) = self.by_material[id as usize].as_mut() else {
                continue;
            };
            let current = &shaders.sets[(set.slot as usize).min(shaders.sets.len() - 1)];
            if !Arc::ptr_eq(&set.shader, current) {
                set.shader = current.clone();
                set.pipelines = std::array::from_fn(|_| OnceLock::new());
            }
        }
    }

    /// A freed shader's materials keep their stencil state with the built-in shader.
    pub(super) fn use_builtin_shader(&mut self, shaders: &Shaders, material: usize) {
        if let Some(Some(set)) = self.by_material.get_mut(material) {
            set.slot = 0;
            set.shader = shaders.sets[0].clone();
            set.pipelines = std::array::from_fn(|_| OnceLock::new());
        }
    }

    /// Whether material `id` has a stencil state.
    pub(super) fn has(&self, id: u32) -> bool {
        self.by_material.get(id as usize).is_some_and(Option::is_some)
    }

    /// Material `id`'s stencil reference, if it has a stencil state.
    pub(super) fn reference(&self, id: u32) -> Option<u32> {
        self.by_material.get(id as usize)?.as_ref().map(|s| s.reference)
    }

    /// Whether any live material has a stencil state.
    pub(super) fn any(&self) -> bool {
        !self.ids.is_empty()
    }

    /// Material `id`'s pipeline of `role` for `instancing`, built now if
    /// needed; `read_only`: its stencil writes off. `None` when it has no
    /// stencil or `role` no stencil pipeline (the caller then uses the
    /// shader's own).
    pub(super) fn pipeline(
        &self,
        id: u32,
        instancing: Instancing,
        role: PipelineRole,
        read_only: bool,
    ) -> Option<&wgpu::RenderPipeline> {
        let set = self.by_material.get(id as usize)?.as_ref()?;
        let r = ROLES.iter().position(|&x| x == role)?;
        let n = INSTANCING.iter().position(|&x| x == instancing)?;
        let slot = (n * ROLES.len() + r) * 2 + usize::from(read_only);
        set.pipelines[slot]
            .get_or_init(|| {
                let mut state = set.state.clone();
                if read_only {
                    state.write_mask = 0;
                }
                let variant = ShaderVariant::new(set.sides, instancing);
                let start = crate::perf::Instant::now();
                let built = set.shader.builder.build_stencil(variant, role, state);
                set.shader.counters.built(start.elapsed());
                built
            })
            .as_ref()
    }

    /// Where material `id` draws among the solid ones: 0 without a stencil
    /// (first), otherwise after them by its [`Stencil::order`].
    pub(super) fn rank(&self, id: u32) -> u64 {
        match self.by_material.get(id as usize) {
            Some(Some(set)) => 1 + (i64::from(set.order) - i64::from(i32::MIN)) as u64,
            _ => 0,
        }
    }
}

/// The stencil state of `stencil`.
pub(super) fn state(stencil: &Stencil) -> wgpu::StencilState {
    wgpu::StencilState {
        front: face(&stencil.front),
        back: face(&stencil.back),
        read_mask: u32::from(stencil.read_mask),
        write_mask: u32::from(stencil.write_mask),
    }
}

fn face(face: &StencilFace) -> wgpu::StencilFaceState {
    wgpu::StencilFaceState {
        compare: compare(face.compare),
        fail_op: op(face.fail),
        depth_fail_op: op(face.depth_fail),
        pass_op: op(face.pass),
    }
}

fn compare(c: StencilCompare) -> wgpu::CompareFunction {
    use wgpu::CompareFunction as C;
    match c {
        StencilCompare::Never => C::Never,
        StencilCompare::Less => C::Less,
        StencilCompare::Equal => C::Equal,
        StencilCompare::LessEqual => C::LessEqual,
        StencilCompare::Greater => C::Greater,
        StencilCompare::NotEqual => C::NotEqual,
        StencilCompare::GreaterEqual => C::GreaterEqual,
        StencilCompare::Always => C::Always,
    }
}

fn op(o: StencilOp) -> wgpu::StencilOperation {
    use wgpu::StencilOperation as O;
    match o {
        StencilOp::Keep => O::Keep,
        StencilOp::Zero => O::Zero,
        StencilOp::Replace => O::Replace,
        StencilOp::Invert => O::Invert,
        StencilOp::IncrementClamp => O::IncrementClamp,
        StencilOp::DecrementClamp => O::DecrementClamp,
        StencilOp::IncrementWrap => O::IncrementWrap,
        StencilOp::DecrementWrap => O::DecrementWrap,
    }
}

/// The stencil operations of a scene depth pass whose depth has
/// `depth_ops`: none without stencil bits, otherwise the same access as the
/// depth (cleared to 0 with it, read-only with it), so the two aspects
/// never disagree.
pub(super) fn ops(stencil: bool, depth_ops: Option<wgpu::Operations<f32>>) -> Option<wgpu::Operations<u32>> {
    let depth = depth_ops.filter(|_| stencil)?;
    Some(wgpu::Operations {
        load: match depth.load {
            wgpu::LoadOp::Clear(_) => wgpu::LoadOp::Clear(0),
            _ => wgpu::LoadOp::Load,
        },
        store: wgpu::StoreOp::Store,
    })
}
