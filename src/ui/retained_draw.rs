use super::retained::*;
use crate::input::{GamepadButton, MouseButton};
use crate::render::batch::{SpriteInstance, uv_region};
use crate::{camera::Rect, color::Color, ctx::Ctx, input::Key, text::TextParams};
use glam::Vec2;

/// A click an input scenario asked for: where, and the text typed after it.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScriptedClick {
    pub(crate) at: Vec2,
    pub(crate) text: String,
}

impl Ctx {
    pub(crate) fn update_retained_ui(&mut self) {
        self.localize_ui();
        let text = std::mem::take(&mut self.ui_state.retained_input_text);
        let mut input = self.retained_input(&self.ui, text);
        if let Some(click) = self.ui_state.scripted.take() {
            // Pressed and released in one frame: the UI clicks what is under
            // the pointer, as a quick tap of the mouse does.
            input.pointer = click.at;
            input.pressed = true;
            input.held = false;
            input.released = true;
            if !click.text.is_empty() {
                // Typed at the end of what the field holds.
                input.end = true;
                input.text.push_str(&click.text);
            }
        }
        let shortcut = self.input.key_down(Key::LeftCtrl)
            || self.input.key_down(Key::RightCtrl)
            || self.input.key_down(Key::LeftSuper)
            || self.input.key_down(Key::RightSuper);
        if shortcut && !self.input.composing() {
            if [Key::A, Key::C, Key::X, Key::V]
                .iter()
                .any(|k| self.input.key_pressed(*k))
            {
                input.text.clear();
            }
            if (self.input.key_pressed(Key::C) || self.input.key_pressed(Key::X))
                && let Some(text) = self.ui.selected_text().filter(|s| !s.is_empty())
                && self.window.clipboard_write(text).is_ok()
                && self.input.key_pressed(Key::X)
            {
                input.backspace = true;
            }
            if self.input.key_pressed(Key::V) && self.ui.selected_text().is_some() {
                self.window.paste_target = self.ui.focus();
            }
        }
        if let Some(target) = self.window.paste_target {
            if self.input.composing() || self.ui.focus() != Some(target) {
                self.window.paste_target = None;
            } else {
                match self.window.clipboard_read() {
                    Ok(Some(text)) => {
                        input.text.push_str(&text);
                        self.window.paste_target = None;
                    }
                    Err(_) => self.window.paste_target = None,
                    Ok(None) => {}
                }
            }
        }
        // The IME composition goes to the screen's UI only: world panels
        // don't place the candidate window.
        let mut preedit = std::mem::take(&mut self.ui_state.retained_preedit);
        preedit.clear();
        preedit.push_str(self.input.preedit());
        input.preedit = preedit;
        input.preedit_cursor = self.input.preedit_cursor();
        input.composing = self.input.composing();
        self.ui.pixels = self.gfx.scale_factor;
        self.ui.safe_area = Some(self.window.safe_area());
        self.ui.frame_with_text(self.window_size(), &input, &mut self.gfx.text);
        self.ui_state.retained_input_text = input.text;
        self.ui_state.retained_preedit = input.preedit;
        self.follow_ui_ime();
        #[cfg(feature = "3d")]
        self.update_ui_panels();
    }

    /// Keyed UI text follows the string tables (`Ui::localize`): the fonts
    /// tables name are loaded first, falling back to the built-in font.
    pub(crate) fn localize_ui(&mut self) {
        let Some(strings) = self.world.get_resource_mut::<crate::locale::Strings>() else {
            return;
        };
        // Nodes given a key since the last frame are filled in even when the
        // tables didn't change (`Ui::localize` knows).
        if strings.revision() == self.ui_state.localized {
            self.ui.localize(strings);
            return;
        }
        for (language, path) in strings.fonts_to_load() {
            strings.fonts_tried.push((language.clone(), path.clone()));
            match self.gfx.try_load_font(&path) {
                Ok(font) => {
                    if self.gfx.font_fallbacks(font).is_empty() {
                        self.gfx.set_font_fallbacks(font, &[crate::text::Font::DEFAULT]);
                    }
                    strings.set_font(&language, font);
                }
                Err(e) => eprintln!("ponko: the {language} string table's font: {e}"),
            }
        }
        self.ui.localize(strings);
        self.ui_state.localized = strings.revision();
    }

    /// After the UI's frame: IME is allowed while its focus is on a text
    /// input that takes it, with the candidate window at the caret; a
    /// composition the focus left is dropped. When no text input wants IME,
    /// the game's own setting stands, except that turning it off once
    /// follows the UI's text input losing the focus.
    fn follow_ui_ime(&mut self) {
        if self.ui.composition_interrupted() {
            self.input.cancel_composition();
        }
        if self.ui.wants_ime() {
            self.input.set_ime_allowed(true);
            if let Some(area) = self.ui.ime_area() {
                self.input.set_ime_cursor_area(area);
            }
            self.ui_state.ime_owned = true;
        } else if std::mem::take(&mut self.ui_state.ime_owned) {
            self.input.set_ime_allowed(false);
        }
    }

    /// This frame's input for `ui`, as the window's mouse, keyboard and
    /// gamepads give it; `text` (emptied) receives the typed text.
    pub(super) fn retained_input(&self, ui: &crate::ui::Ui, mut text: String) -> crate::ui::UiInput {
        text.clear();
        text.push_str(self.input.typed_text());
        let pressed = |key| self.input.key_pressed(key);
        let pad = |b| self.input.gamepads().iter().any(|&p| self.input.gamepad_pressed(p, b));
        let shift = self.input.key_down(Key::LeftShift) || self.input.key_down(Key::RightShift);
        let ctrl = self.input.key_down(Key::LeftCtrl)
            || self.input.key_down(Key::RightCtrl)
            || self.input.key_down(Key::LeftSuper)
            || self.input.key_down(Key::RightSuper);
        // The mouse's left button, or the first finger when one was used last.
        let (pointer, pointer_pressed, held, released) = self.input.ui_pointer();
        crate::ui::UiInput {
            pointer,
            pressed: pointer_pressed,
            held,
            released,
            wheel: -self.input.mouse_wheel() * 40.0,
            next: pressed(Key::Tab) && !shift,
            previous: pressed(Key::Tab) && shift,
            direction: Vec2::new(
                (pressed(Key::Right) || pad(GamepadButton::DPadRight)) as u8 as f32
                    - (pressed(Key::Left) || pad(GamepadButton::DPadLeft)) as u8 as f32,
                (pressed(Key::Down) || pad(GamepadButton::DPadDown)) as u8 as f32
                    - (pressed(Key::Up) || pad(GamepadButton::DPadUp)) as u8 as f32,
            ),
            confirm: pressed(Key::Enter)
                || pad(GamepadButton::South)
                || (pressed(Key::Space)
                    && !ui
                        .focus()
                        .is_some_and(|id| ui.node(id).is_some_and(|n| n.widget == crate::ui::UiWidget::TextInput))),
            cancel: pressed(Key::Escape) || pad(GamepadButton::East),
            shift,
            select_all: ctrl && pressed(Key::A),
            backspace: pressed(Key::Backspace),
            delete: pressed(Key::Delete),
            home: pressed(Key::Home),
            end: pressed(Key::End),
            text,
            dt: self.time.frame_dt(),
            secondary: self.input.mouse_pressed(MouseButton::Right),
            menu: (shift && pressed(Key::F10)) || pad(GamepadButton::North),
            // Only a listening key-binding field reads it.
            captured: ui
                .capturing()
                .then(|| self.input.pressed_this_frame().find_map(|p| p.button().cloned()))
                .flatten(),
            ..Default::default()
        }
    }

    /// An input scenario's click on the node named `name` (and, with
    /// `text`, typing into it): queued for the retained UI's next frame, at
    /// the centre of the node as the UI lays it out now. Refused, with a
    /// sentence for the report, when no node has that name, it is not shown,
    /// something else is under its centre (a modal, a disabled node, an
    /// overlapping one), or `text` is for a node that is not a text input.
    pub(crate) fn script_ui_click(&mut self, name: &str, text: Option<&str>) -> Result<(), String> {
        let size = self.window_size();
        self.ui.layout(size);
        let ui = &self.ui;
        let id = ui.find(name).ok_or_else(|| format!("no UI node is named `{name}`"))?;
        if !ui.is_shown(id) {
            return Err(format!("UI node `{name}` is not on a shown screen"));
        }
        if text.is_some() && ui.node(id).is_some_and(|n| n.widget != UiWidget::TextInput) {
            return Err(format!("UI node `{name}` is not a text input"));
        }
        let rect = ui.rect(id).unwrap_or_default();
        let at = (rect.min + rect.max) / 2.0;
        let hit = ui.hit_test(at);
        let mut over = hit;
        while let Some(node) = over
            && node != id
        {
            over = ui.parent(node);
        }
        if over != Some(id) {
            let what = match hit {
                Some(other) => match ui.name(other) {
                    Some(other) => format!("`{other}`"),
                    None => "another node".to_string(),
                },
                None => "nothing that takes clicks (disabled, under a modal, or not on the top screen)".to_string(),
            };
            return Err(format!(
                "UI node `{name}` can't be clicked: at its centre ({:.0}, {:.0}) the pointer is over {what}",
                at.x, at.y
            ));
        }
        self.ui_state.scripted = Some(ScriptedClick {
            at,
            text: text.unwrap_or_default().to_string(),
        });
        Ok(())
    }

    /// `ponko_bench`: the retained UI's work before a frame's systems, as a
    /// window's frame does it (this frame's input, layout, events).
    /// Unstable (`bench-internals`).
    #[doc(hidden)]
    #[cfg(feature = "bench-internals")]
    pub fn bench_ui_update(&mut self) {
        self.update_retained_ui();
    }

    /// `ponko_bench`: the retained UI's frame with this frame's input, its
    /// clock moved `dt` seconds (frames of a headless run take no time):
    /// its animations play, it is laid out and sends its events.
    /// Unstable (`bench-internals`).
    #[doc(hidden)]
    #[cfg(feature = "bench-internals")]
    pub fn bench_ui_frame(&mut self, dt: f32) {
        let text = std::mem::take(&mut self.ui_state.retained_input_text);
        let mut input = self.retained_input(&self.ui, text);
        input.dt = dt;
        self.ui.pixels = self.gfx.scale_factor;
        self.ui.safe_area = Some(self.window.safe_area());
        self.ui.frame_with_text(self.window_size(), &input, &mut self.gfx.text);
        self.ui_state.retained_input_text = input.text;
    }

    /// `ponko_bench`: text pushes its glyphs as with a GPU (a headless run
    /// skips them), into stand-in textures. Unstable (`bench-internals`).
    #[doc(hidden)]
    #[cfg(feature = "bench-internals")]
    pub fn bench_ui_glyphs(&mut self) {
        self.gfx.stand_in_textures();
        self.gfx.headless = false;
    }

    /// `ponko_bench`: the retained UI's work after a frame's systems, as a
    /// window's frame does it (layout of what the systems changed, the draw
    /// list), then forgets the draw list. Returns the sprites drawn.
    /// Unstable (`bench-internals`).
    #[doc(hidden)]
    #[cfg(feature = "bench-internals")]
    pub fn bench_ui_draw(&mut self) -> usize {
        self.draw_retained_ui();
        let drawn = self.gfx.ui_list.len();
        self.gfx.ui_list.clear();
        drawn
    }

    pub(crate) fn draw_retained_ui(&mut self) {
        // The language may have changed in this frame's systems.
        self.localize_ui();
        // Move the tree out while issuing draw calls; no callback borrowing the
        // game, context and node tree simultaneously is required.
        let spare = self.ui_state.retained_spare.take().unwrap_or_default();
        let mut ui = std::mem::replace(&mut self.ui, spare);
        self.draw_ui_tree(&mut ui, self.window_size());
        std::mem::swap(&mut self.ui, &mut ui);
        self.ui_state.retained_spare = Some(ui);
    }

    /// Draws `ui`, laid out in `viewport`, into `ui_list`. A node whose
    /// drawing depends on nothing that changed since its last frame
    /// ([`DrawKey`]) pushes the sprites it pushed then, moved to its depth
    /// now (nodes added or removed before it in drawing order don't draw it
    /// again); the others are drawn again, and kept.
    pub(crate) fn draw_ui_tree(&mut self, ui: &mut crate::ui::Ui, viewport: Vec2) {
        // Mutations made by game systems (including screen pushes) show now.
        ui.pixels = self.gfx.scale_factor;
        ui.layout_with_text(viewport, &mut self.gfx.text);
        let frame = FrameKey {
            viewport,
            scale: ui.scale(),
            pixels: self.gfx.scale_factor,
            theme: *ui.theme(),
            fonts: self.gfx.text.revision(),
            atlas: self.gfx.text.atlas_resets(),
            headless: self.gfx.headless,
        };
        let cache = &mut ui.draw_cache;
        if cache.frame != Some(frame) {
            cache.frame = Some(frame);
            cache.epoch += 1;
        }
        let (epoch, keep) = (cache.epoch, !cache.off);
        let mut text = std::mem::take(&mut ui.draw_cache.text);
        for order in 0..ui.visible.len() {
            let id = ui.visible[order];
            let Some(n) = ui.node(id) else { continue };
            let key = DrawKey::of(ui, n);
            shown_text(ui, n, &mut text);
            // A spinner's dots go round with the clock.
            let kept = keep && n.widget != UiWidget::Spinner && n.drawn.is_for(epoch, &key, &text);
            let z = node_z(order);
            if kept {
                self.gfx.ui_list.replay(&n.drawn.sprites, z - n.drawn.z);
                continue;
            }
            let start = self.gfx.ui_list.len();
            self.draw_ui_node(ui, id, order, viewport);
            ui.draw_cache.redraws += 1;
            let glyphs = &mut self.ui_state.glyphs;
            if keep && let Some(n) = ui.node_mut(id) {
                self.gfx.ui_list.record_from(start, &mut n.drawn.sprites);
                n.drawn.keep(epoch, key, &text, z);
                if glyphs.fresh {
                    // Its text was laid out again: kept for its next drawing.
                    std::mem::swap(&mut n.drawn.glyphs, glyphs);
                    n.drawn.glyphs.fresh = false;
                }
            }
            glyphs.fresh = false;
        }
        ui.draw_cache.text = text;
        self.gfx.ui_list.set_clip(None);
        let top = 100.0 + ui.visible.len() as f32 * 4.0 + 8.0;
        self.draw_widget_overlays(ui, top);
    }

    /// Draws node `id`, the `order`th visible one, into `ui_list`.
    fn draw_ui_node(&mut self, ui: &crate::ui::Ui, id: UiId, order: usize, viewport: Vec2) {
        let scale = ui.scale();
        let Some(n) = ui.node(id) else { return };
        let z = node_z(order);
        // Where its animation draws it: everything below is drawn in
        // its place, then moved, scaled and faded together.
        let xf = n.motion.xf;
        if xf.opacity <= 0.0 {
            return;
        }
        if ui.modal_root(id) || ui.exiting_modal(id) {
            self.gfx.ui_list.set_clip(None);
            self.gfx.ui_fill(
                Rect::new(0.0, 0.0, viewport.x, viewport.y),
                Color::rgba(0.0, 0.0, 0.0, 0.55 * xf.opacity),
                z - 1.0,
            );
        }
        let sprites = self.gfx.ui_list.len();
        self.gfx.ui_list.set_clip(Some(n.motion.shown_clip));
        let fill = ui.shown_fill(id).unwrap_or(n.paint.background);
        let width = if ui.focus == Some(id) || ui.is_capturing(id) {
            n.paint.border_width.max(2.0)
        } else {
            n.paint.border_width
        } * scale;
        let radius = ui.corner_radius(id);
        if width > 0.0 && fill.a >= 0.999 {
            self.gfx.ui_fill_rounded(n.rect, n.paint.border, z, radius);
        } else if width > 0.0 {
            // A see-through fill would show a solid border under it.
            super::widgets_draw::outline(&mut self.gfx, n.rect, width, n.paint.border, z);
        }
        let rect = n.rect.expand(-width);
        self.gfx.ui_fill_rounded(rect, fill, z + 0.1, (radius - width).max(0.0));
        if let Some(texture) = n.paint.texture {
            self.retained_image(
                texture,
                rect,
                n.paint.slice * scale,
                n.paint.slice,
                n.paint.color,
                z + 0.2,
            );
        }
        self.gfx
            .ui_list
            .set_clip(Some(intersect(n.motion.shown_clip, xf.map_rect(n.rect))));
        // Every node in the tree has a Taffy node.
        let Ok(layout) = ui.tree.layout(n.layout) else { return };
        // A control keeps what it draws at least three quarters of the theme's
        // spacing from its left and right edges.
        let framed = super::widgets_draw::framed(n.widget);
        let side = |padding: f32| {
            if framed {
                padding.max(ui.theme().spacing * 0.75)
            } else {
                padding
            }
        };
        let inset = Vec2::new(
            side(layout.padding.left) + layout.border.left,
            layout.padding.top + layout.border.top,
        ) * scale;
        let content = Rect {
            min: n.rect.min + inset,
            max: n.rect.max
                - Vec2::new(
                    side(layout.padding.right) + layout.border.right,
                    layout.padding.bottom + layout.border.bottom,
                ) * scale,
        };
        let mut text_rect = content;
        self.draw_widget_parts(ui, id, content, &mut text_rect, z);
        if n.widget == crate::ui::UiWidget::Toggle {
            // The box, centred on the row.
            let side = content.size().y.min(20.0 * scale);
            let top = content.center().y - side * 0.5;
            self.gfx
                .ui_fill(Rect::new(content.min.x, top, side, side), n.paint.border, z + 0.4);
            if n.value > 0.5 {
                self.gfx.ui_fill(
                    Rect::new(
                        content.min.x + 4.0 * scale,
                        top + 4.0 * scale,
                        (side - 8.0 * scale).max(0.0),
                        (side - 8.0 * scale).max(0.0),
                    ),
                    n.paint.color,
                    z + 0.5,
                );
            }
            text_rect.min.x += side + 6.0 * scale;
        }
        if n.widget == crate::ui::UiWidget::Slider {
            self.gfx.ui_fill(
                Rect::new(
                    content.min.x,
                    content.center().y - 2.0 * scale,
                    content.size().x,
                    4.0 * scale,
                ),
                n.paint.border,
                z + 0.4,
            );
            self.gfx.ui_fill(
                Rect::new(
                    content.min.x + n.value * (content.size().x - 8.0 * scale),
                    content.min.y,
                    8.0 * scale,
                    content.size().y,
                ),
                n.paint.color,
                z + 0.5,
            );
        }
        let font_size = n.paint.font_size * scale;
        // A text input shows an IME composition at its caret.
        let is_input = n.widget == crate::ui::UiWidget::TextInput;
        let mut shown = std::mem::take(&mut self.ui_state.shown_scratch);
        let at = is_input.then(|| n.shown_text(&mut shown));
        let text: &str = if is_input {
            &shown
        } else if ui.is_capturing(id) {
            // A key-binding field listening for a key.
            LISTENING
        } else {
            &n.text
        };
        let params = TextParams {
            font: n.paint.font,
            size: font_size,
            color: n.paint.color,
            z: z + 0.7,
        };
        // A text field keeps its text at its left edge, where its caret
        // and selection are measured from; labels and buttons align
        // right-to-left paragraphs from the right.
        let directional = !is_input;
        let flags = (n.paint.wrap, directional);
        let centred = framed && n.rich().is_none();
        // The text keeps its glyphs from its last drawing while what they
        // are laid out from holds: only their depth, clip and color change
        // (an animation of the node's opacity, scale, offset or colors moves
        // and tints them afterwards). A focused text field measures its
        // caret and selection each time it is drawn.
        let cache = &ui.draw_cache;
        let focused = at.is_some() && ui.focus == Some(id);
        let glyph_key = (!cache.off && !focused).then(|| GlyphKey {
            rect: text_rect,
            font: n.paint.font,
            size: font_size,
            wrap: n.paint.wrap,
            directional,
            align: n.paint.align,
            centred,
            rich: n.rich.as_deref().map(|r| r.parsed),
        });
        if let Some(key) = &glyph_key
            && n.drawn.glyphs.is_for(cache.epoch, key, text)
        {
            let glyphs = &n.drawn.glyphs;
            self.gfx
                .ui_list
                .replay_recolored(&glyphs.sprites, z - glyphs.z, &glyphs.tinted, n.paint.color.to_rgba8());
            self.ui_state.shown_scratch = shown;
            self.draw_scroll_thumb(n, scale, z);
            if !xf.is_identity() {
                self.gfx
                    .ui_list
                    .transform_from(sprites, xf.scale, xf.shift.into(), xf.opacity);
            }
            return;
        }
        let mut origin = text_rect.min;
        // A control's text sits in the middle of its height.
        if centred {
            let height = self.retained_text(text, origin, text_rect.size().x, params, flags, n.paint.align, false);
            if height < text_rect.size().y {
                origin.y = text_rect.center().y - height * 0.5;
            }
        }
        if let Some(at) = at.filter(|_| ui.focus == Some(id)) {
            let caret = self
                .gfx
                .caret_x(text, n.paint.font, font_size, at.caret.unwrap_or(at.preedit.end));
            origin.x -= (caret - text_rect.size().x + 3.0 * scale).max(0.0);
            // The selection, or the preedit's underline: one span or
            // several where it crosses a change of direction.
            let mut spans = std::mem::take(&mut self.gfx.selection_scratch);
            if at.preedit.is_empty() {
                self.gfx.selection_spans(
                    text,
                    n.paint.font,
                    font_size,
                    n.anchor.min(n.caret),
                    n.anchor.max(n.caret),
                    &mut spans,
                );
                for &[a, b] in &spans {
                    if a != b {
                        self.gfx.ui_fill(
                            Rect::new(origin.x + a, origin.y, b - a, font_size),
                            n.paint.hover,
                            z + 0.6,
                        );
                    }
                }
            } else {
                let thin = scale.max(1.0);
                let mut underline = |gfx: &mut crate::gfx::Graphics, lo: usize, hi: usize, thick: f32| {
                    gfx.selection_spans(text, n.paint.font, font_size, lo, hi, &mut spans);
                    for &[a, b] in &spans {
                        if a != b {
                            gfx.ui_fill(
                                Rect::new(origin.x + a, origin.y + font_size - thick, b - a, thick),
                                n.paint.color,
                                z + 0.75,
                            );
                        }
                    }
                };
                underline(&mut self.gfx, at.preedit.start, at.preedit.end, thin);
                // The part the IME is converting, underlined thicker.
                if let Some((a, b)) = n.preedit_cursor.filter(|(a, b)| a != b) {
                    let start = at.preedit.start;
                    underline(&mut self.gfx, start + a, start + b, 2.0 * thin);
                }
            }
            self.gfx.selection_scratch = spans;
            if let Some(caret_at) = at.caret {
                let caret = self.gfx.caret_x(text, n.paint.font, font_size, caret_at);
                self.gfx.ui_fill(
                    Rect::new(origin.x + caret, origin.y, scale, font_size),
                    n.paint.color,
                    z + 0.8,
                );
            }
        }
        let start = self.gfx.ui_list.len();
        let mut tinted = std::mem::take(&mut self.ui_state.glyphs.tinted);
        tinted.clear();
        if let Some(rich) = n.rich() {
            self.retained_rich(rich, &n.paint, text_rect, scale, params.z, &mut tinted);
        } else {
            self.retained_text(text, origin, text_rect.size().x, params, flags, n.paint.align, true);
            // Every glyph takes the node's text color.
            tinted.resize(self.gfx.ui_list.len() - start, true);
        }
        let glyphs = &mut self.ui_state.glyphs;
        glyphs.tinted = tinted;
        if let Some(key) = glyph_key {
            self.gfx.ui_list.record_from(start, &mut glyphs.sprites);
            glyphs.epoch = cache.epoch;
            glyphs.key = Some(key);
            glyphs.z = z;
            glyphs.text.clear();
            glyphs.text.push_str(text);
            glyphs.fresh = true;
        }
        self.ui_state.shown_scratch = shown;
        self.draw_scroll_thumb(n, scale, z);
        if !xf.is_identity() {
            self.gfx
                .ui_list
                .transform_from(sprites, xf.scale, xf.shift.into(), xf.opacity);
        }
    }

    /// A scroll area's thumb, when it has something to scroll.
    fn draw_scroll_thumb(&mut self, n: &Node, scale: f32, z: f32) {
        if n.widget == crate::ui::UiWidget::Scroll && n.max_scroll.y > 0.0 {
            let h = n.rect.size().y;
            let thumb = (h * h / (h + n.max_scroll.y * scale)).max(20.0 * scale).min(h);
            let y = n.rect.min.y + (h - thumb) * n.scroll.y / n.max_scroll.y;
            self.gfx.ui_fill(
                Rect::new(n.rect.max.x - 8.0 * scale, y, 6.0 * scale, thumb),
                n.paint.border,
                z + 0.9,
            );
        }
    }
    /// Lays `text` out from `origin` in lines `width` wide and, with `draw`,
    /// draws it; returns how tall it is.
    #[allow(clippy::too_many_arguments)]
    fn retained_text(
        &mut self,
        text: &str,
        origin: Vec2,
        width: f32,
        params: TextParams,
        (wrap, directional): (bool, bool),
        align: crate::ui::UiTextAlign,
        draw: bool,
    ) -> f32 {
        let mut y = origin.y;
        for paragraph in text.split('\n') {
            let mut rest = paragraph;
            loop {
                let end = if wrap {
                    // Measured at the drawn size, in physical pixels.
                    let pixels = self.gfx.scale_factor;
                    wrap_end(
                        &mut self.gfx.text,
                        rest,
                        params.font,
                        params.size * pixels,
                        width * pixels,
                    )
                } else {
                    rest.len()
                };
                let line = &rest[..end];
                let size = self.gfx.measure_text_ex(line, params.font, params.size);
                if !draw {
                    y += size.y;
                    if end == rest.len() {
                        break;
                    }
                    rest = rest[end..].trim_start();
                    continue;
                }
                let w = size.x;
                // Start and End follow the paragraph's direction: a
                // right-to-left paragraph starts at the right.
                let rtl = directional && crate::text::Text::is_rtl(paragraph);
                let x = origin.x
                    + match (align, rtl) {
                        (crate::ui::UiTextAlign::Start, false) | (crate::ui::UiTextAlign::End, true) => 0.0,
                        (crate::ui::UiTextAlign::Center, _) => (width - w) * 0.5,
                        (crate::ui::UiTextAlign::End, false) | (crate::ui::UiTextAlign::Start, true) => width - w,
                    };
                self.gfx.draw_text_ex(line, Vec2::new(x, y), params);
                y += size.y;
                if end == rest.len() {
                    break;
                }
                rest = rest[end..].trim_start();
            }
        }
        y - origin.y
    }
    /// Draws a rich node's text ([`crate::ui::Ui::set_rich`]) in `rect`:
    /// each run in its font, size and color, its images, underlines. Marks
    /// in `tinted` (empty) which sprites take `paint`'s color: those of
    /// runs with no color of their own.
    fn retained_rich(
        &mut self,
        rich: &super::rich::RichText,
        paint: &crate::ui::UiPaint,
        rect: Rect,
        scale: f32,
        z: f32,
        tinted: &mut Vec<bool>,
    ) {
        use super::rich::{Align, Base, bold_offset};
        let start = self.gfx.ui_list.len();
        let base = Base {
            font: paint.font,
            size: paint.font_size,
            scale,
            fonts: true,
            pixels: self.gfx.scale_factor,
        };
        let align = Align {
            align: paint.align,
            width: rect.size().x,
            directional: true,
        };
        let wrap = paint.wrap.then_some(rect.size().x);
        let mut out = std::mem::take(&mut self.ui_state.rich_layout);
        rich.layout(base, wrap, align, &mut self.gfx.text, &mut out);
        for item in &out.items {
            let span = &rich.spans[item.span as usize];
            let (font, size) = rich.font_size(span, base);
            let color = span.style.color.unwrap_or(paint.color);
            let tint = span.style.color.is_none();
            let at = rect.min + item.at;
            if let Some(image) = span.image {
                // Images keep their own colors, and their edges go on
                // physical pixels like the text around them.
                let size = Vec2::new(item.width, item.image_height);
                self.retained_image(
                    image,
                    Rect {
                        min: self.gfx.snap_to_pixels(at),
                        max: self.gfx.snap_to_pixels(at + size),
                    },
                    0.0,
                    0.0,
                    Color::WHITE,
                    z,
                );
                tinted.resize(self.gfx.ui_list.len() - start, false);
            } else {
                let params = TextParams { font, size, color, z };
                let run = &rich.text[item.start as usize..item.end as usize];
                let at = Vec2::new(
                    at.x,
                    self.rich_run_top(rect.min.y + item.baseline, item.baseline - item.at.y),
                );
                self.gfx.draw_text_ex(run, at, params);
                if span.style.bold {
                    self.gfx
                        .draw_text_ex(run, at + Vec2::new(bold_offset(size), 0.0), params);
                }
            }
            if span.style.underline {
                let thick = (size / 16.0).max(1.0);
                let y = rect.min.y + item.baseline + thick;
                self.gfx.ui_fill(Rect::new(at.x, y, item.width, thick), color, z);
            }
            tinted.resize(self.gfx.ui_list.len() - start, tint);
        }
        self.ui_state.rich_layout = out;
    }
    /// The top, in window pixels, to draw a rich text run from so that its
    /// glyphs sit on `baseline` rounded to a physical pixel. `ascent` is
    /// the run's, in window pixels. Glyphs are placed `ascent` rounded to
    /// whole physical pixels below their run's top, which
    /// [`draw_text_ex`](crate::gfx::Graphics::draw_text_ex) snaps; snapping
    /// each run's top on its own would round runs of different fonts and
    /// sizes onto baselines a pixel apart.
    fn rich_run_top(&self, baseline: f32, ascent: f32) -> f32 {
        let pixels = self.gfx.scale_factor;
        ((baseline * pixels).round() - (ascent * pixels).round()) / pixels
    }
    fn retained_image(
        &mut self,
        texture: crate::texture::Texture,
        rect: Rect,
        border: f32,
        source_border: f32,
        color: Color,
        z: f32,
    ) {
        let source = texture.size();
        let src = source_border.min(source.x * 0.5).min(source.y * 0.5).max(0.0);
        let bx = border.min(rect.size().x * 0.5).max(0.0);
        let by = border.min(rect.size().y * 0.5).max(0.0);
        let xs = [0.0, bx, rect.size().x - bx, rect.size().x];
        let ys = [0.0, by, rect.size().y - by, rect.size().y];
        let us = [0.0, src / source.x, 1.0 - src / source.x, 1.0];
        let vs = [0.0, src / source.y, 1.0 - src / source.y, 1.0];
        for y in 0..3 {
            for x in 0..3 {
                let size = Vec2::new(xs[x + 1] - xs[x], ys[y + 1] - ys[y]);
                if size.cmpgt(Vec2::ZERO).all() {
                    self.gfx.ui_list.push(
                        texture.id,
                        z,
                        SpriteInstance::new(
                            (rect.min + Vec2::new(xs[x], ys[y])).into(),
                            size.into(),
                            [0.0, 0.0],
                            0.0,
                            color.to_rgba8(),
                        )
                        .with_uv(uv_region([us[x], vs[y]], [us[x + 1] - us[x], vs[y + 1] - vs[y]])),
                    );
                }
            }
        }
    }
}

/// What a key-binding field shows while it listens.
const LISTENING: &str = "Press a key";

/// The depth of the `order`th visible node: each one 4 above the last, room
/// for its parts in between.
fn node_z(order: usize) -> f32 {
    100.0 + order as f32 * 4.0
}

/// What every node's drawing depends on: when it changes, every node is
/// drawn again.
#[derive(Clone, Copy, PartialEq)]
struct FrameKey {
    /// The size of what it is drawn into (a modal's dim overlay covers it).
    viewport: Vec2,
    scale: f32,
    /// Physical pixels per window pixel: text and edges snap to them.
    pixels: f32,
    /// Corner radii, spacing, the accent of chosen items.
    theme: crate::ui::UiTheme,
    /// The fonts and how text is shaped ([`crate::text::Text::revision`]).
    fonts: u64,
    /// Glyphs move in the atlas when it is emptied.
    atlas: u64,
    /// Headless, text draws no glyphs.
    headless: bool,
}

/// What one node's drawing depends on besides its text and the
/// [`FrameKey`]: when any of it changes, the node is drawn again.
///
/// Its place among the visible nodes is not part of it: kept sprites move
/// to the depth it has now.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct DrawKey {
    rect: Rect,
    clip: Rect,
    /// Where its animation draws it, and how opaque.
    xf: super::anim::Xf,
    /// A modal screen's root draws the dim overlay under it.
    modal: bool,
    /// The fill its state (and a state transition) gives it now.
    fill: Option<Color>,
    focused: bool,
    capturing: bool,
    paint: crate::ui::UiPaint,
    widget: UiWidget,
    /// Its padding and border, from the layout: where its content sits.
    edges: [f32; 8],
    value: f32,
    scroll: Vec2,
    max_scroll: Vec2,
    /// A text field's caret, selection and composition.
    caret: usize,
    anchor: usize,
    preedit_cursor: Option<(usize, usize)>,
    /// The revision a rich node's markup was parsed with.
    rich: Option<u64>,
}

impl DrawKey {
    fn of(ui: &crate::ui::Ui, n: &Node) -> Self {
        let id = n.id;
        let edges = ui.tree.layout(n.layout).map_or([0.0; 8], |l| {
            let (p, b) = (l.padding, l.border);
            [p.left, p.right, p.top, p.bottom, b.left, b.right, b.top, b.bottom]
        });
        Self {
            rect: n.rect,
            clip: n.motion.shown_clip,
            xf: n.motion.xf,
            modal: ui.modal_root(id) || ui.exiting_modal(id),
            fill: ui.shown_fill(id),
            focused: ui.focus == Some(id),
            capturing: ui.is_capturing(id),
            paint: n.paint,
            widget: n.widget,
            edges,
            value: n.value,
            scroll: n.scroll,
            max_scroll: n.max_scroll,
            caret: n.caret,
            anchor: n.anchor,
            preedit_cursor: n.preedit_cursor,
            rich: n.rich.as_deref().map(|r| r.parsed),
        }
    }
}

/// The text node `n` shows now, into `out`: a text field's with its IME
/// composition, a listening key-binding field's prompt, or its own.
fn shown_text(ui: &crate::ui::Ui, n: &Node, out: &mut String) {
    if n.widget == UiWidget::TextInput {
        n.shown_text(out);
    } else {
        out.clear();
        out.push_str(if ui.is_capturing(n.id) { LISTENING } else { &n.text });
    }
}

/// The sprites a node pushed the last time it was drawn, and what it was
/// drawn from.
#[derive(Default)]
pub(super) struct Drawn {
    /// The [`DrawCache`] epoch it was drawn in; 0: never drawn.
    epoch: u64,
    key: Option<DrawKey>,
    text: String,
    /// The depth it was drawn at ([`node_z`]).
    z: f32,
    sprites: Vec<crate::render::batch::Recorded>,
    /// Its text as laid out then, for drawing it again around it.
    glyphs: Glyphs,
}

impl Drawn {
    /// Whether its sprites are what drawing it from `key` and `text` would
    /// push (at the depth it was drawn at).
    fn is_for(&self, epoch: u64, key: &DrawKey, text: &str) -> bool {
        self.epoch == epoch && self.key.as_ref() == Some(key) && self.text == text
    }

    /// Remembers what the sprites just recorded were drawn from.
    fn keep(&mut self, epoch: u64, key: DrawKey, text: &str, z: f32) {
        self.epoch = epoch;
        self.key = Some(key);
        self.z = z;
        if self.text != text {
            self.text.clear();
            self.text.push_str(text);
        }
    }
}

/// What a node's text is laid out from, besides the text itself and the
/// [`FrameKey`]. Its color is not part of it, nor where the node's
/// animation draws it: those are applied to the glyphs kept.
#[derive(Clone, Copy, PartialEq)]
struct GlyphKey {
    /// Where it is laid out: its width, its start, the height a control's
    /// text is centred in.
    rect: Rect,
    font: crate::text::Font,
    /// The font size times the UI's scale.
    size: f32,
    wrap: bool,
    /// Right-to-left paragraphs start at the right.
    directional: bool,
    align: crate::ui::UiTextAlign,
    /// Centred in the height of a control.
    centred: bool,
    /// The revision a rich node's markup was parsed with.
    rich: Option<u64>,
}

/// A node's text as last drawn, before its animation moved it: its glyphs
/// (a rich node's images and underlines too), kept until what they are
/// laid out from changes ([`GlyphKey`]). A node drawn again for anything
/// else (its fill, its colors, its opacity, scale or offset, its depth)
/// pushes these again instead of laying its text out.
#[derive(Default)]
pub(crate) struct Glyphs {
    /// The [`DrawCache`] epoch they were laid out in; 0: never.
    epoch: u64,
    key: Option<GlyphKey>,
    text: String,
    /// The depth of the node they were drawn for ([`node_z`]).
    z: f32,
    sprites: Vec<crate::render::batch::Recorded>,
    /// Which sprites take the node's text color ([`crate::ui::UiPaint::color`]):
    /// every glyph of plain text, a rich node's runs with no color of their own.
    tinted: Vec<bool>,
    /// The engine's scratch copy was just laid out, for the node drawn now.
    fresh: bool,
}

impl Glyphs {
    /// Whether they are what laying out `text` from `key` would give.
    fn is_for(&self, epoch: u64, key: &GlyphKey, text: &str) -> bool {
        self.epoch == epoch && self.key.as_ref() == Some(key) && self.text == text
    }
}

/// What the kept drawings were drawn for, and how many nodes were drawn.
#[derive(Default)]
pub(super) struct DrawCache {
    frame: Option<FrameKey>,
    /// Moves on when `frame` changes: drawings of an older epoch are stale.
    epoch: u64,
    /// Nodes drawn, not taken from what they drew before.
    redraws: u64,
    /// Scratch for a node's shown text.
    text: String,
    /// Tests: every node is drawn again and nothing is kept.
    off: bool,
}

impl crate::ui::Ui {
    /// How many times a node was drawn rather than repeated from its last
    /// frame: a still screen adds none.
    #[cfg(test)]
    pub(crate) fn node_redraws(&self) -> u64 {
        self.draw_cache.redraws
    }
}

#[cfg(test)]
mod tests {
    use crate::ctx::Ctx;
    use crate::prelude::*;
    use crate::render::batch::{Batch, SpriteInstance};
    use crate::texture::Texture;
    use crate::ui::{UiAxis, UiId, UiInput, UiLayout, UiScreenTransition, UiTheme, UiTransition, UiTween};

    const VIEW: Vec2 = Vec2::new(800.0, 600.0);
    /// A named change to the UI, giving the input of the frame that shows it.
    type Step = (&'static str, Box<dyn Fn(&mut Ctx) -> UiInput>);
    /// The glyph atlas's texture id (`stand_in_textures`).
    const GLYPHS: u32 = 1;

    fn ctx() -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, VIEW);
        // Texture ids for the glyph atlas and rectangles, as with a GPU, so
        // text pushes its glyphs.
        ctx.gfx.stand_in_textures();
        ctx
    }

    /// The UI's frame with `input`, then its drawing as a window draws it.
    /// Returns the nodes drawn rather than repeated, and what was drawn.
    fn frame(ctx: &mut Ctx, input: &UiInput) -> (u64, Vec<SpriteInstance>, Vec<Batch>) {
        ctx.ui.frame_with_text(VIEW, input, &mut ctx.gfx.text);
        let before = ctx.ui.node_redraws();
        ctx.draw_retained_ui();
        let redrawn = ctx.ui.node_redraws() - before;
        let (sprites, batches) = ctx.gfx.ui_list.finish();
        let drawn = (sprites.to_vec(), batches.to_vec());
        ctx.gfx.ui_list.clear();
        (redrawn, drawn.0, drawn.1)
    }

    /// [`frame`], then the same frame drawn again with every node drawn
    /// anew: what was kept must be exactly what drawing gives. Returns the
    /// nodes the first drawing drew rather than repeated.
    fn same_as_drawn_anew(ctx: &mut Ctx, input: &UiInput, what: &str) -> u64 {
        let (redrawn, kept, kept_batches) = frame(ctx, input);
        ctx.ui.draw_cache.off = true;
        ctx.draw_retained_ui();
        ctx.ui.draw_cache.off = false;
        let (fresh, fresh_batches) = ctx.gfx.ui_list.finish();
        assert_eq!(kept.len(), fresh.len(), "{what}: as many sprites");
        for (i, (a, b)) in kept.iter().zip(fresh).enumerate() {
            assert_eq!(a, b, "{what}: sprite {i}");
        }
        assert_eq!(kept_batches, fresh_batches, "{what}: the batches");
        ctx.gfx.ui_list.clear();
        redrawn
    }

    struct Hud {
        score: UiId,
        coins: UiId,
        play: UiId,
        menu: UiId,
    }

    /// A HUD of labels and a menu of buttons, drawn once.
    fn hud(ctx: &mut Ctx) -> Hud {
        let ui = &mut ctx.ui;
        let hud = ui.root().layout(UiLayout::fill().padding(8.0)).id();
        let bar = ui.panel(hud).layout(UiLayout::row(30.0).gap(8.0)).id();
        let score = ui.label(bar, "Score: 0").id();
        let coins = ui.label(bar, "Coins: 3").id();
        ui.show_hud(hud);
        let menu = ui.root().layout(UiLayout::centered(300.0, 200.0).gap(8.0)).id();
        let play = ui.button(menu, "Play").layout(UiLayout::row(40.0)).id();
        ui.button(menu, "Quit").layout(UiLayout::row(40.0));
        ui.push_modeless(menu);
        let (redrawn, ..) = frame(ctx, &UiInput::default());
        assert!(redrawn >= 6, "the first frame draws every node: {redrawn}");
        Hud {
            score,
            coins,
            play,
            menu,
        }
    }

    #[test]
    fn still_frames_draw_no_node_again() {
        let mut ctx = ctx();
        let h = hud(&mut ctx);
        let (_, first, batches) = frame(&mut ctx, &UiInput::default());
        assert!(
            batches.iter().any(|b| b.texture == GLYPHS),
            "text is drawn and kept too"
        );
        for _ in 0..10 {
            // A HUD system writing the text the label already shows.
            ctx.ui.set_text(h.score, "Score: 0");
            let (redrawn, drawn, _) = frame(&mut ctx, &UiInput::default());
            assert_eq!(redrawn, 0, "nothing changed");
            assert_eq!(drawn, first, "and the same is drawn");
        }
    }

    #[test]
    fn an_emptied_glyph_atlas_draws_every_node_again() {
        let mut ctx = ctx();
        hud(&mut ctx);
        assert_eq!(frame(&mut ctx, &UiInput::default()).0, 0);
        // Huge glyphs fill the atlas; the next frame starts it over, and
        // the glyphs kept would point at what is no longer there.
        let many: String = ('A'..='Z').chain('a'..='z').collect();
        ctx.gfx.text.layout(&many, crate::text::Font::DEFAULT, 400.0, true);
        assert!(ctx.gfx.text.begin_frame());
        let nodes = ctx.ui.visible.len() as u64;
        assert_eq!(
            same_as_drawn_anew(&mut ctx, &UiInput::default(), "atlas emptied"),
            nodes
        );
        assert_eq!(frame(&mut ctx, &UiInput::default()).0, 0);
    }

    #[test]
    fn a_changed_node_alone_is_drawn_again() {
        let mut ctx = ctx();
        let h = hud(&mut ctx);
        frame(&mut ctx, &UiInput::default());
        // A fixed-size label: its text changes, nothing moves.
        ctx.ui.set_layout(h.coins, UiLayout::fixed(120.0, 30.0));
        frame(&mut ctx, &UiInput::default());
        ctx.ui.set_text(h.coins, "Coins: 4");
        assert_eq!(same_as_drawn_anew(&mut ctx, &UiInput::default(), "text"), 1);
        assert_eq!(frame(&mut ctx, &UiInput::default()).0, 0, "and kept again");
        let mut paint = ctx.ui.paint(h.coins).unwrap();
        paint.color = Color::YELLOW;
        ctx.ui.set_paint(h.coins, paint);
        assert_eq!(same_as_drawn_anew(&mut ctx, &UiInput::default(), "paint"), 1);
        // The pointer onto a button: that button's fill changes.
        let over = UiInput {
            pointer: ctx.ui.rect(h.play).unwrap().center(),
            ..Default::default()
        };
        assert_eq!(same_as_drawn_anew(&mut ctx, &over, "hover"), 1);
        assert_eq!(frame(&mut ctx, &over).0, 0, "resting on it");
        // A label that grows moves the ones after it in its row.
        ctx.ui.set_text(h.score, "Score: 1,000,000");
        assert_eq!(same_as_drawn_anew(&mut ctx, &over, "a wider label"), 2);
        // An animation draws its node and those under it each frame it plays.
        ctx.ui.animate(h.menu, UiTween::opacity(0.5).seconds(0.1));
        let step = UiInput {
            dt: 0.05,
            ..over.clone()
        };
        assert_eq!(
            same_as_drawn_anew(&mut ctx, &step, "fading"),
            3,
            "the menu and its two buttons"
        );
        same_as_drawn_anew(&mut ctx, &step, "faded");
        assert_eq!(frame(&mut ctx, &step).0, 0, "the animation ended");
    }

    #[test]
    fn a_changed_subtree_alone_is_drawn_again() {
        let mut ctx = ctx();
        let h = hud(&mut ctx);
        // More of the menu, drawn after the HUD.
        for i in 0..6 {
            let row = ctx.ui.panel(h.menu).layout(UiLayout::row(20.0)).id();
            ctx.ui.label(row, format!("Option {i}"));
        }
        frame(&mut ctx, &UiInput::default());
        assert_eq!(frame(&mut ctx, &UiInput::default()).0, 0);
        let bar = ctx.ui.parent(h.score).unwrap();
        let menu_nodes = ctx.ui.visible.len() - ctx.ui.visible.iter().position(|&n| n == h.menu).unwrap();
        assert!(menu_nodes > 10, "{menu_nodes}");
        // A slot added to the HUD: every node of the menu comes later in
        // drawing order, but none of them changed.
        let slot = ctx.ui.panel(bar).layout(UiLayout::fixed(30.0, 30.0)).id();
        ctx.ui.label(slot, "9");
        assert_eq!(
            same_as_drawn_anew(&mut ctx, &UiInput::default(), "a slot added"),
            2,
            "the slot and its label"
        );
        assert_eq!(frame(&mut ctx, &UiInput::default()).0, 0, "and kept");
        // Removed again: the menu comes back to its depth, still kept.
        ctx.ui.remove(slot);
        assert_eq!(same_as_drawn_anew(&mut ctx, &UiInput::default(), "a slot removed"), 0);
        // The HUD's row moves: it and what is in it, nothing else.
        ctx.ui.set_layout(bar, UiLayout::row(30.0).gap(8.0).padding(4.0));
        assert_eq!(
            same_as_drawn_anew(&mut ctx, &UiInput::default(), "the HUD's row moved"),
            3,
            "the row and its two labels"
        );
        // A dialog over it all, and gone again: drawn alone.
        let dialog = ctx.ui.root().layout(UiLayout::centered(200.0, 100.0)).id();
        ctx.ui.button(dialog, "OK").layout(UiLayout::fill());
        ctx.ui.push_modal(dialog);
        assert_eq!(same_as_drawn_anew(&mut ctx, &UiInput::default(), "a dialog"), 2);
        ctx.ui.pop_screen();
        assert_eq!(same_as_drawn_anew(&mut ctx, &UiInput::default(), "no dialog"), 0);
    }

    /// The strings laid out so far.
    fn layouts(ctx: &Ctx) -> u64 {
        ctx.gfx.text.layouts()
    }

    #[test]
    fn animated_nodes_keep_their_text_laid_out() {
        let mut ctx = ctx();
        ctx.ui.set_theme(UiTheme {
            transition: UiTransition::new(0.2),
            ..UiTheme::default()
        });
        let image = Texture::new(10, 32, 32);
        ctx.ui.set_inline_image("coin", image);
        let ui = &mut ctx.ui;
        let menu = ui.root().layout(UiLayout::centered(400.0, 400.0).gap(4.0)).id();
        let title = ui.label(menu, "Settings").id();
        let play = ui.button(menu, "Play").layout(UiLayout::row(28.0)).id();
        let rich = ui
            .label(menu, "[color=#ff0]Gold[/color] [img=coin] [u]x3[/u]")
            .rich()
            .id();
        ui.label(menu, "A paragraph long enough to wrap onto a second line in its row.")
            .wrap();
        ui.toggle(menu, "Music").layout(UiLayout::row(28.0));
        ui.dropdown(menu, ["Low", "High"]).layout(UiLayout::row(28.0));
        ui.text_input(menu).text("Ada").layout(UiLayout::row(28.0));
        ui.push_modeless(menu);
        same_as_drawn_anew(&mut ctx, &UiInput::default(), "first frame");
        let step = UiInput {
            dt: 0.05,
            ..Default::default()
        };
        let animations: [(&str, UiId, UiTween); 6] = [
            ("opacity", menu, UiTween::opacity(0.4)),
            ("scale", menu, UiTween::scale(1.2)),
            ("offset", play, UiTween::offset(vec2(30.0, -10.0))),
            ("text color", title, UiTween::text_color(Color::RED)),
            ("rich text color", rich, UiTween::text_color(Color::GREEN)),
            ("background", play, UiTween::background(Color::BLUE)),
        ];
        for (what, id, tween) in animations {
            ctx.ui.animate(id, tween.seconds(1.0));
            for _ in 0..5 {
                let before = layouts(&ctx);
                let (redrawn, ..) = frame(&mut ctx, &step);
                assert!(redrawn > 0, "{what}: the node is drawn again");
                assert_eq!(layouts(&ctx), before, "{what}: no text laid out");
                // What it draws is what drawing anew gives.
                same_as_drawn_anew(&mut ctx, &step, what);
            }
        }
        // The pointer onto the button: its fill fades to the hovered one.
        let over = UiInput {
            pointer: ctx.ui.rect(play).unwrap().center(),
            dt: 0.05,
            ..Default::default()
        };
        for _ in 0..5 {
            let before = layouts(&ctx);
            frame(&mut ctx, &over);
            assert_eq!(layouts(&ctx), before, "hover: no text laid out");
            same_as_drawn_anew(&mut ctx, &over, "hover");
        }
        // The control: new text is laid out.
        ctx.ui.set_text(title, "Options");
        let before = layouts(&ctx);
        same_as_drawn_anew(&mut ctx, &UiInput::default(), "new text");
        assert!(layouts(&ctx) > before, "the new text is laid out");
    }

    #[test]
    fn what_is_kept_is_what_drawing_again_gives() {
        let mut ctx = ctx();
        let image = Texture::new(10, 32, 32);
        ctx.ui.set_inline_image("coin", image);
        ctx.ui.set_theme(UiTheme {
            transition: UiTransition::new(0.2),
            ..UiTheme::default()
        });
        let ui = &mut ctx.ui;
        let hud = ui.root().layout(UiLayout::fill().padding(8.0).gap(4.0)).id();
        let score = ui.label(hud, "Score: 0").id();
        let rich = ui.label(hud, "[color=#ff0]Gold[/color] [img=coin] x3").rich().id();
        ui.image(hud, image)
            .layout(UiLayout::fixed(48.0, 48.0))
            .texture(image, 8.0);
        let progress = ui.progress_bar(hud).layout(UiLayout::fixed(160.0, 16.0)).id();
        ui.spinner(hud).layout(UiLayout::fixed(24.0, 24.0));
        ui.show_hud(hud);
        let menu = ui.root().layout(UiLayout::centered(420.0, 560.0).gap(4.0)).id();
        let button = ui.button(menu, "Play").layout(UiLayout::row(28.0)).id();
        ui.set_tooltip(button, "Starts a new game");
        let name = ui.text_input(menu).text("Ada").layout(UiLayout::row(28.0)).id();
        let toggle = ui.toggle(menu, "Music").layout(UiLayout::row(28.0)).id();
        let slider = ui.slider(menu).layout(UiLayout::row(20.0)).id();
        let dropdown = ui.dropdown(menu, ["Low", "High"]).layout(UiLayout::row(28.0)).id();
        let group = ui.radio_group(menu).id();
        ui.radio(group, "Off").layout(UiLayout::fixed(90.0, 24.0));
        ui.radio(group, "On").layout(UiLayout::fixed(90.0, 24.0));
        ui.number_field(menu, 5.0).layout(UiLayout::fixed(120.0, 28.0));
        let binding = ui.key_binding(menu, None).layout(UiLayout::fixed(160.0, 28.0)).id();
        let tabs = ui.tabs(menu).layout(UiLayout::row(60.0)).id();
        for title in ["One", "Two"] {
            let page = ui.tab(tabs, title).id();
            ui.label(page, "Page");
        }
        let list = ui.list(menu, 20.0).layout(UiLayout::fixed(160.0, 60.0)).id();
        ui.set_items(list, ["A", "B", "C", "D", "E"]);
        ui.split(menu, UiAxis::Horizontal).layout(UiLayout::row(24.0));
        let scroll = ui.scroll_area(menu).layout(UiLayout::fill().gap(2.0)).id();
        for i in 0..12 {
            ui.label(scroll, format!("Row {i}")).layout(UiLayout::row(20.0));
        }
        ui.push_modeless(menu);

        let still = UiInput::default();
        same_as_drawn_anew(&mut ctx, &still, "first frame");
        assert_eq!(
            same_as_drawn_anew(&mut ctx, &still, "still"),
            1,
            "only the spinner, whose dots go round"
        );
        let mut steps: Vec<Step> = vec![
            (
                "text",
                Box::new(move |c| {
                    c.ui.set_text(score, "Score: 10");
                    UiInput::default()
                }),
            ),
            (
                "markup",
                Box::new(move |c| {
                    c.ui.set_text(rich, "[b]Gold[/b] [u][img=coin][/u] x4");
                    UiInput::default()
                }),
            ),
            (
                "value",
                Box::new(move |c| {
                    c.ui.set_value(progress, 0.6);
                    c.ui.set_value(slider, 0.3);
                    UiInput::default()
                }),
            ),
            (
                "hover",
                Box::new(move |c| UiInput {
                    pointer: c.ui.rect(button).unwrap().center(),
                    dt: 0.05,
                    ..Default::default()
                }),
            ),
            (
                "hover transition",
                Box::new(move |c| UiInput {
                    pointer: c.ui.rect(button).unwrap().center(),
                    dt: 0.05,
                    ..Default::default()
                }),
            ),
            (
                "tooltip",
                Box::new(move |c| UiInput {
                    pointer: c.ui.rect(button).unwrap().center(),
                    dt: 2.0,
                    ..Default::default()
                }),
            ),
            (
                "press",
                Box::new(move |c| UiInput {
                    pointer: c.ui.rect(toggle).unwrap().center(),
                    pressed: true,
                    held: true,
                    ..Default::default()
                }),
            ),
            (
                "release",
                Box::new(move |c| UiInput {
                    pointer: c.ui.rect(toggle).unwrap().center(),
                    released: true,
                    ..Default::default()
                }),
            ),
            (
                "focus",
                Box::new(move |c| {
                    c.ui.set_focus(name);
                    UiInput::default()
                }),
            ),
            (
                "typing",
                Box::new(|_| UiInput {
                    text: " Lovelace".into(),
                    ..Default::default()
                }),
            ),
            (
                "caret",
                Box::new(|_| UiInput {
                    home: true,
                    ..Default::default()
                }),
            ),
            (
                "selection",
                Box::new(|_| UiInput {
                    select_all: true,
                    ..Default::default()
                }),
            ),
            (
                "composition",
                Box::new(|_| UiInput {
                    preedit: "かな".into(),
                    preedit_cursor: Some((0, 3)),
                    composing: true,
                    ..Default::default()
                }),
            ),
            ("composition ends", Box::new(|_| UiInput::default())),
            (
                "listening",
                Box::new(move |c| {
                    c.ui.set_focus(binding);
                    UiInput {
                        confirm: true,
                        ..Default::default()
                    }
                }),
            ),
            (
                "stops listening",
                Box::new(|_| UiInput {
                    cancel: true,
                    ..Default::default()
                }),
            ),
            (
                "dropdown",
                Box::new(move |c| {
                    c.ui.set_focus(dropdown);
                    UiInput {
                        confirm: true,
                        ..Default::default()
                    }
                }),
            ),
            (
                "dropdown closes",
                Box::new(|_| UiInput {
                    cancel: true,
                    ..Default::default()
                }),
            ),
            (
                "chosen",
                Box::new(move |c| {
                    c.ui.set_selected(group, Some(1));
                    c.ui.set_selected(tabs, Some(1));
                    c.ui.set_selected(list, Some(2));
                    UiInput::default()
                }),
            ),
            (
                "scroll",
                Box::new(move |c| {
                    c.ui.set_scroll(scroll, vec2(0.0, 30.0));
                    UiInput::default()
                }),
            ),
            (
                "paint",
                Box::new(move |c| {
                    let mut paint = c.ui.paint(button).unwrap();
                    paint.border_width = 3.0;
                    paint.font_size = 22.0;
                    c.ui.set_paint(button, paint);
                    UiInput::default()
                }),
            ),
            (
                "disabled",
                Box::new(move |c| {
                    c.ui.set_enabled(toggle, false);
                    UiInput::default()
                }),
            ),
            (
                "theme",
                Box::new(|c| {
                    c.ui.set_theme(UiTheme::dark());
                    UiInput::default()
                }),
            ),
            (
                "scale",
                Box::new(|c| {
                    c.ui.set_scale(1.25);
                    UiInput::default()
                }),
            ),
            (
                "window scale factor",
                Box::new(|c| {
                    c.set_scale_factor(2.0);
                    UiInput::default()
                }),
            ),
        ];
        // A modal screen sliding in, and off again.
        let dialog_steps = |steps: &mut Vec<Step>| {
            steps.push((
                "modal",
                Box::new(|c| {
                    let dialog = c.ui.root().layout(UiLayout::centered(200.0, 100.0)).id();
                    c.ui.button(dialog, "OK").layout(UiLayout::fill());
                    c.ui.set_screen_transition(
                        dialog,
                        UiScreenTransition::slide(vec2(0.0, 40.0), 0.2),
                        UiScreenTransition::fade(0.2),
                    );
                    c.ui.push_modal(dialog);
                    UiInput::default()
                }),
            ));
            for _ in 0..5 {
                steps.push((
                    "modal sliding in",
                    Box::new(|_| UiInput {
                        dt: 0.05,
                        ..Default::default()
                    }),
                ));
            }
            steps.push((
                "modal closes",
                Box::new(|c| {
                    c.ui.pop_screen();
                    UiInput::default()
                }),
            ));
            for _ in 0..5 {
                steps.push((
                    "modal fading out",
                    Box::new(|_| UiInput {
                        dt: 0.05,
                        ..Default::default()
                    }),
                ));
            }
        };
        dialog_steps(&mut steps);
        let mut redrawn = 0;
        for (what, step) in &steps {
            let input = step(&mut ctx);
            redrawn += same_as_drawn_anew(&mut ctx, &input, what);
            // A still frame after each: what changed is kept again.
            same_as_drawn_anew(&mut ctx, &UiInput::default(), what);
        }
        let nodes = ctx.ui.visible.len() as u64;
        assert!(
            redrawn < nodes * steps.len() as u64 / 2,
            "most nodes were kept: {redrawn} drawn of {}",
            nodes * steps.len() as u64
        );
    }
}
