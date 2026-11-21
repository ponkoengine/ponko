//! Groups that fade as one image ([`Ui::set_fade_as_group`]).
//!
//! Opacity is otherwise applied to each piece a node draws: a panel at half
//! opacity shows its own fill through its buttons, and a child through the
//! sibling it overlaps. A node marked to fade as a group is drawn, while it
//! is partly transparent, with its whole subtree at full opacity into a
//! texture of the window's size ([`DrawList::offscreen`]), which one quad
//! then shows at the group's opacity: what is behind the group shows
//! through it evenly, nothing inside it shows through anything else.
//!
//! The layer is used only while a marked group is between transparent and
//! opaque: at rest, or with no marked group, nothing changes. Its nodes'
//! opacity relative to the group stays the same while the group fades, so
//! their drawings are kept and repeated ([`super::retained_draw`]) instead
//! of drawn again each frame of the fade.
//!
//! Only the screen's UI fades groups this way; world-space panels
//! (`UiPanel3D`) still fade each piece.

use super::retained::{Ui, UiId};
use crate::color::Color;
use crate::ctx::Ctx;
use crate::render::batch::{DrawList, SpriteInstance};
use crate::sprite_material::{BlendMode, style_key};
use glam::Vec2;

impl Ui {
    /// Make node `id` and everything inside it fade as one image: while
    /// its opacity (its own, from [`animate`](Self::animate) or a screen
    /// transition, or an ancestor's) is between 0 and 1, the group is drawn
    /// opaque into an offscreen layer that is then shown at that opacity.
    /// Overlapping children no longer show through each other, and a
    /// panel's fill no longer shows through its buttons. Off by default:
    /// each piece is faded on its own, which costs nothing extra.
    ///
    /// The layer is a texture of the window's size, drawn only while the
    /// group is partly transparent. Groups inside a fading group get their
    /// own layer. World-space panels (`UiPanel3D`) fade each piece.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{UiLayout, UiTween};
    /// let mut ui = ponko::ui::Ui::default();
    /// let menu = ui.root().layout(UiLayout::fill()).id();
    /// ui.set_fade_as_group(menu, true);
    /// ui.animate(menu, UiTween::opacity(0.0).seconds(0.3));
    /// assert!(ui.fades_as_group(menu));
    /// ```
    pub fn set_fade_as_group(&mut self, id: UiId, on: bool) {
        if self.node(id).is_none() {
            return;
        }
        let at = self.fade_groups.iter().position(|&g| g == id);
        match (on, at) {
            (true, None) => self.fade_groups.push(id),
            (false, Some(at)) => {
                self.fade_groups.swap_remove(at);
            }
            _ => {}
        }
    }

    /// Whether node `id` fades as one image
    /// ([`set_fade_as_group`](Self::set_fade_as_group)); `false` for an id
    /// not in the tree.
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// assert!(!ui.fades_as_group(root));
    /// ```
    pub fn fades_as_group(&self, id: UiId) -> bool {
        self.node(id).is_some() && self.fade_groups.contains(&id)
    }
}

/// One group drawn into a layer this frame.
#[derive(Clone, Copy)]
struct Layer {
    /// Its root's place among the visible nodes.
    root: usize,
    /// The list it is shown in: 0 the screen's, `k` layer `k - 1`'s.
    parent: usize,
    /// Its root's opacity, its ancestors' included.
    opacity: f32,
    /// How opaque it is shown in its parent list.
    alpha: f32,
    texture: u32,
}

/// The engine's side: this frame's layers and the lists drawn into, kept
/// between frames so that a fade allocates nothing after its first frame.
#[derive(Default)]
pub(crate) struct FadeLayers {
    /// The screen's UI is drawn now (not a world-space panel's).
    pub(crate) screen: bool,
    layers: Vec<Layer>,
    /// Per visible node, the list it is drawn into (as `Layer::parent`).
    list_of: Vec<u32>,
    /// The ancestors of the node looked at, with the list each one's
    /// children go into.
    path: Vec<(UiId, usize)>,
    /// The nodes whose opacity was made relative to their layer, and what
    /// it was.
    saved: Vec<(UiId, f32)>,
    /// The lists not drawn into now: 0 the screen's, `k` layer `k - 1`'s;
    /// the one drawn into now is `Graphics::ui_list`, an empty one in its
    /// place here.
    parked: Vec<DrawList>,
    /// The list drawn into now.
    current: usize,
    viewport: Vec2,
}

impl Ctx {
    /// Before drawing `ui`'s nodes: which go into layers this frame, their
    /// opacity made relative to their layer's. Whether any does: when not
    /// (no group fading, the common case), nothing else is done.
    pub(super) fn begin_fade_layers(&mut self, ui: &mut Ui, viewport: Vec2) -> bool {
        if ui.fade_groups.is_empty() || !self.ui_state.fade.screen {
            return false;
        }
        let mut groups = std::mem::take(&mut ui.fade_groups);
        groups.retain(|&g| ui.node(g).is_some());
        ui.fade_groups = groups;
        let state = &mut self.ui_state.fade;
        state.layers.clear();
        state.list_of.clear();
        state.path.clear();
        for (order, &id) in ui.visible.iter().enumerate() {
            let Some(n) = ui.node(id) else {
                state.list_of.push(0);
                continue;
            };
            // `visible` is in tree order: its parent is on the path.
            while state.path.last().is_some_and(|&(top, _)| Some(top) != n.parent) {
                state.path.pop();
            }
            let parent = state.path.last().map_or(0, |&(_, list)| list);
            let mut list = parent;
            let opacity = n.motion.xf.opacity;
            if opacity > 0.0 && ui.fade_groups.contains(&id) {
                let outer = parent.checked_sub(1).map_or(1.0, |k| state.layers[k].opacity);
                let alpha = (opacity / outer).min(1.0);
                if alpha < 1.0 {
                    state.layers.push(Layer {
                        root: order,
                        parent,
                        opacity,
                        alpha,
                        texture: 0,
                    });
                    list = state.layers.len();
                }
            }
            state.path.push((id, list));
            state.list_of.push(list as u32);
        }
        if state.layers.is_empty() {
            return false;
        }
        for (i, layer) in state.layers.iter_mut().enumerate() {
            layer.texture = self.gfx.renderer.as_mut().map_or(0, |r| r.ui_layer_texture(i));
        }
        let lists = state.layers.len() + 1;
        if state.parked.len() < lists {
            state.parked.resize_with(lists, DrawList::default);
        }
        for list in &mut state.parked[1..lists] {
            list.clear();
        }
        state.current = 0;
        state.viewport = viewport;
        // Drawn opaque in their layer: their opacity relative to its root's.
        state.saved.clear();
        for order in 0..ui.visible.len() {
            let list = state.list_of[order] as usize;
            if list == 0 {
                continue;
            }
            let id = ui.visible[order];
            let of = state.layers[list - 1].opacity;
            if let Some(n) = ui.node_mut(id) {
                state.saved.push((id, n.motion.xf.opacity));
                n.motion.xf.opacity = (n.motion.xf.opacity / of).min(1.0);
            }
        }
        true
    }

    /// Before drawing the `order`th visible node: its list made the one
    /// drawn into. A layer's root first shows the layer in its parent list.
    pub(super) fn enter_fade_layer(&mut self, ui: &Ui, order: usize) {
        let list = self.ui_state.fade.list_of[order] as usize;
        if list == self.ui_state.fade.current {
            return;
        }
        if list > 0 {
            let layer = self.ui_state.fade.layers[list - 1];
            if layer.root == order {
                self.switch_fade_list(layer.parent);
                let clip = ui.node(ui.visible[order]).map(|n| n.motion.shown_clip);
                let viewport = self.ui_state.fade.viewport;
                let shown = SpriteInstance::new(
                    [0.0, 0.0],
                    viewport.into(),
                    [0.0, 0.0],
                    0.0,
                    Color::rgba(1.0, 1.0, 1.0, layer.alpha).to_rgba8(),
                );
                let list = &mut self.gfx.ui_list;
                list.set_clip(clip);
                // The layer holds colors times their alpha.
                list.push_styled(
                    layer.texture,
                    style_key(BlendMode::Premultiplied, None),
                    super::retained_draw::node_z(order),
                    shown,
                );
            }
        }
        self.switch_fade_list(list);
    }

    /// After drawing `ui`'s nodes: back to the screen's list, which takes
    /// this frame's layers, and the nodes' opacity as it was.
    pub(super) fn end_fade_layers(&mut self, ui: &mut Ui) {
        self.switch_fade_list(0);
        let state = &mut self.ui_state.fade;
        let slots = self.gfx.ui_list.offscreen_slots(state.layers.len());
        for (k, (slot, layer)) in slots.iter_mut().zip(&state.layers).enumerate() {
            slot.texture = layer.texture;
            std::mem::swap(&mut slot.list, &mut state.parked[k + 1]);
        }
        for &(id, opacity) in &state.saved {
            if let Some(n) = ui.node_mut(id) {
                n.motion.xf.opacity = opacity;
            }
        }
        state.saved.clear();
    }

    /// Makes list `to` (as `Layer::parent`) the one drawn into.
    fn switch_fade_list(&mut self, to: usize) {
        let state = &mut self.ui_state.fade;
        if to == state.current {
            return;
        }
        std::mem::swap(&mut self.gfx.ui_list, &mut state.parked[state.current]);
        std::mem::swap(&mut self.gfx.ui_list, &mut state.parked[to]);
        state.current = to;
    }
}

#[cfg(test)]
mod tests {
    use crate::ctx::Ctx;
    use crate::prelude::*;
    use crate::render::batch::{Batch, SpriteInstance};
    use crate::sprite_material::{BlendMode, style_key};
    use crate::ui::{UiId, UiInput, UiLayout, UiTween};

    const VIEW: Vec2 = Vec2::new(320.0, 240.0);

    fn ctx() -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, VIEW);
        ctx.gfx.stand_in_textures();
        ctx
    }

    /// A group of two overlapping opaque squares, shown; returns the group
    /// and the squares.
    fn squares(ctx: &mut Ctx, parent: Option<UiId>) -> (UiId, [UiId; 2]) {
        let ui = &mut ctx.ui;
        let group = match parent {
            Some(p) => ui.panel(p),
            None => ui.root(),
        }
        .layout(UiLayout::fill())
        .background(Color::TRANSPARENT)
        .border_width(0.0)
        .id();
        let mut ids = [group; 2];
        for (i, x) in [0.1, 0.3].into_iter().enumerate() {
            ids[i] = ui
                .panel(group)
                .layout(UiLayout::anchored(x, 0.1, 0.4, 0.4))
                .background(Color::rgb(x, 0.0, 1.0 - x))
                .border_width(0.0)
                .id();
        }
        if parent.is_none() {
            ui.push_modeless(group);
        }
        (group, ids)
    }

    struct Drawn {
        redrawn: u64,
        sprites: Vec<SpriteInstance>,
        batches: Vec<Batch>,
        /// Each layer's texture, sprites and batches.
        layers: Vec<(u32, Vec<SpriteInstance>, Vec<Batch>)>,
    }

    /// The UI's frame `dt` seconds on, then its drawing as a window draws it.
    fn frame(ctx: &mut Ctx, dt: f32) -> Drawn {
        let input = UiInput {
            pointer: vec2(-10.0, -10.0),
            dt,
            ..Default::default()
        };
        ctx.ui.frame_with_text(VIEW, &input, &mut ctx.gfx.text);
        let before = ctx.ui.node_redraws();
        ctx.draw_retained_ui();
        let redrawn = ctx.ui.node_redraws() - before;
        let (sprites, batches, layers) = ctx.gfx.ui_list.finish_with_offscreen();
        let drawn = Drawn {
            redrawn,
            sprites: sprites.to_vec(),
            batches: batches.to_vec(),
            layers: layers
                .iter()
                .map(|l| (l.texture, l.finished().0.to_vec(), l.finished().1.to_vec()))
                .collect(),
        };
        ctx.gfx.ui_list.clear();
        drawn
    }

    /// The alpha of each sprite that shows something (the group's own
    /// transparent fill shows nothing).
    fn alphas(sprites: &[SpriteInstance]) -> Vec<u8> {
        sprites.iter().map(|s| s.color[3]).filter(|&a| a > 0).collect()
    }

    /// The sprites that show a layer: drawn premultiplied.
    fn shown_layers(d: &Drawn) -> Vec<&SpriteInstance> {
        shown_in(&d.sprites, &d.batches)
    }

    fn shown_in<'a>(sprites: &'a [SpriteInstance], batches: &[Batch]) -> Vec<&'a SpriteInstance> {
        let premultiplied = style_key(BlendMode::Premultiplied, None);
        batches
            .iter()
            .filter(|b| b.material == premultiplied)
            .flat_map(|b| &sprites[b.start as usize..b.end as usize])
            .collect()
    }

    #[test]
    fn a_ui_whose_groups_do_not_fade_draws_no_layer() {
        // Fading piece by piece: no layer, each piece half transparent.
        let mut faded = ctx();
        let (group, _) = squares(&mut faded, None);
        faded.ui.animate(group, UiTween::opacity(0.5).seconds(0.0));
        let d = frame(&mut faded, 0.0);
        assert!(d.layers.is_empty());
        assert!(!d.sprites.is_empty() && alphas(&d.sprites).iter().all(|&a| a == 128));

        // A group marked to fade as one, at rest: no layer either, and the
        // same sprites as without the mark.
        let mut plain = ctx();
        squares(&mut plain, None);
        let mut marked = ctx();
        let (group, _) = squares(&mut marked, None);
        marked.ui.set_fade_as_group(group, true);
        let (a, b) = (frame(&mut plain, 0.0), frame(&mut marked, 0.0));
        assert!(b.layers.is_empty(), "at rest the group draws no layer");
        assert_eq!(a.sprites, b.sprites);
        assert_eq!(a.batches, b.batches);
    }

    #[test]
    fn a_fading_group_draws_opaque_into_a_layer_shown_once_at_its_opacity() {
        let mut ctx = ctx();
        let (group, [a, _]) = squares(&mut ctx, None);
        ctx.ui.set_fade_as_group(group, true);
        ctx.ui.animate(group, UiTween::opacity(0.5).seconds(0.0));
        let d = frame(&mut ctx, 0.0);
        assert_eq!(d.layers.len(), 1, "one layer");
        let (_, layer, _) = &d.layers[0];
        assert!(
            !layer.is_empty() && alphas(layer).iter().all(|&a| a == 255),
            "{:?}",
            alphas(layer)
        );
        // The screen's list holds the layer's quad and nothing else.
        assert_eq!(d.sprites.len(), 1, "only the layer is shown");
        let shown = shown_layers(&d);
        assert_eq!(shown.len(), 1);
        assert_eq!((shown[0].pos, shown[0].size), ([0.0, 0.0], VIEW.into()));
        assert_eq!(shown[0].color, [255, 255, 255, 128]);
        // What the game reads stays the opacity it is drawn at.
        assert_eq!(ctx.ui.drawn(a).unwrap().opacity, 0.5);
        assert_eq!(ctx.ui.drawn(group).unwrap().opacity, 0.5);
    }

    #[test]
    fn a_fading_group_keeps_its_nodes_drawings_through_the_fade() {
        for as_group in [false, true] {
            let mut ctx = ctx();
            let (group, _) = squares(&mut ctx, None);
            ctx.ui.set_fade_as_group(group, as_group);
            frame(&mut ctx, 0.0);
            ctx.ui.animate(
                group,
                UiTween::opacity(0.0)
                    .seconds(1.0)
                    .with_ease(crate::skeleton2d::Bezier::LINEAR),
            );
            frame(&mut ctx, 0.1);
            let mut redrawn = 0;
            let mut shown = Vec::new();
            for _ in 0..5 {
                let d = frame(&mut ctx, 0.1);
                redrawn += d.redrawn;
                shown.extend(shown_layers(&d).iter().map(|s| s.color[3]));
            }
            if as_group {
                assert_eq!(redrawn, 0, "the layer's nodes are repeated, only the layer fades");
                assert_eq!(shown.len(), 5);
                assert!(shown.windows(2).all(|w| w[1] < w[0]), "fading: {shown:?}");
            } else {
                // Negative control: piece by piece, all three are drawn again
                // every frame.
                assert_eq!(redrawn, 15);
                assert!(shown.is_empty());
            }
        }
    }

    #[test]
    fn a_group_fading_inside_a_fading_group_has_a_layer_of_its_own() {
        let mut ctx = ctx();
        let (outer, _) = squares(&mut ctx, None);
        let (inner, _) = squares(&mut ctx, Some(outer));
        ctx.ui.set_fade_as_group(outer, true);
        ctx.ui.set_fade_as_group(inner, true);
        ctx.ui.animate(outer, UiTween::opacity(0.5).seconds(0.0));
        ctx.ui.animate(inner, UiTween::opacity(0.5).seconds(0.0));
        let d = frame(&mut ctx, 0.0);
        assert_eq!(d.layers.len(), 2, "the outer group's layer, then the inner one's");
        let shown: Vec<_> = shown_layers(&d).iter().map(|s| s.color[3]).collect();
        assert_eq!(shown, [128], "the screen shows the outer layer");
        let (_, sprites, batches) = &d.layers[0];
        let inside: Vec<_> = shown_in(sprites, batches).iter().map(|s| s.color[3]).collect();
        assert_eq!(inside, [128], "the outer layer shows the inner one at its own opacity");
        let (_, inner_sprites, _) = &d.layers[1];
        assert!(alphas(inner_sprites).iter().all(|&a| a == 255));
        assert_eq!(ctx.ui.drawn(inner).unwrap().opacity, 0.25);
    }

    #[test]
    fn an_ancestors_fade_fades_a_marked_group_as_one_image() {
        let mut ctx = ctx();
        let (outer, _) = squares(&mut ctx, None);
        let (inner, _) = squares(&mut ctx, Some(outer));
        ctx.ui.set_fade_as_group(inner, true);
        ctx.ui.animate(outer, UiTween::opacity(0.5).seconds(0.0));
        let d = frame(&mut ctx, 0.0);
        assert_eq!(d.layers.len(), 1);
        assert_eq!(shown_layers(&d).iter().map(|s| s.color[3]).collect::<Vec<_>>(), [128]);
        // The outer group's own squares fade piece by piece.
        let outer_alphas = alphas(&d.sprites);
        assert!(
            outer_alphas.len() >= 3 && outer_alphas.iter().all(|&a| a == 128),
            "{outer_alphas:?}"
        );
    }

    #[test]
    fn a_removed_group_fades_no_more() {
        let mut ctx = ctx();
        let (group, _) = squares(&mut ctx, None);
        let (inner, _) = squares(&mut ctx, Some(group));
        ctx.ui.set_fade_as_group(inner, true);
        assert!(ctx.ui.fades_as_group(inner));
        ctx.ui.remove(inner);
        assert!(!ctx.ui.fades_as_group(inner));
        ctx.ui.animate(group, UiTween::opacity(0.5).seconds(0.0));
        assert!(frame(&mut ctx, 0.0).layers.is_empty());
        assert!(ctx.ui.fade_groups.is_empty(), "forgotten once drawn");
        // Unmarking works too.
        ctx.ui.set_fade_as_group(group, true);
        ctx.ui.set_fade_as_group(group, false);
        assert!(!ctx.ui.fades_as_group(group));
    }
}
