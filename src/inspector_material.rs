//! The materials and shader windows.
//!
//! **Materials** edits the materials of a glTF file (`.gltf`, the JSON
//! kind): base colour, metallic, roughness, emission, normal strength,
//! alpha mode, double-sidedness, and which image each texture slot uses
//! (one of the file's, or a PNG from `assets/`). Every model drawn with the
//! file's materials changes as you edit, and Save writes the `.gltf` back,
//! which `load_mesh` and scenes load as before. A `.glb` keeps its JSON
//! inside a binary file: export it as `.gltf` to edit it here.
//!
//! **Shader** edits a custom material shader (`.wgsl`) with its errors in
//! place: the line with the first error is marked and the compiler's
//! message is under the code. While the code compiles, the game's shader
//! loaded from the file takes it at once (on an error the last good code
//! stays in use, as with hot reload). Its materials' custom values
//! (`material.custom[0]` to `[3]`) and custom textures are edited live too;
//! those are set in code (`MaterialDesc::with_custom`), so Save writes the
//! `.wgsl` alone, and **Copy as Rust** copies the values to paste there.
//!
//! A glTF material can name its custom shader and values itself, in its
//! `extras` (see `three::material_extras`): the materials window's
//! **Custom shader** section edits them there, by name and type, and they
//! load with the file.

use egui::{Color32, RichText, Ui};
use glam::Vec4;
use serde_json::{Value, json};

use super::Panels;
use super::author::{near_first, read_text, relative, write_json};
use crate::ctx::Ctx;
use crate::render::{Material, ShaderDiagnostic};

/// The texture slots of a glTF material: label, and where the slot is in
/// the material's JSON.
const SLOTS: [(&str, &[&str]); 5] = [
    ("Base colour", &["pbrMetallicRoughness", "baseColorTexture"]),
    (
        "Metallic-roughness",
        &["pbrMetallicRoughness", "metallicRoughnessTexture"],
    ),
    ("Normal", &["normalTexture"]),
    ("Occlusion", &["occlusionTexture"]),
    ("Emissive", &["emissiveTexture"]),
];

/// The two windows' state.
#[derive(Default)]
pub(crate) struct MaterialState {
    /// The `.gltf` in the materials window.
    pub gltf: Option<String>,
    /// Which of its materials is shown.
    pub selected: usize,
    applied: u64,
    /// The file's images, textures and texture slots as last applied: when
    /// they change the images are read again, else only values change.
    images_applied: Option<String>,
    /// Whether the file is loaded (its edits show), after the last apply.
    pub live: bool,
    pub error: Option<String>,
    /// The `.wgsl` in the shader window.
    pub shader: Option<String>,
    shader_applied: u64,
    /// The first error of the code as it is now.
    pub diagnostic: Option<Box<ShaderDiagnostic>>,
    /// Code that checked but isn't swapped in yet: (version, when edited).
    pending: Option<(u64, f64)>,
    /// What swapping the code in said, if it failed.
    pub compile_error: Option<String>,
}

/// glTF colour factors are linear; the colour editors show sRGB.
fn to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn number(v: &Value, default: f32) -> f32 {
    v.as_f64().map_or(default, |x| x as f32)
}

/// The value at `path` inside `v`, made (as objects) if it isn't there.
fn at_mut<'a>(v: &'a mut Value, path: &[&str]) -> &'a mut Value {
    path.iter().fold(v, |v, key| &mut v[*key])
}

fn at<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(v, |v, key| &v[*key])
}

/// Takes `key` out of object `v`, and `v` out of its parent when that
/// leaves it empty (one level, for `pbrMetallicRoughness`).
fn remove(v: &mut Value, path: &[&str]) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    if let Some(o) = at_mut(v, parents).as_object_mut() {
        o.remove(*last);
    }
}

/// What the file's images and texture slots are, to tell an edit of values
/// from one that needs the images read again.
fn images_signature(gltf: &Value) -> String {
    let slots: Vec<Value> = gltf["materials"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| Value::Array(SLOTS.iter().map(|(_, path)| at(m, path)["index"].clone()).collect()))
        .collect();
    json!([gltf["images"], gltf["textures"], gltf["samplers"], slots]).to_string()
}

/// A short name for texture `i` of the file: its image's name or file.
fn texture_name(gltf: &Value, i: usize) -> String {
    let image = gltf["textures"][i]["source"]
        .as_u64()
        .map(|s| &gltf["images"][s as usize]);
    image
        .and_then(|im| {
            im["name"]
                .as_str()
                .or_else(|| im["uri"].as_str().filter(|u| !u.starts_with("data:")))
        })
        .map_or_else(|| format!("texture {i}"), str::to_string)
}

/// Writes the shader document's code to its file.
fn write_shader(path: &str, doc: &Value) -> Result<(), crate::error::Error> {
    crate::assets::write_asset(path, doc["source"].as_str().unwrap_or("").as_bytes())
}

/// The custom values in a shader document.
fn custom_of(doc: &Value) -> [Vec4; 4] {
    let mut out = [Vec4::ZERO; 4];
    for (i, row) in out.iter_mut().enumerate() {
        for k in 0..4 {
            row[k] = number(&doc["custom"][i][k], 0.0);
        }
    }
    out
}

impl Panels {
    fn materials(&mut self) -> Option<&mut MaterialState> {
        self.app.as_mut().map(|a| &mut a.author.materials)
    }

    /// Opens the glTF file at `path` in the materials window.
    pub(super) fn open_materials(&mut self, ctx: &mut Ctx, path: &str) -> Result<(), String> {
        if path.to_ascii_lowercase().ends_with(".glb") {
            return Err(format!(
                "{path} is binary glTF: export it as .gltf (JSON) to edit its materials here"
            ));
        }
        let text = read_text(ctx, path)?;
        let gltf: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: not glTF JSON: {e}"))?;
        if gltf["materials"].as_array().is_none_or(Vec::is_empty) {
            return Err(format!("{path} has no materials"));
        }
        ctx.editor.docs.open(path, gltf, write_json);
        let state = self.materials().ok_or("not the editor")?;
        state.gltf = Some(path.to_string());
        state.selected = 0;
        state.applied = u64::MAX;
        state.images_applied = None;
        state.error = None;
        Ok(())
    }

    /// Opens the material shader at `path` in the shader window.
    pub(super) fn open_shader(&mut self, ctx: &mut Ctx, path: &str) -> Result<(), String> {
        let source = read_text(ctx, path)?;
        // The custom values of the game's materials drawn with it, if any.
        let first = ctx
            .gfx
            .loaded_shader(path)
            .and_then(|s| ctx.gfx.materials_with_shader(s).into_iter().next())
            .map(|(_, d)| d);
        let custom = first.map_or([Vec4::ZERO; 4], |d| d.custom);
        let doc = json!({
            "source": source,
            "custom": custom.iter().map(|v| v.to_array().to_vec()).collect::<Vec<_>>(),
            "textures": [null, null],
        });
        ctx.editor.docs.open(path, doc, write_shader);
        let state = self.materials().ok_or("not the editor")?;
        state.shader = Some(path.to_string());
        state.shader_applied = u64::MAX;
        state.diagnostic = None;
        state.compile_error = None;
        state.pending = None;
        Ok(())
    }

    /// The materials window, one frame, and the file's materials following
    /// its edits.
    pub(super) fn material_window(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let Some(state) = self.materials() else { return };
        let Some(path) = state.gltf.clone() else { return };
        let Some(gltf) = ctx.editor.docs.get(&path).cloned() else {
            state.gltf = None;
            return;
        };
        let version = ctx.editor.docs.version();
        if state.applied != version {
            state.applied = version;
            let images = images_signature(&gltf);
            let values = state.images_applied.as_deref() == Some(images.as_str());
            let text = serde_json::to_vec(&gltf).unwrap_or_default();
            match ctx.gfx.apply_file_materials(&path, &text, values) {
                Ok(live) => {
                    state.live = live;
                    state.error = None;
                    state.images_applied = Some(images);
                }
                Err(e) => state.error = Some(e),
            }
        }
        let unsaved = ctx.editor.docs.unsaved(&path);
        let pngs = near_first(self.asset_files("png"), &path);
        let shaders = near_first(self.asset_files("wgsl"), &path);
        let mut open = true;
        let mut save = false;
        let mut edited = None;
        egui::Window::new(format!("Materials — {path}{}", if unsaved { " *" } else { "" }))
            .id(egui::Id::unique("ponko-materials-window"))
            .open(&mut open)
            .default_pos(egui::pos2(320.0, 80.0))
            .show(ui.ctx(), |ui| {
                let Some(state) = self.app.as_mut().map(|a| &mut a.author.materials) else {
                    return;
                };
                let mut g = gltf.clone();
                let count = g["materials"].as_array().map_or(0, Vec::len);
                state.selected = state.selected.min(count.saturating_sub(1));
                ui.horizontal(|ui| {
                    save = ui.add_enabled(unsaved, egui::Button::new("Save")).clicked();
                    let name = |i: usize| {
                        g["materials"][i]["name"]
                            .as_str()
                            .map_or(format!("material {i}"), str::to_string)
                    };
                    egui::ComboBox::from_id_salt("ponko-materials-pick")
                        .selected_text(name(state.selected))
                        .show_ui(ui, |ui| {
                            for i in 0..count {
                                ui.selectable_value(&mut state.selected, i, name(i));
                            }
                        });
                });
                if !state.live {
                    ui.label(
                        RichText::new("Not loaded in the game: load or place it to see the edits on its models").weak(),
                    );
                }
                let m = &mut g["materials"][state.selected];
                egui::Grid::new("ponko-materials-grid").num_columns(2).show(ui, |ui| {
                    material_fields(ui, m);
                    ui.end_row();
                });
                ui.separator();
                ui.label(RichText::new("Textures").strong());
                egui::Grid::new("ponko-materials-textures")
                    .num_columns(2)
                    .show(ui, |ui| {
                        let selected = state.selected;
                        for (label, slot) in SLOTS {
                            ui.label(label);
                            texture_slot(ui, &mut g, selected, slot, &path, &pngs);
                            ui.end_row();
                        }
                    });
                ui.separator();
                custom_section(ui, &mut g["materials"][state.selected], &path, &shaders);
                if let Some(e) = &state.error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
                if g != gltf {
                    edited = Some(g);
                }
            });
        if let Some(g) = edited {
            ctx.editor.set_doc_merged(&path, g);
        }
        if save {
            let saved = ctx.editor.docs.save(&path).map_err(|e| e.to_string());
            if self.report(saved).is_some() {
                self.status = Some(format!("saved {path}"));
            }
        }
        if !open {
            ctx.editor.docs.close(&path);
            if let Some(state) = self.materials() {
                state.gltf = None;
            }
        }
    }

    /// The shader window, one frame, and the game's shader and materials
    /// following its edits.
    pub(super) fn shader_window(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        let Some(state) = self.materials() else { return };
        let Some(path) = state.shader.clone() else { return };
        let Some(doc) = ctx.editor.docs.get(&path).cloned() else {
            state.shader = None;
            return;
        };
        let version = ctx.editor.docs.version();
        let now = ui.input(|i| i.time);
        let source = doc["source"].as_str().unwrap_or("").to_string();
        let shader = ctx.gfx.loaded_shader(&path);
        if state.shader_applied != version {
            state.shader_applied = version;
            state.diagnostic = crate::gfx::Graphics::check_material_shader(&path, &source).err();
            // Code that checks is swapped in once typing pauses.
            state.pending = state.diagnostic.is_none().then_some((version, now));
            apply_custom(ctx, shader, &doc);
        }
        if let Some((_, since)) = state.pending
            && now - since >= 0.3
        {
            state.pending = None;
            state.compile_error = match shader {
                Some(s) => ctx.gfx.replace_shader_source(s, &path, &source).err(),
                None => None,
            };
        }
        if state.pending.is_some() {
            ui.ctx().request_repaint();
        }
        let users = shader.map_or(0, |s| ctx.gfx.materials_with_shader(s).len());
        let unsaved = ctx.editor.docs.unsaved(&path);
        let pngs = near_first(self.asset_files("png"), &path);
        let mut open = true;
        let mut save = false;
        let mut edited = None;
        egui::Window::new(format!("Shader — {path}{}", if unsaved { " *" } else { "" }))
            .id(egui::Id::unique("ponko-shader-window"))
            .open(&mut open)
            .default_pos(egui::pos2(340.0, 100.0))
            .default_width(560.0)
            .show(ui.ctx(), |ui| {
                let Some(state) = self.app.as_mut().map(|a| &mut a.author.materials) else {
                    return;
                };
                let mut d = doc.clone();
                ui.horizontal(|ui| {
                    save = ui.add_enabled(unsaved, egui::Button::new("Save")).clicked();
                    if ui
                        .button("Copy as Rust")
                        .on_hover_text(
                            "The custom values as MaterialDesc code, to paste where the game makes its material",
                        )
                        .clicked()
                    {
                        ui.ctx().copy_text(rust_snippet(&path, &d));
                    }
                    ui.label(
                        RichText::new(match (shader, users) {
                            (None, _) => "not loaded in the game".to_string(),
                            (Some(_), n) => format!("{n} material(s) in the game use it"),
                        })
                        .weak(),
                    );
                });
                let error_line = state
                    .diagnostic
                    .as_ref()
                    .filter(|d| !d.in_engine_code)
                    .and_then(|d| d.line);
                code_editor(ui, &mut d, error_line);
                match (&state.diagnostic, &state.compile_error) {
                    (Some(e), _) => {
                        let place = match (e.line, e.column, e.in_engine_code) {
                            (Some(l), Some(c), false) => format!("line {l}, column {c}: "),
                            (Some(l), None, false) => format!("line {l}: "),
                            _ => String::new(),
                        };
                        ui.colored_label(Color32::LIGHT_RED, format!("{place}{}", e.message));
                        if !e.label.is_empty() {
                            ui.colored_label(Color32::LIGHT_RED, &e.label);
                        }
                        ui.label(RichText::new("The game keeps the last code that compiled").weak());
                    }
                    (None, Some(e)) => drop(ui.colored_label(Color32::LIGHT_RED, e)),
                    (None, None) => drop(ui.colored_label(Color32::LIGHT_GREEN, "Compiles")),
                }
                ui.separator();
                ui.label(RichText::new("Custom values").strong()).on_hover_text(
                    "material.custom[0] to [3] in the shader; set in code with MaterialDesc::with_custom",
                );
                for i in 0..4 {
                    ui.horizontal(|ui| {
                        ui.label(format!("custom[{i}]"));
                        for k in 0..4 {
                            let mut x = number(&d["custom"][i][k], 0.0);
                            if ui.add(egui::DragValue::new(&mut x).speed(0.01)).changed() {
                                d["custom"][i][k] = json!(x);
                            }
                        }
                    });
                }
                for t in 0..2 {
                    ui.horizontal(|ui| {
                        ui.label(format!("custom_texture{t}"));
                        let now = d["textures"][t].as_str().unwrap_or("(none)").to_string();
                        egui::ComboBox::from_id_salt(("ponko-shader-texture", t))
                            .selected_text(now)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(d["textures"][t].is_null(), "(none)").clicked() {
                                    d["textures"][t] = Value::Null;
                                }
                                for png in &pngs {
                                    if ui.selectable_label(d["textures"][t] == **png, png).clicked() {
                                        d["textures"][t] = json!(png);
                                    }
                                }
                            });
                    });
                }
                if d != doc {
                    edited = Some(d);
                }
            });
        if let Some(d) = edited {
            ctx.editor.set_doc_merged(&path, d);
        }
        if save {
            let saved = ctx.editor.docs.save(&path).map_err(|e| e.to_string());
            if self.report(saved).is_some() {
                self.status = Some(format!("saved {path}"));
            }
        }
        if !open {
            ctx.editor.docs.close(&path);
            if let Some(state) = self.materials() {
                state.shader = None;
            }
        }
    }
}

/// Sets the custom values and textures of the shader document on every
/// material in the game drawn with the shader.
fn apply_custom(ctx: &mut Ctx, shader: Option<crate::render::Shader>, doc: &Value) {
    let Some(shader) = shader else { return };
    let custom = custom_of(doc);
    let textures: [Option<crate::texture::Texture>; 2] = std::array::from_fn(|t| {
        doc["textures"][t]
            .as_str()
            .and_then(|p| ctx.gfx.try_load_texture(p).ok())
    });
    let users: Vec<(Material, crate::render::MaterialDesc)> = ctx.gfx.materials_with_shader(shader);
    for (m, desc) in users {
        let changed = crate::render::MaterialDesc {
            custom,
            custom_textures: textures,
            ..desc
        };
        if changed != desc {
            ctx.gfx.update_material(m, changed);
        }
    }
}

/// The custom values as Rust, to paste into the game's code.
fn rust_snippet(path: &str, doc: &Value) -> String {
    let custom = custom_of(doc);
    let rows: Vec<String> = custom
        .iter()
        .map(|v| format!("vec4({:?}, {:?}, {:?}, {:?})", v.x, v.y, v.z, v.w))
        .collect();
    let mut out = format!(
        "let shader = ctx.gfx.load_shader({path:?});\nlet desc = ponko::render::MaterialDesc::default()\n    .with_shader(Some(shader))\n    .with_custom([{}])",
        rows.join(", ")
    );
    let textures: Vec<String> = (0..2)
        .map(|t| match doc["textures"][t].as_str() {
            Some(p) => format!("Some(ctx.gfx.load_texture({p:?}))"),
            None => "None".into(),
        })
        .collect();
    if textures.iter().any(|t| t != "None") {
        out.push_str(&format!("\n    .with_custom_textures([{}])", textures.join(", ")));
    }
    out.push(';');
    out
}

/// Where the code editor was drawn, and its line height (egui memory, for
/// tests).
pub(crate) fn code_id() -> egui::Id {
    egui::Id::unique("ponko-shader-code")
}

/// The code, with line numbers and the line of `error` marked.
fn code_editor(ui: &mut Ui, doc: &mut Value, error: Option<u32>) {
    let mut text = doc["source"].as_str().unwrap_or("").to_string();
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let mark = Color32::from_rgba_unmultiplied(200, 40, 40, 90);
    let normal = ui.visuals().text_color();
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
        let mut job = egui::text::LayoutJob::default();
        for (i, line) in text.as_str().split_inclusive('\n').enumerate() {
            let marked = error == Some(i as u32 + 1);
            job.append(
                line,
                0.0,
                egui::TextFormat {
                    font_id: mono.clone(),
                    color: normal,
                    background: if marked { mark } else { Color32::TRANSPARENT },
                    ..Default::default()
                },
            );
        }
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let lines = text.lines().count().max(1);
    egui::ScrollArea::vertical()
        .id_salt("ponko-shader-scroll")
        .max_height(320.0)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let numbers: String = (1..=lines).map(|n| format!("{n:>3}\n")).collect();
                ui.label(RichText::new(numbers.trim_end()).font(mono.clone()).weak());
                let response = ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(12)
                        .layouter(&mut layouter),
                );
                ui.data_mut(|d| d.insert_temp(code_id(), response.rect));
            });
        });
    if doc["source"].as_str() != Some(text.as_str()) {
        doc["source"] = json!(text);
    }
}

/// The value fields of a glTF material, as grid rows.
/// The material's custom shader and its values, kept in its `extras`
/// (`ponko:shader`, `ponko:params`; see `three::material_extras`).
fn custom_section(ui: &mut Ui, m: &mut Value, path: &str, shaders: &[String]) {
    use crate::three::material_extras::{CustomExtras, Param, ParamKind};
    ui.label(RichText::new("Custom shader").strong()).on_hover_text(
        "Saved in the material's extras: the shader draws it when the file loads, with these values as material.custom[0] to [3]",
    );
    let read = CustomExtras::read(&m["extras"]);
    let mut c = match read {
        Ok(c) => c.unwrap_or_default(),
        Err(e) => {
            ui.colored_label(Color32::LIGHT_RED, e);
            return;
        }
    };
    let before = c.clone();
    let mut remove = None;
    egui::Grid::new("ponko-materials-custom").num_columns(2).show(ui, |ui| {
        ui.label("Shader");
        let shown = c.shader.clone().unwrap_or_else(|| "built-in".into());
        egui::ComboBox::from_id_salt("ponko-materials-shader")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(c.shader.is_none(), "built-in").clicked() {
                    c.shader = None;
                }
                for file in shaders {
                    let name = relative(path, file);
                    if ui.selectable_label(c.shader.as_deref() == Some(&name), file).clicked() {
                        c.shader = Some(name);
                    }
                }
            });
        ui.end_row();
        for (i, p) in c.params.iter_mut().enumerate() {
            ui.label(format!("custom[{i}]"));
            ui.horizontal(|ui| {
                for k in 0..p.kind.len() {
                    ui.add(egui::DragValue::new(&mut p.value[k]).speed(0.01));
                }
                if p.kind == ParamKind::Color {
                    let mut rgba = p.value.to_array();
                    if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                        p.value = Vec4::from_array(rgba);
                    }
                }
                egui::ComboBox::from_id_salt(("ponko-materials-param-type", i))
                    .selected_text(p.kind.name())
                    .width(60.0)
                    .show_ui(ui, |ui| {
                        for kind in ParamKind::ALL {
                            if ui.selectable_label(p.kind == kind, kind.name()).clicked() && p.kind != kind {
                                p.kind = kind;
                                // The numbers past the new length are 0 (a colour's alpha 1).
                                for k in kind.len()..4 {
                                    p.value[k] = 0.0;
                                }
                                if kind == ParamKind::Color && p.value.w == 0.0 {
                                    p.value.w = 1.0;
                                }
                            }
                        }
                    });
                let mut name = p.name.clone();
                let field = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .desired_width(80.0)
                        .hint_text("name"),
                );
                if field.changed() {
                    p.name = name;
                }
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
            });
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        c.params.remove(i);
    }
    if c.params.len() < 4 && ui.button("+ value").clicked() {
        let i = c.params.len();
        c.params.push(Param {
            name: format!("value{i}"),
            kind: ParamKind::Float,
            value: Vec4::ZERO,
        });
    }
    if c != before {
        c.write(&mut m["extras"]);
        if m["extras"].as_object().is_some_and(serde_json::Map::is_empty)
            && let Some(o) = m.as_object_mut()
        {
            o.remove("extras");
        }
    }
}

fn material_fields(ui: &mut Ui, m: &mut Value) {
    ui.label("Name");
    let mut name = m["name"].as_str().unwrap_or("").to_string();
    if ui.text_edit_singleline(&mut name).lost_focus() && m["name"].as_str().unwrap_or("") != name {
        m["name"] = json!(name);
    }
    ui.end_row();

    ui.label("Base colour");
    let f = &m["pbrMetallicRoughness"]["baseColorFactor"];
    let mut rgba = [
        to_srgb(number(&f[0], 1.0)),
        to_srgb(number(&f[1], 1.0)),
        to_srgb(number(&f[2], 1.0)),
        number(&f[3], 1.0),
    ];
    if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
        m["pbrMetallicRoughness"]["baseColorFactor"] =
            json!([to_linear(rgba[0]), to_linear(rgba[1]), to_linear(rgba[2]), rgba[3]]);
    }
    ui.end_row();

    for (label, key) in [("Metallic", "metallicFactor"), ("Roughness", "roughnessFactor")] {
        ui.label(label);
        let mut x = number(&m["pbrMetallicRoughness"][key], 1.0);
        if ui
            .add(egui::DragValue::new(&mut x).speed(0.01).range(0.0..=1.0))
            .changed()
        {
            m["pbrMetallicRoughness"][key] = json!(x);
        }
        ui.end_row();
    }

    ui.label("Emissive");
    let e = &m["emissiveFactor"];
    let mut rgb = [
        to_srgb(number(&e[0], 0.0)),
        to_srgb(number(&e[1], 0.0)),
        to_srgb(number(&e[2], 0.0)),
    ];
    if ui.color_edit_button_rgb(&mut rgb).changed() {
        m["emissiveFactor"] = json!([to_linear(rgb[0]), to_linear(rgb[1]), to_linear(rgb[2])]);
    }
    ui.end_row();

    if m["normalTexture"].is_object() {
        ui.label("Normal strength");
        let mut x = number(&m["normalTexture"]["scale"], 1.0);
        if ui
            .add(egui::DragValue::new(&mut x).speed(0.01).range(0.0..=10.0))
            .changed()
        {
            m["normalTexture"]["scale"] = json!(x);
        }
        ui.end_row();
    }

    ui.label("Alpha");
    let mode = m["alphaMode"].as_str().unwrap_or("OPAQUE").to_string();
    let mut new_mode = mode.clone();
    egui::ComboBox::from_id_salt("ponko-materials-alpha")
        .selected_text(match mode.as_str() {
            "MASK" => "Cut out (MASK)",
            "BLEND" => "Transparent (BLEND)",
            _ => "Opaque",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut new_mode, "OPAQUE".to_string(), "Opaque");
            ui.selectable_value(&mut new_mode, "MASK".to_string(), "Cut out (MASK)");
            ui.selectable_value(&mut new_mode, "BLEND".to_string(), "Transparent (BLEND)");
        });
    if new_mode != mode {
        if new_mode == "OPAQUE" {
            remove(m, &["alphaMode"]);
        } else {
            m["alphaMode"] = json!(new_mode);
        }
    }
    ui.end_row();

    ui.label("Double-sided");
    let mut two = m["doubleSided"].as_bool().unwrap_or(false);
    if ui.checkbox(&mut two, "").changed() {
        m["doubleSided"] = json!(two);
    }
}

/// A texture slot's picker: none, one of the file's textures, or a PNG
/// from `assets/` (added to the file as an image beside it).
fn texture_slot(ui: &mut Ui, g: &mut Value, material: usize, slot: &[&str], path: &str, pngs: &[String]) {
    let now = at(&g["materials"][material], slot)["index"]
        .as_u64()
        .map(|i| i as usize);
    let count = g["textures"].as_array().map_or(0, Vec::len);
    let mut pick: Option<Option<usize>> = None;
    let mut add: Option<String> = None;
    egui::ComboBox::from_id_salt(("ponko-materials-slot", slot.join(".")))
        .selected_text(now.map_or("(none)".into(), |i| texture_name(g, i)))
        .show_ui(ui, |ui| {
            if ui.selectable_label(now.is_none(), "(none)").clicked() {
                pick = Some(None);
            }
            for i in 0..count {
                if ui.selectable_label(now == Some(i), texture_name(g, i)).clicked() {
                    pick = Some(Some(i));
                }
            }
            ui.separator();
            for png in pngs {
                if ui.selectable_label(false, png).clicked() {
                    add = Some(png.clone());
                }
            }
        });
    if let Some(png) = add {
        let uri = relative(path, &png);
        // The file's own image of that PNG, if it has one.
        let image = g["images"]
            .as_array()
            .and_then(|a| a.iter().position(|im| im["uri"] == uri.as_str()));
        let image = image.unwrap_or_else(|| {
            let name = png.rsplit('/').next().unwrap_or(&png).trim_end_matches(".png");
            push(&mut g["images"], json!({ "uri": uri, "name": name }))
        });
        let texture = g["textures"].as_array().and_then(|a| {
            a.iter()
                .position(|t| t["source"] == image && t.get("sampler").is_none())
        });
        let texture = texture.unwrap_or_else(|| push(&mut g["textures"], json!({ "source": image })));
        pick = Some(Some(texture));
    }
    match pick {
        Some(None) => remove(&mut g["materials"][material], slot),
        Some(Some(i)) => {
            let s = at_mut(&mut g["materials"][material], slot);
            if s.is_object() {
                s["index"] = json!(i);
            } else {
                *s = json!({ "index": i });
            }
        }
        None => {}
    }
}

/// Pushes `item` onto list `v` (made if missing); returns its index.
fn push(v: &mut Value, item: Value) -> usize {
    match v.as_array_mut() {
        Some(list) => {
            list.push(item);
            list.len() - 1
        }
        None => {
            *v = json!([item]);
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::author_harness::*;
    use super::*;

    /// A one-triangle glTF file with one material, "Paint", in
    /// `assets/ponko-test-materials-*/` (its buffer beside it), with a 2×2
    /// PNG and a material shader.
    struct Fixture {
        dir: std::path::PathBuf,
        gltf: String,
        shader: String,
    }

    const SHADER: &str = "fn surface(s: Surface) -> Surface {\n    var out = s;\n    out.albedo = material.custom[0].rgb;\n    return out;\n}\n";

    impl Fixture {
        fn new(what: &str) -> Self {
            let folder = format!("ponko-test-materials-{what}-{}", std::process::id());
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&folder);
            std::fs::create_dir_all(&dir).unwrap();
            // Three positions (36 bytes), as an embedded buffer.
            let positions: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();
            std::fs::write(dir.join("thing.bin"), &positions).unwrap();
            let gltf = json!({
                "asset": {"version": "2.0"},
                "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "material": 0}]}],
                "materials": [{"name": "Paint", "pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.5}}],
                "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}],
                "bufferViews": [{"buffer": 0, "byteLength": 36}],
                "buffers": [{"byteLength": 36, "uri": "thing.bin"}]
            });
            std::fs::write(dir.join("thing.gltf"), serde_json::to_string_pretty(&gltf).unwrap()).unwrap();
            let mut png = Vec::new();
            let mut encoder = png::Encoder::new(&mut png, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&[200u8; 16]).unwrap();
            std::fs::write(dir.join("grain.png"), png).unwrap();
            std::fs::write(dir.join("tint.wgsl"), SHADER).unwrap();
            Self {
                dir,
                gltf: format!("{folder}/thing.gltf"),
                shader: format!("{folder}/tint.wgsl"),
            }
        }

        fn on_disk(&self) -> Value {
            serde_json::from_str(&std::fs::read_to_string(self.dir.join("thing.gltf")).unwrap()).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn open(h: &mut Harness, item: &str, path: &str) {
        h.click_text(|t| t == "Author");
        h.click_text(|t| t == item);
        h.frame(vec![]);
        h.type_text(path);
        assert!(
            h.panels.app.as_ref().unwrap().author.picking.is_none(),
            "{:?}",
            h.panels.app.as_ref().unwrap().author.error
        );
    }

    fn material(h: &Harness, f: &Fixture) -> Value {
        h.ctx.editor.docs.get(&f.gltf).unwrap()["materials"][0].clone()
    }

    #[test]
    fn dragging_metallic_changes_the_loaded_material_at_once_and_undo_puts_it_back() {
        let f = Fixture::new("drag");
        let mut h = Harness::new();
        h.ctx.gfx.try_load_mesh(&f.gltf).unwrap();
        let paint = h.ctx.gfx.file_material_handles(&f.gltf).unwrap()[0];
        open(&mut h, "Materials…", &f.gltf);
        let field = field_right_of(&h.frame(vec![]), "Metallic");
        h.drag(field, field + glam::vec2(40.0, 0.0), 4);
        let metallic = number(&material(&h, &f)["pbrMetallicRoughness"]["metallicFactor"], 0.0);
        assert!(metallic > 0.2, "{metallic}");
        h.frame(vec![]);
        let live = h.ctx.gfx.material_desc(paint).unwrap();
        assert!(
            (live.metallic - metallic).abs() < 1e-4,
            "the model's material follows: {}",
            live.metallic
        );
        assert!((live.roughness - 0.5).abs() < 1e-4, "the rest stays");
        h.undo();
        h.frame(vec![]);
        assert_eq!(
            h.ctx.gfx.material_desc(paint).unwrap().metallic,
            0.0,
            "the drag was one step"
        );
        h.redo();
        h.save();
        let disk = f.on_disk();
        assert!((number(&disk["materials"][0]["pbrMetallicRoughness"]["metallicFactor"], 0.0) - metallic).abs() < 1e-4);
        assert!(disk["meshes"].is_array(), "the rest of the file stays");
    }

    #[test]
    fn a_png_picked_for_the_base_colour_is_added_to_the_file_and_shown_on_the_model() {
        let f = Fixture::new("texture");
        let mut h = Harness::new();
        h.ctx.gfx.try_load_mesh(&f.gltf).unwrap();
        let paint = h.ctx.gfx.file_material_handles(&f.gltf).unwrap()[0];
        assert!(h.ctx.gfx.material_desc(paint).unwrap().texture.is_none());
        open(&mut h, "Materials…", &f.gltf);
        // The texture row's "Base colour" is the second one drawn.
        let shapes = h.frame(vec![]);
        let rows: Vec<_> = texts(&shapes)
            .into_iter()
            .filter(|(t, _)| t == "Base colour")
            .map(|(_, at)| at)
            .collect();
        assert_eq!(rows.len(), 2, "the colour and the texture");
        let row = rows[1];
        let combo = texts(&shapes)
            .into_iter()
            .filter(|(t, at)| t == "(none)" && (at.y - row.y).abs() < 4.0)
            .map(|(_, at)| at)
            .next()
            .unwrap();
        h.click(combo);
        let png = f.gltf.replace("thing.gltf", "grain.png");
        h.click_last_text(|t| t == png);
        let doc = h.ctx.editor.docs.get(&f.gltf).unwrap().clone();
        assert_eq!(doc["images"][0]["uri"], "grain.png", "beside the file");
        assert_eq!(
            doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"],
            0
        );
        h.frame(vec![]);
        assert!(
            h.ctx.gfx.material_desc(paint).unwrap().texture.is_some(),
            "the model shows it"
        );
        h.undo();
        h.frame(vec![]);
        assert!(h.ctx.gfx.material_desc(paint).unwrap().texture.is_none());
    }

    #[test]
    fn a_shader_error_is_shown_at_its_line_and_fixing_it_compiles_again() {
        let f = Fixture::new("shader");
        let mut h = Harness::new();
        open(&mut h, "Shader…", &f.shader);
        h.frame(vec![]);
        assert!(h.find(|t| t == "Compiles").is_some());
        // Break line 3 by typing over the whole code.
        let code = h.rect(code_id());
        h.click(glam::vec2(code.center().x, code.top() + 10.0));
        let broken = SHADER.replace("material.custom[0].rgb;", "material.custom[0].rgb");
        h.type_text(&broken);
        let state = &h.panels.app.as_ref().unwrap().author.materials;
        let error = state.diagnostic.as_ref().expect("an error");
        assert!(error.line.is_some(), "{error:?}");
        assert!(
            h.find(|t| t.starts_with("line ")).is_some(),
            "the error is shown with its place"
        );
        // One undo step back to the code as it was.
        h.undo();
        h.frame(vec![]);
        assert!(h.find(|t| t == "Compiles").is_some());
    }

    #[test]
    fn custom_values_dragged_in_the_shader_window_reach_its_materials() {
        let f = Fixture::new("custom");
        let mut h = Harness::new();
        let shader = h.ctx.gfx.try_load_shader(&f.shader).unwrap();
        let m = h
            .ctx
            .gfx
            .create_material(crate::render::MaterialDesc::default().with_shader(Some(shader)));
        open(&mut h, "Shader…", &f.shader);
        let field = field_right_of(&h.frame(vec![]), "custom[0]");
        h.drag(field, field + glam::vec2(50.0, 0.0), 4);
        h.frame(vec![]);
        let x = h.ctx.gfx.material_desc(m).unwrap().custom[0].x;
        assert!(x > 0.2, "{x}");
        h.undo();
        h.frame(vec![]);
        assert_eq!(h.ctx.gfx.material_desc(m).unwrap().custom[0].x, 0.0);
        let snippet = rust_snippet(&f.shader, h.ctx.editor.docs.get(&f.shader).unwrap());
        assert!(snippet.contains("with_custom([vec4(0.0"), "{snippet}");
    }

    #[test]
    fn custom_values_set_in_the_materials_window_survive_saving_and_loading_the_gltf() {
        let f = Fixture::new("extras");
        let mut h = Harness::new();
        h.ctx.gfx.try_load_mesh(&f.gltf).unwrap();
        let paint = h.ctx.gfx.file_material_handles(&f.gltf).unwrap()[0];
        open(&mut h, "Materials…", &f.gltf);
        // A shader, and two values: a colour "tint" and a float.
        h.click_text(|t| t == "built-in");
        h.click_last_text(|t| t == f.shader);
        h.click_text(|t| t == "+ value");
        h.click_text(|t| t == "+ value");
        h.click_text(|t| t == "value0");
        h.type_text("tint");
        h.click_text(|t| t == "float");
        h.click_last_text(|t| t == "color");
        let field = field_right_of(&h.frame(vec![]), "custom[1]");
        h.drag(field, field + glam::vec2(50.0, 0.0), 4);
        let extras = material(&h, &f)["extras"].clone();
        assert_eq!(extras["ponko:shader"], "tint.wgsl", "relative to the file");
        assert_eq!(
            extras["ponko:params"][0],
            json!({"name": "tint", "type": "color", "value": [0.0, 0.0, 0.0, 1.0]})
        );
        let speed = number(&extras["ponko:params"][1]["value"], 0.0);
        assert!(speed > 0.2, "{extras}");
        // The model's material follows at once.
        h.frame(vec![]);
        let shader = h.ctx.gfx.loaded_shader(&f.shader).expect("the shader was loaded");
        let live = h.ctx.gfx.material_desc(paint).unwrap();
        assert_eq!(live.shader, Some(shader));
        assert!((live.custom[1].x - speed).abs() < 1e-4, "{:?}", live.custom);
        assert_eq!(live.custom[0], Vec4::new(0.0, 0.0, 0.0, 1.0));
        h.save();
        let disk = f.on_disk();
        assert_eq!(disk["materials"][0]["extras"], extras, "saved in the glTF");
        // Loaded by a game that never saw the editor: the same values.
        let mut game = Harness::new();
        game.ctx.gfx.try_load_mesh(&f.gltf).unwrap();
        let loaded = game.ctx.gfx.file_material_handles(&f.gltf).unwrap()[0];
        let desc = game.ctx.gfx.material_desc(loaded).unwrap();
        assert_eq!(desc.shader, game.ctx.gfx.loaded_shader(&f.shader));
        assert!(desc.shader.is_some());
        assert_eq!(desc.custom[0], Vec4::new(0.0, 0.0, 0.0, 1.0));
        assert!((desc.custom[1].x - speed).abs() < 1e-4);
        // And the window reads them back with their names and types.
        open(&mut game, "Materials…", &f.gltf);
        assert!(game.find(|t| t == "tint").is_some() && game.find(|t| t == "color").is_some());
    }

    #[test]
    fn relative_paths_climb_out_of_the_files_folder() {
        use super::super::author::relative;
        assert_eq!(relative("models/a.gltf", "models/t/x.png"), "t/x.png");
        assert_eq!(relative("models/a.gltf", "textures/x.png"), "../textures/x.png");
        assert_eq!(relative("a.gltf", "x.png"), "x.png");
    }
}
