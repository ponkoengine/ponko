//! Reading compute results back on the CPU: a [`GpuBuffer`] range or a
//! [`RenderTarget`] rectangle, asked for on one frame and answered on a
//! later one ([`Graphics::read_gpu_buffer`], [`Graphics::read_render_target`],
//! [`Graphics::readback`]).
//!
//! The engine side lives here: each read's slot, its state and its values,
//! reused once the game has read them. The renderer copies the data into a
//! staging buffer after the frame's passes and maps it
//! (`render/readback.rs`); [`Graphics::collect_readbacks`] picks up what
//! arrived at the start of each frame.

use super::compute::GpuBuffer;
use super::target::RenderTarget;
use crate::error::{Error, ErrorKind};
use crate::gfx::Graphics;
use crate::handle::{INDEX_MASK, SlotIds};
use glam::{UVec2, Vec4};

/// A read of GPU memory back to the CPU that is under way: made by
/// [`Graphics::read_gpu_buffer`] or [`Graphics::read_render_target`], and
/// answered by [`Graphics::readback`] on a later frame. A plain id, cheap
/// to copy and keep in a resource or a component.
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let totals = ctx.gfx.create_gpu_buffer(4, &[]);
///     let pending: ponko::render::Readback = ctx.gfx.read_gpu_buffer(totals, 0..4);
///     // Never answered on the frame it was asked for.
///     assert!(ctx.gfx.readback(pending).is_none());
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)] // shared with C/C++ (`PonkoReadback`)
pub struct Readback {
    pub(crate) id: u32,
}

/// Where a read stands.
#[derive(Debug)]
enum State {
    /// Waiting for the GPU.
    Pending,
    /// Arrived: the values are in the slot.
    Done,
    /// Can't be answered: what went wrong.
    Failed(ErrorKind, &'static str),
}

/// One read: its state, the frame it was asked for, the frame the game
/// first got its answer (it is reused after that frame), and its values.
#[derive(Debug)]
struct Slot {
    state: State,
    asked: u64,
    answered: Option<u64>,
    /// Kept between reads, so a slot used again doesn't allocate.
    values: Vec<Vec4>,
    live: bool,
}

/// More finished reads than this, none of them read yet, is a game that
/// asks and never collects: the engine says so once per run (never drops
/// them; there is no way to free a read but to read it).
pub(crate) const UNCOLLECTED_LIMIT: usize = 64;

/// Every read of the game, by slot (see [`Readback`]).
#[derive(Debug, Default)]
pub(crate) struct Readbacks {
    slots: Vec<Slot>,
    ids: SlotIds,
    /// The uncollected-results warning was given (once per run).
    warned: bool,
}

/// Why a read can't be answered where no GPU runs.
const NO_GPU: &str = "no GPU reads anything back here: a headless run or a build without the `window` feature runs no compute passes; \
                      test GPU results with a windowed or offscreen run";

impl Readbacks {
    /// A new read asked for on `frame`.
    fn start(&mut self, frame: u64) -> Readback {
        let id = self.ids.make(self.slots.len(), "readback");
        let slot = (id & INDEX_MASK) as usize;
        if slot == self.slots.len() {
            self.slots.push(Slot {
                state: State::Pending,
                asked: frame,
                answered: None,
                values: Vec::new(),
                live: true,
            });
        }
        let s = &mut self.slots[slot];
        s.state = State::Pending;
        s.asked = frame;
        s.answered = None;
        s.values.clear();
        s.live = true;
        Readback { id }
    }

    fn slot_mut(&mut self, read: Readback) -> Option<&mut Slot> {
        let slot = self.ids.slot(read.id)?;
        self.slots.get_mut(slot).filter(|s| s.live)
    }

    /// The renderer's answer to read `id` (its whole id, as the renderer
    /// was given it): ignored when the read was given up meanwhile.
    pub(crate) fn arrive(&mut self, id: u32, values: Result<&[Vec4], (ErrorKind, &'static str)>) {
        let Some(slot) = self.slot_mut(Readback { id }) else {
            return;
        };
        if !matches!(slot.state, State::Pending) {
            return;
        }
        match values {
            Ok(values) => {
                slot.values.clear();
                slot.values.extend_from_slice(values);
                slot.state = State::Done;
            }
            Err((kind, why)) => slot.state = State::Failed(kind, why),
        }
    }

    /// Start of frame `frame`: reads answered on an earlier frame are
    /// reused from now on, and with no GPU (`gpu: false`), every read
    /// still waiting fails.
    fn settle(&mut self, frame: u64, gpu: bool) {
        let mut uncollected = 0;
        for (slot, s) in self.slots.iter_mut().enumerate() {
            if !s.live {
                continue;
            }
            if s.answered.is_some_and(|f| f < frame) {
                s.live = false;
                let id = self.ids.id_in(slot);
                self.ids.release(id);
                continue;
            }
            if !gpu && matches!(s.state, State::Pending) {
                s.state = State::Failed(ErrorKind::Unsupported, NO_GPU);
            }
            if s.answered.is_none() && !matches!(s.state, State::Pending) {
                uncollected += 1;
            }
        }
        if uncollected > UNCOLLECTED_LIMIT && !self.warned {
            self.warned = true;
            Self::warn_uncollected(uncollected);
        }
    }

    /// Says (once per run) that `waiting` finished reads wait for the game.
    fn warn_uncollected(waiting: usize) {
        crate::render::traffic::warn(&format!(
            "ponko: {waiting} readback results are waiting to be collected (more than {UNCOLLECTED_LIMIT}): \
             the game asks for GPU data and never reads it, so they pile up in memory and nothing frees them. \
             Read each result with `ctx.gfx.readback(read)`, which also frees it, or stop asking for \
             results you don't read"
        ));
    }

    /// How many reads are live (asked for and not reused yet).
    #[cfg(test)]
    fn live(&self) -> usize {
        self.slots.iter().filter(|s| s.live).count()
    }
}

impl Graphics {
    /// Asks for elements `range` of `buffer` back on the CPU, as they are
    /// once this frame's compute passes have run. The values arrive
    /// through [`readback`](Self::readback) on a later frame, typically one
    /// to three frames from now: read the handle again each frame until
    /// they do.
    ///
    /// Read a few values, not a whole simulation each frame: a second
    /// compute pass can sum up the grid on the GPU (cells burning per
    /// region, a total) into a small buffer, and only that is read. The
    /// copy and the staging memory cost about what is read.
    ///
    /// GPU results are **not deterministic**: floating point differs
    /// between GPUs and drivers, and the frame an answer arrives on
    /// varies. Use them for presentation and single-player rules, never in
    /// a `sim_system`, a replay, rollback or lockstep networking (see
    /// STABILITY.md).
    ///
    /// Where nothing runs compute passes, the read fails (with
    /// [`ErrorKind::Unsupported`]) instead of returning the CPU's data: in
    /// headless runs and builds without a window, and on GPUs without
    /// storage buffers (WebGL2), where a GPU buffer has no GPU copy.
    ///
    /// # Panics
    ///
    /// For a buffer that is freed or from another game, or an empty range
    /// or one that doesn't fit in the buffer.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let counts = ctx.gfx.create_gpu_buffer(64, &[]);
    ///     let pending = ctx.gfx.read_gpu_buffer(counts, 0..4); // elements 0 to 3
    ///     # let _ = pending;
    /// });
    /// ```
    #[track_caller]
    pub fn read_gpu_buffer(&mut self, buffer: GpuBuffer, range: std::ops::Range<u32>) -> Readback {
        let len = self.gpu_buffer_len(buffer).expect(
            "ponko: GPU buffer was freed or belongs to another game; read a live buffer made with create_gpu_buffer",
        );
        assert!(
            range.start < range.end && range.end <= len,
            "ponko: read_gpu_buffer range must hold at least one element and fit in the buffer's {len}; got {range:?}"
        );
        let read = self.assets3d.readbacks.start(self.frame);
        if let Some(r) = self.renderer.as_mut()
            && !self.headless
        {
            r.read_gpu_buffer(read.id, buffer.slot(), range.start, range.end - range.start);
        }
        read
    }

    /// Asks for the texels of `target` in the rectangle at `at` (its top
    /// left corner, in pixels from the target's top left) of `size` back
    /// on the CPU, as they are once this frame's passes have written it.
    /// The values arrive through [`readback`](Self::readback) on a later
    /// frame, row by row from the top, `size.x` texels a row; each texel
    /// is the target's linear RGBA (half floats on the GPU, so about three
    /// significant digits).
    ///
    /// As with [`read_gpu_buffer`](Self::read_gpu_buffer): read small
    /// rectangles (a target a compute pass reduced to a few texels), the
    /// values are not deterministic, and in headless runs and builds
    /// without a window the read fails with [`ErrorKind::Unsupported`].
    /// WebGL2 reads render targets too.
    ///
    /// # Panics
    ///
    /// For a render target that is freed or from another game, or an
    /// empty rectangle or one that doesn't fit in the target.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let heat = ctx.gfx.create_render_target(64, 64);
    ///     // The 2 x 2 texels at the top left.
    ///     let pending = ctx.gfx.read_render_target(heat, uvec2(0, 0), uvec2(2, 2));
    ///     # let _ = pending;
    /// });
    /// ```
    #[track_caller]
    pub fn read_render_target(&mut self, target: RenderTarget, at: UVec2, size: UVec2) -> Readback {
        assert!(
            self.textures_live.contains(target.texture.id) && self.is_render_target(target.texture),
            "ponko: render target was freed or belongs to another game; read a live one made with create_render_target"
        );
        let (width, height) = (target.width(), target.height());
        assert!(
            size.x > 0
                && size.y > 0
                && at.x.checked_add(size.x).is_some_and(|x| x <= width)
                && at.y.checked_add(size.y).is_some_and(|y| y <= height),
            "ponko: read_render_target rectangle must be at least 1 x 1 and fit in the {width} x {height} target; got {size} at {at}"
        );
        let read = self.assets3d.readbacks.start(self.frame);
        if let Some(r) = self.renderer.as_mut()
            && !self.headless
        {
            r.read_render_target(read.id, target.texture.id, [at.x, at.y], [size.x, size.y]);
        }
        read
    }

    /// The answer to `read`, once it has arrived: `None` while the GPU is
    /// still working on it (always on the frame it was asked for), then
    /// the values (`Some(Ok(..))`) or why there are none (`Some(Err(..))`,
    /// see [`read_gpu_buffer`](Self::read_gpu_buffer)). A buffer read gives
    /// one `Vec4` per element, a render target read one per texel.
    ///
    /// Once answered, the answer stays for the rest of that frame; from
    /// the next frame `read` is used up and gives
    /// [`ErrorKind::BadHandle`], as a handle from another game does. Read
    /// every read you ask for: an answer nobody reads is kept, because
    /// reading it is the only way to free it (there is no `free` for a
    /// [`Readback`]). The engine never drops a result silently; when more
    /// than 64 finished results are waiting to be read, it logs one
    /// warning per run (naming `readback`) and keeps them all.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// #[derive(Default)]
    /// struct Fire {
    ///     counts: Option<ponko::render::GpuBuffer>,
    ///     pending: Option<ponko::render::Readback>,
    ///     burning: f32,
    /// }
    ///
    /// fn read_fire(ctx: &mut Ctx) {
    ///     let mut fire = ctx.world.remove_resource::<Fire>().unwrap_or_default();
    ///     let counts = *fire.counts.get_or_insert_with(|| ctx.gfx.create_gpu_buffer(4, &[]));
    ///     // One read in flight at a time: ask again once the last one is answered.
    ///     let pending = *fire.pending.get_or_insert_with(|| ctx.gfx.read_gpu_buffer(counts, 0..1));
    ///     match ctx.gfx.readback(pending) {
    ///         None => {}
    ///         Some(Ok(values)) => {
    ///             fire.burning = values[0].x;
    ///             fire.pending = None;
    ///         }
    ///         Some(Err(e)) => {
    ///             // No GPU here (headless): keep the last value.
    ///             assert_eq!(e.kind(), ErrorKind::Unsupported);
    ///             fire.pending = None;
    ///         }
    ///     }
    ///     ctx.world.insert_resource(fire);
    /// }
    /// # Game::new("t", 1, 1).headless_frames(true).system(read_fire).run_headless(3, |_| {});
    /// ```
    pub fn readback(&mut self, read: Readback) -> Option<Result<&[Vec4], Error>> {
        let frame = self.frame;
        let Some(slot) = self.assets3d.readbacks.slot_mut(read) else {
            return Some(Err(Error::new(
                ErrorKind::BadHandle,
                "this readback was used up (read on an earlier frame) or belongs to another game; ask again with read_gpu_buffer or read_render_target",
            )));
        };
        if slot.asked >= frame {
            return None;
        }
        let answer = match slot.state {
            State::Pending => return None,
            State::Done => Ok(&slot.values[..]),
            State::Failed(kind, why) => Err(Error::new(kind, why)),
        };
        slot.answered.get_or_insert(frame);
        Some(answer)
    }

    /// Start of a frame (after `frame` is set): takes the answers the
    /// renderer has, retires reads answered on an earlier frame, and with
    /// no GPU fails the reads still waiting.
    pub(crate) fn collect_readbacks(&mut self) {
        let gpu = !self.headless && self.renderer.is_some();
        let readbacks = &mut self.assets3d.readbacks;
        if gpu && let Some(r) = self.renderer.as_mut() {
            r.collect_readbacks(|id, values| readbacks.arrive(id, values));
        }
        readbacks.settle(self.frame, gpu);
    }

    /// End of a frame, after it was drawn: the renderer copies what this
    /// frame's reads asked for.
    pub(crate) fn submit_readbacks(&mut self) {
        if let Some(r) = self.renderer.as_mut()
            && !self.headless
        {
            r.submit_readbacks();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_are_answered_once_and_their_slots_reused() {
        let mut r = Readbacks::default();
        let a = r.start(1);
        r.arrive(a.id, Ok(&[Vec4::X]));
        assert!(matches!(r.slot_mut(a).unwrap().state, State::Done));
        // Answered on frame 2: kept for frame 2, reused from frame 3.
        r.slot_mut(a).unwrap().answered = Some(2);
        r.settle(2, true);
        assert_eq!(r.live(), 1);
        r.settle(3, true);
        assert_eq!(r.live(), 0);
        let b = r.start(3);
        assert_eq!(b.id & INDEX_MASK, a.id & INDEX_MASK, "the slot is reused");
        assert_ne!(a, b, "under a new id");
        assert!(r.slot_mut(a).is_none());
        // An answer for the old id doesn't land in the new read.
        r.arrive(a.id, Ok(&[Vec4::Y]));
        assert!(matches!(r.slot_mut(b).unwrap().state, State::Pending));
    }

    #[test]
    fn more_than_the_limit_of_uncollected_results_warns_once() {
        let mut r = Readbacks::default();
        let reads: Vec<_> = (0..UNCOLLECTED_LIMIT).map(|_| r.start(1)).collect();
        for read in &reads {
            r.arrive(read.id, Ok(&[Vec4::X]));
        }
        r.settle(2, true);
        assert!(!r.warned, "exactly the limit is fine");
        // Reads still waiting for the GPU don't count: they aren't results yet.
        let pending = r.start(2);
        r.settle(3, true);
        assert!(!r.warned);
        let one_more = r.start(2);
        r.arrive(one_more.id, Ok(&[Vec4::Y]));
        r.settle(3, true);
        assert!(r.warned, "one over the limit warns");
        // Nothing is dropped: every result is still there to be read.
        assert_eq!(r.live(), UNCOLLECTED_LIMIT + 2);
        assert!(matches!(r.slot_mut(reads[0]).unwrap().state, State::Done));
        assert!(matches!(r.slot_mut(pending).unwrap().state, State::Pending));
    }

    #[test]
    fn results_that_are_read_never_count_as_uncollected() {
        let mut r = Readbacks::default();
        // A game that asks every frame and reads each answer: never warns.
        for frame in 1..=200u64 {
            let read = r.start(frame);
            r.arrive(read.id, Ok(&[Vec4::X]));
            r.slot_mut(read).unwrap().answered = Some(frame + 1);
            r.settle(frame + 1, true);
        }
        assert!(!r.warned);
        assert!(r.live() <= 2, "slots are reused: {} live", r.live());
    }

    #[test]
    fn without_a_gpu_waiting_reads_fail() {
        let mut r = Readbacks::default();
        let a = r.start(1);
        r.settle(1, true);
        assert!(matches!(r.slot_mut(a).unwrap().state, State::Pending));
        r.settle(2, false);
        assert!(matches!(
            r.slot_mut(a).unwrap().state,
            State::Failed(ErrorKind::Unsupported, _)
        ));
    }
}
