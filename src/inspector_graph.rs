//! The animation graph window: a 2D skeleton file's state machine (its
//! `graph`: parameters, states playing a clip or a 1D blend tree,
//! transitions with conditions, exit times and fades) drawn as nodes and
//! arrows and edited there, with a preview that runs the graph on the
//! skeleton with test values for its parameters.
//!
//! In the canvas a click selects a state or a transition, a left drag
//! moves a state, and a right drag from one state onto another adds a
//! transition between them ("Any state" for a transition from anywhere).
//! The selection is edited below the canvas. Every edit is a step in the
//! editor's history; Save writes the skeleton file back in its own format
//! (`"format": "ponko-skeleton2d"`), which the runtime reads with
//! `load_skeleton2d`. A graph the runtime would refuse (a transition to a
//! state that isn't there, a blend tree on a bool) shows why under the
//! canvas, and the preview keeps the last graph that loaded.
//!
//! Where the states sit in the canvas is the editor's own: the file format
//! has no place for it.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2 as EVec2};
use serde_json::{Value, json};

use super::Panels;
use super::author::{read_text, write_json};
use crate::ctx::Ctx;
use crate::skeleton2d::{Skeleton, SkeletonData};

/// The node standing for "from any state".
const ANY: &str = "Any state";
/// A node's size in the canvas.
const NODE: EVec2 = EVec2::new(110.0, 30.0);

/// What is selected in the canvas.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Selected {
    State(String),
    Transition(usize),
}

/// A parameter's test value for the preview.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Test {
    Float(f32),
    Bool(bool),
}

/// The graph window's state.
#[derive(Default)]
pub(crate) struct GraphState {
    pub path: Option<String>,
    pub selected: Option<Selected>,
    /// Where each state's node is in the canvas, from its top-left corner.
    pos: HashMap<String, EVec2>,
    /// Test values by parameter name, and triggers fired for the next step.
    test: HashMap<String, Test>,
    fire: Vec<String>,
    /// The preview, made from the graph as of `applied`.
    pub preview: Option<Skeleton>,
    applied: u64,
    /// Why the graph as it is now doesn't load, if it doesn't.
    pub error: Option<String>,
    /// A right drag from this node, adding a transition.
    linking: Option<String>,
    /// A state's new name as typed so far.
    naming: Option<(String, String)>,
}

/// Whether the file at `path` (in `assets/`) is a skeleton in Ponko's format.
pub(crate) fn is_skeleton_file(path: &str) -> bool {
    crate::assets::resolve(path)
        .ok()
        .and_then(|p| crate::assets::read_to_string(&p).ok())
        .is_some_and(|t| t.contains("\"ponko-skeleton2d\""))
}

/// Where the canvas was drawn, and each node (egui memory, for tests).
pub(crate) fn canvas_id() -> egui::Id {
    egui::Id::unique("ponko-graph-canvas")
}

pub(crate) fn node_id(name: &str) -> egui::Id {
    egui::Id::unique(("ponko-graph-node", name))
}

fn names(list: &Value) -> Vec<String> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["name"].as_str().map(str::to_string))
        .collect()
}

/// A parameter's kind: `"float"`, `"bool"` or `"trigger"`.
fn kind_of(p: &Value) -> &'static str {
    if p.get("bool").is_some() {
        "bool"
    } else if p.get("trigger").is_some() {
        "trigger"
    } else {
        "float"
    }
}

/// A name not taken in `taken`, from `base`.
fn fresh(base: &str, taken: &[String]) -> String {
    (1..)
        .map(|i| {
            if i == 1 {
                base.to_string()
            } else {
                format!("{base} {i}")
            }
        })
        .find(|n| !taken.contains(n))
        .unwrap_or_default()
}

impl Panels {
    fn graph(&mut self) -> Option<&mut GraphState> {
        self.app.as_mut().map(|a| &mut a.author.graph)
    }

    /// Opens the skeleton file at `path` in the graph window.
    pub(super) fn open_graph(&mut self, ctx: &mut Ctx, path: &str) -> Result<(), String> {
        let text = read_text(ctx, path)?;
        let mut file: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: not JSON: {e}"))?;
        if file["format"] != "ponko-skeleton2d" {
            return Err(format!(
                "{path} isn't a skeleton in Ponko's format (\"format\": \"ponko-skeleton2d\"); convert a Spine export with SkeletonData::to_json first"
            ));
        }
        SkeletonData::from_json(&text, path).map_err(|e| format!("{path}: {e}"))?;
        if file.get("graph").is_none() {
            let clips = names(&file["clips"]);
            let state = clips.first().cloned().unwrap_or_else(|| "idle".into());
            let mut s = json!({ "name": state });
            if let Some(c) = clips.first() {
                s["clip"] = json!(c);
            }
            file["graph"] = json!({ "states": [s] });
        }
        ctx.editor.docs.open(path, file, write_json);
        let state = self.graph().ok_or("not the editor")?;
        *state = GraphState {
            path: Some(path.to_string()),
            ..GraphState::default()
        };
        Ok(())
    }

    /// The graph window, one frame.
    pub(super) fn graph_window(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let Some(path) = self.graph().and_then(|g| g.path.clone()) else {
            return;
        };
        let Some(file) = ctx.editor.docs.get(&path).cloned() else {
            if let Some(g) = self.graph() {
                *g = GraphState::default();
            }
            return;
        };
        let version = ctx.editor.docs.version();
        let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
        let unsaved = ctx.editor.docs.unsaved(&path);
        let mut new_file = None;
        let mut save = false;
        let mut open = true;
        {
            let Some(g) = self.graph() else { return };
            if g.applied != version {
                g.applied = version;
                rebuild(g, &file, &path);
            }
            step(g, dt);
        }
        egui::Window::new(format!("Animation graph — {path}{}", if unsaved { " *" } else { "" }))
            .id(egui::Id::unique("ponko-graph-window"))
            .open(&mut open)
            .default_pos(egui::pos2(320.0, 60.0))
            .default_width(560.0)
            .show(ui.ctx(), |ui| {
                let Some(g) = self.app.as_mut().map(|a| &mut a.author.graph) else {
                    return;
                };
                let mut f = file.clone();
                ui.horizontal(|ui| {
                    save = ui.add_enabled(unsaved, egui::Button::new("Save")).clicked();
                    if ui.button("+ State").clicked() {
                        let taken = names(&f["graph"]["states"]);
                        let name = fresh("state", &taken);
                        let mut s = json!({ "name": name });
                        if let Some(c) = names(&f["clips"]).first() {
                            s["clip"] = json!(c);
                        }
                        push(&mut f["graph"]["states"], s);
                        g.selected = Some(Selected::State(name));
                    }
                    ui.menu_button("+ Parameter", |ui| {
                        for kind in ["float", "bool", "trigger"] {
                            if ui.button(kind).clicked() {
                                let taken = names(&f["graph"]["parameters"]);
                                let name = fresh(kind, &taken);
                                let default = match kind {
                                    "float" => json!(0.0),
                                    _ => json!(false),
                                };
                                push(&mut f["graph"]["parameters"], json!({ "name": name, kind: default }));
                                ui.close();
                            }
                        }
                    });
                });
                canvas(ui, g, &mut f);
                if let Some(e) = &g.error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
                ui.separator();
                match g.selected.clone() {
                    Some(Selected::State(name)) => state_editor(ui, g, &mut f, &name),
                    Some(Selected::Transition(i)) => transition_editor(ui, g, &mut f, i),
                    None => drop(ui.label(RichText::new("Click a state or an arrow to edit it").weak())),
                }
                ui.separator();
                parameters(ui, g, &mut f);
                ui.separator();
                preview(ui, g);
                if f != file {
                    new_file = Some(f);
                }
            });
        if let Some(f) = new_file {
            ctx.editor.set_doc_merged(&path, f);
        }
        if save {
            let saved = ctx.editor.docs.save(&path).map_err(|e| e.to_string());
            if self.report(saved).is_some() {
                self.status = Some(format!("saved {path}"));
            }
        }
        if !open {
            ctx.editor.docs.close(&path);
            if let Some(g) = self.graph() {
                *g = GraphState::default();
            }
        }
    }
}

fn push(list: &mut Value, item: Value) {
    match list.as_array_mut() {
        Some(a) => a.push(item),
        None => *list = json!([item]),
    }
}

/// Makes the preview again from the file as it is now; on an error keeps
/// the last one and says why.
fn rebuild(g: &mut GraphState, file: &Value, path: &str) {
    let text = serde_json::to_string(file).unwrap_or_default();
    match SkeletonData::from_json(&text, path) {
        Ok(data) => {
            g.error = None;
            let mut skeleton = Skeleton::new(Arc::new(data));
            // Test values the graph still has carry over.
            apply_tests(g, &mut skeleton);
            g.preview = Some(skeleton);
        }
        Err(e) => g.error = Some(e.to_string()),
    }
}

fn apply_tests(g: &GraphState, skeleton: &mut Skeleton) {
    for (name, value) in &g.test {
        if let Some(p) = skeleton.parameter_id(name) {
            match value {
                Test::Float(v) => drop(skeleton.try_set_float(p, *v)),
                Test::Bool(v) => drop(skeleton.try_set_bool(p, *v)),
            }
        }
    }
}

/// One preview step: the test values, triggers fired, then `dt`.
fn step(g: &mut GraphState, dt: f32) {
    let tests = std::mem::take(&mut g.test);
    let fire = std::mem::take(&mut g.fire);
    if let Some(skeleton) = &mut g.preview {
        let view = GraphState {
            test: tests.clone(),
            ..GraphState::default()
        };
        apply_tests(&view, skeleton);
        for name in &fire {
            if let Some(p) = skeleton.parameter_id(name) {
                let _ = skeleton.try_set_trigger(p, true);
            }
        }
        skeleton.update(dt);
    }
    g.test = tests;
}

/// The node of each state (and "Any state"), laid out in rows of three
/// until moved.
fn layout(g: &mut GraphState, states: &[String]) -> Vec<(String, EVec2)> {
    let mut out = vec![(
        ANY.to_string(),
        *g.pos.entry(ANY.into()).or_insert(EVec2::new(8.0, 8.0)),
    )];
    for (i, s) in states.iter().enumerate() {
        let default = EVec2::new(140.0 + (i % 3) as f32 * 130.0, 20.0 + (i / 3) as f32 * 70.0);
        out.push((s.clone(), *g.pos.entry(s.clone()).or_insert(default)));
    }
    out
}

/// The canvas: nodes and arrows; click to select, drag to move, right drag
/// to link.
fn canvas(ui: &mut Ui, g: &mut GraphState, f: &mut Value) {
    let states = names(&f["graph"]["states"]);
    let initial = f["graph"]["initial"]
        .as_str()
        .map(str::to_string)
        .or_else(|| states.first().cloned());
    let (rect, response) = ui.allocate_exact_size(EVec2::new(540.0, 260.0), Sense::click_and_drag());
    ui.data_mut(|d| d.insert_temp(canvas_id(), rect));
    let nodes = layout(g, &states);
    let node_rect = |name: &str| {
        nodes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, at)| Rect::from_min_size(rect.min + *at, NODE))
    };
    for (name, _) in &nodes {
        if let Some(r) = node_rect(name) {
            ui.data_mut(|d| d.insert_temp(node_id(name), r));
        }
    }
    let node_at = |p: Pos2| {
        nodes
            .iter()
            .rev()
            .find(|(n, _)| node_rect(n).is_some_and(|r| r.contains(p)))
            .map(|(n, _)| n.clone())
    };
    // The arrows, from edge to edge of their nodes.
    let transitions: Vec<(Option<String>, String)> = f["graph"]["transitions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            (
                t["from"].as_str().map(str::to_string),
                t["to"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let arrow = |from: &Option<String>, to: &str| -> Option<(Pos2, Pos2)> {
        let a = node_rect(from.as_deref().unwrap_or(ANY))?.center();
        let b = node_rect(to)?.center();
        // Two arrows between the same states (there and back) side by side.
        let side = (b - a).normalized().rot90() * 6.0;
        Some((a + side, b + side))
    };
    let press = ui.input(|i| i.pointer.press_origin());
    let (secondary, primary) = ui.input(|i| (i.pointer.secondary_down(), i.pointer.primary_down()));
    if response.drag_started() {
        if let Some(p) = press
            && let Some(n) = node_at(p)
        {
            if secondary {
                g.linking = Some(n);
            } else {
                g.selected = (n != ANY).then(|| Selected::State(n.clone()));
                ui.data_mut(|d| d.insert_temp(canvas_id().with("drag"), n));
            }
        }
    }
    if response.dragged()
        && primary
        && let Some(n) = ui.data(|d| d.get_temp::<String>(canvas_id().with("drag")))
    {
        let at = g.pos.entry(n).or_insert(EVec2::ZERO);
        *at += response.drag_delta();
        at.x = at.x.clamp(0.0, rect.width() - NODE.x);
        at.y = at.y.clamp(0.0, rect.height() - NODE.y);
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.remove::<String>(canvas_id().with("drag")));
        if let Some(from) = g.linking.take()
            && let Some(p) = response
                .interact_pointer_pos()
                .or_else(|| ui.input(|i| i.pointer.latest_pos()))
            && let Some(to) = node_at(p)
            && to != ANY
            && to != from
        {
            let mut t = json!({ "to": to });
            if from != ANY {
                t["from"] = json!(from);
            }
            push(&mut f["graph"]["transitions"], t);
            let n = f["graph"]["transitions"].as_array().map_or(0, Vec::len);
            g.selected = Some(Selected::Transition(n - 1));
        }
    }
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        g.selected = match node_at(p) {
            Some(n) if n != ANY => Some(Selected::State(n)),
            Some(_) => None,
            None => transitions
                .iter()
                .enumerate()
                .filter_map(|(i, (from, to))| {
                    let (a, b) = arrow(from, to)?;
                    Some((i, distance_to_segment(p, a, b)))
                })
                .filter(|(_, d)| *d < 6.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| Selected::Transition(i)),
        };
    }
    // Drawing.
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 4.0, visuals.extreme_bg_color);
    let text = visuals.text_color();
    let accent = Color32::from_rgb(240, 180, 60);
    for (i, (from, to)) in transitions.iter().enumerate() {
        let Some((a, b)) = arrow(from, to) else { continue };
        let chosen = g.selected == Some(Selected::Transition(i));
        let stroke = Stroke::new(if chosen { 3.0 } else { 1.5 }, if chosen { accent } else { text });
        // Stop at the target node's edge.
        let end = node_rect(to).map_or(b, |r| clip_to_rect(a, b, r.expand(2.0)));
        painter.line_segment([a, end], stroke);
        let dir = (end - a).normalized();
        let head = [
            end,
            end - dir * 9.0 + dir.rot90() * 5.0,
            end - dir * 9.0 - dir.rot90() * 5.0,
        ];
        painter.add(egui::Shape::convex_polygon(head.to_vec(), stroke.color, Stroke::NONE));
    }
    if let Some(from) = &g.linking
        && let (Some(r), Some(p)) = (node_rect(from), ui.input(|i| i.pointer.latest_pos()))
    {
        painter.line_segment([r.center(), p], Stroke::new(1.5, accent));
    }
    for (name, _) in &nodes {
        let Some(r) = node_rect(name) else { continue };
        let chosen = g.selected == Some(Selected::State(name.clone()));
        let playing = g
            .preview
            .as_ref()
            .and_then(|s| Some(s.data().graph()?.states().get(s.state()?.0)?.name.to_string()))
            .is_some_and(|n| n == *name);
        let fill = if name == ANY {
            Color32::from_gray(70)
        } else if playing {
            Color32::from_rgb(40, 110, 70)
        } else {
            Color32::from_rgb(45, 60, 90)
        };
        painter.rect_filled(r, 6.0, fill);
        if chosen {
            painter.rect_stroke(r, 6.0, Stroke::new(2.0, accent), egui::StrokeKind::Outside);
        }
        if initial.as_deref() == Some(name.as_str()) {
            painter.circle_filled(r.left_center() + EVec2::new(8.0, 0.0), 4.0, accent);
        }
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(13.0),
            Color32::WHITE,
        );
    }
    response.on_hover_text("Click to select; drag to move; right-drag from one state onto another to add a transition");
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// Where the segment from `a` (outside) to `b` (inside `r`) enters `r`.
fn clip_to_rect(a: Pos2, b: Pos2, r: Rect) -> Pos2 {
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if r.contains(a + (b - a) * mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    a + (b - a) * hi
}

/// A clip picker over the skeleton's clips.
fn clip_combo(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, clips: &[String], value: &mut Value) {
    let now = value.as_str().unwrap_or("").to_string();
    egui::ComboBox::from_id_salt(id).selected_text(&now).show_ui(ui, |ui| {
        for c in clips {
            if ui.selectable_label(now == *c, c).clicked() {
                *value = json!(c);
            }
        }
    });
}

fn state_editor(ui: &mut Ui, g: &mut GraphState, f: &mut Value, name: &str) {
    let clips = names(&f["clips"]);
    let floats: Vec<String> = f["graph"]["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| kind_of(p) == "float")
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    let states = names(&f["graph"]["states"]);
    let Some(i) = states.iter().position(|s| s == name) else {
        g.selected = None;
        return;
    };
    ui.label(RichText::new("State").strong());
    // The name being typed, applied when the field loses focus.
    let mut text = match &g.naming {
        Some((of, text)) if of == name => text.clone(),
        _ => name.to_string(),
    };
    let mut renamed = name.to_string();
    ui.horizontal(|ui| {
        ui.label("Name");
        let field = ui.text_edit_singleline(&mut text);
        if field.lost_focus() {
            let wanted = text.trim();
            if !wanted.is_empty() && wanted != name && !states.iter().any(|s| s == wanted) && wanted != ANY {
                renamed = wanted.to_string();
            }
            g.naming = None;
        } else if field.has_focus() {
            g.naming = Some((name.to_string(), text.clone()));
        }
    });
    let s = &mut f["graph"]["states"][i];
    let blend = s.get("blend1d").is_some();
    ui.horizontal(|ui| {
        if ui.radio(!blend, "One clip").clicked()
            && blend
            && let Some(obj) = s.as_object_mut()
        {
            obj.remove("blend1d");
            obj.insert("clip".into(), json!(clips.first().cloned().unwrap_or_default()));
        }
        if ui.radio(blend, "Blend tree (1D)").clicked()
            && !blend
            && let Some(obj) = s.as_object_mut()
        {
            obj.remove("clip");
            let c = clips.first().cloned().unwrap_or_default();
            obj.insert(
                "blend1d".into(),
                json!({ "parameter": floats.first().cloned().unwrap_or_default(), "clips": [[0.0, c], [1.0, c]] }),
            );
        }
    });
    if s.get("blend1d").is_some() {
        let b = &mut s["blend1d"];
        ui.horizontal(|ui| {
            ui.label("Parameter");
            let now = b["parameter"].as_str().unwrap_or("").to_string();
            egui::ComboBox::from_id_salt("ponko-graph-blend-param")
                .selected_text(&now)
                .show_ui(ui, |ui| {
                    for p in &floats {
                        if ui.selectable_label(now == *p, p).clicked() {
                            b["parameter"] = json!(p);
                        }
                    }
                });
        });
        let mut remove = None;
        let count = b["clips"].as_array().map_or(0, Vec::len);
        for k in 0..count {
            ui.horizontal(|ui| {
                let mut at = b["clips"][k][0].as_f64().unwrap_or(0.0);
                if ui.add(egui::DragValue::new(&mut at).speed(0.05)).changed() {
                    b["clips"][k][0] = json!(at as f32);
                }
                clip_combo(ui, ("ponko-graph-blend-clip", k), &clips, &mut b["clips"][k][1]);
                if ui.small_button("×").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove
            && let Some(a) = b["clips"].as_array_mut()
        {
            a.remove(k);
        }
        if ui.small_button("+ child").clicked() {
            let last = b["clips"][count.saturating_sub(1)][0].as_f64().unwrap_or(0.0);
            push(
                &mut b["clips"],
                json!([last as f32 + 1.0, clips.first().cloned().unwrap_or_default()]),
            );
        }
    } else {
        ui.horizontal(|ui| {
            ui.label("Clip");
            clip_combo(ui, "ponko-graph-clip", &clips, &mut s["clip"]);
        });
    }
    ui.horizontal(|ui| {
        ui.label("Speed");
        let mut speed = s["speed"].as_f64().unwrap_or(1.0);
        if ui
            .add(egui::DragValue::new(&mut speed).speed(0.05).range(0.0..=100.0))
            .changed()
        {
            s["speed"] = json!(speed as f32);
        }
        let mut looping = s["loop"].as_bool().unwrap_or(true);
        if ui.checkbox(&mut looping, "Loop").changed() {
            s["loop"] = json!(looping);
        }
    });
    let mut delete = false;
    let mut to_add = None;
    ui.horizontal(|ui| {
        if ui.button("Initial state").clicked() {
            f["graph"]["initial"] = json!(name);
        }
        delete = ui.button("Delete state").clicked();
        egui::ComboBox::from_id_salt("ponko-graph-add-transition")
            .selected_text("Add transition to")
            .show_ui(ui, |ui| {
                for other in states.iter().filter(|o| *o != name) {
                    if ui.selectable_label(false, other).clicked() {
                        to_add = Some(other.clone());
                    }
                }
            });
    });
    if let Some(to) = to_add {
        push(&mut f["graph"]["transitions"], json!({ "from": name, "to": to }));
        let n = f["graph"]["transitions"].as_array().map_or(0, Vec::len);
        g.selected = Some(Selected::Transition(n - 1));
    }
    if delete {
        if let Some(a) = f["graph"]["states"].as_array_mut() {
            a.remove(i);
        }
        if let Some(a) = f["graph"]["transitions"].as_array_mut() {
            a.retain(|t| t["to"] != name && t["from"] != name);
        }
        if f["graph"]["initial"] == name
            && let Some(obj) = f["graph"].as_object_mut()
        {
            obj.remove("initial");
        }
        g.selected = None;
    } else if renamed != name {
        rename_state(f, name, &renamed);
        if let Some(at) = g.pos.remove(name) {
            g.pos.insert(renamed.clone(), at);
        }
        g.selected = Some(Selected::State(renamed));
    }
}

/// Renames state `from` to `to` everywhere the graph names it.
fn rename_state(f: &mut Value, from: &str, to: &str) {
    for s in f["graph"]["states"].as_array_mut().into_iter().flatten() {
        if s["name"] == from {
            s["name"] = json!(to);
        }
    }
    for t in f["graph"]["transitions"].as_array_mut().into_iter().flatten() {
        for key in ["from", "to"] {
            if t[key] == from {
                t[key] = json!(to);
            }
        }
    }
    if f["graph"]["initial"] == from {
        f["graph"]["initial"] = json!(to);
    }
}

fn transition_editor(ui: &mut Ui, g: &mut GraphState, f: &mut Value, i: usize) {
    let params: Vec<(String, &'static str)> = f["graph"]["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["name"].as_str()?.to_string(), kind_of(p))))
        .collect();
    let Some(t) = f["graph"]["transitions"].get_mut(i) else {
        g.selected = None;
        return;
    };
    ui.label(
        RichText::new(format!(
            "Transition {} → {}",
            t["from"].as_str().unwrap_or(ANY),
            t["to"].as_str().unwrap_or("?")
        ))
        .strong(),
    );
    // Conditions: all must hold.
    let mut remove = None;
    let count = t["when"].as_array().map_or(0, Vec::len);
    for k in 0..count {
        ui.horizontal(|ui| {
            let c = &mut t["when"][k];
            let (kind, args) = c
                .as_object()
                .and_then(|o| o.iter().next())
                .map(|(k, v)| (k.clone(), v.clone()))
                .unwrap_or_default();
            let param = match &args {
                Value::Array(a) => a.first().and_then(Value::as_str).unwrap_or("").to_string(),
                Value::String(s) => s.clone(),
                _ => String::new(),
            };
            let mut new_kind = kind.clone();
            egui::ComboBox::from_id_salt(("ponko-graph-cond-kind", k))
                .selected_text(&kind)
                .show_ui(ui, |ui| {
                    for option in ["greater", "less", "bool", "trigger"] {
                        ui.selectable_value(&mut new_kind, option.to_string(), option);
                    }
                });
            let mut new_param = param.clone();
            egui::ComboBox::from_id_salt(("ponko-graph-cond-param", k))
                .selected_text(&param)
                .show_ui(ui, |ui| {
                    for (p, _) in &params {
                        ui.selectable_value(&mut new_param, p.clone(), p);
                    }
                });
            let mut value = args.get(1).cloned().unwrap_or(Value::Null);
            match new_kind.as_str() {
                "greater" | "less" => {
                    let mut x = value.as_f64().unwrap_or(0.0);
                    ui.add(egui::DragValue::new(&mut x).speed(0.05));
                    value = json!(x as f32);
                }
                "bool" => {
                    let mut on = value.as_bool().unwrap_or(true);
                    ui.checkbox(&mut on, "");
                    value = json!(on);
                }
                _ => {}
            }
            *c = match new_kind.as_str() {
                "trigger" => json!({ "trigger": new_param }),
                k => json!({ k: [new_param, value] }),
            };
            if ui.small_button("×").clicked() {
                remove = Some(k);
            }
        });
    }
    if let Some(k) = remove
        && let Some(a) = t["when"].as_array_mut()
    {
        a.remove(k);
        if a.is_empty()
            && let Some(o) = t.as_object_mut()
        {
            o.remove("when");
        }
    }
    if ui.small_button("+ condition").clicked() {
        let c = match params.first() {
            Some((p, "bool")) => json!({ "bool": [p, true] }),
            Some((p, "trigger")) => json!({ "trigger": p }),
            Some((p, _)) => json!({ "greater": [p, 0.0] }),
            None => json!({ "greater": ["", 0.0] }),
        };
        push(&mut t["when"], c);
    }
    ui.horizontal(|ui| {
        let mut exit = t["exit_time"].as_f64();
        let mut on = exit.is_some();
        if ui.checkbox(&mut on, "Exit time").changed() {
            exit = on.then_some(1.0);
        }
        if let Some(x) = &mut exit {
            ui.add(egui::DragValue::new(x).speed(0.01).range(0.0..=100.0));
        }
        match exit {
            Some(x) => t["exit_time"] = json!(x as f32),
            None => drop(t.as_object_mut().and_then(|o| o.remove("exit_time"))),
        }
    });
    for (key, label) in [("duration", "Fade (s)"), ("offset", "Offset")] {
        ui.horizontal(|ui| {
            ui.label(label);
            let mut x = t[key].as_f64().unwrap_or(0.0);
            if ui
                .add(egui::DragValue::new(&mut x).speed(0.01).range(0.0..=100.0))
                .changed()
            {
                t[key] = json!(x as f32);
            }
        });
    }
    if ui.button("Delete transition").clicked() {
        if let Some(a) = f["graph"]["transitions"].as_array_mut() {
            a.remove(i);
        }
        g.selected = None;
    }
}

/// The parameters: their names and defaults (in the file) and their test
/// values (for the preview only).
fn parameters(ui: &mut Ui, g: &mut GraphState, f: &mut Value) {
    ui.label(RichText::new("Parameters").strong());
    let mut remove = None;
    let count = f["graph"]["parameters"].as_array().map_or(0, Vec::len);
    for k in 0..count {
        let p = &mut f["graph"]["parameters"][k];
        let name = p["name"].as_str().unwrap_or("").to_string();
        let kind = kind_of(p);
        ui.horizontal(|ui| {
            ui.label(format!("{name} ({kind})"));
            match kind {
                "float" => {
                    let mut x = g
                        .test
                        .get(&name)
                        .map_or(p["float"].as_f64().unwrap_or(0.0) as f32, |t| match t {
                            Test::Float(v) => *v,
                            Test::Bool(_) => 0.0,
                        });
                    ui.label("test");
                    if ui.add(egui::DragValue::new(&mut x).speed(0.05)).changed() {
                        g.test.insert(name.clone(), Test::Float(x));
                    }
                }
                "bool" => {
                    let mut on = g
                        .test
                        .get(&name)
                        .map_or(p["bool"].as_bool().unwrap_or(false), |t| matches!(t, Test::Bool(true)));
                    if ui.checkbox(&mut on, "test").changed() {
                        g.test.insert(name.clone(), Test::Bool(on));
                    }
                }
                _ => {
                    if ui
                        .button("Fire")
                        .on_hover_text("Set the trigger for the preview")
                        .clicked()
                    {
                        g.fire.push(name.clone());
                    }
                }
            }
            if ui.small_button("×").on_hover_text("Remove the parameter").clicked() {
                remove = Some(k);
            }
        });
    }
    if let Some(k) = remove
        && let Some(a) = f["graph"]["parameters"].as_array_mut()
    {
        a.remove(k);
    }
}

/// Where the preview's pose was drawn (egui memory, for tests).
pub(crate) fn preview_id() -> egui::Id {
    egui::Id::unique("ponko-graph-preview")
}

/// The preview: the state playing and the pose, bone by bone.
fn preview(ui: &mut Ui, g: &mut GraphState) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Preview").strong());
        if ui.button("Restart").clicked()
            && let Some(s) = &g.preview
        {
            let mut fresh = Skeleton::new(s.data().clone());
            apply_tests(g, &mut fresh);
            g.preview = Some(fresh);
        }
    });
    let Some(s) = &g.preview else { return };
    let state = s
        .state()
        .and_then(|id| Some(s.data().graph()?.states().get(id.0)?.name.to_string()))
        .unwrap_or_else(|| "(none)".into());
    ui.label(format!("Now: {state}, {:.0}%", s.normalized_time() * 100.0));
    let (rect, _) = ui.allocate_exact_size(EVec2::new(240.0, 160.0), Sense::hover());
    ui.data_mut(|d| d.insert_temp(preview_id(), rect));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::from_gray(20));
    let bones = s.data().bones();
    let points: Vec<(glam::Vec2, glam::Vec2)> = (0..bones.len())
        .map(|i| {
            (
                s.bone_position(i),
                s.bone_point(i, glam::Vec2::new(bones[i].length.max(4.0), 0.0)),
            )
        })
        .collect();
    let (mut lo, mut hi) = (glam::Vec2::splat(f32::MAX), glam::Vec2::splat(f32::MIN));
    for (a, b) in &points {
        lo = lo.min(a.min(*b));
        hi = hi.max(a.max(*b));
    }
    let span = (hi - lo).max(glam::Vec2::splat(1.0));
    let scale = ((rect.width() - 20.0) / span.x).min((rect.height() - 20.0) / span.y);
    let middle = (lo + hi) / 2.0;
    // Skeletons are y up; the screen is y down.
    let to = |p: glam::Vec2| rect.center() + EVec2::new((p.x - middle.x) * scale, -(p.y - middle.y) * scale);
    for (a, b) in &points {
        painter.line_segment([to(*a), to(*b)], Stroke::new(3.0, Color32::from_rgb(120, 220, 255)));
        painter.circle_filled(to(*a), 3.0, Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::super::author_harness::*;
    use super::*;

    /// A skeleton of two bones with clips "idle" (the arm still) and
    /// "wave" (the arm turned 90°), and a graph with a float parameter
    /// "speed" and the state "idle".
    struct Fixture {
        file: std::path::PathBuf,
        path: String,
    }

    impl Fixture {
        fn new(what: &str) -> Self {
            let path = format!("ponko-test-graph-{what}-{}.json", std::process::id());
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&path);
            let skeleton = json!({
                "format": "ponko-skeleton2d", "version": 1,
                "bones": [{"name": "root"}, {"name": "arm", "parent": "root", "length": 40}],
                "clips": [
                    {"name": "idle", "bones": [{"bone": "arm", "rotate": [{"time": 0, "value": 0}, {"time": 1, "value": 0}]}]},
                    {"name": "wave", "bones": [{"bone": "arm", "rotate": [{"time": 0, "value": 90}, {"time": 1, "value": 90}]}]}
                ],
                "graph": {"parameters": [{"name": "speed", "float": 0.0}], "states": [{"name": "idle", "clip": "idle"}]}
            });
            std::fs::write(&file, serde_json::to_string_pretty(&skeleton).unwrap()).unwrap();
            Self { file, path }
        }

        fn on_disk(&self) -> Value {
            serde_json::from_str(&std::fs::read_to_string(&self.file).unwrap()).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.file);
        }
    }

    fn graph_editor(f: &Fixture) -> Harness {
        let mut h = Harness::new();
        h.click_text(|t| t == "Author");
        h.click_text(|t| t == "Animation graph…");
        h.frame(vec![]);
        h.type_text(&f.path);
        let g = &h.panels.app.as_ref().unwrap().author;
        assert_eq!(g.graph.path.as_deref(), Some(f.path.as_str()), "{:?}", g.error);
        h
    }

    fn graph(h: &Harness, f: &Fixture) -> Value {
        h.ctx.editor.docs.get(&f.path).unwrap()["graph"].clone()
    }

    fn state(h: &Harness) -> &GraphState {
        &h.panels.app.as_ref().unwrap().author.graph
    }

    fn centre(r: Rect) -> glam::Vec2 {
        glam::vec2(r.center().x, r.center().y)
    }

    /// Presses the right button at `from`, moves to `to`, lets go.
    fn right_drag(h: &mut Harness, from: glam::Vec2, to: glam::Vec2) {
        let right = |at: glam::Vec2, pressed| egui::Event::PointerButton {
            pos: egui::pos2(at.x, at.y),
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        };
        h.frame(vec![moved(from)]);
        h.frame(vec![right(from, true)]);
        for i in 1..=5 {
            h.frame(vec![moved(from.lerp(to, i as f32 / 5.0))]);
        }
        h.frame(vec![right(to, false)]);
        h.frame(vec![]);
    }

    #[test]
    fn adding_a_state_and_a_transition_with_a_condition_by_mouse_saves_a_graph_the_runtime_loads() {
        let f = Fixture::new("build");
        let mut h = graph_editor(&f);
        h.click_text(|t| t == "+ State");
        assert_eq!(graph(&h, &f)["states"][1]["name"], "state");
        // Rename it "wave", playing clip "wave".
        let field = field_right_of(&h.frame(vec![]), "Name");
        h.click(field);
        h.type_text("wave");
        assert_eq!(graph(&h, &f)["states"][1]["name"], "wave");
        let field = field_right_of(&h.frame(vec![]), "Clip");
        h.click(field);
        h.click_last_text(|t| t == "wave");
        assert_eq!(graph(&h, &f)["states"][1]["clip"], "wave");
        // A right drag from idle onto wave adds the transition.
        let from = centre(h.rect(node_id("idle")));
        let to = centre(h.rect(node_id("wave")));
        right_drag(&mut h, from, to);
        assert_eq!(graph(&h, &f)["transitions"][0], json!({"from": "idle", "to": "wave"}));
        assert_eq!(state(&h).selected, Some(Selected::Transition(0)));
        h.click_text(|t| t == "+ condition");
        assert_eq!(
            graph(&h, &f)["transitions"][0]["when"],
            json!([{"greater": ["speed", 0.0]}])
        );
        h.save();
        let disk = f.on_disk();
        assert_eq!(disk["graph"]["transitions"][0]["to"], "wave");
        let data = SkeletonData::from_json(&disk.to_string(), &f.path).expect("the runtime loads it");
        assert_eq!(data.graph().unwrap().states().len(), 2);
    }

    #[test]
    fn the_preview_follows_the_test_parameter_through_the_transition() {
        let f = Fixture::new("preview");
        let mut h = graph_editor(&f);
        // wave, and idle → wave when speed > 0.5.
        let mut file = h.ctx.editor.docs.get(&f.path).unwrap().clone();
        push(&mut file["graph"]["states"], json!({"name": "wave", "clip": "wave"}));
        file["graph"]["transitions"] = json!([{"from": "idle", "to": "wave", "when": [{"greater": ["speed", 0.5]}]}]);
        h.ctx.editor.set_doc(&f.path, file);
        for _ in 0..3 {
            h.frame(vec![]);
        }
        let now = |h: &Harness| {
            let s = state(h).preview.as_ref().unwrap();
            s.data().graph().unwrap().states()[s.state().unwrap().0]
                .name
                .to_string()
        };
        assert_eq!(now(&h), "idle");
        // Drag the test value of speed up past 0.5.
        let field = field_right_of(&h.frame(vec![]), "test");
        h.drag(field, field + glam::vec2(60.0, 0.0), 4);
        for _ in 0..3 {
            h.frame(vec![]);
        }
        assert_eq!(now(&h), "wave");
        assert!(
            !h.ctx.editor.docs.unsaved(&f.path) || graph(&h, &f)["parameters"][0]["float"] == 0.0,
            "a test value isn't saved in the file"
        );
    }

    #[test]
    fn a_broken_graph_says_why_in_place_and_undo_mends_it() {
        let f = Fixture::new("broken");
        let mut h = graph_editor(&f);
        let mut file = h.ctx.editor.docs.get(&f.path).unwrap().clone();
        file["graph"]["transitions"] = json!([{"from": "idle", "to": "nowhere"}]);
        h.ctx.editor.set_doc(&f.path, file);
        let shapes = h.frame(vec![]);
        let error = state(&h).error.clone().expect("the graph doesn't load");
        assert!(error.contains("nowhere"), "{error}");
        assert!(
            texts(&shapes).iter().any(|(t, _)| t.contains("nowhere")),
            "shown in the window"
        );
        h.undo();
        h.frame(vec![]);
        assert!(state(&h).error.is_none());
    }

    #[test]
    fn deleting_a_state_removes_its_transitions_and_renaming_follows_them() {
        let mut f = json!({"graph": {"states": [{"name": "a"}, {"name": "b"}],
            "transitions": [{"from": "a", "to": "b"}, {"to": "a"}], "initial": "a"}});
        rename_state(&mut f, "a", "c");
        assert_eq!(
            f["graph"]["transitions"],
            json!([{"from": "c", "to": "b"}, {"to": "c"}])
        );
        assert_eq!(f["graph"]["initial"], "c");
    }
}
