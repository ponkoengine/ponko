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

    /// Draws `ui`, laid out in `viewport`, into `ui_list`.
    pub(crate) fn draw_ui_tree(&mut self, ui: &mut crate::ui::Ui, viewport: Vec2) {
        // Mutations made by game systems (including screen pushes) show now.
        ui.pixels = self.gfx.scale_factor;
        ui.layout_with_text(viewport, &mut self.gfx.text);
        let scale = ui.scale();
        for (order, &id) in ui.visible.iter().enumerate() {
            let n = ui.node(id).unwrap();
            let z = 100.0 + order as f32 * 4.0;
            // Where its animation draws it: everything below is drawn in
            // its place, then moved, scaled and faded together.
            let xf = n.motion.xf;
            if xf.opacity <= 0.0 {
                continue;
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
            let layout = ui.tree.layout(n.layout).unwrap();
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
            let mut origin = text_rect.min;
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
            // A control's text sits in the middle of its height.
            if framed && n.rich().is_none() {
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
            if let Some(rich) = n.rich() {
                self.retained_rich(rich, &n.paint, text_rect, scale, params.z);
            } else {
                self.retained_text(text, origin, text_rect.size().x, params, flags, n.paint.align, true);
            }
            self.ui_state.shown_scratch = shown;
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
            if !xf.is_identity() {
                self.gfx
                    .ui_list
                    .transform_from(sprites, xf.scale, xf.shift.into(), xf.opacity);
            }
        }
        self.gfx.ui_list.set_clip(None);
        let top = 100.0 + ui.visible.len() as f32 * 4.0 + 8.0;
        self.draw_widget_overlays(ui, top);
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
    /// each run in its font, size and color, its images, underlines.
    fn retained_rich(
        &mut self,
        rich: &super::rich::RichText,
        paint: &crate::ui::UiPaint,
        rect: Rect,
        scale: f32,
        z: f32,
    ) {
        use super::rich::{Align, Base, bold_offset};
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
