//! The editor's particle section: every setting of the selected entity's
//! 2D or 3D emitter (`ParticleEmitter2D`, `ParticleEmitter3D`), the size
//! curve and colour gradient as graphs to drag, and a live preview: while
//! the game is paused in the editor, the selected emitter keeps simulating,
//! and a change carries on with the particles already alive.
//!
//! Each change is a merged step of the editor's history (one undo step per
//! drag) and is saved with the scene or prefab, as any component is.

use crate::reflect::WorldReflect;
use egui::{CollapsingHeader, Color32, DragValue, Pos2, RichText, Sense, Stroke, Ui, Vec2 as EVec2};
use ponko_ecs::Entity;
use serde_json::{Value, json};

use super::Panels;
use crate::ctx::Ctx;
use crate::particles2d::ParticleEmitter2D;

/// The reflected names of the two emitters.
pub(crate) const EMITTER_2D: &str = "ParticleEmitter2D";
pub(crate) const EMITTER_3D: &str = "ParticleEmitter3D";

/// Where the size curve's graph was drawn last (egui memory, for tests and
/// for dragging its keys).
pub(crate) fn curve_id() -> egui::Id {
    egui::Id::unique("ponko-particle-curve")
}

/// Where the colour gradient's bar was drawn last.
pub(crate) fn gradient_id() -> egui::Id {
    egui::Id::unique("ponko-particle-gradient")
}

/// Whether the preview runs (egui memory: it outlives the selection).
fn preview_id() -> egui::Id {
    egui::Id::unique("ponko-particle-preview")
}

/// The key being dragged in a graph, and the graph's scale when it started.
#[derive(Clone, Copy)]
struct Dragging {
    key: usize,
    top: f32,
}

/// Which emitter `e` has, if any.
fn emitter_of(ctx: &Ctx, e: Entity) -> Option<&'static str> {
    if ctx.world.get::<ParticleEmitter2D>(e).is_some() {
        return Some(EMITTER_2D);
    }
    #[cfg(feature = "3d")]
    if ctx.world.get::<crate::three::particles::ParticleEmitter3D>(e).is_some() {
        return Some(EMITTER_3D);
    }
    None
}

impl Panels {
    /// The particle section of entity `e`'s inspector (nothing if it has no
    /// emitter), and its live preview.
    pub(super) fn particles_section(&mut self, ui: &mut Ui, ctx: &mut Ctx, e: Entity) {
        let Some(name) = emitter_of(ctx, e) else {
            return;
        };
        let preview = ui.data_mut(|d| *d.get_persisted_mut_or(preview_id(), true));
        if preview && ctx.time.is_paused() {
            let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
            simulate(ctx, e, dt);
        }
        let Ok(before) = ctx.world.get_json(e, name) else {
            ui.label(
                RichText::new("Particles: made in code (an image or material without a file), so not editable here")
                    .weak(),
            );
            return;
        };
        let three = name == EMITTER_3D;
        let mut v = before.clone();
        let mut restart = false;
        CollapsingHeader::new("Particles")
            .id_salt("ponko-particle-editor")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut on = preview;
                    if ui
                        .checkbox(&mut on, "Live preview")
                        .on_hover_text("Simulate the selected emitter while the game is paused")
                        .changed()
                    {
                        ui.data_mut(|d| d.insert_persisted(preview_id(), on));
                    }
                    restart = ui.button("Restart").clicked();
                    ui.label(RichText::new(format!("{} alive", live(ctx, e))).weak());
                });
                settings(ui, &mut v, three);
            });
        if restart {
            restart_emitter(ctx, e);
        }
        if v != before {
            let mut old = take_particles(ctx, e);
            let set = ctx.edit(|ed, ctx| ed.set_merged(ctx, e, name, v));
            if self.report(set).is_some() {
                carry_on(ctx, e, &mut old);
            }
        }
    }
}

/// Every setting, as widgets editing the emitter's JSON.
fn settings(ui: &mut Ui, v: &mut Value, three: bool) {
    egui::Grid::new("ponko-particle-grid").num_columns(2).show(ui, |ui| {
        number(ui, "Rate", v, "rate", 1.0, 0.0);
        unsigned(ui, "Burst", v, "burst");
        number(ui, "Duration", v, "duration", 0.05, 0.001);
        flag(ui, "Looping", v, "looping");
        pair(ui, "Lifetime", v, "lifetime", 0.05, 0.001);
        pair(ui, "Speed", v, "speed", if three { 0.1 } else { 1.0 }, 0.0);
        pair(ui, "Size", v, "size", if three { 0.01 } else { 0.5 }, 0.0);
        pair(ui, "Rotation", v, "rotation", 0.05, f64::NEG_INFINITY);
        shape(ui, v, three);
        vector(ui, "Gravity", v, "gravity", if three { 3 } else { 2 });
        number(ui, "Drag", v, "drag", 0.05, 0.0);
        texture(ui, v);
        sheet(ui, v);
        let blends: &[&str] = if three {
            &["alpha", "additive"]
        } else {
            &["alpha", "additive", "multiply", "premultiplied"]
        };
        choice(ui, "Blend", v, "blend", blends);
        if three {
            number(ui, "Softness", v, "softness", 0.05, 0.0);
        } else {
            number(ui, "Z", v, "z", 0.1, f64::NEG_INFINITY);
        }
        unsigned(ui, "Max particles", v, "max_particles");
        choice(ui, "Space", v, "space", &["world", "local"]);
        unsigned(ui, "Seed", v, "seed");
    });
    ui.label("Size over life");
    curve(ui, v);
    ui.label("Colour over life");
    gradient(ui, v);
}

fn number(ui: &mut Ui, label: &str, v: &mut Value, key: &str, speed: f64, min: f64) {
    ui.label(label);
    let mut x = v[key].as_f64().unwrap_or(0.0);
    if ui
        .add(DragValue::new(&mut x).speed(speed).range(min..=f64::INFINITY))
        .changed()
    {
        v[key] = json!(x as f32);
    }
    ui.end_row();
}

fn unsigned(ui: &mut Ui, label: &str, v: &mut Value, key: &str) {
    ui.label(label);
    let mut x = v[key].as_u64().unwrap_or(0);
    if ui.add(DragValue::new(&mut x).speed(1.0)).changed() {
        v[key] = json!(x);
    }
    ui.end_row();
}

fn flag(ui: &mut Ui, label: &str, v: &mut Value, key: &str) {
    ui.label(label);
    let mut on = v[key].as_bool().unwrap_or(false);
    if ui.checkbox(&mut on, "").changed() {
        v[key] = json!(on);
    }
    ui.end_row();
}

/// A `[min, max]` range: the minimum never passes the maximum.
fn pair(ui: &mut Ui, label: &str, v: &mut Value, key: &str, speed: f64, min: f64) {
    ui.label(label);
    let mut lo = v[key][0].as_f64().unwrap_or(0.0);
    let mut hi = v[key][1].as_f64().unwrap_or(0.0);
    ui.horizontal(|ui| {
        let a = ui.add(DragValue::new(&mut lo).speed(speed).range(min..=hi)).changed();
        ui.label("to");
        let b = ui
            .add(DragValue::new(&mut hi).speed(speed).range(lo..=f64::INFINITY))
            .changed();
        if a || b {
            v[key] = json!([lo as f32, hi as f32]);
        }
    });
    ui.end_row();
}

fn vector(ui: &mut Ui, label: &str, v: &mut Value, key: &str, n: usize) {
    ui.label(label);
    ui.horizontal(|ui| {
        for i in 0..n {
            let mut x = v[key][i].as_f64().unwrap_or(0.0);
            if ui.add(DragValue::new(&mut x).speed(0.1)).changed() {
                v[key][i] = json!(x as f32);
            }
        }
    });
    ui.end_row();
}

fn choice(ui: &mut Ui, label: &str, v: &mut Value, key: &str, options: &[&str]) {
    ui.label(label);
    let now = v[key].as_str().unwrap_or(options[0]).to_string();
    egui::ComboBox::from_id_salt(("ponko-particle", key))
        .selected_text(&now)
        .show_ui(ui, |ui| {
            for o in options {
                if ui.selectable_label(now == *o, *o).clicked() {
                    v[key] = json!(o);
                }
            }
        });
    ui.end_row();
}

/// The shape, and its size: `"point"` or `{ "cone": { "angle": 0.4 } }`.
fn shape(ui: &mut Ui, v: &mut Value, three: bool) {
    let kinds: &[(&str, &str)] = if three {
        &[
            ("point", ""),
            ("sphere", "radius"),
            ("cone", "angle"),
            ("box", "half_extents"),
        ]
    } else {
        &[
            ("point", ""),
            ("circle", "radius"),
            ("cone", "angle"),
            ("rect", "half_extents"),
        ]
    };
    let now = match &v["shape"] {
        Value::String(s) => s.clone(),
        Value::Object(m) => m.keys().next().cloned().unwrap_or_default(),
        _ => "point".into(),
    };
    ui.label("Shape");
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("ponko-particle-shape")
            .selected_text(&now)
            .show_ui(ui, |ui| {
                for (kind, field) in kinds {
                    if ui.selectable_label(now == *kind, *kind).clicked() && now != *kind {
                        v["shape"] = match *field {
                            "" => json!(kind),
                            "half_extents" if three => json!({ *kind: { "half_extents": [1.0, 1.0, 1.0] } }),
                            "half_extents" => json!({ *kind: { "half_extents": [16.0, 16.0] } }),
                            "angle" => json!({ *kind: { "angle": 0.4 } }),
                            _ => json!({ *kind: { "radius": if three { 1.0 } else { 16.0 } } }),
                        };
                    }
                }
            });
        let Some((kind, field)) = v["shape"]
            .as_object()
            .and_then(|m| m.iter().next())
            .and_then(|(k, inner)| Some((k.clone(), inner.as_object()?.keys().next()?.clone())))
        else {
            return;
        };
        let slot = &mut v["shape"][&kind][&field];
        match slot {
            Value::Array(items) => {
                for item in items.iter_mut() {
                    let mut x = item.as_f64().unwrap_or(0.0);
                    if ui
                        .add(DragValue::new(&mut x).speed(0.1).range(0.0..=f64::INFINITY))
                        .changed()
                    {
                        *item = json!(x as f32);
                    }
                }
            }
            _ => {
                let mut x = slot.as_f64().unwrap_or(0.0);
                ui.label(&field);
                if ui
                    .add(DragValue::new(&mut x).speed(0.02).range(0.0..=f64::INFINITY))
                    .changed()
                {
                    *slot = json!(x as f32);
                }
            }
        }
    });
    ui.end_row();
}

fn texture(ui: &mut Ui, v: &mut Value) {
    ui.label("Image");
    let mut path = v["texture"].as_str().unwrap_or("").to_string();
    let field = ui
        .add(egui::TextEdit::singleline(&mut path).hint_text("none (white)"))
        .on_hover_text("An image in assets/; empty for plain white squares");
    if field.lost_focus() && path != v["texture"].as_str().unwrap_or("") {
        v["texture"] = if path.trim().is_empty() {
            Value::Null
        } else {
            json!(path.trim())
        };
    }
    ui.end_row();
}

fn sheet(ui: &mut Ui, v: &mut Value) {
    ui.label("Sheet");
    ui.horizontal(|ui| {
        for i in 0..2 {
            let mut n = v["sheet"][i].as_u64().unwrap_or(1);
            if ui.add(DragValue::new(&mut n).range(1..=256)).changed() {
                v["sheet"][i] = json!(n);
            }
        }
        ui.label("columns × rows");
    });
    ui.end_row();
}

/// Keys `(t, value)` of a JSON list, or `fallback` if it's empty.
fn keys(list: &Value, fallback: &[(f32, Value)]) -> Vec<(f32, Value)> {
    let found: Vec<(f32, Value)> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|k| Some((k[0].as_f64()? as f32, k[1].clone())))
        .collect();
    if found.is_empty() { fallback.to_vec() } else { found }
}

/// Where `t` may go for key `i`: strictly between its neighbours.
fn between(times: &[f32], i: usize, t: f32) -> f32 {
    const GAP: f32 = 0.001;
    let lo = if i == 0 { 0.0 } else { times[i - 1] + GAP };
    let hi = if i + 1 == times.len() { 1.0 } else { times[i + 1] - GAP };
    t.clamp(lo, hi.max(lo))
}

/// The size curve: drag a key, click to add one, double-click one to
/// remove it.
fn curve(ui: &mut Ui, v: &mut Value) {
    let mut ks: Vec<(f32, f32)> = keys(&v["size_over_life"], &[(0.0, json!(1.0))])
        .into_iter()
        .map(|(t, x)| (t, x.as_f64().unwrap_or(1.0) as f32))
        .collect();
    let width = ui.available_width().clamp(160.0, 260.0);
    let (rect, response) = ui.allocate_exact_size(EVec2::new(width, 80.0), Sense::click_and_drag());
    ui.data_mut(|d| d.insert_temp(curve_id(), rect));
    let dragging: Option<Dragging> = ui.data(|d| d.get_temp(curve_id().with("drag")));
    let top = dragging.map_or_else(|| ks.iter().map(|k| k.1).fold(1.0, f32::max) * 1.25, |d| d.top);
    let at = |(t, x): (f32, f32)| Pos2::new(rect.left() + t * rect.width(), rect.bottom() - x / top * rect.height());
    let from = |p: Pos2| {
        (
            ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - p.y) / rect.height() * top).max(0.0),
        )
    };
    let nearest = |ks: &[(f32, f32)], p: Pos2| {
        ks.iter()
            .enumerate()
            .map(|(i, k)| (i, at(*k).distance(p)))
            .filter(|(_, d)| *d < 8.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let mut changed = false;
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
        && let Some(key) = nearest(&ks, p)
    {
        ui.data_mut(|d| d.insert_temp(curve_id().with("drag"), Dragging { key, top }));
    }
    if let Some(d) = ui.data(|d| d.get_temp::<Dragging>(curve_id().with("drag")))
        && let Some(p) = response.interact_pointer_pos()
        && d.key < ks.len()
    {
        let (t, x) = from(p);
        let times: Vec<f32> = ks.iter().map(|k| k.0).collect();
        let k = (between(&times, d.key, t), x);
        if k != ks[d.key] {
            ks[d.key] = k;
            changed = true;
        }
    }
    if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
        ui.data_mut(|d| d.remove::<Dragging>(curve_id().with("drag")));
    }
    if response.double_clicked() {
        if let Some(p) = response.interact_pointer_pos()
            && let Some(i) = nearest(&ks, p)
            && ks.len() > 1
        {
            ks.remove(i);
            changed = true;
        }
    } else if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
        && nearest(&ks, p).is_none()
    {
        let (t, x) = from(p);
        let i = ks.partition_point(|k| k.0 < t);
        if ks.get(i).is_none_or(|k| k.0 != t) {
            ks.insert(i, (t, x));
            changed = true;
        }
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let line: Vec<Pos2> = std::iter::once((0.0, ks[0].1))
        .chain(ks.iter().copied())
        .chain(std::iter::once((1.0, ks[ks.len() - 1].1)))
        .map(at)
        .collect();
    painter.add(egui::Shape::line(line, Stroke::new(1.5, ui.visuals().text_color())));
    for k in &ks {
        painter.circle_filled(at(*k), 4.0, Color32::from_rgb(240, 180, 60));
    }
    response.on_hover_text("Drag a key; click to add one; double-click one to remove it");
    if changed {
        v["size_over_life"] = json!(ks.iter().map(|k| [k.0, k.1]).collect::<Vec<_>>());
    }
}

fn color32(c: &Value) -> Color32 {
    let f = |k: &str| (c[k].as_f64().unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(f("r"), f("g"), f("b"), f("a"))
}

/// The colour gradient: a bar with a marker per key (drag it along, click
/// the bar to add one with the colour there, double-click one to remove
/// it), and below it each key's time and colour.
fn gradient(ui: &mut Ui, v: &mut Value) {
    let white = json!({"r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0});
    let clear = json!({"r": 1.0, "g": 1.0, "b": 1.0, "a": 0.0});
    let mut ks = keys(&v["color_over_life"], &[(0.0, white), (1.0, clear)]);
    let width = ui.available_width().clamp(160.0, 260.0);
    let (rect, response) = ui.allocate_exact_size(EVec2::new(width, 22.0), Sense::click_and_drag());
    ui.data_mut(|d| d.insert_temp(gradient_id(), rect));
    let x_of = |t: f32| rect.left() + t * rect.width();
    let t_of = |p: Pos2| ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    let nearest = |ks: &[(f32, Value)], p: Pos2| {
        ks.iter()
            .enumerate()
            .map(|(i, k)| (i, (x_of(k.0) - p.x).abs()))
            .filter(|(_, d)| *d < 6.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let drag_id = gradient_id().with("drag");
    let mut changed = false;
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
        && let Some(key) = nearest(&ks, p)
    {
        ui.data_mut(|d| d.insert_temp(drag_id, Dragging { key, top: 0.0 }));
    }
    if let Some(d) = ui.data(|d| d.get_temp::<Dragging>(drag_id))
        && let Some(p) = response.interact_pointer_pos()
        && d.key < ks.len()
    {
        let times: Vec<f32> = ks.iter().map(|k| k.0).collect();
        let t = between(&times, d.key, t_of(p));
        if t != ks[d.key].0 {
            ks[d.key].0 = t;
            changed = true;
        }
    }
    if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
        ui.data_mut(|d| d.remove::<Dragging>(drag_id));
    }
    if response.double_clicked() {
        if let Some(p) = response.interact_pointer_pos()
            && let Some(i) = nearest(&ks, p)
            && ks.len() > 1
        {
            ks.remove(i);
            changed = true;
        }
    } else if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
        && nearest(&ks, p).is_none()
    {
        let t = t_of(p);
        let i = ks.partition_point(|k| k.0 < t);
        if ks.get(i).is_none_or(|k| k.0 != t) {
            let c = sample(&ks, t);
            ks.insert(i, (t, c));
            changed = true;
        }
    }
    let painter = ui.painter_at(rect);
    let mut mesh = egui::Mesh::default();
    let stops: Vec<(f32, Color32)> = std::iter::once((0.0, color32(&ks[0].1)))
        .chain(ks.iter().map(|k| (k.0, color32(&k.1))))
        .chain(std::iter::once((1.0, color32(&ks[ks.len() - 1].1))))
        .collect();
    for (t, c) in &stops {
        let x = x_of(*t);
        mesh.colored_vertex(Pos2::new(x, rect.top()), *c);
        mesh.colored_vertex(Pos2::new(x, rect.bottom()), *c);
    }
    for i in 0..stops.len() as u32 - 1 {
        let a = i * 2;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    painter.rect_filled(rect, 0.0, Color32::from_gray(90));
    painter.add(mesh);
    for k in &ks {
        let x = x_of(k.0);
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(2.0, Color32::BLACK),
        );
    }
    response.on_hover_text("Drag a key; click to add one; double-click one to remove it");
    let mut remove = None;
    for (i, (t, c)) in ks.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut x = *t as f64;
            if ui.add(DragValue::new(&mut x).speed(0.01).range(0.0..=1.0)).changed() {
                *t = x as f32;
                changed = true;
            }
            let mut rgba = color32(c).to_srgba_unmultiplied().map(|b| b as f32 / 255.0);
            if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                *c = json!({"r": rgba[0], "g": rgba[1], "b": rgba[2], "a": rgba[3]});
                changed = true;
            }
            if ui.small_button("×").on_hover_text("Remove this key").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove
        && ks.len() > 1
    {
        ks.remove(i);
        changed = true;
    }
    if changed {
        // Times typed out of order are put back in order, one apart.
        ks.sort_by(|a, b| a.0.total_cmp(&b.0));
        for i in 1..ks.len() {
            if ks[i].0 <= ks[i - 1].0 {
                ks[i].0 = (ks[i - 1].0 + 0.001).min(1.0);
            }
        }
        ks.dedup_by(|b, a| b.0 <= a.0);
        v["color_over_life"] = json!(ks.iter().map(|k| json!([k.0, k.1])).collect::<Vec<_>>());
    }
}

/// The gradient's colour at `t`, as a JSON colour.
fn sample(ks: &[(f32, Value)], t: f32) -> Value {
    let i = ks.partition_point(|k| k.0 <= t);
    let (a, b, f) = match i {
        0 => (&ks[0].1, &ks[0].1, 0.0),
        i if i == ks.len() => (&ks[i - 1].1, &ks[i - 1].1, 0.0),
        i => (&ks[i - 1].1, &ks[i].1, (t - ks[i - 1].0) / (ks[i].0 - ks[i - 1].0)),
    };
    let mix = |k: &str| {
        let (x, y) = (a[k].as_f64().unwrap_or(1.0) as f32, b[k].as_f64().unwrap_or(1.0) as f32);
        x + (y - x) * f
    };
    json!({"r": mix("r"), "g": mix("g"), "b": mix("b"), "a": mix("a")})
}

/// How many particles `e`'s emitter has alive.
fn live(ctx: &Ctx, e: Entity) -> usize {
    if let Some(p) = ctx.world.get::<ParticleEmitter2D>(e) {
        return p.live_count();
    }
    #[cfg(feature = "3d")]
    if let Some(p) = ctx.world.get::<crate::three::particles::ParticleEmitter3D>(e) {
        return p.live_count();
    }
    0
}

/// One preview step of `dt` seconds for `e`'s emitter.
fn simulate(ctx: &mut Ctx, e: Entity, dt: f32) {
    if let Some(t) = ctx.world.get::<crate::d2::Transform>(e).copied()
        && let Some(p) = ctx.world.get_mut::<ParticleEmitter2D>(e)
    {
        p.update(dt, &t);
    }
    #[cfg(feature = "3d")]
    if let Some(t) = ctx.world.get::<crate::d3::Transform>(e).copied()
        && let Some(p) = ctx.world.get_mut::<crate::three::particles::ParticleEmitter3D>(e)
    {
        p.update(dt, &t);
    }
}

/// Starts `e`'s emitter over: no particles, its first burst again.
fn restart_emitter(ctx: &mut Ctx, e: Entity) {
    if let Some(p) = ctx.world.get_mut::<ParticleEmitter2D>(e) {
        *p = ParticleEmitter2D::new(p.settings().clone());
    }
    #[cfg(feature = "3d")]
    if let Some(p) = ctx.world.get_mut::<crate::three::particles::ParticleEmitter3D>(e) {
        *p = crate::three::particles::ParticleEmitter3D::new(p.settings().clone());
    }
}

/// The emitter before a change, to carry its particles over.
// One short-lived value on the stack: boxing the emitter would only add an allocation.
#[allow(clippy::large_enum_variant)]
enum Old {
    None,
    D2(ParticleEmitter2D),
    #[cfg(feature = "3d")]
    D3(crate::three::particles::ParticleEmitter3D),
}

fn take_particles(ctx: &Ctx, e: Entity) -> Old {
    if let Some(p) = ctx.world.get::<ParticleEmitter2D>(e) {
        return Old::D2(p.clone());
    }
    #[cfg(feature = "3d")]
    if let Some(p) = ctx.world.get::<crate::three::particles::ParticleEmitter3D>(e) {
        return Old::D3(p.clone());
    }
    Old::None
}

fn carry_on(ctx: &mut Ctx, e: Entity, old: &mut Old) {
    match old {
        Old::D2(old) => {
            if let Some(p) = ctx.world.get_mut::<ParticleEmitter2D>(e) {
                p.carry_on_from(old);
            }
        }
        #[cfg(feature = "3d")]
        Old::D3(old) => {
            if let Some(p) = ctx.world.get_mut::<crate::three::particles::ParticleEmitter3D>(e) {
                p.carry_on_from(old);
            }
        }
        Old::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::author_harness::*;
    use super::*;
    use crate::d2::Transform as T2;
    use egui::Rect;

    fn emitter_editor() -> (Harness, Entity) {
        let mut h = Harness::new();
        let middle = h.ctx.camera.center();
        let e = h.ctx.world.spawn((T2::at(middle), ParticleEmitter2D::default()));
        h.select(e);
        h.frame(vec![]);
        (h, e)
    }

    fn json_of(h: &Harness, e: Entity) -> Value {
        h.ctx.world.get_json(e, EMITTER_2D).unwrap()
    }

    #[test]
    fn dragging_the_rate_field_is_one_undo_step_and_saved_with_the_scene() {
        let (mut h, e) = emitter_editor();
        let shapes = h.frame(vec![]);
        let field = field_right_of(&shapes, "Rate");
        h.drag(field, field + glam::vec2(40.0, 0.0), 4);
        let rate = json_of(&h, e)["rate"].as_f64().unwrap();
        assert!(rate > 10.0, "the rate went up from 10: {rate}");
        assert!(h.ctx.editor.dirty());
        let saved = h.ctx.world.save();
        assert!(saved.to_string().contains("ParticleEmitter2D"));
        h.undo();
        assert_eq!(
            json_of(&h, e)["rate"].as_f64(),
            Some(10.0),
            "one step takes the drag back"
        );
        assert!(!h.ctx.editor.can_undo());
    }

    #[test]
    fn clicking_the_size_curve_adds_a_key_and_dragging_moves_it() {
        let (mut h, e) = emitter_editor();
        let rect: Rect = h.rect(curve_id());
        // Click the middle of the graph, high up: a key at t = 0.5.
        let at = glam::vec2(rect.center().x, rect.top() + 10.0);
        h.click(at);
        let keys = json_of(&h, e)["size_over_life"].clone();
        assert_eq!(keys.as_array().unwrap().len(), 2, "{keys}");
        assert!((keys[1][0].as_f64().unwrap() - 0.5).abs() < 0.02, "{keys}");
        let high = keys[1][1].as_f64().unwrap();
        assert!(high > 1.0, "{keys}");
        // Drag it down to the bottom: one more undo step.
        h.drag(at, glam::vec2(at.x, rect.bottom() - 1.0), 4);
        let keys = json_of(&h, e)["size_over_life"].clone();
        assert!(keys[1][1].as_f64().unwrap() < 0.1, "{keys}");
        h.undo();
        assert_eq!(json_of(&h, e)["size_over_life"][1][1].as_f64(), Some(high));
        h.undo();
        assert_eq!(json_of(&h, e)["size_over_life"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn dragging_a_gradient_key_moves_its_time_and_double_click_removes_it() {
        let (mut h, e) = emitter_editor();
        let rect: Rect = h.rect(gradient_id());
        h.click(glam::vec2(rect.center().x, rect.center().y));
        let keys = json_of(&h, e)["color_over_life"].clone();
        assert_eq!(keys.as_array().unwrap().len(), 3, "a key added in the middle: {keys}");
        // The new key's colour is the gradient's there: half transparent.
        assert!((keys[1][1]["a"].as_f64().unwrap() - 0.5).abs() < 0.05, "{keys}");
        let from = glam::vec2(rect.center().x, rect.center().y);
        h.drag(from, from + glam::vec2(rect.width() * 0.25, 0.0), 4);
        let t = json_of(&h, e)["color_over_life"][1][0].as_f64().unwrap();
        assert!((t - 0.75).abs() < 0.03, "{t}");
        let at = glam::vec2(rect.left() + rect.width() * t as f32, rect.center().y);
        h.double_click(at);
        assert_eq!(json_of(&h, e)["color_over_life"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn the_preview_simulates_while_paused_and_an_edit_keeps_the_particles() {
        let (mut h, e) = emitter_editor();
        assert!(h.ctx.time.is_paused(), "the editor starts paused");
        for _ in 0..30 {
            h.frame(vec![]);
        }
        let alive = h.ctx.world.get::<ParticleEmitter2D>(e).unwrap().live_count();
        assert!(alive > 0, "half a second at 10 a second makes particles");
        // An edit makes a new emitter: the particles alive go on.
        h.ctx
            .edit(|ed, ctx| {
                let mut v = ctx.world.get_json(e, EMITTER_2D).unwrap();
                v["drag"] = json!(0.5);
                let mut old = take_particles(ctx, e);
                ed.set_merged(ctx, e, EMITTER_2D, v)?;
                carry_on(ctx, e, &mut old);
                Ok::<_, crate::error::Error>(())
            })
            .unwrap();
        assert_eq!(h.ctx.world.get::<ParticleEmitter2D>(e).unwrap().live_count(), alive);
        // With the preview off nothing moves.
        let shapes = h.frame(vec![]);
        let toggle = text_at(&shapes, |t| t == "Live preview").unwrap();
        h.click(toggle);
        h.click_text(|t| t == "Restart");
        for _ in 0..30 {
            h.frame(vec![]);
        }
        assert_eq!(h.ctx.world.get::<ParticleEmitter2D>(e).unwrap().live_count(), 0);
    }

    #[test]
    fn changing_the_shape_from_its_menu_writes_the_new_shape() {
        let (mut h, e) = emitter_editor();
        let shapes = h.frame(vec![]);
        let menu = text_at(&shapes, |t| t == "point").expect("the shape menu shows point");
        h.click(menu);
        let shapes = h.frame(vec![]);
        let cone = text_at(&shapes, |t| t == "cone").expect("the menu lists cone");
        h.click(cone);
        let angle = json_of(&h, e)["shape"]["cone"]["angle"].as_f64();
        assert!(angle.is_some_and(|a| (a - 0.4).abs() < 1e-6), "{angle:?}");
    }

    #[cfg(feature = "3d")]
    #[test]
    fn a_3d_emitter_shows_its_own_settings_and_saves_as_particle_emitter_3d() {
        let mut h = Harness::new();
        let e = h.ctx.world.spawn((
            crate::d3::Transform::default(),
            crate::three::particles::ParticleEmitter3D::default(),
        ));
        h.select(e);
        let shapes = h.frame(vec![]);
        assert!(text_at(&shapes, |t| t == "Softness").is_some());
        assert!(text_at(&shapes, |t| t == "Z").is_none());
        let field = field_right_of(&shapes, "Softness");
        h.drag(field, field + glam::vec2(30.0, 0.0), 3);
        let soft = h.ctx.world.get_json(e, EMITTER_3D).unwrap()["softness"]
            .as_f64()
            .unwrap();
        assert!(soft > 0.0, "{soft}");
        h.undo();
        assert_eq!(
            h.ctx.world.get_json(e, EMITTER_3D).unwrap()["softness"].as_f64(),
            Some(0.0)
        );
    }
}
