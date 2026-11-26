//! More cameras in the window (`SceneSettings::views`): each one's own
//! frame state, swapped in while it is prepared and drawn.
//!
//! A window view is a whole frame of its camera, drawn after the main one
//! and submitted before the next view is prepared (so every view can reuse
//! the renderer's uniform and light buffers). What is the view's own, and
//! must outlive a frame, lives in a [`ViewState`]: its scene-sized targets
//! (with the temporal history images), its temporal history, its
//! screen-space passes (temporal AO and SSR keep their last frame), SMAA's
//! targets, its auto exposure (the exposure it adapted to, and its meter), its draw lists and its instance buffer (each view's own, so a
//! still view uploads nothing). Everything else (meshes, materials,
//! pipelines, skinning, shadow maps, target cameras, global illumination)
//! is shared, and the main frame alone updates it: views skip those steps
//! (`Renderer3D::extra`).

use super::*;

/// One window view's frame state (see the module docs).
pub(super) struct ViewState {
    targets: Option<Targets>,
    history: History,
    screen: ScreenSpace,
    smaa: Option<crate::render::smaa::Smaa>,
    metering: Option<super::post::Metering>,
    lists: FrameLists,
    instances: wgpu::Buffer,
    instance_mirror: Mirror,
    capacity: usize,
}

impl ViewState {
    fn new(device: &wgpu::Device, taa_supported: bool) -> Self {
        let capacity = 64;
        Self {
            targets: None,
            history: History::default(),
            screen: ScreenSpace::new(taa_supported),
            smaa: None,
            metering: None,
            lists: FrameLists::default(),
            instances: Renderer3D::instance_buffer(device, capacity),
            instance_mirror: Mirror::default(),
            capacity,
        }
    }

    /// A reload changed the scene's shading: the view's history is dropped
    /// the next time it draws.
    pub(super) fn content_changed(&mut self) {
        self.history.content_changed = true;
    }

    /// Counts the view's own targets and buffer (see `memory.rs`).
    pub(super) fn tally_memory(&self, t: &mut crate::render::memory::MemoryTally) {
        use crate::render::memory::MemoryClass as C;
        if let Some(targets) = &self.targets {
            targets.tally_memory(t);
        }
        if let Some(smaa) = &self.smaa {
            smaa.tally_memory(t);
        }
        if let Some(metering) = &self.metering {
            metering.tally_memory(t);
        }
        t.buffer(C::InstanceBatches, "view instances", &self.instances);
    }

    /// The size its scene is drawn at, once drawn (test hook).
    #[cfg(test)]
    pub(super) fn scene_size(&self) -> Option<(u32, u32)> {
        self.targets.as_ref().map(|t| t.size)
    }
}

impl Renderer3D {
    /// Ends the 3D frame in `format` from now on (the window switched to
    /// HDR output's frame image while the game runs): the passes writing
    /// the window's image are made again; the scene's images and SMAA's,
    /// the main view's and each window view's, with the next frame.
    pub(crate) fn set_output_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        self.post.set_output_format(device, format);
        for view in &mut self.extra_views {
            view.smaa = None;
        }
    }

    /// The format the 3D frame ends in (tests).
    #[cfg(test)]
    pub(crate) fn output_format(&self) -> wgpu::TextureFormat {
        self.post.output_format()
    }

    /// Trades the renderer's frame state with view `index`'s.
    fn swap_view(&mut self, index: usize) {
        let v = &mut self.extra_views[index];
        std::mem::swap(&mut self.targets, &mut v.targets);
        std::mem::swap(&mut self.history, &mut v.history);
        std::mem::swap(&mut self.screen, &mut v.screen);
        self.post.swap_smaa(&mut v.smaa);
        self.post.swap_metering(&mut v.metering);
        std::mem::swap(&mut self.lists, &mut v.lists);
        std::mem::swap(&mut self.instances, &mut v.instances);
        std::mem::swap(&mut self.instance_mirror, &mut v.instance_mirror);
        std::mem::swap(&mut self.capacity, &mut v.capacity);
    }

    /// Keeps the state of `count` window views: new ones start empty (no
    /// history), the ones past `count` are freed.
    pub(crate) fn keep_views(&mut self, device: &wgpu::Device, count: usize) {
        let taa_supported = self.screen.taa_supported;
        self.extra_views.truncate(count);
        while self.extra_views.len() < count {
            self.extra_views.push(ViewState::new(device, taa_supported));
        }
    }

    /// How many window views keep state.
    pub(crate) fn view_count(&self) -> usize {
        self.extra_views.len()
    }

    /// Prepares window view `index` (kept by [`keep_views`](Self::keep_views))
    /// from its own frame in `input`, after the main frame (`main`) was
    /// drawn and submitted: the view draws its poses and reads its global
    /// illumination.
    pub(crate) fn prepare_view_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        index: usize,
        input: &Prepare3D<'_>,
        main: &crate::three::Frame3D,
    ) {
        self.swap_view(index);
        self.extra = true;
        self.batch_writes(device, queue, |r| r.prepare_extra(device, queue, input, main));
        self.extra = false;
        self.swap_view(index);
    }

    /// Draws window view `index`, prepared by
    /// [`prepare_view_frame`](Self::prepare_view_frame), into `view` (its
    /// size). The buffers must be submitted before the next view is prepared.
    pub(crate) fn draw_view_frame(
        &mut self,
        device: &wgpu::Device,
        index: usize,
        view: &wgpu::TextureView,
        clear: wgpu::Color,
        textures: &dyn TextureTable,
        timer: Option<&GpuTimer>,
    ) -> FrameBuffers {
        self.swap_view(index);
        self.extra = true;
        let buffers = self.draw(device, view, clear, textures, timer);
        self.extra = false;
        self.swap_view(index);
        buffers
    }

    /// Window view `index`'s auto exposure, while it is on (test hook).
    #[cfg(test)]
    pub(super) fn view_metering(&self, index: usize) -> Option<&super::post::Metering> {
        self.extra_views.get(index).and_then(|v| v.metering.as_ref())
    }

    /// The size window view `index`'s scene is drawn at (test hook).
    #[cfg(test)]
    pub(crate) fn view_scene_size(&self, index: usize) -> Option<(u32, u32)> {
        self.extra_views.get(index).and_then(ViewState::scene_size)
    }

    /// Draw calls and instances of the window views' last frames, on top
    /// of the main view's (`draw_calls`, `counts`).
    pub(crate) fn views_draw_calls(&mut self) -> usize {
        (0..self.extra_views.len())
            .map(|i| {
                self.swap_view(i);
                let n = self.draw_calls();
                self.swap_view(i);
                n
            })
            .sum()
    }
}
