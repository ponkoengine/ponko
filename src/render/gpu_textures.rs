//! The window renderer's texture table: one slot per texture, and a freed
//! texture's slot given to the next texture made, under a new id (the
//! scheme of [`SlotIds`]). A game that makes and frees textures all
//! session keeps a table the size of the most it held at once.
//!
//! Every lookup checks the id's generation: an id kept past its free never
//! reaches the texture made in its slot after it, it sees a 1x1 white
//! stand-in (what a freed texture always showed).

use super::gpu::GpuTexture;
use crate::handle::{INDEX_BITS, INDEX_MASK, SlotIds};

struct Slot {
    texture: GpuTexture,
    /// Freed: the slot shows the white stand-in until it is given out again.
    freed: bool,
}

#[derive(Default)]
pub(super) struct GpuTextures {
    slots: Vec<Slot>,
    ids: SlotIds,
    /// The 1x1 white texture (with its bind group) every freed texture's
    /// id shows; made at the first free.
    blank: Option<GpuTexture>,
}

impl GpuTextures {
    /// Adds `texture` in a freed slot or a new one; returns its id.
    pub(super) fn add(&mut self, texture: GpuTexture) -> u32 {
        let id = self.ids.make(self.slots.len(), "texture");
        let slot = Slot { texture, freed: false };
        match self.slots.get_mut((id & INDEX_MASK) as usize) {
            Some(old) => *old = slot,
            None => self.slots.push(slot),
        }
        id
    }

    /// What texture `id` shows: its own texture while live, the white
    /// stand-in once freed (also after its slot went to a newer texture).
    /// `None` for an id never given out, such as `Texture::NONE`.
    pub(super) fn get(&self, id: u32) -> Option<&GpuTexture> {
        let slot = (id & INDEX_MASK) as usize;
        let entry = self.slots.get(slot)?;
        match self.ids.slot(id) {
            Some(_) => Some(&entry.texture),
            // An older generation of a slot given out since: freed.
            None if id >> INDEX_BITS < self.ids.id_in(slot) >> INDEX_BITS => self.blank.as_ref(),
            None => None,
        }
    }

    /// Texture `id` if it is live (made and not freed).
    pub(super) fn live(&self, id: u32) -> Option<&GpuTexture> {
        let slot = self.ids.slot(id)?;
        self.slots.get(slot).filter(|s| !s.freed).map(|s| &s.texture)
    }

    /// Texture `id` to change, if it is live.
    pub(super) fn live_mut(&mut self, id: u32) -> Option<&mut GpuTexture> {
        let slot = self.ids.slot(id)?;
        self.slots.get_mut(slot).filter(|s| !s.freed).map(|s| &mut s.texture)
    }

    /// The white stand-in, if a texture was freed yet.
    pub(super) fn blank(&self) -> Option<&GpuTexture> {
        self.blank.as_ref()
    }

    /// Sets the white stand-in (before the first [`free`](Self::free)).
    pub(super) fn set_blank(&mut self, blank: GpuTexture) {
        self.blank = Some(blank);
    }

    /// Frees live texture `id`: its slot shows the white stand-in until the
    /// next texture made takes it. False if `id` wasn't live.
    pub(super) fn free(&mut self, id: u32) -> bool {
        let blank = self.blank.clone().expect("the white stand-in is made before a free");
        let Some(slot) = self.ids.slot(id) else {
            return false;
        };
        let Some(entry) = self.slots.get_mut(slot).filter(|s| !s.freed) else {
            return false;
        };
        *entry = Slot {
            texture: blank,
            freed: true,
        };
        self.ids.release(id);
        true
    }

    /// Slots in the table, live or freed.
    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Every slot's texture (freed ones show the white stand-in).
    pub(super) fn iter(&self) -> impl Iterator<Item = &GpuTexture> {
        self.slots.iter().map(|s| &s.texture)
    }
}

/// Looks textures up by id, for the passes that draw them: the renderer's
/// [`GpuTextures`], or a plain list indexed by id (render tests and
/// benchmarks that build their own). Shared with the threads that record
/// 3D passes (none on the web).
pub(super) trait TextureTable: Shared {
    /// What texture `id` shows (see [`GpuTextures::get`]).
    fn texture(&self, id: u32) -> Option<&GpuTexture>;
}

impl TextureTable for GpuTextures {
    fn texture(&self, id: u32) -> Option<&GpuTexture> {
        self.get(id)
    }
}

impl TextureTable for Vec<GpuTexture> {
    fn texture(&self, id: u32) -> Option<&GpuTexture> {
        self.get(id as usize)
    }
}

impl<const N: usize> TextureTable for [GpuTexture; N] {
    fn texture(&self, id: u32) -> Option<&GpuTexture> {
        self.get(id as usize)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) trait Shared: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> Shared for T {}
#[cfg(target_arch = "wasm32")]
pub(super) trait Shared {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> Shared for T {}

/// The view of a render target's first level: what passes render into
/// (a view of every level can't be an attachment).
pub(crate) fn first_level(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("render target"),
        base_mip_level: 0,
        mip_level_count: Some(1),
        ..Default::default()
    })
}
