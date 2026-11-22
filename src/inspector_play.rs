//! The editor app's play bar: Play, Pause, Step and Stop, and the Stop
//! dialog that asks which edits made while playing to keep (see
//! `editor_play.rs`).

use egui::{Button, Color32, DragValue, RichText, Ui};

use super::Panels;
use super::app::Dialog;
use super::keys::Action;
use super::scene::unloaded_line;
use crate::ctx::Ctx;
use crate::editor::play::PlayEdit;

impl Panels {
    /// Play / Pause, Step, Stop, the speed and the tick.
    pub(super) fn play_bar(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let playing = ctx.editor.playing();
        let paused = ctx.time.is_paused();
        if paused {
            let (text, tip) = if playing {
                ("Resume", "Run on from here")
            } else {
                ("Play", "Run the game; Stop puts the world back as it is now")
            };
            if ui
                .button(self.labelled(text, Action::Play))
                .on_hover_text(tip)
                .clicked()
            {
                self.play(ctx);
            }
        } else if ui.button(self.labelled("Pause", Action::Pause)).clicked() {
            ctx.time.pause();
        }
        if ui
            .button(self.labelled("Step", Action::Step))
            .on_hover_text("Run one tick, then pause")
            .clicked()
        {
            self.step(ctx);
        }
        if ui
            .add_enabled(playing, Button::new(self.labelled("Stop", Action::Stop)))
            .on_hover_text("Put the world back as it was at Play (asks about edits made while playing)")
            .clicked()
        {
            self.ask_stop(ctx);
        }
        let mut speed = ctx.time.speed();
        let drag = DragValue::new(&mut speed).range(0.0..=16.0).speed(0.02).suffix("×");
        if ui.add(drag).on_hover_text("Simulation speed").changed() {
            ctx.time.set_speed(speed);
        }
        let state = match (playing, paused) {
            (false, _) => RichText::new("editing").weak(),
            (true, false) => RichText::new("playing").color(Color32::from_rgb(120, 200, 255)),
            (true, true) => RichText::new("paused").color(Color32::from_rgb(255, 190, 60)),
        };
        ui.label(state);
        ui.label(RichText::new(format!("tick {}", ctx.time.sim_tick())).weak());
    }

    /// Play, Pause, Step and Stop keys; they work while the game runs too.
    pub(super) fn play_keys(&mut self, ui: &Ui, ctx: &mut Ctx) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (play, pause, step, stop) = ui.input(|i| {
            (
                self.keys.pressed(i, Action::Play),
                self.keys.pressed(i, Action::Pause),
                self.keys.pressed(i, Action::Step),
                self.keys.pressed(i, Action::Stop),
            )
        });
        if play && ctx.time.is_paused() {
            self.play(ctx);
        } else if pause || (play && ctx.editor.playing()) {
            ctx.time.pause();
        } else if step {
            self.step(ctx);
        } else if stop && ctx.editor.playing() {
            self.ask_stop(ctx);
        }
    }

    fn play(&mut self, ctx: &mut Ctx) {
        self.end_gizmo_drag2(ctx);
        self.end_gizmo_drag(ctx);
        ctx.edit(|ed, ctx| ed.play(ctx));
    }

    /// One tick, paused. The first step is Play's too: Stop goes back to
    /// before it.
    fn step(&mut self, ctx: &mut Ctx) {
        if !ctx.editor.playing() {
            self.play(ctx);
        }
        ctx.time.pause();
        ctx.time.step();
    }

    /// Stop: straight away if nothing was edited while playing, else the
    /// dialog that asks which edits to keep (all ticked).
    pub(crate) fn ask_stop(&mut self, ctx: &mut Ctx) {
        let edits = ctx.editor.play_edits(ctx);
        if edits.is_empty() {
            self.stop(ctx, &[]);
            return;
        }
        ctx.time.pause();
        self.set_dialog(Some(Dialog::Stop {
            keep: edits.into_iter().map(|e| (e, true)).collect(),
        }));
    }

    pub(crate) fn stop(&mut self, ctx: &mut Ctx, keep: &[PlayEdit]) {
        let before = unloaded_line(&ctx.editor.doc.unloaded);
        let stopped = ctx.edit(|ed, ctx| ed.stop_keeping(ctx, keep));
        if self.report(stopped).is_some() {
            // The glTF scenes that didn't spawn again are in the panel of
            // scenes that didn't load; the error line names them too.
            match unloaded_line(&ctx.editor.doc.unloaded) {
                Some(line) => self.error = Some(line),
                None if self.error == before => self.error = None,
                None => {}
            }
            self.scene.forget_drag();
            // A selected glTF node was spawned again as another entity.
            self.selected = self.selected.map(|e| ctx.editor.resolve(e));
            for e in &mut self.others {
                *e = ctx.editor.resolve(*e);
            }
            self.status = Some(match keep.len() {
                0 => "stopped: the world is back as it was at Play".into(),
                n => format!("stopped, keeping {n} edit(s) made while playing"),
            });
        }
    }

    /// The Stop dialog's body: each edit made while playing, ticked to keep.
    pub(super) fn stop_dialog(
        &mut self,
        ui: &mut Ui,
        ctx: &mut Ctx,
        keep: &mut [(PlayEdit, bool)],
        enter: bool,
    ) -> bool {
        ui.label("Stop puts the world back as it was at Play. Keep these edits made while playing?");
        egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
            for (edit, on) in keep.iter_mut() {
                let text = match edit {
                    PlayEdit::Component(e, name) => format!("{}: {name}", super::scene::label(ctx, *e)),
                    PlayEdit::Resource(name) => format!("resource {name}"),
                };
                ui.checkbox(on, text);
            }
        });
        ui.label(RichText::new("A kept value is the one it has now (your edit, and what the game did since).").weak());
        let mut done = false;
        ui.horizontal(|ui| {
            if ui.button("Stop and keep ticked").clicked() || enter {
                let chosen: Vec<PlayEdit> = keep.iter().filter(|(_, on)| *on).map(|(e, _)| e.clone()).collect();
                self.stop(ctx, &chosen);
                done = true;
            }
            if ui.button("Stop, keep none").clicked() {
                self.stop(ctx, &[]);
                done = true;
            }
            if ui.button("Cancel").clicked() {
                done = true;
            }
        });
        done
    }
}
