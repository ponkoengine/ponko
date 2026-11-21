//! Input a system feeds reaches the retained UI in the frame it was fed.
//!
//! The UI takes the frame's input before the first system (so systems see
//! its focus, events and screens). A system that feeds input later (a
//! replay, a test, a bot pressing a menu button) would otherwise reach the
//! UI only on the next frame, after the frame's edges were cleared: a tap
//! fed by a system never clicked anything. Now the engine runs the UI again
//! before the next system (and once after the last), over only what was
//! fed since: the edges the first pass took are hidden from it, its clock
//! does not move, and the events it queues join the frame's. Events a pass
//! after the last system queues are kept for the next frame's systems.
//! The pass lays out only what the input changed (see `retained_layout`).

use super::Ui;
use crate::ctx::Ctx;

/// A [`Ui`]'s state across the passes of one frame.
#[derive(Debug, Default)]
pub(super) struct SameFrame {
    /// The next [`Ui::frame`] is a second pass of this frame: it keeps the
    /// events, the closed screens and the clock of the first.
    pub(super) again: bool,
    /// How many events at the back of the queue a same-frame pass queued:
    /// the next frame keeps those nobody took yet.
    pub(super) carried: usize,
}

impl Ui {
    /// The start of [`Ui::frame`]: `true` for a second pass in the same
    /// frame, which keeps what the first queued; otherwise the frame's
    /// queue starts empty but for the events a late pass of the frame
    /// before queued that no system took yet.
    pub(super) fn start_frame_pass(&mut self) -> bool {
        if std::mem::take(&mut self.same_frame.again) {
            return true;
        }
        let keep = std::mem::take(&mut self.same_frame.carried).min(self.events.len());
        let old = self.events.len() - keep;
        self.events.drain(..old);
        false
    }

    /// Marks the next [`Ui::frame`] as a second pass of this frame; returns
    /// how many events are queued now.
    fn begin_same_frame_pass(&mut self) -> usize {
        self.same_frame.again = true;
        self.events.len()
    }

    /// After a second pass: the events it queued are carried to the next
    /// frame if no system takes them first.
    fn end_same_frame_pass(&mut self, queued_before: usize) {
        self.same_frame.again = false;
        self.same_frame.carried += self.events.len().saturating_sub(queued_before);
    }
}

impl Ctx {
    /// Between systems: if a system fed input since the retained UI took
    /// this frame's, the UI takes what is new now, so the next system sees
    /// the click, the focus or the closed screen it made.
    #[inline]
    pub(crate) fn feed_ui_same_frame(&mut self) {
        if self.input.fed_since_ui() {
            self.ui_same_frame_pass();
        }
    }

    #[cold]
    fn ui_same_frame_pass(&mut self) {
        self.input.hide_seen_edges();
        let before = self.ui.begin_same_frame_pass();
        #[cfg(feature = "3d")]
        self.world
            .query::<&mut super::panel3d::UiPanel3D>()
            .for_each(|p| p.ui.same_frame.again = true);
        self.update_retained_ui();
        #[cfg(feature = "3d")]
        self.world
            .query::<&mut super::panel3d::UiPanel3D>()
            .for_each(|p| p.ui.same_frame.again = false);
        self.ui.end_same_frame_pass(before);
        self.input.show_seen_edges();
    }
}
